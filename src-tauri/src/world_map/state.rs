//! 世界模拟 · **地图运行时状态**（P3 的 Rust 半：状态 → 工具 → 注入）
//!
//! ## 这个模块解决什么问题
//! 对话管线（`role_manager.rs::sync_memories`）在 **`game_status` 锁内**跑，
//! 那里绝对不能再 `await` 网络；而「角色在哪、附近有什么、天气如何」这些信息
//! 全在前端手里（地图页是前端渲染的）。所以中间需要一个**进程内共享状态**：
//!
//! ```text
//!   前端地图页 ──world_map_update_runtime(patch)──▶ MapRuntime（本模块，Arc<RwLock<_>>）
//!                                                      │  同步读，不 await
//!                            AI 工具（get_my_location / get_nearby_facilities / move_to）
//!                                                      │
//!                                    对话注入（role_manager.rs → summary::render）
//! ```
//!
//! ## 为什么不挂进 `AppState`
//! `AppState` 的 22 个字段全是「对话/存档/LLM」那条主干的，且是**两级壳**
//! （`OnceLock<InnerAppState>`，填一次就不再变）。地图是旁路模块，硬塞进去会
//! 让 `fill()` 的实参表再长一截，也会让地图模块从"可整块删除"变成"牵着主干"。
//! 所以照架构文档 §2 的做法**另开一个 `app.manage` 的状态壳**
//! （[`MapRuntimeHandle`]，与 `pet::HitTestState` / `cast::CastManager` 同档）。
//!
//! ## 为什么同时又有一个进程级 `static`
//! `GameRoleManager` **拿不到 `AppHandle`**（它只有 `data_dir` / `llm` / 台词表，
//! 见 `role_manager.rs:20-42`），而注入点恰好在它内部。若为了拿状态把
//! `AppHandle` 一路透传下去，要改 `GameStatus` → `AIService` → 每一条命令的签名，
//! 那是"改坏既有功能"的高风险区。
//!
//! 妥协办法：**同一个 `Arc` 既 `app.manage` 又放在进程级 `static` 里**
//! （[`shared`]），两条路读到的是**同一份数据**。
//! 命令侧走 `State<MapRuntimeHandle>`（正规的 Tauri 状态），注入侧走 `static`
//! （拿不到 AppHandle 的唯一出路）。这是本模块唯一一个"看着多余"的设计，
//! 原因就是上面这条借用链，不是随手写的。
//!
//! ## 前端契约（`src/api/services/worldMap.ts` 那一侧要照着推）
//!
//! | 命令 | 参数 | 说明 |
//! |---|---|---|
//! | `world_map_update_runtime` | `{ patch }` | 只推**变化的那一块**，字段级合并 |
//! | `world_map_runtime` | `{ role? }` | 读回全量状态（附 `injection.text` 预览） |
//!
//! `patch` 的键：`scene` / `me` / `actors` / `events` / `weather` / `facilities` /
//! `cell_m` / `current_role` / `pending_move`（逐条语义见 [`MapRuntime::apply_patch`]）。
//! 三条要点：
//! 1. `actors` 的**键必须是角色的显示名**（`GameRole.display_name` =
//!    `settings.yml` 的 `ai_name`）—— 注入侧就是按它查"这个角色在哪"的。
//! 2. **退出世界模拟时推 `{scene: null}`**：注入的开关就是"有没有 scene"，
//!    不清掉的话对话会一直带着上一次的地图上下文（这是前端唯一必须记得做的事）。
//! 3. `weather` 推 `world_map_weather` 的结果对象即可（存进来时自动打时间戳，
//!    注入侧按 [`summary::WEATHER_TTL_SECS`] 判过期；**不要在注入路径抓天气**）。
//!
//! ## 注入路径上的两条铁律（都会在代码里再注释一遍）
//! 1. **[`injection_for`] 是同步函数，内部一次 `await` 都没有**：用
//!    `try_write()` 拿锁（要顺带写回"上次注入的文本 + 指纹"缓存），
//!    拿不到（正好有前端在推 patch）就退回去用上一次的文本，绝不等锁。
//! 2. **天气只用已抓好的快照**（[`MapRuntime::weather`] 带抓取时间戳），
//!    过期（> [`summary::WEATHER_TTL_SECS`]）就当没有 —— 注入路径**不抓天气**。

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use chrono::{DateTime, Local, Timelike};
use serde_json::{json, Value};
use tauri::State;
use tokio::sync::RwLock;

use super::summary::{self, SummaryInput, WEATHER_TTL_SECS};

