//! 交通工具系统（T2-3 的 Rust 版，逐行对照 Python 原型 `world_map/transport.py`）
//!
//! ## 这个模块干什么
//! 9 种交通方式（walk/bike/bus/subway/taxi/car/train/plane/ferry）的参数表 →
//! 票价/耗时估算 → 门到门路径规划（接驳段 + 主段）→ 方案排序 →
//! 出行状态机（路线铺平成 phases 时间轴，按秒推进算当前位置）。
//! 前端 `transport.js` 只负责画，算法全在这里。
//!
//! ## 为什么这么设计
//! · **一张参数表吃全部**：新增交通方式只往 [`MODES`] 加一行，规划/排序/时间轴都不用改。
//!   `speed_kmh` 是「含停站与红绿灯的平均速度」而不是最高速度——前端动画要的是真实到达时间。
//! · **对外一律 `serde_json::Value`**：与 Python 的 dict 一一对应，前端与 HTTP 层拿到的
//!   JSON 形状和原型逐字段相同，移植期可以两边并跑对拍；只有内部计算才用小 struct/enum。
//!   本工程 serde_json 开了 `preserve_order`，字段顺序也和 Python dict 一致，方便肉眼 diff。
//! · **不比原型聪明**：阈值、权重、四舍五入的位置全部照抄，否则两边的自检结论会对不上
//!   （自检脚本断言的是「15km 市内推荐公交/地铁」「广深门到门 80~120 分钟」这类具体数值）。
//! · **facilities 不做硬依赖**：Python 版靠同目录 `facilities.py` 把接驳段吸附到真实站点，
//!   那个模块还在移植中，硬依赖会让本文件编不过。所以这里改成：**上游把设施节点 JSON 传进来
//!   就用真实数据做站点查找，不传就退回纯几何估计**（沿直线按 access_km 推一个虚拟站点）。
//!   `find_facilities` 的查找语义（type 过滤 + near 半径 + dist 升序 + limit）在本模块内复刻，
//!   等 facilities.rs 落地时把 [`makes_context`] 的入参换成它的输出即可，规划代码一行不用动。
//! · **不引 `rand`**：原型里没有任何随机成分（trip id = 时间戳 + 全局自增序号），这里也不加。
//!   「同输入同输出」是可对拍、可回归的前提，随手加抖动反而会破坏它。
//! · **不 panic**：Python 靠异常返回错误、靠 KeyError 暴露调用方 bug；Rust 版对缺失字段、
//!   未知交通方式一律退化取值（未知方式退化成 walk 的参数），非法输入得到 Null / 0，
//!   因为游戏主循环里一次 panic 就是整局崩。HTTP 适配层保留了 `{ok:false,error}` 的返回形状。
//! · **只读复用 `coord`**：与 Python `import world_coord as WC` 对应（网格 ↔ 地理换算）。
//!
//! ## 与 Python 原型的对应关系（函数级）
//! | Python | Rust |
//! |---|---|
//! | `MODE_ORDER` / `MODES` / `ALIASES` / `STRATEGY_ALIASES` / `PUBLIC_MODES` | 同名常量 |
//! | `haversine_m` / `_offset_point` / `_ll` / `_round` | [`haversine_m`] / 私有同名实现 |
//! | `makes_context` / `find_station_in_grid` / `_station_for` | [`makes_context`] / [`find_station_in_grid`] / 私有 |
//! | `set_water_checker` / `detect_cross_water` | [`set_water_checker`] / [`detect_cross_water`] |
//! | `fare_of` / `ride_minutes` / `fmt_*` / `transfer_text` / `modes_table` | 同名 |
//! | `is_urban` / `recommend_mode` | 同名 |
//! | `_leg` / `build_legs` / `build_route` | 同名（`_leg` 私有） |
//! | `applicable_modes` / `score_route` / `plan_options` / `normalize_prefer` / `plan_route` | 同名 |
//! | `build_timeline` / `start_trip` / `tick_trip` / `tick_to_now` / `trip_polyline` / `trip_payload` | 同名 |
//! | `_q` / `api_plan` / `api_modes` | 私有 `q` / 同名 |
//! | `__main__` 里那段自检 | 文件末尾 `mod tests`（原型的自检是内联的，没有单独的 selftest 文件） |
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::world_map::coord::{self, METERS_PER_DEG_LAT};

/// 地球平均半径（米），haversine 用（与原型一致）
pub const EARTH_R_M: f64 = 6_371_008.8;

/// 9 种交通方式的固定展示顺序（前端 tab 顺序也用它）
pub const MODE_ORDER: [&str; 9] = [
    "walk", "bike", "bus", "subway", "taxi", "car", "train", "plane", "ferry",
];

/// 公共交通三件套（`prefer='public'` 策略只在这些里挑）
pub const PUBLIC_MODES: [&str; 3] = ["bus", "subway", "train"];

/// 两端 50km 以内视为同一都市圈（地铁/公交通达）
pub const URBAN_MAX_M: f64 = 50_000.0;

/// 一种交通方式的全部参数。
///
/// 对应 Python `MODES[key]` 那个 dict：内部计算要频繁取这些数，用 struct 比每次
/// `value.get("speed_kmh").as_f64()` 更省事也更不容易写错；对外仍由
/// [`modes_json`] / [`modes_table`] 输出成 JSON。
#[derive(Debug, Clone, Copy)]
pub struct Mode {
    pub key: &'static str,
    pub name: &'static str,
    pub en: &'static str,
    pub icon: &'static str,
    pub color: &'static str,
    /// 平均速度，含停站/红绿灯（不是最高速度）
    pub speed_kmh: f64,
    /// 适用距离下限 / 上限（km），`applicable_modes` 据此过滤候选
    pub min_km: f64,
    pub max_km: f64,
    /// 是否有「换乘」概念
    pub need_transfer: bool,
    /// 每公里换乘次数系数（公交 1/8 ≈ 每 8km 换乘一次）
    pub transfer_per_km: f64,
    /// 每次换乘的额外等待（分钟）
    pub transfer_penalty_min: f64,
    /// 固定耗时：候车/安检/值机登机（分钟）
    pub overhead_min: f64,
    /// 起步价（元）/ 起步价含的公里数 / 超出部分单价
    pub fare_base: f64,
    pub fare_included_km: f64,
    pub fare_per_km: f64,
    pub fare_note: &'static str,
    /// 到站/到港接驳距离估计（km，单侧）与接驳方式
    pub access_km: f64,
    pub access_mode: &'static str,
    pub desc: &'static str,
}

/// 用 `static` 而不是 `const`：`mode()` 要返回 `&'static Mode`，
/// 而 `&MODES[i]`（运行期下标）无法从 const 提升为 'static 引用。
pub static MODES: [Mode; 9] = [
    Mode {
        key: "walk", name: "步行", en: "walk", icon: "🚶", color: "#8a8f98",
        speed_kmh: 4.5, min_km: 0.0, max_km: 2.0,
        need_transfer: false, transfer_per_km: 0.0, transfer_penalty_min: 0.0,
        overhead_min: 0.0,
        fare_base: 0.0, fare_included_km: 0.0, fare_per_km: 0.0, fare_note: "免费",
        access_km: 0.0, access_mode: "walk",
        desc: "短距离最灵活，不受路况影响，雨天/负重时慢",
    },
    Mode {
        key: "bike", name: "自行车/电动车", en: "bike", icon: "🚲", color: "#4caf7d",
        speed_kmh: 15.0, min_km: 0.3, max_km: 8.0,
        need_transfer: false, transfer_per_km: 0.0, transfer_penalty_min: 0.0,
        overhead_min: 3.0,
        fare_base: 1.5, fare_included_km: 3.0, fare_per_km: 0.3,
        fare_note: "共享单车 1.5 元/30 分钟或电动车充电费",
        access_km: 0.0, access_mode: "walk",
        desc: "中短距离性价比最高，可穿小路，需找车/停车",
    },
    Mode {
        key: "bus", name: "公交", en: "bus", icon: "🚌", color: "#f0a020",
        speed_kmh: 20.0, min_km: 0.8, max_km: 30.0,
        need_transfer: true, transfer_per_km: 1.0 / 8.0, transfer_penalty_min: 6.0,
        overhead_min: 5.0,
        fare_base: 2.0, fare_included_km: 999.0, fare_per_km: 0.0,
        fare_note: "投币/扫码 2 元一票制（分段计价城市按段加收）",
        access_km: 0.4, access_mode: "walk",
        desc: "覆盖最广、最便宜，受路况影响大",
    },
    Mode {
        key: "subway", name: "地铁", en: "subway", icon: "🚇", color: "#3d7bd6",
        speed_kmh: 35.0, min_km: 1.5, max_km: 60.0,
        need_transfer: true, transfer_per_km: 1.0 / 20.0, transfer_penalty_min: 4.0,
        overhead_min: 4.0,
        fare_base: 3.0, fare_included_km: 6.0, fare_per_km: 0.25,
        fare_note: "起步 3 元（含 6km），超出约 0.25 元/km",
        access_km: 0.5, access_mode: "walk",
        desc: "准点快速，站间距大，需安检进出站",
    },
    Mode {
        key: "taxi", name: "出租车/网约车", en: "taxi", icon: "🚕", color: "#f5c518",
        speed_kmh: 28.0, min_km: 0.8, max_km: 200.0,
        need_transfer: false, transfer_per_km: 0.0, transfer_penalty_min: 0.0,
        overhead_min: 4.0,
        fare_base: 13.0, fare_included_km: 3.0, fare_per_km: 2.6,
        fare_note: "起步价 13 元/3km，之后 2.6 元/km（含低速等候费）",
        access_km: 0.1, access_mode: "walk",
        desc: "门到门、随叫随走，贵且堵车时计价上涨",
    },
    Mode {
        key: "car", name: "私家车", en: "car", icon: "🚗", color: "#7e57c2",
        speed_kmh: 45.0, min_km: 1.0, max_km: 500.0,
        need_transfer: false, transfer_per_km: 0.0, transfer_penalty_min: 0.0,
        overhead_min: 4.0,
        fare_base: 0.0, fare_included_km: 0.0, fare_per_km: 0.85,
        fare_note: "油费+损耗约 0.85 元/km，另加停车费",
        access_km: 0.2, access_mode: "walk",
        desc: "自由度高、可带行李，需考虑停车与疲劳驾驶",
    },
    Mode {
        key: "train", name: "火车/高铁", en: "train", icon: "🚄", color: "#e05252",
        speed_kmh: 200.0, min_km: 30.0, max_km: 1500.0,
        need_transfer: true, transfer_per_km: 1.0 / 2000.0, transfer_penalty_min: 20.0,
        overhead_min: 30.0,
        fare_base: 5.0, fare_included_km: 0.0, fare_per_km: 0.46,
        fare_note: "二等座约 0.46 元/km（票价系数随线路浮动）",
        access_km: 6.0, access_mode: "taxi",
        desc: "中长途主力，准点舒适，两端需接驳车站",
    },
    Mode {
        key: "plane", name: "飞机", en: "plane", icon: "✈️", color: "#2fa8d6",
        speed_kmh: 750.0, min_km: 250.0, max_km: 12000.0,
        need_transfer: true, transfer_per_km: 0.0, transfer_penalty_min: 60.0,
        overhead_min: 100.0,
        fare_base: 50.0, fare_included_km: 0.0, fare_per_km: 0.7,
        fare_note: "票价约 0.7 元/km + 机场建设费/燃油附加约 50 元",
        access_km: 25.0, access_mode: "taxi",
        desc: "超长途最快，两端机场接驳与安检占时较多",
    },
    Mode {
        key: "ferry", name: "轮船/渡轮", en: "ferry", icon: "⛴️", color: "#1f9ea8",
        speed_kmh: 22.0, min_km: 0.5, max_km: 500.0,
        need_transfer: true, transfer_per_km: 1.0 / 300.0, transfer_penalty_min: 20.0,
        overhead_min: 20.0,
        fare_base: 10.0, fare_included_km: 0.0, fare_per_km: 0.35,
        fare_note: "起步 10 元，按航程计价（跨海航线另计）",
        access_km: 3.0, access_mode: "taxi",
        desc: "跨水域唯一/最直接的选择，班次少、受天气影响",
    },
];

/// 工具名别名（中英混用都认）→ 规范 key。顺序无关，查找是线性扫描。
pub const ALIASES: &[(&str, &str)] = &[
    ("walk", "walk"), ("walking", "walk"), ("步行", "walk"), ("走路", "walk"), ("徒步", "walk"),
    ("bike", "bike"), ("bicycle", "bike"), ("自行车", "bike"), ("电动车", "bike"), ("单车", "bike"),
    ("骑行", "bike"), ("共享单车", "bike"),
    ("bus", "bus"), ("公交车", "bus"), ("公交", "bus"), ("巴士", "bus"), ("公共汽车", "bus"),
    ("subway", "subway"), ("metro", "subway"), ("地铁", "subway"), ("轨道交通", "subway"),
    ("taxi", "taxi"), ("打车", "taxi"), ("出租车", "taxi"), ("网约车", "taxi"), ("滴滴", "taxi"),
    ("car", "car"), ("自驾", "car"), ("开车", "car"), ("私家车", "car"), ("驾车", "car"),
    ("train", "train"), ("火车", "train"), ("高铁", "train"), ("动车", "train"), ("铁路", "train"),
    ("plane", "plane"), ("飞机", "plane"), ("航班", "plane"), ("航空", "plane"), ("坐飞机", "plane"),
    ("ferry", "ferry"), ("轮船", "ferry"), ("渡轮", "ferry"), ("轮渡", "ferry"), ("船", "ferry"),
    ("客船", "ferry"),
];

