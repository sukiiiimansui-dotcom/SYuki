//! 世界模拟 · 现实事件引擎的**接线层**（P5-1 收尾 + P5-2 / P5-3 的后端一半）
//!
//! 上游是 [`super::events`]（纯函数：`plan_event` / `global_throttled` / 四种文案形态），
//! 下游是前端（Tauri 命令 + `world_map:event` 广播）与记忆管线（待写记忆队列）。
//! 本文件只做四件事：**组上下文 → 查节流 → 摇号 → 落状态 + 广播**。
//!
//! ## 为什么"组上下文"要抽成不依赖 `AppHandle` 的纯函数
//!
//! 本机（Termux/Android）不能跑 `cargo test`（Tauri 全量编译 40 分钟起），能真跑的
//! 只有"用已编译好的 rlib + `rustc --test`"这一条路。`#[tauri::command]` 要
//! `AppHandle`/`State`，测试里构造不出来 —— 所以凡是**判定逻辑**（字段映射、天气
//! 过期、事件历史映射、闸门、drain 语义）全部放在不碰 Tauri 的纯函数里，
//! 命令体里只剩下"取锁 → 调纯函数 → `app.emit`"。这也是 [`build_context`] 的
//! 签名（`&RuntimeSnapshot` + `role` + `now`）的由来。
//!
//! ## 三条命令
//!
//! | 命令 | 作用 | 谁调 |
//! |---|---|---|
//! | [`world_map_tick`] | 事件引擎的驱动器（兼心跳） | 前端地图页，每 5–30 秒一次 |
//! | [`world_map_events_recent`] | 读最近 N 条已发生事件 | 事件流面板 / 调试 |
//! | [`world_map_take_pending_memory`] | **取走**待写记忆（drain） | 记忆管线的接入点 |
//!
//! ## 锁纪律（本项目踩过 `std::sync::Mutex` 非重入自死锁，这里刻意避开）
//!
//! * 快照与"落状态"是**两段互不重叠**的锁：读锁 → 组上下文/摇号（锁外）→ 写锁 → 广播（锁外）。
//! * 命令里没有任何"持锁再调另一个也加锁的函数"的嵌套。
//! * `std::sync::Mutex` 一个都没有；`MapRuntime` 的 `tokio::sync::RwLock` 只在
//!   命令体里用 `.read().await` / `.write().await`，且 `await` 期间持锁时间极短。
//!
//! ## 两个刻意的取舍（如实记录）
//!
//! * **没有场景（`scene == null`）时直接不出事**（返回 `reason:"no_scene"`）。
//!   世界模拟没开时前端从不推 `scene`，而事件会写进待写记忆队列、可能被接进对话
//!   记忆 —— 那就等于"世界模拟关着也在改变对话行为"，是 K53 底线不允许的。
//!   所以这一层闸门比节流更靠前。
//! * **写锁内会再查一次节流**（双检）：前端（或多个组件）可能在同一秒里并发调两次
//!   `world_map_tick`，两次都读到"还没有事件"的快照 → 会在同一秒里连出两件事。
//!   第二次检查在写锁内做，代价只有一次"最近事件"的重算。

use chrono::{DateTime, Local, Timelike};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use super::events::{self, EventContext, PlannedEvent, RecentEvent};
// `state::` 只在 `mod tests` 里用（测试模块自己 `use super::super::state;`），
// 所以这里**不要**再 `self` 进来 —— 非 test 构建会报 `unused import: self`。
use super::state::{MapRuntime, MapRuntimeHandle, PendingMemory};
use super::summary::{self, WEATHER_TTL_SECS};

/// 前端事件名：抽中一条事件就广播一次（前端不监听也没有副作用）。
pub const EVENT_WORLD: &str = "world_map:event";

/// `world_map_events_recent` 的默认条数。
pub const RECENT_DEFAULT: usize = 20;

/// `world_map_events_recent` 的上限（再多就是让前端一次吞一个列表，
/// 而且内存里的环形缓冲本来就只留 `state::EVENTS_MAX` = 20 条）。
pub const RECENT_MAX: usize = 100;

// ═══════════════════════════════════════════════════════════════════
//  环境快照（把 `MapRuntime` 的字段搬到一个可脱离锁/脱离 Tauri 的结构里）
// ═══════════════════════════════════════════════════════════════════

/// 组 `EventContext` 需要的全部**只读**输入。
///
/// 存在的理由：`build_context` 要能在单测里直接构造（不碰 `RwLock`、不碰 `AppHandle`）。
/// 字段与 [`MapRuntime`] 一一对应，**只读不改**。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuntimeSnapshot {
    pub scene: Option<Value>,
    pub me: Option<Value>,
    pub actors: Value,
    pub events: Vec<Value>,
    /// `(抓取时刻, 天气对象)` —— 与 `MapRuntime::weather` 同形状
    pub weather: Option<(u64, Value)>,
    pub facilities: Value,
    pub cell_m: f64,
    pub current_role: Option<String>,
}

impl RuntimeSnapshot {
    /// 从运行时状态拷一份快照（**调用方持锁**，本函数不取锁）。
    pub fn from_runtime(rt: &MapRuntime) -> Self {
        Self {
            scene: rt.scene.clone(),
            me: rt.me.clone(),
            actors: rt.actors.clone(),
            events: rt.events.clone(),
            weather: rt.weather.clone(),
            facilities: rt.facilities.clone(),
            cell_m: rt.cell_m,
            current_role: rt.current_role.clone(),
        }
    }

    /// 世界模拟开着吗（唯一判据是**有没有 `scene`**，与 `state::world_sim_enabled` 同款）。
    pub fn has_scene(&self) -> bool {
        self.scene.is_some()
    }
}

// ═══════════════════════════════════════════════════════════════════
//  纯函数：字段映射
// ═══════════════════════════════════════════════════════════════════

/// 这次 tick 是给谁算的：显式 `role` 优先，其次 `current_role`，都没有就是空串
/// （`EventContext.role` 为空时 `{role}` 占位符会渲染成"你"，见 `events.rs`）。
pub fn resolve_role<'a>(snapshot: &'a RuntimeSnapshot, role: Option<&'a str>) -> &'a str {
    role.map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            snapshot
                .current_role
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("")
}

