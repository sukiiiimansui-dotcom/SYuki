//! 城市级**真拼接**大图（T5-1）：把一个市下辖各区县的**街区图**按地理网格拼成一张大 SVG。
//!
//! 与 `/api/bigmap`（行政区划总览：把区县轮廓画成一张图）的区别：
//!   · bigmap      —— 看得见「整个市的范围」，看不见街区细节
//!   · bigmap_stitch —— 每块是**该区县自己的一张街区图**（sketch + render），按经纬度排布拼起来
//! 两者是「总览」与「拼接」两件事，不互相替代。
//!
//! ## 怎么排（算法）
//! 1. `properties.center`（DataV 给的标注点，缺则用几何 bbox 中心）→ Web Mercator 世界坐标
//!    （`coord::lng_lat_to_world`），再用 **既有** 的 `coord::world_to_grid(x,y,anchor,cell)`
//!    换算成「以市质心为原点、以 cell 米为格距」的网格坐标 —— 不另造一套投影。
//! 2. 一块 = 一个格子：`col = round(gx)`、`row = round(gy)`（row 向南增大 = 屏幕向下）。
//!    撞格时按「离理想位置最近的空格」螺旋外扩；再做一轮**成对交换**局部搜索，
//!    让每块离自己的理想格最近（相对方位因此不会被挤乱）。
//! 3. 格距 `cell_m` 不是拍脑袋来的：以 `sqrt(市跨度x × 市跨度y / 块数)` 为基准，
//!    在 8 个倍率里挑一个 —— **先看方位错了几对**（只统计真实距离 ≥ 一格的那些对，
//!    比一格还近的对本来就不是这个分辨率能表达的），再挑填充率最高（格子最少、块最大）的。
//! 4. 像素几何由**大图上限反推**：`tile_px = min(可用宽/列数, 可用高/行数)`，
//!    于是画布一定落在 `width × height` 上限内；每块的街区网格数再由 `tile_px` 反推
//!    （`tile_px / 22px`，夹在 12..40），块越大街道越细。
//!
//! ## 规模与内存（手机红线）
//!   · `max_tiles` 默认 9（3×3），硬上限 36；超出的按**重要性分页**（不是丢掉）：
//!     重要性 = 面积排名 + 离市中心距离排名（名次和越小越重要），页码取切片。
//!   · 街道细节（人行道/斑马线/树/车位/路灯）**默认不挂**：全挂会让大图体积翻几倍。
//!     要细节就 `detail=true`，或者只给某几块开高精度（`hi=440103,440104`）——
//!     这正是「先出低精度整张，再按需替换某一块」的两遍法。
//!   · 拼完若超过 `max_bytes`（默认 4MB）自动降级重拼一次（关细节 + 网格降到 ≤16）。
//!   · 产物走 `maplib` 既有入库逻辑：每块 SVG 与整张 SVG 都写文件 + `MapLib::register`，
//!     键由参数哈希得到 → 第二遍只有被替换的那块需要重画。
//!
//! 本模块**不依赖 tauri**（可脱离工程 `rustc --test` 跑单测）：命令层在 `stitch_cmd.rs`，
//! HTTP 版在独立调试工程 `world_map_rs` 的 `/api/bigmap_stitch`。
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::world_map::coord;
use crate::world_map::details;
use crate::world_map::maplib::MapLib;
use crate::world_map::osm;
use crate::world_map::render;
use crate::world_map::sketch;

// ── 版本与默认值（改了缓存键的语义就要动 STITCH_VERSION，否则老缓存会被误用）──
pub const STITCH_VERSION: u32 = 1;
/// 默认最多几块（3×3）
pub const DEFAULT_MAX_TILES: usize = 9;
/// 硬上限：6×6。再多就该分页而不是硬拼（手机内存）
pub const MAX_TILES_CAP: usize = 36;
pub const DEFAULT_CANVAS: f64 = 2000.0;
pub const MIN_CANVAS: f64 = 600.0;
pub const MAX_CANVAS: f64 = 2400.0;
pub const MARGIN: f64 = 26.0;
pub const GAP: f64 = 14.0;
pub const HEADER_H: f64 = 26.0;
pub const TITLE_H: f64 = 38.0;
pub const MIN_TILE_PX: f64 = 150.0;
pub const MAX_TILE_PX: f64 = 820.0;
/// 每块街区图最少/最多多少格（格数决定街道密度与 SVG 元素量）
pub const GRID_MIN: i32 = 12;
pub const GRID_MAX: i32 = 40;
/// 高精度块的上限（`hi` 用）
pub const GRID_MAX_HI: i32 = 48;
/// 街区块的目标像素密度：多少像素一格
pub const GRID_PX_PER_CELL: f64 = 22.0;
/// 拼完体积上限（超出自动降级重拼）
pub const DEFAULT_MAX_BYTES: usize = 4 * 1024 * 1024;
/// 格距候选倍率（相对 `sqrt(跨度x×跨度y/块数)`）
const CELL_MULTS: [f64; 8] = [0.5, 0.6, 0.7, 0.85, 1.0, 1.2, 1.4, 1.7];
const MIN_CELL_M: f64 = 1200.0;
/// 撞格时最多向外找几圈
const MAX_SPIRAL: i32 = 24;
/// 局部搜索（成对交换）最多迭代几轮
const MAX_SWAP_PASSES: usize = 40;

// ═══════════════════════════════════════════════════════════════════
//  数据模型
// ═══════════════════════════════════════════════════════════════════

/// 一个下辖区县（几何已算好，纯数据 → 可测）
#[derive(Debug, Clone, PartialEq)]
pub struct District {
    pub adcode: String,
    pub name: String,
    /// 标注点（DataV `properties.center`，缺则 bbox 中心）
    pub lng: f64,
    pub lat: f64,
    /// bbox 长边（米）—— 只用于展示与排序说明，不参与格距
    pub span_m: f64,
    /// bbox 面积（km²，近似值；用于重要性排序）
    pub area_km2: f64,
}

impl District {
    pub fn span_km(&self) -> f64 {
        (self.span_m / 100.0).round() / 10.0
    }
    pub fn area_km2_rounded(&self) -> f64 {
        self.area_km2.round()
    }
}

fn walk_coords(c: &Value, out: &mut Vec<(f64, f64)>) {
    if let Some(arr) = c.as_array() {
        if arr.len() >= 2 && arr[0].is_number() && arr[1].is_number() {
            out.push((arr[0].as_f64().unwrap_or(0.0), arr[1].as_f64().unwrap_or(0.0)));
            return;
        }
        for x in arr {
            walk_coords(x, out);
        }
    }
}

fn num_pair(v: Option<&Value>) -> Option<(f64, f64)> {
    let a = v?.as_array()?;
    if a.len() < 2 {
        return None;
    }
    Some((a[0].as_f64()?, a[1].as_f64()?))
}

/// 从一个市的 FeatureCollection 里取「下辖区县」列表。
///
/// 为什么不用 `geo::features`：这里要 `properties.center`（DataV 的标注点，
/// 比 bbox 中心更贴近"这块地方的表示点"），而 `Feature` 只留了几何。
pub fn districts_of(fc: &Value) -> Vec<District> {
    let mut out = Vec::new();
    let list = match fc.get("features").and_then(|v| v.as_array()) {
        Some(l) => l,
        None => return out,
    };
    for f in list {
        let p = match f.get("properties") {
            Some(p) => p,
            None => continue,
        };
        let name = p
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let adcode = match p.get("adcode") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        let mut pts = Vec::new();
        if let Some(c) = f.get("geometry").and_then(|g| g.get("coordinates")) {
            walk_coords(c, &mut pts);
        }
        if pts.is_empty() {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &(lng, lat) in &pts {
            if lng < x0 { x0 = lng; }
            if lat < y0 { y0 = lat; }
            if lng > x1 { x1 = lng; }
            if lat > y1 { y1 = lat; }
        }
        let (clng, clat) = num_pair(p.get("center"))
            .or_else(|| num_pair(p.get("centroid")))
            .unwrap_or(((x0 + x1) / 2.0, (y0 + y1) / 2.0));
        let mlng = coord::meters_per_deg_lng(clat);
        let w = (x1 - x0) * mlng;
        let h = (y1 - y0) * coord::METERS_PER_DEG_LAT;
        out.push(District {
            adcode,
            name,
            lng: clng,
            lat: clat,
            span_m: w.max(h),
            area_km2: w * h / 1e6,
        });
    }
    out
}

/// 读一个市的地理缓存：先 `{ad}.json`，再 `{ad}_full.json`。
///
/// 为什么不是 `GeoSource::load_cached`：它只看 `{ad}.json`，而本地缓存在**市这一级**
/// 常常只有 `_full`（例如 440300 深圳只有 `440300_full.json`）→ 会白白退化成联网。
/// 拼接是"看一眼就出图"的功能，缓存能命中就别等网络。
pub fn load_city_fc(cache: &Path, ad: &str) -> Result<(Value, PathBuf), String> {
    let a = ad.trim();
    if a.is_empty() {
        return Err("ad 不能为空".into());
    }
    for name in [format!("{a}.json"), format!("{a}_full.json")] {
        let p = cache.join(&name);
        if let Ok(txt) = std::fs::read_to_string(&p) {
            if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                let ok = v
                    .get("features")
                    .and_then(|f| f.as_array())
                    .map(|a| !a.is_empty())
                    .unwrap_or(false);
                if ok {
                    return Ok((v, p));
                }
            }
        }
    }
    Err(format!("没有 {a} 的本地地理缓存（找过 {a}.json 与 {a}_full.json）"))
}

