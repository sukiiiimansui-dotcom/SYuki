//! 城市级**真拼接**大图的 **Tauri 命令层**（T5-1）
//!
//! 纯逻辑全在 [`super::stitch`]（不依赖 tauri、不读系统时间，可脱离工程 `rustc --test`
//! 直接跑单测 —— 手机上没有 Tauri 全量编译预算），这里只做四件事：
//!   ① 取路径（geo 缓存 / 地图库 / OSM 只读缓存）
//!   ② 本地没有该市的 geojson 时联网抓一次（`GeoSource::fetch` 会写回缓存）
//!   ③ 调 `stitch::stitch`（选块 → 排布 → 逐块渲染 → 拼装 → 入库）
//!   ④ 把结果包成「裸 SVG」或「计划 JSON」
//!
//! ## 两条命令（**注册必须带全路径**，写短了会 E0433 —— 本项目踩过）
//!   · `world_map::stitch_cmd::world_map_bigmap_svg`  —— 返回 SVG 文本（前端直接内联/`<img>`）
//!   · `world_map::stitch_cmd::world_map_bigmap_plan` —— 返回计划与统计（做两遍法前先看它）
//!
//! ## 参数命名（Rust snake_case → JS camelCase）
//! 单词参数两侧同名：`ad` / `style` / `page` / `detail` / `hi` / `grid` / `seed` / `zoom` / `refresh`；
//! 多词参数 JS 写驼峰：`max_tiles` → `maxTiles`、`max_mb` → `maxMb`。
//!
//! ## 两遍法（低精度整张 → 按需替换某一块）
//! ```text
//! ① world_map_bigmap_svg({ ad:"440100" })                    // 9 块低精度整张（默认不挂街道细节）
//! ② world_map_bigmap_svg({ ad:"440100", hi:"440104" })        // 只把越秀换成高精度（其余块走缓存）
//! ```
//! 两次的**几何完全一致**（[`super::stitch::plan`] 与 `hi`/`detail` 无关，已有单测钉住），
//! 所以第二遍可以直接盖在第一遍上，不会错位。
use serde_json::{json, Value};
use tauri::AppHandle;

use super::stitch;

/// 大图与分块缓存的**相对路径前缀**。
///
/// 基准是「地图库根目录的祖父」（`MapLib::abs` 就是这么解析的）：
///   `<data_dir>/world_map/maplib` → 祖父 = `<data_dir>` → 落到 `<data_dir>/world_map/images/bigmap_stitch/`。
/// 与 `world_dir()`（`<data_dir>/world_map`）同根，不另造数据目录。
const REL_PREFIX: &str = "world_map/images/bigmap_stitch";