/// 天气快照 → `(desc, temp_c)`：**过期（> [`WEATHER_TTL_SECS`]）/ 带 `error` / 空**
/// 一律当"没有天气"。
///
/// 这一条与 `summary.rs` 的注入路径**完全一致**（那边是 `MapRuntime::fresh_weather`
/// 判过期 + `render` 里 `w.get("error").is_none()` 才取 `desc`/`temp_c`）：
/// 注入文本里写着"晴 26°C"、事件引擎却按"下雨"开闸，模型会收到自相矛盾的上下文。
/// 设备时钟被改到过去时 `at > now`（差值为负）不算过期 —— 与 `saturating_sub` 同效。
pub fn weather_of(weather: Option<&(u64, Value)>, now_secs: i64) -> (String, Option<f64>) {
    let Some((at, data)) = weather else {
        return (String::new(), None);
    };
    if now_secs.saturating_sub(*at as i64) > WEATHER_TTL_SECS as i64 {
        return (String::new(), None);
    }
    if data.get("error").is_some() {
        return (String::new(), None);
    }
    let desc = summary::str_of(data, "desc")
        .or_else(|| summary::str_of(data, "desc_en"))
        .unwrap_or_default();
    // 非有限值（`"inf"` 之类的手滑数据）当没有：不能让闸门拿到 NaN。
    let temp = summary::num_of(data, "temp_c").filter(|t| t.is_finite());
    (desc, temp)
}

/// 角色记录里的布尔开关（`indoor` / `indoors` / `inside` 都认）。
fn actor_flag(rec: Option<&Value>, keys: &[&str]) -> Option<bool> {
    let rec = rec?;
    keys.iter().find_map(|k| rec.get(*k).and_then(Value::as_bool))
}

/// 角色记录里的 0.0–1.0 标量（心情/体力）。
///
/// **越界一律当"不知道"**（`None`）而不是夹到边界：`events.rs` 的阈值是按
/// 0–1 定的（`LOW_MOOD_THRESHOLD = 0.35`），前端要是推上来一个 `80`（百分制），
/// 夹成 1.0 会让"低落"永远不成立，反而比"不知道"更误导。
fn actor_unit(rec: Option<&Value>, keys: &[&str]) -> Option<f64> {
    let rec = rec?;
    keys.iter()
        .find_map(|k| summary::num_of(rec, k))
        .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
}

/// 设施表里的 `indoor` 开关（按设施名匹配，宽容同名的大小写/空白）。
///
/// 配套：`facilities.rs::indoor_of` 会给每条设施记录写一个 `indoor` 字段
/// （在此之前没有，于是 `indoors` 恒为 `false`，停电/失眠那批室内事件永不触发）。
fn facility_indoor(facilities: &Value, place: &str) -> Option<bool> {
    facility_record(facilities, place)
        .and_then(|f| actor_flag(Some(f), &["indoor", "indoors", "inside"]))
}

/// 设施表里的类型（`type`）—— 按设施名匹配，同一套宽容口径。
///
/// 为什么要它当 `place_kind` 的兜底：前端只推得来 `actors[role].facility`（设施**名**），
/// 推不来 `type`。没有它的话 `place_kind` 恒为空 → `events.rs` 里
/// 「在餐厅/商场提高消费类」「在公园提高户外类」这两条权重修正**永远不生效**，
/// 而事件照样在出，只是分布不对 —— 又是那种不报错的坏法。
fn facility_kind(facilities: &Value, place: &str) -> Option<String> {
    facility_record(facilities, place)
        .and_then(|f| summary::str_of(f, "type").or_else(|| summary::str_of(f, "group")))
}

/// 按名字在设施表里找一条（去首尾空白 + 忽略 ASCII 大小写）。
fn facility_record<'a>(facilities: &'a Value, place: &str) -> Option<&'a Value> {
    if place.trim().is_empty() {
        return None;
    }
    facilities.as_array()?.iter().find(|f| {
        summary::str_of(f, "name").is_some_and(|n| n.trim().eq_ignore_ascii_case(place.trim()))
    })
}

/// 玩家与角色的距离（米）：**格点距离 × 格边长**优先，退化到经纬度 haversine。
///
/// 与 `summary.rs::render` 的「用户位置（约 300m）」用的是同一对函数、同一套优先级 ——
/// 注入里写着"约 300m"、事件引擎按 5km 的"很远"因子调权重就自相矛盾了。
pub fn player_distance(actors: &Value, me: &Value, role: &str, cell_m: f64) -> Option<f64> {
    let null = Value::Null;
    let pos = summary::actor_pos(actors, role);
    let (ax, ay) = (pos.x, pos.y);
    let (mx, my) = summary::cell_xy(me);
    let rec = summary::actor_record(actors, role).unwrap_or(&null);
    let (alng, alat) = summary::lng_lat(rec);
    let (mlng, mlat) = summary::lng_lat(me);
    summary::distance_m((ax, ay), (mx, my), cell_m)
        .or_else(|| summary::geo_distance_m((alng, alat), (mlng, mlat)))
        .filter(|d| d.is_finite() && *d >= 0.0)
}

/// 今天是不是节日：`scene.festival`（也认 `is_festival`）为真，或 `scene.festival_name` 非空。
///
/// 返回 `(festival, festival_name)`，且**不是节日时 `festival_name` 一定是 `None`**
/// （占位符 `{festival}` 只在节日文案里出现，留一个孤儿名字只会误导）。
/// 本函数不带农历表：节日数据是数据问题，不该焊死在事件引擎里（`events.rs` 同款说明）。
pub fn festival_of(scene: &Value) -> (bool, Option<String>) {
    let name = summary::str_of(scene, "festival_name");
    let flag = scene
        .get("festival")
        .and_then(Value::as_bool)
        .or_else(|| scene.get("is_festival").and_then(Value::as_bool))
        .unwrap_or(false);
    let festival = flag || name.is_some();
    (festival, if festival { name } else { None })
}

/// `MapRuntime.events`（自由 JSON）→ 事件引擎认识的 [`RecentEvent`] 列表。
///
/// 两种形状都吃：
/// * 事件引擎自己落的 `PlannedEvent`（`id` / `category` / `at_secs`）—— 直接反序列化；
/// * 老事件（`move_cmd` 落的 `{"kind":"move","text":"…","at":123}` 之类）—— 退到
///   `RecentEvent::at(id, at_secs)`：`id` 认 `id`/`kind`/`event_id`，时刻认
///   `at_secs`/`at`/`ts`（数字或字符串数字）。
///
/// **没有时刻的条目按 `at_secs = 0`（1970）处理**：冷却与"最近一条"都只看
/// `now - at`，1970 既不会误触发节流，也不会被当成"刚发生"。
/// 连 `id` 和时刻都没有的条目（`{"text":"…"}`）直接丢掉 —— 它们对引擎没有任何用。
pub fn recent_from(events: &[Value]) -> Vec<RecentEvent> {
    events.iter().filter_map(recent_one).collect()
}

