//! 生活设施(T2-1) + 交通设施(T2-2)（Rust 版，逐行对照 Python 原型 `world_map/facilities.py`）
//!
//! ## 这个模块干什么
//! 拿一张格网 `layout`（建筑 + 路网 + 公园 + 水体，约定见文件末尾 `demo_layout()`），在**空格**里摆设施：
//!   1. [`generate_facilities`]：7 类生活设施（住宅/商业/教育/医疗/休闲娱乐/市政服务/住宿）。
//!      按权重把名额分摊给各类（[`alloc`]），占地大/成片要求强的先放（教育 → 休闲 → 医疗 → 住宅 → …）；
//!   2. [`generate_transport_nodes`]：7 类交通设施（公交站/地铁站/停车场/加油站/机场/火车站/码头）。
//!      按社区/区县/城市三级 `LEVEL_PLAN` 出不同组合：地铁从区县级起、机场只在城市级、码头必须有水体；
//!   3. [`generate_all`]：一次出齐生活 + 交通，附 [`facility_stats`] 统计。
//! 另配查询（[`find_facilities`] / [`facility_at`]）与前端元数据表（[`types_payload`]，与 `facilities.js` 同源）。
//!
//! ## 为什么这么设计
//! · **打分而不是撒点**：原型把「城市怎么长」的直觉写成了权重表——临路 `w_road`、同类成组
//!   `w_cluster`、近绿地 `w_green`、靠/离中心 `w_center`（机场加油站是负权重＝偏外围），
//!   外加一点点 `jitter`。[`rank`] 给每个空格打分，再在**前几名里随机挑一个**：结果既有规划感
//!   （住宅成组、商业沿街、学校成片），又不会每次刷新都落在同一个角。本模块逐项照抄这些权重与
//!   阈值，因为两边的自检断言的是同一批结构特征。
//! · **交通节点避让生活设施**：`generate_transport_nodes` 收一份已生成的生活设施，把这些格子从
//!   空格表里挖掉再放交通节点，所以两类设施天然不重叠；停车场额外以商业/休闲/市政设施为偏好锚点
//!   （`pref`），因为出行节点跟着人流走。
//! · **对外一律 `serde_json::Value`**：与 Python 的 dict/list 一一对应，HTTP 层和前端拿到的 JSON
//!   形状与原型逐字段相同；本工程 serde_json 开了 `preserve_order`，字段顺序也一致，移植期两边
//!   并跑可以肉眼 diff。内部计算才用小 struct（[`State`] / [`FacType`]）。
//! · **确定性**：名字池取名字、预算分摊、结果排序全部无随机；随机只出现在三处——「前几名挑一个」
//!   「分数抖动」「邻格尝试顺序」，且统一用 [`StdRng::seed_from_u64`] 播种，种子由
//!   `md5("{area}|{size}|{salt}")` 前 4 字节给出（[`seed_of`]，与原型 `_seed_of` 逐位相同），
//!   生活设施用 salt=`life`、交通设施用 salt=`traffic`。**同输入永远同输出**，可缓存、可回归。
//!   ⚠️ 但 Rust 的 `StdRng`（ChaCha12）与 Python 的 `random.Random`（MT19937）**不是同一个算法**，
//!   随机序不同 ⇒ 具体落点坐标不会与 Python 逐位相同。这是**有意接受的行为差异**：结构、数量、
//!   类型分布、规则约束（临路/临水/不重叠/间距）两边一致，只有「第几个候选」的挑选不同。
//!   要逐位对拍得把 MT19937 也搬过来，那还不如直接用 Python 原型跑。
//! · **Python 数值语义照抄**（上一位移植者踩过的坑，这里同样适用）：
//!   - `round(x, n)` 用「按 n 位格式化再解析回来」实现（[`round_n`]），不用 `(x*10^n).round()/10^n`
//!     ——后者在十进制边界上会偏（本机实测 8999 例差 89 例，约 1%），会变成「2.67 vs 2.7」这种
//!     可见差异；`_seg_cells` 里的 `int(round(v))` 也吃这个语义（银行家舍入，.5 取偶）。
//!   - 浮点 `sum()` 用 Neumaier 补偿求和（[`py_sum`]），因为 Python 3.12+ 的 `sum()` 就是它，
//!     朴素累加会差 1e-14 级。全文件只有 [`alloc`] 里那一处浮点求和（名额分摊的总权重）。
//! · **不 panic**：原型靠异常暴露调用方 bug（KeyError / TypeError / ValueError），Rust 版对缺失字段、
//!   类型不对、越界一律退化取值（默认值 / 跳过该条 / `FAR`），因为游戏主循环里一次 panic 就是整局崩。
//!   唯一的例外是给 `size` 与线段采样数各加了一个安全上限（[`MAX_SIZE`] / [`MAX_SEG_SAMPLES`]），
//!   防止畸形输入把内存打爆或让栅格化循环跑到天荒地老——正常输入永远碰不到它们。
//! · **不依赖 `coord`**：原型 `facilities.py` 只 import math/random/hashlib（PIL 只用于可选出图），
//!   不碰地理换算，所以本模块也不依赖兄弟模块，可以单独编译、单独测试。
//!
//! ## 与 Python 原型的对应关系（函数级）
//! | Python | Rust |
//! |---|---|
//! | `DEFAULT_CELL_METERS` / `FAR` | [`DEFAULT_CELL_METERS`] / [`FAR`] |
//! | `LIFE_TYPES` / `TRANSPORT_TYPES` / `ALL_TYPES` | 一张 `ALL_TYPES` 静态表（前 7 项生活、后 7 项交通）+ [`life_meta`]/[`transport_meta`] |
//! | `GROUP_ZH` / `LEVEL_ZH` / `LEVEL_PLAN` | [`group_zh`] / [`level_zh`] / `LEVEL_PLAN` |
//! | `_rect_cells` | [`rect_range`] + [`State::mark_rect`] |
//! | `_seg_cells` | [`seg_cells`] |
//! | `_dist_field` | [`dist_field`] |
//! | `_build_state` | [`build_state`]（返回 [`State`]） |
//! | `_cheb` / `_fits` / `_cell_of` | [`cheb`]+[`cheb_f`] / [`fits`] / [`cell_of`] |
//! | `_score` / `_rank` / `_make_name` / `_mk_fac` / `_place_type` | 同名（去掉下划线，模块私有） |
//! | `_alloc` / `_seed_of` | [`alloc`] / [`seed_of`]（私有） |
//! | `generate_facilities` | 同名 |
//! | `_neighbor_free` / `_place_bus_stops` | [`neighbor_free`] / [`place_bus_stops`]（私有） |
//! | `generate_transport_nodes` / `generate_all` | 同名 |
//! | `find_facilities` / `facility_at` / `facility_stats` / `types_payload` | 同名 |
//! | `draw_facilities`（PIL 出图） | [`draw_facilities`] 只回「未移植」说明；无依赖替代品 [`facilities_svg`] |
//! | `_demo_layout` / `_overlap_check` / `__main__` 自检 | 文件末尾 `mod tests`（原型没有单独的 selftest 文件） |
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::{HashSet, VecDeque};

/// 每格米数默认值（公交站间距、地铁稀疏度都按它折算）
pub const DEFAULT_CELL_METERS: f64 = 30.0;
/// 距离场的「不可达」值（与原型 `FAR = 999` 相同，也是个足够大的分数惩罚基数）
pub const FAR: i32 = 999;
/// 网格边长安全上限。原型没有这个限制（Python 里一张 10 万边长的图会直接把内存吃光），
/// 这里挡一下畸形输入；实测布局边长 ≤ 64，正常输入永远碰不到。
pub const MAX_SIZE: i64 = 512;
/// 单条线段栅格化的采样数上限（原型是 `max(1, int(4 * 线段长度))`，超长线段会采样到天荒地老）。
/// 网格内最长的对角线约 `1.42 * size`，采样数 `≈ 5.7 * size < 8 * size`，正常输入碰不到这个上限。
const MAX_SEG_SAMPLES: i64 = 8 * MAX_SIZE;

// ══════════════════════════════════════════════════════════════════════════
// 一、设施类型表（对应 Python 的 LIFE_TYPES / TRANSPORT_TYPES / ALL_TYPES）
// ══════════════════════════════════════════════════════════════════════════

/// 一类设施的全部参数。对应原型 `ALL_TYPES[key]` 那个 dict：
/// 内部计算要频繁取这些数，用 struct 比每次 `value.get("w_road").as_f64()` 省事也更不容易写错；
/// 对外仍由 [`types_payload`] 输出成 JSON（前端只认那 8 个展示字段）。
#[derive(Debug, Clone, Copy)]
struct FacType {
    /// 类型键（JSON 里的 `type`）
    key: &'static str,
    /// 中文名（`type_zh`）
    zh: &'static str,
    icon: &'static str,
    /// 配色 RGB（前端图例用）
    color: (u8, u8, u8),
    /// id 前缀：`R001` / `B001` …
    prefix: &'static str,
    /// `life` | `transport`
    group: &'static str,
    /// 生成权重（只对生活设施有效；交通设施的名额由 LEVEL_PLAN 直接给）
    weight: f64,
    /// 占地尺寸（格）
    foot: (i64, i64),
    /// 期望的「临路距离」区间（格）
    road_band: (f64, f64),
    /// 成组半径：≤ 该距离的同类型设施给加分（住宅成组、学校成片）
    cluster_r: i64,
    /// 同类型之间的最小间距（格），0 = 不限
    min_gap: i64,
    /// 打分权重：临路 / 成组 / 近绿地 / 靠中心（负值 = 离中心）
    w_road: f64,
    w_cluster: f64,
    w_green: f64,
    w_center: f64,
    /// 分数抖动幅度：名次相近的候选之间才随机，避免每次都挑同一个角
    jitter: f64,
    /// 名称池（[`make_name`] 从里面取，重名加序号）
    names: &'static [&'static str],
    /// 硬约束：必须临路（公交/地铁/停车场/加油站/火车站）
    hard_road: bool,
    /// 硬约束：必须临水（码头）
    need_water: bool,
}

/// 用 `static` 而不是 `const`：`life_meta()` 要返回 `&'static FacType`，
/// 而 `&ALL_TYPES[i]`（运行期下标）无法从 const 提升为 'static 引用。
///
/// 顺序 = Python 的 `ALL_TYPES`（`LIFE_TYPES` 后接 `TRANSPORT_TYPES`）：
/// 前 7 项生活设施、后 7 项交通设施。`types_payload()` 的键顺序、`by_type` 的展示顺序都依赖它。
static ALL_TYPES: [FacType; 14] = [
    // ── T2-1 生活设施 7 类 ──
    FacType {
        key: "residential", zh: "住宅", icon: "🏠", color: (196, 186, 166), prefix: "R",
        group: "life", weight: 0.30, foot: (1, 1), road_band: (1.0, 3.0), cluster_r: 2, min_gap: 0,
        w_road: 2.0, w_cluster: 6.0, w_green: 0.5, w_center: 0.5, jitter: 1.5,
        names: &["阳光苑", "临江苑", "安宁里", "梧桐公寓", "青竹小区", "望江新邨"],
        hard_road: false, need_water: false,
    },
    FacType {
        key: "commercial", zh: "商业", icon: "🛒", color: (230, 190, 120), prefix: "C",
        group: "life", weight: 0.20, foot: (1, 1), road_band: (1.0, 1.0), cluster_r: 0, min_gap: 0,
        w_road: 8.0, w_cluster: 0.0, w_green: 0.0, w_center: 1.0, jitter: 1.5,
        names: &["便利店", "生鲜超市", "购物中心", "沿街商铺", "百货商场"],
        hard_road: false, need_water: false,
    },
    FacType {
        key: "education", zh: "教育", icon: "🏫", color: (150, 200, 240), prefix: "E",
        group: "life", weight: 0.10, foot: (2, 1), road_band: (1.0, 2.0), cluster_r: 3, min_gap: 0,
        w_road: 3.0, w_cluster: 7.0, w_green: 1.5, w_center: 0.0, jitter: 1.0,
        names: &["实验小学", "第三中学", "育才幼儿园", "社区学院"],
        hard_road: false, need_water: false,
    },
    FacType {
        key: "medical", zh: "医疗", icon: "🏥", color: (235, 150, 150), prefix: "M",
        group: "life", weight: 0.08, foot: (2, 1), road_band: (1.0, 1.0), cluster_r: 4, min_gap: 0,
        w_road: 6.0, w_cluster: 2.0, w_green: 0.0, w_center: 1.5, jitter: 1.0,
        names: &["社区卫生服务中心", "人民医院", "便民诊所", "大药房"],
        hard_road: false, need_water: false,
    },
    FacType {
        key: "leisure", zh: "休闲娱乐", icon: "🎡", color: (170, 220, 160), prefix: "L",
        group: "life", weight: 0.15, foot: (1, 2), road_band: (1.0, 3.0), cluster_r: 3, min_gap: 0,
        w_road: 1.0, w_cluster: 3.0, w_green: 7.0, w_center: 0.0, jitter: 1.2,
        names: &["社区公园", "篮球场", "健身房", "电影院", "茶馆"],
        hard_road: false, need_water: false,
    },
    FacType {
        key: "civic", zh: "市政服务", icon: "🏛️", color: (190, 180, 220), prefix: "V",
        group: "life", weight: 0.09, foot: (1, 1), road_band: (1.0, 2.0), cluster_r: 0, min_gap: 0,
        w_road: 2.0, w_cluster: 0.0, w_green: 0.0, w_center: 6.0, jitter: 1.0,
        names: &["社区服务中心", "派出所", "邮政支局", "消防站", "图书馆"],
        hard_road: false, need_water: false,
    },
    FacType {
        key: "lodging", zh: "住宿", icon: "🏨", color: (220, 180, 210), prefix: "H",
        group: "life", weight: 0.08, foot: (1, 1), road_band: (1.0, 1.0), cluster_r: 0, min_gap: 0,
        w_road: 7.0, w_cluster: 0.0, w_green: 0.0, w_center: 0.5, jitter: 1.2,
        names: &["快捷酒店", "街角民宿", "青年旅舍", "商务宾馆"],
        hard_road: false, need_water: false,
    },
    // ── T2-2 交通设施 7 类（第 8~14 类扩展） ──
    FacType {
        key: "bus_stop", zh: "公交站", icon: "🚌", color: (120, 180, 235), prefix: "B",
        group: "transport", weight: 0.0, foot: (1, 1), road_band: (1.0, 1.0), cluster_r: 0, min_gap: 10,
        w_road: 12.0, w_cluster: 0.0, w_green: 0.0, w_center: 0.5, jitter: 1.5,
        names: &["人民路站", "中山路站", "建设街站", "文化路口站", "公园前站", "滨江路站"],
        hard_road: true, need_water: false,
    },
    FacType {
        key: "subway", zh: "地铁站", icon: "🚇", color: (90, 130, 200), prefix: "S",
        group: "transport", weight: 0.0, foot: (1, 1), road_band: (1.0, 2.0), cluster_r: 0, min_gap: 33,
        w_road: 6.0, w_cluster: 0.0, w_green: 0.0, w_center: 5.0, jitter: 1.0,
        names: &["中心站", "东门站", "滨江站", "体育中心站"],
        hard_road: true, need_water: false,
    },
    FacType {
        key: "parking", zh: "停车场", icon: "🅿️", color: (150, 160, 175), prefix: "P",
        group: "transport", weight: 0.0, foot: (2, 1), road_band: (1.0, 1.0), cluster_r: 0, min_gap: 4,
        w_road: 6.0, w_cluster: 0.0, w_green: 0.0, w_center: 0.0, jitter: 1.5,
        names: &["公共停车场", "地下车库", "立体停车楼"],
        hard_road: true, need_water: false,
    },
    FacType {
        key: "gas", zh: "加油站", icon: "⛽", color: (240, 170, 90), prefix: "F",
        group: "transport", weight: 0.0, foot: (2, 1), road_band: (1.0, 1.0), cluster_r: 0, min_gap: 26,
        // 偏外围：w_center 为负 → 越靠中心分越低
        w_road: 8.0, w_cluster: 0.0, w_green: 0.0, w_center: -6.0, jitter: 1.0,
        names: &["中国石化加油站", "能源补给站"],
        hard_road: true, need_water: false,
    },
    FacType {
        key: "airport", zh: "机场", icon: "✈️", color: (160, 200, 220), prefix: "A",
        group: "transport", weight: 0.0, foot: (3, 2), road_band: (1.0, 8.0), cluster_r: 0, min_gap: 60,
        // 只在城市级、远离中心
        w_road: 2.0, w_cluster: 0.0, w_green: 0.0, w_center: -8.0, jitter: 0.5,
        names: &["国际机场", "通用机场"],
        hard_road: false, need_water: false,
    },
    FacType {
        key: "train_station", zh: "火车站", icon: "🚄", color: (200, 130, 130), prefix: "T",
        group: "transport", weight: 0.0, foot: (3, 2), road_band: (1.0, 2.0), cluster_r: 0, min_gap: 40,
        w_road: 6.0, w_cluster: 0.0, w_green: 0.0, w_center: 2.0, jitter: 0.8,
        names: &["火车站", "高铁站", "中央车站"],
        // 注意：原型 train_station **没有** hard_road（火车站不强制临路，只是加分），
        // 最早写成 true 时「空 layout 的区县级」会一个火车站都放不出来，被结构对拍抓出来
        hard_road: false, need_water: false,
    },
    FacType {
        key: "pier", zh: "码头", icon: "⚓", color: (110, 165, 200), prefix: "W",
        group: "transport", weight: 0.0, foot: (1, 1), road_band: (1.0, 6.0), cluster_r: 0, min_gap: 20,
        w_road: 3.0, w_cluster: 0.0, w_green: 2.0, w_center: 0.0, jitter: 1.0,
        names: &["客运码头", "渔人码头", "轮渡码头"],
        hard_road: false, need_water: true,
    },
];