fn parse_hi(hi: Option<String>) -> Vec<String> {
    hi.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

/// 组装选项（各命令参数 → `stitch::Opts`，默认值都在 `stitch::Opts::default`）
#[allow(clippy::too_many_arguments)]
fn make_opts(
    style: Option<String>,
    max_tiles: Option<usize>,
    width: Option<f64>,
    height: Option<f64>,
    page: Option<usize>,
    detail: Option<bool>,
    hi: Option<String>,
    grid: Option<i32>,
    seed: Option<u64>,
    zoom: Option<i32>,
    max_mb: Option<f64>,
) -> stitch::Opts {
    let d = stitch::Opts::default();
    stitch::Opts {
        style: style.unwrap_or(d.style),
        max_tiles: max_tiles.unwrap_or(d.max_tiles),
        page: page.unwrap_or(d.page),
        width: width.unwrap_or(d.width),
        height: height.unwrap_or(d.height),
        detail: detail.unwrap_or(d.detail),
        hi: parse_hi(hi),
        grid,
        seed,
        zoom: zoom.unwrap_or(d.zoom),
        max_bytes: max_mb
            .map(|m| (m * 1048576.0) as usize)
            .unwrap_or(d.max_bytes),
    }
}

/// 真正的编排：读缓存（必要时联网）→ 拼 → 返回结构化结果
async fn build(
    app: &AppHandle,
    ad: &str,
    opts: stitch::Opts,
    refresh: bool,
) -> Result<stitch::StitchOut, String> {
    let ad = ad.trim();
    if ad.is_empty() {
        return Err("ad（市 adcode）不能为空".into());
    }
    let cache = super::cache_dir(app);
    // 本地缓存命中不了才联网（拼接是"点一下就该出图"的功能，能离线就离线）。
    // 注意 stitch::load_city_fc 会同时看 `{ad}.json` 与 `{ad}_full.json`，
    // 而 GeoSource::load_cached 只看前者 —— 深圳这类只有 _full 的市就是靠这一步不白跑网络。
    if stitch::load_city_fc(&cache, ad).is_err() {
        let src = super::make_source(app);
        src.fetch(ad)
            .await
            .map_err(|e| format!("{ad} 没有本地地理缓存，联网抓也失败：{e}"))?;
    }
    let (fc, _path) = stitch::load_city_fc(&cache, ad)?;
    let ds = stitch::districts_of(&fc);
    if ds.is_empty() {
        return Err(format!(
            "{ad} 的缓存里没有带名字的区县要素（省级/区县级 adcode？拼接要的是「市」）"
        ));
    }
    let city = stitch::city_name_of(&cache, ad).unwrap_or_default();
    let lib = super::maplib::MapLib::new(super::maplib_root(app));
    let osm = super::osm_dir(app);
    let osm_dir = if osm.is_dir() { Some(osm.as_path()) } else { None };
    let inp = stitch::StitchInput {
        ad,
        city_name: &city,
        districts: &ds,
        opts,
        lib: &lib,
        rel_prefix: REL_PREFIX,
        osm_dir,
        refresh,
    };
    stitch::stitch(&inp)
}

/// **城市级拼接大图**（返回裸 SVG 文本，与 `world_map_render` 一致）
///
/// 把 `ad` 这个市下辖的每个区县各自的**街区图**，按经纬度排布拼成一张连续大图。
/// 与 `world_map_geo_svg`（行政区划总览）是两件事：那边画区县轮廓，这边每块都是街区图。
///
/// 参数（JS 侧多词写驼峰）：
///   · `ad`        市 adcode（如 `"440100"`），必填
///   · `style`     `gaode` / `dark` / `water`，默认 `gaode`
///   · `maxTiles`  一页最多几块，默认 9（3×3），硬上限 36；超出的**分页**（不是丢弃）
///   · `page`      第几页（1 起；`pages > 1` 时前端可翻页）
///   · `width`/`height` 大图尺寸上限，默认 2000×2000（夹在 600..2400）
///   · `detail`    是否挂街道细节（人行道/斑马线/树/车位/路灯），默认 **false**（体积约 ×3）
///   · `hi`        要高精度的块，逗号分隔的 adcode（如 `"440104"`）→ 该块网格 ×1.5 且挂细节
///   · `grid`      强制统一网格数（不传 = 由块像素反推）
///   · `seed`      强制随机种子（不传 = 按 adcode 派生，保证同块同图）
///   · `zoom`      1/2/3，写进根元素 `data-zoom`（CSS 控制细节显隐），默认 3
///   · `maxMb`     体积上限（MB），超出自动降级重拼，默认 4
///   · `refresh`   默认 false；true = 连每块缓存都不用，全部重画
///
/// 返回：SVG 文本。产物同时落盘并登记进地图库（kind = `bigmap_stitch`，块级 = `bigmap_tile`）。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn world_map_bigmap_svg(
    app: AppHandle,
    ad: String,
    style: Option<String>,
    max_tiles: Option<usize>,
    width: Option<f64>,
    height: Option<f64>,
    page: Option<usize>,
    detail: Option<bool>,
    hi: Option<String>,
    grid: Option<i32>,
    seed: Option<u64>,
    zoom: Option<i32>,
    max_mb: Option<f64>,
    refresh: Option<bool>,
) -> Result<String, String> {
    let opts = make_opts(style, max_tiles, width, height, page, detail, hi, grid, seed, zoom, max_mb);
    let out = build(&app, &ad, opts, refresh.unwrap_or(false)).await?;
    Ok(out.svg)
}

/// 同上，但返回**计划与统计**（不出图）：先看这一页有哪几块、格距多少、缓存键是什么。
///
/// 典型用法（两遍法）：先 `plan` 看 `pages`/`tiles`，再决定给哪几块开 `hi`。
/// 返回 `{ ok, key, file, bytes, ms, cached, degraded, tilesRendered, tilesReused, plan }`，
/// `plan` 结构见 `stitch::Plan::to_json`（含每块的 adcode/名字/行列/坐标/网格/是否高精）。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn world_map_bigmap_plan(
    app: AppHandle,
    ad: String,
    style: Option<String>,
    max_tiles: Option<usize>,
    width: Option<f64>,
    height: Option<f64>,
    page: Option<usize>,
    detail: Option<bool>,
    hi: Option<String>,
    grid: Option<i32>,
    seed: Option<u64>,
    zoom: Option<i32>,
    max_mb: Option<f64>,
    refresh: Option<bool>,
) -> Result<Value, String> {
    let opts = make_opts(style, max_tiles, width, height, page, detail, hi, grid, seed, zoom, max_mb);
    let out = build(&app, &ad, opts, refresh.unwrap_or(false)).await?;
    Ok(json!({
        "ok": true,
        "key": out.cache_key,
        "file": out.rel_path,
        "bytes": out.bytes,
        "ms": (out.ms * 10.0).round() / 10.0,
        "cached": out.cached,
        "degraded": out.degraded,
        "tilesRendered": out.tiles_rendered,
        "tilesReused": out.tiles_reused,
        "plan": out.plan.to_json(),
        "source": "rust",
    }))
}