fn recent_one(v: &Value) -> Option<RecentEvent> {
    v.as_object()?;
    // ① 先按 `PlannedEvent` 序列化出来的形状整体反序列化（`category` 能一起还原）
    let parsed: Option<RecentEvent> = serde_json::from_value(v.clone()).ok();
    // ② `id` / 时刻再各取一次：老事件（`{"kind":"move","at":123}`）走这条，
    //    "有新 id 但时刻写成老键 `at`" 的混合形状也能救回来
    let id = parsed
        .as_ref()
        .map(|r| r.id.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            ["kind", "event_id"]
                .iter()
                .find_map(|k| summary::str_of(v, k))
        })
        .unwrap_or_default();
    let at = ["at_secs", "at", "ts"]
        .iter()
        .find_map(|k| summary::num_of(v, k))
        .map(|t| t as i64)
        .unwrap_or(0);
    if id.is_empty() && at == 0 {
        return None;
    }
    Some(RecentEvent {
        id,
        category: parsed.and_then(|r| r.category),
        at_secs: at,
    })
}

/// 组装一次事件触发需要的世界上下文（**纯函数**，不取锁、不碰 Tauri、不取系统时间）。
///
/// 字段映射表（取不到时的默认值都写在括号里）：
///
/// | `EventContext` | 来源 | 默认 |
/// |---|---|---|
/// | `role` | 参数 `role` → `current_role` | `""`（占位符渲染成"你"） |
/// | `area` | `scene.area` → `scene.label` → `scene.path[].name` → `scene.name` | `""` |
/// | `place` | `actors[role].facility` → `.place` → `.name` | `""` |
/// | `place_kind` | `actors[role].type` → `.kind` | `""` |
/// | `indoors` | `actors[role].indoor/indoors/inside` → 设施表同名开关 | `false`（按户外） |
/// | `hour` | `now.hour()`（本机时区） | —— |
/// | `weather` | `weather.desc` → `.desc_en`（**过期/`error` 当没有**） | `""` |
/// | `temp_c` | `weather.temp_c`（数字或字符串数字） | `None` |
/// | `mood` / `energy` | `actors[role].mood` / `.energy`（须落在 0–1） | `None` |
/// | `player_distance_m` | 角色格点 ↔ `me` 格点 ×`cell_m`，退化到经纬度 | `None` |
/// | `festival` | `scene.festival` / `.is_festival` / 有 `festival_name` | `false` |
/// | `festival_name` | `scene.festival_name`（非节日时强制 `None`） | `None` |
/// | `recent` | `events` 环形缓冲 → [`recent_from`] | `[]` |
pub fn build_context(
    snapshot: &RuntimeSnapshot,
    role: Option<&str>,
    now: &DateTime<Local>,
) -> EventContext {
    let who = resolve_role(snapshot, role);
    let now_secs = now.timestamp();

    let null = Value::Null;
    let scene = snapshot.scene.as_ref().unwrap_or(&null);
    let me = snapshot.me.as_ref().unwrap_or(&null);
    let rec = summary::actor_record(&snapshot.actors, who);
    let pos = summary::actor_pos(&snapshot.actors, who);
    let place = pos.place.clone().unwrap_or_default();
    // `place_kind`：前端推了 `type` 就用它；没推（常态）就从设施表按名字反查。
    // 两者都没有才留空 —— 空着只会少两条权重修正，不会崩。
    let place_kind = pos
        .kind
        .clone()
        .or_else(|| facility_kind(&snapshot.facilities, &place))
        .unwrap_or_default();

    let (weather, temp_c) = weather_of(snapshot.weather.as_ref(), now_secs);
    let (festival, festival_name) = festival_of(scene);
    // 室内外：前端/设施表给了就用，没给按"户外"（`events.rs` 已声明的取舍）
    let indoors = actor_flag(rec, &["indoor", "indoors", "inside"])
        .or_else(|| facility_indoor(&snapshot.facilities, &place))
        .unwrap_or(false);

    EventContext {
        role: who.to_string(),
        area: summary::scene_label(scene).unwrap_or_default(),
        place,
        place_kind,
        indoors,
        // `Timelike::hour()` 保证 0..=23；用 chrono 而不是 libc（Windows 上编不过）
        hour: now.hour() as u8,
        weather,
        temp_c,
        mood: actor_unit(rec, &["mood"]),
        energy: actor_unit(rec, &["energy", "stamina"]),
        player_distance_m: player_distance(&snapshot.actors, me, who, snapshot.cell_m),
        festival,
        festival_name,
        recent: recent_from(&snapshot.events),
    }
}

// ═══════════════════════════════════════════════════════════════════
//  纯函数：闸门 / 落状态
// ═══════════════════════════════════════════════════════════════════

/// tick 的闸门：`Some((reason, next_ok_in_secs))` = 这一轮**不许摇号**。
///
/// 顺序是刻意的：先看"世界模拟开着吗"（`scene` 缺失 = 前端没在世界模拟里，
/// 这时出事件会污染对话记忆），再看全局节流。两条都能省掉一次摇号，
/// 顺带把"下次最早什么时候来"告诉前端。
pub fn tick_gate(
    snapshot: &RuntimeSnapshot,
    ctx: &EventContext,
    now_secs: i64,
) -> Option<(&'static str, i64)> {
    if !snapshot.has_scene() {
        return Some(("no_scene", 0));
    }
    if events::global_throttled(ctx, now_secs) {
        let remain = events::last_event_at(ctx, now_secs)
            .map(|at| (events::MIN_GAP_SECS - (now_secs - at)).max(0))
            .unwrap_or(0);
        return Some(("throttled", remain));
    }
    None
}

/// 写锁内的**双检**：只看 `events` 缓冲重算一次节流。
///
/// 存在的理由：两次并发 tick 可能都读到"还没有事件"的快照，各自摇出一条事件。
/// 这个函数在写锁内跑，代价只有一次"最近事件"的重算（不克隆 actors/facilities）。
pub fn throttled_now(runtime: &MapRuntime, now_secs: i64) -> bool {
    let ctx = EventContext {
        recent: recent_from(&runtime.events),
        ..Default::default()
    };
    events::global_throttled(&ctx, now_secs)
}

/// 一次已抽中事件的**全部产物**（四通道文案 + 落进 `events` 的那份 JSON）。
#[derive(Debug, Clone, PartialEq)]
pub struct EventOutcome {
    /// `PlannedEvent` 的 JSON（= 同时落进 `MapRuntime.events` 的那一份）
    pub event: Value,
    pub popup: String,
    pub bubble: String,
    pub speech_hint: String,
    pub memory_line: String,
    pub at: i64,
    /// 这条事件是给谁的（广播载荷里的额外字段，见 [`EventOutcome::payload`]）
    pub role: String,
}

impl EventOutcome {
    /// `world_map:event` 的载荷形状（**前端契约**）：
    /// `{ event, popup, bubble, speech_hint, memory_line, at, role }`。
    /// `role` 是在约定的 6 个字段之外**多加的一个**（前端放气泡要知道挂在谁头上）；
    /// 只加不改，按老契约解构的前端不受影响。
    pub fn payload(&self) -> Value {
        json!({
            "event": self.event,
            "popup": self.popup,
            "bubble": self.bubble,
            "speech_hint": self.speech_hint,
            "memory_line": self.memory_line,
            "at": self.at,
            "role": self.role,
        })
    }
}

