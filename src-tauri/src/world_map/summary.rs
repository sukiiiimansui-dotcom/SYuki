//! 世界模拟 · 地图状态摘要 + 运行时字段合并（**纯函数层**，P3）
//!
//! ## 为什么单独拆一个文件
//! 注入点在 `GameRoleManager::sync_memories`，它是在 **`game_status` 锁内**被调用的
//! （`api/chat.rs` 拿 `gs.lock().await` → `gs.add_line(..).await` → `game_status.rs`
//! `refresh_memories` → `role_manager.rs::sync_memories`），所以那条路径上
//! **一个字节的网络都不许有，连 `await` 都不该出现**。
//!
//! 于是把「把运行时状态拼成一段中文摘要」这件事单独放在这里：
//! 本文件 **不 import tauri / tokio / reqwest / chrono**，输入全是
//! `serde_json::Value` 与标量。好处有两个：
//!   1. `state.rs` 的注入路径只剩「读锁 → 调本文件 → 写缓存」，不可能掺进 IO；
//!   2. 本文件可以**脱离整个工程单独 `rustc` 编译并跑单测**（文件末尾 `mod tests`），
//!      是这条链路里唯一能"跑起来看输出"的部分（Tauri 全量编译在本机被纪律禁止）。
//!
//! ## 数据契约（前端 `world_map_update_runtime` 推上来的形状）
//! ```jsonc
//! {
//!   "scene":   {"area": "广州市·越秀区·东山口", "place": "便利店", "adcode": "440104"},
//!   "me":      {"lat": 23.13, "lng": 113.29, "source": "gps", "area": "广州市·越秀区",
//!               "gx": 3.5, "gy": 4.0},
//!   "actors":  {"小满": {"facility": "便利店", "type": "commercial",
//!                        "x": 3, "y": 4, "since": "19:20"}},
//!   "events":  [{"text": "在便利店买了伞", "at": 1730000000}],
//!   "weather": {"desc": "小雨", "temp_c": 22, "city": "广州"},
//!   "facilities": [{"name": "咖啡馆", "type": "commercial", "grid": [6, 7]}],
//!   "cell_m": 30
//! }
//! ```
//! 所有字段都是**可选**的：缺什么就少哪一行，一个都没有时 `render` 返回空串
//! （调用方据此**不注入**，绝不塞空段落进 system 提示）。
//!
//! ## 注入格式（机主定稿，见 `docs/world-map/13-世界模拟最终方案.md`）
//! ```text
//! 【当前场景】
//! 你在：广州市·越秀区·东山口·便利店里
//! 用户位置：广州市·越秀区（约 300m）
//! 时间/天气：19:24（傍晚）· 小雨 22°C
//! 附近：咖啡馆(80m)、地铁站(200m)、公园(350m)
//! 最近：刚才在便利店买了伞
//! ```

use serde_json::Value;

/// `Value::Null` 的常量引用（`Option` 缺席时当占位，零成本）
const NULL_VALUE: Value = Value::Null;

/// 「附近」一行最多列几个设施（多了会把 system 提示撑长，收益又不大）
pub const NEARBY_LIMIT: usize = 3;

/// 天气快照有效期（秒）。取 30 分钟与 `live.rs::WEATHER_TTL_SECS` 同值：
/// 前端那次 `world_map_weather` 本身就按这个窗口命中缓存，两边判"过期"的口径一致，
/// 才不会出现「前端显示 22°C，注入里却因为没有天气而少一行」。
pub const WEATHER_TTL_SECS: u64 = 1800;

// ═══════════════════════════════════════════════════════════════════
//  小工具：宽容取值
// ═══════════════════════════════════════════════════════════════════