/// 生活设施类型数（`ALL_TYPES` 的前 7 项）
const LIFE_N: usize = 7;

/// 交通设施类型数（`ALL_TYPES` 的后 7 项）
const TRANSPORT_N: usize = 7;

/// 生活设施表（对应 `LIFE_TYPES`，顺序即原型 dict 的插入顺序）
fn life_types() -> &'static [FacType] {
    &ALL_TYPES[..LIFE_N]
}

/// 交通设施表（对应 `TRANSPORT_TYPES`）
fn transport_types() -> &'static [FacType] {
    &ALL_TYPES[LIFE_N..LIFE_N + TRANSPORT_N]
}

/// 按 key 查生活设施参数（不在表里返回 None，对应 `k in LIFE_TYPES` 的判断）
fn life_meta(key: &str) -> Option<&'static FacType> {
    life_types().iter().find(|m| m.key == key)
}

/// 按 key 查交通设施参数
fn transport_meta(key: &str) -> Option<&'static FacType> {
    transport_types().iter().find(|m| m.key == key)
}

/// 按 key 查任意设施参数（对应 `ALL_TYPES.get(k)`，`facility_stats` 取中文名要用）
fn any_meta(key: &str) -> Option<&'static FacType> {
    ALL_TYPES.iter().find(|m| m.key == key)
}

/// 分组中文名（对应 `GROUP_ZH`）
fn group_zh(g: &str) -> &'static str {
    match g {
        "life" => "生活设施",
        "transport" => "交通设施",
        _ => "设施",
    }
}

/// 层级中文名（对应 `LEVEL_ZH`）
fn level_zh(level: &str) -> &'static str {
    match level {
        "community" => "小区",
        "district" => "区县",
        "city" => "城市",
        _ => "小区",
    }
}

/// 各层级生成哪些交通设施（对应 `LEVEL_PLAN`）。
/// 机场/火车站只在城市级（火车站区县级也有 1 个），地铁从区县级开始，社区级只有公交 + 停车场。
/// 每项顺序 = 原型 dict 的插入顺序，`types_payload()['level_plan']` 的键顺序依赖它。
static LEVEL_PLAN: [(&str, &[(&str, i64)]); 3] = [
    ("community", &[("bus_stop", 2), ("parking", 1)]),
    (
        "district",
        &[("bus_stop", 5), ("subway", 1), ("parking", 3), ("gas", 1), ("train_station", 1)],
    ),
    (
        "city",
        &[
            ("bus_stop", 10),
            ("subway", 3),
            ("parking", 6),
            ("gas", 2),
            ("train_station", 1),
            ("airport", 1),
            ("pier", 1),
        ],
    ),
];

/// 取某层级的计划表；未知层级退回 community（对应原型 `level if level in LEVEL_PLAN else 'community'`）
fn level_plan_of(level: &str) -> &'static [(&'static str, i64)] {
    LEVEL_PLAN
        .iter()
        .find(|(k, _)| *k == level)
        .map(|(_, v)| *v)
        .unwrap_or(LEVEL_PLAN[0].1)
}

/// 交通设施的放置顺序（对应原型里那个写死的 tuple）：
/// 大件/强约束的先放（机场、火车站、码头），沿街小件（公交站）最后铺。
const TRANSPORT_ORDER: [&str; 7] = [
    "airport",
    "train_station",
    "pier",
    "gas",
    "subway",
    "parking",
    "bus_stop",
];

// ══════════════════════════════════════════════════════════════════════════
// 二、Python 数值语义工具（round / sum / 宽松取数）
// ══════════════════════════════════════════════════════════════════════════

/// 复刻 **CPython 3.12+ `sum()` 对 float 的 Neumaier 补偿求和**。
///
/// 朴素累加与它差 1e-14 级，而这一丁点差异会顺着「名额分摊取整」放大成整栋楼少一个设施。
/// 本模块只有 [`alloc`] 一处浮点求和（权重总和），但保持与 `transport::py_sum` 同样的语义。
/// （与 `transport.rs` 里那份是同一套实现，两份互不影响：本模块不依赖兄弟模块。）
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

/// 保留 n 位小数，**与 Python 的 `round(x, n)` 逐位一致**（含 .5 的银行家舍入）。
///
/// 不用更快的 `(x * 10^n).round() / 10^n`：乘 10^n 会再引入一次舍入，使恰好落在十进制边界
/// 附近的值偏到另一边（本机实测 8999 例差 89 例，约 1%：`2.675` 会变成 2.7 而不是 2.67）。
/// 改成「按 n 位格式化再解析回来」后 0 例不一致——两边都是在做「对二进制精确值做十进制正确
/// 舍入、.5 取偶」。本机实测 `format!("{:.0}")`：0.5→0、1.5→2、2.5→2、3.5→4、-2.5→-2，
/// 与 Python `round()` 完全一致。`_seg_cells` 的 `int(round(v))` 与 `find_facilities` 的
/// `round(d, 2)` 都走这里。
fn round_n(x: f64, n: i32) -> f64 {
    if !x.is_finite() {
        return x; // inf/NaN 原样返回（Python 在 int()/round 上会抛，这里不 panic）
    }
    match format!("{:.*}", n.max(0) as usize, x).parse::<f64>() {
        Ok(v) => v,
        Err(_) => x,
    }
}

/// Python 的一个值是不是「假值」（`or` 语义要用）：None/False/0/""/[]/{}
fn is_falsy(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::Number(n) => n.as_f64().map(|x| x == 0.0).unwrap_or(false),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// 宽松取数：数字直接用；数字字符串按 Python `float()` 解析；其它给 None。
fn loose_num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }), // Python 里 float(True) == 1.0
        _ => None,
    }
}

/// 替代 Python 的 `d.get(k, dv)`：取不到或不是数就用默认值
fn num(v: &Value, k: &str, dv: f64) -> f64 {
    match v.get(k) {
        Some(x) => loose_num(x).unwrap_or(dv),
        None => dv,
    }
}

/// 替代 Python 的 `d.get(k, dv) or dv`：假值（0/None/空串）也算取不到。
/// 原型对 `w`/`h`/`size` 用的就是这个语义（`b.get('w', 1) or 1`）。
fn nz(v: &Value, k: &str, dv: f64) -> f64 {
    match v.get(k) {
        None => dv,
        Some(x) => {
            if is_falsy(x) {
                dv
            } else {
                loose_num(x).unwrap_or(dv)
            }
        }
    }
}

/// 取字符串字段（非字符串按默认值处理；原型是 `str(v)`，实际输入里不会是别的类型）
fn str_field<'a>(v: &'a Value, k: &str, d: &'a str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or(d)
}

/// 标量转文本（区域名兜底用；对象/数组给 None）
fn as_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(_) | Value::Bool(_) => Some(v.to_string()),
        _ => None,
    }
}

/// 取数组字段；缺失/类型不对当成空数组（对应 `layout.get(k) or []`）
fn items<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k)
        .and_then(|x| x.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

/// 每格米数：`meters_per_cell or cell_meters or 30`（对应原型那一行 `or` 链）
fn mpc_of(layout: &Value) -> f64 {
    for k in ["meters_per_cell", "cell_meters"] {
        if let Some(v) = layout.get(k) {
            if !is_falsy(v) {
                if let Some(x) = loose_num(v) {
                    return x;
                }
            }
        }
    }
    DEFAULT_CELL_METERS
}

// ══════════════════════════════════════════════════════════════════════════
// 三、几何与占用工具（_rect_cells / _seg_cells / _dist_field / _build_state / _cheb / _fits / _cell_of）
// ══════════════════════════════════════════════════════════════════════════

/// `State.kind` 的位标记：一格里可能同时压着建筑和路（手工造的 layout 常见），所以按位存
const B_BUILDING: u8 = 1;
const B_PARK: u8 = 2;
const B_WATER: u8 = 4;
const B_ROAD: u8 = 8;
const B_ROAD_MAIN: u8 = 16;
const B_ROAD_OTHER: u8 = 32;

/// 生成器状态：把 layout 归一化成「每格地物位标记 + 空格表 + 3 张距离场」。
///
/// 与原型 `_build_state()` 返回的那个 dict 一一对应，只是把 Python 的 set/dict 换成定长网格：
/// 格数最多 512²，网格比哈希表更快，而且**迭代顺序固定**（原型用 set 迭代，顺序依赖哈希，
/// 这是 Rust 侧唯一「有意换掉」的实现细节，见文件头关于随机序的说明）。
struct State {
    size: i64,
    mpc: f64,
    /// 每格的地物位（B_* 的按位或）
    kind: Vec<u8>,
    /// 空格表（kind == 0 的格子）。设施只往这里放；放一个就挖掉它占的 footprint
    free: Vec<bool>,
    /// 主路总长度（米/格未换算，单位是格；公交站数量按它折算），对应 `main_road_len`
    main_road_len: f64,
    /// 主路线段列表（公交站沿它布点）
    main_segs: Vec<(f64, f64, f64, f64)>,
    /// 网格中心（浮点，用于 w_center）
    center: (f64, f64),
    /// 到最近道路 / 水体 / 绿地（公园∪水）的切比雪夫距离场
    road_dist: Vec<i32>,
    water_dist: Vec<i32>,
    green_dist: Vec<i32>,
    /// 有没有水体（码头要不要跳过看它）
    has_water: bool,
}

impl State {
    #[inline]
    fn idx(&self, x: i64, y: i64) -> usize {
        (y * self.size + x) as usize
    }

    #[inline]
    fn in_bounds(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && x < self.size && y < self.size
    }

    /// 这一格能不能放设施（越界一律 false，对应原型「不在 free 集合里」）
    #[inline]
    fn is_free(&self, x: i64, y: i64) -> bool {
        self.in_bounds(x, y) && self.free[self.idx(x, y)]
    }

    /// 距离场取值，越界按不可达 —— 对应 Python `dist.get(cell, FAR)`
    #[inline]
    fn at(&self, field: &[i32], x: i64, y: i64) -> i32 {
        if self.in_bounds(x, y) {
            field[self.idx(x, y)]
        } else {
            FAR
        }
    }

    /// 把一个矩形区域标上地物位（越界自动裁剪）—— 对应 `_rect_cells` + 集合的 `|=`
    fn mark_rect(&mut self, x: f64, y: f64, w: f64, h: f64, bit: u8) {
        let (x0, y0, x1, y1) = rect_range(x, y, w, h, self.size);
        for gy in y0..y1 {
            for gx in x0..x1 {
                let i = self.idx(gx, gy);
                self.kind[i] |= bit;
            }
        }
    }
}

/// 矩形（格）覆盖的行列范围 `[x0,x1) × [y0,y1)`，越界自动裁剪。
/// 对应原型 `_rect_cells` 的边界算法：`max(0, floor(x))` 到 `min(size, ceil(x+w))`。
/// `as i64` 对 NaN 给 0、对超大值饱和，都不会 panic。
fn rect_range(x: f64, y: f64, w: f64, h: f64, size: i64) -> (i64, i64, i64, i64) {
    let x0 = (x.floor() as i64).max(0);
    let y0 = (y.floor() as i64).max(0);
    let x1 = ((x + w).ceil() as i64).min(size);
    let y1 = ((y + h).ceil() as i64).min(size);
    (x0, y0, x1, y1)
}

/// 线段（格）覆盖的格集合：按 4x 采样栅格化，每个采样点 `round` 到最近格（对应 `_seg_cells`）。
/// 返回去重后的格子（原型的 set 无序，这里保序，只用于标记地物位，顺序无关结果）。
fn seg_cells(x1: f64, y1: f64, x2: f64, y2: f64, size: i64) -> Vec<(i64, i64)> {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let n = ((dx.abs().max(dy.abs()) * 4.0) as i64)
        .max(1)
        .min(MAX_SEG_SAMPLES.max(64));
    let mut out: Vec<(i64, i64)> = Vec::new();
    let mut seen: HashSet<(i64, i64)> = HashSet::new();
    for i in 0..=n {
        let t = i as f64 / n as f64;
        let gx = round_n(x1 + dx * t, 0) as i64;
        let gy = round_n(y1 + dy * t, 0) as i64;
        if gx >= 0 && gy >= 0 && gx < size && gy < size && seen.insert((gx, gy)) {
            out.push((gx, gy));
        }
    }
    out
}

/// 多源 BFS 距离场（切比雪夫）：每格到最近源点的格数，源点为 0，够不到保持 [`FAR`]。
/// 对应 `_dist_field`（原型的 deque 顺序依赖 set 迭代顺序，但多源 BFS 的距离值与顺序无关）。
fn dist_field(size: i64, sources: &[bool]) -> Vec<i32> {
    let n = (size * size) as usize;
    let mut dist = vec![FAR; n];
    let mut dq: VecDeque<usize> = VecDeque::new();
    for (i, &src) in sources.iter().enumerate().take(n) {
        if src {
            dist[i] = 0;
            dq.push_back(i);
        }
    }
    while let Some(i) = dq.pop_front() {
        let x = (i as i64) % size;
        let y = (i as i64) / size;
        let d = dist[i] + 1;
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= size || ny >= size {
                    continue;
                }
                let j = (ny * size + nx) as usize;
                if dist[j] == FAR {
                    dist[j] = d;
                    dq.push_back(j);
                }
            }
        }
    }
    dist
}