/// 把一条已抽中的事件落进运行时状态（**纯函数**：调用方持写锁）：
/// ① 事件本体进 `events` 环形缓冲（下一轮注入的「最近：…」就能接上）；
/// ② `memory_line` 进待写记忆队列（等前端 `world_map_take_pending_memory` 取走）。
///
/// 返回四通道文案（`events::*` 的格式化函数原样调用，本文件不自己拼文案）。
pub fn commit_event(
    runtime: &mut MapRuntime,
    ev: &PlannedEvent,
    role: &str,
    now_secs: i64,
) -> Result<EventOutcome, String> {
    let event = serde_json::to_value(ev).map_err(|e| format!("事件序列化失败: {e}"))?;
    runtime.push_event(event.clone());
    let memory_line = events::memory_line(ev);
    runtime.push_memory(role, memory_line.clone(), now_secs);
    Ok(EventOutcome {
        event,
        popup: events::popup_text(ev),
        bubble: events::bubble_text(ev),
        speech_hint: events::speech_hint(ev),
        memory_line,
        at: now_secs,
        role: role.to_string(),
    })
}

/// `world_map_events_recent` 的条数：缺省 [`RECENT_DEFAULT`]，上限 [`RECENT_MAX`]。
/// `Some(0)` 就是 0 条（照字面意思办，不偷偷给 1 条）。
pub fn clamp_limit(limit: Option<usize>) -> usize {
    match limit {
        None => RECENT_DEFAULT,
        Some(n) => n.min(RECENT_MAX),
    }
}

/// 最近的 N 条**事件引擎事件**（`PlannedEvent` 形状的才认）。
///
/// 返回的是**时间正序的尾巴**（最早在前、最新在后，与 `events` 环形缓冲同序）：
/// 前端要"最新的在最上面"自己 `reverse()` 一下即可，注入侧的「最近：…」也是取最后一条。
/// 认不出的条目（`move_cmd` 落的老事件、脏数据）直接跳过而不是报错。
pub fn recent_events(events: &[Value], limit: Option<usize>) -> Vec<PlannedEvent> {
    let n = clamp_limit(limit);
    if n == 0 {
        return Vec::new();
    }
    let parsed: Vec<PlannedEvent> = events
        .iter()
        .filter_map(|v| serde_json::from_value::<PlannedEvent>(v.clone()).ok())
        .collect();
    let start = parsed.len().saturating_sub(n);
    parsed[start..].to_vec()
}

/// 待写记忆 → `{role, line, at}` 列表（命令层回包用）。
pub fn pending_json(items: &[PendingMemory]) -> Vec<Value> {
    items.iter().map(PendingMemory::to_json).collect()
}

// ═══════════════════════════════════════════════════════════════════
//  Tauri 命令
// ═══════════════════════════════════════════════════════════════════

/// `world_map_tick` —— **事件引擎的驱动器**（前端每隔几秒调一次，它同时兼心跳）。
///
/// 参数 `role` 缺省用 `current_role`。返回值（前端契约，字段名写全）：
///
/// * 抽中：`{ ok:true, fired:true, at, role, next_ok_in_secs, event:{id,category,title,text,weight_used,at_secs},
///   popup, bubble, speech_hint, memory_line }`
/// * 没抽中：`{ ok:true, fired:false, reason:"no_candidate", at, role, next_ok_in_secs:0 }`
/// * 节流中：`{ ok:true, fired:false, reason:"throttled", at, role, next_ok_in_secs }`
/// * 世界模拟没开：`{ ok:true, fired:false, reason:"no_scene", at, role, next_ok_in_secs:0 }`
///
/// `reason` 只在 `fired:false` 时出现；`next_ok_in_secs` 四种情况都有
/// （= 距"下一次最早可能出事"还有多少秒，前端可以据此放缓轮询）。
///
/// **永远返回 `Ok`**（除了内部序列化失败）：这是每几秒一次的心跳，前端不该为了
/// "这一刻没发生事情"去处理一个 `Err`。
///
/// 抽中时同时向**所有窗口**广播 `world_map:event`（载荷见 [`EventOutcome::payload`]）。
/// 注册路径必须是 `world_map::event_cmd::world_map_tick`（命令宏在定义处生成，写短了 E0433）。
#[tauri::command]
pub async fn world_map_tick(
    app: AppHandle,
    state: State<'_, MapRuntimeHandle>,
    role: Option<String>,
) -> Result<Value, String> {
    let now_dt = Local::now();
    let now = now_dt.timestamp();

    // ① 读一份快照：读锁只覆盖这一段，组上下文与摇号都在锁外做
    let snapshot = {
        let guard = state.0.read().await;
        RuntimeSnapshot::from_runtime(&guard)
    };
    let ctx = build_context(&snapshot, role.as_deref(), &now_dt);
    let who = ctx.role.clone();
    let reply = |fired: bool, reason: &str, next_ok: i64| {
        json!({
            "ok": true,
            "fired": fired,
            "reason": reason,
            "at": now,
            "role": who,
            "next_ok_in_secs": next_ok,
        })
    };

    // ② 闸门（省一次摇号）
    if let Some((reason, remain)) = tick_gate(&snapshot, &ctx, now) {
        return Ok(reply(false, reason, remain));
    }

    // ③ 摇号：随机数只在调用侧产生（`events.rs` 是纯函数，roll/rng 从参数进来）
    let roll: f64 = rand::random();
    let planned = events::plan_event(events::table(), &ctx, now, roll, &mut || rand::random::<f64>());
    let Some(ev) = planned else {
        return Ok(reply(false, "no_candidate", 0));
    };

    // ④ 落状态：写锁内**再查一次节流**（并发两次 tick 都摇中的话，这里只放一条过去）
    let outcome = {
        let mut guard = state.0.write().await;
        if throttled_now(&guard, now) {
            None
        } else {
            Some(commit_event(&mut guard, &ev, &who, now)?)
        }
    };
    let Some(outcome) = outcome else {
        return Ok(reply(false, "throttled", events::MIN_GAP_SECS));
    };

    // ⑤ 广播（锁已经放了；前端不监听也没有副作用）
    let payload = outcome.payload();
    if let Err(e) = app.emit(EVENT_WORLD, &payload) {
        tracing::warn!("emit {EVENT_WORLD} 失败: {e}");
    }
    tracing::info!(
        "[world_map] 事件触发: {} ({}) at={} next_ok_in={}s",
        outcome.event.get("id").and_then(|v| v.as_str()).unwrap_or("?"),
        who,
        now,
        events::MIN_GAP_SECS
    );

    Ok(json!({
        "ok": true,
        "fired": true,
        "at": outcome.at,
        "role": outcome.role,
        "next_ok_in_secs": events::MIN_GAP_SECS,
        "event": outcome.event,
        "popup": outcome.popup,
        "bubble": outcome.bubble,
        "speech_hint": outcome.speech_hint,
        "memory_line": outcome.memory_line,
    }))
}

