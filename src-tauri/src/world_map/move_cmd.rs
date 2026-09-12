//! 移动状态机的 **Tauri 命令层 + 派发胶水**（P4-2 / P4-3 的接线）。
//!
//! 纯逻辑在 [`super::r#move`]，指令剥离在 [`super::directive`]；本文件只做三件事：
//! 1. **派发**：把 AI 的位置指令翻成一条行程（读 `MapRuntime` 快照 → 解析目的地 → 起行程）
//! 2. **落事件**：把"动身""到达"写进 `MapRuntime`（事件环形缓冲 + `pending_move`），
//!    这样下一轮的注入文本里「最近：…」就能接上
//! 3. **到点叫一次**：挂一个**一次性**定时器（不是每秒 tick）等到达；
//!    同时所有读接口都顺手做一次"按时间戳收口"，定时器被系统冻掉也不影响正确性
//!
//! ## 与 P3 的分工
//!
//! P3 的 `move_to` 工具只记 `pending_move` 意图、**不动 `actors`**（动画归前端）。
//! P4 也一样：后端只维护"这条行程现在应该在哪儿"，前端轮询
//! [`world_map_trip_status`] 拿到 `pos` 自己推进动画，再用 `world_map_update_runtime`
//! 把 `actors` 推回来 —— 后端不跟前端抢同一个字段。

use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use super::directive::Directive;
use super::r#move as trip;
use super::state::{self, MapRuntimeHandle};

/// 前端事件名：起程 / 到达 / 取消都推它（前端不监听也没有副作用）。
pub const EVENT_TRIP: &str = "world_map:trip";

/// 一次性定时器的单段上限：1 小时。
/// 不是"轮询间隔"——绝大多数行程一次就睡到点；只有跨小时的远途才会多醒一次，
/// 顺带把"睡的过程中系统时钟被改"这种情况收回来。
const TIMER_SEGMENT_MS: u64 = 3_600_000;

/// 当前 unix 秒（`MapRuntime` 的时间戳字段用的是秒）。
fn now_secs() -> u64 {
    trip::now_ms() / 1000
}

// ═══════════════════════════════════════════════════════════════════
//  派发：AI 位置指令 → 行程
// ═══════════════════════════════════════════════════════════════════

/// 位置指令派发入口（**由 `producer.rs` 在流结束后调用**）。
///
/// 一轮里出现多条时取**最后一条**（模型最后的表态才是它真正想去的地方，见
/// [`super::directive`] 的容错表）。返回真正起程的条数（0 或 1；抢不到读锁时会
/// 交给异步任务重试，此时也返回 0）。
pub fn dispatch_directives(app: &AppHandle, directives: &[Directive]) -> usize {
    let Some(last) = directives.last() else {
        return 0;
    };
    if directives.len() > 1 {
        tracing::warn!(
            "[world_map] 一轮回复里有 {} 条位置指令，只派发最后一条: to={}",
            directives.len(),
            last.to
        );
    }
    match read_snapshot() {
        Some(snapshot) => dispatch_with(app, last, &snapshot),
        None => {
            // 写锁被占（前端正在推 patch）：指令不能丢，挪到异步任务里重来一次。
            // 与 `state::record_move_intent` 的兜底同款（那边是 try_write 版本）。
            let app = app.clone();
            let last = last.clone();
            let runtime = state::shared();
            tauri::async_runtime::spawn(async move {
                let snapshot = {
                    let guard = runtime.read().await;
                    guard.to_json(now_secs())
                };
                dispatch_with(&app, &last, &snapshot);
            });
            0
        }
    }
}

/// 抢一次**读锁**拿 MapRuntime 快照；抢不到返回 `None`（不等待）。
///
/// 单独抽成函数是为了避开一个真实的借用坑（rustc 报 E0597，垫片编译时踩到过）：
/// 在函数末尾直接写 `match runtime.try_read() { … }` 时，`try_read()` 产生的
/// 临时 guard 会活过局部变量 `runtime` 本身。写成 `let locked = …;` 之后，
/// 析构顺序就变成"locked 先、runtime 后"，正确了。
fn read_snapshot() -> Option<Value> {
    let runtime = state::shared();
    let locked = runtime.try_read();
    match locked {
        Ok(guard) => Some(guard.to_json(now_secs())),
        Err(_) => None,
    }
}