/// 取对象的字符串字段；空串按"没有"处理（前端常把没值的字段写成 `""`）。
pub fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 取数字字段；字符串数字也认（`world_map_weather` 的 `temp_C` 就是字符串）。
pub fn num_of(v: &Value, key: &str) -> Option<f64> {
    let raw = v.get(key)?;
    match raw {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// 行政链路文案：`{"path":[{"name":"广东省"},…]}` → `"广东省·广州市·越秀区"`。
///
/// 名字里已经带了 `·` 的（真源 `location_payload` 的 `area` 就是拼好的）直接返回，
/// 避免拼出 `"广州市·越秀区·广州市·越秀区"`。
fn path_label(v: &Value) -> Option<String> {
    let arr = v.get("path")?.as_array()?;
    let names: Vec<String> = arr
        .iter()
        .filter_map(|p| match p {
            Value::String(s) => Some(s.trim().to_string()),
            _ => str_of(p, "name"),
        })
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() {
        None
    } else {
        Some(names.join("·"))
    }
}

/// 场景文案：`area` 优先（已经是「市·区·小区」那种人话），否则由 `path` 拼。
pub fn scene_label(scene: &Value) -> Option<String> {
    str_of(scene, "area")
        .or_else(|| str_of(scene, "label"))
        .or_else(|| path_label(scene))
        .or_else(|| str_of(scene, "name"))
}

/// 玩家所在区域文案：`world_map_location` 的 `area` / `path` / `city` 都认。
pub fn me_label(me: &Value) -> Option<String> {
    str_of(me, "area")
        .or_else(|| path_label(me))
        .or_else(|| {
            let city = str_of(me, "city");
            let district = str_of(me, "district");
            match (city, district) {
                (Some(c), Some(d)) if !c.contains(&d) => Some(format!("{c}·{d}")),
                (Some(c), _) => Some(c),
                (None, d) => d,
            }
        })
        .or_else(|| str_of(me, "name"))
}

/// 格点坐标：`grid:[x,y]` / `pos:[x,y]` / 平铺的 `x`/`y` 三种都认
/// （`facilities.rs` 两种都出现过，前端拼的 actor 记录又是平铺的）。
pub fn cell_xy(v: &Value) -> (Option<f64>, Option<f64>) {
    for key in ["grid", "pos", "cell"] {
        if let Some(arr) = v.get(key).and_then(Value::as_array) {
            if arr.len() >= 2 {
                let x = arr[0].as_f64().or_else(|| num_of(&arr[0], "x"));
                let y = arr[1].as_f64().or_else(|| num_of(&arr[1], "y"));
                if x.is_some() && y.is_some() {
                    return (x, y);
                }
            }
        }
    }
    (num_of(v, "x").or_else(|| num_of(v, "gx")), num_of(v, "y").or_else(|| num_of(v, "gy")))
}

/// 经纬度：`{lat,lng}`（也认 `lon`）
pub fn lng_lat(v: &Value) -> (Option<f64>, Option<f64>) {
    (
        num_of(v, "lng").or_else(|| num_of(v, "lon")),
        num_of(v, "lat"),
    )
}

/// 两点距离（米）。
///
/// 优先**格点距离 × 格边长**：小区地图里「附近设施」本来就是按格摆的，
/// 拿格点算既精确又不需要经纬度（前端在小区层级未必有设施的真实坐标）。
/// 退化到经纬度时才用 haversine —— 与 `coord::haversine_m` 同一套公式
/// （这里不能 `use coord`，否则本文件就没法单独编译了；公式只有 6 行，照抄更省事）。
pub fn distance_m(
    a: (Option<f64>, Option<f64>),
    b: (Option<f64>, Option<f64>),
    cell_m: f64,
) -> Option<f64> {
    match (a, b) {
        ((Some(ax), Some(ay)), (Some(bx), Some(by))) if ax != bx || ay != by => {
            let cells = (ax - bx).hypot(ay - by);
            Some(cells * if cell_m > 0.0 { cell_m } else { 30.0 })
        }
        _ => None,
    }
}

/// 经纬度距离（米，haversine）；缺坐标返回 `None`。
pub fn geo_distance_m(a: (Option<f64>, Option<f64>), b: (Option<f64>, Option<f64>)) -> Option<f64> {
    let (Some(alng), Some(alat)) = a else { return None };
    let (Some(blng), Some(blat)) = b else { return None };
    const R: f64 = 6_371_008.8;
    let d2r = std::f64::consts::PI / 180.0;
    let p1 = alat * d2r;
    let p2 = blat * d2r;
    let dp = (blat - alat) * d2r;
    let dl = (blng - alng) * d2r;
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    Some(2.0 * R * h.sqrt().min(1.0).asin())
}

/// 距离文案：`80m` / `350m` / `1.2km`（跟文档示例一个口径）
pub fn fmt_dist(m: f64) -> String {
    if m < 20.0 {
        format!("{:.0}m", m.max(0.0))
    } else if m < 1000.0 {
        format!("{}m", ((m / 10.0).round() as i64) * 10)
    } else {
        let km = (m / 100.0).round() / 10.0;
        if (km.fract()).abs() < f64::EPSILON {
            format!("{}km", km as i64)
        } else {
            format!("{km:.1}km")
        }
    }
}

/// 中文时段。
///
/// 分界线按**机主给的示例**反推：示例是 `19:24（傍晚）`，而工程里既有的
/// `world_map/mod.rs::period_of` 把 19 点算作 `evening`（晚上）—— 照抄它对不上示例，
/// 所以这里自建一张中文表，并把 17..=19 划进「傍晚」。表比 `period_of` 少一档
/// （没有 dawn/night 的英文键），因为注入文本是给人看的，不需要机读 key。
pub fn zh_period(hour: u32) -> &'static str {
    match hour {
        0..=4 => "深夜",
        5..=7 => "清晨",
        8..=10 => "上午",
        11..=12 => "中午",
        13..=16 => "下午",
        17..=19 => "傍晚",
        20..=22 => "晚上",
        _ => "深夜",
    }
}

/// 设施名前加「里」：`便利店` → `便利店里`。
///
/// 只在**设施名**上用（小区名不加：`东山口里` 是错的）。
/// 已经带方位词的不重复加（`家里` / `车内` / `楼上`），
/// 也不给「码头 / 机场 / 桥」这类开敞地名加（`码头里` 读着别扭）。
fn with_inside(name: &str) -> String {
    let open_ended = ["码头", "机场", "桥", "路口", "广场", "海滩", "山顶", "门口"];
    let has_suffix = ["里", "内", "中", "上", "外", "旁", "边", "口"];
    if has_suffix.iter().any(|s| name.ends_with(s)) || open_ended.iter().any(|s| name.ends_with(s)) {
        name.to_string()
    } else {
        format!("{name}里")
    }
}

// ═══════════════════════════════════════════════════════════════════
//  角色位置 / 附近设施 / 最近事件
// ═══════════════════════════════════════════════════════════════════

/// 一个角色在地图上的落脚点（`actors` 里的一条）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActorPos {
    /// 设施名（`facility` / `place` / `name` 都认）
    pub place: Option<String>,
    /// 设施类型 key（`commercial` / `medical` …），用于按类型过滤
    pub kind: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    /// 落位时刻（前端给的人话，如 `"19:20"`）
    pub since: Option<String>,
}