/// `world_map_events_recent` —— 最近的 N 条事件（默认 20，上限 100）。
///
/// 返回 `{ ok, items:[PlannedEvent], count, limit }`；`items` 是**时间正序**的尾巴。
/// 只认事件引擎落的 `PlannedEvent`（`move_cmd` 那种老事件在这里读不到 ——
/// 它们由 `world_map_recent_events` 那条落盘通路负责）。
#[tauri::command]
pub async fn world_map_events_recent(
    state: State<'_, MapRuntimeHandle>,
    limit: Option<usize>,
) -> Result<Value, String> {
    let guard = state.0.read().await;
    let items = recent_events(&guard.events, limit);
    Ok(json!({
        "ok": true,
        "items": items,
        "count": items.len(),
        "limit": clamp_limit(limit),
    }))
}

/// `world_map_take_pending_memory` —— **取走**待写记忆（drain 语义：拿到即清空，
/// 同一个调用方拿两次不会重复）。
///
/// * `role` 传了（且非空白）：只取这个角色的行，其余留在队列里；
/// * `role` 不传 / 传空串：把队列**全部**取走。
///
/// 返回 `{ ok, role, lines:[String], count, items:[{role,line,at}] }`。
/// `lines` 是纯文本（`旁白: …`，直接喂记忆管线即可）；`items` 是带归属的明细，
/// `role` 缺省（取全部）时靠它才知道每行是谁的经历。
#[tauri::command]
pub async fn world_map_take_pending_memory(
    state: State<'_, MapRuntimeHandle>,
    role: Option<String>,
) -> Result<Value, String> {
    let wanted = role
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let (taken, left) = {
        let mut guard = state.0.write().await;
        let taken = guard.take_memory(wanted.as_deref());
        let left = guard.pending_memory_len();
        (taken, left)
    };
    let lines: Vec<String> = taken.iter().map(|p| p.line.clone()).collect();
    let items = pending_json(&taken);
    Ok(json!({
        "ok": true,
        "role": wanted,
        "lines": lines,
        "count": lines.len(),
        "items": items,
        // 取走之后队列里还剩几行（别的角色的）—— 前端据此判断要不要再来一次
        "pending_after": left,
    }))
}