/// `Value::Null` 的常量引用（给 `Option<Value>` 缺席时当占位用，零成本）
const NULL: Value = Value::Null;

/// 最近事件环形缓冲上限。20 条足够「最近一条」注入 + 前端事件流回溯，
/// 再多就是白占内存（事件本身另有 `world_map_push_events` 落盘那条持久化通路）。
pub const EVENTS_MAX: usize = 20;

/// 待写记忆队列的上限（P5-3）。超了**丢最旧的**：这个队列只是"事件引擎到记忆管线"
/// 的中转站，正常情况下前端每隔几秒就会 `world_map_take_pending_memory` 取走；
/// 攒满说明前端那条通路没接上，宁可丢最旧的几行，也不让它无限长。
pub const PENDING_MEMORY_MAX: usize = 64;

/// 一行**待写记忆**（事件引擎抽中的事件 → 等前端取走 → 交给记忆管线）。
///
/// 形状与 [`MapRuntime::take_memory`] 的返回一致：`role` 是谁的经历，
/// `line` 是 `events::memory_line()` 拼好的 `旁白: …`（≤40 字，与对话记忆同形），
/// `at` 是事件发生时刻（unix 秒，同时刻的墙钟口径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMemory {
    pub role: String,
    pub line: String,
    pub at: i64,
}

impl PendingMemory {
    /// 给命令层回包用的一行（`{role, line, at}`）。
    pub fn to_json(&self) -> Value {
        json!({ "role": self.role, "line": self.line, "at": self.at })
    }
}

/// 注入缓存表的上限：超过就整体清空。
/// 缓存只是"省一次拼字符串"，清空的代价可以忽略；设上限是为了防止
/// 角色名被反复改名时表无限膨胀（每个角色名一条）。
const INJECTION_CACHE_MAX: usize = 64;

/// 注入文本 + 它的指纹（「变化时才更新」的落点，机主定的 H38）。
#[derive(Clone, Debug)]
pub struct InjectionCache {
    /// [`summary::fingerprint`] 的结果：文本没变 → 指纹没变 → 直接复用文本
    pub fp: u64,
    pub text: String,
}

/// 地图运行时状态（**进程内共享**，见模块注释）。
///
/// 五个字段是任务定稿的骨架；后面几个是 P3 落地时补的（纯新增字段，
/// 不影响 `world_map_update_runtime` 的字段级合并语义，旧前端不推它们就是空值）：
/// `facilities` / `cell_m` 给「附近有什么」用（光有角色坐标算不出距离），
/// `current_role` 让工具知道**是谁**在问，`pending_move` 是 `move_to` 的意图出口。
#[derive(Debug)]
pub struct MapRuntime {
    /// 当前场景：adcode 路径 + 名称（国·省·市·区县·小区）
    pub scene: Option<Value>,
    /// 玩家位置（lat/lng + 精度来源）
    pub me: Option<Value>,
    /// 角色在地图上的位置（name -> {facility, x, y, since}）
    pub actors: Value,
    /// 最近事件（环形缓冲，最多 [`EVENTS_MAX`] 条）
    pub events: Vec<Value>,
    /// 天气快照（含抓取时间，用于判断是否过期）
    pub weather: Option<(u64, Value)>,

    // ── 以下是 P3 的附加字段 ──
    /// 当前小区/片区的设施表（`facilities.rs` 生成的那一份，前端推上来）
    pub facilities: Value,
    /// 小区格网一格多少米（`facilities::DEFAULT_CELL_METERS` = 30）
    pub cell_m: f64,
    /// 正在说话的角色名（`actors` 的键）；工具 `get_my_location` 据此知道"我"是谁
    pub current_role: Option<String>,
    /// `move_to` 工具记录的移动意图，前端取走后用 `null` 清掉
    pub pending_move: Option<Value>,

    // ── 以下是 P5 的附加字段（**纯追加**，不影响上面任何字段的语义）──
    /// **待写记忆队列**（P5-3）：事件引擎抽中的事件在这里留一行 `旁白: …`，
    /// 等前端用 `world_map_take_pending_memory` **取走**（drain）再交给记忆管线。
    ///
    /// 为什么放在 `MapRuntime` 里而不是模块级 `static`：
    /// ① 与其它字段同一条生命周期（退出世界模拟、`injection` 缓存清理都在一起）；
    /// ② [`MapRuntime::apply_patch`] 是**字段级合并**，前端推局部 patch 不会把它抹掉；
    /// ③ 注入路径（`static RUNTIME`）与命令路径（`app.manage` 的壳）读的是同一个
    ///    `Arc`，不存在"队列写在一份、读在另一份"的第二真源问题。
    pub pending_memory: Vec<PendingMemory>,
    /// 最后一次 patch 落地的时刻（unix 秒），调试用
    pub updated_at: u64,
    /// 每个角色上一次注入的文本 + 指纹（H38：变化时才更新）
    injection: HashMap<String, InjectionCache>,
}

