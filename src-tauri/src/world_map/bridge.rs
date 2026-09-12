//! 实时绘制小区的**应用内**通路（Tauri 命令 + IPC Channel）
//!
//! ## 为什么另起一条路，而不是直接用 `stream.rs::spawn_stream`
//!
//! `spawn_stream` 是**自己用 reqwest 直连 LLM** 的：key 从 `WM_LLM_KEY` 环境变量或旧的
//! `world_map/config.local.js` 里读（`LlmConfig::from_env` / `from_legacy_config`）。
//! 那是独立调试 HTTP 服务（`world_map_rs`，127.0.0.1:8791 的 `/api/district_stream`）的用法：
//! 打包成 APK 后既没有那个服务、也没有那些环境变量，只有 LingChat 自己在设置页里配好的
//! LLM 槽位（`AppState.chat.llm`，支持运行时热切换）。
//!
//! 所以这里**复用 `stream.rs` 的全部纯逻辑**（`build_prompt` / `extract_scalar` /
//! `extract_new_objects` / `assemble_layout` / `Event::to_json`），只把「token 从哪来」
//! 换成 `LlmClient::complete_stream`，出口换成 `tauri::ipc::Channel`。
//! 事件形状与 SSE 版逐字段一致（同一个 `Event::to_json`），前端两条路能共用一套处理逻辑。
//!
//! `spawn_stream` 与它依赖的 `LlmConfig` **保留不动**：浏览器/调试服务那条路还在用。
//!
//! ## 事件流（顺序与 SSE 版一致）
//!
//! `start` →（`meta` / `size` 各一次）→ `building`/`road`/`park`/`water`（增量，一栋一条）
//! →（读流出错时一条 `warn`）→ `debug` → `done`；**任何失败都走 `error` 事件**。
//!
//! 「没配 LLM」也走 `error` 事件、命令正常返回 Ok —— 若让命令返回 Err，前端只会拿到
//! 一个字符串，分不清「没配模型」「网络断」「模型抽风」这三种情况。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::State;

use crate::ai_service::llm::{slot_snapshot, LlmChunk, LlmClient};
use crate::ai_service::types::LlmMessage;
use crate::AppState;

use super::stream::{self, Event};

// ───────────────────────── 取消登记表 ─────────────────────────

/// 正在跑的实时绘制：`stream_id` → 取消标志。
///
/// 为什么需要它：Tauri 的 Channel 是**单向**的（Rust → 前端），前端把页面关掉或点
/// 「停止」时，Rust 这边收不到任何通知 —— 不登记的话，任务会把整轮生成跑完
/// （白烧 token、白占内存，手机上还费电）。前端 stop() 时调
/// [`world_map_district_stream_cancel`] 把标志立起来，流循环下一轮就收摊。
static RUNNING: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn running() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    RUNNING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 取登记表。中毒（持锁时 panic 过）也要能用：表里只有标志位，不存在半写状态。
fn lock_running() -> MutexGuard<'static, HashMap<String, Arc<AtomicBool>>> {
    running().lock().unwrap_or_else(|e| e.into_inner())
}