/// 策略型 prefer：不指定具体工具，而是给规划目标
pub const STRATEGY_ALIASES: &[(&str, &str)] = &[
    ("auto", "balanced"), ("自动", "balanced"), ("推荐", "balanced"),
    ("fastest", "fastest"), ("最快", "fastest"), ("时间最短", "fastest"),
    ("cheapest", "cheapest"), ("最便宜", "cheapest"), ("省钱", "cheapest"),
    ("balanced", "balanced"), ("均衡", "balanced"), ("性价比", "balanced"),
    ("public", "public"), ("transit", "public"), ("公共交通", "public"), ("公交地铁", "public"),
];

/// 自检/示例用的城市坐标（与原型 `__main__` 里那份一致）
pub const GUANGZHOU: (f64, f64) = (113.2640, 23.1290); // 广州市中心（北京路）
pub const SHENZHEN: (f64, f64) = (114.0579, 22.5431); // 深圳市中心
pub const BEIJING: (f64, f64) = (116.4074, 39.9042); // 北京市中心
pub const GZ_TA: (f64, f64) = (113.3240, 23.1065); // 广州塔
pub const GZ_TIANHE: (f64, f64) = (113.3244, 23.1374); // 天河体育中心
pub const GZ_SOUTH: (f64, f64) = (113.2690, 22.9890); // 广州南站
pub const XUWEN: (f64, f64) = (110.1900, 20.2800); // 徐闻海安港（雷州半岛）
pub const HAIKOU: (f64, f64) = (110.2800, 20.0300); // 海口秀英港

// ══════════════════════════════════════════════════════════════════════════
// 一、基础工具：取数、取整、格式化
// ══════════════════════════════════════════════════════════════════════════

/// 从对象里取数字字段，缺失/类型不对时用默认值（替代 Python 的 `.get(k, d)`）
fn num(v: &Value, k: &str, d: f64) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(d)
}

/// 复刻 **CPython 3.12+ `sum()` 对 float 的 Neumaier 补偿求和**。
///
/// 为什么不是简单的 fold：Python 3.12 起 `sum()` 遇到 float 会切换成
/// Arnold Neumaier 的改进 Kahan 算法，于是 `sum([1.3, 32.9, 1.3])` 是 35.5，
/// 而朴素的从左往右累加是 35.49999999999999。差这 1e-14 会顺着
/// `duration_min → fmt_duration` 变成「40分钟 vs 39分钟」这种肉眼可见的差别
/// （本机实测：广州→广州南站方案列表第 3 条的时长文案）。
/// 原型的自检数值是在这台机器上（Python 3.14）跑出来的，所以这里跟着它走。
/// 如果哪天要退回 Python ≤3.11 的语义，把 `py_sum` 换成普通的 `.sum()` 即可。
fn py_sum<I: IntoIterator<Item = f64>>(items: I) -> f64 {
    let mut total = 0.0f64;
    let mut comp = 0.0f64;
    for x in items {
        let t = total + x;
        if total.abs() >= x.abs() {
            comp += (total - t) + x;
        } else {
            comp += (x - t) + total;
        }
        total = t;
    }
    total + comp
}

/// 取 [lng, lat] 形式的点；不是数组就退化成默认点（Python 会抛，这里不 panic）
fn pair_of(v: &Value, d: (f64, f64)) -> (f64, f64) {
    match v.as_array() {
        Some(a) => (
            a.first().and_then(|x| x.as_f64()).unwrap_or(d.0),
            a.get(1).and_then(|x| x.as_f64()).unwrap_or(d.1),
        ),
        None => d,
    }
}

/// 保留 n 位小数，**与 Python 的 `round(x, n)` 逐位一致**（含 .5 的银行家舍入）。
///
/// 为什么不用更快的 `(x * 10^n).round() / 10^n`：乘 10^n 会再引入一次舍入，
/// 使恰好落在十进制边界附近的值偏到另一边。实测 8999 个用例（含 1500 个随机位模式）
/// 有 89 例与 Python 不一致（约 1%），例如 `2.675` 会变成 2.7 而不是 2.67；
/// 这些 1 ulp 的差异会顺着 duration_min → 总时长 → `fmt_duration` 放大成
/// 「39分钟 vs 40分钟」这种肉眼可见的差别。改成「按 n 位格式化再解析回来」后 0 例不一致
/// —— 两边都是在做「对二进制精确值做十进制正确舍入、.5 取偶」。
/// 代价是每次调用一次格式化，量级是每 tick 几次，可忽略。
fn round_n(x: f64, n: i32) -> f64 {
    if !x.is_finite() {
        return x; // inf/NaN 原样返回（Python 在 int()/round 上会抛，这里不 panic）
    }
    match format!("{:.*}", n.max(0) as usize, x).parse::<f64>() {
        Ok(v) => v,
        Err(_) => x,
    }
}

/// 点取整为 6 位（约 0.1m 精度够了）→ 元组，对应 Python `_round`
fn round_pair(p: (f64, f64)) -> (f64, f64) {
    (round_n(p.0, 6), round_n(p.1, 6))
}

/// 经纬度取整成 JSON 数组（JSON 友好），对应 Python `_ll`
fn ll(p: (f64, f64)) -> Value {
    json!([round_n(p.0, 6), round_n(p.1, 6)])
}

/// 去掉小数尾随 0（`"12000.0"` → `"12000"`，`"0.30"` → `"0.3"`）
fn trim_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let t = s.trim_end_matches('0');
    t.trim_end_matches('.').to_string()
}

/// 模拟 Python 的 `'%g'`（6 位有效数字，去尾随 0）——`range_text` 要跟原型逐字符一致
fn fmt_g(x: f64) -> String {
    if !x.is_finite() {
        return format!("{x}");
    }
    if x == 0.0 {
        return "0".to_string();
    }
    let exp = x.abs().log10().floor() as i32;
    if exp < -4 || exp >= 6 {
        // 科学计数法：Python 输出 "1.23457e+06"，Rust 的 {:e} 是 "1.234567e6"（最短往返），
        // 所以固定 5 位小数凑够 6 位有效数字，再补成 Python 的 e±NN 形状
        let s = format!("{:.5e}", x);
        let (m, e) = s.split_once('e').unwrap_or((s.as_str(), "0"));
        let ei: i32 = e.parse().unwrap_or(0);
        let sign = if ei < 0 { "-" } else { "+" };
        format!("{}e{}{:02}", trim_zeros(m), sign, ei.abs())
    } else {
        let prec = (6 - 1 - exp).max(0) as usize;
        trim_zeros(&format!("{:.*}", prec, x))
    }
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// 交通方式的规范 key 是否存在（Python 里未知 key 会 KeyError，这里给你一个查询口）
pub fn has_mode(key: &str) -> bool {
    MODES.iter().any(|m| m.key == key)
}

/// 按 key 查参数表
pub fn mode(key: &str) -> Option<&'static Mode> {
    MODES.iter().find(|m| m.key == key)
}

/// 内部用：未知 key 退化成 walk（绝不 panic）。Python 这里会抛 KeyError。
fn mode_of(key: &str) -> &'static Mode {
    mode(key).unwrap_or(&MODES[0])
}

/// 别名 → 规范 key（大小写敏感，调用方负责先 lower）
pub fn alias_mode(p: &str) -> Option<&'static str> {
    ALIASES.iter().find(|(k, _)| *k == p).map(|(_, v)| *v)
}

/// 策略别名 → 策略名
pub fn alias_strategy(p: &str) -> Option<&'static str> {
    STRATEGY_ALIASES.iter().find(|(k, _)| *k == p).map(|(_, v)| *v)
}

/// 别名表 → JSON（给 API/UI 展示，对应 Python 的 `ALIASES`）
pub fn aliases_json() -> Value {
    let mut m = serde_json::Map::new();
    for (k, v) in ALIASES {
        m.insert((*k).to_string(), json!(v));
    }
    Value::Object(m)
}

/// 策略别名表 → JSON（对应 Python 的 `STRATEGY_ALIASES`）
pub fn strategy_aliases_json() -> Value {
    let mut m = serde_json::Map::new();
    for (k, v) in STRATEGY_ALIASES {
        m.insert((*k).to_string(), json!(v));
    }
    Value::Object(m)
}

/// 交通工具 → facilities 节点类型（facilities.TRANSPORT_TYPES 的 key 子集）
pub fn fac_types_for(mode_key: &str) -> Option<&'static [&'static str]> {
    match mode_key {
        "bus" => Some(&["bus_stop"]),
        "subway" => Some(&["subway"]),
        "train" => Some(&["train_station"]),
        "plane" => Some(&["airport"]),
        "ferry" => Some(&["pier"]),
        "car" | "taxi" => Some(&["parking"]),
        _ => None,
    }
}

/// 交通工具 → 中文站名（无 facilities 时用于拼接入驳段文案）
pub fn station_zh(mode_key: &str) -> Option<&'static str> {
    match mode_key {
        "bus" => Some("公交站"),
        "subway" => Some("地铁站"),
        "train" => Some("火车站"),
        "plane" => Some("机场"),
        "ferry" => Some("码头"),
        "car" => Some("停车场"),
        "taxi" => Some("上车点"),
        _ => None,
    }
}

/// 站名兜底：没有约定俗成的中文站名就用「<方式名>站点」
fn station_label(mode_key: &str) -> String {
    station_zh(mode_key)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}站点", mode_of(mode_key).name))
}

// ══════════════════════════════════════════════════════════════════════════
// 二、基础几何：距离与插值
// ══════════════════════════════════════════════════════════════════════════

/// 两点球面距离（米）。a/b 为 (lng, lat)。自实现，不依赖第三方（与原型逐行一致）。
///
/// 坐标含 NaN 时 Python 的 `min(1.0, sqrt(s))` 会取到 1.0，Rust 的 `f64::min` 同样
/// 会忽略 NaN 取另一个操作数，两边行为一致（返回半个大圆周长，而不是 NaN）。
pub fn haversine_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dlat = (b.1 - a.1) * coord::D2R;
    let dlng = (b.0 - a.0) * coord::D2R;
    let s = (dlat / 2.0).sin().powi(2)
        + (a.1 * coord::D2R).cos() * (b.1 * coord::D2R).cos() * (dlng / 2.0).sin().powi(2);
    2.0 * EARTH_R_M * s.sqrt().min(1.0).asin()
}

/// 从 a 沿 a→b 方向前进 dist_m 的近似点（接驳段几何估计用，局部平面近似）。
/// 这就是「没传 facilities 时的虚拟站点」：纯几何、确定性，与原型 `_offset_point` 一致。
fn offset_point(a: (f64, f64), b: (f64, f64), dist_m: f64) -> (f64, f64) {
    let mlng = coord::meters_per_deg_lng(a.1);
    let dx = (b.0 - a.0) * mlng;
    let dy = (b.1 - a.1) * METERS_PER_DEG_LAT;
    let l = (dx * dx + dy * dy).sqrt();
    if !(l >= 1e-6) {
        // 起终点重合（或坐标非法）→ 原地不动，等价于 Python 的 `if L < 1e-6`
        return a;
    }
    let f = (dist_m / l).min(1.0);
    (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f)
}

// ══════════════════════════════════════════════════════════════════════════
// 三、设施节点复用（可选：上游传 facilities JSON 就吸附真实站点）
// ══════════════════════════════════════════════════════════════════════════
// 说明：facilities 的交通节点是「小区网格坐标 (gx,gy)」而不是经纬度，
// 所以吸附要先把查询点 world_to_grid 落到网格，找最近节点，再 grid_to_world →
// world_to_lng_lat 换回经纬度。整条桥接都在本模块内，换算复用 coord。
//
// 与 Python 的差别（刻意）：Python 的 `_facilities()` 是惰性 import facilities.py，
// 导入失败就退回几何估计；facilities.rs 还没移植完，这里不看模块是否存在，
// 只看调用方有没有在 context 里塞 `facilities` 数组——语义上等价，且没有硬依赖。

/// 站点信息（内部计算用的小 struct，对外仍是 JSON dict）
#[derive(Debug, Clone)]
struct Station {
    name: String,
    kind: String,
    gx: f64,
    gy: f64,
    lng: f64,
    lat: f64,
    /// 网格距离（格），对应 facilities 的 `dist` 字段
    grid_dist: Option<f64>,
    /// 实际球面距离（米）
    dist_m: f64,
}