impl Default for MapRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl MapRuntime {
    pub fn new() -> Self {
        Self {
            scene: None,
            me: None,
            actors: json!({}),
            events: Vec::new(),
            weather: None,
            facilities: json!([]),
            cell_m: super::facilities::DEFAULT_CELL_METERS,
            current_role: None,
            pending_move: None,
            pending_memory: Vec::new(),
            updated_at: 0,
            injection: HashMap::new(),
        }
    }

    /// 当前时刻的 unix 秒（负数按 0 处理 —— 设备时间异常时别让"过期"判断反过来）。
    fn now_secs() -> u64 {
        Local::now().timestamp().max(0) as u64
    }

    /// 快照（前端读、注入读、调试读都是它）。
    pub fn to_json(&self, now: u64) -> Value {
        json!({
            "scene": self.scene.clone().unwrap_or(Value::Null),
            "me": self.me.clone().unwrap_or(Value::Null),
            "actors": self.actors.clone(),
            "events": self.events.clone(),
            "weather": match &self.weather {
                Some((at, data)) => json!({
                    "at": at,
                    "age_secs": now.saturating_sub(*at),
                    "stale": now.saturating_sub(*at) > WEATHER_TTL_SECS,
                    "data": data,
                }),
                None => Value::Null,
            },
            "facilities": self.facilities.clone(),
            "cell_m": self.cell_m,
            "current_role": self
                .current_role
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
            "pending_move": self.pending_move.clone().unwrap_or(Value::Null),
            "updated_at": self.updated_at,
        })
    }

    /// 不过期的天气快照（注入与工具共用同一把尺子）。
    pub fn fresh_weather(&self, now: u64) -> Option<&Value> {
        self.weather
            .as_ref()
            .filter(|(at, _)| now.saturating_sub(*at) <= WEATHER_TTL_SECS)
            .map(|(_, data)| data)
    }

    /// 拼这次的注入文本（带指纹缓存）。
    ///
    /// **同步函数**：注入点握着 `game_status` 锁，这里一旦 `await` 就可能被
    /// 排到别的任务后面（甚至和写锁互等）。所有输入都来自本结构体 + 调用方传进来的
    /// `now`，没有任何 IO。
    pub fn injection_text(&mut self, role: &str, now: &DateTime<Local>) -> String {
        let secs = now.timestamp().max(0) as u64;
        let text = self.render(role, now, secs);

        // 空文本不缓存也不注入（世界模拟没开时每一条台词都会走到这里）
        if text.is_empty() {
            self.injection.remove(role);
            return text;
        }

        let fp = summary::fingerprint(&text);
        if let Some(cached) = self.injection.get(role) {
            if cached.fp == fp {
                // 位置/天气/时间都没变：直接复用上一次的文本（H38）。
                // 顺带的好处是这段 system 文本在连续几轮里逐字相同，
                // 供应商侧的 prompt 前缀缓存更容易命中。
                return cached.text.clone();
            }
        }
        if self.injection.len() >= INJECTION_CACHE_MAX {
            self.injection.clear();
        }
        self.injection.insert(
            role.to_string(),
            InjectionCache {
                fp,
                text: text.clone(),
            },
        );
        text
    }

    /// 只读预览（不写缓存）：给 `world_map_runtime` 返回「AI 现在能看到什么」。
    pub fn preview(&self, role: &str) -> String {
        let now = Local::now();
        let secs = now.timestamp().max(0) as u64;
        self.render(role, &now, secs)
    }

    /// 真正的拼装：把借用凑齐交给 [`summary::render`]（纯函数，可单测）。
    fn render(&self, role: &str, now: &DateTime<Local>, secs: u64) -> String {
        summary::render(&SummaryInput {
            scene: self.scene.as_ref().unwrap_or(&NULL),
            me: self.me.as_ref().unwrap_or(&NULL),
            actors: &self.actors,
            events: &self.events,
            facilities: &self.facilities,
            cell_m: self.cell_m,
            weather: self.fresh_weather(secs),
            role,
            hhmm: &now.format("%H:%M").to_string(),
            // `Timelike::hour()` 保证 0..=23，直接给 u32
            hour: now.hour(),
        })
    }