/// 市名（拼接大图的标题用）：从上级（省）的缓存里找 adcode 对应的 name。
/// 找不到就返回 None，调用方退回用 adcode 当标题 —— 绝不为了个名字去联网。
pub fn city_name_of(cache: &Path, ad: &str) -> Option<String> {
    let a = ad.trim();
    if a.len() != 6 || !a.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let parent = format!("{}0000", &a[..2]);
    if parent == a {
        return None;
    }
    let (fc, _) = load_city_fc(cache, &parent).ok()?;
    for f in fc.get("features")?.as_array()? {
        let p = f.get("properties")?;
        let code = match p.get("adcode") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => continue,
        };
        if code == a {
            return p
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
    }
    None
}

// ═══════════════════════════════════════════════════════════════════
//  选项
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct Opts {
    pub style: String,
    pub max_tiles: usize,
    pub page: usize,
    pub width: f64,
    pub height: f64,
    /// 是否挂街道细节（人行道/斑马线/树/车位/路灯）—— 默认关，大图会爆
    pub detail: bool,
    /// 要出高精度的块（adcode 列表）：这些块网格 ×1.5 且强制挂细节
    pub hi: Vec<String>,
    /// 强制统一网格数（不传 = 由 tile_px 反推）
    pub grid: Option<i32>,
    pub seed: Option<u64>,
    pub zoom: i32,
    pub max_bytes: usize,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            style: "gaode".into(),
            max_tiles: DEFAULT_MAX_TILES,
            page: 1,
            width: DEFAULT_CANVAS,
            height: DEFAULT_CANVAS,
            detail: false,
            hi: Vec::new(),
            grid: None,
            seed: None,
            zoom: 3,
            max_bytes: DEFAULT_MAX_BYTES,
        }
    }
}

impl Opts {
    /// 夹紧到合法区间（命令层/HTTP 层都从这里过一道，越界参数不会变成奇怪的画布）
    pub fn normalized(&self) -> Opts {
        let mut o = self.clone();
        o.max_tiles = o.max_tiles.clamp(1, MAX_TILES_CAP);
        o.page = o.page.max(1);
        o.width = clamp_canvas(o.width);
        o.height = clamp_canvas(o.height);
        o.zoom = o.zoom.clamp(1, 3);
        o.grid = o.grid.map(|g| g.clamp(GRID_MIN, GRID_MAX_HI));
        if o.style.trim().is_empty() || !render::STYLE_NAMES.contains(&o.style.as_str()) {
            o.style = "gaode".into();
        }
        o.hi = o
            .hi
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if o.max_bytes == 0 {
            o.max_bytes = DEFAULT_MAX_BYTES;
        }
        o
    }
}

fn clamp_canvas(v: f64) -> f64 {
    if !v.is_finite() || v <= 0.0 {
        return DEFAULT_CANVAS;
    }
    v.clamp(MIN_CANVAS, MAX_CANVAS)
}

// ═══════════════════════════════════════════════════════════════════
//  ① 选块（重要性 + 分页）
// ═══════════════════════════════════════════════════════════════════

/// 一次选择的产物
#[derive(Debug, Clone)]
pub struct Selection {
    pub picked: Vec<District>,
    pub dropped: Vec<District>,
    pub page: usize,
    pub pages: usize,
    pub total: usize,
}

fn centroid(ds: &[District]) -> (f64, f64) {
    let n = ds.len().max(1) as f64;
    let lng = ds.iter().map(|d| d.lng).sum::<f64>() / n;
    let lat = ds.iter().map(|d| d.lat).sum::<f64>() / n;
    (lng, lat)
}

fn center_distance_m(d: &District, c: (f64, f64)) -> f64 {
    coord::haversine_m((d.lng, d.lat), c)
}