/// 抢一次**写锁**改点东西；抢不到返回 `false`，由调用方决定怎么兜底。
fn try_write_runtime(f: impl FnOnce(&mut state::MapRuntime)) -> bool {
    let runtime = state::shared();
    let locked = runtime.try_write();
    match locked {
        Ok(mut guard) => {
            f(&mut guard);
            true
        }
        Err(_) => false,
    }
}

/// 拿着快照真正起一条行程（`dispatch_directives` 的同步内核）。
fn dispatch_with(app: &AppHandle, directive: &Directive, snapshot: &Value) -> usize {
    let role = snapshot
        .get("current_role")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if role.is_empty() {
        tracing::warn!(
            "[world_map] 收到位置指令但不知道是谁在移动（current_role 为空）: to={}",
            directive.to
        );
        return 0;
    }
    let now = trip::now_ms();
    match trip::dispatch(
        snapshot,
        &role,
        &directive.to,
        directive.kind.as_deref(),
        now,
        trip::speedup(),
    ) {
        Ok(t) => {
            start_and_publish(app, t);
            1
        }
        Err(e) => {
            // 目的地解析不出来（地图数据里没这个名字）是最常见的一种：
            // 只记日志，**不写事件** —— 别把一次失败的意图塞进 AI 的上下文。
            tracing::warn!(
                "[world_map] 位置指令派发失败（{} → {}）: {}",
                role,
                directive.to,
                e.zh()
            );
            0
        }
    }
}

/// 登记行程 → 写 `pending_move` + 事件 → 通知前端 → 挂到达定时器。
fn start_and_publish(app: &AppHandle, t: trip::Trip) {
    if let Some(old) = trip::start(t.clone()) {
        // 顶替掉的那条：只记日志（不写事件，避免"改道"把事件流刷成噪音）
        tracing::info!(
            "[world_map] {} 的新行程顶替了旧行程 #{}（去 {}）",
            t.role,
            old.id,
            old.to.name
        );
    }
    let view = t.view(trip::now_ms());
    let text = trip::move_text(&t.role, &t.to.name, t.kind, t.distance_m);
    let intent = json!({
        "target": t.to.name,
        "by": t.role,
        "kind": t.kind.key(),
        "kind_zh": t.kind.zh(),
        "at": now_secs(),
        "status": "moving",
        "source": "directive",
        "trip_id": t.id,
        "distance_m": (t.distance_m * 10.0).round() / 10.0,
        "eta_secs": (t.duration_secs() * 10.0).round() / 10.0,
    });
    let event = json!({
        "kind": "move",
        "text": text,
        "at": now_secs(),
        "trip_id": t.id,
        "role": t.role,
        "to": t.to.name,
        "by": t.kind.key(),
    });
    write_runtime(Some(intent), event);
    emit_trip(app, "start", &view);
    arm_arrival_timer(app, t.role.clone(), t.arrive_at_ms(trip::now_ms()));
}

/// 写运行时的两个字段（`pending_move` + 一条事件）。
///
/// 抢不到写锁就丢给异步任务重试 —— 与 `state::record_move_intent` 同一套兜底。
/// 起程/到达才走这条冷路径，clone 一份 JSON 换掉借用上的麻烦是划算的。
fn write_runtime(pending: Option<Value>, event: Value) {
    let pending2 = pending.clone();
    let event2 = event.clone();
    let ok = try_write_runtime(move |guard| {
        if let Some(intent) = pending2 {
            guard.pending_move = Some(intent);
        }
        guard.push_event(event2);
    });
    if ok {
        return;
    }
    let runtime = state::shared();
    tauri::async_runtime::spawn(async move {
        let mut guard = runtime.write().await;
        if let Some(intent) = pending {
            guard.pending_move = Some(intent);
        }
        guard.push_event(event);
    });
}

/// 到点收口 + 写事件（幂等：真正跨过终点的那一次才写）。
///
/// 返回本次真的到达的行程视图（命令层顺手回给前端）。
pub fn publish_arrivals(app: &AppHandle) -> Vec<Value> {
    let now = trip::now_ms();
    let arrived = trip::refresh_all(now);
    for t in &arrived {
        finish_arrival(app, t, now);
    }
    arrived.iter().map(|t| t.view(now)).collect()
}