impl Station {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "kind": self.kind,
            "gx": self.gx,
            "gy": self.gy,
            "lng": self.lng,
            "lat": self.lat,
            "grid_dist": self.grid_dist,
            "dist_m": self.dist_m,
        })
    }
}

/// 构造小区布局上下文（复用设施节点做接驳吸附）。
///
/// * `facilities` — `facilities.generate_transport_nodes(...)` 的返回值（网格坐标节点列表）
/// * `anchor`     — 网格原点 (lng, lat)（Python 里 dict/tuple 都收，这里统一成 tuple）
/// * `cell_meters`— 每格米数（与节点生成时用的同一个值）
/// * `grid_size`  — 小区网格边长（格），用于圈定最大吸附半径
pub fn makes_context(
    facilities: &[Value],
    anchor: (f64, f64),
    cell_meters: f64,
    grid_size: Option<f64>,
) -> Value {
    json!({
        "facilities": facilities,
        "anchor": {"lng": anchor.0, "lat": anchor.1},
        "cell_meters": cell_meters,
        "grid_size": grid_size,
    })
}

/// 在 facilities 数组里找最近节点（复刻 `facilities.find_facilities` 的 near 语义）：
/// 先按 type 过滤，再按 `hypot` 距离（格）过滤半径，按 dist 升序取最近的若干个。
fn find_facilities_near(
    facilities: &[Value],
    types: Option<&[&str]>,
    near: (f64, f64),
    radius: Option<f64>,
    limit: Option<usize>,
) -> Vec<Value> {
    let mut out: Vec<(f64, Value)> = Vec::new();
    for f in facilities {
        if let Some(ts) = types {
            let t = f.get("type").and_then(|x| x.as_str()).unwrap_or("");
            if !ts.contains(&t) {
                continue;
            }
        }
        let gx = num(f, "gx", 0.0);
        let gy = num(f, "gy", 0.0);
        let d = ((gx - near.0).powi(2) + (gy - near.1).powi(2)).sqrt();
        if let Some(r) = radius {
            if d > r {
                continue;
            }
        }
        let dist = round_n(d, 2);
        let mut item = f.clone();
        if let Some(o) = item.as_object_mut() {
            o.insert("dist".to_string(), json!(dist));
        }
        out.push((dist, item));
    }
    // 稳定排序：距离相同时保持传入顺序（与 Python 的 list.sort 一致）
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out.into_iter()
        .take(limit.unwrap_or(usize::MAX))
        .map(|(_, v)| v)
        .collect()
}

/// 在小区布局里找最近的交通节点（内部版，返回 struct）。
/// 布局只覆盖它自己那片小区：查询点落在网格外（含 15% 边距）就不吸附，
/// 否则会把几百米外的节点硬套到别处的坐标上。
fn station_in_grid(
    lng: f64,
    lat: f64,
    context: Option<&Value>,
    fac_types: Option<&[&str]>,
) -> Option<Station> {
    let ctx = context?;
    let facilities = ctx.get("facilities").and_then(|v| v.as_array())?;
    if facilities.is_empty() {
        return None;
    }
    let anchor = ctx.get("anchor")?;
    let alng = anchor.get("lng").and_then(|v| v.as_f64())?;
    let alat = anchor.get("lat").and_then(|v| v.as_f64())?;
    let mpc = num(ctx, "cell_meters", 0.0);
    if mpc <= 0.0 {
        return None;
    }
    let size = ctx
        .get("grid_size")
        .and_then(|v| v.as_f64())
        .filter(|v| *v != 0.0);

    let (wx, wy) = coord::lng_lat_to_world(lng, lat);
    let (gx, gy) = coord::world_to_grid(wx, wy, alng, alat, mpc);

    if let Some(s) = size {
        let margin = (s * 0.15).max(2.0);
        if !(-margin <= gx && gx <= s + margin && -margin <= gy && gy <= s + margin) {
            return None;
        }
    }
    let radius = size.map(|s| s * 0.5).unwrap_or(10.0);
    let cand = find_facilities_near(facilities, fac_types, (gx, gy), Some(radius), Some(1));
    let f = cand.first()?;

    let (gwx, gwy) = coord::grid_to_world(num(f, "gx", 0.0), num(f, "gy", 0.0), alng, alat, mpc);
    let (slng, slat) = coord::world_to_lng_lat(gwx, gwy);
    Some(Station {
        name: f.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        kind: f.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        gx: num(f, "gx", 0.0),
        gy: num(f, "gy", 0.0),
        lng: slng,
        lat: slat,
        grid_dist: f.get("dist").and_then(|v| v.as_f64()),
        dist_m: haversine_m((lng, lat), (slng, slat)),
    })
}

/// 公开版：与 Python `find_station_in_grid` 同形状的 dict
/// → `{'name','kind','gx','gy','lng','lat','grid_dist','dist_m'}` 或 None
pub fn find_station_in_grid(
    lng: f64,
    lat: f64,
    context: Option<&Value>,
    fac_types: Option<&[&str]>,
) -> Option<Value> {
    station_in_grid(lng, lat, context, fac_types).map(|s| s.to_json())
}

/// 为某交通方式找站点：优先 facilities 网格节点，否则 None（调用方退回几何估计）
fn station_for(mode_key: &str, lng: f64, lat: f64, context: Option<&Value>) -> Option<Station> {
    station_in_grid(lng, lat, context, fac_types_for(mode_key))
}

// ══════════════════════════════════════════════════════════════════════════
// 四、跨水检测钩子（跨水优先轮船）
// ══════════════════════════════════════════════════════════════════════════
// 本模块不内置水域/海岸线数据（体积大且需与地图数据同源）。两种用法：
// ① 调用方直接传 cross_water=True/False；② 注册水域检测器，由地理模块注入。
// 与 Python 的唯一差别：Rust 没有「捕获异常」，检测器里 panic 会传播给调用方，
// 所以约定检测器必须自己保证不 panic（锁中毒时我们会取回内层值而不是二次 panic）。

/// 水域检测器签名：两点是否跨水
pub type WaterChecker = Box<dyn Fn((f64, f64), (f64, f64)) -> bool + Send + Sync + 'static>;

static WATER_CHECKER: Mutex<Option<WaterChecker>> = Mutex::new(None);

/// 注册跨水检测器：`fn((lng1,lat1),(lng2,lat2)) -> bool`。传 None 取消注册。
/// 例如可由地图数据模块实现「两点连线是否跨越水域多边形」。
/// 注意：这是进程级全局状态（与原型一致），测试里串行使用或只在单测内设置/复原。
pub fn set_water_checker(f: Option<WaterChecker>) {
    let mut slot = WATER_CHECKER.lock().unwrap_or_else(|e| e.into_inner());
    *slot = f;
}

/// 是否已注册检测器（自检/调试用，原型里靠打印 `_WATER_CHECKER` 判断）
pub fn water_checker_registered() -> bool {
    WATER_CHECKER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
}

/// 用显式传入的检测器判断（纯函数版，方便测试与多实例场景）
pub fn detect_cross_water_with<F>(checker: Option<&F>, a: (f64, f64), b: (f64, f64)) -> bool
where
    F: Fn((f64, f64), (f64, f64)) -> bool + ?Sized,
{
    match checker {
        Some(f) => f((a.0, a.1), (b.0, b.1)),
        None => false,
    }
}

/// 自动判断是否跨水。未注册检测器时返回 false（保守，由调用方显式指定）。
pub fn detect_cross_water(a: (f64, f64), b: (f64, f64)) -> bool {
    let slot = WATER_CHECKER.lock().unwrap_or_else(|e| e.into_inner());
    match slot.as_ref() {
        Some(f) => f((a.0, a.1), (b.0, b.1)),
        None => false,
    }
}

// ══════════════════════════════════════════════════════════════════════════
// 五、票价 / 耗时 / 格式化
// ══════════════════════════════════════════════════════════════════════════

/// 按里程估算票价（元）。起步价含 `fare_included_km`，超出部分按 `fare_per_km`。
/// 未知方式返回 0（Python 会 KeyError）——调用方本来就用 `applicable_modes` 校验过 key。
pub fn fare_of(mode_key: &str, distance_km: f64) -> f64 {
    match mode(mode_key) {
        Some(m) => {
            let billable = (distance_km - m.fare_included_km).max(0.0);
            round_n(m.fare_base + m.fare_per_km * billable, 2)
        }
        None => 0.0,
    }
}

/// 纯行驶时间（分钟），不含候车/接驳。未知方式返回 0。
pub fn ride_minutes(mode_key: &str, distance_m: f64) -> f64 {
    match mode(mode_key) {
        Some(m) => distance_m / 1000.0 / m.speed_kmh * 60.0,
        None => 0.0,
    }
}

/// 分钟 → 中文时长（`45分钟` / `1小时20分` / `3小时`）
pub fn fmt_duration(minutes: f64) -> String {
    // Python 是 int(round(m))，round 为银行家舍入 → 走 round_n(_, 0) 保持一致
    let m = round_n(minutes, 0) as i64; // NaN/∞ → 饱和，不 panic（Python 会抛）
    if m < 60 {
        return format!("{m}分钟");
    }
    let (h, r) = (m / 60, m % 60);
    if r != 0 {
        format!("{h}小时{r}分")
    } else {
        format!("{h}小时")
    }
}

/// 米 → 中文距离（`850米` / `1.5公里`）
pub fn fmt_distance(meters: f64) -> String {
    if meters < 1000.0 {
        return format!("{}米", round_n(meters, 0) as i64);
    }
    format!("{:.1}公里", meters / 1000.0)
}

/// 金额 → 中文（免费 / ¥9.5 / ¥13）
pub fn fmt_cost(cost: f64) -> String {
    if cost <= 0.0 {
        return "免费".to_string();
    }
    if cost >= 10.0 {
        format!("¥{cost:.0}")
    } else {
        format!("¥{cost:.1}")
    }
}

/// 换乘文案：不需要换乘的工具说「无需换乘」，需要的说「直达」
pub fn transfer_text(tc: i64, need_transfer: bool) -> String {
    if tc <= 0 {
        return if need_transfer { "直达" } else { "无需换乘" }.to_string();
    }
    format!("换乘{tc}次")
}

/// 9 种工具的参数表（供 API / UI 展示），字段与 `MODES` 一致并附格式化文本
pub fn modes_table() -> Vec<Value> {
    MODE_ORDER
        .iter()
        .filter_map(|k| mode(k))
        .map(|m| {
            json!({
                "key": m.key, "name": m.name, "en": m.en, "icon": m.icon, "color": m.color,
                "speed_kmh": m.speed_kmh,
                "min_km": m.min_km, "max_km": m.max_km,
                "range_text": format!("{}~{} km", fmt_g(m.min_km), fmt_g(m.max_km)),
                "need_transfer": m.need_transfer, "desc": m.desc,
                "fare_note": m.fare_note, "fare_base": m.fare_base, "fare_per_km": m.fare_per_km,
                "overhead_min": m.overhead_min, "access_km": m.access_km, "access_mode": m.access_mode,
            })
        })
        .collect()
}

/// 原始参数表（含换乘系数、起步价含公里数等内部字段）→ JSON，对应 Python 的 `MODES`
pub fn modes_json() -> Value {
    let mut out = serde_json::Map::new();
    for m in MODES.iter() {
        out.insert(
            m.key.to_string(),
            json!({
                "key": m.key, "name": m.name, "en": m.en, "icon": m.icon, "color": m.color,
                "speed_kmh": m.speed_kmh, "min_km": m.min_km, "max_km": m.max_km,
                "need_transfer": m.need_transfer, "transfer_per_km": m.transfer_per_km,
                "transfer_penalty_min": m.transfer_penalty_min, "overhead_min": m.overhead_min,
                "fare_base": m.fare_base, "fare_included_km": m.fare_included_km,
                "fare_per_km": m.fare_per_km, "fare_note": m.fare_note,
                "access_km": m.access_km, "access_mode": m.access_mode, "desc": m.desc,
            }),
        );
    }
    Value::Object(out)
}

// ══════════════════════════════════════════════════════════════════════════
// 六、推荐逻辑
// ══════════════════════════════════════════════════════════════════════════

/// 是否都市圈内出行。`urban` 显式传入则优先（None = 按距离自动判断）。
pub fn is_urban(distance_m: f64, urban: Option<bool>) -> bool {
    match urban {
        Some(u) => u,
        None => distance_m < URBAN_MAX_M,
    }
}