/// 重要性 = 面积名次 + 离市中心距离名次（名次和越小越重要，名次从 1 开始）。
///
/// 为什么两条一起看：只看面积会把市中心的越秀/荔湾全挤掉（它们小），
/// 只看中心距又会让从化/增城这种占了大半个市的大块消失。名次和是个便宜的折中。
pub fn importance_order(ds: &[District]) -> Vec<usize> {
    let n = ds.len();
    let c = centroid(ds);
    let mut by_area: Vec<usize> = (0..n).collect();
    by_area.sort_by(|&i, &j| {
        ds[j].area_km2
            .partial_cmp(&ds[i].area_km2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(ds[i].adcode.cmp(&ds[j].adcode))
    });
    let mut by_center: Vec<usize> = (0..n).collect();
    by_center.sort_by(|&i, &j| {
        center_distance_m(&ds[i], c)
            .partial_cmp(&center_distance_m(&ds[j], c))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(ds[i].adcode.cmp(&ds[j].adcode))
    });
    let mut rank_area = vec![0usize; n];
    let mut rank_center = vec![0usize; n];
    for (r, &i) in by_area.iter().enumerate() {
        rank_area[i] = r + 1;
    }
    for (r, &i) in by_center.iter().enumerate() {
        rank_center[i] = r + 1;
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| {
        (rank_area[i] + rank_center[i])
            .cmp(&(rank_area[j] + rank_center[j]))
            // 名次和打平时市中心的优先（同等重要时，它更该出现在拼接图上）
            .then(
                center_distance_m(&ds[i], c)
                    .partial_cmp(&center_distance_m(&ds[j], c))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(ds[i].adcode.cmp(&ds[j].adcode))
    });
    order
}

/// 分页选块：`max_tiles` 装满一页，超出的**进下一页**（不是丢弃）。
pub fn select_page(ds: &[District], max_tiles: usize, page: usize) -> Selection {
    let n = ds.len();
    let per = max_tiles.clamp(1, MAX_TILES_CAP);
    let pages = n.div_ceil(per).max(1);
    let page = page.clamp(1, pages);
    let order = importance_order(ds);
    let start = (page - 1) * per;
    let end = (start + per).min(n);
    let mut picked = Vec::new();
    let mut dropped = Vec::new();
    for (pos, &i) in order.iter().enumerate() {
        if pos >= start && pos < end {
            picked.push(ds[i].clone());
        } else {
            dropped.push(ds[i].clone());
        }
    }
    Selection {
        picked,
        dropped,
        page,
        pages,
        total: n,
    }
}

// ═══════════════════════════════════════════════════════════════════
//  ② 地理网格排布
// ═══════════════════════════════════════════════════════════════════

/// 一次格距试算的结果
#[derive(Debug, Clone, PartialEq)]
pub struct LatticeFit {
    pub cell_m: f64,
    /// 与 `sel` 同序的格子坐标（col, row），row 向南增大
    pub cells: Vec<(i32, i32)>,
    /// 方位错的对数（只统计真实距离 ≥ 一格的"可表达"对）
    pub flips: usize,
    /// 参与统计的对数
    pub pairs: usize,
    /// 填充率 = 块数 / (列数×行数)
    pub fill: f64,
    /// 均方位移（单位：格）
    pub rms: f64,
    pub cols: i32,
    pub rows: i32,
}

fn ideal_cells(sel: &[District], alng: f64, alat: f64, cell_m: f64) -> Vec<(f64, f64)> {
    sel.iter()
        .map(|d| {
            let (wx, wy) = coord::lng_lat_to_world(d.lng, d.lat);
            // 用**既有**投影换算（不要另造一套）：世界坐标 → 以 (alng,alat) 为原点、cell_m 米为格距的网格
            coord::world_to_grid(wx, wy, alng, alat, cell_m)
        })
        .collect()
}

fn base_cell_m(sel: &[District], alat: f64) -> f64 {
    let lngs: Vec<f64> = sel.iter().map(|d| d.lng).collect();
    let lats: Vec<f64> = sel.iter().map(|d| d.lat).collect();
    let (minx, maxx) = (
        lngs.iter().cloned().fold(f64::MAX, f64::min),
        lngs.iter().cloned().fold(f64::MIN, f64::max),
    );
    let (miny, maxy) = (
        lats.iter().cloned().fold(f64::MAX, f64::min),
        lats.iter().cloned().fold(f64::MIN, f64::max),
    );
    let sx = (maxx - minx) * coord::meters_per_deg_lng(alat);
    let sy = (maxy - miny) * coord::METERS_PER_DEG_LAT;
    let n = sel.len().max(1) as f64;
    let base = (sx.max(1.0) * sy.max(1.0) / n).sqrt();
    base.max(MIN_CELL_M)
}

/// 按"面积大的先占好格"的顺序落到网格上，撞格就向外找最近的空格。
fn greedy_cells(sel: &[District], ideal: &[(f64, f64)]) -> Vec<(i32, i32)> {
    let mut order: Vec<usize> = (0..sel.len()).collect();
    order.sort_by(|&i, &j| {
        sel[j]
            .area_km2
            .partial_cmp(&sel[i].area_km2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(sel[i].adcode.cmp(&sel[j].adcode))
    });
    let mut taken: BTreeMap<(i32, i32), usize> = BTreeMap::new();
    let mut out = vec![(0i32, 0i32); sel.len()];
    for (seq, &i) in order.iter().enumerate() {
        let (fx, fy) = ideal[i];
        let (c0, r0) = (fx.round() as i32, fy.round() as i32);
        let mut chosen: Option<(i32, i32)> = None;
        for rad in 0..=MAX_SPIRAL {
            if chosen.is_some() {
                break;
            }
            let mut ring: Vec<(i32, i32)> = Vec::new();
            for dc in -rad..=rad {
                for dr in -rad..=rad {
                    if dc.abs().max(dr.abs()) != rad {
                        continue;
                    }
                    ring.push((c0 + dc, r0 + dr));
                }
            }
            ring.sort_by(|a, b| {
                let ca = (a.0 as f64 - fx).powi(2) + (a.1 as f64 - fy).powi(2);
                let cb = (b.0 as f64 - fx).powi(2) + (b.1 as f64 - fy).powi(2);
                ca.partial_cmp(&cb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.1.cmp(&b.1))
                    .then(a.0.cmp(&b.0))
            });
            for c in ring {
                if !taken.contains_key(&c) {
                    chosen = Some(c);
                    break;
                }
            }
        }
        // 理论上到不了这里（格子无限多）；真到了就往下压，保证不 panic、不重叠
        let c = chosen.unwrap_or((c0, r0 + MAX_SPIRAL + 1 + seq as i32));
        taken.insert(c, i);
        out[i] = c;
    }
    out
}

/// 局部搜索：成对交换格子，只要"两块离各自理想格的总平方距离"变小就换。
/// 纯贪心只保证不撞格，换一轮之后中心那几块才不会被挤到隔壁两格去。
fn refine_cells(cells: &mut [(i32, i32)], ideal: &[(f64, f64)], max_passes: usize) -> usize {
    let cost = |idx: usize, p: (i32, i32)| {
        let (fx, fy) = ideal[idx];
        (p.0 as f64 - fx).powi(2) + (p.1 as f64 - fy).powi(2)
    };
    let mut swaps = 0usize;
    for _ in 0..max_passes {
        let mut improved = 0usize;
        for i in 0..cells.len() {
            for j in (i + 1)..cells.len() {
                let (pi, pj) = (cells[i], cells[j]);
                let before = cost(i, pi) + cost(j, pj);
                let after = cost(i, pj) + cost(j, pi);
                if after < before - 1e-12 {
                    cells[i] = pj;
                    cells[j] = pi;
                    improved += 1;
                    swaps += 1;
                }
            }
        }
        if improved == 0 {
            break;
        }
    }
    swaps
}

/// 一次格距试算：落格 + 精修 + 评分
pub fn fit_lattice(sel: &[District], alng: f64, alat: f64, cell_m: f64) -> LatticeFit {
    let ideal = ideal_cells(sel, alng, alat, cell_m);
    let mut cells = greedy_cells(sel, &ideal);
    refine_cells(&mut cells, &ideal, MAX_SWAP_PASSES);

    let mlng = coord::meters_per_deg_lng(alat);
    let n = sel.len();
    let mut flips = 0usize;
    let mut pairs = 0usize;
    for i in 0..n {
        for j in (i + 1)..n {
            let dx = (sel[j].lng - sel[i].lng) * mlng;
            let dy = (sel[j].lat - sel[i].lat) * coord::METERS_PER_DEG_LAT;
            let dist = (dx * dx + dy * dy).sqrt();
            // 比一格还近的一对，本来就落不进不同的格子 —— 不计入方位统计
            if dist < cell_m * 0.999 {
                continue;
            }
            pairs += 1;
            let ax = (cells[j].0 - cells[i].0) as f64;
            let ay = -((cells[j].1 - cells[i].1) as f64); // 行号向南增大 → 取负才是"北为正"
            if dx * ax + dy * ay < 0.0 {
                flips += 1;
            }
        }
    }
    let (minc, maxc) = cells
        .iter()
        .fold((i32::MAX, i32::MIN), |(lo, hi), c| (lo.min(c.0), hi.max(c.0)));
    let (minr, maxr) = cells
        .iter()
        .fold((i32::MAX, i32::MIN), |(lo, hi), c| (lo.min(c.1), hi.max(c.1)));
    let cols = (maxc - minc + 1).max(1);
    let rows = (maxr - minr + 1).max(1);
    let fill = n as f64 / (cols as f64 * rows as f64);
    let rms = if n == 0 {
        0.0
    } else {
        (cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let (fx, fy) = ideal[i];
                (c.0 as f64 - fx).powi(2) + (c.1 as f64 - fy).powi(2)
            })
            .sum::<f64>()
            / n as f64)
            .sqrt()
    };
    LatticeFit {
        cell_m,
        cells,
        flips,
        pairs,
        fill,
        rms,
        cols,
        rows,
    }
}

/// 在候选格距里挑一个：**先看方位错得少**，再看填充率高（格子少 = 每块更大），
/// 再看位移小，最后取格距小的。全流程确定性 → 同输入永远同一张图。
pub fn choose_lattice(sel: &[District], alng: f64, alat: f64) -> LatticeFit {
    let base = base_cell_m(sel, alat);
    let mut best: Option<LatticeFit> = None;
    for &mult in CELL_MULTS.iter() {
        let fit = fit_lattice(sel, alng, alat, (base * mult).max(MIN_CELL_M));
        let take = match &best {
            None => true,
            Some(b) => is_better(&fit, b),
        };
        if take {
            best = Some(fit);
        }
    }
    best.unwrap_or_else(|| fit_lattice(sel, alng, alat, base))
}

fn is_better(a: &LatticeFit, b: &LatticeFit) -> bool {
    if a.flips != b.flips {
        return a.flips < b.flips;
    }
    if (a.fill - b.fill).abs() > 1e-9 {
        return a.fill > b.fill;
    }
    if (a.rms - b.rms).abs() > 1e-9 {
        return a.rms < b.rms;
    }
    a.cell_m < b.cell_m - 1e-6
}

// ═══════════════════════════════════════════════════════════════════
//  ③ 像素几何（由大图上限反推）
// ═══════════════════════════════════════════════════════════════════

/// 画布尺寸与每块边长：**由上限反推**，保证 `≤ width × height`
pub fn geometry(cols: i32, rows: i32, width: f64, height: f64) -> (f64, f64, f64) {
    let cols = cols.max(1) as f64;
    let rows = rows.max(1) as f64;
    let avail_w = (width - 2.0 * MARGIN - GAP * (cols - 1.0)).max(80.0);
    let avail_h = (height - 2.0 * MARGIN - TITLE_H - HEADER_H * rows - GAP * (rows - 1.0)).max(80.0);
    let tile = (avail_w / cols)
        .min(avail_h / rows)
        .clamp(MIN_TILE_PX, MAX_TILE_PX)
        .floor()
        .max(1.0);
    let w = (2.0 * MARGIN + tile * cols + GAP * (cols - 1.0)).ceil();
    let h = (2.0 * MARGIN + TITLE_H + (tile + HEADER_H) * rows + GAP * (rows - 1.0)).ceil();
    (tile, w, h)
}

/// 每块的街区网格数：**由块边长反推**（块越大街道越细），高精度块再 ×1.5
pub fn grid_for(tile_px: f64, hi: bool, forced: Option<i32>) -> i32 {
    let base = match forced {
        Some(g) => g.clamp(GRID_MIN, GRID_MAX),
        None => ((tile_px / GRID_PX_PER_CELL).round() as i32).clamp(GRID_MIN, GRID_MAX),
    };
    if hi {
        (base * 3 / 2).clamp(GRID_MIN, GRID_MAX_HI)
    } else {
        base
    }
}

// ═══════════════════════════════════════════════════════════════════
//  ④ 计划
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct Tile {
    /// 1 起的编号（按阅读顺序：先上后下、先左后右）
    pub index: usize,
    pub adcode: String,
    pub name: String,
    pub col: i32,
    pub row: i32,
    /// 该块「标题带」左上角在画布里的坐标（街区方块在 y + header 处）
    pub x: f64,
    pub y: f64,
    /// 街区方块边长
    pub size: f64,
    pub pad: f64,
    pub grid: i32,
    pub hi: bool,
    pub lng: f64,
    pub lat: f64,
    pub span_km: f64,
    pub area_km2: f64,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub ad: String,
    pub style: String,
    pub page: usize,
    pub pages: usize,
    pub total: usize,
    pub max_tiles: usize,
    pub cols: i32,
    pub rows: i32,
    pub tile_px: f64,
    pub width: f64,
    pub height: f64,
    pub cell_m: f64,
    pub flips: usize,
    pub pairs: usize,
    pub fill: f64,
    pub detail: bool,
    pub zoom: i32,
    pub tiles: Vec<Tile>,
    pub dropped: Vec<District>,
}

impl Plan {
    pub fn tile_of(&self, ad: &str) -> Option<&Tile> {
        self.tiles.iter().find(|t| t.adcode == ad)
    }
    pub fn to_json(&self) -> Value {
        json!({
            "ad": self.ad,
            "style": self.style,
            "page": self.page,
            "pages": self.pages,
            "total": self.total,
            "maxTiles": self.max_tiles,
            "cols": self.cols,
            "rows": self.rows,
            "tilePx": self.tile_px,
            "canvas": [self.width, self.height],
            "cellKm": (self.cell_m / 100.0).round() / 10.0,
            "dirFlips": self.flips,
            "dirPairs": self.pairs,
            "fill": (self.fill * 100.0).round() / 100.0,
            "detail": self.detail,
            "zoom": self.zoom,
            "tiles": self.tiles.iter().map(|t| json!({
                "index": t.index, "adcode": t.adcode, "name": t.name,
                "col": t.col, "row": t.row, "x": t.x, "y": t.y,
                "size": t.size, "grid": t.grid, "hi": t.hi,
                "spanKm": t.span_km, "areaKm2": t.area_km2,
                "center": [t.lng, t.lat],
            })).collect::<Vec<_>>(),
            "dropped": self.dropped.iter().map(|d| json!({
                "adcode": d.adcode, "name": d.name,
                "areaKm2": d.area_km2_rounded(), "spanKm": d.span_km(),
            })).collect::<Vec<_>>(),
        })
    }
}

/// 出计划：选块 → 定格距 → 定像素 → 编号
pub fn plan(ds: &[District], ad: &str, opts: &Opts) -> Plan {
    let o = opts.normalized();
    let sel = select_page(ds, o.max_tiles, o.page);
    if sel.picked.is_empty() {
        return Plan {
            ad: ad.to_string(),
            style: o.style.clone(),
            page: sel.page,
            pages: sel.pages,
            total: sel.total,
            max_tiles: o.max_tiles,
            cols: 0,
            rows: 0,
            tile_px: 0.0,
            width: 0.0,
            height: 0.0,
            cell_m: 0.0,
            flips: 0,
            pairs: 0,
            fill: 0.0,
            detail: o.detail,
            zoom: o.zoom,
            tiles: Vec::new(),
            dropped: sel.dropped.clone(),
        };
    }
    let (alng, alat) = centroid(&sel.picked);
    let fit = choose_lattice(&sel.picked, alng, alat);
    let (tile, w, h) = geometry(fit.cols, fit.rows, o.width, o.height);

    let minc = fit.cells.iter().map(|c| c.0).min().unwrap_or(0);
    let minr = fit.cells.iter().map(|c| c.1).min().unwrap_or(0);
    let mut order: Vec<usize> = (0..sel.picked.len()).collect();
    // 编号 = 阅读顺序（行优先）；同一行再按列
    order.sort_by_key(|&i| (fit.cells[i].1, fit.cells[i].0, sel.picked[i].adcode.clone()));

    let pad = (tile * 0.05).round().max(8.0);
    let mut tiles = Vec::new();
    for (k, &i) in order.iter().enumerate() {
        let d = &sel.picked[i];
        let (c, r) = fit.cells[i];
        let (col, row) = (c - minc, r - minr);
        let hi = o.hi.iter().any(|a| a == &d.adcode);
        tiles.push(Tile {
            index: k + 1,
            adcode: d.adcode.clone(),
            name: d.name.clone(),
            col,
            row,
            x: MARGIN + col as f64 * (tile + GAP),
            y: MARGIN + TITLE_H + row as f64 * (tile + HEADER_H + GAP),
            size: tile,
            pad,
            grid: grid_for(tile, hi, o.grid),
            hi,
            lng: d.lng,
            lat: d.lat,
            span_km: d.span_km(),
            area_km2: d.area_km2_rounded(),
        });
    }
    Plan {
        ad: ad.to_string(),
        style: o.style.clone(),
        page: sel.page,
        pages: sel.pages,
        total: sel.total,
        max_tiles: o.max_tiles,
        cols: fit.cols,
        rows: fit.rows,
        tile_px: tile,
        width: w,
        height: h,
        cell_m: fit.cell_m,
        flips: fit.flips,
        pairs: fit.pairs,
        fill: fit.fill,
        detail: o.detail,
        zoom: o.zoom,
        tiles,
        dropped: sel.dropped.clone(),
    }
}

// ═══════════════════════════════════════════════════════════════════
//  ⑤ 拼装 SVG
// ═══════════════════════════════════════════════════════════════════

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 取出渲染器输出的**内部内容**（丢掉最外层 `<svg …>` / `</svg>`）。
/// 拿不到就返回 None（调用方退回原样嵌 `<svg>`，绝不 panic）。
pub fn body_of(svg: &str) -> Option<String> {
    let s = svg.trim_start();
    if !s.starts_with("<svg") {
        return None;
    }
    let open_end = s.find('>')?;
    let close = s.rfind("</svg>")?;
    if close <= open_end {
        return None;
    }
    Some(s[open_end + 1..close].to_string())
}

/// 把 `<style>…</style>` 从块内容里摘出来（大图里只保留一份 CSS，省体积）
pub fn take_style(body: &str) -> (String, Option<String>) {
    if let Some(start) = body.find("<style>") {
        if let Some(rel_end) = body[start..].find("</style>") {
            let end = start + rel_end + "</style>".len();
            let css = body[start + "<style>".len()..start + rel_end].to_string();
            let mut rest = String::with_capacity(body.len());
            rest.push_str(&body[..start]);
            rest.push_str(&body[end..]);
            return (rest, Some(css));
        }
    }
    (body.to_string(), None)
}

/// 拼装：外框 + 标题带（编号/区县名/面积）+ 每块的街区图（顶部标题条被裁掉）。
///
/// `bodies` 按 adcode 给每块的内部 SVG 内容（`render::render_svg` 的输出过一遍 [`body_of`]）。
pub fn compose_svg(plan: &Plan, bodies: &BTreeMap<String, String>, title: &str, subtitle: &str) -> String {
    let st = render::style_of(&plan.style);
    let (w, h) = (plan.width, plan.height);
    let mut p: Vec<String> = Vec::new();
    p.push(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" data-zoom="{}" data-stitch="1" data-ad="{}" data-page="{}" data-pages="{}" data-tiles="{}" data-tile-px="{:.0}" data-cell-km="{:.1}" font-family="system-ui,sans-serif">"#,
        plan.zoom,
        esc(&plan.ad),
        plan.page,
        plan.pages,
        plan.tiles.len(),
        plan.tile_px,
        plan.cell_m / 1000.0
    ));
    let mut css: Vec<String> = Vec::new();
    let mut first_style = true;
    for t in &plan.tiles {
        if let Some(b) = bodies.get(&t.adcode) {
            let (_, s) = take_style(b);
            if first_style {
                if let Some(s) = s {
                    css.push(s);
                    first_style = false;
                }
            }
        }
    }
    css.push(format!(
        ".wm-tile-head{{fill:{}}}.wm-tile-name{{fill:{};font-weight:600}}.wm-tile-meta{{fill:{};font-size:11.5px}}\
         .wm-tile-frame{{fill:none;stroke:{};stroke-width:1.5}}\
         .wm-title{{fill:{};font-weight:700}}.wm-sub{{fill:{};font-size:12.5px}}\
         .wm-idx{{fill:{};font-weight:700;font-size:12px}}",
        // 外框用 bld_edge（建筑描边色）而不是 road_edge：后者在高德配色下几乎是白的，
        // 拼起来根本看不出"每块有框"（渲染成 PNG 看过一次才发现）
        st.blockbg, st.text, st.sub_text, st.bld_edge, st.text, st.sub_text, st.text_halo
    ));
    p.push(format!("<style>{}</style>", css.join("")));
    p.push(format!(
        r#"<rect width="{w:.0}" height="{h:.0}" fill="{}"/>"#,
        st.bg
    ));
    // 画布标题（左上）与副标题（右上）
    p.push(format!(
        r#"<text class="wm-title" x="{:.0}" y="{:.0}" font-size="19">{}</text>"#,
        MARGIN,
        MARGIN + 20.0,
        esc(title)
    ));
    p.push(format!(
        r#"<text class="wm-sub" x="{:.0}" y="{:.0}" text-anchor="end">{}</text>"#,
        w - MARGIN,
        MARGIN + 19.0,
        esc(subtitle)
    ));

    for t in &plan.tiles {
        let size = t.size;
        let hdr = HEADER_H;
        p.push(format!(
            r#"<g class="wm-tile" data-ad="{}" data-index="{}" data-hi="{}" data-grid="{}" transform="translate({:.1},{:.1})">"#,
            esc(&t.adcode),
            t.index,
            if t.hi { 1 } else { 0 },
            t.grid,
            t.x,
            t.y
        ));
        // ① 标题带：编号 + 区县名（左）· adcode + 跨度（右）
        p.push(format!(
            r#"<rect class="wm-tile-head" x="0" y="0" width="{size:.0}" height="{hdr:.0}" rx="4"/>"#
        ));
        p.push(format!(
            r#"<text class="wm-tile-name" x="7" y="{:.0}" font-size="13.5">{} {}</text>"#,
            hdr - 8.0,
            t.index,
            esc(&t.name)
        ));
        p.push(format!(
            r#"<text class="wm-tile-meta" x="{:.0}" y="{:.0}" text-anchor="end">{} · {:.0}km{}</text>"#,
            size - 7.0,
            hdr - 8.0,
            esc(&t.adcode),
            t.span_km,
            if t.hi { " · 高精" } else { "" }
        ));
        // ② 街区方块（顶部标题条 pad 高被 clip 裁掉，只留街区）
        p.push(format!(r#"<g transform="translate(0,{hdr:.0})">"#));
        p.push(format!(
            r#"<clipPath id="wmc-{}-{}"><rect x="0" y="{:.1}" width="{:.0}" height="{:.0}"/></clipPath>"#,
            plan.page,
            t.index,
            t.pad,
            size,
            size - t.pad
        ));
        p.push(format!(
            r#"<rect x="0" y="0" width="{size:.0}" height="{size:.0}" rx="4" fill="{}"/>"#,
            st.bg
        ));
        match bodies.get(&t.adcode) {
            Some(b) => {
                let (body, _) = take_style(b);
                p.push(format!(
                    r#"<g clip-path="url(#wmc-{}-{})">{}</g>"#,
                    plan.page, t.index, body
                ));
            }
            None => {
                p.push(format!(
                    r#"<text x="{:.0}" y="{:.0}" text-anchor="middle" fill="{}" font-size="13">（这一块没渲染出来）</text>"#,
                    size / 2.0,
                    size / 2.0,
                    st.sub_text
                ));
            }
        }
        p.push(format!(
            r#"<rect class="wm-tile-frame" x="0.75" y="0.75" width="{:.1}" height="{:.1}" rx="4"/>"#,
            size - 1.5,
            size - 1.5
        ));
        p.push("</g></g>".into());
    }
    p.push("</svg>".into());
    p.join("")
}

// ═══════════════════════════════════════════════════════════════════
//  ⑥ 缓存（走 maplib 既有入库逻辑）
// ═══════════════════════════════════════════════════════════════════

fn md5_hex(s: &str, n: usize) -> String {
    let d = md5::compute(s.as_bytes());
    d.0.iter().take(n).map(|b| format!("{b:02x}")).collect()
}

/// 每块的确定性随机种子（同 adcode 永远同一张街区图）
pub fn tile_seed(adcode: &str) -> u64 {
    let d = md5::compute(adcode.as_bytes());
    u64::from_be_bytes([
        d.0[0], d.0[1], d.0[2], d.0[3], d.0[4], d.0[5], d.0[6], d.0[7],
    ])
}

/// 整张大图的缓存键：**形状参数 + 每块的 (adcode, 网格, 是否高精)** 全进哈希。
/// 于是：换尺寸/换页/换高精块 → 新键；同样的两遍法第二遍 → 命中。
pub fn cache_key(ad: &str, o: &Opts, plan: &Plan) -> String {
    let mut s = format!(
        "stitch|v{}|{}|{}|{:.0}x{:.0}|p{}/{}|n{}|d{}|g{}|z{}|s{}|",
        STITCH_VERSION,
        ad.trim(),
        o.style,
        o.width,
        o.height,
        plan.page,
        plan.pages,
        o.max_tiles,
        o.detail as u8,
        o.grid.map(|g| g.to_string()).unwrap_or_else(|| "-".into()),
        o.zoom,
        o.seed.map(|x| x.to_string()).unwrap_or_else(|| "-".into())
    );
    for t in &plan.tiles {
        s.push_str(&format!("{}:{}:{}|", t.adcode, t.grid, t.hi as u8));
    }
    // 请求的高精块列表也进键（哪怕它这一页没被选中：键跟着"请求"走更好解释）
    let mut hi = o.hi.clone();
    hi.sort();
    s.push_str(&format!("hi={}|", hi.join(",")));
    md5_hex(&s, 12)
}

/// 单块 SVG 的缓存键
pub fn tile_key(t: &Tile, style: &str, detail: bool, seed: Option<u64>) -> String {
    md5_hex(
        &format!(
            "tile|v{}|{}|{}|{}|g{}|d{}|px{:.0}|z{}|s{}",
            STITCH_VERSION,
            t.adcode,
            t.name,
            style,
            t.grid,
            detail as u8,
            t.size,
            "-",
            seed.map(|x| x.to_string()).unwrap_or_else(|| "-".into())
        ),
        12,
    )
}

/// maplib 的相对路径基准（= `MapLib::abs` 用的那个"项目根"：地图库根目录的祖父）。
/// 抽出来是为了让缓存读写与 `MapLib::register` 落在同一个地方。
pub fn lib_base(lib: &MapLib) -> PathBuf {
    lib.root()
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| lib.root().to_path_buf())
}

/// 写一份 SVG 进地图库：写文件 + `register`（kind 见调用方）。
/// 返回 (相对路径, 绝对路径)。
pub fn save_svg(lib: &MapLib, rel: &str, svg: &str, kind: &str, ad: &str, style: &str, meta: Value) -> Option<(String, PathBuf)> {
    let abs = lib_base(lib).join(rel);
    if let Some(dir) = abs.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&abs, svg).ok()?;
    lib.register(rel, kind, ad, style, meta);
    Some((rel.to_string(), abs))
}