    /// **字段级合并**一个 patch，返回真的改动了的字段名列表。
    ///
    /// 语义（前端契约，逐条都在下面代码里有对应分支）：
    /// | patch 键 | 行为 |
    /// |---|---|
    /// | `scene` / `me` | object → 逐字段合并；`null` → 清空 |
    /// | `actors` | 按角色名逐字段合并；某角色给 `null` → 删掉该角色 |
    /// | `events` | 数组 → **追加**（环形裁剪到 [`EVENTS_MAX`]）；`clear_events: true` 先清空 |
    /// | `weather` | object → 存快照并**打上此刻的时间戳**；`null` → 清空 |
    /// | `facilities` | 数组 → 整表替换（设施表随小区切换整批换，合并没有意义）；`null` → 清空 |
    /// | `cell_m` | 数字 → 覆盖 |
    /// | `current_role` | 字符串 → 覆盖；`null` → 清空 |
    /// | `pending_move` | object → 覆盖；`null` → 清空（前端取走意图后调） |
    pub fn apply_patch(&mut self, patch: &Value) -> Vec<String> {
        let mut changed: Vec<String> = Vec::new();
        let Some(obj) = patch.as_object() else {
            return changed;
        };
        let now = Self::now_secs();

        // scene / me：字段级合并
        for (key, slot) in [("scene", 0usize), ("me", 1usize)] {
            let Some(v) = obj.get(key) else { continue };
            let target = if slot == 0 {
                &mut self.scene
            } else {
                &mut self.me
            };
            if v.is_null() {
                if target.take().is_some() {
                    changed.push(key.to_string());
                }
                continue;
            }
            if !v.is_object() {
                continue;
            }
            let slot_value = target.get_or_insert_with(|| json!({}));
            if summary::merge_object_slot(slot_value, v) {
                changed.push(key.to_string());
            }
        }

        if let Some(v) = obj.get("actors") {
            if v.is_null() {
                if self.actors.as_object().is_some_and(|m| !m.is_empty()) {
                    self.actors = json!({});
                    changed.push("actors".into());
                }
            } else if summary::merge_actors(&mut self.actors, v) {
                changed.push("actors".into());
            }
        }

        if obj.get("clear_events").and_then(Value::as_bool) == Some(true) && !self.events.is_empty() {
            self.events.clear();
            changed.push("events".into());
        }
        if let Some(v) = obj.get("events") {
            if let Some(list) = v.as_array() {
                if summary::push_events(&mut self.events, list, EVENTS_MAX) {
                    if !changed.iter().any(|c| c == "events") {
                        changed.push("events".into());
                    }
                }
            }
        }

        if let Some(v) = obj.get("weather") {
            if v.is_null() {
                if self.weather.take().is_some() {
                    changed.push("weather".into());
                }
            } else if v.is_object() {
                self.weather = Some((now, v.clone()));
                changed.push("weather".into());
            }
        }

        if let Some(v) = obj.get("facilities") {
            if v.is_null() {
                if self.facilities.as_array().is_some_and(|a| !a.is_empty()) {
                    self.facilities = json!([]);
                    changed.push("facilities".into());
                }
            } else if v.is_array() && self.facilities != *v {
                self.facilities = v.clone();
                changed.push("facilities".into());
            }
        }

        if let Some(v) = obj.get("cell_m").and_then(Value::as_f64) {
            if v > 0.0 && (v - self.cell_m).abs() > f64::EPSILON {
                self.cell_m = v;
                changed.push("cell_m".into());
            }
        }

        if let Some(v) = obj.get("current_role") {
            // null = 清空；字符串 = 覆盖；其它类型（脏数据）保持原值不动，
            // 别让一个手滑的 patch 把"当前是谁在说话"抹掉。
            let next = if v.is_null() {
                None
            } else {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| self.current_role.clone())
            };
            if next != self.current_role {
                self.current_role = next;
                changed.push("current_role".into());
            }
        }

        if let Some(v) = obj.get("pending_move") {
            if v.is_null() {
                if self.pending_move.take().is_some() {
                    changed.push("pending_move".into());
                }
            } else if v.is_object() {
                self.pending_move = Some(v.clone());
                changed.push("pending_move".into());
            }
        }