/// 按距离自动推荐交通方式 → (mode_key, reason_text)。
/// 规则：跨水优先轮船；<1.5km 步行；<4km 公交；都市圈内 <15km 地铁；
///       <300km 火车/高铁；更远飞机。
pub fn recommend_mode(distance_m: f64, cross_water: bool, urban: Option<bool>) -> (String, String) {
    let d = distance_m;
    let d_km = d / 1000.0;
    let urban = is_urban(d, urban);
    if cross_water && d_km <= mode_of("ferry").max_km {
        return ("ferry".to_string(), format!("跨水域 {}，轮渡最直接", fmt_distance(d)));
    }
    if d < 1500.0 {
        return ("walk".to_string(), format!("仅 {}，步行最省事", fmt_distance(d)));
    }
    if d < 4000.0 {
        return ("bus".to_string(), format!("{}，公交一票制最划算", fmt_distance(d)));
    }
    if urban && d < 15000.0 {
        return ("subway".to_string(), format!("都市圈内 {}，地铁准点不堵车", fmt_distance(d)));
    }
    if d < 300000.0 {
        if urban && d < 20000.0 {
            return ("subway".to_string(), format!("都市圈内 {}，地铁准点不堵车", fmt_distance(d)));
        }
        if d < mode_of("train").min_km * 1000.0 {
            return ("car".to_string(), format!("{} 未达铁路优势区间，自驾更灵活", fmt_distance(d)));
        }
        return ("train".to_string(), format!("{}，高铁中长途性价比最优", fmt_distance(d)));
    }
    ("plane".to_string(), format!("{} 超长途，飞机最快", fmt_distance(d)))
}

// ══════════════════════════════════════════════════════════════════════════
// 七、路径规划
// ══════════════════════════════════════════════════════════════════════════

/// 构造一段行程（steps 元素）。`dist_m` 为 None 时按 haversine 计算。
fn leg(mode_key: &str, a: (f64, f64), b: (f64, f64), dist_m: Option<f64>, note: &str) -> Value {
    let m = mode_of(mode_key);
    let dist = dist_m.unwrap_or_else(|| haversine_m(a, b));
    let dur = ride_minutes(mode_key, dist);
    json!({
        "mode": mode_key,
        "mode_name": m.name,
        "icon": m.icon,
        "color": m.color,
        "from": ll(a),
        "to": ll(b),
        "distance_m": round_n(dist, 1),
        "duration_min": round_n(dur, 1),
        "cost": fare_of(mode_key, dist / 1000.0),
        "speed_kmh": m.speed_kmh,
        "note": note,
    })
}

/// 把一段「门到门」出行拆成 接驳 → 主段 → 接驳 的 steps。
/// 主段两端站点位置：① context 里有 facilities 节点 → 吸附真实站点（网格↔地理换算）；
/// ② 否则沿直线按 `access_km` 推一个虚拟站点位置（几何估计）。
pub fn build_legs(
    mode_key: &str,
    a: (f64, f64),
    b: (f64, f64),
    distance_m: Option<f64>,
    context: Option<&Value>,
) -> Vec<Value> {
    let m = mode_of(mode_key);
    let a = round_pair(a);
    let b = round_pair(b);
    let dist = distance_m.unwrap_or_else(|| haversine_m(a, b));
    let d_km = dist / 1000.0;
    // 接驳不超过全程 20%，避免短途出现「接驳比主干还长」的怪结果
    let access_km = m.access_km.min(d_km * 0.2);
    let mut steps = Vec::new();
    if access_km > 0.05 {
        let st_a = station_for(mode_key, a.0, a.1, context);
        let st_b = station_for(mode_key, b.0, b.1, context);
        let p1 = st_a
            .as_ref()
            .map(|s| (s.lng, s.lat))
            .unwrap_or_else(|| offset_point(a, b, access_km * 1000.0));
        let p2 = st_b
            .as_ref()
            .map(|s| (s.lng, s.lat))
            .unwrap_or_else(|| offset_point(b, a, access_km * 1000.0));
        let am = m.access_mode;
        let zh = station_label(mode_key);
        let n1 = st_a
            .as_ref()
            .map(|s| s.name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| zh.clone());
        let n2 = st_b
            .as_ref()
            .map(|s| s.name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| zh.clone());
        steps.push(leg(am, a, p1, None, &format!("接驳到{n1}")));
        steps.push(leg(
            mode_key,
            p1,
            p2,
            Some(haversine_m(p1, p2)),
            &format!("{}行程", m.name),
        ));
        steps.push(leg(am, p2, b, None, &format!("从{n2}到目的地")));
    } else {
        steps.push(leg(mode_key, a, b, Some(dist), &format!("{}直达", m.name)));
    }
    steps
}

/// 按指定交通方式生成完整路线对象（[`plan_route`] 的骨架）。
pub fn build_route(
    mode_key: &str,
    a: (f64, f64),
    b: (f64, f64),
    distance_m: Option<f64>,
    cross_water: bool,
    reason: &str,
    context: Option<&Value>,
) -> Value {
    let m = mode_of(mode_key);
    let a = round_pair(a);
    let b = round_pair(b);
    let dist = distance_m.unwrap_or_else(|| haversine_m(a, b));
    let d_km = dist / 1000.0;

    let steps = build_legs(mode_key, a, b, Some(dist), context);
    // Python 的 int() 与 Rust 的 `as i64` 都是朝零截断。
    // 夹一个上限：坐标是 NaN/∞ 时 `as i64` 会饱和到 i64::MAX，而 build_timeline 会按
    // 换乘次数 for 循环，那样等于死循环（Python 在 int(inf) 处直接 OverflowError）。
    // 10 万次换乘已经远超任何真实数据（公交要 80 万公里），正常输入完全不受影响。
    let tc = ((m.transfer_per_km * d_km) as i64).clamp(0, 100_000);
    let overhead = m.overhead_min;
    let ride = py_sum(steps.iter().map(|s| num(s, "duration_min", 0.0)));
    let total_min = ride + overhead + tc as f64 * m.transfer_penalty_min;
    let cost = round_n(py_sum(steps.iter().map(|s| num(s, "cost", 0.0))), 2);

    // 摘要：把连续重复的工具合并，如「出租车 → 高铁 → 出租车」
    let mut seq: Vec<String> = Vec::new();
    for s in &steps {
        let name = s.get("mode_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if seq.last().map(|x| x != &name).unwrap_or(true) {
            seq.push(name);
        }
    }
    let summary = seq.join(" → ");

    let reason_text = if reason.is_empty() {
        format!("{} · {}", fmt_distance(dist), m.name)
    } else {
        reason.to_string()
    };
    let wait_min = overhead + tc as f64 * m.transfer_penalty_min;

    json!({
        "from": ll(a), "to": ll(b),
        "mode": mode_key, "mode_name": m.name, "icon": m.icon, "color": m.color,
        "distance_m": round_n(dist, 1), "distance_text": fmt_distance(dist),
        "duration_min": round_n(total_min, 1), "duration_text": fmt_duration(total_min),
        "ride_min": round_n(ride, 1),
        "wait_min": round_n(wait_min, 1),
        "cost": cost, "cost_text": fmt_cost(cost),
        "transfer_count": tc, "transfer_text": transfer_text(tc, m.need_transfer),
        "transfer_penalty_min": m.transfer_penalty_min,
        "overhead_min": overhead,
        "need_transfer": m.need_transfer,
        "cross_water": cross_water,
        "steps": steps, "summary": summary,
        "reason": reason_text,
        "fare_note": m.fare_note,
        "speed_kmh": m.speed_kmh,
    })
}

/// 按适用距离范围筛出候选工具（[`plan_options`] 用）。
/// `only` 里出现未知 key 时跳过（Python 会 KeyError）——前端传错参数不该让服务端 500。
pub fn applicable_modes(
    distance_m: f64,
    cross_water: bool,
    urban: Option<bool>,
    only: Option<&[&str]>,
) -> Vec<String> {
    let d_km = distance_m / 1000.0;
    let urban = is_urban(distance_m, urban);
    let pool: Vec<&str> = match only {
        Some(o) if !o.is_empty() => o.to_vec(),
        _ => MODE_ORDER.to_vec(),
    };
    let mut out: Vec<String> = Vec::new();
    for k in pool {
        let m = match mode(k) {
            Some(m) => m,
            None => continue,
        };
        if k == "ferry" && !cross_water {
            continue;
        }
        if k == "subway" && !urban && d_km > 30.0 {
            continue; // 非都市圈不谈地铁
        }
        if (k == "walk" || k == "bike") && d_km > m.max_km {
            continue;
        }
        if d_km < m.min_km || d_km > m.max_km {
            continue;
        }
        out.push(k.to_string());
    }
    // 跨水时轮渡必须在候选里
    if cross_water && !out.iter().any(|k| k == "ferry") && d_km <= mode_of("ferry").max_km {
        out.push("ferry".to_string());
    }
    // 至少保证「推荐方式」在候选内，否则候选为空时给个兜底
    // （这里 urban 已经是解析后的 bool，套 Some 传进去与原型语义一致）
    if out.is_empty() {
        out.push(recommend_mode(distance_m, cross_water, Some(urban)).0);
    }
    out
}

/// 方案排序分（越小越优）。balanced 把 1 元折算 0.6 分钟，属经验权重。
pub fn score_route(route: &Value, sort: &str) -> f64 {
    if sort == "fastest" {
        return num(route, "duration_min", 0.0);
    }
    if sort == "cheapest" {
        return num(route, "cost", 0.0);
    }
    num(route, "duration_min", 0.0) + num(route, "cost", 0.0) * 0.6
}

fn cmp_f64(a: f64, b: f64) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

/// 列出多个可行方案（前端「推荐方案列表」的数据源），已按 `sort` 排序。
#[allow(clippy::too_many_arguments)]
pub fn plan_options(
    from_lnglat: (f64, f64),
    to_lnglat: (f64, f64),
    cross_water: Option<bool>,
    urban: Option<bool>,
    sort: &str,
    only: Option<&[&str]>,
    limit: Option<usize>,
    context: Option<&Value>,
) -> Vec<Value> {
    let a = round_pair(from_lnglat);
    let b = round_pair(to_lnglat);
    let d = haversine_m(a, b);
    let cw = cross_water.unwrap_or_else(|| detect_cross_water(a, b));
    let modes = applicable_modes(d, cw, urban, only);
    let mut opts: Vec<Value> = modes
        .iter()
        .map(|k| {
            let mut why = format!("{} · {}", fmt_distance(d), mode_of(k).name);
            if k == "ferry" && cw {
                why = format!("跨水域 {}，轮渡直航", fmt_distance(d));
            }
            build_route(k, a, b, Some(d), cw, &why, context)
        })
        .collect();
    // Python 的 key 是 (score, duration_min) 元组；Vec::sort_by 是稳定排序，
    // 与 list.sort 一样，同分保持候选顺序
    opts.sort_by(|x, y| {
        cmp_f64(score_route(x, sort), score_route(y, sort))
            .then_with(|| cmp_f64(num(x, "duration_min", 0.0), num(y, "duration_min", 0.0)))
    });
    if let Some(l) = limit {
        if l > 0 && opts.len() > l {
            opts.truncate(l);
        }
    }
    opts
}

/// 把 prefer 归一化成 `(Some("mode"), key)` / `(Some("strategy"), name)` / `(None, None)`。
pub fn normalize_prefer(prefer: Option<&str>) -> (Option<&'static str>, Option<String>) {
    let raw_in = match prefer {
        Some(p) if !p.is_empty() => p,
        _ => return (None, None),
    };
    let low = raw_in.trim().to_lowercase();
    if let Some(v) = alias_mode(&low) {
        return (Some("mode"), Some(v.to_string()));
    }
    if let Some(v) = alias_strategy(&low) {
        return (Some("strategy"), Some(v.to_string()));
    }
    // 中文别名与大小写无关，再按原样查一次（与原型一致）
    let raw = raw_in.trim();
    if let Some(v) = alias_mode(raw) {
        return (Some("mode"), Some(v.to_string()));
    }
    if let Some(v) = alias_strategy(raw) {
        return (Some("strategy"), Some(v.to_string()));
    }
    (None, None)
}