/// 读一份已入库的 SVG（不在库里/文件没了 → None）
pub fn load_svg(lib: &MapLib, rel: &str) -> Option<String> {
    let abs = lib_base(lib).join(rel);
    std::fs::read_to_string(abs).ok()
}

// ═══════════════════════════════════════════════════════════════════
//  ⑦ 编排：计划 → 每块（带缓存）→ 拼装（超限降级）
// ═══════════════════════════════════════════════════════════════════

pub struct StitchInput<'a> {
    pub ad: &'a str,
    pub city_name: &'a str,
    pub districts: &'a [District],
    pub opts: Opts,
    pub lib: &'a MapLib,
    /// 入库用的相对路径前缀（相对地图库根目录的祖父），如 `images/bigmap_stitch`
    pub rel_prefix: &'a str,
    /// OSM 缓存目录（只读；命中就让草图贴近真实密度，不联网）
    pub osm_dir: Option<&'a Path>,
    /// true = 连每块缓存都不用，全部重画
    pub refresh: bool,
}

pub struct StitchOut {
    pub svg: String,
    pub plan: Plan,
    pub bytes: usize,
    pub ms: f64,
    pub cached: bool,
    pub degraded: bool,
    pub tiles_rendered: usize,
    pub tiles_reused: usize,
    pub cache_key: String,
    pub rel_path: String,
}