/// 在 `actors` 里找某个角色的记录。
///
/// 先精确匹配键，再退一步做「去掉首尾空白 + 忽略 ASCII 大小写」的匹配：
/// 注入侧用的键是角色的 `display_name`（= `settings.yml` 的 `ai_name`），
/// 前端拼 `actors` 时多一个空格、少一个大写，不该让整行「·便利店」凭空消失。
/// 找不到就返回 `None`（**不做"只有一个 actor 就当他是"的猜测**：
/// 那在多人场景里会把别人的位置安到当前角色头上，比缺一行糟得多）。
pub fn actor_record<'a>(actors: &'a Value, name: &str) -> Option<&'a Value> {
    if let Some(v) = actors.get(name) {
        if !v.is_null() {
            return Some(v);
        }
    }
    let target = name.trim();
    if target.is_empty() {
        return None;
    }
    actors
        .as_object()?
        .iter()
        .find(|(k, v)| k.trim().eq_ignore_ascii_case(target) && !v.is_null())
        .map(|(_, v)| v)
}

/// 取角色位置。`actors` 的形状：`{"角色名": {facility,type,x,y,since}}`。
pub fn actor_pos(actors: &Value, name: &str) -> ActorPos {
    let Some(rec) = actor_record(actors, name) else {
        return ActorPos::default();
    };
    let (x, y) = cell_xy(rec);
    ActorPos {
        place: str_of(rec, "facility")
            .or_else(|| str_of(rec, "place"))
            .or_else(|| str_of(rec, "name")),
        kind: str_of(rec, "type").or_else(|| str_of(rec, "kind")),
        x,
        y,
        since: str_of(rec, "since")
            .or_else(|| rec.get("since_ts").and_then(Value::as_i64).map(|t| t.to_string())),
    }
}

/// 一条「附近设施」结果
#[derive(Debug, Clone, PartialEq)]
pub struct NearbyItem {
    pub name: String,
    pub kind: String,
    pub dist_m: f64,
}

/// 附近设施：按距离升序取前 `limit` 个。
///
/// * `exclude` —— 角色**当前所在**的那个设施名，要从列表里剔掉
///   （否则「附近」第一项永远是「便利店(0m)」，白占一行）；
/// * 同名设施只留最近的一个（`facilities.rs` 会生成多个「住宅楼」）；
/// * 距离口径：格点 × `cell_m`；没有格点时退化成"跳过"（宁缺毋滥，不猜）。
pub fn nearby(
    facilities: &Value,
    from: (Option<f64>, Option<f64>),
    exclude: Option<&str>,
    cell_m: f64,
    limit: usize,
) -> Vec<NearbyItem> {
    let Some((fx, fy)) = (match from {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    }) else {
        return Vec::new();
    };
    let cell_m = if cell_m > 0.0 { cell_m } else { 30.0 };
    let list = facilities.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut out: Vec<NearbyItem> = Vec::new();
    for f in list {
        let name = str_of(f, "name").unwrap_or_default();
        if name.is_empty() || exclude.is_some_and(|e| e == name) {
            continue;
        }
        let kind = str_of(f, "type").or_else(|| str_of(f, "kind")).unwrap_or_default();
        let (x, y) = cell_xy(f);
        let (Some(x), Some(y)) = (x, y) else { continue };
        // 优先用**设施自己记录**的格边长：`facilities.rs::mk_fac` 每条都带
        // `cell_meters`（生成时用的那套），比调用方给的全局值更准。
        let m = num_of(f, "cell_meters").filter(|m| *m > 0.0).unwrap_or(cell_m);
        let dist = (x - fx).hypot(y - fy) * m;
        if let Some(slot) = out.iter_mut().find(|i| i.name == name) {
            if dist < slot.dist_m {
                slot.dist_m = dist;
            }
        } else {
            out.push(NearbyItem { name, kind, dist_m: dist });
        }
    }
    out.sort_by(|a, b| a.dist_m.partial_cmp(&b.dist_m).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(limit);
    out
}

/// 最近一条事件的人话文案。
///
/// 事件是前端（或工具 `move_to`）推进来的自由 JSON，字段名不统一，
/// 所以 `text` / `summary` / `title` / `desc` / `content` 挨个认。
/// 没有"刚才/刚刚"这类时间词时补一个 —— 注入是给模型看的当下时态，
/// 「在便利店买了伞」不如「刚才在便利店买了伞」明确。
pub fn last_event(events: &[Value]) -> Option<String> {
    let raw = events.iter().rev().find_map(|e| {
        if let Value::String(s) = e {
            let t = s.trim();
            return (!t.is_empty()).then(|| t.to_string());
        }
        ["text", "summary", "title", "desc", "content"]
            .iter()
            .find_map(|k| str_of(e, k))
    })?;
    let text: String = raw.chars().take(60).collect();
    let has_time_word = ["刚才", "刚刚", "刚", "现在", "正在", "已经", "昨天", "今天", "早上", "中午", "下午", "晚上"]
        .iter()
        .any(|w| text.starts_with(w));
    Some(if has_time_word { text } else { format!("刚才{text}") })
}

// ═══════════════════════════════════════════════════════════════════
//  摘要拼装
// ═══════════════════════════════════════════════════════════════════

/// `render` 的入参（全是借来的引用，避免在锁内做无谓的 clone）。
pub struct SummaryInput<'a> {
    pub scene: &'a Value,
    pub me: &'a Value,
    pub actors: &'a Value,
    pub events: &'a [Value],
    pub facilities: &'a Value,
    pub cell_m: f64,
    /// 已抓好的天气快照（**必须**是缓存，不许在注入路径上现抓）
    pub weather: Option<&'a Value>,
    /// 这次注入是给哪个角色的（`actors` 的键）
    pub role: &'a str,
    /// `"19:24"` 形式的本机时间（由调用方取，本文件不引 chrono）
    pub hhmm: &'a str,
    /// 0..=23，用来算中文时段
    pub hour: u32,
}