// ═══════════════════════════════════════════════════════════════════
//  单测（全部脱离 `AppHandle`：这批是"接线逻辑"的回归基线）
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    /// `state` 模块本身（文件顶部的 `use` 只带进来了三个类型名）
    use super::super::state;

    /// 本机时区的固定时刻（`Local` 而不是 `Utc`：`hour` 字段要的就是本机小时）
    fn at(hour: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 9, 12, hour, 24, 0)
            .single()
            .expect("本机时区里这个时刻必须唯一")
    }

    fn snap(scene: Option<Value>, actors: Value) -> RuntimeSnapshot {
        RuntimeSnapshot {
            scene,
            actors,
            cell_m: 30.0,
            ..Default::default()
        }
    }

    fn scene_area() -> Value {
        json!({"area": "广州市·越秀区·东山口"})
    }

    fn actor_rec() -> Value {
        json!({"小满": {"facility": "便利店", "type": "commercial", "x": 3.0, "y": 4.0, "since": "19:20"}})
    }

    // ── build_context 的字段映射 ──────────────────────────────────

    #[test]
    fn build_context_maps_every_field_from_the_snapshot() {
        let now = at(19);
        let mut s = snap(Some(scene_area()), actor_rec());
        s.me = Some(json!({"area": "广州市·越秀区", "gx": 13.0, "gy": 4.0}));
        // 天气快照必须是"刚抓的"（拿同一个 `now` 当抓取时刻，不然按 TTL 就过期了）
        s.weather = Some((
            now.timestamp().max(0) as u64,
            json!({"desc": "小雨", "temp_c": 22}),
        ));
        s.current_role = Some("小满".into());
        let ctx = build_context(&s, None, &now);

        assert_eq!(ctx.role, "小满");
        assert_eq!(ctx.area, "广州市·越秀区·东山口");
        assert_eq!(ctx.place, "便利店");
        assert_eq!(ctx.place_kind, "commercial");
        assert_eq!(ctx.hour, 19);
        assert_eq!(ctx.weather, "小雨");
        assert_eq!(ctx.temp_c, Some(22.0));
        assert!(!ctx.festival);
        assert_eq!(ctx.festival_name, None);
        // 格点距离：(3,4) → (13,4) = 10 格 × 30m
        assert_eq!(ctx.player_distance_m, Some(300.0));
        // 没有 indoor 开关 → 按户外（events.rs 的取舍）
        assert!(!ctx.indoors);
        assert_eq!(ctx.mood, None);
        assert_eq!(ctx.energy, None);
    }

    #[test]
    fn build_context_role_prefers_argument_then_current_role() {
        let mut s = snap(Some(scene_area()), actor_rec());
        s.current_role = Some("小满".into());
        // 显式 role 优先
        assert_eq!(build_context(&s, Some("阿岚"), &at(19)).role, "阿岚");
        // 空白 role 当没传 → 退到 current_role
        assert_eq!(build_context(&s, Some("   "), &at(19)).role, "小满");
        assert_eq!(build_context(&s, None, &at(19)).role, "小满");
        // 都没有 → 空串（占位符会渲染成"你"）
        s.current_role = None;
        assert_eq!(build_context(&s, None, &at(19)).role, "");
    }

    #[test]
    fn build_context_uses_local_hour_and_normalizes_nothing() {
        for h in [0u32, 5, 12, 19, 23] {
            let ctx = build_context(&snap(Some(scene_area()), actor_rec()), None, &at(h));
            assert_eq!(ctx.hour, h as u8);
            assert_eq!(ctx.hour, ctx.hour_of());
        }
        // 深夜判据（23–5 点）也要跟着走：引擎的闸门靠它
        assert!(build_context(&snap(Some(scene_area()), actor_rec()), None, &at(23)).is_deep_night());
        assert!(!build_context(&snap(Some(scene_area()), actor_rec()), None, &at(12)).is_deep_night());
    }

    #[test]
    fn build_context_survives_a_missing_actor_record() {
        let s = snap(Some(scene_area()), json!({}));
        let ctx = build_context(&s, Some("小满"), &at(19));
        assert_eq!(ctx.place, "");
        assert_eq!(ctx.place_kind, "");
        assert_eq!(ctx.player_distance_m, None);
        assert!(!ctx.indoors);
    }

    // ── 天气快照的过期判定 ────────────────────────────────────────

    #[test]
    fn weather_snapshot_expires_after_the_ttl() {
        let snap_weather = (1_000_000u64, json!({"desc": "小雨", "temp_c": 22}));
        let now = 1_000_000i64;
        assert_eq!(
            weather_of(Some(&snap_weather), now),
            ("小雨".to_string(), Some(22.0))
        );
        // 正好 TTL：还算新鲜（与 `MapRuntime::fresh_weather` 的 `<=` 一致）
        assert_eq!(
            weather_of(Some(&snap_weather), now + WEATHER_TTL_SECS as i64),
            ("小雨".to_string(), Some(22.0))
        );
        // 超一秒就作废
        assert_eq!(
            weather_of(Some(&snap_weather), now + WEATHER_TTL_SECS as i64 + 1),
            (String::new(), None)
        );
        assert_eq!(weather_of(None, now), (String::new(), None));
    }

    #[test]
    fn weather_ignores_error_payloads_and_bad_numbers() {
        let now = 5_000i64;
        let err = (4_000u64, json!({"error": "网络不可达", "desc": "小雨"}));
        assert_eq!(weather_of(Some(&err), now), (String::new(), None));

        let en = (4_000u64, json!({"desc_en": "light rain", "temp_c": "22"}));
        assert_eq!(
            weather_of(Some(&en), now),
            ("light rain".to_string(), Some(22.0))
        );

        // 非有限值当没有（`"inf"` 这类手滑数据不能让闸门拿到 NaN）
        let bad = (4_000u64, json!({"desc": "小雨", "temp_c": "inf"}));
        assert_eq!(weather_of(Some(&bad), now), ("小雨".to_string(), None));

        let empty = (4_000u64, json!({}));
        assert_eq!(weather_of(Some(&empty), now), (String::new(), None));
    }

    // ── 节日 / 心情 / 体力 / 室内外 ───────────────────────────────

    #[test]
    fn festival_comes_from_the_scene_and_never_leaks_a_name() {
        assert_eq!(festival_of(&json!({})), (false, None));
        assert_eq!(
            festival_of(&json!({"festival": true, "festival_name": "中秋"})),
            (true, Some("中秋".into()))
        );
        // 有名字就算节日（与 `EventContext::is_festival` 同款）
        assert_eq!(
            festival_of(&json!({"festival_name": "中秋"})),
            (true, Some("中秋".into()))
        );
        // 名字在场时 `festival:false` 压不过它 —— 名字本身就是"今天是节日"的证据
        assert_eq!(
            festival_of(&json!({"festival": false, "festival_name": "中秋"})),
            (true, Some("中秋".into()))
        );
        // 不是节日（既没 flag 也没名字）→ 名字一定是 None
        assert_eq!(festival_of(&json!({"festival": false})), (false, None));
        assert_eq!(festival_of(&json!({"is_festival": true})), (true, None));
        // 空白名字不算
        assert_eq!(festival_of(&json!({"festival_name": "   "})), (false, None));
    }

    #[test]
    fn mood_and_energy_must_be_unit_scaled() {
        let actors = json!({"小满": {"mood": 0.2, "energy": "0.9"}});
        let ctx = build_context(&snap(Some(scene_area()), actors), Some("小满"), &at(19));
        assert_eq!(ctx.mood, Some(0.2));
        assert_eq!(ctx.energy, Some(0.9));

        // 百分制/越界一律当"不知道"：夹到 1.0 会让"低落"永远不成立
        let actors = json!({"小满": {"mood": 80, "energy": -1}});
        let ctx = build_context(&snap(Some(scene_area()), actors), Some("小满"), &at(19));
        assert_eq!(ctx.mood, None);
        assert_eq!(ctx.energy, None);
    }

    #[test]
    fn indoors_comes_from_actor_flag_or_the_facility_table() {
        let actors = json!({"小满": {"facility": "便利店", "indoor": true}});
        let mut s = snap(Some(scene_area()), actors);
        assert!(build_context(&s, Some("小满"), &at(19)).indoors);

        // 角色记录没写 → 查设施表
        s.actors = json!({"小满": {"facility": "便利店"}});
        s.facilities = json!([{"name": "便利店", "indoor": true}]);
        assert!(build_context(&s, Some("小满"), &at(19)).indoors);
        // 设施表里没有这家 → 默认户外
        s.facilities = json!([{"name": "公园", "indoor": false}]);
        assert!(!build_context(&s, Some("小满"), &at(19)).indoors);
        // 显式 false 优先于设施表（角色已经出门了）
        s.actors = json!({"小满": {"facility": "便利店", "indoors": false}});
        s.facilities = json!([{"name": "便利店", "indoor": true}]);
        assert!(!build_context(&s, Some("小满"), &at(19)).indoors);
    }

    #[test]
    fn player_distance_prefers_grid_then_geo() {
        let actors = json!({"小满": {"x": 0.0, "y": 0.0}});
        let me = json!({"gx": 2.0, "gy": 0.0});
        assert_eq!(player_distance(&actors, &me, "小满", 30.0), Some(60.0));

        // 没有格点 → 经纬度 haversine（0.001° 纬度 ≈ 111m）
        let actors = json!({"小满": {"lat": 23.1000, "lng": 113.2000}});
        let me = json!({"lat": 23.1010, "lng": 113.2000});
        let d = player_distance(&actors, &me, "小满", 30.0).expect("经纬度算得出距离");
        assert!((d - 111.2).abs() < 1.0, "haversine 结果异常: {d}");

        // 两边都没有 → None（不许凭空写"约 0m"）
        assert_eq!(player_distance(&json!({}), &json!({}), "小满", 30.0), None);
    }

    // ── 最近事件的映射 ────────────────────────────────────────────

    #[test]
    fn recent_from_reads_planned_events_back() {
        let ev = PlannedEvent {
            id: "traffic.jam".into(),
            category: events::Category::Traffic,
            title: "堵车".into(),
            text: "路上堵成一片".into(),
            weight_used: 3.0,
            at_secs: 1_700_000_000,
        };
        let value = serde_json::to_value(&ev).unwrap();
        let recent = recent_from(&[value]);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, "traffic.jam");
        assert_eq!(recent[0].category(), Some(events::Category::Traffic));
        assert_eq!(recent[0].at_secs, 1_700_000_000);
    }

    #[test]
    fn recent_from_falls_back_to_legacy_events() {
        // move_cmd 落的形状：{"kind":"move","at":123}
        let recent = recent_from(&[json!({"kind": "move", "text": "小满动身去咖啡馆", "at": 123})]);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, "move");
        assert_eq!(recent[0].at_secs, 123);
        assert_eq!(recent[0].category(), None); // "move" 不是类别前缀

        // 没有时刻 → 1970（既不触发节流也不被当成"刚发生"）
        let recent = recent_from(&[json!({"kind": "move", "text": "…"})]);
        assert_eq!(recent[0].at_secs, 0);

        // 字符串数字也认
        let recent = recent_from(&[json!({"id": "luck.puddle", "at": "456"})]);
        assert_eq!(recent[0].at_secs, 456);

        // 什么都没有的条目直接丢掉，顺序保持不变
        let recent = recent_from(&[
            json!("就是一句话"),
            json!({"text": "只有文案"}),
            json!({"id": "mood.good", "at_secs": 1}),
            json!({"id": "work.overtime", "at_secs": 2}),
        ]);
        assert_eq!(
            recent.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["mood.good", "work.overtime"]
        );
    }

    #[test]
    fn recent_from_feeds_the_global_throttle() {
        let now = 1_700_000_000i64;
        let ctx = |recent: Vec<RecentEvent>| EventContext {
            recent,
            ..Default::default()
        };
        // 刚发生过 → 节流生效
        assert!(events::global_throttled(
            &ctx(recent_from(&[json!({"id": "traffic.jam", "at_secs": now - 10})])),
            now
        ));
        // 5 分钟前 → 放行
        assert!(!events::global_throttled(
            &ctx(recent_from(&[json!({
                "id": "traffic.jam",
                "at_secs": now - events::MIN_GAP_SECS
            })])),
            now
        ));
        // 老事件（没有时刻）不该把引擎锁死
        assert!(!events::global_throttled(
            &ctx(recent_from(&[json!({"kind": "move"})])),
            now
        ));
    }

    // ── tick 的闸门 ───────────────────────────────────────────────

    #[test]
    fn tick_gate_requires_a_scene() {
        let s = snap(None, actor_rec());
        let ctx = build_context(&s, Some("小满"), &at(19));
        assert_eq!(tick_gate(&s, &ctx, 1_000), Some(("no_scene", 0)));
        // 世界模拟开着、最近没有事件 → 放行
        let s = snap(Some(scene_area()), actor_rec());
        let ctx = build_context(&s, Some("小满"), &at(19));
        assert_eq!(tick_gate(&s, &ctx, 1_000), None);
    }

    #[test]
    fn tick_gate_throttles_within_min_gap_and_reports_the_wait() {
        let s = snap(Some(scene_area()), actor_rec());
        let now = 1_700_000_000i64;
        let ctx = build_context(&s, Some("小满"), &at(19));
        // 100 秒前刚出过事 → 还要等 200 秒
        let ctx = EventContext {
            recent: vec![RecentEvent::at("traffic.jam", now - 100)],
            ..ctx
        };
        assert_eq!(tick_gate(&s, &ctx, now), Some(("throttled", 200)));
        // 整整 5 分钟前 → 放行
        let ctx = EventContext {
            recent: vec![RecentEvent::at("traffic.jam", now - events::MIN_GAP_SECS)],
            ..ctx
        };
        assert_eq!(tick_gate(&s, &ctx, now), None);
        // 时钟回拨/脏数据（时间戳比现在还晚的）**不参与全局节流**：
        // `events.rs::last_event_at` 只看 `at_secs <= now` 的，免得把引擎永久锁死。
        // 节流因此放行（该条事件自己的冷却仍生效，那是 cooldown_active 的事）。
        let ctx = EventContext {
            recent: vec![RecentEvent::at("traffic.jam", now + 9_999)],
            ..ctx
        };
        assert_eq!(tick_gate(&s, &ctx, now), None);
    }

    // ── 落状态：事件 + 待写记忆 ───────────────────────────────────

    #[test]
    fn commit_event_pushes_the_event_and_queues_the_memory_line() {
        let mut rt = MapRuntime::new();
        let ev = PlannedEvent {
            id: "health.oversleep".into(),
            category: events::Category::Health,
            title: "睡过头".into(),
            text: "闹钟响了三次都没听见，一睁眼已经十点半了。".into(),
            weight_used: 1.5,
            at_secs: 1_700_000_000,
        };
        let out = commit_event(&mut rt, &ev, "小满", 1_700_000_000).unwrap();

        // ① 事件进了环形缓冲（注入侧的「最近：…」就能接上）
        assert_eq!(rt.events.len(), 1);
        assert_eq!(rt.events[0]["id"], json!("health.oversleep"));
        assert_eq!(rt.events[0]["at_secs"], json!(1_700_000_000));
        assert_eq!(out.event, rt.events[0]);

        // ② 记忆行进了待写队列（形状 = events::memory_line）
        assert_eq!(rt.pending_memory_len(), 1);
        let taken = rt.take_memory(Some("小满"));
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].line, events::memory_line(&ev));
        assert!(taken[0].line.starts_with("旁白: "), "{}", taken[0].line);
        assert_eq!(taken[0].at, 1_700_000_000);
        assert_eq!(taken[0].role, "小满");
    }

    #[test]
    fn commit_event_payload_carries_the_four_text_forms() {
        let mut rt = MapRuntime::new();
        let ev = PlannedEvent {
            id: "traffic.jam".into(),
            category: events::Category::Traffic,
            title: "堵车".into(),
            text: "路上堵成一片，车挪得像蜗牛。".into(),
            weight_used: 2.0,
            at_secs: 42,
        };
        let out = commit_event(&mut rt, &ev, "小满", 42).unwrap();
        assert_eq!(out.popup, events::popup_text(&ev));
        assert_eq!(out.bubble, events::bubble_text(&ev));
        assert_eq!(out.speech_hint, events::speech_hint(&ev));
        assert_eq!(out.memory_line, events::memory_line(&ev));

        let payload = out.payload();
        for key in ["event", "popup", "bubble", "speech_hint", "memory_line", "at", "role"] {
            assert!(!payload[key].is_null(), "广播载荷缺字段 {key}");
        }
        assert_eq!(payload["at"], json!(42));
        assert_eq!(payload["role"], json!("小满"));
        assert!(payload["popup"].as_str().unwrap().starts_with("【堵车】"));
        assert!(payload["speech_hint"].as_str().unwrap().starts_with("刚才发生了："));
    }

    #[test]
    fn double_check_sees_an_event_committed_a_moment_ago() {
        let mut rt = MapRuntime::new();
        let now = 1_700_000_000i64;
        assert!(!throttled_now(&rt, now), "空状态不该被节流");
        let ev = PlannedEvent {
            id: "luck.puddle".into(),
            category: events::Category::Luck,
            title: "踩到水坑".into(),
            text: "一脚踩进积水里".into(),
            weight_used: 1.0,
            at_secs: now,
        };
        commit_event(&mut rt, &ev, "小满", now).unwrap();
        // 同一秒的第二次 tick：写锁内的双检必须拦住它
        assert!(throttled_now(&rt, now));
        assert!(throttled_now(&rt, now + events::MIN_GAP_SECS - 1));
        assert!(!throttled_now(&rt, now + events::MIN_GAP_SECS));
    }

    // ── 待写记忆的 drain 语义 ─────────────────────────────────────

    #[test]
    fn take_memory_drains_only_the_requested_role() {
        let mut rt = MapRuntime::new();
        assert!(rt.push_memory("小满", "旁白: 一", 1));
        assert!(rt.push_memory("阿岚", "旁白: 二", 2));
        assert!(rt.push_memory("小满", "旁白: 三", 3));

        let mine = rt.take_memory(Some("小满"));
        assert_eq!(
            mine.iter().map(|p| p.line.as_str()).collect::<Vec<_>>(),
            vec!["旁白: 一", "旁白: 三"]
        );
        assert_eq!(rt.pending_memory_len(), 1, "别人的行必须留在队列里");

        // 取过就没了（同一个调用方拿两次不会重复）
        assert!(rt.take_memory(Some("小满")).is_empty());
        // 宽容匹配：前后空格 / 大小写不影响
        assert!(rt.push_memory("小满", "旁白: 四", 4));
        assert_eq!(rt.take_memory(Some(" 小满 ")).len(), 1);
        assert!(rt.push_memory("Alice", "旁白: 五", 5));
        assert_eq!(rt.take_memory(Some("alice")).len(), 1);
    }

    #[test]
    fn take_memory_without_role_drains_everything() {
        let mut rt = MapRuntime::new();
        rt.push_memory("小满", "旁白: 一", 1);
        rt.push_memory("阿岚", "旁白: 二", 2);
        let all = rt.take_memory(None);
        assert_eq!(all.len(), 2);
        assert_eq!(rt.pending_memory_len(), 0);
        assert!(rt.take_memory(None).is_empty());
        // 空串 / 全空白 role 与 None 同义（前端把"不筛选"写成 ""）
        rt.push_memory("小满", "旁白: 三", 3);
        assert_eq!(rt.take_memory(Some("   ")).len(), 1);
    }

    #[test]
    fn pending_memory_is_capped_and_drops_the_oldest() {
        let mut rt = MapRuntime::new();
        for i in 0..(state::PENDING_MEMORY_MAX + 5) {
            assert!(rt.push_memory("小满", format!("旁白: 第{i}行"), i as i64));
        }
        assert_eq!(rt.pending_memory_len(), state::PENDING_MEMORY_MAX);
        let kept = rt.take_memory(Some("小满"));
        assert_eq!(kept.first().unwrap().line, "旁白: 第5行");
        assert_eq!(
            kept.last().unwrap().line,
            format!("旁白: 第{}行", state::PENDING_MEMORY_MAX + 4)
        );
        // 空行不进队列
        assert!(!rt.push_memory("小满", "   ", 1));
        assert_eq!(rt.pending_memory_len(), 0);
    }

    #[test]
    fn pending_memory_survives_a_partial_runtime_patch() {
        let mut rt = MapRuntime::new();
        rt.push_memory("小满", "旁白: 一", 1);
        // 前端推局部 patch（P3 的字段级合并）不能把队列抹掉 —— 这正是把它放进
        // MapRuntime 而不是模块级 static 的理由之一
        rt.apply_patch(&json!({"me": {"gx": 13.0, "gy": 4.0}}));
        rt.apply_patch(&json!({"clear_events": true, "pending_move": null}));
        assert_eq!(rt.pending_memory_len(), 1);
        assert_eq!(rt.take_memory(Some("小满"))[0].line, "旁白: 一");
    }

    // ── 读最近事件 / 条数夹取 ─────────────────────────────────────

    #[test]
    fn recent_events_returns_the_tail_and_skips_legacy_entries() {
        let mk = |id: &str, at: i64| {
            serde_json::to_value(PlannedEvent {
                id: id.into(),
                category: events::Category::Luck,
                title: "标题".into(),
                text: "文案".into(),
                weight_used: 1.0,
                at_secs: at,
            })
            .unwrap()
        };
        let events = vec![
            json!({"kind": "move", "text": "老事件，读不出来"}),
            mk("a", 1),
            mk("b", 2),
            mk("c", 3),
        ];
        let items = recent_events(&events, Some(2));
        assert_eq!(
            items.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "c"],
            "取的是时间正序的尾巴"
        );
        assert_eq!(recent_events(&events, None).len(), 3);
        assert!(recent_events(&events, Some(0)).is_empty());
        assert!(recent_events(&[], None).is_empty());
    }

    #[test]
    fn clamp_limit_defaults_and_caps() {
        assert_eq!(clamp_limit(None), RECENT_DEFAULT);
        assert_eq!(clamp_limit(Some(5)), 5);
        assert_eq!(clamp_limit(Some(RECENT_MAX + 1)), RECENT_MAX);
        assert_eq!(clamp_limit(Some(usize::MAX)), RECENT_MAX);
        assert_eq!(clamp_limit(Some(0)), 0);
    }

    #[test]
    fn pending_json_keeps_the_attribution() {
        let items = vec![
            PendingMemory {
                role: "小满".into(),
                line: "旁白: 一".into(),
                at: 1,
            },
            PendingMemory {
                role: "阿岚".into(),
                line: "旁白: 二".into(),
                at: 2,
            },
        ];
        let json_items = pending_json(&items);
        assert_eq!(json_items[0]["role"], json!("小满"));
        assert_eq!(json_items[1]["line"], json!("旁白: 二"));
        assert_eq!(json_items[0]["at"], json!(1));
    }

    // ── 快照 ──────────────────────────────────────────────────────

    #[test]
    fn snapshot_copies_the_runtime_without_taking_a_lock() {
        let mut rt = MapRuntime::new();
        rt.apply_patch(&json!({
            "scene": {"area": "广州市·越秀区·东山口"},
            "me": {"gx": 13.0, "gy": 4.0},
            "actors": {"小满": {"facility": "便利店"}},
            "events": [{"id": "traffic.jam", "category": "traffic", "at_secs": 10}],
            "weather": {"desc": "小雨", "temp_c": 22},
            "current_role": "小满",
            "cell_m": 30.0,
        }));
        let s = RuntimeSnapshot::from_runtime(&rt);
        assert!(s.has_scene());
        assert_eq!(s.current_role.as_deref(), Some("小满"));
        assert_eq!(s.events.len(), 1);
        assert!(s.weather.is_some());
        // 快照是只读的拷贝：改它不影响运行时
        let mut s2 = s.clone();
        s2.scene = None;
        assert!(rt.scene.is_some());
        assert!(!s2.has_scene());

        // 空 runtime：没有场景 = 世界模拟没开
        assert!(!RuntimeSnapshot::from_runtime(&MapRuntime::new()).has_scene());
    }
}