/// 从 `kind` 里抽一张布尔源点表（BFS 的输入）
fn bit_grid(kind: &[u8], bit: u8) -> Vec<bool> {
    kind.iter().map(|k| k & bit != 0).collect()
}

/// 把 layout 归一化成生成器状态 —— 对应 `_build_state`。
/// `layout` 不是对象/字段缺失/类型不对都退化取值，绝不 panic。
fn build_state(layout: &Value) -> State {
    let size = (nz(layout, "size", 20.0) as i64).max(4).min(MAX_SIZE);
    let mpc = mpc_of(layout);
    let n = (size * size) as usize;
    let mut st = State {
        size,
        mpc,
        kind: vec![0u8; n],
        free: vec![true; n],
        main_road_len: 0.0,
        main_segs: Vec::new(),
        center: (size as f64 / 2.0, size as f64 / 2.0),
        road_dist: Vec::new(),
        water_dist: Vec::new(),
        green_dist: Vec::new(),
        has_water: false,
    };
    for b in items(layout, "buildings") {
        st.mark_rect(
            num(b, "x", 0.0),
            num(b, "y", 0.0),
            nz(b, "w", 1.0),
            nz(b, "h", 1.0),
            B_BUILDING,
        );
    }
    for p in items(layout, "parks") {
        st.mark_rect(
            num(p, "x", 0.0),
            num(p, "y", 0.0),
            nz(p, "w", 1.0),
            nz(p, "h", 1.0),
            B_PARK,
        );
    }
    for w in items(layout, "water") {
        st.mark_rect(
            num(w, "x", 0.0),
            num(w, "y", 0.0),
            nz(w, "w", 1.0),
            nz(w, "h", 1.0),
            B_WATER,
        );
    }
    for r in items(layout, "roads") {
        let (x1, y1) = (num(r, "x1", 0.0), num(r, "y1", 0.0));
        let (x2, y2) = (num(r, "x2", 0.0), num(r, "y2", 0.0));
        let cells = seg_cells(x1, y1, x2, y2, size);
        // 原型是 str(r.get('type','secondary')) == 'main'；非字符串一律不算主路
        let is_main = str_field(r, "type", "secondary") == "main";
        let bit = if is_main { B_ROAD_MAIN } else { B_ROAD_OTHER };
        for (gx, gy) in cells {
            let i = st.idx(gx, gy);
            st.kind[i] |= bit | B_ROAD;
        }
        if is_main {
            // math.hypot：std 的 f64::hypot 同样是「先缩放再开方」的稳健实现，
            // 与 CPython 的 vector_norm 可能差最后 1 ulp（见文件头关于数值差异的说明）
            st.main_road_len += (x2 - x1).hypot(y2 - y1);
            st.main_segs.push((x1, y1, x2, y2));
        }
    }
    // occupied = 所有地物位的并集；free = 全网格 - occupied；green = park ∪ water
    for i in 0..n {
        st.free[i] = st.kind[i] == 0;
    }
    let road = bit_grid(&st.kind, B_ROAD);
    let water = bit_grid(&st.kind, B_WATER);
    let green = bit_grid(&st.kind, B_PARK | B_WATER);
    st.has_water = water.iter().any(|&b| b);
    st.road_dist = dist_field(size, &road);
    st.water_dist = dist_field(size, &water);
    st.green_dist = dist_field(size, &green);
    st
}