/// 拼出注入文本；**没有场景时返回空串**（调用方据此跳过注入）。
///
/// 「有场景才注入」这条闸门是故意的：世界模拟没开的时候，前端从不会推 `scene`，
/// 这时 runtime 里最多只有一个孤零零的 `me`（玩家定位本来就是别的功能在用的），
/// 若照注入就会给所有角色的 system 提示塞一行「用户位置：…」——
/// 那属于**凭空改变原有对话行为**，是 K53 底线里不允许的。
pub fn render(inp: &SummaryInput<'_>) -> String {
    let Some(scene) = scene_label(inp.scene) else {
        return String::new();
    };
    let actor = actor_pos(inp.actors, inp.role);
    let (ax, ay) = (actor.x, actor.y);
    let (mx, my) = cell_xy(inp.me);
    let (mlng, mlat) = lng_lat(inp.me);
    let (alng, alat) = lng_lat(actor_record(inp.actors, inp.role).unwrap_or(&NULL_VALUE));

    let mut out = String::from("【当前场景】");

    // 你在：场景路径（·所在设施）
    let mut you = scene.clone();
    if let Some(place) = actor.place.as_deref().filter(|s| !s.is_empty()) {
        you.push('·');
        you.push_str(&with_inside(place));
    }
    out.push_str("\n你在：");
    out.push_str(&you);

    // 用户位置（+ 距离）
    if let Some(area) = me_label(inp.me) {
        out.push_str(&format!("\n用户位置：{area}"));
        // 距离优先按格点算，其次按经纬度；两边都没有就算不出来 —— 宁可不写"约 0m"
        let dist = distance_m((ax, ay), (mx, my), inp.cell_m)
            .or_else(|| geo_distance_m((alng, alat), (mlng, mlat)));
        if let Some(d) = dist {
            out.push_str(&format!("（约 {}）", fmt_dist(d)));
        }
    }

    // 时间 / 天气（天气过期就只写时间）
    out.push_str(&format!(
        "\n时间/天气：{}（{}）",
        inp.hhmm,
        zh_period(inp.hour)
    ));
    if let Some(w) = inp.weather {
        if w.get("error").is_none() {
            let desc = str_of(w, "desc").or_else(|| str_of(w, "desc_en"));
            let temp = num_of(w, "temp_c").map(|t| t.round() as i64);
            match (desc, temp) {
                (Some(d), Some(t)) => out.push_str(&format!("· {d} {t}°C")),
                (Some(d), None) => out.push_str(&format!("· {d}")),
                (None, Some(t)) => out.push_str(&format!("· {t}°C")),
                (None, None) => {}
            }
        }
    }

    // 附近（以角色自己的位置为准；角色还没落位就退到玩家位置）
    let origin = if ax.is_some() && ay.is_some() {
        (ax, ay)
    } else {
        (mx, my)
    };
    let items = nearby(
        inp.facilities,
        origin,
        actor.place.as_deref(),
        inp.cell_m,
        NEARBY_LIMIT,
    );
    if !items.is_empty() {
        out.push_str("\n附近：");
        out.push_str(
            &items
                .iter()
                .map(|i| format!("{}({})", i.name, fmt_dist(i.dist_m)))
                .collect::<Vec<_>>()
                .join("、"),
        );
    }

    // 最近（一条就够 —— 注入是"当下感"，不是事件日志）
    if let Some(ev) = last_event(inp.events) {
        out.push_str("\n最近：");
        out.push_str(&ev);
    }

    out
}

// ═══════════════════════════════════════════════════════════════════
//  待写记忆 → 注入块（P5-3）
// ═══════════════════════════════════════════════════════════════════

/// 一次注入最多带几行「待写记忆」。
///
/// 为什么有上限：这段文本进的是 system prompt，每一行都在跟「附近」「最近」
/// 抢模型的注意力；五行已经足够说清"刚才发生了什么"，再多就成事件日志了。
/// 超出的行**留在队列里**（下一轮注入接着写），不是丢掉。
pub const MEMORY_LINES_MAX: usize = 5;