/// 取消一轮绘制。返回 true 表示确实找到并叫停了在跑的任务。
fn cancel_stream(id: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    match lock_running().remove(id) {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

// ───────────────────────── 事件泵 ─────────────────────────

/// 事件泵：把 [`Event`] 序列化后推给前端。
///
/// 记 `dead` 的意义：`Channel::send` 最终是往 webview 里 eval，webview 已经销毁时
/// 它会返回 Err —— 这是**唯一**能发现「前端已经走了」的信号。发现后立刻收摊，
/// 别在后台把整轮生成跑完。
struct Pump<'a> {
    ch: &'a Channel<Value>,
    dead: bool,
}

impl<'a> Pump<'a> {
    fn send(&mut self, ev: Event) -> bool {
        if self.dead {
            return false;
        }
        match self.ch.send(ev.to_json()) {
            Ok(()) => true,
            Err(e) => {
                self.dead = true;
                tracing::warn!("实时绘制：事件通道已不可达，提前收尾（{e}）");
                false
            }
        }
    }
}

/// 从缓冲区里抠出**新闭合**的元素并逐条推送（增量：推过的不再推）。
///
/// 返回本次推出去的条数；`pump.dead` 为 true 时调用方应立即收摊。
fn drain_items(
    pump: &mut Pump<'_>,
    buf: &str,
    consumed: &mut HashMap<String, usize>,
    counts: &mut HashMap<String, usize>,
    t0: Instant,
) -> usize {
    let mut pushed = 0usize;
    // 顺序与 SSE 版一致：建筑 → 道路 → 绿地 → 水面
    for (key, kind) in [
        ("buildings", "building"),
        ("roads", "road"),
        ("parks", "park"),
        ("water", "water"),
    ] {
        let from = *consumed.get(key).unwrap_or(&0);
        let (objs, next) = stream::extract_new_objects(buf, key, from);
        consumed.insert(key.to_string(), next);
        for o in objs {
            let c = counts.entry(key.to_string()).or_insert(0);
            *c += 1;
            pushed += 1;
            let ok = pump.send(Event::Item {
                kind: kind.to_string(),
                item: o,
                index: *c,
                elapsed: t0.elapsed().as_secs_f64(),
            });
            if !ok {
                return pushed;
            }
        }
    }
    pushed
}

// ───────────────────────── 干活的后台任务 ─────────────────────────

/// 把 LLM 的增量文本变成一条条绘制事件。
///
/// 不返回 Err：错误一律变成 `error` 事件（前端要的是可读原因，不是一个字符串）。
async fn run_stream(
    ch: Channel<Value>,
    cancel: Arc<AtomicBool>,
    llm: Arc<LlmClient>,
    messages: Vec<LlmMessage>,
    area: String,
    base: i32,
    model: String,
) {
    let t0 = Instant::now();
    let mut pump = Pump {
        ch: &ch,
        dead: false,
    };

    // 先报 start：前端据此把阶段条推到「AI 思考中」，并校对网格大小
    if !pump.send(Event::Start {
        area: area.clone(),
        size: base,
        model,
    }) {
        return;
    }

    let mut chunk_stream = match llm.complete_stream(&messages).await {
        Ok(s) => s,
        Err(e) => {
            pump.send(Event::Error {
                message: format!("LLM 请求失败：{e}"),
            });
            return;
        }
    };

    let mut buf = String::new();
    let mut consumed: HashMap<String, usize> = HashMap::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for k in ["buildings", "roads", "parks", "water"] {
        consumed.insert(k.to_string(), 0);
        counts.insert(k.to_string(), 0);
    }
    let mut name_sent = false;
    // (分片数, 正文字符数, 元素条数)。SSE 版第三项是 data 行数，这里没有「行」的概念，
    // 用「已产出的元素条数」顶上；前端只是把这组数字打印进绘制日志。
    let mut dbg = (0usize, 0usize, 0usize);
    let mut aborted = false;
    let mut read_error: Option<String> = None;

    loop {
        // 取消 / 前端没了 → 收摊（下一轮循环的检查点，配合下面的 120ms 兜底）
        if cancel.load(Ordering::SeqCst) || pump.dead {
            aborted = true;
            break;
        }
        // `select!` 是为了让「取消」在**流正卡着**的时候也能生效（否则要等下一个分片）。
        // 120ms 足够灵敏，又不至于把流拆得太碎；`StreamExt::next` 是取消安全的
        // （丢掉重建不丢数据，`complete_stream` 内部本来就用 timeout 包着 next）。
        let item = tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(120)) => continue,
            item = chunk_stream.next() => item,
        };
        let Some(item) = item else { break };
        match item {
            Ok(LlmChunk::Content(piece)) => {
                if piece.is_empty() {
                    continue;
                }
                dbg.0 += 1;
                dbg.1 += piece.chars().count();
                buf.push_str(&piece);

                // 小区名 / 规模：最先出现，各报一次（与 SSE 版同序、同去重方式）
                if !name_sent {
                    if let Some(Value::String(n)) = stream::extract_scalar(&buf, "name") {
                        name_sent = true;
                        if !pump.send(Event::Meta { name: n }) {
                            aborted = true;
                            break;
                        }
                    }
                    if let Some(sz) = stream::extract_scalar(&buf, "size").and_then(|v| v.as_i64()) {
                        if !pump.send(Event::Size { size: sz as i32 }) {
                            aborted = true;
                            break;
                        }
                    }
                }

                dbg.2 += drain_items(&mut pump, &buf, &mut consumed, &mut counts, t0);
                if pump.dead {
                    aborted = true;
                    break;
                }
            }
            Ok(LlmChunk::Reasoning(_)) => {
                // 思考链不进画布，只计数（有的模型会在 JSON 前吐一大段思考）
                dbg.0 += 1;
            }
            Ok(_) => {
                // ToolCalls / ToolCallProgress / StreamEnd：实时绘制用不到
            }
            Err(e) => {
                // 与 SSE 版一致：读流异常只丢一条 warn，已经画出来的继续用
                read_error = Some(format!("读流异常：{e}"));
                break;
            }
        }
    }

    if let Some(msg) = read_error {
        let _ = pump.send(Event::Warn { message: msg });
    }
    let _ = pump.send(Event::Debug {
        chunks: dbg.0,
        chars: dbg.1,
        lines: dbg.2,
    });
    if aborted {
        // 用户停止 / 页面走了：**不发 done** —— 前端已按「中断」收尾，
        // 再补一条 done 等于把半截布局说成成品
        return;
    }
    let layout = stream::assemble_layout(&buf, &area, base);

    // ── 世界模拟：布局落地 = 这个小区「成为现实」的那一刻 ──
    // 顺手把设施铺出来写进 MapRuntime（为什么不交给前端：见
    // `state::install_facilities` 的文档注释 —— 前端只有 SVG，算不出设施表）。
    // 种子用区域名的哈希而**不是**随机数：同一个小区的店名与位置必须可复现，
    // 否则每次重进都换一批店，AI 上一轮说的「楼下便利店」就找不到了。
    let facs = super::facilities::generate_all(
        &layout,
        &area,
        None,
        "district",
        Some(super::bookmark::area_seed(&area)),
    );
    let cell_m = facs
        .get("facilities")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|f| f.get("cell_meters"))
        .and_then(Value::as_f64)
        .filter(|c| *c > 0.0)
        .unwrap_or(super::facilities::DEFAULT_CELL_METERS);
    if let Some(list) = facs.get("facilities") {
        super::state::install_facilities(&area, list, cell_m);
    }

    let _ = pump.send(Event::Done {
        layout,
        elapsed: t0.elapsed().as_secs_f64(),
    });
}