/// 一条行程**真正到达**时的收口（事件 + 清意图 + 通知前端）。
///
/// 定时器那条路和轮询那条路都走它 —— 幂等由 `trip::refresh` 保证，
/// 所以"事件只写一条、`pending_move` 只清一次"。
fn finish_arrival(app: &AppHandle, t: &trip::Trip, now: u64) {
    let event = json!({
        "kind": "arrive",
        "text": trip::arrive_text(&t.role, &t.to.name),
        "at": now_secs(),
        "trip_id": t.id,
        "role": t.role,
        "to": t.to.name,
    });
    // 这条行程的 `pending_move` 意图已兑现，清掉（别的行程的不能误伤：
    // 按 trip_id 认领，认不出来就不动它）
    let mine = state::shared()
        .try_read()
        .ok()
        .and_then(|g| g.pending_move.clone())
        .and_then(|v| v.get("trip_id").and_then(Value::as_u64))
        == Some(t.id);
    let event2 = event.clone();
    let ok = try_write_runtime(move |guard| {
        if mine {
            guard.pending_move = None;
        }
        guard.push_event(event2);
    });
    if !ok {
        let runtime = state::shared();
        let event3 = event.clone();
        tauri::async_runtime::spawn(async move {
            let mut guard = runtime.write().await;
            if mine {
                guard.pending_move = None;
            }
            guard.push_event(event3);
        });
    }
    emit_trip(app, "arrived", &t.view(now));
}

/// 给前端推一条行程事件（前端不监听也无害）。
fn emit_trip(app: &AppHandle, phase: &str, view: &Value) {
    let payload = json!({ "phase": phase, "trip": view, "at": now_secs() });
    if let Err(e) = app.emit(EVENT_TRIP, &payload) {
        tracing::warn!("emit {EVENT_TRIP} 失败: {e}");
    }
}

/// 挂**一次性**到达定时器（手机会息屏/切后台，定时器只用来"到点叫一次"，
/// 正确性不依赖它 —— 每次读接口都会按时间戳重新收口）。
fn arm_arrival_timer(app: &AppHandle, role: String, at_ms: u64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let now = trip::now_ms();
            let remain = at_ms.saturating_sub(now);
            if remain == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(remain.min(TIMER_SEGMENT_MS))).await;
            // 行程被取消/顶替掉了就别再等了（不然任务会一直挂着）
            if trip::get(&role, trip::now_ms()).is_none() {
                return;
            }
            if at_ms.saturating_sub(trip::now_ms()) == 0 {
                break;
            }
        }
        let now = trip::now_ms();
        if let Some(t) = trip::refresh(&role, now) {
            // 与轮询那条路共用收口逻辑（事件 + 清 pending_move + 通知前端）
            finish_arrival(&app, &t, now);
        }
    });
}

/// 加速开关拨动后，给还在路上的行程重挂定时器（倍率变了，到点时刻也变了）。
fn rearm_timers(app: &AppHandle, now_ms: u64) -> Vec<Value> {
    let mut out = Vec::new();
    for (role, eta) in trip::moving_etas(now_ms) {
        arm_arrival_timer(app, role.clone(), eta);
        out.push(json!({ "role": role, "arrive_at_ms": eta }));
    }
    out
}

// ═══════════════════════════════════════════════════════════════════
//  Tauri 命令（注册时必须带 `world_map::move_cmd::` 前缀）
// ═══════════════════════════════════════════════════════════════════