/// 把事件引擎攒下的 `旁白: …` 行拼成注入文本里的一块（**纯函数**）。
///
/// 返回 `(块文本, 已交付的下标)`：
///   · 没有可注入的行 → `(String::new(), vec![])` —— 调用方 `push_str` 上去
///     **一个字节都不多**（世界模拟没跑、或队列空时，注入文本与 P5-2 之前逐字相同）；
///   · 第二个分量是给调用方**消费队列**用的：这些行的内容**已经在这一轮注入里
///     交付给模型了**，留在队列里下一轮就会重复注入。
///     它 = 渲染出来的那几行 ∪ 与「最近：…」重复而没渲染的行 ∪ 队列内重复的行
///     ∪ 空白行。**没被交付的行一个都不会出现**在这种下标里（没渲染又没被
///     「最近」覆盖的行必须留着，下一轮接着讲）。
///
/// 三条规则（顺序即优先级）：
///   1. **去重**（[`memory_key`]）：与「最近：…」那一行同一件事不写第二遍，
///      队列内部重复的行也只留第一条 —— 同一句话在这一轮注入里最多出现一次；
///   2. **上限** `limit`：从**最旧的一行**开始取（FIFO）—— 攒了一堆时按发生顺序
///      逐轮讲完，语序不会乱，也不会漏掉任何一行（每一轮都消费掉交付过的那几行）；
///   3. 空白行直接跳过（不占名额，也只能被消费掉 —— 留着永远不会被渲染）。
///
/// 为什么不是"取最新的几行"：注入是**当下感**，队列里最新的几行当然更"当下"，
/// 但那样攒下来的旧行会永远轮不到（新事件一直插队），最后被 `PENDING_MEMORY_MAX`
/// 当成垃圾丢掉 —— 角色的经历就这么静默消失了。FIFO 慢一点，但一句不落。
pub fn memory_block(lines: &[&str], recent: Option<&str>, limit: usize) -> (String, Vec<usize>) {
    if lines.is_empty() {
        return (String::new(), Vec::new());
    }
    let recent_key = recent.map(memory_key);
    let mut picked: Vec<usize> = Vec::new();
    let mut delivered: Vec<usize> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        let key = memory_key(raw);
        // 空白行：渲染不了，也没有任何信息 —— 直接算"处理过了"（消费掉，别占名额）
        if key.is_empty() {
            delivered.push(i);
            continue;
        }
        // ① 与「最近：…」重复：内容已经在注入里了 → 交付，不渲染
        if recent_key.as_deref() == Some(key.as_str()) {
            delivered.push(i);
            continue;
        }
        // ② 队列内部重复：第一条已经交付过 → 这一条也算交付
        if seen.iter().any(|s| *s == key) {
            delivered.push(i);
            continue;
        }
        if limit == 0 || picked.len() >= limit {
            // 到上限了：这一行**没**交付，留在队列里下一轮再讲
            continue;
        }
        seen.push(key);
        picked.push(i);
        delivered.push(i);
    }
    if picked.is_empty() {
        return (String::new(), delivered);
    }
    let mut out = String::from("\n记忆：");
    out.push_str(
        &picked
            .iter()
            .map(|&i| lines[i].trim())
            .collect::<Vec<_>>()
            .join("；"),
    );
    (out, delivered)
}

/// 「同一句话」的判据（去重用）。
///
/// 两条通路描述同一件事时，文字并不逐字相同：
///   · `events::memory_line` → `旁白: 在便利店买了伞`（待写记忆那一路）
///   · `summary::last_event` → `刚才在便利店买了伞`（「最近：…」那一路，会补时间词）
/// 所以这里统一剥掉 `旁白` 前缀、开头的时间词，以及全部空白与句读，
/// 剩下的当钥匙比 —— 相同就是同一件事。
///
/// 宁可漏（判不出重复，多说一遍）也不要错杀：所以只剥**开头**的时间词，
/// 不碰正文里的字。
fn memory_key(s: &str) -> String {
    let mut t = s.trim();
    if let Some(rest) = t.strip_prefix("旁白") {
        t = rest.trim_start_matches([':', '：', ' ', '\u{3000}']);
    }
    for w in [
        "刚才", "刚刚", "刚", "现在", "正在", "已经", "昨天", "今天", "早上", "中午",
        "下午", "晚上",
    ] {
        if let Some(rest) = t.strip_prefix(w) {
            t = rest;
            break;
        }
    }
    t.chars()
        .filter(|c| !c.is_whitespace() && !"。，、！？；：,.!?;:".contains(*c))
        .collect()
}