/// 路径规划主入口。
///
/// * `from`/`to`   — (lng, lat)
/// * `prefer`      — None 自动推荐；交通工具名（中英）强制指定；策略词见 [`STRATEGY_ALIASES`]
/// * `cross_water` — Some(true/false) 强制；None = 用已注册的水域检测器自动判断
/// * `context`     — 可选小区布局上下文（[`makes_context`] 构造）→ 接驳段吸附真实站点
///
/// 返回 `{mode, distance_m, duration_min, steps[], cost, transfer_count, ...}`。
///
/// 与 Python 的一处刻意差异：原型在策略分支里 `best = opts[k]` 后又 `best['options'] = opts`，
/// 于是 best 自己出现在自己的 options 里，形成**自引用环**——`json.dumps` 会直接报
/// "Circular reference detected"。`serde_json::Value` 是树，本来就表达不了环，
/// 这里把 options 的**副本**放进 best（best 在 options 里是没带 strategy/options 的干净版本），
/// 字段名与取值不变，但结果一定可序列化。
#[allow(clippy::too_many_arguments)]
pub fn plan_route(
    from_lnglat: (f64, f64),
    to_lnglat: (f64, f64),
    prefer: Option<&str>,
    cross_water: Option<bool>,
    urban: Option<bool>,
    sort: &str,
    context: Option<&Value>,
) -> Value {
    let a = round_pair(from_lnglat);
    let b = round_pair(to_lnglat);
    let d = haversine_m(a, b);
    let cw = cross_water.unwrap_or_else(|| detect_cross_water(a, b));
    let (kind, val) = normalize_prefer(prefer);

    if kind == Some("mode") {
        let key = val.unwrap_or_else(|| "walk".to_string());
        let why = format!("按指定方式 {}（{}）", mode_of(&key).name, fmt_distance(d));
        return build_route(&key, a, b, Some(d), cw && key == "ferry", &why, context);
    }

    if kind == Some("strategy") {
        let sv = val.unwrap_or_else(|| "balanced".to_string());
        let opts = if sv == "public" {
            let pub_only: &[&str] = &PUBLIC_MODES;
            let o = plan_options(a, b, Some(cw), urban, "balanced", Some(pub_only), None, context);
            if o.is_empty() {
                plan_options(a, b, Some(cw), urban, "balanced", None, None, context)
            } else {
                o
            }
        } else {
            plan_options(a, b, Some(cw), urban, &sv, None, None, context)
        };
        // Python 的 min() 在同分时取**第一个**；Rust 的 Iterator::min_by 取最后一个，
        // 所以这里手写 fold，严格小于才替换，保持与原型一致。
        let mut best: Option<Value> = None;
        let mut best_score = f64::INFINITY;
        for o in &opts {
            let s = score_route(o, &sv);
            if best.is_none() || s < best_score {
                best_score = s;
                best = Some(o.clone());
            }
        }
        let mut best = match best {
            Some(v) => v,
            None => return build_route("walk", a, b, Some(d), false, "", context),
        };
        let names = match sv.as_str() {
            "fastest" => "最快",
            "cheapest" => "最便宜",
            "balanced" => "综合最优",
            "public" => "公共交通",
            _ => "推荐",
        };
        let mode_name = best
            .get("mode_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(obj) = best.as_object_mut() {
            obj.insert(
                "reason".to_string(),
                json!(format!("{names}：{} · {mode_name}", fmt_distance(d))),
            );
            obj.insert("strategy".to_string(), json!(sv));
            obj.insert("options".to_string(), Value::Array(opts));
        }
        return best;
    }

    let (mode_key, why) = recommend_mode(d, cw, urban);
    let mut route = build_route(&mode_key, a, b, Some(d), cw && mode_key == "ferry", &why, context);
    let options = plan_options(a, b, Some(cw), urban, sort, None, None, context);
    if let Some(obj) = route.as_object_mut() {
        obj.insert("strategy".to_string(), json!("auto"));
        obj.insert("options".to_string(), Value::Array(options));
    }
    route
}

// ══════════════════════════════════════════════════════════════════════════
// 八、出行状态机
// ══════════════════════════════════════════════════════════════════════════

/// trip id 里的全局自增序号（对应 Python 的模块级 `_TRIP_SEQ`）。
/// 注意这是进程级状态：同一次运行里每个 trip 的 id 都不同，但**同一输入 + 同一时刻**
/// 的行程内容（除 id 外）完全一致，可对拍。
static TRIP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 把路线铺平成时间轴 phases：
/// wait(候车/安检) → [transfer(换乘等待)…] → ride(各段行程) → …
/// 状态机只需沿时间轴推进，即可算出当前位置。
pub fn build_timeline(route: &Value) -> Vec<Value> {
    let steps: Vec<Value> = route
        .get("steps")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut phases: Vec<Value> = Vec::new();
    let overhead = num(route, "overhead_min", 0.0);
    let from = pair_of(route.get("from").unwrap_or(&Value::Null), (0.0, 0.0));
    if overhead > 0.0 {
        phases.push(json!({
            "kind": "wait", "mode": Value::Null, "mode_name": "候车/安检",
            "at": ll(from), "duration_min": round_n(overhead, 2),
            "distance_m": 0.0, "note": "候车/安检/值机登机",
        }));
    }
    // 主段 = 距离最长的一段（同长取第一个，与 Python max() 一致），
    // 换乘等待集中在主段起点（车站/机场）
    let mut main_i = 0usize;
    let mut main_d = f64::NEG_INFINITY;
    for (i, s) in steps.iter().enumerate() {
        let dist = num(s, "distance_m", 0.0);
        if dist > main_d {
            main_d = dist;
            main_i = i;
        }
    }
    let tc = num(route, "transfer_count", 0.0) as i64;
    let tp = num(route, "transfer_penalty_min", 0.0);
    for (i, st) in steps.iter().enumerate() {
        if i == main_i && tc > 0 && tp > 0.0 {
            let at = ll(pair_of(st.get("from").unwrap_or(&Value::Null), (0.0, 0.0)));
            for k in 0..tc {
                phases.push(json!({
                    "kind": "transfer", "mode": Value::Null, "mode_name": "换乘",
                    "at": at, "duration_min": round_n(tp, 2), "distance_m": 0.0,
                    "note": format!("第{}次换乘等待", k + 1),
                }));
            }
        }
        let st_mode = st.get("mode").and_then(|v| v.as_str()).unwrap_or("");
        // Python 的 `or` 把 0/None 都当假 → 回落到参数表速度
        let speed = st
            .get("speed_kmh")
            .and_then(|v| v.as_f64())
            .filter(|v| *v != 0.0)
            .unwrap_or_else(|| mode_of(st_mode).speed_kmh);
        phases.push(json!({
            "kind": "ride",
            "mode": st.get("mode").cloned().unwrap_or(Value::Null),
            "mode_name": st.get("mode_name").cloned().unwrap_or(Value::Null),
            "icon": st.get("icon").cloned().unwrap_or(Value::Null),
            "color": st.get("color").cloned().unwrap_or(Value::Null),
            "from": st.get("from").cloned().unwrap_or(Value::Null),
            "to": st.get("to").cloned().unwrap_or(Value::Null),
            "duration_min": num(st, "duration_min", 0.0),
            "distance_m": num(st, "distance_m", 0.0),
            "speed_kmh": speed,
            "note": st.get("note").and_then(|v| v.as_str()).unwrap_or(""),
            "is_main": i == main_i,
        }));
    }
    if phases.is_empty() {
        phases.push(json!({
            "kind": "wait", "mode": Value::Null, "mode_name": "原地",
            "at": ll(from), "duration_min": 0.0, "distance_m": 0.0, "note": "",
        }));
    }
    phases
}

/// 开始一次出行 → trip 对象。`time_scale > 1` 可让游戏时间加速（[`tick_to_now`] 时生效）。
pub fn start_trip(route: &Value, now: Option<f64>, label: &str, time_scale: f64) -> Value {
    let now = now.unwrap_or_else(now_secs);
    let phases: Vec<Value> = route
        .get("phases")
        .and_then(|v| v.as_array())
        .filter(|a| !a.is_empty())
        .cloned()
        .unwrap_or_else(|| build_timeline(route));
    let total = py_sum(phases.iter().map(|p| num(p, "duration_min", 0.0)));
    let seq = TRIP_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    // 起点若是候车/换乘期，状态先置 waiting
    let mut status = "moving";
    if let Some(first) = phases.first() {
        if first.get("kind").and_then(|v| v.as_str()) != Some("ride") {
            status = "waiting";
        }
    }
    let from = pair_of(route.get("from").unwrap_or(&Value::Null), (0.0, 0.0));
    let to = pair_of(route.get("to").unwrap_or(&Value::Null), (0.0, 0.0));
    let distance_m = num(route, "distance_m", 0.0);
    json!({
        "id": format!("trip_{}_{}", (now * 1000.0) as i64, seq),
        "label": label,
        "mode": route.get("mode").cloned().unwrap_or(Value::Null),
        "mode_name": route.get("mode_name").cloned().unwrap_or(Value::Null),
        "icon": route.get("icon").cloned().unwrap_or(Value::Null),
        "color": route.get("color").cloned().unwrap_or(Value::Null),
        "from": ll(from), "to": ll(to),
        "distance_m": distance_m,
        "cost": num(route, "cost", 0.0),
        "summary": route.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
        "phases": phases,
        "total_duration_min": round_n(total, 2),
        "elapsed_sec": 0.0, "elapsed_min": 0.0,
        "remaining_min": round_n(total, 2), "remaining_m": distance_m,
        "distance_done_m": 0.0,
        "position": ll(from), "status": status, "finished": false,
        "progress": 0.0, "current_phase": 0, "phase_progress": 0.0,
        "current_speed_kmh": 0.0,
        "started_at": now, "last_tick_at": now, "time_scale": time_scale,
        "eta_at": round_n(now + total * 60.0, 1),
    })
}

/// 写回字段：不是对象就什么也不做（Python 是 `trip.update({...})`）
fn set(v: &mut Value, k: &str, val: Value) {
    if let Some(obj) = v.as_object_mut() {
        obj.insert(k.to_string(), val);
    }
}

/// 按经过的秒数推进出行进度，原地更新 trip。
/// 前端渲染所需：position(lng,lat)、status(moving/waiting/arrived)、progress、
/// remaining_min、current_speed_kmh。
pub fn tick_trip(trip: &mut Value, elapsed_sec: f64) {
    if trip.get("finished").and_then(|v| v.as_bool()).unwrap_or(false) {
        return;
    }
    let dt = 0.0_f64.max(elapsed_sec); // NaN → 0（Python 的 max(0.0, nan) 同样返回 0.0）
    let elapsed_sec = num(trip, "elapsed_sec", 0.0) + dt;
    let em = elapsed_sec / 60.0;
    let phases: Vec<Value> = trip
        .get("phases")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let total = num(trip, "total_duration_min", 0.0);
    set(trip, "elapsed_sec", json!(elapsed_sec));

    if total <= 0.0 || em >= total {
        let done = py_sum(phases.iter().map(|p| num(p, "distance_m", 0.0)));
        let to = pair_of(trip.get("to").unwrap_or(&Value::Null), (0.0, 0.0));
        set(trip, "elapsed_min", json!(round_n(em.min(total), 3)));
        set(trip, "status", json!("arrived"));
        set(trip, "finished", json!(true));
        set(trip, "position", ll(to));
        set(trip, "remaining_min", json!(0.0));
        set(trip, "remaining_m", json!(0.0));
        set(trip, "distance_done_m", json!(round_n(done, 1)));
        set(trip, "progress", json!(1.0));
        // Python 是 len(phases)-1（空列表会得到 -1）；这里夹到 0，避免负下标
        set(trip, "current_phase", json!(phases.len().saturating_sub(1)));
        set(trip, "phase_progress", json!(1.0));
        set(trip, "current_speed_kmh", json!(0.0));
        return;
    }

    let mut acc = 0.0;
    let mut cur = phases.len().saturating_sub(1);
    let mut frac = 1.0;
    for (i, p) in phases.iter().enumerate() {
        let d = num(p, "duration_min", 0.0);
        if em < acc + d {
            cur = i;
            frac = if d > 0.0 { (em - acc) / d } else { 1.0 };
            break;
        }
        acc += d;
    }
    let p = match phases.get(cur) {
        Some(p) => p,
        None => return, // 没有 phases 的畸形 trip：不推进也不 panic
    };
    let kind = p.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let trip_distance_m = num(trip, "distance_m", 0.0);

    let mut done = py_sum((0..cur).map(|i| num(&phases[i], "distance_m", 0.0)));
    let (pos, status, speed);
    if kind == "ride" {
        done += num(p, "distance_m", 0.0) * frac;
        let a = pair_of(p.get("from").unwrap_or(&Value::Null), (0.0, 0.0));
        let b = pair_of(p.get("to").unwrap_or(&Value::Null), (0.0, 0.0));
        pos = (a.0 + (b.0 - a.0) * frac, a.1 + (b.1 - a.1) * frac);
        status = "moving";
        speed = num(p, "speed_kmh", 0.0);
    } else {
        let at = p.get("at").cloned().unwrap_or(Value::Null);
        pos = if at.is_array() {
            pair_of(&at, (0.0, 0.0))
        } else {
            pair_of(trip.get("from").unwrap_or(&Value::Null), (0.0, 0.0))
        };
        status = "waiting";
        speed = 0.0;
    }

    set(trip, "elapsed_min", json!(round_n(em, 3)));
    set(trip, "remaining_min", json!(round_n(total - em, 2)));
    set(trip, "remaining_m", json!(round_n((trip_distance_m - done).max(0.0), 1)));
    set(trip, "distance_done_m", json!(round_n(done, 1)));
    set(trip, "position", ll(pos));
    set(trip, "progress", json!(round_n(em / total, 4)));
    set(trip, "current_phase", json!(cur));
    set(trip, "phase_progress", json!(round_n(frac, 4)));
    set(
        trip,
        "current_phase_kind",
        p.get("kind").cloned().unwrap_or(Value::Null),
    );
    set(trip, "current_mode", p.get("mode").cloned().unwrap_or(Value::Null));
    set(trip, "status", json!(status));
    set(trip, "current_speed_kmh", json!(speed));
}