/// 切比雪夫距离（格）—— 对应 `_cheb`
#[inline]
fn cheb(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

/// 切比雪夫距离（浮点，与网格中心比较用）
#[inline]
fn cheb_f(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

/// footprint 是否完整落在空格里（不与建筑/公园/水/路/已放设施重叠）—— 对应 `_fits`。
/// 注意 `gx` 为负时逐格查空必然失败（对应原型「越界格不在 free 里」），无需额外判断。
fn fits(state: &State, gx: i64, gy: i64, foot: (i64, i64)) -> bool {
    let (fw, fh) = foot;
    if gx + fw > state.size || gy + fh > state.size {
        return false;
    }
    for x in gx..gx + fw {
        for y in gy..gy + fh {
            if !state.is_free(x, y) {
                return false;
            }
        }
    }
    true
}

/// 设施所在的格 —— 对应 `_cell_of`
fn cell_of(fac: &Value) -> (i64, i64) {
    (num(fac, "gx", 0.0) as i64, num(fac, "gy", 0.0) as i64)
}

// ══════════════════════════════════════════════════════════════════════════
// 四、打分与放置（_score / _rank / _make_name / _mk_fac / _place_type / _alloc / _seed_of）
// ══════════════════════════════════════════════════════════════════════════

/// 按类别的放置规则给一个空格打分（越高越该放）—— 对应 `_score`。
/// 每一项都逐句照抄原型（含加法顺序），因为分数排序直接决定落点。
/// `pref` 是可选偏好锚点（格列表），如停车场靠近商业/休闲/市政设施。
fn score(
    state: &State,
    meta: &FacType,
    cell: (i64, i64),
    same_pool: &[(i64, i64)],
    all_pool: &[(i64, i64)],
    pref: Option<&[(i64, i64)]>,
) -> f64 {
    let (gx, gy) = cell;
    let mut s = 0.0f64;
    // 偏好锚点（停车场贴商业/休闲/市政）
    if let Some(pref) = pref {
        if !pref.is_empty() {
            let pd = pref.iter().map(|p| cheb(cell, *p)).min().unwrap_or(0);
            if pd <= 2 {
                s += 5.0 * (3 - pd) as f64;
            }
        }
    }
    // 临路：区间内加分，区间外加负分（越远扣越多，最多扣 4 格的距离）
    let rd = state.at(&state.road_dist, gx, gy) as f64;
    let (lo, hi) = meta.road_band;
    if lo <= rd && rd <= hi {
        s += meta.w_road;
    } else {
        s -= meta.w_road * (4.0f64).min((rd - lo).abs().min((rd - hi).abs())) * 0.5;
    }
    // 成组/成片：靠近同类给高分（住宅成组、学校/医院成片）
    if meta.w_cluster != 0.0 {
        let cr = meta.cluster_r;
        if !same_pool.is_empty() {
            let d = same_pool.iter().map(|p| cheb(cell, *p)).min().unwrap_or(0);
            if d <= cr {
                s += meta.w_cluster * (1.0 + (cr - d) as f64 * 0.5);
            } else if d > cr * 4 {
                s -= meta.w_cluster * 0.5;
            }
        } else {
            s += meta.w_cluster * 0.2; // 第一个：普通位置即可
        }
    }
    // 近绿地（公园/水）
    if meta.w_green != 0.0 {
        let gd = state.at(&state.green_dist, gx, gy) as f64;
        if gd <= 2.0 {
            s += meta.w_green * (3.0 - gd);
        }
    }
    // 靠中心（市政/地铁）或远离中心（机场/加油站，负权重）
    if meta.w_center != 0.0 {
        let half = (state.size as f64 / 2.0).max(1.0);
        let dc = cheb_f((gx as f64, gy as f64), state.center);
        let sign = if meta.w_center > 0.0 { 1.0 } else { -1.0 };
        s += meta.w_center * (1.0 - dc / half) * sign;
    }
    // 临水硬条件（码头）：直接判死
    if meta.need_water && state.at(&state.water_dist, gx, gy) > 1 {
        return -(FAR as f64);
    }
    // 别的设施贴太近轻微减分，避免挤成一团
    if !all_pool.is_empty() {
        if all_pool.iter().map(|p| cheb(cell, *p)).min().unwrap_or(0) <= 1 {
            s -= 4.0;
        }
    }
    s
}

/// 返回按分数降序的候选格（已排除间距不足/放不下的）—— 对应 `_rank`。
/// 排序用稳定排序：分数相同时保持空格表的遍历顺序（网格行优先，顺序固定）。
/// 注意：**每一个通过过滤的候选都会消耗一次 RNG**（抖动分数），与原型一致。
fn rank(
    state: &State,
    meta: &FacType,
    rng: &mut StdRng,
    same_pool: &[(i64, i64)],
    all_pool: &[(i64, i64)],
    min_gap: i64,
    pref: Option<&[(i64, i64)]>,
) -> Vec<(f64, (i64, i64))> {
    let mut out: Vec<(f64, (i64, i64))> = Vec::new();
    for y in 0..state.size {
        for x in 0..state.size {
            let i = state.idx(x, y);
            if !state.free[i] {
                continue;
            }
            let cell = (x, y);
            if meta.hard_road {
                let rd = state.road_dist[i] as f64;
                if !(meta.road_band.0 <= rd && rd <= meta.road_band.1) {
                    continue; // 硬约束：必须临路
                }
            }
            if meta.need_water && state.water_dist[i] > 1 {
                continue; // 硬约束：码头必须临水
            }
            if min_gap != 0 && !same_pool.is_empty() {
                if same_pool.iter().map(|p| cheb(cell, *p)).min().unwrap_or(0) < min_gap {
                    continue;
                }
            }
            if !fits(state, x, y, meta.foot) {
                continue;
            }
            let mut sc = score(state, meta, cell, same_pool, all_pool, pref);
            sc += rng.gen::<f64>() * meta.jitter;
            out.push((sc, cell));
        }
    }
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
    out
}

/// 从名称池取名字，重名加序号（`小明` → `小明2` → `小明3`）—— 对应 `_make_name`。
/// `idx` 参数原型有但没用（保留签名以免调用点看不出对应关系）。
fn make_name(meta: &FacType, rng: &mut StdRng, used: &HashSet<String>, _idx: i64) -> String {
    let pool = meta.names;
    let mut base = pool[rng.gen_range(0..pool.len())].to_string();
    if used.contains(&base) {
        let mut k = 2;
        while used.contains(&format!("{}{}", base, k)) {
            k += 1;
        }
        base = format!("{}{}", base, k);
    }
    base
}

/// 造一个设施 dict，并占掉它的 footprint —— 对应 `_mk_fac`。
/// 字段顺序与原型一致（本工程 serde_json 开了 preserve_order，前端肉眼 diff 用得上）。
fn mk_fac(
    state: &mut State,
    meta: &FacType,
    typ: &str,
    cell: (i64, i64),
    seq: i64,
    used_names: &mut HashSet<String>,
    rng: &mut StdRng,
) -> Value {
    let (gx, gy) = cell;
    let (fw, fh) = meta.foot;
    for x in gx..gx + fw {
        for y in gy..gy + fh {
            if state.in_bounds(x, y) {
                let i = state.idx(x, y);
                state.free[i] = false; // 相当于原型 state['free'].discard((x, y))
            }
        }
    }
    let nm = make_name(meta, rng, used_names, seq);
    used_names.insert(nm.clone());
    json!({
        "id": format!("{}{:03}", meta.prefix, seq),
        "gx": gx, "gy": gy, "w": fw, "h": fh,
        "type": typ, "type_zh": meta.zh, "name": nm,
        "icon": meta.icon, "color": [meta.color.0, meta.color.1, meta.color.2],
        "group": meta.group,
        "indoor": indoor_of(typ),
        "cell_meters": state.mpc,
    })
}

/// 这个类型的设施算不算「室内」。
///
/// **为什么必须有这个字段**：事件引擎（`world_map/event_cmd.rs::build_context`）
/// 把「室内外」当作一类闸门 —— 停电 / 停水 / 失眠 / 睡过头这批事件**只在室内**才开闸。
/// 而在此之前 `mk_fac` 根本不产出这个字段，于是：
///   `facility_indoor()` 恒返回 `None` → `indoors` 恒为 `false`（按户外）
///   → 那批室内事件**永远不会发生**，而且不报错、不打日志，只是"从来没见过停电"。
///
/// 口径按常识分（拿不准的一律按**户外**，与 `events.rs` 已声明的保守取舍一致：
/// 宁可漏掉一次室内事件，也不要让角色在大街上"因为停电而害怕"）：
///   · 室内：住宅 / 商业 / 教育 / 医疗 / 市政服务 / 住宿 / 机场 / 火车站 / 地铁站
///   · 户外：公交站 / 停车场 / 加油站 / 码头 / 休闲娱乐（多半指公园）
pub fn indoor_of(typ: &str) -> bool {
    matches!(
        typ,
        "residential"
            | "commercial"
            | "education"
            | "medical"
            | "civic"
            | "lodging"
            | "airport"
            | "train_station"
            | "subway"
    )
}

/// 按规则放 n 个同一类型的设施；放不下就少放 —— 对应 `_place_type`。
///
/// 每个位置试三档间距：`min_gap` → `min_gap/2` → 0（原型的「放宽重试」），
/// 拿到候选后**在分数前几名里随机挑一个**（`k = min(len, 8)`，且至少 3），
/// 避免每次都落在同一个角。`pref` 为偏好锚点（格列表）。
fn place_type(
    state: &mut State,
    typ: &str,
    meta: &'static FacType,
    n: i64,
    rng: &mut StdRng,
    facilities: &mut Vec<Value>,
    pref: Option<&[(i64, i64)]>,
) -> Vec<Value> {
    let mut placed: Vec<Value> = Vec::new();
    let mut used_names: HashSet<String> = facilities
        .iter()
        .filter_map(|f| f.get("name").and_then(|x| x.as_str()).map(String::from))
        .collect();
    let seq0 = facilities
        .iter()
        .filter(|f| str_field(f, "type", "") == typ)
        .count() as i64;
    let gap = meta.min_gap;
    for _ in 0..n.max(0) {
        let mut spot: Option<(i64, i64)> = None;
        // 三档间距：先按规矩来，放不下再放宽（原型 `[gap, max(1,gap//2), 0] if gap else [0]`）
        let trials: Vec<i64> = if gap != 0 {
            vec![gap, (gap / 2).max(1), 0]
        } else {
            vec![0]
        };
        for g in trials {
            let same: Vec<(i64, i64)> = facilities
                .iter()
                .filter(|f| str_field(f, "type", "") == typ)
                .map(cell_of)
                .collect();
            let allp: Vec<(i64, i64)> = facilities.iter().map(cell_of).collect();
            let cands = rank(state, meta, rng, &same, &allp, g, pref);
            if !cands.is_empty() {
                let len = cands.len();
                let k = len.min(3.max(len.min(8)));
                spot = Some(cands[rng.gen_range(0..k)].1);
                break;
            }
        }
        let spot = match spot {
            Some(s) => s,
            None => break, // 放不下了
        };
        let fac = mk_fac(
            state,
            meta,
            typ,
            spot,
            seq0 + placed.len() as i64 + 1,
            &mut used_names,
            rng,
        );
        facilities.push(fac.clone());
        placed.push(fac);
    }
    placed
}

/// 按权重分摊名额（向下取整 + 余数给小数最大者）—— 对应 `_alloc`。
/// 返回 (类型, 名额) 列表，顺序与 `weights` 相同；`total` 用补偿求和（Python 3.12+ 的 sum）。
fn alloc(count: i64, weights: &[(String, f64)]) -> Vec<(String, i64)> {
    let total = {
        let s = py_sum(weights.iter().map(|w| w.1));
        if s == 0.0 {
            1.0
        } else {
            s
        }
    };
    let raw: Vec<(String, f64)> = weights
        .iter()
        .map(|(k, v)| (k.clone(), count as f64 * *v / total))
        .collect();
    let mut out: Vec<(String, i64)> = raw.iter().map(|(k, v)| (k.clone(), *v as i64)).collect();
    let used: i64 = out.iter().map(|x| x.1).sum();
    // 余数给小数部分最大的那几个（稳定排序 = 平手时保持原顺序，与 Python sorted 一致）
    let mut order: Vec<usize> = (0..raw.len()).collect();
    order.sort_by(|&a, &b| {
        let fa = raw[a].1 - (raw[a].1 as i64) as f64;
        let fb = raw[b].1 - (raw[b].1 as i64) as f64;
        fb.partial_cmp(&fa).unwrap_or(Ordering::Equal)
    });
    let extra = (count - used).max(0) as usize;
    for &i in order.iter().take(extra) {
        out[i].1 += 1;
    }
    out
}

/// 从 md5 派生随机种子 —— 对应 `_seed_of`：`int(md5("{area}|{size}|{salt}").hexdigest()[:8], 16)`
/// 即摘要前 4 字节按大端解释。**逐位与 Python 相同**，所以「区域名 → 种子」这条链路两边一致
/// （不一致的只有种子之后的随机序，见文件头）。
fn seed_of(area: &str, size: i64, salt: &str) -> u64 {
    let d = md5::compute(format!("{}|{}|{}", area, size, salt).as_bytes());
    u32::from_be_bytes([d.0[0], d.0[1], d.0[2], d.0[3]]) as u64
}

/// 造随机源：显式 seed 优先，否则按区域名派生。
/// ⚠️ `StdRng`(ChaCha12) ≠ Python 的 `random.Random`(MT19937)：随机序不同是有意接受的差异，
/// 两边只保证「同输入同输出」与结构一致，不保证坐标逐位相同。
fn rng_of(area: &str, size: i64, salt: &str, seed: Option<i64>) -> StdRng {
    match seed {
        Some(s) => StdRng::seed_from_u64(s as u64),
        None => StdRng::seed_from_u64(seed_of(area, size, salt)),
    }
}

/// 区域名兜底：`area_name or layout['name'] or ''`（原型用 f-string，非标量值这里按空串处理）
fn area_or_name(area_name: &str, layout: &Value) -> String {
    if !area_name.is_empty() {
        return area_name.to_string();
    }
    layout.get("name").and_then(as_text).unwrap_or_default()
}

/// 结果排序键：`(type, id)`，与原型 `sort(key=lambda f: (f['type'], f['id']))` 一致
fn type_id_key(f: &Value) -> (String, String) {
    (
        str_field(f, "type", "").to_string(),
        str_field(f, "id", "").to_string(),
    )
}

/// 按 `(type, id)` 排序（稳定排序，等价于 Python 的 list.sort）
fn sort_by_type_id(v: &mut [Value]) {
    v.sort_by(|a, b| type_id_key(a).cmp(&type_id_key(b)));
}

// ══════════════════════════════════════════════════════════════════════════
// 五、T2-1 生活设施
// ══════════════════════════════════════════════════════════════════════════

/// 生成生活设施的内部实现（返回 Vec，方便 [`generate_all`] 复用）—— 对应 `generate_facilities`。
fn gen_life(
    layout: &Value,
    area_name: &str,
    count: Option<i64>,
    seed: Option<i64>,
    types: Option<&Value>,
) -> Vec<Value> {
    let mut state = build_state(layout);
    // 指定类型：只认生活设施表里的键（交通设施由 generate_transport_nodes 负责）
    let keys: Vec<&'static str> = match types {
        Some(Value::Array(a)) if !a.is_empty() => a
            .iter()
            .filter_map(|x| x.as_str())
            .filter_map(life_meta)
            .map(|m| m.key)
            .collect(),
        _ => life_types().iter().map(|m| m.key).collect(),
    };
    // 默认总数按网格面积估算：max(4, size²/26)；显式给定时只做 max(0, ...)
    let count = match count {
        None => (((state.size * state.size) as f64 / 26.0) as i64).max(4),
        Some(c) => c.max(0),
    };
    if keys.is_empty() || !state.free.iter().any(|&b| b) || count <= 0 {
        return Vec::new();
    }
    let area = area_or_name(area_name, layout);
    let mut rng = rng_of(&area, state.size, "life", seed);
    let budget = alloc(
        count,
        &keys
            .iter()
            .map(|k| (k.to_string(), life_meta(k).map(|m| m.weight).unwrap_or(0.0)))
            .collect::<Vec<_>>(),
    );
    let mut facilities: Vec<Value> = Vec::new();
    // 先放占地大/位置要求强的（教育成片、医疗、休闲），再铺住宅与沿街商铺
    let mut order: Vec<&'static str> = keys.clone();
    order.sort_by(|a, b| {
        let ma = life_meta(a).unwrap();
        let mb = life_meta(b).unwrap();
        let ka = (-(ma.foot.0 * ma.foot.1) as f64, -ma.w_cluster);
        let kb = (-(mb.foot.0 * mb.foot.1) as f64, -mb.w_cluster);
        ka.partial_cmp(&kb).unwrap_or(Ordering::Equal)
    });
    for typ in order {
        let Some(meta) = life_meta(typ) else { continue };
        let n = budget
            .iter()
            .find(|(k, _)| k == typ)
            .map(|x| x.1)
            .unwrap_or(0);
        place_type(&mut state, typ, meta, n, &mut rng, &mut facilities, None);
    }
    sort_by_type_id(&mut facilities);
    facilities
}

/// 在 layout 的空格里生成生活设施 —— 对应 `generate_facilities`。
///
/// * `layout`：`district_gen.gen_layout` 的输出（见文件末尾 `demo_layout()` 的约定）
/// * `area_name`：区域名，只用于命名与播种（保证同一区域结果稳定）
/// * `count`：设施总数，`None` 时按网格面积估算（`max(4, size²/26)`）
/// * `seed`：显式随机种子，`None` 时按 `area_name` 派生
/// * `types`：只生成指定的生活设施类型（数组），`None` = 全部 7 类
///
/// 返回 JSON 数组，保证：不与建筑/公园/水体/道路重叠、设施之间互不重叠、id 形如 `R001`。
pub fn generate_facilities(
    layout: &Value,
    area_name: &str,
    count: Option<i64>,
    seed: Option<i64>,
    types: Option<&Value>,
) -> Value {
    Value::Array(gen_life(layout, area_name, count, seed, types))
}

// ══════════════════════════════════════════════════════════════════════════
// 六、T2-2 交通设施（_neighbor_free / _place_bus_stops / generate_transport_nodes）
// ══════════════════════════════════════════════════════════════════════════

/// 取道路格旁边可用的空格（站台在路边，不占路面），顺序做轻微随机化 —— 对应 `_neighbor_free`。
/// 8 邻格的枚举顺序与原型一致（dx 外层、dy 内层），只有 shuffle 的随机序不同。
fn neighbor_free(
    state: &State,
    cell: (i64, i64),
    foot: (i64, i64),
    rng: &mut StdRng,
) -> Option<(i64, i64)> {
    let (x, y) = cell;
    let mut cand: Vec<(i64, i64)> = Vec::with_capacity(8);
    for dx in [-1i64, 0, 1] {
        for dy in [-1i64, 0, 1] {
            if dx != 0 || dy != 0 {
                cand.push((x + dx, y + dy));
            }
        }
    }
    cand.shuffle(rng);
    cand.into_iter().find(|c| fits(state, c.0, c.1, foot))
}

/// 公交站：沿主路每 ~300m 一个（`meters_per_cell` 折算成格）—— 对应 `_place_bus_stops`。
///
/// 按每条主路的走向行进累加里程，够一站就贴在路边放一个站台（不占路面）；
/// 站太近或旁边没空位就再走半站重试；主路不够用时退回「临路空格里挑」的通用放置。
/// 返回本次放下的站台列表（已并入 `facilities`）。
fn place_bus_stops(
    state: &mut State,
    meta: &'static FacType,
    n: i64,
    rng: &mut StdRng,
    facilities: &mut Vec<Value>,
) -> Vec<Value> {
    let step = (300.0 / state.mpc).max(1.0); // 300m 折算成格
    let segs = state.main_segs.clone();
    let mut placed: Vec<Value> = Vec::new();
    let seq0 = facilities
        .iter()
        .filter(|f| str_field(f, "type", "") == "bus_stop")
        .count() as i64;
    let mut used: HashSet<String> = facilities
        .iter()
        .filter_map(|f| f.get("name").and_then(|x| x.as_str()).map(String::from))
        .collect();
    let min_sep = (step * 0.6).max(2.0); // 站间距下限：路口两侧不要挤在一起
    for (x1, y1, x2, y2) in segs {
        let l = (x2 - x1).hypot(y2 - y1);
        if l < 1.0 {
            continue;
        }
        let mut acc = step * 0.5; // 街口先来一站
        let mut t = 0.0;
        while t <= l && (placed.len() as i64) < n {
            if acc >= step {
                let gx = round_n(x1 + (x2 - x1) * t / l, 0) as i64;
                let gy = round_n(y1 + (y2 - y1) * t / l, 0) as i64;
                let spot = neighbor_free(state, (gx, gy), meta.foot, rng);
                let ok = match spot {
                    Some(s) => placed
                        .iter()
                        .all(|p| cheb(s, cell_of(p)) as f64 >= min_sep),
                    None => false,
                };
                if let Some(s) = spot {
                    if ok {
                        let fac = mk_fac(
                            state,
                            meta,
                            "bus_stop",
                            s,
                            seq0 + placed.len() as i64 + 1,
                            &mut used,
                            rng,
                        );
                        placed.push(fac);
                        acc = 0.0;
                    } else {
                        acc = step * 0.5; // 太近/无位置：再走半站重试
                    }
                } else {
                    acc = step * 0.5;
                }
            }
            t += 0.5;
            acc += 0.5;
        }
        if (placed.len() as i64) >= n {
            break;
        }
    }
    facilities.extend(placed.iter().cloned()); // 先并入，后续补足时序号/重名才连续
    if (placed.len() as i64) < n {
        // 主路不够：通用补足（仍要求临路）
        let more = place_type(
            state,
            "bus_stop",
            meta,
            n - placed.len() as i64,
            rng,
            facilities,
            None,
        );
        placed.extend(more);
    }
    placed
}

/// 按层级生成交通节点的内部实现 —— 对应 `generate_transport_nodes`。
fn gen_transport(
    layout: &Value,
    area_name: &str,
    level: &str,
    facilities: Option<&Value>,
    seed: Option<i64>,
) -> Vec<Value> {
    let mut state = build_state(layout);
    // 生活设施已占的格当作障碍，保证交通节点不压上去
    let mut pool: Vec<Value> = facilities
        .and_then(|v| v.as_array())
        .map(|a| a.to_vec())
        .unwrap_or_default();
    for f in pool.iter() {
        let (gx, gy) = cell_of(f);
        let (fw, fh) = (
            num(f, "w", 1.0) as i64,
            num(f, "h", 1.0) as i64,
        );
        for x in gx..gx + fw {
            for y in gy..gy + fh {
                if state.in_bounds(x, y) {
                    let i = state.idx(x, y);
                    state.free[i] = false;
                }
            }
        }
    }
    // 未知层级退回社区级（对应 `level if level in LEVEL_PLAN else 'community'`）
    let plan0 = level_plan_of(level);
    let mut plan: Vec<(&'static str, i64)> = plan0.to_vec();
    // 公交站按主路长度修正（每 ~300m 一个，最多 40 个）
    if let Some(entry) = plan.iter_mut().find(|(k, _)| *k == "bus_stop") {
        let n_by_road = if state.main_road_len != 0.0 {
            (state.main_road_len * state.mpc / 300.0) as i64
        } else {
            0
        };
        entry.1 = entry.1.max(n_by_road.min(40));
    }
    let area = area_or_name(area_name, layout);
    let mut rng = rng_of(&area, state.size, "traffic", seed);
    let mut out: Vec<Value> = Vec::new();
    for typ in TRANSPORT_ORDER {
        let n = plan
            .iter()
            .find(|(k, _)| *k == typ)
            .map(|x| x.1)
            .unwrap_or(0);
        if n <= 0 {
            continue;
        }
        let Some(meta) = transport_meta(typ) else { continue };
        if meta.need_water && !state.has_water {
            continue; // 无水体：码头跳过
        }
        if typ == "bus_stop" {
            out.extend(place_bus_stops(&mut state, meta, n, &mut rng, &mut pool));
            continue;
        }
        // 停车场优先贴近商业/休闲/市政设施（出行节点跟人流走）
        let pref: Option<Vec<(i64, i64)>> = if typ == "parking" {
            let v: Vec<(i64, i64)> = pool
                .iter()
                .filter(|f| {
                    matches!(
                        str_field(f, "type", ""),
                        "commercial" | "leisure" | "civic"
                    )
                })
                .map(cell_of)
                .collect();
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        } else {
            None
        };
        out.extend(place_type(
            &mut state,
            typ,
            meta,
            n,
            &mut rng,
            &mut pool,
            pref.as_deref(),
        ));
    }
    sort_by_type_id(&mut out);
    out
}

/// 按层级生成交通节点 —— 对应 `generate_transport_nodes`。
///
/// `level`：`community`(小区) | `district`(区县) | `city`(城市)（未知值退回 `community`）
///   · 公交站：沿主路每 ~300m 一个（`meters_per_cell` 换算成格），上限 40
///   · 地铁站：稀疏（约 1km 一站），社区级不出现
///   · 机场只在城市级，火车站区县级起
///   · 码头必须临水（无水体则自动跳过）
/// * `facilities`：可选，已生成的生活设施——会被避让（不重叠），停车场会靠近商业/休闲设施
///
/// 返回结构与 [`generate_facilities`] 相同（`group='transport'`），并已按 `(type, id)` 排序。
/// 注意：返回的是**本次交通节点**，不是「生活 + 交通」的全集。
pub fn generate_transport_nodes(
    layout: &Value,
    area_name: &str,
    level: &str,
    facilities: Option<&Value>,
    seed: Option<i64>,
) -> Value {
    Value::Array(gen_transport(layout, area_name, level, facilities, seed))
}

// ══════════════════════════════════════════════════════════════════════════
// 七、组合生成
// ══════════════════════════════════════════════════════════════════════════

/// 一次生成生活设施 + 交通节点，并附统计 —— 对应 `generate_all`。
/// 交通设施用 `seed + 1`（不同于生活设施，避免两次摆放共用同一条随机序列）。
pub fn generate_all(
    layout: &Value,
    area_name: &str,
    count: Option<i64>,
    level: &str,
    seed: Option<i64>,
) -> Value {
    let life = generate_facilities(layout, area_name, count, seed, None);
    let next = seed.map(|s| s.wrapping_add(1));
    let trans = generate_transport_nodes(layout, area_name, level, Some(&life), next);
    let mut all: Vec<Value> = life.as_array().cloned().unwrap_or_default();
    all.extend(trans.as_array().cloned().unwrap_or_default());
    json!({
        "area": area_name,
        "level": level,
        "size": nz(layout, "size", 20.0) as i64,
        "facilities": life,
        "transport": trans,
        "stats": facility_stats(&Value::Array(all)),
    })
}

// ══════════════════════════════════════════════════════════════════════════
// 八、查询
// ══════════════════════════════════════════════════════════════════════════

/// 按类型/名称/位置筛选设施 —— 对应 `find_facilities`。
///
/// * `type_`：字符串或字符串数组（对应 Python 的 `type`，Rust 里 `type` 是关键字）
/// * `name`：子串匹配
/// * `near` + `radius`：给定坐标（格）时按距离升序，每条结果追加 `dist` 字段（保留 2 位小数）
/// * `group`：`life` | `transport`
/// * `limit`：截断（`Some(0)` 与 None 一样不截断——原型是 `if limit:`，0 是假值）
pub fn find_facilities(
    facilities: &Value,
    type_: Option<&Value>,
    name: Option<&str>,
    near: Option<(f64, f64)>,
    radius: Option<f64>,
    group: Option<&str>,
    limit: Option<usize>,
) -> Value {
    let ts: Option<Vec<String>> = match type_ {
        Some(Value::String(s)) => Some(vec![s.clone()]),
        Some(Value::Array(a)) => {
            let v: Vec<String> = a
                .iter()
                .filter_map(|x| x.as_str())
                .map(String::from)
                .collect();
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        }
        _ => None,
    };
    let list = facilities.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let mut out: Vec<Value> = Vec::new();
    for f in list {
        if let Some(ts) = &ts {
            if !ts.iter().any(|t| t == str_field(f, "type", "\u{0}")) {
                continue;
            }
        }
        if let Some(g) = group {
            if !g.is_empty() && str_field(f, "group", "life") != g {
                continue;
            }
        }
        if let Some(nm) = name {
            if !nm.is_empty() && !str_field(f, "name", "").contains(nm) {
                continue;
            }
        }
        match near {
            Some(n) => {
                let (gx, gy) = cell_of(f);
                let d = (gx as f64 - n.0).hypot(gy as f64 - n.1);
                if let Some(r) = radius {
                    if d > r {
                        continue;
                    }
                }
                // dict(f, dist=round(d, 2))：复制一份并追加 dist（键插在最后）
                let mut o = f.clone();
                if let Value::Object(m) = &mut o {
                    m.insert("dist".to_string(), json!(round_n(d, 2)));
                }
                out.push(o);
            }
            None => out.push(f.clone()),
        }
    }
    if near.is_some() {
        out.sort_by(|a, b| {
            num(a, "dist", f64::MAX)
                .partial_cmp(&num(b, "dist", f64::MAX))
                .unwrap_or(Ordering::Equal)
        });
    }
    if let Some(n) = limit {
        if n > 0 && out.len() > n {
            out.truncate(n);
        }
    }
    Value::Array(out)
}

/// 命中测试：返回覆盖 `(gx, gy)` 的设施（考虑 footprint），没有则 `null` —— 对应 `facility_at`。
pub fn facility_at(facilities: &Value, gx: i64, gy: i64) -> Value {
    let list = facilities.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    for f in list {
        let (fx, fy) = cell_of(f);
        let (fw, fh) = (num(f, "w", 1.0) as i64, num(f, "h", 1.0) as i64);
        if fx <= gx && gx < fx + fw && fy <= gy && gy < fy + fh {
            return f.clone();
        }
    }
    Value::Null
}

/// 分类统计：总数 / 按类型 / 按分组 / 类型中文名 —— 对应 `facility_stats`。
/// 键顺序 = 首次出现的顺序（本工程 serde_json 开了 preserve_order，与 Python dict 一致）。
pub fn facility_stats(facilities: &Value) -> Value {
    let list = facilities.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let mut by_type: Map<String, Value> = Map::new();
    let mut by_group: Map<String, Value> = Map::new();
    for f in list {
        // 原型是 f['type']（缺了会 KeyError）；这里缺失按空串归类，不 panic
        let t = str_field(f, "type", "").to_string();
        let n = by_type.get(&t).and_then(|v| v.as_i64()).unwrap_or(0) + 1;
        by_type.insert(t, json!(n));
        let g = str_field(f, "group", "life").to_string();
        let n = by_group.get(&g).and_then(|v| v.as_i64()).unwrap_or(0) + 1;
        by_group.insert(g, json!(n));
    }
    let mut zh: Map<String, Value> = Map::new();
    for k in by_type.keys() {
        let name = any_meta(k).map(|m| m.zh).unwrap_or(k.as_str());
        zh.insert(k.clone(), json!(name));
    }
    json!({
        "total": list.len(),
        "by_type": Value::Object(by_type),
        "by_group": Value::Object(by_group),
        "zh": Value::Object(zh),
    })
}

/// 给前端的元数据表（图标/中文名/配色/尺寸/分组/层级计划）—— 对应 `types_payload`，
/// 前端 `facilities.js` 是同源镜像。`types` 的键顺序 = 原型 `ALL_TYPES`（生活 7 类 + 交通 7 类）。
pub fn types_payload() -> Value {
    let mut out: Map<String, Value> = Map::new();
    for m in ALL_TYPES.iter() {
        out.insert(
            m.key.to_string(),
            json!({
                "zh": m.zh, "icon": m.icon,
                "color": [m.color.0, m.color.1, m.color.2],
                "group": m.group, "prefix": m.prefix,
                "w": m.foot.0, "h": m.foot.1,
                "group_zh": group_zh(m.group),
            }),
        );
    }
    let mut levels: Map<String, Value> = Map::new();
    for (k, _) in LEVEL_PLAN.iter() {
        levels.insert(k.to_string(), json!(level_zh(k)));
    }
    let mut plan: Map<String, Value> = Map::new();
    for (k, rows) in LEVEL_PLAN.iter() {
        let mut m: Map<String, Value> = Map::new();
        for (t, n) in rows.iter() {
            m.insert(t.to_string(), json!(n));
        }
        plan.insert(k.to_string(), Value::Object(m));
    }
    json!({
        "types": Value::Object(out),
        "levels": Value::Object(levels),
        "level_plan": Value::Object(plan),
        "cell_meters": DEFAULT_CELL_METERS,
    })
}

// ══════════════════════════════════════════════════════════════════════════
// 九、调试出图（Python 侧是 PIL；这里给出无依赖的矢量预览）
// ══════════════════════════════════════════════════════════════════════════

/// 把设施标记画成一张 **SVG 文本**（调试/预览用）—— 对应原型 `draw_facilities` 的画法，
/// 但**不依赖任何图像库**（本工程不许新增依赖，Rust 侧也没有 PIL）。
///
/// 复刻原型的取景：`pad=40`、`cell=min((W-2pad)/size, (H-2pad)/size)`、居中留边；
/// 每个设施一个描白边的色块，格子够大时写名字（原型的 PIL 版还会叠一层 `district_gen` 底图，
/// 这里只画设施层——底图请用 Python 原型或前端页面看）。
/// 返回 SVG 文本；调用方负责落盘（HTTP 层可直接当 `image/svg+xml` 返回）。
pub fn facilities_svg(layout: &Value, facilities: &Value, w: f64, h: f64) -> String {
    let size = (nz(layout, "size", 20.0) as i64).max(4);
    let pad = 40.0f64;
    let cell = ((w - 2.0 * pad) / size as f64).min((h - 2.0 * pad) / size as f64);
    let (ox, oy) = (
        (w - cell * size as f64) / 2.0,
        (h - cell * size as f64) / 2.0,
    );
    let mut s = String::new();
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        w, h, w, h
    ));
    s.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#2e422e\"/>");
    // 网格底纹（原型是画底图，这里退化成格线，便于核对坐标）
    for i in 0..=size {
        let p = cell * i as f64;
        s.push_str(&format!(
            "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"#3d553d\" stroke-width=\"1\"/>",
            ox + p, oy, ox + p, oy + cell * size as f64
        ));
        s.push_str(&format!(
            "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"#3d553d\" stroke-width=\"1\"/>",
            ox, oy + p, ox + cell * size as f64, oy + p
        ));
    }
    let list = facilities.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    for f in list {
        let (gx, gy) = cell_of(f);
        let (fw, fh) = (num(f, "w", 1.0) as i64, num(f, "h", 1.0) as i64);
        let (r, g, b) = match f.get("color").and_then(|c| c.as_array()) {
            Some(c) if c.len() >= 3 => (
                c[0].as_u64().unwrap_or(200) as u8,
                c[1].as_u64().unwrap_or(200) as u8,
                c[2].as_u64().unwrap_or(200) as u8,
            ),
            _ => (200, 200, 200),
        };
        let (x0, y0) = (ox + gx as f64 * cell, oy + gy as f64 * cell);
        let (x1, y1) = (ox + (gx + fw) as f64 * cell, oy + (gy + fh) as f64 * cell);
        s.push_str(&format!(
            "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"#{:02x}{:02x}{:02x}\" stroke=\"#ffffff\" stroke-width=\"2\"/>",
            x0 + 1.0, y0 + 1.0, (x1 - x0) - 2.0, (y1 - y0) - 2.0, r, g, b
        ));
        if cell > 14.0 {
            let nm: String = str_field(f, "name", "").chars().take(5).collect();
            s.push_str(&format!(
                "<text x=\"{:.2}\" y=\"{:.2}\" font-size=\"11\" fill=\"#191e28\">{}</text>",
                x0 + 3.0,
                y0 + 12.0,
                xml_escape(&nm)
            ));
        }
    }
    s.push_str("</svg>");
    s
}