/// `world_map_trip_status` —— 前端轮询行程状态。
///
/// 返回（**这是给前端代理的接口契约**）：
/// ```jsonc
/// {
///   "ok": true,
///   "now_ms": 1730000000000,
///   "speedup": 1.0,                  // 当前加速倍率（1 或 100）
///   "role": "小满",                   // 查询的角色（没传就是 current_role）
///   "active": { …行程视图… } | null, // 该角色当前的行程
///   "trips": [ { …行程视图… } ],      // 全部角色的行程
///   "arrived_now": [ … ]             // 本次调用刚好收口的行程（通常是空数组）
/// }
/// ```
/// 行程视图字段见 [`trip::Trip::view`]：
/// `id / role / status / kind / kind_zh / kind_explicit / space / from / to /
///  pos / distance_m / done_m / remaining_m / progress / speed_mps / speed_eff_mps /
///  speedup / duration_secs / eta_secs / eta_secs_eff / created_ms / depart_ms /
///  arrive_at_ms / eta_1x_ms / arrived_ms / cancelled_ms`。
/// `pos` 在 `space="grid"` 时是 `{gx,gy}`（小区地图直接用），`space="geo"` 时是 `{lng,lat}`。
#[tauri::command]
pub async fn world_map_trip_status(
    app: AppHandle,
    state: State<'_, MapRuntimeHandle>,
    role: Option<String>,
) -> Result<Value, String> {
    let now = trip::now_ms();
    let arrived_now = publish_arrivals(&app);
    let who = match role.clone() {
        Some(r) => Some(r),
        None => {
            let guard = state.0.read().await;
            guard.current_role.clone()
        }
    };
    Ok(json!({
        "ok": true,
        "now_ms": now,
        "speedup": trip::speedup(),
        "role": who,
        "active": who.as_deref().and_then(|r| trip::view_of(r, now)),
        "trips": trip::views(now),
        "arrived_now": arrived_now,
    }))
}

/// `world_map_trip_cancel` —— 取消行程（位置冻结在当前进度）。
///
/// 没传 `role` 就用运行时里的 `current_role`。没有进行中的行程时返回
/// `{ok:false, message:"…"}`（**不是** Err —— 前端重复调用不该报错）。
#[tauri::command]
pub async fn world_map_trip_cancel(
    app: AppHandle,
    state: State<'_, MapRuntimeHandle>,
    role: Option<String>,
    reason: Option<String>,
) -> Result<Value, String> {
    let who = match role {
        Some(r) => Some(r),
        None => {
            let guard = state.0.read().await;
            guard.current_role.clone()
        }
    };
    let Some(who) = who.filter(|r| !r.trim().is_empty()) else {
        return Err("不知道要取消谁的行程（role 未传且 current_role 为空）".into());
    };
    let now = trip::now_ms();
    let Some(t) = trip::cancel(&who, now) else {
        return Ok(json!({
            "ok": false,
            "role": who,
            "message": "这个角色没有进行中的行程",
            "trips": trip::views(now),
        }));
    };
    if let Some(r) = reason.as_deref().filter(|r| !r.trim().is_empty()) {
        tracing::info!("[world_map] 行程 #{} 被取消: {r}", t.id);
    }
    let event = json!({
        "kind": "move_cancel",
        "text": trip::cancel_text(&t.role, &t.to.name),
        "at": now_secs(),
        "trip_id": t.id,
        "role": t.role,
        "to": t.to.name,
    });
    let mine = state::shared()
        .try_read()
        .ok()
        .and_then(|g| g.pending_move.clone())
        .and_then(|v| v.get("trip_id").and_then(Value::as_u64))
        == Some(t.id);
    let event2 = event.clone();
    let ok = try_write_runtime(move |guard| {
        if mine {
            guard.pending_move = None;
        }
        guard.push_event(event2);
    });
    if !ok {
        let runtime = state::shared();
        let event3 = event.clone();
        tauri::async_runtime::spawn(async move {
            let mut guard = runtime.write().await;
            if mine {
                guard.pending_move = None;
            }
            guard.push_event(event3);
        });
    }
    let view = t.view(now);
    emit_trip(&app, "cancelled", &view);
    Ok(json!({ "ok": true, "role": t.role, "trip": view, "reason": reason }))
}

/// `world_map_trip_speedup` —— 100× 加速开关（读/写同一个命令）。
///
/// - 都不传：只读当前倍率
/// - `enabled: true/false`：切到 100× / 回到 1×
/// - `speedup: <数字>`：直接给倍率（夹到 `1.0..=1000.0`，非有限值按 1.0）
///
/// 拨动后会**重锚**进行中的行程（已走的路不算白走）并重挂到达定时器。
#[tauri::command]
pub async fn world_map_trip_speedup(
    app: AppHandle,
    speedup: Option<f64>,
    enabled: Option<bool>,
) -> Result<Value, String> {
    let before = trip::speedup();
    let target = match (speedup, enabled) {
        (Some(v), _) => v,
        (None, Some(true)) => trip::SPEEDUP_FAST,
        (None, Some(false)) => trip::SPEEDUP_OFF,
        (None, None) => before,
    };
    let now = trip::now_ms();
    let after = if speedup.is_some() || enabled.is_some() {
        trip::set_speedup(target, now)
    } else {
        before
    };
    let rearmed = if (after - before).abs() > f64::EPSILON {
        rearm_timers(&app, now)
    } else {
        Vec::new()
    };
    Ok(json!({
        "ok": true,
        "speedup": after,
        "changed": (after - before).abs() > f64::EPSILON,
        "rearmed": rearmed,
    }))
}