        if !changed.is_empty() {
            self.updated_at = now;
        }
        changed
    }

    /// 追加一条事件（工具 `move_to` 记录意图时用；前端也可以只推事件不推别的）。
    pub fn push_event(&mut self, event: Value) -> bool {
        let added = summary::push_events(&mut self.events, std::slice::from_ref(&event), EVENTS_MAX);
        if added {
            self.updated_at = Self::now_secs();
        }
        added
    }

    /// 往待写记忆队列里压一行（P5-3 的写入口，薄封装）。
    ///
    /// 空行（全空白）直接丢弃并返回 `false` —— 事件引擎正常不会产出空行，
    /// 但这一层是"给记忆管线的输入"，不值得让一条空记忆流进去。
    /// **不动 `updated_at`**：那个字段的语义是"前端最后一次推 patch 的时刻"，
    /// 事件引擎在服务端自己攒的行不该冒充前端的动作。
    pub fn push_memory(&mut self, role: impl Into<String>, line: impl Into<String>, at: i64) -> bool {
        let line = line.into();
        if line.trim().is_empty() {
            return false;
        }
        self.pending_memory.push(PendingMemory {
            role: role.into(),
            line,
            at,
        });
        if self.pending_memory.len() > PENDING_MEMORY_MAX {
            let drop_n = self.pending_memory.len() - PENDING_MEMORY_MAX;
            self.pending_memory.drain(0..drop_n);
        }
        true
    }

    /// **取走**待写记忆（drain 语义：返回的同时从队列里删掉，同一个调用方拿两次
    /// 不会重复）。`role` 为 `None`（或空串）时取走全部。
    ///
    /// 角色名匹配与 `summary::actor_record` 同一套宽容度：去首尾空白 + 忽略
    /// ASCII 大小写（前端传的名字多一个空格不该让记忆留在队列里烂掉）。
    pub fn take_memory(&mut self, role: Option<&str>) -> Vec<PendingMemory> {
        let want = role.map(str::trim).filter(|s| !s.is_empty());
        let mut taken = Vec::new();
        let mut keep = Vec::new();
        for item in std::mem::take(&mut self.pending_memory) {
            match want {
                Some(w) if !item.role.trim().eq_ignore_ascii_case(w) => keep.push(item),
                _ => taken.push(item),
            }
        }
        self.pending_memory = keep;
        taken
    }

    /// 队列里还有几行（调试/测试用；`world_map_runtime` 的快照里不带它）。
    pub fn pending_memory_len(&self) -> usize {
        self.pending_memory.len()
    }
}

/// `app.manage` 的状态壳。
///
/// 新开一个 newtype 而不是直接 manage `Arc<RwLock<MapRuntime>>`：Tauri 的
/// `State<T>` 按**类型**取，直接用通用容器类型容易与将来别的 `Arc<RwLock<_>>`
/// 撞车（`AppState` 那套两级壳也是同一个理由）。
#[derive(Clone)]
pub struct MapRuntimeHandle(pub Arc<RwLock<MapRuntime>>);

impl Default for MapRuntimeHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl MapRuntimeHandle {
    /// 绑定进程级那份单例（与 [`shared`] 是同一个 `Arc`）。
    pub fn new() -> Self {
        Self(shared())
    }
}

/// 进程级单例（注入路径的唯一入口 —— 那条路拿不到 `AppHandle`，见模块注释）。
static RUNTIME: LazyLock<Arc<RwLock<MapRuntime>>> =
    LazyLock::new(|| Arc::new(RwLock::new(MapRuntime::new())));

/// 上一次成功算出的注入文本（按角色名）。
///
/// 用途只有一个：`try_read()` 撞上写锁时（前端正好在推 patch），
/// 注入不能让整条对话管线等锁，也不能塞空文本，就退回上一次这份。
/// 是一把 **std Mutex**，持锁时间只有一次 HashMap 查找。
static LAST_INJECTION: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 取进程级单例（`app.manage` 用的也是它）。
pub fn shared() -> Arc<RwLock<MapRuntime>> {
    RUNTIME.clone()
}

/// 给 `lib.rs` 用的状态壳。
pub fn handle() -> MapRuntimeHandle {
    MapRuntimeHandle::new()
}