/// 注入指纹（FNV-1a 64）。**直接对拼好的文本取指纹**：
/// 文本没变 = 什么都没变，比维护一份"参与字段清单"（漏一个字段就静默不更新）稳得多。
///
/// 用途是「变化时才更新」（机主定的 H38）：指纹相同就复用上一次的文本，
/// 位置/天气没动的那些轮次不会重新拼字符串；文本稳定也意味着
/// 供应商侧的 prompt 前缀缓存更容易命中。
pub fn fingerprint(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ═══════════════════════════════════════════════════════════════════
//  字段级合并（world_map_update_runtime 的纯逻辑）
// ═══════════════════════════════════════════════════════════════════

/// 往 `slot`（可能还是 `Value::Null`）里**逐字段**合并一个 patch 对象。
///
/// 为什么是"字段级"而不是整体替换：前端一次只知道自己变化的那一小块
/// （比如只更新了玩家坐标），整体替换会把上一次推的场景/设施全抹掉。
/// 返回是否真的改动了（调用方据此决定要不要作废注入缓存）。
pub fn merge_object_slot(slot: &mut Value, patch: &Value) -> bool {
    let Some(src) = patch.as_object() else {
        return false;
    };
    if !slot.is_object() {
        *slot = Value::Object(serde_json::Map::new());
    }
    let Some(dst) = slot.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    for (k, v) in src {
        if dst.get(k) != Some(v) {
            dst.insert(k.clone(), v.clone());
            changed = true;
        }
    }
    changed
}

/// `actors` 合并：`{"小满": {...}}` 按角色逐个字段合并；`{"小满": null}` 删除该角色。
pub fn merge_actors(actors: &mut Value, patch: &Value) -> bool {
    let Some(src) = patch.as_object() else {
        return false;
    };
    if !actors.is_object() {
        *actors = Value::Object(serde_json::Map::new());
    }
    let Some(dst) = actors.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    for (name, rec) in src {
        if rec.is_null() {
            changed |= dst.remove(name).is_some();
            continue;
        }
        match dst.get_mut(name) {
            Some(existing) if existing.is_object() => {
                changed |= merge_object_slot(existing, rec);
            }
            _ => {
                dst.insert(name.clone(), rec.clone());
                changed = true;
            }
        }
    }
    changed
}

/// 事件环形缓冲：追加 + 只留最近 `cap` 条。
///
/// `null` 条目直接丢掉：前端数组里混进 `null`（比如 `map(...)` 里有空项）时
/// 不该在「最近」那一行变出一个空事件，也不该占环形缓冲的名额。
pub fn push_events(events: &mut Vec<Value>, incoming: &[Value], cap: usize) -> bool {
    let fresh: Vec<&Value> = incoming.iter().filter(|v| !v.is_null()).collect();
    if fresh.is_empty() {
        return false;
    }
    events.extend(fresh.into_iter().cloned());
    if events.len() > cap {
        let drop_n = events.len() - cap;
        events.drain(0..drop_n);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input<'a>(
        scene: &'a Value,
        me: &'a Value,
        actors: &'a Value,
        events: &'a [Value],
        facilities: &'a Value,
        weather: Option<&'a Value>,
    ) -> SummaryInput<'a> {
        SummaryInput {
            scene,
            me,
            actors,
            events,
            facilities,
            cell_m: 30.0,
            weather,
            role: "小满",
            hhmm: "19:24",
            hour: 19,
        }
    }

    /// 机主给的示例格式必须能一字不差地拼出来。
    #[test]
    fn renders_the_agreed_format() {
        let scene = json!({"area": "广州市·越秀区·东山口"});
        let me = json!({"area": "广州市·越秀区", "gx": 13.0, "gy": 4.0});
        let actors = json!({"小满": {"facility": "便利店", "type": "commercial", "x": 3.0, "y": 4.0, "since": "19:20"}});
        let facilities = json!([
            {"name": "便利店", "type": "commercial", "grid": [3, 4]},
            {"name": "咖啡馆", "type": "commercial", "grid": [5, 5]},
            {"name": "地铁站", "type": "transit", "grid": [9, 6]},
            {"name": "公园", "type": "leisure", "grid": [13, 5]},
        ]);
        let events = vec![json!({"text": "在便利店买了伞"})];
        let weather = json!({"desc": "小雨", "temp_c": 22});
        let text = render(&input(&scene, &me, &actors, &events, &facilities, Some(&weather)));
        // 咖啡馆：格点 (5,5)-(3,4) → √5 格 ≈ 2.236×30 ≈ 67m → 就近取整 70m
        // 地铁站：(9,6)-(3,4) → √40 ≈ 6.32×30 ≈ 190m → 190m
        // 公园：(13,5)-(3,4) → √101 ≈ 10.05×30 ≈ 301m → 300m
        assert_eq!(
            text,
            "【当前场景】\n\
             你在：广州市·越秀区·东山口·便利店里\n\
             用户位置：广州市·越秀区（约 300m）\n\
             时间/天气：19:24（傍晚）· 小雨 22°C\n\
             附近：咖啡馆(70m)、地铁站(190m)、公园(300m)\n\
             最近：刚才在便利店买了伞"
        );
    }

    /// 世界模拟没开（runtime 全空）→ 必须**一个字都不注入**。
    #[test]
    fn empty_runtime_renders_nothing() {
        let null = Value::Null;
        let text = render(&input(&null, &null, &null, &[], &null, None));
        assert!(text.is_empty(), "空 runtime 不该产出注入文本: {text:?}");
    }

    /// 只有玩家位置、没有场景 → 还是不注入（避免凭空改变原有对话行为）。
    #[test]
    fn me_without_scene_renders_nothing() {
        let null = Value::Null;
        let me = json!({"area": "广州市·越秀区", "lat": 23.1, "lng": 113.2});
        assert!(render(&input(&null, &me, &null, &[], &null, None)).is_empty());
    }

    /// 只填了场景 → 只出「你在」+ 时间两行。
    #[test]
    fn scene_only_keeps_two_lines() {
        let scene = json!({"area": "广州市·越秀区"});
        let null = Value::Null;
        let text = render(&input(&scene, &null, &null, &[], &null, None));
        assert_eq!(text, "【当前场景】\n你在：广州市·越秀区\n时间/天气：19:24（傍晚）");
    }

    /// 天气带 error（`world_map_weather` 失败时就是这形状）不能进注入。
    #[test]
    fn failed_weather_is_skipped() {
        let scene = json!({"area": "广州市"});
        let null = Value::Null;
        let w = json!({"error": "天气请求失败", "city": "广州"});
        let text = render(&input(&scene, &null, &null, &[], &null, Some(&w)));
        assert!(text.contains("时间/天气：19:24（傍晚）"));
        assert!(!text.contains("天气请求失败"));
        assert!(!text.contains('·'), "没有天气时不该出现天气分隔符: {text}");
    }

    /// 指纹：同文本同指纹，变一个字就变。
    #[test]
    fn fingerprint_is_stable_and_sensitive() {
        assert_eq!(fingerprint("abc"), fingerprint("abc"));
        assert_ne!(fingerprint("abc"), fingerprint("abd"));
        assert_ne!(fingerprint(""), fingerprint(" "));
    }

    /// 字段级合并：只动 patch 里的键，其余原样保留。
    #[test]
    fn merge_object_slot_is_field_level() {
        let mut slot = json!({"area": "广州市", "place": "便利店", "adcode": "440100"});
        let changed = merge_object_slot(&mut slot, &json!({"place": "咖啡馆"}));
        assert!(changed);
        assert_eq!(slot["area"], json!("广州市"));
        assert_eq!(slot["place"], json!("咖啡馆"));
        assert_eq!(slot["adcode"], json!("440100"));
        assert!(!merge_object_slot(&mut slot, &json!({"place": "咖啡馆"})));
    }

    /// actors：按角色合并、null 删人、没记录的角色直接落位。
    #[test]
    fn merge_actors_merges_per_role() {
        let mut actors = json!({"小满": {"facility": "便利店", "x": 3.0, "y": 4.0, "since": "19:20"}});
        assert!(merge_actors(&mut actors, &json!({"小满": {"x": 5.0}, "阿离": {"facility": "公园"}})));
        assert_eq!(actors["小满"]["facility"], json!("便利店"));
        assert_eq!(actors["小满"]["x"], json!(5.0));
        assert_eq!(actors["阿离"]["facility"], json!("公园"));
        assert!(merge_actors(&mut actors, &json!({"阿离": null})));
        assert!(actors.get("阿离").is_none());
    }

    /// 事件环形缓冲：超上限丢最老的，空数组/null 不算变化。
    #[test]
    fn events_ring_buffer_drops_oldest() {
        let mut ev = vec![json!({"text": "a"})];
        assert!(push_events(&mut ev, &[json!({"text": "b"}), json!({"text": "c"})], 2));
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0]["text"], json!("b"));
        assert!(!push_events(&mut ev, &[], 20));
        // 只有 null 的数组同样不算变化，也不能把 null 塞进缓冲
        assert!(!push_events(&mut ev, &[Value::Null], 20));
        assert_eq!(ev.len(), 2);
        assert!(last_event(&[Value::Null]).is_none());
        assert!(last_event(&[]).is_none());
    }

    /// 距离文案与"里"后缀的边界。
    #[test]
    fn formatting_edges() {
        assert_eq!(fmt_dist(8.0), "8m");
        assert_eq!(fmt_dist(84.0), "80m");
        assert_eq!(fmt_dist(96.0), "100m");
        assert_eq!(fmt_dist(1240.0), "1.2km");
        assert_eq!(fmt_dist(2000.0), "2km");
        assert_eq!(with_inside("便利店"), "便利店里");
        assert_eq!(with_inside("公园"), "公园里");
        assert_eq!(with_inside("码头"), "码头");
        assert_eq!(with_inside("家里"), "家里");
    }

    /// 角色定位：精确键优先，容忍首尾空白/大小写；找不到就是找不到（不猜）。
    #[test]
    fn actor_lookup_tolerates_padding_but_never_guesses() {
        let actors = json!({"小满": {"facility": "便利店"}, " Alice ": {"facility": "咖啡馆"}});
        assert_eq!(actor_pos(&actors, "小满").place.as_deref(), Some("便利店"));
        assert_eq!(actor_pos(&actors, "Alice").place.as_deref(), Some("咖啡馆"));
        assert_eq!(actor_pos(&actors, "alice").place.as_deref(), Some("咖啡馆"));
        // 只有别人在场时，绝不能把别人的位置安到当前角色头上
        assert_eq!(actor_pos(&actors, "阿离").place, None);
        assert_eq!(actor_pos(&json!({}), "小满").place, None);
        // 值为 null 的键视为"角色不在场"
        assert_eq!(actor_pos(&json!({"小满": null}), "小满").place, None);
    }

    /// 时段表按机主示例校准（19 点=傍晚）。
    #[test]
    fn periods_match_the_example() {
        assert_eq!(zh_period(19), "傍晚");
        assert_eq!(zh_period(17), "傍晚");
        assert_eq!(zh_period(20), "晚上");
        assert_eq!(zh_period(9), "上午");
        assert_eq!(zh_period(2), "深夜");
    }

    // ══════════════════════════════════════════════════════════════
    //  P5-3：待写记忆 → 注入块
    // ══════════════════════════════════════════════════════════════

    /// 没有待写记忆 → 空块 + 空下标（调用方拼上去一个字节都不多）。
    #[test]
    fn memory_block_is_empty_when_there_is_nothing_to_write() {
        assert_eq!(memory_block(&[], None, 5), (String::new(), Vec::new()));
        assert_eq!(memory_block(&[], Some("刚才在便利店买了伞"), 5), (String::new(), Vec::new()));
        // 空白行渲染不出东西，但也不该留在队列里烂着 → 算"处理过了"
        assert_eq!(memory_block(&["   ", "\t"], None, 5), (String::new(), vec![0, 1]));
    }

    /// 按发生顺序渲染，一次最多 limit 行；**没渲染的不许算交付**（否则那几行就丢了）。
    #[test]
    fn memory_block_renders_in_order_and_never_drops_the_backlog() {
        let lines = ["旁白: 一", "旁白: 二", "旁白: 三", "旁白: 四", "旁白: 五", "旁白: 六", "旁白: 七"];
        let (block, delivered) = memory_block(&lines, None, 5);
        assert_eq!(block, "\n记忆：旁白: 一；旁白: 二；旁白: 三；旁白: 四；旁白: 五");
        assert_eq!(delivered, vec![0, 1, 2, 3, 4], "只交付渲染出来的那几行");
        // 剩下的下一轮接着讲（FIFO，不插队）
        let (block2, delivered2) = memory_block(&lines[5..], None, 5);
        assert_eq!(block2, "\n记忆：旁白: 六；旁白: 七");
        assert_eq!(delivered2, vec![0, 1]);
        // limit = 0：一行都不渲染，也不交付（调用方会原样留着）
        assert_eq!(memory_block(&lines, None, 0).0, "");
        assert!(memory_block(&lines, None, 0).1.is_empty());
    }

    /// 与「最近：…」同一件事 → 不写第二遍，但**算已交付**（内容已经在注入里了）。
    #[test]
    fn memory_block_dedupes_against_the_recent_line() {
        let lines = ["旁白: 在便利店买了伞", "旁白: 遇到阿离"];
        let (block, delivered) = memory_block(&lines, Some("刚才在便利店买了伞"), 5);
        assert_eq!(block, "\n记忆：旁白: 遇到阿离");
        assert_eq!(delivered, vec![0, 1], "被「最近」覆盖的那行也算交付");
        // 全部都被覆盖 → 空块，但两行都算交付（不然它们会永远赖在队列里）
        let (block2, delivered2) = memory_block(&lines[..1], Some("刚才在便利店买了伞"), 5);
        assert_eq!(block2, "");
        assert_eq!(delivered2, vec![0]);
    }

    /// 队列内部重复：只渲染第一条，其余算交付（同一句话不在一轮里说两遍）。
    #[test]
    fn memory_block_dedupes_within_the_queue() {
        let lines = ["旁白: 同一件事", "旁白: 另一件", "旁白: 同一件事"];
        let (block, delivered) = memory_block(&lines, None, 5);
        assert_eq!(block, "\n记忆：旁白: 同一件事；旁白: 另一件");
        assert_eq!(delivered, vec![0, 1, 2]);
        // 重复项**不占** limit 名额：下面三条里有一条是重复的，五条上限下应全渲染
        let lines2 = ["旁白: a", "旁白: a", "旁白: b"];
        let (block2, _) = memory_block(&lines2, None, 2);
        assert_eq!(block2, "\n记忆：旁白: a；旁白: b");
    }

    /// 判据归一：`旁白:` 前缀、开头时间词、空白与句读都不影响"是不是同一句话"。
    #[test]
    fn memory_key_ignores_prefix_timewords_and_punctuation() {
        let base = memory_key("旁白: 在便利店买了伞");
        assert_eq!(base, "在便利店买了伞");
        for variant in [
            "旁白：在便利店买了伞",
            "旁白 在便利店买了伞",
            "刚才在便利店买了伞",
            "刚刚在便利店买了伞",
            "在便利店买了伞。",
            " 在便利店，买了伞 ",
        ] {
            assert_eq!(memory_key(variant), base, "{variant} 应判为同一句话");
        }
        // 不同的两件事绝不能被判成同一件（错杀 = 丢记忆）
        assert_ne!(memory_key("旁白: 在便利店买了伞"), memory_key("旁白: 在咖啡馆买了伞"));
        assert_ne!(memory_key("旁白: 去了公园"), memory_key("旁白: 去了公园门口"));
        assert_eq!(memory_key("   "), "");
    }

    /// 同一条「最近」行与队列混合时：该渲染的渲染、该去重的去重、该留的留。
    #[test]
    fn memory_block_mixes_dedupe_limit_and_backlog_in_one_pass() {
        let lines = [
            "旁白: 在便利店买了伞", // 0 与「最近」重复 → 交付，不渲染
            "旁白: 去了公园",       // 1 渲染
            "旁白: 去了公园",       // 2 队列内重复 → 交付，不渲染
            "旁白: 遇到阿离",       // 3 渲染
            "旁白: 天黑了",         // 4 超上限 → 留着
        ];
        let (block, delivered) = memory_block(&lines, Some("刚才在便利店买了伞"), 2);
        assert_eq!(block, "\n记忆：旁白: 去了公园；旁白: 遇到阿离");
        assert_eq!(delivered, vec![0, 1, 2, 3], "第 4 条没交付，必须留在队列里");
    }
}