/// 按真实时间推进（游戏循环/前端轮询用），也支持 `time_scale` 加速。
pub fn tick_to_now(trip: &mut Value, now: Option<f64>) {
    let now = now.unwrap_or_else(now_secs);
    let last = trip
        .get("last_tick_at")
        .and_then(|v| v.as_f64())
        .filter(|v| *v != 0.0)
        .or_else(|| {
            trip.get("started_at")
                .and_then(|v| v.as_f64())
                .filter(|v| *v != 0.0)
        })
        .unwrap_or(now);
    let scale = trip
        .get("time_scale")
        .and_then(|v| v.as_f64())
        .filter(|v| *v != 0.0)
        .unwrap_or(1.0);
    set(trip, "last_tick_at", json!(now));
    tick_trip(trip, (now - last) * scale);
}

/// 去重相邻重复点并取整到 6 位
fn dedupe(pts: Vec<(f64, f64)>) -> Value {
    let mut out: Vec<(f64, f64)> = Vec::new();
    for p in pts {
        let dup = out
            .last()
            .map(|l| l.0 == p.0 && l.1 == p.1)
            .unwrap_or(false);
        if !dup {
            out.push(round_pair(p));
        }
    }
    Value::Array(out.into_iter().map(|p| json!([p.0, p.1])).collect())
}

/// 返回 {done, remain} 两段折线（地理坐标），供前端画「已走/剩余」虚实线。
pub fn trip_polyline(trip: &Value) -> Value {
    let phases: Vec<Value> = trip
        .get("phases")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let from = pair_of(trip.get("from").unwrap_or(&Value::Null), (0.0, 0.0));
    let to = pair_of(trip.get("to").unwrap_or(&Value::Null), (0.0, 0.0));
    let position = pair_of(trip.get("position").unwrap_or(&Value::Null), from);
    let kind_of = |p: &Value| p.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let pt = |p: &Value, k: &str| pair_of(p.get(k).unwrap_or(&Value::Null), from);

    if trip.get("finished").and_then(|v| v.as_bool()).unwrap_or(false) {
        let mut done = vec![from];
        for p in &phases {
            if kind_of(p) == "ride" {
                done.push(pt(p, "to"));
            }
        }
        return json!({"done": dedupe(done), "remain": dedupe(vec![to])});
    }

    let cur = trip.get("current_phase").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
    let mut done = vec![from];
    for (i, p) in phases.iter().enumerate() {
        if kind_of(p) != "ride" {
            continue;
        }
        if i < cur {
            done.push(pt(p, "to"));
        } else if i == cur {
            done.push(position);
        }
    }
    let mut remain = vec![position];
    for (i, p) in phases.iter().enumerate() {
        if kind_of(p) != "ride" {
            continue;
        }
        if i > cur {
            remain.push(pt(p, "from"));
            remain.push(pt(p, "to"));
        } else if i == cur {
            remain.push(pt(p, "to"));
        }
    }
    remain.push(to);
    json!({"done": dedupe(done), "remain": dedupe(remain)})
}