// 区域名 → 稳定种子（`super::bookmark::area_seed`，FNV-1a 64 位取正）。
//
// 它原来私有在本文件里；P3-1 要把它写进「地图存档」（读档后同一个小区里还得是
// 那批店），于是搬进 `bookmark.rs` 作为**唯一实现**，这里改成调用它 ——
// 存档里记的种子与生成设施表用的种子必须是同一个函数，两份实现迟早走偏。

// ───────────────────────── Tauri 命令 ─────────────────────────

/// 实时绘制小区：流式生成并把每个元素推给前端的 `onEvent` Channel。
///
/// 参数名在 JS 侧要写 **camelCase**（`streamId` / `onEvent`）：Tauri 2 的命令宏默认
/// 把 Rust 的 snake_case 参数名转成 camelCase 再取值（见 tauri-macros `ArgumentCase::Camel`）。
///
/// 命令本身**立刻返回** Ok，真正的生成在后台任务里跑（与 `editor_agent_start_chat`
/// 同一个套路）：一轮生成要几十秒到两分钟，阻塞在命令里没有任何好处。
/// 因此「生成结束」只以 `done` / `error` 事件为准，不是 invoke 的 resolve。
///
/// `stream_id` 可选，但前端一定要给：停止时靠它找回对应的任务（见
/// [`world_map_district_stream_cancel`]）。
#[tauri::command]
pub async fn world_map_district_stream(
    state: State<'_, AppState>,
    area: String,
    context: Option<String>,
    expand: Option<i32>,
    stream_id: Option<String>,
    on_event: Channel<Value>,
) -> Result<(), String> {
    // 参数兜底：前端可能给空串 / 负数，别把坏参数变成事件流里的一场空
    let area = {
        let a = area.trim();
        if a.is_empty() {
            "广州市·越秀区".to_string()
        } else {
            a.to_string()
        }
    };
    let context = context.unwrap_or_default();
    // 网格大小算法沿用 `stream.rs`：base = 20 + expand*8（0/1/2 → 20/28/36）
    let base = 20 + expand.unwrap_or(0).clamp(0, 8) * 8;
    let sid = stream_id.unwrap_or_default().trim().to_string();

    // 同一个 id 又开了一轮（前端重画）：先把上一轮掐掉，免得两条流抢同一张画布
    if !sid.is_empty() {
        let _ = cancel_stream(&sid);
    }

    // 从 LLM 槽位取当前客户端（支持设置页热切换）。取不到 = 还没配好模型。
    let llm = match slot_snapshot(&state.chat.llm).await {
        Some(c) => c,
        None => {
            // 「没配 LLM」不是命令失败，而是一种可读的业务结果：发一条 error 事件，
            // 命令正常返回（前端会把它显示成「生成失败：未配置 LLM…」）
            let _ = on_event.send(json!({
                "type": "error",
                "message": "未配置 LLM：请先在「设置 → LLM」里选好模型并填 API Key，再回来重试",
            }));
            return Ok(());
        }
    };

    let model = llm.config().model.clone();
    // OSM 提示与 SSE 版一样留空（HTTP 调试服务那边同样传 None）
    let (sys_p, usr) = stream::build_prompt(&area, &context, base, None);
    let messages = vec![LlmMessage::system(sys_p), LlmMessage::user(usr)];

    let cancel = Arc::new(AtomicBool::new(false));
    if !sid.is_empty() {
        lock_running().insert(sid.clone(), cancel.clone());
    }

    let sid_done = sid.clone();
    let cancel_done = cancel.clone();
    tokio::spawn(async move {
        run_stream(on_event, cancel, llm, messages, area, base, model).await;
        // 自然收尾时把登记撤掉。只撤「还是自己那一份」的登记（Arc::ptr_eq）：
        // 否则可能误删重开后新一轮的那条，让第二次「停止」失效。
        if !sid_done.is_empty() {
            let mut map = lock_running();
            let mine = map
                .get(&sid_done)
                .map(|f| Arc::ptr_eq(f, &cancel_done))
                .unwrap_or(false);
            if mine {
                map.remove(&sid_done);
            }
        }
    });

    Ok(())
}

/// 停止一轮实时绘制（前端 stop() / 离开页面时调用）。
///
/// 找不到 id 也返回 Ok：说明这一轮已经自己收尾了 —— 那是正常情况，不该报错。
#[tauri::command]
pub fn world_map_district_stream_cancel(stream_id: String) -> Result<(), String> {
    let _ = cancel_stream(stream_id.trim());
    Ok(())
}