/// SVG 文本转义（名字里有 `&`/`<` 时不至于把文件写坏）
fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

/// **未移植**：原型用 PIL 把设施叠画到 PNG 上（还会复用 `district_gen.draw_district` 出底图）。
/// Rust 侧不引图像库（本工程只能离线用已缓存 crate，没有 PIL/image），所以这里不写文件，
/// 只回一个说明对象；预览请用 [`facilities_svg`]（无依赖矢量图）或 Python 原型 / 前端页面。
/// 返回 `{ok, path, error, hint}`——保持与原型「返回出图路径」的调用形状一致，
/// HTTP 层可以照旧透传，只是 `ok=false`。
pub fn draw_facilities(
    _layout: &Value,
    _facilities: &Value,
    out_path: &str,
    _style: &str,
    _w: f64,
    _h: f64,
) -> Value {
    json!({
        "ok": false,
        "path": out_path,
        "error": "Rust 侧未移植 PIL 栅格出图（不引图像库）",
        "hint": "用 facilities_svg(layout, facilities, w, h) 拿无依赖矢量预览，或用 Python 原型/前端页面",
    })
}

// ══════════════════════════════════════════════════════════════════════════
// 十、单元测试（对应 Python 原型 `__main__` 里那段自检）
// ══════════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 与原型 `_demo_layout()` 逐字段相同的那张 24×24 自检图
    fn demo_layout() -> Value {
        json!({
            "name": "越秀测试小区", "size": 24, "meters_per_cell": 30,
            "buildings": [
                {"x": 1, "y": 1, "w": 3, "h": 2, "type": "residential", "name": "1号楼"},
                {"x": 5, "y": 1, "w": 3, "h": 2, "type": "residential", "name": "2号楼"},
                {"x": 1, "y": 5, "w": 3, "h": 2, "type": "residential", "name": "3号楼"},
                {"x": 9, "y": 8, "w": 4, "h": 3, "type": "commercial", "name": "沿街商铺"},
                {"x": 16, "y": 2, "w": 3, "h": 2, "type": "office", "name": "写字楼"},
                {"x": 2, "y": 14, "w": 4, "h": 3, "type": "residential", "name": "4号楼"},
                {"x": 14, "y": 14, "w": 3, "h": 3, "type": "residential", "name": "5号楼"},
                {"x": 20, "y": 18, "w": 3, "h": 2, "type": "shop", "name": "临街店面"}
            ],
            "roads": [
                {"x1": 0, "y1": 4, "x2": 24, "y2": 4, "type": "main"},
                {"x1": 8, "y1": 0, "x2": 8, "y2": 24, "type": "main"},
                {"x1": 0, "y1": 12, "x2": 24, "y2": 12, "type": "secondary"},
                {"x1": 18, "y1": 0, "x2": 18, "y2": 24, "type": "secondary"}
            ],
            "parks": [{"x": 12, "y": 6, "w": 4, "h": 3}],
            "water": [{"x": 0, "y": 20, "w": 10, "h": 4}],
        })
    }

    fn facs(v: &Value) -> Vec<Value> {
        v.as_array().cloned().unwrap_or_default()
    }

    fn count_type(fs: &[Value], t: &str) -> usize {
        fs.iter().filter(|f| str_field(f, "type", "") == t).count()
    }

    fn ser(v: &Value) -> String {
        serde_json::to_string(v).unwrap()
    }

    /// 原型 `_overlap_check`：越界 / 压建筑 / 压绿地水体 / 压道路 / 设施互相重叠
    fn overlap_check(layout: &Value, facilities: &[Value]) -> Vec<String> {
        let size = nz(layout, "size", 20.0) as i64;
        let mut bset: HashSet<(i64, i64)> = HashSet::new();
        for b in items(layout, "buildings") {
            let (x0, y0, x1, y1) = rect_range(
                num(b, "x", 0.0),
                num(b, "y", 0.0),
                nz(b, "w", 1.0),
                nz(b, "h", 1.0),
                size,
            );
            for y in y0..y1 {
                for x in x0..x1 {
                    bset.insert((x, y));
                }
            }
        }
        let mut park: HashSet<(i64, i64)> = HashSet::new();
        for p in items(layout, "parks") {
            let (x0, y0, x1, y1) = rect_range(
                num(p, "x", 0.0),
                num(p, "y", 0.0),
                nz(p, "w", 1.0),
                nz(p, "h", 1.0),
                size,
            );
            for y in y0..y1 {
                for x in x0..x1 {
                    park.insert((x, y));
                }
            }
        }
        let mut water: HashSet<(i64, i64)> = HashSet::new();
        for w in items(layout, "water") {
            let (x0, y0, x1, y1) = rect_range(
                num(w, "x", 0.0),
                num(w, "y", 0.0),
                nz(w, "w", 1.0),
                nz(w, "h", 1.0),
                size,
            );
            for y in y0..y1 {
                for x in x0..x1 {
                    water.insert((x, y));
                }
            }
        }
        let mut road: HashSet<(i64, i64)> = HashSet::new();
        for r in items(layout, "roads") {
            for c in seg_cells(
                num(r, "x1", 0.0),
                num(r, "y1", 0.0),
                num(r, "x2", 0.0),
                num(r, "y2", 0.0),
                size,
            ) {
                road.insert(c);
            }
        }
        let mut seen: HashMap<(i64, i64), String> = HashMap::new();
        let mut errs: Vec<String> = Vec::new();
        for f in facilities {
            let (gx, gy) = cell_of(f);
            let (fw, fh) = (num(f, "w", 1.0) as i64, num(f, "h", 1.0) as i64);
            let id = str_field(f, "id", "?").to_string();
            for x in gx..gx + fw {
                for y in gy..gy + fh {
                    let c = (x, y);
                    if !(0..size).contains(&x) || !(0..size).contains(&y) {
                        errs.push(format!("{id} 越界 {c:?}"));
                    }
                    if bset.contains(&c) {
                        errs.push(format!("{id} 压建筑 {c:?}"));
                    }
                    if park.contains(&c) || water.contains(&c) {
                        errs.push(format!("{id} 压绿地/水体 {c:?}"));
                    }
                    if road.contains(&c) {
                        errs.push(format!("{id} 压道路 {c:?}"));
                    }
                    if let Some(o) = seen.get(&c) {
                        errs.push(format!("{id} 与 {o} 重叠 {c:?}"));
                    }
                    seen.insert(c, id.clone());
                }
            }
        }
        errs
    }

    #[test]
    fn seven_life_types_are_all_generated() {
        let lay = demo_layout();
        let life = generate_facilities(&lay, "广州·越秀", Some(26), None, None);
        let fs = facs(&life);
        assert_eq!(fs.len(), 26, "count=26 时 24×24 网格放得下 26 个生活设施");
        for m in life_types() {
            let n = count_type(&fs, m.key);
            assert!(n >= 1, "{} 一个都没生成", m.key);
            assert_eq!(m.group, "life");
            for f in fs.iter().filter(|f| str_field(f, "type", "") == m.key) {
                assert_eq!(str_field(f, "type_zh", ""), m.zh);
                assert_eq!(str_field(f, "icon", ""), m.icon);
                assert_eq!(str_field(f, "group", ""), "life");
                assert_eq!(num(f, "w", 0.0) as i64, m.foot.0);
                assert_eq!(num(f, "h", 0.0) as i64, m.foot.1);
                assert_eq!(num(f, "cell_meters", 0.0), 30.0);
                let id = str_field(f, "id", "");
                assert!(id.starts_with(m.prefix), "{id} 前缀应为 {}", m.prefix);
                assert_eq!(id.len(), 4, "{id} 形如 R001");
            }
        }
        // 名额分摊是纯确定性的（与随机无关），应当与 Python 原型逐个一致
        for (k, want) in [
            ("residential", 8),
            ("commercial", 5),
            ("education", 3),
            ("medical", 2),
            ("leisure", 4),
            ("civic", 2),
            ("lodging", 2),
        ] {
            assert_eq!(count_type(&fs, k), want, "{k} 的名额应为 {want}");
        }
    }

    #[test]
    fn level_plan_differs_by_tier() {
        let lay = demo_layout();
        let life = generate_facilities(&lay, "广州·越秀", Some(26), None, None);
        let com = facs(&generate_transport_nodes(&lay, "广州·越秀", "community", Some(&life), None));
        let dis = facs(&generate_transport_nodes(&lay, "广州·越秀", "district", Some(&life), None));
        let cit = facs(&generate_transport_nodes(&lay, "广州·越秀", "city", Some(&life), None));
        // 三级数量单调递增（原型自检：5 / 11 / 24）
        assert_eq!(com.len(), 5, "社区级 = 公交 4(按主路 48 格×30m 修正) + 停车场 1");
        assert_eq!(dis.len(), 11);
        assert_eq!(cit.len(), 24);
        // 层级差异：地铁从区县级起、机场只在城市级
        assert_eq!(count_type(&com, "subway"), 0);
        assert_eq!(count_type(&com, "airport"), 0);
        assert_eq!(count_type(&com, "train_station"), 0);
        assert_eq!(count_type(&dis, "subway"), 1);
        assert_eq!(count_type(&dis, "train_station"), 1);
        assert_eq!(count_type(&dis, "airport"), 0);
        assert_eq!(count_type(&cit, "airport"), 1);
        assert_eq!(count_type(&cit, "subway"), 3);
        assert_eq!(count_type(&cit, "gas"), 2);
        assert_eq!(count_type(&cit, "parking"), 6);
        assert_eq!(count_type(&cit, "bus_stop"), 10);
        // 未知 level 退回社区级
        assert_eq!(
            ser(&generate_transport_nodes(&lay, "广州·越秀", "galaxy", None, None)),
            ser(&generate_transport_nodes(&lay, "广州·越秀", "community", None, None))
        );
        // 交通节点也是 group=transport、id 形如 B001
        for f in com.iter().chain(dis.iter()).chain(cit.iter()) {
            assert_eq!(str_field(f, "group", ""), "transport");
            assert_eq!(str_field(f, "id", "").len(), 4);
        }
    }

    #[test]
    fn same_input_gives_identical_output() {
        let lay = demo_layout();
        let a = generate_all(&lay, "广州·越秀", Some(26), "city", None);
        let b = generate_all(&lay, "广州·越秀", Some(26), "city", None);
        assert_eq!(ser(&a), ser(&b), "同输入两次生成必须逐字节一致");
        let c = generate_all(&lay, "广州·越秀", Some(26), "city", Some(7));
        let d = generate_all(&lay, "广州·越秀", Some(26), "city", Some(7));
        assert_eq!(ser(&c), ser(&d), "显式 seed 也必须稳定");
        // 换区域名 → 换种子 → 换结果（这条兜住「种子真的接上了」）
        assert_ne!(
            ser(&generate_facilities(&lay, "甲区", Some(26), None, None)),
            ser(&generate_facilities(&lay, "乙区", Some(26), None, None))
        );
        // 生活用 salt=life、交通用 salt=traffic，两条种子链路不同
        assert_ne!(seed_of("甲区", 24, "life"), seed_of("甲区", 24, "traffic"));
        assert_eq!(seed_of("甲区", 24, "life"), seed_of("甲区", 24, "life"));
    }

    #[test]
    fn facilities_never_overlap_buildings_water_parks_or_roads() {
        let lay = demo_layout();
        let pack = generate_all(&lay, "广州·越秀", Some(26), "city", None);
        let life = facs(&pack["facilities"]);
        let trans = facs(&pack["transport"]);
        assert!(overlap_check(&lay, &life).is_empty(), "生活设施不许压建筑/路/水/绿地: {:?}", overlap_check(&lay, &life));
        assert!(overlap_check(&lay, &trans).is_empty(), "交通节点不许压建筑/路/水/绿地: {:?}", overlap_check(&lay, &trans));
        let mut both = life.clone();
        both.extend(trans.clone());
        assert!(overlap_check(&lay, &both).is_empty(), "两类设施之间也不许重叠");
        // 生活设施 + 交通节点 = stats.total
        assert_eq!(pack["stats"]["total"], json!(both.len()));
        assert_eq!(pack["stats"]["by_group"]["life"], json!(life.len()));
        assert_eq!(pack["stats"]["by_group"]["transport"], json!(trans.len()));
        assert_eq!(pack["area"], json!("广州·越秀"));
        assert_eq!(pack["size"], json!(24));
    }

    #[test]
    fn dist_field_and_occupancy_edges() {
        // 空 layout：size 兜底 20，全空
        let st = build_state(&json!({}));
        assert_eq!(st.size, 20);
        assert!(st.free.iter().all(|&b| b));
        assert!(st.road_dist.iter().all(|&d| d == FAR), "没有路 → 距离场全不可达");
        assert_eq!(st.at(&st.road_dist, 0, 0), FAR);
        assert_eq!(st.at(&st.road_dist, 999, 999), FAR, "越界也按不可达");
        assert!(!st.has_water);

        // 越界矩形被裁掉：(-3,-3,3,3) 一个格都不占
        let st = build_state(&json!({"size": 8, "buildings": [{"x": -3, "y": -3, "w": 3, "h": 3}]}));
        assert!(st.free.iter().all(|&b| b), "完全在界外的矩形不占任何格");
        // 水体从 (6,6) 铺到边界外 → 裁成 2×2
        let st = build_state(&json!({"size": 8, "water": [{"x": 6, "y": 6, "w": 99, "h": 99}]}));
        assert!(st.has_water);
        assert_eq!(st.at(&st.water_dist, 6, 6), 0);
        assert_eq!(st.at(&st.water_dist, 7, 7), 0);
        assert_eq!(st.at(&st.water_dist, 5, 5), 1, "切比雪夫距离");
        assert_eq!(st.at(&st.water_dist, 0, 0), 6);
        assert!(!st.is_free(6, 6));
        assert!(st.is_free(5, 5));
        assert!(!st.is_free(-1, 0), "越界格不算空格");
        // 建筑 w/h 为 0 时按 1 处理（原型的 `or 1`）
        let st = build_state(&json!({"size": 8, "buildings": [{"x": 2, "y": 2, "w": 0, "h": 0}]}));
        assert!(!st.is_free(2, 2));
        assert!(st.is_free(3, 3));

        // 线段栅格化：水平线 (0,4)→(8,4) 在 size=8 里覆盖 y=4 的 0..8 共 8 格
        let cells = seg_cells(0.0, 4.0, 8.0, 4.0, 8);
        assert_eq!(cells.len(), 8);
        assert!(cells.iter().all(|c| c.1 == 4));
        assert!(cells.iter().all(|c| (0..8).contains(&c.0)));
        // 零长线段退化成 1 个采样点（原型 n = max(1, ...)）
        assert_eq!(seg_cells(3.0, 3.0, 3.0, 3.0, 8), vec![(3, 3)]);
        // 全在界外的线段 → 空
        assert!(seg_cells(-5.0, -5.0, -1.0, -1.0, 8).is_empty());
        // 空源点表 → 距离场全 FAR
        assert!(dist_field(4, &[false; 16]).iter().all(|&d| d == FAR));
        // 多源：四角全是源 → 每格都在 1 以内
        let mut src = vec![false; 16];
        for i in [0usize, 3, 12, 15] {
            src[i] = true;
        }
        let d = dist_field(4, &src);
        assert!(d.iter().all(|&x| x <= 1));

        // _fits 边界：3×2 的 footprint 放在 size=8 的右下角放不下
        let st = build_state(&json!({"size": 8}));
        assert!(fits(&st, 5, 6, (3, 2)));
        assert!(!fits(&st, 6, 6, (3, 2)), "越过右边界");
        assert!(!fits(&st, 5, 7, (3, 2)), "越过下边界");
        assert!(!fits(&st, -1, 0, (1, 1)), "负坐标放不下");
    }

    #[test]
    fn extreme_inputs_do_not_panic() {
        let layouts = [
            json!(null),
            json!({}),
            json!([]),
            json!({"size": 0}),
            json!({"size": -5}),
            json!({"size": 1}),
            json!({"size": "abc"}),
            json!({"size": 4, "buildings": [{"x": "x", "w": null}]}),
            json!({"size": 6, "roads": [{"x1": 0, "y1": 0, "x2": 6, "y2": 6}]}),
        ];
        for lay in layouts.iter() {
            for lvl in ["community", "district", "city", "???"] {
                let pack = generate_all(lay, "", None, lvl, None);
                assert!(pack["facilities"].is_array());
                assert!(pack["transport"].is_array());
                assert!(pack["stats"]["total"].is_u64());
            }
            let _ = find_facilities(&json!([]), None, None, None, None, None, None);
            let _ = facility_at(&json!(null), 0, 0);
        }
        // 全是建筑 → 没有空格 → 一个都放不出来
        let full = json!({"size": 6, "buildings": [{"x": 0, "y": 0, "w": 6, "h": 6}]});
        assert!(facs(&generate_facilities(&full, "x", None, None, None)).is_empty());
        assert!(facs(&generate_transport_nodes(&full, "x", "city", None, None)).is_empty());
        // count=0 / 负数
        assert!(facs(&generate_facilities(&demo_layout(), "x", Some(0), None, None)).is_empty());
        assert!(facs(&generate_facilities(&demo_layout(), "x", Some(-9), None, None)).is_empty());
        // 空类型表 → 空结果（对应原型 `types or LIFE_TYPES` 里的空数组也走「全部」，
        // 但只有非生活类型时 keys 为空）
        assert!(facs(&generate_facilities(&demo_layout(), "x", None, None, Some(&json!(["bus_stop"])))).is_empty());
        // 超小网格 + 一条主路：能跑通，不 panic
        let tiny = json!({"size": 1, "roads": [{"x1": 0, "y1": 0, "x2": 1, "y2": 0, "type": "main"}]});
        let _ = generate_all(&tiny, "t", Some(3), "city", None);
        // 超大 size 走安全上限，不会 OOM
        assert_eq!(build_state(&json!({"size": 100000})).size, MAX_SIZE);
        assert_eq!(build_state(&json!({"size": 3})).size, 4, "size 下限 4");
        // 畸形线段（超长）不会把栅格化跑爆
        let st = build_state(&json!({"size": 8, "roads": [{"x1": 0, "y1": 0, "x2": 1e18, "y2": 0, "type": "main"}]}));
        assert!(st.main_road_len > 0.0);
    }

    #[test]
    fn bus_stops_sit_on_the_roadside() {
        let lay = demo_layout();
        let st = build_state(&lay);
        let tr = facs(&generate_transport_nodes(&lay, "广州·越秀", "district", None, None));
        let bus: Vec<&Value> = tr
            .iter()
            .filter(|f| str_field(f, "type", "") == "bus_stop")
            .collect();
        assert_eq!(bus.len(), 5, "区县级计划 5 个公交站");
        for f in &bus {
            let (gx, gy) = cell_of(f);
            assert!(
                st.at(&st.road_dist, gx, gy) <= 1,
                "{}@({gx},{gy}) 必须临路",
                str_field(f, "id", "")
            );
            assert_eq!((num(f, "w", 0.0) as i64, num(f, "h", 0.0) as i64), (1, 1));
        }
        // 间距下限：站台之间至少 2 格（原型 min_sep = max(2, step*0.6) 的下限）
        for (i, a) in bus.iter().enumerate() {
            let d = bus
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, b)| cheb(cell_of(a), cell_of(b)))
                .min()
                .unwrap_or(i64::MAX);
            assert!(d >= 2, "公交站间距不得小于 2 格，实测 {d}");
        }
        // 主路长度修正：每 ~300m 一站 → 48 格 × 30m / 300 = 4，社区级计划 2 被抬到 4
        assert!(((st.main_road_len * st.mpc / 300.0) as i64) == 4);
        let com = facs(&generate_transport_nodes(&lay, "广州·越秀", "community", None, None));
        assert_eq!(count_type(&com, "bus_stop"), 4);
    }

    #[test]
    fn pier_needs_water_and_skips_without_it() {
        let lay = demo_layout();
        let st = build_state(&lay);
        let tr = facs(&generate_transport_nodes(&lay, "广州·越秀", "city", None, None));
        let piers: Vec<&Value> = tr
            .iter()
            .filter(|f| str_field(f, "type", "") == "pier")
            .collect();
        assert_eq!(piers.len(), 1, "城市级 1 个码头");
        assert!(piers.iter().all(|f| {
            let (gx, gy) = cell_of(f);
            st.at(&st.water_dist, gx, gy) <= 1
        }), "码头必须临水");
        // 没有水体 → 整类跳过（不是报错）；别的交通设施照常生成。
        // 注意用宽一点的图：12×12 那种窄图里「先放的机场/加油站/停车场把临路格吃干净」
        // 是完全合法的结果（随机序不同就可能没有公交站），不适合当断言。
        let dry = json!({"size": 20, "meters_per_cell": 30, "roads": [{"x1": 0, "y1": 5, "x2": 20, "y2": 5, "type": "main"}]});
        let tr2 = facs(&generate_transport_nodes(&dry, "旱地", "city", None, None));
        assert_eq!(count_type(&tr2, "pier"), 0);
        assert!(!tr2.is_empty(), "别的交通设施照常生成");
    }

    #[test]
    fn transport_avoids_life_facilities() {
        let lay = demo_layout();
        let life = generate_facilities(&lay, "广州·越秀", Some(26), None, None);
        let tr = generate_transport_nodes(&lay, "广州·越秀", "city", Some(&life), None);
        let mut both = facs(&life);
        both.extend(facs(&tr));
        assert!(overlap_check(&lay, &both).is_empty(), "交通节点必须避让生活设施");
        // 停车场优先贴商业/休闲/市政：pref 锚点存在时至少有一个停车场落在 2 格内
        let life_cells: Vec<(i64, i64)> = facs(&life)
            .iter()
            .filter(|f| matches!(str_field(f, "type", ""), "commercial" | "leisure" | "civic"))
            .map(cell_of)
            .collect();
        assert!(!life_cells.is_empty(), "这张图上有商业/休闲/市政设施");
        let near = facs(&tr)
            .iter()
            .filter(|f| str_field(f, "type", "") == "parking")
            .any(|f| life_cells.iter().map(|c| cheb(cell_of(f), *c)).min().unwrap_or(99) <= 2);
        assert!(near, "至少有一个停车场贴着人流节点");
    }

    #[test]
    fn query_helpers_match_python_semantics() {
        let lay = demo_layout();
        let life = generate_facilities(&lay, "广州·越秀", Some(26), None, None);
        let n_all = facs(&life).len();
        // 按类型（字符串）
        let edu = facs(&find_facilities(&life, Some(&json!("education")), None, None, None, None, None));
        assert!(!edu.is_empty());
        assert!(edu.iter().all(|f| str_field(f, "type", "") == "education"));
        // 按类型（数组）
        let two = facs(&find_facilities(
            &life,
            Some(&json!(["education", "medical"])),
            None,
            None,
            None,
            None,
            None,
        ));
        assert_eq!(two.len(), edu.len() + count_type(&facs(&life), "medical"));
        // 按名称子串
        let sup = facs(&find_facilities(&life, None, Some("超市"), None, None, None, None));
        assert!(!sup.is_empty());
        assert!(sup.iter().all(|f| str_field(f, "name", "").contains("超市")));
        // near + radius + limit + dist 升序
        let near = facs(&find_facilities(&life, None, None, Some((12.0, 12.0)), Some(4.0), None, Some(3)));
        assert!(!near.is_empty() && near.len() <= 3);
        let ds: Vec<f64> = near.iter().map(|f| num(f, "dist", -1.0)).collect();
        assert!(ds.iter().all(|&d| (0.0..=4.0).contains(&d)));
        assert!(ds.windows(2).all(|w| w[0] <= w[1]), "dist 必须升序");
        // dist 字段是复制出来的，原数组不受污染
        assert!(facs(&life).iter().all(|f| f.get("dist").is_none()));
        // group 过滤
        assert_eq!(facs(&find_facilities(&life, None, None, None, None, Some("life"), None)).len(), n_all);
        assert_eq!(facs(&find_facilities(&life, None, None, None, None, Some("transport"), None)).len(), 0);
        // limit=0 在原型里是假值 → 不截断
        assert_eq!(facs(&find_facilities(&life, None, None, None, None, None, Some(0))).len(), n_all);
        assert_eq!(facs(&find_facilities(&life, None, None, None, None, None, Some(2))).len(), 2);
        // 命中测试（考虑 footprint）
        let life_arr = facs(&life);
        let first = &life_arr[0];
        let (gx, gy) = cell_of(first);
        assert_eq!(facility_at(&life, gx, gy), first.clone());
        assert!(facility_at(&life, -1, -1).is_null());
        assert!(facility_at(&json!([]), 0, 0).is_null());
        // 统计
        let s = facility_stats(&life);
        assert_eq!(s["total"], json!(n_all));
        assert_eq!(s["by_group"]["life"], json!(n_all));
        assert_eq!(s["zh"]["residential"], json!("住宅"));
        // 交通类型的中文名也能查到（stats 对任意设施数组都成立）
        let tr = generate_transport_nodes(&lay, "广州·越秀", "city", Some(&life), None);
        let mut both = facs(&life);
        both.extend(facs(&tr));
        let s2 = facility_stats(&Value::Array(both));
        assert_eq!(s2["zh"]["bus_stop"], json!("公交站"));
        assert_eq!(s2["by_group"]["transport"], json!(facs(&tr).len()));
        let empty = facility_stats(&json!([]));
        assert_eq!(empty["total"], json!(0));
        assert_eq!(empty["by_type"], json!({}));
    }

    #[test]
    fn types_payload_shape_and_order() {
        let p = types_payload();
        let types = p["types"].as_object().unwrap();
        assert_eq!(types.len(), 14, "7 类生活 + 7 类交通");
        assert_eq!(types["residential"]["zh"], json!("住宅"));
        assert_eq!(types["residential"]["color"], json!([196, 186, 166]));
        assert_eq!(types["residential"]["w"], json!(1));
        assert_eq!(types["education"]["w"], json!(2));
        assert_eq!(types["airport"]["h"], json!(2));
        assert_eq!(types["pier"]["group_zh"], json!("交通设施"));
        assert_eq!(types["bus_stop"]["prefix"], json!("B"));
        // 键顺序 = 原型 ALL_TYPES（生活 7 类在前，交通 7 类在后）
        let keys: Vec<&String> = types.keys().collect();
        assert_eq!(keys[0], "residential");
        assert_eq!(keys[6], "lodging");
        assert_eq!(keys[7], "bus_stop");
        assert_eq!(keys[13], "pier");
        assert_eq!(p["levels"]["community"], json!("小区"));
        assert_eq!(p["levels"]["district"], json!("区县"));
        assert_eq!(p["levels"]["city"], json!("城市"));
        assert_eq!(p["level_plan"]["community"]["bus_stop"], json!(2));
        assert_eq!(p["level_plan"]["district"]["train_station"], json!(1));
        assert_eq!(p["level_plan"]["district"]["airport"], json!(null), "区县级没有机场");
        assert_eq!(p["level_plan"]["city"]["airport"], json!(1));
        assert_eq!(p["cell_meters"], json!(30.0));
    }

    /// 类型表的**内部**参数（`types_payload()` 不暴露的那些）逐个对照 Python `facilities.py`。
    /// 这类字段写错不会编译报错，只会让落点悄悄跑偏——「train_station 多写了 hard_road」
    /// 就是靠与 Python 逐字段对拍抓出来的（空 layout 的区县级会一个火车站都放不出来）。
    #[test]
    fn all_types_internal_params_match_python() {
        // (key, weight, foot, road_band, cluster_r, min_gap, w_road, w_cluster, w_green, w_center, jitter, hard_road, need_water)
        #[allow(clippy::type_complexity)]
        let want: [(&str, f64, (i64, i64), (f64, f64), i64, i64, f64, f64, f64, f64, f64, bool, bool); 14] = [
            ("residential", 0.30, (1, 1), (1.0, 3.0), 2, 0, 2.0, 6.0, 0.5, 0.5, 1.5, false, false),
            ("commercial", 0.20, (1, 1), (1.0, 1.0), 0, 0, 8.0, 0.0, 0.0, 1.0, 1.5, false, false),
            ("education", 0.10, (2, 1), (1.0, 2.0), 3, 0, 3.0, 7.0, 1.5, 0.0, 1.0, false, false),
            ("medical", 0.08, (2, 1), (1.0, 1.0), 4, 0, 6.0, 2.0, 0.0, 1.5, 1.0, false, false),
            ("leisure", 0.15, (1, 2), (1.0, 3.0), 3, 0, 1.0, 3.0, 7.0, 0.0, 1.2, false, false),
            ("civic", 0.09, (1, 1), (1.0, 2.0), 0, 0, 2.0, 0.0, 0.0, 6.0, 1.0, false, false),
            ("lodging", 0.08, (1, 1), (1.0, 1.0), 0, 0, 7.0, 0.0, 0.0, 0.5, 1.2, false, false),
            ("bus_stop", 0.0, (1, 1), (1.0, 1.0), 0, 10, 12.0, 0.0, 0.0, 0.5, 1.5, true, false),
            ("subway", 0.0, (1, 1), (1.0, 2.0), 0, 33, 6.0, 0.0, 0.0, 5.0, 1.0, true, false),
            ("parking", 0.0, (2, 1), (1.0, 1.0), 0, 4, 6.0, 0.0, 0.0, 0.0, 1.5, true, false),
            ("gas", 0.0, (2, 1), (1.0, 1.0), 0, 26, 8.0, 0.0, 0.0, -6.0, 1.0, true, false),
            ("airport", 0.0, (3, 2), (1.0, 8.0), 0, 60, 2.0, 0.0, 0.0, -8.0, 0.5, false, false),
            // 原型 train_station 没有 hard_road：火车站不强制临路，只是分数上偏好临路
            ("train_station", 0.0, (3, 2), (1.0, 2.0), 0, 40, 6.0, 0.0, 0.0, 2.0, 0.8, false, false),
            ("pier", 0.0, (1, 1), (1.0, 6.0), 0, 20, 3.0, 0.0, 2.0, 0.0, 1.0, false, true),
        ];
        assert_eq!(want.len(), ALL_TYPES.len(), "表项数必须是 14");
        for (m, w) in ALL_TYPES.iter().zip(want.iter()) {
            let at = |ok: bool, what: &str| assert!(ok, "{} 的 {what} 与原型不一致", m.key);
            at(m.key == w.0, "key");
            at((m.weight - w.1).abs() < 1e-12, "weight");
            at(m.foot == w.2, "foot");
            at(m.road_band == w.3, "road_band");
            at(m.cluster_r == w.4, "cluster_r");
            at(m.min_gap == w.5, "min_gap");
            at((m.w_road - w.6).abs() < 1e-12, "w_road");
            at((m.w_cluster - w.7).abs() < 1e-12, "w_cluster");
            at((m.w_green - w.8).abs() < 1e-12, "w_green");
            at((m.w_center - w.9).abs() < 1e-12, "w_center");
            at((m.jitter - w.10).abs() < 1e-12, "jitter");
            at(m.hard_road == w.11, "hard_road");
            at(m.need_water == w.12, "need_water");
        }
        // 名称池逐字对照（id/名字是前端直接展示的东西，抄错一个字就露馅）
        let name_pools: [(&str, &[&str]); 7] = [
            ("residential", &["阳光苑", "临江苑", "安宁里", "梧桐公寓", "青竹小区", "望江新邨"]),
            ("commercial", &["便利店", "生鲜超市", "购物中心", "沿街商铺", "百货商场"]),
            ("education", &["实验小学", "第三中学", "育才幼儿园", "社区学院"]),
            ("medical", &["社区卫生服务中心", "人民医院", "便民诊所", "大药房"]),
            ("leisure", &["社区公园", "篮球场", "健身房", "电影院", "茶馆"]),
            ("civic", &["社区服务中心", "派出所", "邮政支局", "消防站", "图书馆"]),
            ("lodging", &["快捷酒店", "街角民宿", "青年旅舍", "商务宾馆"]),
        ];
        for (k, pool) in name_pools {
            assert_eq!(life_meta(k).unwrap().names, pool, "{k} 名称池不一致");
        }
        assert_eq!(transport_meta("bus_stop").unwrap().names.len(), 6);
        assert_eq!(transport_meta("airport").unwrap().names, &["国际机场", "通用机场"]);
        assert_eq!(transport_meta("pier").unwrap().names, &["客运码头", "渔人码头", "轮渡码头"]);
    }

    #[test]
    fn round_n_follows_python_bankers_rounding() {
        // Python: round(0.5)=0, round(1.5)=2, round(2.5)=2, round(3.5)=4, round(-2.5)=-2
        assert_eq!(round_n(0.5, 0), 0.0);
        assert_eq!(round_n(1.5, 0), 2.0);
        assert_eq!(round_n(2.5, 0), 2.0);
        assert_eq!(round_n(3.5, 0), 4.0);
        assert_eq!(round_n(-2.5, 0), -2.0);
        // Python: round(2.675, 2) = 2.67（不是 2.68）
        assert_eq!(round_n(2.675, 2), 2.67);
        assert_eq!(round_n(1.005, 2), 1.0);
        assert_eq!(round_n(1.4142135, 2), 1.41);
        // 特值不 panic
        assert!(round_n(f64::NAN, 2).is_nan());
        assert_eq!(round_n(f64::INFINITY, 2), f64::INFINITY);
    }

    #[test]
    fn py_sum_is_neumaier_compensated() {
        assert_eq!(py_sum([1.3, 32.9, 1.3]), 35.5);
        assert_eq!(py_sum(Vec::<f64>::new()), 0.0);
        assert_eq!(py_sum([1.0, 2.0, 3.0]), 6.0);
        // 生活设施权重之和（原型里 sum(weights.values()) 就是它）
        let w = py_sum(life_types().iter().map(|m| m.weight));
        assert!((w - 1.0).abs() < 1e-12, "7 类权重之和 ≈ 1，实测 {w}");
    }

    #[test]
    fn ids_and_names_are_unique_and_sequential() {
        let lay = demo_layout();
        let pack = generate_all(&lay, "广州·越秀", Some(26), "city", None);
        let mut all = facs(&pack["facilities"]);
        all.extend(facs(&pack["transport"]));
        let mut ids: HashSet<String> = HashSet::new();
        let mut names: HashSet<String> = HashSet::new();
        for f in &all {
            assert!(ids.insert(str_field(f, "id", "").to_string()), "id 重复");
            assert!(names.insert(str_field(f, "name", "").to_string()), "name 重复");
        }
        // 同类型内序号从 001 连续
        for m in ALL_TYPES.iter() {
            let mut seqs: Vec<i64> = all
                .iter()
                .filter(|f| str_field(f, "type", "") == m.key)
                .map(|f| {
                    str_field(f, "id", "")
                        .trim_start_matches(m.prefix)
                        .parse::<i64>()
                        .unwrap_or(-1)
                })
                .collect();
            seqs.sort_unstable();
            assert_eq!(
                seqs,
                (1..=seqs.len() as i64).collect::<Vec<_>>(),
                "{} 的序号应从 1 连续",
                m.key
            );
        }
        // 结果按 (type, id) 排序（生活、交通两张表各自有序）
        let mut sorted_life = facs(&pack["facilities"]);
        sort_by_type_id(&mut sorted_life);
        assert_eq!(ser(&Value::Array(sorted_life)), ser(&pack["facilities"]));
        let mut sorted_tr = facs(&pack["transport"]);
        sort_by_type_id(&mut sorted_tr);
        assert_eq!(ser(&Value::Array(sorted_tr)), ser(&pack["transport"]));
    }

    #[test]
    fn alloc_splits_budget_by_weight() {
        let weights: Vec<(String, f64)> = life_types()
            .iter()
            .map(|m| (m.key.to_string(), m.weight))
            .collect();
        let b = alloc(26, &weights);
        let get = |k: &str| b.iter().find(|(n, _)| n == k).map(|x| x.1).unwrap_or(-1);
        assert_eq!(get("residential"), 8);
        assert_eq!(get("commercial"), 5);
        assert_eq!(get("education"), 3);
        assert_eq!(get("medical"), 2);
        assert_eq!(get("leisure"), 4);
        assert_eq!(get("civic"), 2);
        assert_eq!(get("lodging"), 2);
        assert_eq!(b.iter().map(|x| x.1).sum::<i64>(), 26, "名额必须刚好分完");
        // 顺序与入参一致（原型 dict 顺序）
        assert_eq!(b[0].0, "residential");
        assert_eq!(b[6].0, "lodging");
        // count=0 → 全 0；权重全 0 → total 兜底 1.0，不除零
        assert!(alloc(0, &weights).iter().all(|x| x.1 == 0));
        let zero: Vec<(String, f64)> = weights.iter().map(|(k, _)| (k.clone(), 0.0)).collect();
        assert_eq!(alloc(7, &zero).iter().map(|x| x.1).sum::<i64>(), 7);
    }

    #[test]
    fn facilities_svg_and_draw_stub() {
        let lay = demo_layout();
        let pack = generate_all(&lay, "广州·越秀", Some(26), "city", None);
        let mut both = facs(&pack["facilities"]);
        both.extend(facs(&pack["transport"]));
        let svg = facilities_svg(&lay, &Value::Array(both.clone()), 900.0, 900.0);
        assert!(svg.starts_with("<svg"), "得是 SVG");
        assert!(svg.ends_with("</svg>"));
        assert_eq!(svg.matches("<rect").count(), 1 + both.len(), "底色 + 每个设施一个色块");
        // PIL 出图未移植：回说明对象而不是 panic
        let d = draw_facilities(&lay, &Value::Array(both), "/tmp/x.png", "gaode", 900.0, 900.0);
        assert_eq!(d["ok"], json!(false));
        assert_eq!(d["path"], json!("/tmp/x.png"));
        assert!(d["hint"].as_str().unwrap().contains("facilities_svg"));
    }

    /// 结构对拍用：`cargo test -- --nocapture structural_report_vs_python`
    /// （与 Python 原型 `python3 facilities.py` 的输出逐项对照，坐标不要求相同）
    #[test]
    fn structural_report_vs_python() {
        let lay = demo_layout();
        let pack = generate_all(&lay, "广州·越秀", Some(26), "city", None);
        let life = facs(&pack["facilities"]);
        let mut both = life.clone();
        both.extend(facs(&pack["transport"]));
        let ts = types_payload();
        println!("== 结构对拍（Rust 侧）layout: {} size={} ==", lay["name"], pack["size"]);
        println!("[1] 生活设施 {} 个", life.len());
        for m in life_types() {
            let got: Vec<String> = life
                .iter()
                .filter(|f| str_field(f, "type", "") == m.key)
                .map(|f| {
                    format!(
                        "{}@{},{} {}",
                        str_field(f, "id", ""),
                        cell_of(f).0,
                        cell_of(f).1,
                        str_field(f, "name", "")
                    )
                })
                .collect();
            if !got.is_empty() {
                println!("  {} {:<5}({:<11}) {}: {}", m.icon, m.zh, m.key, got.len(), got.join(", "));
            }
        }
        println!("  不重叠校验: {}", if overlap_check(&lay, &life).is_empty() { "OK 通过" } else { "FAIL" });
        println!("[2] 交通设施");
        for lvl in ["community", "district", "city"] {
            let tr = facs(&generate_transport_nodes(&lay, "广州·越秀", lvl, Some(&Value::Array(life.clone())), None));
            let mut desc: Vec<String> = Vec::new();
            for m in transport_types() {
                let n = count_type(&tr, m.key);
                if n > 0 {
                    desc.push(format!("{}{}×{}", m.icon, m.zh, n));
                }
            }
            println!("  {} ({:<9}) {} 个: {}", level_zh(lvl), lvl, tr.len(), desc.join(" | "));
            let mut b2 = tr.clone();
            b2.extend(life.clone());
            println!("      不重叠校验: {}", if overlap_check(&lay, &b2).is_empty() { "OK" } else { "FAIL" });
        }
        let st = build_state(&lay);
        let cit = facs(&generate_transport_nodes(&lay, "广州·越秀", "city", Some(&Value::Array(life.clone())), None));
        let piers: Vec<&Value> = cit.iter().filter(|f| str_field(f, "type", "") == "pier").collect();
        println!(
            "[3] 规则校验: 码头临水 {} ({}) | 公交站临路 {} ({})",
            if piers.iter().all(|f| st.at(&st.water_dist, cell_of(f).0, cell_of(f).1) <= 1) { "OK" } else { "FAIL" },
            piers.len(),
            if cit.iter().filter(|f| str_field(f, "type", "") == "bus_stop").all(|f| st.at(&st.road_dist, cell_of(f).0, cell_of(f).1) <= 1) { "OK" } else { "FAIL" },
            count_type(&cit, "bus_stop")
        );
        let cx: Vec<i64> = both.iter().map(|f| cell_of(f).0).collect();
        let cy: Vec<i64> = both.iter().map(|f| cell_of(f).1).collect();
        println!(
            "[4] 坐标范围 x∈[{},{}] y∈[{},{}] | id 前缀 {} | 元数据表类型数 {} | stats.total {}",
            cx.iter().min().unwrap_or(&-1),
            cx.iter().max().unwrap_or(&-1),
            cy.iter().min().unwrap_or(&-1),
            cy.iter().max().unwrap_or(&-1),
            {
                let mut ps: Vec<String> = both.iter().map(|f| str_field(f, "id", "").chars().next().unwrap_or('?').to_string()).collect();
                ps.sort();
                ps.dedup();
                ps.join("")
            },
            ts["types"].as_object().unwrap().len(),
            pack["stats"]["total"]
        );
        println!("[5] generate_all: 生活 {} + 交通 {} = {}", life.len(), both.len() - life.len(), both.len());
        assert_eq!(cx.iter().filter(|&&x| x < 0 || x >= 24).count(), 0, "所有坐标都在网格内");
    }
}