/// trip → 前端可直接用的精简 JSON（含折线与文案，省得前端再算）。
/// 传入空对象/Null 时返回 None（对应 Python `if not trip: return None`）。
pub fn trip_payload(trip: &Value) -> Option<Value> {
    let obj = trip.as_object()?;
    if obj.is_empty() {
        return None;
    }
    let phases: Vec<Value> = trip
        .get("phases")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let idx = trip
        .get("current_phase")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        .max(0) as usize;
    // Python 是 trip['phases'][current_phase]，越界会 IndexError；这里退化成空对象
    let ph = phases.get(idx).cloned().unwrap_or(Value::Null);
    let remaining_min = num(trip, "remaining_min", 0.0);
    let remaining_m = num(trip, "remaining_m", 0.0);
    let distance_m = num(trip, "distance_m", 0.0);
    let cost = num(trip, "cost", 0.0);
    let slim: Vec<Value> = phases
        .iter()
        .map(|p| {
            json!({
                "kind": p.get("kind").cloned().unwrap_or(Value::Null),
                "mode": p.get("mode").cloned().unwrap_or(Value::Null),
                "mode_name": p.get("mode_name").cloned().unwrap_or(Value::Null),
                "icon": p.get("icon").cloned().unwrap_or(Value::Null),
                "color": p.get("color").cloned().unwrap_or(Value::Null),
                "from": p.get("from").cloned().unwrap_or(Value::Null),
                "to": p.get("to").cloned().unwrap_or(Value::Null),
                "at": p.get("at").cloned().unwrap_or(Value::Null),
                "duration_min": p.get("duration_min").cloned().unwrap_or(Value::Null),
                "distance_m": p.get("distance_m").cloned().unwrap_or(Value::Null),
                "note": p.get("note").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();
    let phase_count = phases.len();
    Some(json!({
        "id": trip.get("id").cloned().unwrap_or(Value::Null),
        "label": trip.get("label").and_then(|v| v.as_str()).unwrap_or(""),
        "mode": trip.get("mode").cloned().unwrap_or(Value::Null),
        "mode_name": trip.get("mode_name").cloned().unwrap_or(Value::Null),
        "icon": trip.get("icon").cloned().unwrap_or(Value::Null),
        "color": trip.get("color").cloned().unwrap_or(Value::Null),
        "from": trip.get("from").cloned().unwrap_or(Value::Null),
        "to": trip.get("to").cloned().unwrap_or(Value::Null),
        "position": trip.get("position").cloned().unwrap_or(Value::Null),
        "status": trip.get("status").cloned().unwrap_or(Value::Null),
        "finished": trip.get("finished").cloned().unwrap_or(json!(false)),
        "progress": trip.get("progress").cloned().unwrap_or(json!(0.0)),
        "elapsed_min": trip.get("elapsed_min").cloned().unwrap_or(json!(0.0)),
        "total_duration_min": trip.get("total_duration_min").cloned().unwrap_or(json!(0.0)),
        "remaining_min": remaining_min, "remaining_text": fmt_duration(remaining_min),
        "remaining_m": remaining_m, "remaining_text_dist": fmt_distance(remaining_m),
        "distance_m": distance_m, "distance_text": fmt_distance(distance_m),
        "cost": cost, "cost_text": fmt_cost(cost),
        "current_speed_kmh": trip.get("current_speed_kmh").cloned().unwrap_or(json!(0.0)),
        "current_phase": trip.get("current_phase").cloned().unwrap_or(json!(0)),
        "current_phase_kind": ph.get("kind").cloned().unwrap_or(Value::Null),
        "current_phase_note": ph.get("note").and_then(|v| v.as_str()).unwrap_or(""),
        "phase_progress": trip.get("phase_progress").cloned().unwrap_or(json!(0.0)),
        "phase_count": phase_count,
        "summary": trip.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
        "started_at": trip.get("started_at").cloned().unwrap_or(Value::Null),
        "eta_at": trip.get("eta_at").cloned().unwrap_or(Value::Null),
        "polyline": trip_polyline(trip),
        "phases": slim,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// 九、HTTP 适配层（给 hier_api / axum 路由用，本模块不自己起服务）
// ══════════════════════════════════════════════════════════════════════════

/// 取查询参数，兼容 parse_qs 的 `{k:[v]}` 与普通 `{k:v}`
fn q(params: &Value, key: &str) -> String {
    match params.get(key) {
        Some(Value::Array(a)) => a.first().map(value_to_string).unwrap_or_default(),
        Some(Value::Null) | None => String::new(),
        Some(v) => value_to_string(v),
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// `'1'|'true'|'yes'|'on'` → true；空串 → None（不强制，交给检测器）
fn opt_bool(s: &str) -> Option<bool> {
    if s.is_empty() {
        return None;
    }
    Some(matches!(s.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

/// GET /api/transport_plan?from_lng=&from_lat=&to_lng=&to_lat=&prefer=&water=&urban=
/// 返回 `{ok, route, options, modes}` 或 `{ok:false, error}`。
/// 规划 + 顺带给出方案列表，前端一次拿全。
pub fn api_plan(params: &Value) -> Value {
    let flng = q(params, "from_lng").parse::<f64>();
    let flat = q(params, "from_lat").parse::<f64>();
    let tlng = q(params, "to_lng").parse::<f64>();
    let tlat = q(params, "to_lat").parse::<f64>();
    let (a, b) = match (flng, flat, tlng, tlat) {
        (Ok(x1), Ok(y1), Ok(x2), Ok(y2)) => ((x1, y1), (x2, y2)),
        _ => {
            return json!({"ok": false, "error": "缺少或非法坐标，需要 from_lng/from_lat/to_lng/to_lat"})
        }
    };
    let prefer = q(params, "prefer");
    let prefer = if prefer.is_empty() { None } else { Some(prefer) };
    let water = opt_bool(&q(params, "water"));
    let urban = opt_bool(&q(params, "urban"));
    // Python 这里包了一层 try/except 返回 {ok:false,error}；Rust 版规划路径不会失败，
    // 所以异常分支换成了上面的坐标解析失败分支
    let route = plan_route(a, b, prefer.as_deref(), water, urban, "balanced", None);
    let options = plan_options(a, b, water, urban, "balanced", None, Some(5), None);
    json!({"ok": true, "route": route, "options": options, "modes": modes_table()})
}

/// GET /api/transport_modes → 9 种交通工具参数表
pub fn api_modes() -> Value {
    json!({"ok": true, "modes": modes_table()})
}

// ══════════════════════════════════════════════════════════════════════════
// 十、单元测试（对应 Python 原型 `__main__` 里那段自检）
// ══════════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    /// 全部测试都显式传 cross_water，避免踩到全局水域检测器（原型也是全局状态）
    fn auto(from: (f64, f64), to: (f64, f64)) -> Value {
        plan_route(from, to, None, Some(false), Some(true), "balanced", None)
    }

    #[test]
    fn haversine_known_distances() {
        let d = haversine_m(GUANGZHOU, BEIJING);
        assert!(
            d > 1_850_000.0 && d < 1_930_000.0,
            "广州→北京直线距离实测 {:.1} km（真实约 1888km）",
            d / 1000.0
        );
        assert!(haversine_m(GUANGZHOU, GUANGZHOU) < 1e-6, "同点距离应为 0");
        let one_deg = haversine_m((113.0, 23.0), (113.0, 24.0));
        assert!(
            one_deg > 110_800.0 && one_deg < 111_600.0,
            "1 度纬度实测 {:.2} km",
            one_deg / 1000.0
        );
        // 对称性
        let back = haversine_m(BEIJING, GUANGZHOU);
        assert!((back - d).abs() < 1e-6, "距离应对称");
    }

    #[test]
    fn prefer_alias_resolution() {
        assert_eq!(
            normalize_prefer(Some("地铁")),
            (Some("mode"), Some("subway".to_string()))
        );
        assert_eq!(
            normalize_prefer(Some("metro")),
            (Some("mode"), Some("subway".to_string()))
        );
        assert_eq!(
            normalize_prefer(Some("METRO")),
            (Some("mode"), Some("subway".to_string())),
            "别名解析应大小写无关"
        );
        assert_eq!(
            normalize_prefer(Some("高铁")),
            (Some("mode"), Some("train".to_string()))
        );
        assert_eq!(
            normalize_prefer(Some("飞机")),
            (Some("mode"), Some("plane".to_string()))
        );
        assert_eq!(
            normalize_prefer(Some("打车")),
            (Some("mode"), Some("taxi".to_string()))
        );
        assert_eq!(
            normalize_prefer(Some("自行车")),
            (Some("mode"), Some("bike".to_string()))
        );
        assert_eq!(
            normalize_prefer(Some("  walk  ")),
            (Some("mode"), Some("walk".to_string())),
            "两端空白应被 strip"
        );
        assert_eq!(
            normalize_prefer(Some("fastest")),
            (Some("strategy"), Some("fastest".to_string()))
        );
        assert_eq!(
            normalize_prefer(Some("公共交通")),
            (Some("strategy"), Some("public".to_string()))
        );
        assert_eq!(
            normalize_prefer(Some("auto")),
            (Some("strategy"), Some("balanced".to_string()))
        );
        assert_eq!(normalize_prefer(Some("自行车手")), (None, None));
        assert_eq!(normalize_prefer(Some("")), (None, None));
        assert_eq!(normalize_prefer(None), (None, None));
        // 强制指定真的生效
        assert_eq!(plan_route(GUANGZHOU, SHENZHEN, Some("地铁"), Some(false), Some(true), "balanced", None)["mode"], json!("subway"));
        assert_eq!(plan_route(GUANGZHOU, SHENZHEN, Some("飞机"), Some(false), Some(true), "balanced", None)["mode"], json!("plane"));
    }

    #[test]
    fn modes_table_matches_python_shape() {
        let t = modes_table();
        assert_eq!(t.len(), 9, "交通工具数量 = 9");
        assert_eq!(t[0]["key"], json!("walk"));
        assert_eq!(t[0]["range_text"], json!("0~2 km"));
        assert_eq!(t[1]["range_text"], json!("0.3~8 km"));
        assert_eq!(t[6]["key"], json!("train"));
        assert_eq!(t[6]["range_text"], json!("30~1500 km"));
        assert_eq!(t[7]["range_text"], json!("250~12000 km"));
        for row in &t {
            for k in [
                "key", "name", "en", "icon", "color", "speed_kmh", "min_km", "max_km",
                "range_text", "need_transfer", "desc", "fare_note", "fare_base",
                "fare_per_km", "overhead_min", "access_km", "access_mode",
            ] {
                assert!(row.get(k).is_some(), "参数表缺字段 {k}");
            }
            assert!(row["speed_kmh"].as_f64().unwrap() > 0.0);
            assert!(row["max_km"].as_f64().unwrap() > row["min_km"].as_f64().unwrap());
        }
        assert_eq!(modes_json().as_object().unwrap().len(), 9);
        assert_eq!(aliases_json()["高铁"], json!("train"));
        assert_eq!(strategy_aliases_json()["省钱"], json!("cheapest"));
        // 票价模型抽查：公交一票制 2 元、地铁起步 3 元（含 6km）
        assert_eq!(fare_of("bus", 12.0), 2.0);
        assert_eq!(fare_of("subway", 4.0), 3.0);
        assert_eq!(fare_of("subway", 16.0), 5.5);
        // 格式化
        assert_eq!(fmt_duration(45.0), "45分钟");
        assert_eq!(fmt_duration(80.0), "1小时20分");
        assert_eq!(fmt_duration(180.0), "3小时");
        assert_eq!(fmt_distance(850.0), "850米");
        assert_eq!(fmt_distance(104_100.0), "104.1公里");
        assert_eq!(fmt_cost(0.0), "免费");
        assert_eq!(fmt_cost(9.5), "¥9.5");
        assert_eq!(fmt_cost(88.94), "¥89");
        assert_eq!(transfer_text(0, true), "直达");
        assert_eq!(transfer_text(0, false), "无需换乘");
        assert_eq!(transfer_text(3, true), "换乘3次");
    }

    #[test]
    fn sum_matches_python312_neumaier() {
        // Python 3.12 起 sum() 对 float 用 Neumaier 补偿求和；移植必须跟着走，
        // 否则偶尔会在时长文案上差 1 分钟（本机实测就是这么发现的）
        assert_eq!(py_sum([1.3, 32.9, 1.3]), 35.5);
        assert_ne!(0.0 + 1.3 + 32.9 + 1.3, 35.5, "朴素累加会差 1e-14");
        assert_eq!(py_sum(Vec::<f64>::new()), 0.0);
        assert_eq!(py_sum([1.0, 2.0, 3.0]), 6.0);
        // 该方案原始总时长恰好 39.5 分钟：银行家舍入 → 40分钟（与原型一致）
        let r = build_route("taxi", GUANGZHOU, GZ_SOUTH, None, false, "", None);
        assert_eq!(r["duration_min"], json!(39.5));
        assert_eq!(r["duration_text"], json!("40分钟"));
        let opts = plan_options(GUANGZHOU, GZ_SOUTH, Some(false), Some(true), "balanced", None, None, None);
        let taxi = opts.iter().find(|o| o["mode"] == json!("taxi")).expect("出租车应在候选里");
        assert_eq!(taxi["duration_text"], json!("40分钟"));
    }

    #[test]
    fn bus_transfer_feasibility() {
        // 广深 ~104km 坐公交：每 8km 换乘一次 → 十几次换乘，等待时间远超高铁
        let r = build_route("bus", GUANGZHOU, SHENZHEN, None, false, "", None);
        let tc = r["transfer_count"].as_i64().unwrap();
        assert!(tc > 5, "长途公交应有多次换乘，实际 {tc}");
        assert_eq!(r["need_transfer"], json!(true));
        assert_eq!(r["transfer_text"], json!(format!("换乘{tc}次")));
        assert!(r["wait_min"].as_f64().unwrap() > 30.0, "长途公交换乘等待应很长");
        assert_eq!(r["speed_kmh"], json!(20.0));

        // 短途公交（0.6km）不该产生换乘 → 直达
        let a = (113.2640, 23.1290);
        let b = (113.2700, 23.1290);
        let s = build_route("bus", a, b, None, false, "", None);
        assert_eq!(s["transfer_count"], json!(0));
        assert_eq!(s["transfer_text"], json!("直达"));
        // 步行没有换乘概念
        let w = build_route("walk", a, b, None, false, "", None);
        assert_eq!(w["transfer_text"], json!("无需换乘"));
        assert_eq!(w["cost"], json!(0.0));
        // 公交换乘可行性：候选列表里公交只在适用距离内出现
        let far = applicable_modes(104_000.0, false, Some(false), None);
        assert!(!far.contains(&"bus".to_string()), "104km 不该推荐公交：{far:?}");
        assert!(far.contains(&"train".to_string()));
    }

    #[test]
    fn cross_water_hook_and_ferry() {
        let a = XUWEN;
        let b = HAIKOU;
        // 未注册检测器时保守返回 false（与 Python 一致）
        assert!(!detect_cross_water(a, b));
        let no_checker: Option<&fn((f64, f64), (f64, f64)) -> bool> = None;
        assert!(!detect_cross_water_with(no_checker, a, b));
        let yes = |_a: (f64, f64), _b: (f64, f64)| true;
        assert!(detect_cross_water_with(Some(&yes), a, b));
        let d = haversine_m(a, b);
        let (mk, why) = recommend_mode(d, true, None);
        assert_eq!(mk, "ferry", "跨水优先轮船");
        assert!(why.contains("跨水域"), "理由文案应提到跨水域：{why}");
        let r = plan_route(a, b, None, Some(true), None, "balanced", None);
        assert_eq!(r["mode"], json!("ferry"));
        assert_eq!(r["cross_water"], json!(true));
        let dur = r["duration_min"].as_f64().unwrap();
        assert!((60.0..=180.0).contains(&dur), "跨海门到门 1~3 小时，实际 {dur} 分");
        // 显式 cross_water=false 时轮渡不该出现（distance ~29km 落在其它工具的范围内）
        let no_water = applicable_modes(d, false, Some(false), None);
        assert!(!no_water.contains(&"ferry".to_string()), "没跨水不该有轮渡：{no_water:?}");

        // 注册检测器后 cross_water=None 也能自动判出来
        let checker: WaterChecker = Box::new(|_a: (f64, f64), _b: (f64, f64)| true);
        set_water_checker(Some(checker));
        assert!(water_checker_registered());
        assert!(detect_cross_water(a, b));
        let auto_opts = plan_options(a, b, None, None, "balanced", None, None, None);
        assert!(
            auto_opts.iter().any(|o| o["mode"] == json!("ferry")),
            "注册检测器后轮渡应进候选"
        );
        set_water_checker(None);
        assert!(!water_checker_registered());
        assert!(!detect_cross_water(a, b));
    }

    #[test]
    fn same_endpoints_are_deterministic() {
        let r1 = plan_route(GUANGZHOU, SHENZHEN, None, Some(false), Some(true), "balanced", None);
        let r2 = plan_route(GUANGZHOU, SHENZHEN, None, Some(false), Some(true), "balanced", None);
        assert_eq!(r1, r2, "同一对起终点两次规划必须完全一致");
        let o1 = plan_options(GZ_TA, GZ_TIANHE, Some(false), Some(true), "balanced", None, None, None);
        let o2 = plan_options(GZ_TA, GZ_TIANHE, Some(false), Some(true), "balanced", None, None, None);
        assert_eq!(o1, o2);
        // 结果里不该出现 NaN/Inf（serde_json 会把它们写成 null，属于隐性破坏）
        assert!(r1["duration_min"].as_f64().unwrap().is_finite());
        assert!(r1["cost"].as_f64().unwrap().is_finite());

        // trip：id 带全局自增序号（对应 Python 的 _TRIP_SEQ），所以去掉 id 再比
        let mut t1 = start_trip(&r1, Some(1_700_000_000.0), "通勤", 1.0);
        let mut t2 = start_trip(&r1, Some(1_700_000_000.0), "通勤", 1.0);
        assert!(t1["id"].as_str().unwrap().starts_with("trip_1700000000000_"));
        t1.as_object_mut().unwrap().remove("id");
        t2.as_object_mut().unwrap().remove("id");
        assert_eq!(t1, t2, "同一路线 + 同一时刻的 trip 必须一致（除 id 外）");
        tick_trip(&mut t1, 600.0);
        tick_trip(&mut t2, 600.0);
        assert_eq!(t1["position"], t2["position"]);
        assert_eq!(t1["progress"], t2["progress"]);
    }

    #[test]
    fn extreme_inputs_do_not_panic() {
        let nan = f64::NAN;
        let inf = f64::INFINITY;
        // NaN 坐标：与 Python 的 min(1.0, sqrt(s)) 同语义（返回半个大圆周长），不是 NaN
        let d = haversine_m((nan, nan), (1.0, 2.0));
        assert!(d.is_finite(), "NaN 坐标按原型语义应得有限值，实际 {d}");
        let _ = haversine_m((inf, inf), (-inf, 0.0));
        // 非法/极端起终点
        for (a, b) in [
            ((nan, nan), (inf, -inf)),
            ((0.0, 0.0), (0.0, 0.0)),
            ((-180.0, -90.0), (180.0, 90.0)),
            ((1e9, 1e9), (-1e9, -1e9)),
        ] {
            let _ = plan_route(a, b, None, Some(false), None, "balanced", None);
            let _ = plan_options(a, b, Some(false), None, "fastest", None, Some(3), None);
        }
        // 未知 prefer / 未知方式 / 未知排序词
        let r = plan_route(GUANGZHOU, SHENZHEN, Some("传送门"), Some(false), None, "随便", None);
        assert!(r.get("mode").is_some(), "未知 prefer 应退回自动推荐");
        let unknown = build_route("hoverboard", (0.0, 0.0), (1.0, 1.0), None, false, "", None);
        assert!(unknown["steps"].as_array().unwrap().len() >= 1, "未知方式退化成 walk 而不是 panic");
        let _ = fare_of("hoverboard", 10.0);
        let _ = ride_minutes("hoverboard", 10_000.0);
        // 未知 key 混进 only：跳过（Python 会 KeyError）
        let only: &[&str] = &["ferry", "hoverboard", "bus"];
        let got = applicable_modes(5000.0, false, Some(true), Some(only));
        assert!(!got.contains(&"hoverboard".to_string()));
        // 空 steps 的畸形路线：时间轴兜底一条「原地」等待
        let empty = json!({"from": [0.0, 0.0], "to": [1.0, 1.0], "steps": []});
        let phases = build_timeline(&empty);
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0]["kind"], json!("wait"));
        let mut t = start_trip(&empty, Some(0.0), "空行程", 1.0);
        tick_trip(&mut t, -5.0); // 倒退时间不推进
        assert_eq!(t["progress"], json!(1.0), "总时长为 0 的行程直接判到达");
        tick_trip(&mut t, nan); // NaN 步长被夹成 0
        tick_trip(&mut t, inf);
        assert!(trip_payload(&t).is_some());
        assert!(trip_polyline(&t).get("done").is_some());
        // 非对象入参
        tick_trip(&mut Value::Null, 10.0);
        tick_trip(&mut json!([]), 10.0);
        assert!(trip_payload(&Value::Null).is_none());
        assert!(trip_payload(&json!({})).is_none());
        assert!(find_station_in_grid(nan, nan, None, None).is_none());
        assert!(find_station_in_grid(113.0, 23.0, Some(&json!({})), None).is_none());
    }

    #[test]
    fn route_shape_and_step_continuity() {
        let r = auto(GUANGZHOU, SHENZHEN);
        assert_eq!(r["mode"], json!("train"), "广深 104km 应推荐高铁");
        for k in [
            "from", "to", "mode", "mode_name", "icon", "color", "distance_m", "distance_text",
            "duration_min", "duration_text", "ride_min", "wait_min", "cost", "cost_text",
            "transfer_count", "transfer_text", "transfer_penalty_min", "overhead_min",
            "need_transfer", "cross_water", "steps", "summary", "reason", "fare_note",
            "speed_kmh", "strategy", "options",
        ] {
            assert!(r.get(k).is_some(), "路线对象缺字段 {k}");
        }
        let steps = r["steps"].as_array().unwrap();
        assert!(!steps.is_empty());
        assert_eq!(steps[0]["from"], r["from"], "首段起点=路线起点");
        assert_eq!(steps[steps.len() - 1]["to"], r["to"], "末段终点=路线终点");
        let dur = r["duration_min"].as_f64().unwrap();
        assert!((80.0..=120.0).contains(&dur), "广深门到门 80~120 分钟，实际 {dur}");
        let cost = r["cost"].as_f64().unwrap();
        assert!((40.0..=95.0).contains(&cost), "广深高铁票价 40~95 元，实际 {cost}");
        assert!(
            r["summary"].as_str().unwrap().contains("火车/高铁"),
            "摘要应含主段工具：{}",
            r["summary"]
        );
        // 火车 access_mode 是 taxi：接驳 → 主段 → 接驳，三段工具依次是 出租/火车/出租
        let step_modes: Vec<&str> = steps.iter().map(|s| s["mode"].as_str().unwrap()).collect();
        assert_eq!(step_modes, vec!["taxi", "train", "taxi"]);
        assert_eq!(r["summary"], json!("出租车/网约车 → 火车/高铁 → 出租车/网约车"));

        // 步行短途：access=0 → 单段直达
        let w = plan_route(
            (113.2640, 23.1290),
            (113.2650, 23.1290),
            Some("步行"),
            Some(false),
            Some(true),
            "balanced",
            None,
        );
        assert_eq!(w["steps"].as_array().unwrap().len(), 1);
        assert!(w["steps"][0]["note"].as_str().unwrap().contains("直达"));
        assert_eq!(w["cost"], json!(0.0));
        assert_eq!(w["cost_text"], json!("免费"));

        // 广州塔 → 天河体育中心：方案列表 ≥ 3 条，且按 balanced 分数升序
        let opts = plan_options(GZ_TA, GZ_TIANHE, Some(false), Some(true), "balanced", None, None, None);
        assert!(opts.len() >= 3, "方案列表应 ≥ 3 条，实际 {}", opts.len());
        for w2 in opts.windows(2) {
            assert!(
                score_route(&w2[0], "balanced") <= score_route(&w2[1], "balanced") + 1e-9,
                "方案应按分数升序"
            );
        }
        // 3.4km 短途：推荐步行或公交（对应原型自检的断言范围）
        let short = auto(GZ_TA, GZ_TIANHE);
        assert!(
            short["mode"] == json!("walk") || short["mode"] == json!("bus"),
            "3.4km 应推荐步行/公交，实际 {}",
            short["mode"]
        );
        let sd = short["duration_min"].as_f64().unwrap();
        assert!((15.0..=35.0).contains(&sd), "3.4km 门到门 15~35 分钟，实际 {sd}");
    }

    #[test]
    fn urban_recommend_and_strategies() {
        assert!(is_urban(40_000.0, None));
        assert!(!is_urban(60_000.0, None));
        assert!(is_urban(60_000.0, Some(true)), "显式 urban 优先");
        assert!(!is_urban(1_000.0, Some(false)));
        assert_eq!(recommend_mode(500.0, false, None).0, "walk");
        assert_eq!(recommend_mode(3_000.0, false, None).0, "bus");
        assert_eq!(recommend_mode(10_000.0, false, None).0, "subway");
        assert_eq!(recommend_mode(20_000.0, false, None).0, "car", "都市圈外又未达铁路区间 → 自驾");
        assert_eq!(recommend_mode(100_000.0, false, None).0, "train");
        assert_eq!(recommend_mode(2_000_000.0, false, None).0, "plane");
        assert_eq!(recommend_mode(30_000.0, true, None).0, "ferry");

        // 策略：cheapest 不贵于 auto；public 只出公共交通工具
        let cheap = plan_route(GUANGZHOU, BEIJING, Some("cheapest"), Some(false), None, "balanced", None);
        let auto_r = plan_route(GUANGZHOU, BEIJING, Some("auto"), Some(false), None, "balanced", None);
        assert_eq!(cheap["strategy"], json!("cheapest"));
        assert!(
            cheap["cost"].as_f64().unwrap() <= auto_r["cost"].as_f64().unwrap() + 1e-6,
            "cheapest({}) 应不贵于 auto({})",
            cheap["cost"],
            auto_r["cost"]
        );
        assert!(cheap["reason"].as_str().unwrap().starts_with("最便宜"));
        assert!(cheap["options"].as_array().unwrap().len() >= 1);
        let pub_r = plan_route(GUANGZHOU, SHENZHEN, Some("公共交通"), Some(false), Some(true), "balanced", None);
        assert!(
            PUBLIC_MODES.contains(&pub_r["mode"].as_str().unwrap()),
            "public 策略只出公交/地铁/火车，实际 {}",
            pub_r["mode"]
        );
        assert_eq!(pub_r["strategy"], json!("public"));
        let fast = plan_route(GUANGZHOU, BEIJING, Some("fastest"), Some(false), None, "balanced", None);
        assert_eq!(fast["strategy"], json!("fastest"));
        assert_eq!(fast["mode"], json!("plane"), "广州→北京最快是飞机");
        // cheapest/fastest 的 options 必须可序列化（原型里 best['options']=opts 是自引用环）
        let s = serde_json::to_string(&fast).expect("策略分支结果必须能序列化");
        assert!(s.contains("\"strategy\":\"fastest\""));
        assert!(fast["options"][0].get("options").is_none(), "options 里的方案是干净副本");
    }

    #[test]
    fn trip_state_machine_progress() {
        let route = plan_route(GZ_TA, GZ_SOUTH, Some("地铁"), Some(false), Some(true), "balanced", None);
        let mut trip = start_trip(&route, Some(1_000_000.0), "上班通勤", 1.0);
        let total = trip["total_duration_min"].as_f64().unwrap();
        assert!(total > 0.0);
        assert!(trip["id"].as_str().unwrap().starts_with("trip_1000000000_"));
        assert_eq!(trip["status"], json!("waiting"), "地铁有候车/安检段，起始状态是 waiting");
        let phase_total = py_sum(
            trip["phases"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["duration_min"].as_f64().unwrap()),
        );
        assert!((phase_total - total).abs() < 0.05, "phases 总时长 == 路线总时长");

        let mut acc = 0.0;
        let mut last_prog = 0.0;
        for frac in [0.0_f64, 0.1, 0.25, 0.5, 0.75, 0.95, 1.0] {
            let target = total * frac;
            tick_trip(&mut trip, (target - acc) * 60.0);
            acc = target;
            let p = trip["progress"].as_f64().unwrap();
            assert!(p + 1e-9 >= last_prog, "进度单调不减：{last_prog} → {p}");
            last_prog = p;
        }
        assert!(trip["finished"].as_bool().unwrap(), "推进到底应 finished");
        assert_eq!(trip["status"], json!("arrived"));
        assert_eq!(trip["position"], route["to"], "到达时位置=终点");
        assert!((trip["progress"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(trip["remaining_min"], json!(0.0));
        let pl = trip_polyline(&trip);
        assert!(pl["done"].as_array().unwrap().len() >= 1);
        assert!(pl["remain"].as_array().unwrap().len() >= 1);
        assert_eq!(pl["remain"][0], route["to"]);

        // t=0 在起点、t=中点夹在起终点之间
        let mut t2 = start_trip(&route, Some(1_000_000.0), "", 1.0);
        tick_trip(&mut t2, 0.0);
        assert_eq!(t2["position"], t2["from"]);
        tick_trip(&mut t2, total * 60.0 / 2.0);
        assert_ne!(t2["position"], t2["from"]);
        assert_ne!(t2["position"], t2["to"]);
        assert_eq!(t2["status"], json!("moving"));
        assert!(t2["current_speed_kmh"].as_f64().unwrap() > 0.0);

        // tick_to_now 按真实时间推进（选一趟 >1 小时的行程，否则会被到达夹住）
        let long = auto(GUANGZHOU, BEIJING);
        let mut t3 = start_trip(&long, Some(1_000_000.0), "出差", 1.0);
        tick_to_now(&mut t3, Some(1_000_000.0 + 3600.0));
        assert!(
            (t3["elapsed_min"].as_f64().unwrap() - 60.0).abs() < 0.01,
            "一小时后 elapsed_min 应 ≈60，实际 {}",
            t3["elapsed_min"]
        );
        // time_scale=2 → 同样的一小时推进双倍游戏时间
        let mut t4 = start_trip(&long, Some(1_000_000.0), "加速", 2.0);
        tick_to_now(&mut t4, Some(1_000_000.0 + 3600.0));
        assert!((t4["elapsed_min"].as_f64().unwrap() - 120.0).abs() < 0.01);

        let pay = trip_payload(&t3).unwrap();
        for k in ["position", "remaining_text", "polyline", "phases", "status", "current_phase_note"] {
            assert!(pay.get(k).is_some(), "trip_payload 缺字段 {k}");
        }
        assert_eq!(pay["phase_count"], json!(t3["phases"].as_array().unwrap().len()));
        assert_eq!(pay["mode"], json!("plane"));
    }

    #[test]
    fn facilities_context_absorbs_named_stations() {
        let anchor = GZ_TA;
        let ctx = makes_context(
            &[
                json!({"gx": 2, "gy": 3, "type": "subway", "name": "广州塔站"}),
                json!({"gx": 9, "gy": 9, "type": "bus_stop", "name": "艺洲路公交站"}),
                json!({"gx": 15, "gy": 16, "type": "pier", "name": "广州塔码头"}),
            ],
            anchor,
            20.0,
            Some(20.0),
        );
        let mlng = coord::meters_per_deg_lng(anchor.1);
        // 网格格 → 经纬度（与 coord 的约定一致：y 向下 = 向南）
        let cell = |gx: f64, gy: f64| {
            (
                anchor.0 + gx * 20.0 / mlng,
                anchor.1 - gy * 20.0 / METERS_PER_DEG_LAT,
            )
        };
        let a_in = cell(6.0, 2.0);
        let b_in = cell(14.0, 14.0);

        let types: &[&str] = &["subway"];
        let st = find_station_in_grid(a_in.0, a_in.1, Some(&ctx), Some(types)).expect("应命中地铁节点");
        assert_eq!(st["name"], json!("广州塔站"));
        assert_eq!(st["kind"], json!("subway"));
        assert!(
            haversine_m(
                (st["lng"].as_f64().unwrap(), st["lat"].as_f64().unwrap()),
                anchor
            ) < 500.0,
            "节点网格坐标能换回经纬度（落点在小区附近）"
        );
        // 越界保护：小区外的点不吸附
        assert!(find_station_in_grid(anchor.0 + 0.02, anchor.1, Some(&ctx), Some(types)).is_none());
        // 站名匹配不上类型时也不吸附（该点附近的 subway 节点在半径外）
        assert!(find_station_in_grid(b_in.0, b_in.1, Some(&ctx), Some(types)).is_none());

        let r_ctx = plan_route(a_in, b_in, Some("地铁"), Some(false), Some(true), "balanced", Some(&ctx));
        let r_geo = plan_route(a_in, b_in, Some("地铁"), Some(false), Some(true), "balanced", None);
        let n_ctx = r_ctx["steps"][0]["note"].as_str().unwrap();
        let n_geo = r_geo["steps"][0]["note"].as_str().unwrap();
        assert!(n_ctx.contains("广州塔站"), "有 facilities 时接驳段吸附命名站点：{n_ctx}");
        assert!(n_geo.contains("地铁站"), "无 facilities 时退回几何估计的通用站名：{n_geo}");
        assert_eq!(r_ctx["steps"].as_array().unwrap().len(), 3, "接驳→主段→接驳");
        assert_eq!(r_geo["steps"].as_array().unwrap().len(), 3);
        assert!(r_ctx["duration_min"].as_f64().unwrap() > 0.0);
        // 公交吸附公交站、轮渡吸附码头
        let bus_note = plan_route(a_in, b_in, Some("bus"), Some(false), Some(true), "balanced", Some(&ctx))
            ["steps"][0]["note"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(bus_note.contains("艺洲路公交站"), "公交往返吸附：{bus_note}");
        // 没传 types 时按方式取默认类型：ferry → pier，但接驳半径内没有码头 → 通用名
        let ferry_note = plan_route(a_in, b_in, Some("ferry"), Some(false), Some(true), "balanced", Some(&ctx))
            ["steps"][0]["note"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(ferry_note.contains("码头"), "轮渡接驳文案：{ferry_note}");
    }

    #[test]
    fn api_layer_shapes() {
        // parse_qs 的 {k:[v]} 形式 + 强制指定高铁（water 显式传，避免全局检测器干扰）
        let res = api_plan(&json!({
            "from_lng": ["113.264"], "from_lat": ["23.129"],
            "to_lng": ["114.0579"], "to_lat": ["22.5431"],
            "prefer": ["高铁"], "water": "0"
        }));
        assert_eq!(res["ok"], json!(true));
        assert_eq!(res["route"]["mode"], json!("train"));
        assert!(res["route"]["duration_text"].as_str().unwrap().ends_with("分")
            || res["route"]["duration_text"].as_str().unwrap().ends_with("小时"));
        assert!(res["options"].as_array().unwrap().len() >= 1);
        assert_eq!(res["modes"].as_array().unwrap().len(), 9);
        // 普通 {k:v} 也认
        let kv = api_plan(&json!({
            "from_lng": 113.264, "from_lat": 23.129,
            "to_lng": 113.265, "to_lat": 23.129, "water": "0"
        }));
        assert_eq!(kv["ok"], json!(true));
        assert_eq!(kv["route"]["mode"], json!("walk"));
        // 缺坐标 → 优雅报错
        let bad = api_plan(&json!({}));
        assert_eq!(bad["ok"], json!(false));
        assert!(bad["error"].as_str().unwrap().contains("from_lng"));
        let bad2 = api_plan(&json!({"from_lng": "abc", "from_lat": "23", "to_lng": "1", "to_lat": "2"}));
        assert_eq!(bad2["ok"], json!(false));
        assert_eq!(api_modes()["modes"].as_array().unwrap().len(), 9);
    }
}