/// **注入路径的入口**：同步、不 `await`、不抓网络。
///
/// 调用点只有一个：`role_manager.rs::sync_memories`（在 `game_status` 锁内）。
/// 拿不到锁就返回上一次的文本 —— 「宁可沿用旧的一行，也不空着、更不阻塞」。
///
/// 这里用 `try_write()` 而不是 `try_read()`：注入要顺带把「这次拼出来的文本 + 指纹」
/// 写回缓存（H38 的变化检测），只读锁拿不到 `&mut MapRuntime`。
/// `try_*` 都不等待，所以写锁被前端占着时不会卡住对话管线，只是这一次不更新缓存。
pub fn injection_for(role_name: &str) -> String {
    let text = match RUNTIME.try_write() {
        Ok(mut guard) => guard.injection_text(role_name, &Local::now()),
        Err(_) => LAST_INJECTION
            .lock()
            .ok()
            .and_then(|m| m.get(role_name).cloned())
            .unwrap_or_default(),
    };
    if !text.is_empty() {
        if let Ok(mut m) = LAST_INJECTION.lock() {
            m.insert(role_name.to_string(), text.clone());
        }
    }
    text
}

/// 让前端/调试看「AI 现在能看到什么」。
pub fn injection_preview(role: &str) -> String {
    match RUNTIME.try_read() {
        Ok(guard) => guard.preview(role),
        Err(_) => LAST_INJECTION
            .lock()
            .ok()
            .and_then(|m| m.get(role).cloned())
            .unwrap_or_default(),
    }
}

/// 「世界模拟开着吗」—— 唯一判据是**有没有 `scene`**（模块注释第 2 条：
/// 前端退出世界模拟时推 `{scene: null}`，注入的开关就是它）。
///
/// 目前只有一个调用点：`ai_service/message_system/producer.rs` 在流式切句前决定
/// 要不要启用位置指令剥离器（`world_map::directive::Scanner`）。
/// **没开世界模拟时必须返回 `false`**，那条路的字节与改造前完全一致 —— 这是给官方
/// 提 PR 的红线（见 `directive.rs` 模块注释与 `~/chk/p4/producer_diff.rs` 的差分测试）。
///
/// 拿不到读锁（前端正好在推 patch）也算"没开"：宁可漏掉一轮指令，
/// 也不去改变官方那条管线上的行为（`try_read` 不等待，见模块注释的铁律 1）。
pub fn world_sim_enabled() -> bool {
    RUNTIME
        .try_read()
        .map(|guard| guard.scene.is_some())
        .unwrap_or(false)
}

/// 记录一次 `move_to` 意图（工具调用；不在这里碰前端，动画由前端做）。
///
/// 返回 `false` 表示目标为空（调用方据此回 `ToolError::InvalidArguments`）。
pub fn record_move_intent(target: &str, by: Option<&str>, kind: Option<&str>) -> bool {
    let target = target.trim();
    if target.is_empty() {
        return false;
    }
    let intent = json!({
        "target": target,
        "by": by,
        "kind": kind,
        "at": MapRuntime::now_secs(),
        // 只记意图，不做动画、不改 actors —— 真正落位由前端（会走路径/动画）完成后
        // 用 world_map_update_runtime 推 actors 回来，避免"后端说到了、画面还在路上"。
        "status": "pending",
    });
    // 事件文本先在这里拼成**自有**数据：下面那条兜底分支要 move 进 'static 任务，
    // 借用 `target`/`by` 是编不过的。
    let event = json!({
        "kind": "move",
        "text": format!("{}动身去{target}", by.unwrap_or("角色")),
    });
    if let Ok(mut guard) = RUNTIME.try_write() {
        guard.pending_move = Some(intent);
        guard.push_event(event);
        true
    } else {
        // 写锁被占（前端正在推状态）：意图不能丢，等一小会儿再写。
        // 这里在**工具**里（`tokio` 任务内，2 秒超时预算），可以 await；
        // 注入路径不会走到这个分支。
        tauri::async_runtime::spawn(async move {
            let mut guard = RUNTIME.write().await;
            guard.pending_move = Some(intent);
            guard.push_event(event);
        });
        true
    }
}