/// 只读 OSM 缓存拿密度提示（拿不到就 None，草图照常出）
pub fn osm_hint(dir: Option<&Path>, lat: f64, lng: f64) -> Option<sketch::OsmHint> {
    let dir = dir?;
    let key = osm::grid_key(lat, lng, 400.0);
    let v = osm::load_cached(dir, &key)?;
    osm::summarize(&v).map(|s| sketch::OsmHint::from_summary(&s))
}

fn render_tile_body(t: &Tile, style: &str, detail: bool, seed: Option<u64>, hint: Option<&sketch::OsmHint>, zoom: i32) -> String {
    let mut lay = sketch::make_sketch(&t.name, t.grid, Some(seed.unwrap_or_else(|| tile_seed(&t.adcode))), hint);
    if detail {
        details::enrich(&mut lay);
    }
    let o = render::Opts {
        style: style.to_string(),
        width: t.size,
        height: t.size,
        pad: t.pad,
        zoom,
        // 拼接大图里不放车流/人流动画：几十个动画同时跑，手机扛不住，也没必要
        animate: false,
        layers: true,
        mode: "2d".into(),
        charts: false,
    };
    render::render_svg(&lay, &o)
}

/// 五步走：计划 → 缓存命中？ → 每块 SVG（带缓存）→ 拼装 → 超限降级重拼
pub fn stitch(inp: &StitchInput) -> Result<StitchOut, String> {
    let t0 = std::time::Instant::now();
    let o = inp.opts.normalized();
    let plan = plan(inp.districts, inp.ad, &o);
    if plan.tiles.is_empty() {
        return Err(format!("{} 下辖没有可拼接的区县（缓存里没有区县几何？）", inp.ad));
    }
    let key = cache_key(inp.ad, &o, &plan);
    let prefix = inp.rel_prefix.trim_end_matches('/').to_string();
    let big_rel = format!("{prefix}/{key}.svg");
    if !inp.refresh {
        if let Some(svg) = load_svg(inp.lib, &big_rel) {
            let bytes = svg.len();
            return Ok(StitchOut {
                svg,
                plan,
                bytes,
                ms: t0.elapsed().as_secs_f64() * 1000.0,
                cached: true,
                degraded: false,
                tiles_rendered: 0,
                tiles_reused: 0,
                cache_key: key,
                rel_path: big_rel,
            });
        }
    }

    let title = if inp.city_name.trim().is_empty() {
        format!("{} · 城市拼接大图", inp.ad)
    } else {
        format!("{} · 城市拼接大图", inp.city_name.trim())
    };

    let mut rendered = 0usize;
    let mut reused = 0usize;
    let mut bodies: BTreeMap<String, String> = BTreeMap::new();
    let mut css: Option<String> = None;
    for t in &plan.tiles {
        let tk = tile_key(t, &o.style, t.hi || o.detail, o.seed);
        let rel = format!("{prefix}/tiles/{tk}.svg");
        let body = if !inp.refresh {
            load_svg(inp.lib, &rel).map(|svg| {
                reused += 1;
                svg
            })
        } else {
            None
        };
        let body = match body {
            Some(b) => b,
            None => {
                let hint = osm_hint(inp.osm_dir, t.lat, t.lng);
                let svg = render_tile_body(t, &o.style, t.hi || o.detail, o.seed, hint.as_ref(), o.zoom);
                let body = body_of(&svg).unwrap_or(svg);
                // 只留一份 CSS 在整图里；单块缓存里存"带 CSS 的完整内容"，
                // 这样单块也能单独打开看（拼装时再摘掉重复的那几份）
                rendered += 1;
                save_svg(
                    inp.lib,
                    &rel,
                    &body,
                    "bigmap_tile",
                    &t.adcode,
                    &o.style,
                    json!({"kind": "stitch_tile", "grid": t.grid, "hi": t.hi, "size": t.size, "style": o.style}),
                );
                bodies.insert(t.adcode.clone(), body.clone());
                body
            }
        };
        if css.is_none() {
            let (_, s) = take_style(&body);
            css = s;
        }
        bodies.insert(t.adcode.clone(), body);
    }

    let subtitle = format!(
        "{}/{} 块 · 第 {}/{} 页 · {}×{} 网格 · 单块 {:.0}px · 格距 {:.0}km{}",
        plan.tiles.len(),
        plan.total,
        plan.page,
        plan.pages,
        plan.cols,
        plan.rows,
        plan.tile_px,
        plan.cell_m / 1000.0,
        if o.detail { " · 含街道细节" } else { "" }
    );
    let mut plan = plan;
    let mut svg = compose_svg(&plan, &bodies, &title, &subtitle);
    let mut degraded = false;
    if svg.len() > o.max_bytes {
        // 超限降级：关细节 + 网格压到 ≤16，再拼一次（宁可粗一点，也不能把 WebView 撑爆）
        degraded = true;
        let mut plan2 = plan.clone();
        for t in plan2.tiles.iter_mut() {
            t.grid = t.grid.min(16);
        }
        plan2.detail = false;
        let mut bodies2: BTreeMap<String, String> = BTreeMap::new();
        for t in &plan2.tiles {
            let hint = osm_hint(inp.osm_dir, t.lat, t.lng);
            let raw = render_tile_body(t, &o.style, false, o.seed, hint.as_ref(), o.zoom);
            if let Some(b) = body_of(&raw) {
                bodies2.insert(t.adcode.clone(), b);
            }
            rendered += 1;
        }
        svg = compose_svg(&plan2, &bodies2, &title, &format!("{subtitle} · 已降级"));
        // 回传"实际画出来的"计划：JSON 里的 grid/细节必须与 SVG 一致
        plan = plan2;
    }
    let bytes = svg.len();
    // kind 用 `bigmap_stitch`（不是 `bigmap`）：maplib 的 eid 是 `{kind}:{ad}:{style}`，
    // 若与「总览」共用 kind，会把 Python 时代那张行政区划 PNG 的索引条目顶掉
    // （本代理在联调时真踩到过：bigmap:440100:gaode 被拼接图覆盖，已还原）。
    save_svg(
        inp.lib,
        &big_rel,
        &svg,
        "bigmap_stitch",
        inp.ad,
        &o.style,
        json!({
            "kind": "stitch", "page": plan.page, "pages": plan.pages,
            "tiles": plan.tiles.len(), "total": plan.total,
            "cols": plan.cols, "rows": plan.rows, "tilePx": plan.tile_px,
            "cellKm": (plan.cell_m / 100.0).round() / 10.0,
            "canvas": [plan.width, plan.height],
            "detail": o.detail, "hi": o.hi, "degraded": degraded,
            "adcodes": plan.tiles.iter().map(|t| t.adcode.clone()).collect::<Vec<_>>(),
        }),
    );
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    Ok(StitchOut {
        svg,
        plan,
        bytes,
        ms,
        cached: false,
        degraded,
        tiles_rendered: rendered,
        tiles_reused: reused,
        cache_key: key,
        rel_path: big_rel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(ad: &str, name: &str, lng: f64, lat: f64, span_km: f64, area_km2: f64) -> District {
        District {
            adcode: ad.into(),
            name: name.into(),
            lng,
            lat,
            span_m: span_km * 1000.0,
            area_km2,
        }
    }

    /// 广州 11 个区（真实坐标/跨度/面积，用来做"像不像"的回归）
    fn guangzhou() -> Vec<District> {
        vec![
            d("440103", "荔湾区", 113.2430, 23.1249, 13.0, 59.0),
            d("440104", "越秀区", 113.2807, 23.1256, 8.5, 33.0),
            d("440105", "海珠区", 113.3309, 23.0787, 18.2, 90.0),
            d("440106", "天河区", 113.3745, 23.1718, 16.3, 96.0),
            d("440111", "白云区", 113.3301, 23.2834, 36.8, 795.0),
            d("440112", "黄埔区", 113.5029, 23.2211, 42.2, 484.0),
            d("440113", "番禺区", 113.4123, 22.9686, 33.4, 530.0),
            d("440114", "花都区", 113.2172, 23.4324, 52.9, 970.0),
            d("440115", "南沙区", 113.5184, 22.7114, 45.6, 527.0),
            d("440117", "从化区", 113.6708, 23.6503, 79.4, 1974.0),
            d("440118", "增城区", 113.7746, 23.3520, 59.2, 1616.0),
        ]
    }

    fn fake_fc(names: &[(&str, &str, f64, f64)]) -> Value {
        json!({
            "type": "FeatureCollection",
            "features": names.iter().map(|(ad, name, lng, lat)| json!({
                "type": "Feature",
                "properties": {"adcode": ad, "name": name, "center": [lng, lat]},
                "geometry": {"type": "Polygon", "coordinates": [[
                    [lng - 0.05, lat - 0.05], [lng + 0.05, lat - 0.05],
                    [lng + 0.05, lat + 0.05], [lng - 0.05, lat + 0.05], [lng - 0.05, lat - 0.05]
                ]]},
            })).collect::<Vec<_>>(),
        })
    }

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(tag: &str) -> Self {
            let base = std::env::var("TMPDIR").unwrap_or_else(|_| {
                format!("{}/.cache", std::env::var("HOME").unwrap_or_else(|_| ".".into()))
            });
            let p = PathBuf::from(base).join(format!("wm_stitch_{tag}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(p.join("worlddata").join("maplib")).unwrap();
            std::fs::create_dir_all(p.join("images")).unwrap();
            Tmp(p)
        }
        fn lib(&self) -> MapLib {
            MapLib::new(self.0.join("worlddata").join("maplib"))
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // ① 解析
    #[test]
    fn districts_of_reads_center_and_span() {
        let fc = fake_fc(&[("440104", "越秀区", 113.28, 23.13), ("440103", "荔湾区", 113.24, 23.12)]);
        let ds = districts_of(&fc);
        assert_eq!(ds.len(), 2);
        assert_eq!(ds[0].adcode, "440104");
        assert!((ds[0].lng - 113.28).abs() < 1e-9 && (ds[0].lat - 23.13).abs() < 1e-9);
        // 0.1° 见方的方块 ≈ 10.2km × 11.1km
        assert!(ds[0].span_m > 10_000.0 && ds[0].span_m < 12_000.0, "span={}", ds[0].span_m);
        assert!(ds[0].area_km2 > 100.0 && ds[0].area_km2 < 130.0, "area={}", ds[0].area_km2);
    }

    #[test]
    fn districts_of_skips_nameless_and_empty() {
        let fc = json!({"features": [
            {"properties": {"adcode": "1", "name": ""}, "geometry": {"coordinates": [[1.0, 2.0]]}},
            {"properties": {"adcode": "2", "name": "空几何"}, "geometry": {"coordinates": []}},
            {"properties": {"adcode": "3", "name": "好区"}, "geometry": {"coordinates": [[[1.0, 2.0], [1.1, 2.1]]]}}
        ]});
        let ds = districts_of(&fc);
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].name, "好区");
    }

    #[test]
    fn load_city_fc_falls_back_to_full() {
        let t = Tmp::new("fc");
        // 只有 _full（深圳就是这样）也必须能读到
        std::fs::write(t.0.join("440300_full.json"), fake_fc(&[("440303", "罗湖区", 114.12, 22.55)]).to_string()).unwrap();
        let (fc, path) = load_city_fc(&t.0, "440300").unwrap();
        assert!(path.to_string_lossy().ends_with("440300_full.json"));
        assert_eq!(districts_of(&fc).len(), 1);
        assert!(load_city_fc(&t.0, "999999").is_err());
    }

    #[test]
    fn city_name_from_parent_cache() {
        let t = Tmp::new("name");
        std::fs::write(
            t.0.join("440000.json"),
            fake_fc(&[("440100", "广州市", 113.26, 23.13), ("440300", "深圳市", 114.05, 22.54)]).to_string(),
        )
        .unwrap();
        assert_eq!(city_name_of(&t.0, "440100").as_deref(), Some("广州市"));
        assert_eq!(city_name_of(&t.0, "440300").as_deref(), Some("深圳市"));
        assert_eq!(city_name_of(&t.0, "440600"), None);
    }

    // ② 选块 / 分页
    #[test]
    fn select_trims_to_max_tiles_and_keeps_both_big_and_central() {
        let gz = guangzhou();
        let sel = select_page(&gz, 9, 1);
        assert_eq!(sel.picked.len(), 9);
        assert_eq!(sel.dropped.len(), 2);
        assert_eq!(sel.total, 11);
        assert_eq!(sel.pages, 2);
        let picked: Vec<&str> = sel.picked.iter().map(|d| d.adcode.as_str()).collect();
        // 大块：从化（最大）；中心：越秀（最小但在市中心）—— 两条都要在
        assert!(picked.contains(&"440117"), "从化（面积第一）必须在");
        assert!(picked.contains(&"440104"), "越秀（最中心）必须在");
        // 11 块被 9 格裁掉 2 块，且两块加起来是全集
        assert_eq!(sel.picked.len() + sel.dropped.len(), 11);
    }

    #[test]
    fn pages_do_not_overlap_and_cover_everything() {
        let gz = guangzhou();
        let p1 = select_page(&gz, 9, 1);
        let p2 = select_page(&gz, 9, 2);
        assert_eq!(p1.pages, 2);
        assert_eq!(p2.picked.len(), 2);
        let mut all: Vec<String> = p1
            .picked
            .iter()
            .chain(p2.picked.iter())
            .map(|d| d.adcode.clone())
            .collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 11, "两页合起来必须正好覆盖 11 个区县，不重不漏");
        // 页码越界要夹回来
        assert_eq!(select_page(&gz, 9, 99).page, 2);
        assert_eq!(select_page(&gz, 9, 0).page, 1);
    }

    // ③ 网格排布
    #[test]
    fn cells_never_collide() {
        let gz = guangzhou();
        let sel = select_page(&gz, 9, 1);
        let (alng, alat) = centroid(&sel.picked);
        let fit = choose_lattice(&sel.picked, alng, alat);
        let mut seen = std::collections::BTreeSet::new();
        for c in &fit.cells {
            assert!(seen.insert(*c), "两块落在同一格：{c:?}");
        }
        assert_eq!(fit.cells.len(), 9);
        assert!(fit.cell_m > 1000.0);
    }

    #[test]
    fn lattice_keeps_cardinal_directions() {
        // 中心 + 正东南西北各一块：方位必须一眼看出来（这是"真拼接"的底线）
        let ds = vec![
            d("100000", "中心", 113.30, 23.10, 10.0, 100.0),
            d("100001", "北块", 113.30, 23.60, 10.0, 100.0),
            d("100002", "东块", 113.90, 23.10, 10.0, 100.0),
            d("100003", "南块", 113.30, 22.60, 10.0, 100.0),
            d("100004", "西块", 112.70, 23.10, 10.0, 100.0),
        ];
        let (alng, alat) = centroid(&ds);
        let fit = choose_lattice(&ds, alng, alat);
        let pos = |i: usize| fit.cells[i];
        let (c, n, e, s, w) = (pos(0), pos(1), pos(2), pos(3), pos(4));
        assert!(n.1 < c.1, "北块的行号必须比中心小（屏幕向上）：{n:?} vs {c:?}");
        assert!(s.1 > c.1, "南块的行号必须比中心大：{s:?} vs {c:?}");
        assert!(e.0 > c.0, "东块的列号必须比中心大：{e:?} vs {c:?}");
        assert!(w.0 < c.0, "西块的列号必须比中心小：{w:?} vs {c:?}");
        assert_eq!(fit.flips, 0, "四正方向不该出现方位错");
    }

    #[test]
    fn cell_search_prefers_fewer_direction_flips_then_compact() {
        let gz = guangzhou();
        let sel = select_page(&gz, 9, 1);
        let (alng, alat) = centroid(&sel.picked);
        let base = base_cell_m(&sel.picked, alat);
        let chosen = choose_lattice(&sel.picked, alng, alat);
        // 选出来的格距必须来自候选集合，且不比基准小
        let mult = chosen.cell_m / base;
        assert!(CELL_MULTS.iter().any(|m| (m - mult).abs() < 1e-6), "格距不在候选集里：{mult}");
        // 不许比"最差候选"更糟：任何候选的方位错数都不小于它
        for &m in CELL_MULTS.iter() {
            let f = fit_lattice(&sel.picked, alng, alat, (base * m).max(MIN_CELL_M));
            assert!(chosen.flips <= f.flips, "选中格距的方位错 {} 多于候选 {m} 的 {}", chosen.flips, f.flips);
        }
        assert!(chosen.flips * 2 <= chosen.pairs.max(1), "方位错不能过半");
        assert!(chosen.fill >= 0.3, "填充率太低会变成一盘散沙：{}", chosen.fill);
    }

    #[test]
    fn guangzhou_lattice_is_recognizable() {
        // 回归：从化在北、南沙在南、增城在东、花都在西北 —— 拼出来的相对方位必须像广州。
        // 这里放开到 11 块（默认 9 块会裁掉两块，缺的块当然查不到），验的是"排布像不像"。
        let gz = guangzhou();
        let p = plan(&gz, "440100", &Opts { max_tiles: 11, ..Default::default() });
        assert_eq!(p.tiles.len(), 11);
        let at = |ad: &str| p.tile_of(ad).expect("这一页该有这块").clone();
        let (ch, ns, zc, hd, yx, by) = (
            at("440117"), at("440115"), at("440118"), at("440114"), at("440104"), at("440111"),
        );
        assert!(ch.row < ns.row, "从化应在南沙北边");
        assert!(zc.col > hd.col, "增城应在花都东边");
        assert!(hd.row < yx.row, "花都应在越秀北边");
        assert!(yx.row < ns.row, "越秀应在南沙北边");
        assert!(by.row < yx.row, "白云应在越秀北边");
        assert!(zx_row_below(&p), "增城应在从化南边（东侧）");
        assert_eq!(p.flips, 0, "真实数据上不该出现方位错：{:?}", p.flips);
        // 每块都在画布里，且互不重叠
        for t in &p.tiles {
            assert!(t.x >= 0.0 && t.y >= 0.0);
            assert!(t.x + t.size <= p.width + 0.5, "{} 越出画布右边", t.name);
            assert!(t.y + HEADER_H + t.size <= p.height + 0.5, "{} 越出画布下边", t.name);
        }
        for i in 0..p.tiles.len() {
            for j in (i + 1)..p.tiles.len() {
                let (a, b) = (&p.tiles[i], &p.tiles[j]);
                let overlap = a.x < b.x + b.size && b.x < a.x + a.size
                    && a.y < b.y + b.size + HEADER_H && b.y < a.y + a.size + HEADER_H;
                assert!(!overlap, "{} 与 {} 的框叠在一起了", a.name, b.name);
            }
        }
    }

    /// 从化在广州最北，增城在它南边
    fn zx_row_below(p: &Plan) -> bool {
        match (p.tile_of("440118"), p.tile_of("440117")) {
            (Some(z), Some(c)) => z.row > c.row,
            _ => false,
        }
    }

    // ④ 尺寸反推
    #[test]
    fn canvas_fits_within_cap_and_tile_scales_with_cap() {
        let gz = guangzhou();
        for cap in [800.0, 1200.0, 2000.0, 2400.0] {
            let o = Opts { width: cap, height: cap, ..Default::default() };
            let p = plan(&gz, "440100", &o);
            assert!(p.width <= cap + 1.0, "宽 {} 超过上限 {cap}", p.width);
            assert!(p.height <= cap + 1.0, "高 {} 超过上限 {cap}", p.height);
            assert!(p.tile_px >= MIN_TILE_PX && p.tile_px <= MAX_TILE_PX);
        }
        let small = plan(&gz, "440100", &Opts { width: 900.0, height: 900.0, ..Default::default() });
        let big = plan(&gz, "440100", &Opts::default());
        assert!(big.tile_px > small.tile_px, "上限变大，块也应该变大");
    }

    #[test]
    fn grid_is_derived_from_tile_px() {
        assert_eq!(grid_for(220.0, false, None), 12, "小块的网格被下限兜住");
        assert_eq!(grid_for(440.0, false, None), 20);
        assert_eq!(grid_for(2000.0, false, None), GRID_MAX, "大块被上限兜住");
        assert!(grid_for(600.0, true, None) > grid_for(600.0, false, None), "高精块网格更细");
        assert_eq!(grid_for(600.0, true, Some(20)), 30, "高精 = 指定网格 ×1.5");
        assert_eq!(grid_for(600.0, false, Some(999)), GRID_MAX, "强制网格也要夹紧");
    }

    #[test]
    fn max_tiles_is_clamped() {
        let gz = guangzhou();
        assert_eq!(plan(&gz, "440100", &Opts { max_tiles: 0, ..Default::default() }).tiles.len(), 1);
        assert_eq!(plan(&gz, "440100", &Opts { max_tiles: 1, ..Default::default() }).tiles.len(), 1);
        let p = plan(&gz, "440100", &Opts { max_tiles: 999, ..Default::default() });
        assert_eq!(p.tiles.len(), 11, "上限比区县还多时，有几块拼几块");
        assert!(p.max_tiles <= MAX_TILES_CAP);
    }

    // ⑤ 两遍法（低精度整张 → 按需替换某一块）
    #[test]
    fn plan_is_independent_of_hi_and_detail() {
        let gz = guangzhou();
        let a = plan(&gz, "440100", &Opts::default());
        let b = plan(
            &gz,
            "440100",
            &Opts { detail: true, hi: vec!["440104".into()], ..Default::default() },
        );
        assert_eq!(a.tiles.len(), b.tiles.len());
        for (x, y) in a.tiles.iter().zip(b.tiles.iter()) {
            assert_eq!((x.adcode.clone(), x.col, x.row, x.x, x.y, x.size), (y.adcode.clone(), y.col, y.row, y.x, y.y, y.size),
                "两遍法的几何必须完全一致，否则高精度块会错位");
        }
        let normal = a.tile_of("440104").unwrap();
        let hi = b.tile_of("440104").unwrap();
        assert!(!normal.hi && hi.hi);
        assert!(hi.grid > normal.grid, "高精度块的网格必须更细：{} vs {}", hi.grid, normal.grid);
    }

    #[test]
    fn cache_key_covers_shape_parameters() {
        let gz = guangzhou();
        let o = Opts::default();
        let p = plan(&gz, "440100", &o);
        let k = |oo: &Opts| cache_key("440100", oo, &plan(&gz, "440100", oo));
        assert_eq!(k(&o), cache_key("440100", &o, &p), "同参数必须同键");
        assert_ne!(k(&o), k(&Opts { page: 2, ..Default::default() }));
        assert_ne!(k(&o), k(&Opts { style: "dark".into(), ..Default::default() }));
        assert_ne!(k(&o), k(&Opts { detail: true, ..Default::default() }));
        assert_ne!(k(&o), k(&Opts { hi: vec!["440104".into()], ..Default::default() }));
        assert_ne!(k(&o), k(&Opts { width: 1200.0, ..Default::default() }));
        assert_ne!(k(&o), k(&Opts { max_tiles: 4, ..Default::default() }));
        assert_ne!(cache_key("440300", &o, &p), cache_key("440100", &o, &p), "换城市必须换键");
    }

    #[test]
    fn tile_seed_is_stable_and_distinct() {
        assert_eq!(tile_seed("440104"), tile_seed("440104"));
        assert_ne!(tile_seed("440104"), tile_seed("440103"));
        let a = sketch::make_sketch("越秀区", 20, Some(tile_seed("440104")), None);
        let b = sketch::make_sketch("越秀区", 20, Some(tile_seed("440103")), None);
        assert_ne!(a["buildings"], b["buildings"], "不同块必须是不同的街区图");
    }

    // ⑥ SVG 拼装
    #[test]
    fn body_of_strips_svg_wrapper() {
        assert_eq!(body_of("<svg width=\"10\"><rect/></svg>").unwrap(), "<rect/>");
        assert_eq!(body_of("  <svg a=\"1\" b=\"2\">X</svg>").unwrap(), "X");
        assert!(body_of("not svg").is_none());
        assert!(body_of("<svg a=\"1\">no close").is_none());
    }

    #[test]
    fn take_style_extracts_and_removes_css() {
        let (rest, css) = take_style("A<style>.x{fill:red}</style>B");
        assert_eq!(rest, "AB");
        assert_eq!(css.as_deref(), Some(".x{fill:red}"));
        let (rest2, css2) = take_style("no style here");
        assert_eq!(rest2, "no style here");
        assert!(css2.is_none());
    }

    #[test]
    fn compose_has_frame_label_and_one_style() {
        let gz = guangzhou();
        let o = Opts { max_tiles: 4, ..Default::default() };
        let p = plan(&gz, "440100", &o);
        let mut bodies = BTreeMap::new();
        for t in &p.tiles {
            bodies.insert(
                t.adcode.clone(),
                format!("<style>.z1{{}}</style><rect class=\"z1\" width=\"{:.0}\" height=\"{:.0}\"/>", t.size, t.size),
            );
        }
        let svg = compose_svg(&p, &bodies, "广州市 · 城市拼接大图", "4 块");
        assert_eq!(svg.matches("class=\"wm-tile\"").count(), p.tiles.len(), "每块都要有外框容器");
        assert_eq!(svg.matches("class=\"wm-tile-frame\"").count(), p.tiles.len(), "每块都要有外框线");
        assert_eq!(svg.matches("<style>").count(), 1, "CSS 只该保留一份");
        assert_eq!(svg.matches("clipPath").count(), p.tiles.len() * 2, "每块一份 clipPath（定义+引用）");
        for t in &p.tiles {
            assert!(svg.contains(&t.name), "区县名 {} 必须出现在大图里", t.name);
            assert!(svg.contains(&format!("data-ad=\"{}\"", t.adcode)));
        }
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        assert!(svg.contains("data-stitch=\"1\""));
    }

    #[test]
    fn compose_survives_missing_tile_body() {
        let gz = guangzhou();
        let p = plan(&gz, "440100", &Opts { max_tiles: 3, ..Default::default() });
        let svg = compose_svg(&p, &BTreeMap::new(), "空", "无块");
        assert!(svg.contains("没渲染出来"));
        assert_eq!(svg.matches("class=\"wm-tile\"").count(), 3);
    }

    #[test]
    fn compose_escapes_names() {
        let ds = vec![d("1", "A&B<C>\"D\"", 113.3, 23.1, 5.0, 10.0)];
        let p = plan(&ds, "440100", &Opts::default());
        let svg = compose_svg(&p, &BTreeMap::new(), "t&t", "s");
        assert!(svg.contains("A&amp;B&lt;C&gt;&quot;D&quot;"));
        assert!(!svg.contains("A&B<C>"));
    }

    // ⑦ 端到端（走真 maplib 缓存目录）
    #[test]
    fn stitch_renders_composes_and_caches() {
        let t = Tmp::new("e2e");
        let lib = t.lib();
        let gz = guangzhou();
        let o = Opts { max_tiles: 4, width: 900.0, height: 900.0, ..Default::default() };
        let inp = StitchInput {
            ad: "440100",
            city_name: "广州市",
            districts: &gz,
            opts: o.clone(),
            lib: &lib,
            rel_prefix: "images/bigmap_stitch",
            osm_dir: None,
            refresh: false,
        };
        let out = stitch(&inp).unwrap();
        assert!(!out.cached && !out.degraded);
        assert_eq!(out.tiles_rendered, 4);
        assert!(out.svg.starts_with("<svg"), "必须是裸 SVG 文本");
        assert!(out.bytes > 2000, "太短了，像空图：{}", out.bytes);
        assert_eq!(out.bytes, out.svg.len());
        // 大图落在 images/bigmap_stitch/<key>.svg，并已入地图库
        let abs = lib_base(&lib).join(&out.rel_path);
        assert!(abs.exists(), "大图没落盘：{abs:?}");
        assert!(
            lib.get(&format!("bigmap_stitch:440100:{}", o.style)).is_some(),
            "地图库里应有这条记录（kind=bigmap_stitch，不能顶掉总览的 bigmap）"
        );
        // 每块的 SVG 也要入库（kind=bigmap_tile），否则第二遍法没有可复用的块
        for t in &out.plan.tiles {
            let eid = format!("bigmap_tile:{}:{}", t.adcode, o.style);
            assert!(lib.get(&eid).is_some(), "块级缓存缺记录：{eid}");
        }
        // 第二遍：命中缓存，且不再重画
        let out2 = stitch(&inp).unwrap();
        assert!(out2.cached, "第二遍必须命中缓存");
        assert_eq!(out2.svg.len(), out.bytes);
        assert_eq!(out2.tiles_rendered, 0);
        // refresh：重画
        let inp3 = StitchInput { refresh: true, ..inp };
        let out3 = stitch(&inp3).unwrap();
        assert!(!out3.cached);
        assert_eq!(out3.tiles_rendered, 4);
    }

    #[test]
    fn stitch_two_pass_reuses_other_tiles() {
        let t = Tmp::new("twopass");
        let lib = t.lib();
        let gz = guangzhou();
        let base = Opts { max_tiles: 9, width: 1200.0, height: 1200.0, ..Default::default() };
        let mk = |o: Opts| StitchInput {
            ad: "440100",
            city_name: "广州市",
            districts: &gz,
            opts: o,
            lib: &lib,
            rel_prefix: "images/bigmap_stitch",
            osm_dir: None,
            refresh: false,
        };
        let low = stitch(&mk(base.clone())).unwrap();
        assert_eq!(low.tiles_rendered, 9);
        // 第二遍：只把海珠（9 块里稳进的那一个）换成高精度
        assert!(low.plan.tile_of("440105").is_some(), "海珠该在这一页里");
        let hi = Opts { hi: vec!["440105".into()], ..base.clone() };
        let up = stitch(&mk(hi)).unwrap();
        assert_ne!(up.cache_key, low.cache_key, "换了高精块必须换大图缓存键");
        assert_eq!(up.tiles_rendered, 1, "只有被替换的那一块该重画，实际 {}", up.tiles_rendered);
        assert_eq!(up.tiles_reused, 8);
    }

    #[test]
    fn stitch_over_budget_falls_back() {
        let t = Tmp::new("budget");
        let lib = t.lib();
        let gz = guangzhou();
        let o = Opts {
            max_tiles: 9,
            width: 1400.0,
            height: 1400.0,
            detail: true,
            max_bytes: 12_000, // 故意压到不可能达到，逼出降级路径
            ..Default::default()
        };
        let inp = StitchInput {
            ad: "440100",
            city_name: "广州市",
            districts: &gz,
            opts: o,
            lib: &lib,
            rel_prefix: "images/bigmap_stitch",
            osm_dir: None,
            refresh: false,
        };
        let out = stitch(&inp).unwrap();
        assert!(out.degraded, "超限必须走降级");
        assert!(out.svg.contains("已降级"));
        assert!(out.plan.tiles.iter().all(|t| t.grid <= 16));
    }

    #[test]
    fn stitch_errors_on_empty_districts() {
        let t = Tmp::new("empty");
        let lib = t.lib();
        let inp = StitchInput {
            ad: "440100",
            city_name: "",
            districts: &[],
            opts: Opts::default(),
            lib: &lib,
            rel_prefix: "images/bigmap_stitch",
            osm_dir: None,
            refresh: false,
        };
        assert!(stitch(&inp).is_err());
    }

    #[test]
    fn lib_base_matches_maplib_relative_paths() {
        let t = Tmp::new("base");
        let lib = t.lib();
        assert_eq!(lib_base(&lib), t.0, "相对路径基准必须与 MapLib::abs 一致（根目录的祖父）");
        let (rel, abs) =
            save_svg(&lib, "images/bigmap_stitch/x.svg", "<svg/>", "bigmap_stitch", "440100", "gaode", json!({})).unwrap();
        assert_eq!(rel, "images/bigmap_stitch/x.svg");
        assert_eq!(abs, t.0.join("images/bigmap_stitch/x.svg"));
        assert_eq!(load_svg(&lib, &rel).as_deref(), Some("<svg/>"));
        assert!(load_svg(&lib, "images/nope.svg").is_none());
    }

    #[test]
    fn plan_json_has_everything_the_frontend_needs() {
        let gz = guangzhou();
        let p = plan(&gz, "440100", &Opts::default());
        let j = p.to_json();
        assert_eq!(j["ad"], "440100");
        assert_eq!(j["total"], 11);
        assert_eq!(j["pages"], 2);
        assert_eq!(j["tiles"].as_array().unwrap().len(), p.tiles.len());
        assert!(j["canvas"][0].as_f64().unwrap() <= DEFAULT_CANVAS);
        assert!(j["cellKm"].as_f64().unwrap() > 1.0);
        assert_eq!(j["dropped"].as_array().unwrap().len(), 2);
        assert!(j["tiles"][0]["name"].as_str().unwrap().len() >= 2);
    }

    #[test]
    fn normalized_clamps_garbage_options() {
        let o = Opts {
            style: "不存在".into(),
            max_tiles: 0,
            page: 0,
            width: f64::NAN,
            height: -5.0,
            zoom: 99,
            grid: Some(999),
            max_bytes: 0,
            ..Default::default()
        }
        .normalized();
        assert_eq!(o.style, "gaode");
        assert_eq!(o.max_tiles, 1);
        assert_eq!(o.page, 1);
        assert_eq!(o.width, DEFAULT_CANVAS);
        assert_eq!(o.height, DEFAULT_CANVAS);
        assert_eq!(o.zoom, 3);
        assert_eq!(o.grid, Some(GRID_MAX_HI));
        assert_eq!(o.max_bytes, DEFAULT_MAX_BYTES);
    }
}