/// `world_map_trip_start` —— 直接开一条行程（调试 / 前端自己驱动 / 长途）。
///
/// 走 `Value` 而不是一堆平铺参数：一是与 `world_map_update_runtime(patch)` 同风格，
/// 二是参数名不受 Tauri 的 camelCase/snake_case 改名规则影响，前端不会踩坑。
///
/// ```jsonc
/// {
///   "role": "小满",            // 可选，缺省用 current_role
///   "to": "咖啡馆",            // 必填（地名）
///   "kind": "taxi",            // 可选：九种之一或中文别名；缺省/认不出 → 按距离自动选
///   "gx": 5, "gy": 5,          // 可选：目的地直接给格点（给了就不按名字找）
///   "lng": 113.3, "lat": 23.1, // 可选：目的地直接给经纬度
///   "cell_m": 30,              // 可选：格边长（默认 30）
///   "from": { "gx": 3, "gy": 4 },  // 可选：出发地（缺省取该角色当前在地图上的位置）
///   "depart_in_secs": 0,       // 可选：多久后出发（默认 0 = 立刻）
///   "speedup": 1               // 可选：这条行程的加速倍率（缺省用全局开关）
/// }
/// ```
#[tauri::command]
pub async fn world_map_trip_start(
    app: AppHandle,
    state: State<'_, MapRuntimeHandle>,
    req: Value,
) -> Result<Value, String> {
    let Some(obj) = req.as_object() else {
        return Err("req 必须是 JSON object".into());
    };
    let to = obj
        .get("to")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "req.to 必须是目的地名字".to_string())?
        .to_string();
    let snapshot = {
        let guard = state.0.read().await;
        guard.to_json(now_secs())
    };
    let role = obj
        .get("role")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            snapshot
                .get("current_role")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or_else(|| "不知道是谁在移动（req.role 未传且 current_role 为空）".to_string())?;

    let cell = obj.get("cell_m").and_then(Value::as_f64).unwrap_or(trip::DEFAULT_CELL_M);
    // 目的地：显式坐标优先，其次按名字在地图数据里找
    let mut dest = trip::Place::from_value(&to, &req, cell);
    if !dest.has_grid() && !dest.has_geo() {
        dest = trip::resolve_destination(&snapshot, &to, None)
            .ok_or_else(|| format!("地图数据里没有这个目的地：{to}"))?;
    }
    // 出发地：req.from 优先，其次角色当前所在
    let from = match obj.get("from").filter(|v| v.is_object()) {
        Some(v) => trip::Place::from_value(&role, v, cell),
        None => trip::resolve_origin(&snapshot, &role)
            .ok_or_else(|| format!("{role} 在地图上还没有位置（actors/me 都没有格点）"))?,
    };

    let kind = obj.get("kind").and_then(Value::as_str).and_then(trip::MoveKind::parse);
    let now = trip::now_ms();
    let depart_secs = trip::normalize_depart_secs(
        obj.get("depart_in_secs").and_then(Value::as_f64).unwrap_or(0.0),
    );
    let depart_ms = now + (depart_secs * 1000.0).round() as u64;
    let speedup = obj
        .get("speedup")
        .and_then(Value::as_f64)
        .map(trip::normalize_speedup)
        .unwrap_or_else(trip::speedup);

    let t = trip::Trip::plan(
        trip::next_id(),
        &role,
        from,
        dest,
        kind,
        speedup,
        now,
        depart_ms,
    )
    .map_err(|e| e.zh().to_string())?;
    let view = t.view(now);
    start_and_publish(&app, t);
    Ok(json!({ "ok": true, "trip": view }))
}