/// 小区布局落地时，**由后端自己**把设施表写进 runtime（`bridge.rs` 流式生成完成时调）。
///
/// ## 为什么必须由后端做（这是本轮补的一个静默缺口）
/// 设施是 `facilities.rs` 从 layout **算出来**的（摆放规则 + 随机种子），前端手里
/// 根本没有这份数据 —— 它只有 SVG。而在这次接线之前，`facilities::generate_all()`
/// 在 Tauri 侧**零调用者**，导致 `runtime.facilities` 永远是空数组，于是：
///   · 注入里「附近：咖啡馆(70m)、地铁站(190m)…」那一行**永远缺失**；
///   · `move_to` 工具与 `⟦wm:…⟧` 指令派发解析不出目的地，只打一条 `warn` 就不起程
///     （`move::resolve_destination` 读的就是 `snapshot["facilities"]`）。
/// 两个症状都是**静默的** —— 只能靠"从来没出现过"察觉，所以宁可在这儿写死。
///
/// ## 语义
/// · `facilities` / `cell_m`：整体覆盖（每次都是新小区，不存在"合并"的意义）；
/// · `scene.place`：总是写（小区名，注入里「你在：…·东山口」那一截）；
/// · `scene.area`：**只在还没有 area 时**补 —— 前端推的行政区链路更权威，
///   不要去覆盖它（`world_sim_enabled()` 的判据是 scene 在不在，不是 area 是谁）。
///
/// 拿不到写锁（前端正好在推状态）时把这次写入交给一个后台任务，**不丢**。
pub fn install_facilities(area: &str, facilities: &Value, cell_m: f64) {
    let Some(list) = facilities.as_array() else {
        return;
    };
    if list.is_empty() {
        return;
    }
    let area = area.trim().to_string();
    let list = Value::Array(list.clone());
    let cell = if cell_m > 0.0 {
        cell_m
    } else {
        super::facilities::DEFAULT_CELL_METERS
    };

    let apply = move |guard: &mut MapRuntime| {
        guard.facilities = list.clone();
        if cell > 0.0 {
            guard.cell_m = cell;
        }
        let place = area.rsplit('·').next().unwrap_or(area.as_str()).trim().to_string();
        if place.is_empty() {
            return;
        }
        let slot = guard.scene.get_or_insert_with(|| json!({}));
        if slot.is_object() {
            if let Some(obj) = slot.as_object_mut() {
                obj.insert("place".into(), Value::String(place));
                if !area.is_empty() && str_of_opt(obj.get("area")).is_none() {
                    obj.insert("area".into(), Value::String(area.clone()));
                }
            }
        }
        guard.updated_at = MapRuntime::now_secs();
    };

    match RUNTIME.try_write() {
        Ok(mut guard) => apply(&mut guard),
        Err(_) => {
            tauri::async_runtime::spawn(async move {
                let mut guard = RUNTIME.write().await;
                apply(&mut guard);
            });
        }
    }
}

/// 取对象里一个非空字符串（`install_facilities` 判"area 有没有"用）。
fn str_of_opt(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

// ═══════════════════════════════════════════════════════════════════
//  Tauri 命令
// ═══════════════════════════════════════════════════════════════════
/// `world_map_update_runtime` —— 前端把当前场景/玩家位置/角色位置推上来。
///
/// **只做字段级合并**（语义表见 [`MapRuntime::apply_patch`]）：前端可以只推变化的那一块，
/// 不会把其余状态抹掉。返回合并后的完整 runtime，前端可以直接拿去渲染/对账。
///
/// 注册路径必须是 `world_map::state::world_map_update_runtime`（命令宏在定义处生成）。
#[tauri::command]
pub async fn world_map_update_runtime(
    state: State<'_, MapRuntimeHandle>,
    patch: Value,
) -> Result<Value, String> {
    if !patch.is_object() {
        return Err("patch 必须是 JSON object（要清空某个字段就传 null 值，别整体传数组）".into());
    }
    let runtime = state.0.clone();
    let (changed, snapshot) = {
        let mut guard = runtime.write().await;
        let changed = guard.apply_patch(&patch);
        let now = MapRuntime::now_secs();
        (changed, guard.to_json(now))
    };
    Ok(json!({ "ok": true, "changed": changed, "runtime": snapshot }))
}

/// `world_map_runtime` —— 读出运行时状态（前端与调试用）。
///
/// 额外给一个 `injection` 预览：`world_map_runtime()` 不带参数时用
/// `current_role` 当预览对象，也可以传 `role` 指定看某个角色的注入文本。
#[tauri::command]
pub async fn world_map_runtime(
    state: State<'_, MapRuntimeHandle>,
    role: Option<String>,
) -> Result<Value, String> {
    let runtime = state.0.clone();
    let (mut snapshot, current_role, preview) = {
        let guard = runtime.read().await;
        let now = MapRuntime::now_secs();
        let current = guard.current_role.clone();
        let who = role.clone().or_else(|| current.clone());
        let preview = who.as_deref().map(|w| guard.preview(w)).unwrap_or_default();
        (guard.to_json(now), current, preview)
    };
    snapshot["injection"] = json!({
        "role": role.or(current_role),
        "text": preview,
        "weather_ttl_secs": WEATHER_TTL_SECS,
        "events_max": EVENTS_MAX,
    });
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// patch 合并：只推一块不会抹掉别的；null 才是清空。
    #[test]
    fn patch_merges_fields_and_never_wipes_on_partial_update() {
        let mut rt = MapRuntime::new();
        let changed = rt.apply_patch(&json!({
            "scene": {"area": "广州市·越秀区·东山口"},
            "me": {"area": "广州市·越秀区", "gx": 13.0, "gy": 4.0},
            "actors": {"小满": {"facility": "便利店", "x": 3.0, "y": 4.0}},
        }));
        assert!(changed.contains(&"scene".to_string()));
        assert!(changed.contains(&"actors".to_string()));

        // 第二次只推玩家坐标：场景与角色必须原样保留
        rt.apply_patch(&json!({"me": {"gx": 14.0}}));
        assert_eq!(rt.scene.as_ref().unwrap()["area"], json!("广州市·越秀区·东山口"));
        assert_eq!(rt.me.as_ref().unwrap()["area"], json!("广州市·越秀区"));
        assert_eq!(rt.me.as_ref().unwrap()["gx"], json!(14.0));
        assert_eq!(rt.actors["小满"]["facility"], json!("便利店"));

        // null 才清空
        rt.apply_patch(&json!({"me": null}));
        assert!(rt.me.is_none());
        assert!(rt.scene.is_some());
    }

    /// 事件环形缓冲 + 天气时间戳 + 过期判断。
    #[test]
    fn events_and_weather_bookkeeping() {
        let mut rt = MapRuntime::new();
        for i in 0..(EVENTS_MAX + 5) {
            rt.apply_patch(&json!({"events": [{"text": format!("第{i}件事")}]}));
        }
        assert_eq!(rt.events.len(), EVENTS_MAX);
        assert_eq!(rt.events.last().unwrap()["text"], json!(format!("第{}件事", EVENTS_MAX + 4)));

        rt.apply_patch(&json!({"weather": {"desc": "小雨", "temp_c": 22}}));
        let now = MapRuntime::now_secs();
        assert!(rt.fresh_weather(now).is_some());
        assert!(rt.fresh_weather(now + WEATHER_TTL_SECS + 1).is_none());

        rt.apply_patch(&json!({"clear_events": true}));
        assert!(rt.events.is_empty());
    }

    /// 注入缓存：同样输入复用同一文本，位置一变就换新文本。
    #[test]
    fn injection_cache_reuses_text_until_something_changes() {
        let mut rt = MapRuntime::new();
        rt.apply_patch(&json!({
            "scene": {"area": "广州市·越秀区·东山口"},
            "actors": {"小满": {"facility": "便利店", "x": 3.0, "y": 4.0}},
        }));
        let now = Local::now();
        let first = rt.injection_text("小满", &now);
        assert!(first.starts_with("【当前场景】\n你在：广州市·越秀区·东山口·便利店里"));
        let fp_before = rt.injection.get("小满").unwrap().fp;
        let again = rt.injection_text("小满", &now);
        assert_eq!(first, again);
        assert_eq!(rt.injection.get("小满").unwrap().fp, fp_before);

        rt.apply_patch(&json!({"actors": {"小满": {"facility": "咖啡馆"}}}));
        let moved = rt.injection_text("小满", &now);
        assert!(moved.contains("咖啡馆"), "{moved}");
        assert_ne!(rt.injection.get("小满").unwrap().fp, fp_before);
    }

    /// 空 runtime：不注入、缓存里也不留垃圾。
    #[test]
    fn empty_runtime_injects_nothing() {
        let mut rt = MapRuntime::new();
        assert!(rt.injection_text("小满", &Local::now()).is_empty());
        assert!(rt.injection.is_empty());
    }

    /// move_to 只记意图：pending_move 落地 + 一条事件，**不动 actors**。
    #[test]
    fn move_intent_is_recorded_without_moving_anyone() {
        let mut rt = MapRuntime::new();
        assert!(!rt.push_event(Value::Null));
        assert!(rt.push_event(json!({"kind": "move", "text": "小满动身去咖啡馆"})));
        assert_eq!(rt.events.len(), 1);
        assert_eq!(rt.actors, json!({}));
    }

    /// 非 object 的 patch 一律不动状态（命令层会先拦，这里再兜一层）。
    #[test]
    fn non_object_patch_is_ignored() {
        let mut rt = MapRuntime::new();
        assert!(rt.apply_patch(&json!([1, 2, 3])).is_empty());
        assert!(rt.apply_patch(&json!("x")).is_empty());
        assert!(rt.updated_at == 0);
    }
}
