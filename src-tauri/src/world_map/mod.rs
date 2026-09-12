//! 世界地图 Rust 后端（T6-5）
//!
//! 模块本体来自独立真源工程 `~/rikka/Dsh-SYuki/world_map_rs/src/`（纯 Rust、无 Tauri 依赖，
//! 在那边能单独编译并跑单元测试）。搬进来时只做了两件事：
//!   1. 模块之间的跨模块引用加 `world_map::` 前缀（`crate::coord` → `crate::world_map::coord`）
//!   2. 真源 `main.rs` 的 axum 路由**没有**搬 —— 那是独立调试 HTTP 服务的入口，不是 Tauri 的东西
//!
//! 分层：coord（坐标/几何） · geo（地理数据与区块） · sketch/details/render（小区生成与渲染）
//!      · render_geo（行政区划渲染） · stats（统计） · stream（流式生成） · maplib（地图库）
//!      · osm（Overpass 真实地物） · transport（交通） · schedule（日程 → 地图位置）
//! 前端通过 `world_map_*` 命令调用；Python 侧车仍可并存（前端 `USE_RUST` 决定走哪边）。
pub mod coord;
pub mod details;
pub mod facilities;
pub mod geo;
pub mod maplib;
pub mod osm;
pub mod render;
pub mod render_geo;
pub mod schedule;
pub mod sketch;
pub mod stats;
pub mod stream;
pub mod transport;

use chrono::{Datelike, Timelike};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::AppHandle;

/// 地理数据缓存目录（应用数据目录下）
pub fn cache_dir(_app: &AppHandle) -> PathBuf {
    crate::api::data_dir().join("world_map").join("geo")
}

/// 额外的只读数据目录（开发期沿用 Python 项目已下好的缓存，避免重复下载）
fn extra_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let h = PathBuf::from(home);
        for rel in [
            "rikka/Dsh-SYuki/world_map/worlddata/cn",
            "Dsh-SYuki/world_map/worlddata/cn",
        ] {
            let p = h.join(rel);
            if p.is_dir() {
                v.push(p);
            }
        }
    }
    v
}

/// 建一个地理数据源：主缓存目录 + 额外只读目录（后者会按需复制进主缓存）
fn make_source(app: &AppHandle) -> geo::GeoSource {
    let dir = cache_dir(app);
    let _ = std::fs::create_dir_all(&dir);
    // 把只读目录里的缓存预热进来（已存在的不覆盖）
    for src in extra_dirs() {
        if let Ok(rd) = std::fs::read_dir(&src) {
            for e in rd.flatten() {
                let name = e.file_name();
                let dst = dir.join(&name);
                if !dst.exists() {
                    let _ = std::fs::copy(e.path(), &dst);
                }
            }
        }
    }
    geo::GeoSource::new(dir)
}

/// 世界地图数据根目录：`geo/`（geojson 缓存）、`maplib/`、`osm/`、`events.json` 都在它下面。
///
/// 注意：`docs/world-map/07` 里规划的是 `<data_dir>/game_data/world_map/`，
/// 而这里（沿用 T6-5 早期实现 + `events.json` 的落盘位置）是 `<data_dir>/world_map/`。
/// 新模块跟着**已有代码**走，避免同一个功能出现两个数据根。
fn world_dir(_app: &AppHandle) -> PathBuf {
    crate::api::data_dir().join("world_map")
}

/// 地图库根目录（索引 index.json + 布局 layouts/*.json）。
/// `WM_MAPLIB_DIR` 可覆盖：测试/自检必须走隔离目录，绝不能碰真实地图库。
fn maplib_root(app: &AppHandle) -> PathBuf {
    if let Some(p) = std::env::var_os("WM_MAPLIB_DIR") {
        return PathBuf::from(p);
    }
    world_dir(app).join("maplib")
}

/// Overpass（OSM）缓存目录。`WM_OSM_DIR` 可覆盖，同样是为了测试隔离。
fn osm_dir(app: &AppHandle) -> PathBuf {
    if let Some(p) = std::env::var_os("WM_OSM_DIR") {
        return PathBuf::from(p);
    }
    world_dir(app).join("osm")
}

/// 前端可能直接给对象、也可能给 JSON 字符串（invoke 的参数有时是序列化过的），两种都认。
fn normalize_layout(v: Option<Value>) -> Option<Value> {
    match v? {
        Value::String(s) => serde_json::from_str::<Value>(&s).ok(),
        Value::Null => None,
        other => Some(other),
    }
}

/// 取一份可直接渲染/统计的 layout（已经挂好街道细节），三种来源按优先级：
/// `layout` 参数 → 地图库布局缓存 `key` → 本地规则草图 `area/size/seed`。
fn resolve_layout(
    app: &AppHandle,
    layout: Option<Value>,
    key: Option<String>,
    area: Option<String>,
    size: Option<i32>,
    seed: Option<u64>,
) -> Result<Value, String> {
    let mut lay = if let Some(v) = normalize_layout(layout) {
        v
    } else if let Some(k) = key.filter(|k| !k.trim().is_empty()) {
        let lib = maplib::MapLib::new(maplib_root(app));
        lib.get_layout(&k)
            .ok_or_else(|| format!("地图库里没有布局缓存：{k}"))?
    } else {
        sketch::make_sketch(
            area.as_deref().unwrap_or("广州市·越秀区"),
            size.unwrap_or(20),
            seed,
            None,
        )
    };
    if !lay.is_object() {
        return Err("layout 必须是一个 JSON 对象".to_string());
    }
    details::enrich(&mut lay);
    Ok(lay)
}

/// 把 LingChat 的真实数据目录告诉 `schedule` 模块。
///
/// `schedule.rs` 的数据目录发现逻辑是「按 `$HOME/...` 候选路径猜」（在真源工程里够用），
/// 而 Tauri 应用的真实数据目录由平台决定（Android 上是应用私有目录），猜不到。
/// 它支持 `WM_LINGCHAT_DATA` 显式指定，所以这里把已知的正确路径喂进去；
/// 用户/测试已经设过就绝不覆盖（测试要靠它锁定数据源）。
fn bridge_lingchat_data_dir() {
    if std::env::var_os("WM_LINGCHAT_DATA").is_some() {
        return;
    }
    let d = crate::api::data_dir();
    if d.join("game_data").is_dir() {
        std::env::set_var("WM_LINGCHAT_DATA", &d);
    }
}

/// 从 `{lat, lng}` / `{lat, lon}` / `[lng, lat]` 里取一个分量（transport 参数的兼容层）
fn point_part(p: &Value, key: &str) -> Option<f64> {
    let names: &[&str] = if key == "lat" {
        &["lat", "latitude"]
    } else {
        &["lng", "lon", "longitude"]
    };
    if let Some(o) = p.as_object() {
        for n in names {
            if let Some(v) = o.get(*n).and_then(|x| x.as_f64()) {
                return Some(v);
            }
        }
    }
    if let Some(a) = p.as_array() {
        let idx = if key == "lat" { 1 } else { 0 };
        return a.get(idx).and_then(|x| x.as_f64());
    }
    None
}

/// 时段（与 Python 侧 `/api/time` 的 period 字段同一张表）
fn period_of(h: i32) -> &'static str {
    match h {
        5..=7 => "dawn",
        8..=10 => "morning",
        11..=13 => "noon",
        14..=16 => "afternoon",
        17..=18 => "dusk",
        19..=22 => "evening",
        _ => "night",
    }
}

/// 主块 + 远处块（T1-3）
#[tauri::command]
pub async fn world_map_blocks(
    app: AppHandle,
    ad: Option<String>,
    remote: Option<String>,
    style: Option<String>,
    scale: Option<u32>,
    limit: Option<usize>,
) -> Result<Value, String> {
    let src = make_source(&app);
    let main_ad = ad.unwrap_or_else(|| "440100".to_string());
    let remotes = remote
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|v: &Vec<String>| !v.is_empty());
    // 风格要在调用前定下来：新版 build_blocks 会把它写进 img/img_url
    let style = style.unwrap_or_else(|| "gaode".to_string());
    let mut out = geo::build_blocks(
        &src,
        &main_ad,
        remotes,
        limit.unwrap_or(6),
        80.0,
        // 新版 build_blocks 多了一个 style 参数（它会把 style 写进 img/img_url）
        &style,
    )
    .await?;
    // 风格/缩放回填到图片地址里（scale 只有这里知道，所以要覆盖一次）
    let scale = scale.unwrap_or(1).clamp(1, 3);
    if let Some(m) = out.get_mut("main") {
        if let Some(o) = m.as_object_mut() {
            let adc = o
                .get("adcode")
                .and_then(|v| v.as_str())
                .unwrap_or(&main_ad)
                .to_string();
            o.insert(
                "img".into(),
                Value::String(format!("/api/bigmap?ad={adc}&style={style}&scale={scale}")),
            );
            o.insert(
                "img_url".into(),
                Value::String(format!("/api/bigmap_img?ad={adc}&style={style}&scale={scale}")),
            );
            o.insert("style".into(), Value::String(style.clone()));
        }
    }
    if let Some(rs) = out.get_mut("remotes").and_then(|v| v.as_array_mut()) {
        for r in rs.iter_mut() {
            if let Some(o) = r.as_object_mut() {
                let adc = o.get("adcode").and_then(|v| v.as_str()).unwrap_or("").to_string();
                o.insert(
                    "img".into(),
                    Value::String(format!("/api/map?ad={adc}&style={style}")),
                );
            }
        }
    }
    Ok(out)
}

/// 按坐标定主块（用于「按定位」）
#[tauri::command]
pub async fn world_map_blocks_at(
    app: AppHandle,
    lat: f64,
    lng: f64,
    style: Option<String>,
    limit: Option<usize>,
) -> Result<Value, String> {
    let src = make_source(&app);
    let (ad, _d) = geo::nearest_cached(&src, lng, lat)
        .ok_or_else(|| "本地地理缓存里没有覆盖该坐标的区域".to_string())?;
    world_map_blocks(app, Some(ad), None, style, Some(1), limit).await
}

/// 地理数据状态（调试/自检用）
#[tauri::command]
pub async fn world_map_geo_status(app: AppHandle) -> Result<Value, String> {
    let src = make_source(&app);
    let codes = src.cached_adcodes();
    Ok(serde_json::json!({
        "cacheDir": cache_dir(&app).to_string_lossy(),
        "cachedCount": codes.len(),
        "cached": codes,
        "extraDirs": extra_dirs().iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>(),
        "source": "rust",
    }))
}

/// 坐标自检（前端/CI 可一键验证几何正确性）
#[tauri::command]
pub async fn world_map_coord_selftest() -> Result<Value, String> {
    let mut checks: Vec<Value> = Vec::new();
    let mut pass = 0usize;
    let mut fail = 0usize;
    let mut check = |name: &str, ok: bool, detail: String| {
        if ok {
            pass += 1;
        } else {
            fail += 1;
        }
        checks.push(serde_json::json!({ "name": name, "ok": ok, "detail": detail }));
    };

    // 往返
    let (lng, lat) = (113.264, 23.129);
    let (x, y) = coord::lng_lat_to_world(lng, lat);
    let (lng2, lat2) = coord::world_to_lng_lat(x, y);
    check(
        "经纬度往返",
        (lng - lng2).abs() < 1e-9 && (lat - lat2).abs() < 1e-9,
        format!("{lng},{lat} → {lng2},{lat2}"),
    );

    // 网格往返
    let (gx, gy) = coord::world_to_grid(x, y, lng, lat, 20.0);
    check("网格往返", gx.abs() < 1e-6 && gy.abs() < 1e-6, format!("{gx},{gy}"));

    // 距离
    let d = coord::haversine_m((113.2644, 23.1291), (114.0579, 22.5431));
    check(
        "广州→深圳约 100km",
        d > 90_000.0 && d < 110_000.0,
        format!("{:.1} km", d / 1000.0),
    );

    // 方位
    let (nx, ny) = coord::edge_point(0.0, 4.0 / 3.0, 0.06);
    check(
        "正北落在顶部中间",
        (nx - 0.5).abs() < 0.02 && ny < 0.1,
        format!("{nx:.3},{ny:.3}"),
    );
    let (ex, ey) = coord::edge_point(90.0, 4.0 / 3.0, 0.06);
    check(
        "正东落在右侧中间",
        ex > 0.9 && (ey - 0.5).abs() < 0.02,
        format!("{ex:.3},{ey:.3}"),
    );

    // 点在多边形内
    let sq = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)];
    check(
        "射线法判点在多边形内",
        coord::point_in_poly((1.0, 1.0), &sq) && !coord::point_in_poly((3.0, 1.0), &sq),
        "正方形内外各一例".into(),
    );

    Ok(serde_json::json!({
        "pass": pass, "fail": fail, "total": checks.len(),
        "checks": checks, "source": "rust",
    }))
}

/// 渲染某区域为 SVG（T1-2 的 Rust 版）
///
/// 返回 SVG 文本，前端可直接 `<img src="data:image/svg+xml;utf8,...">` 或用 innerHTML 内联。
///
/// 实现已换成真源工程里更完整的 `render_geo.rs`（顶点抽稀、按行政级别配色、区划可点击），
/// 参数与返回值保持不变（`world_map_render_svg(fc, style, w, h, pad, labels)` 的老实现对得上）。
#[tauri::command]
pub async fn world_map_render_svg(
    app: AppHandle,
    ad: String,
    style: Option<String>,
    width: Option<f64>,
    height: Option<f64>,
    pad: Option<f64>,
    labels: Option<bool>,
) -> Result<String, String> {
    let src = make_source(&app);
    let fc = src.fetch(&ad).await?;
    let opts = render_geo::GeoOpts {
        level: geo::level_of(&ad).to_string(),
        style: style.unwrap_or_else(|| "gaode".to_string()),
        width: width.unwrap_or(1200.0),
        height: height.unwrap_or(900.0),
        pad: pad.unwrap_or(30.0),
        labels: labels.unwrap_or(true),
        ..Default::default()
    };
    render_geo::render_geo_svg(&fc, &opts)
}

/// 世界事件落盘（T6-3）：地图里的重大事件写到 app 数据目录，
/// LingChat 的主动系统可以读它来决定「要不要拿这件事主动搭话」。
#[tauri::command]
pub async fn world_map_push_events(app: AppHandle, events: Vec<Value>) -> Result<usize, String> {
    if events.is_empty() {
        return Ok(0);
    }
    let p = crate::api::data_dir().join("world_map").join("events.json");
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut all: Vec<Value> = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
        .unwrap_or_default();
    all.extend(events);
    if all.len() > 200 {
        let drop_n = all.len() - 200;
        all.drain(0..drop_n);
    }
    std::fs::write(&p, serde_json::to_string(&all).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(all.len())
}

/// 读最近的世界事件（主动系统 / 调试用）
#[tauri::command]
pub async fn world_map_recent_events(app: AppHandle, limit: Option<usize>) -> Result<Vec<Value>, String> {
    let p = crate::api::data_dir().join("world_map").join("events.json");
    let all: Vec<Value> = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
        .unwrap_or_default();
    let n = limit.unwrap_or(20).min(all.len());
    Ok(all[all.len() - n..].to_vec())
}

// ═══════════════════════════════════════════════════════════════════
//  真源模块搬入后新暴露的命令
//
//  参数命名：Rust 侧是 snake_case，Tauri 按惯例给 JS 转成 camelCase ——
//  单词参数两侧同名（ad / style / limit / now / area / layout / key …），
//  多词参数 JS 要写驼峰：`max_mb` → `maxMb`、`dry_run` → `dryRun`、
//  `from_lng` → `fromLng`。每个命令的注释里都标了。
// ═══════════════════════════════════════════════════════════════════

/// 小区 SVG（`sketch` + `details` + `render`）
///
/// 取 layout 的优先级：`layout`（前端给，通常是 AI 流式生成的结果）→ `key`（地图库布局缓存）
/// → `area`+`size`+`seed`（本地规则草图，确定性随机）。
/// 返回**裸 SVG 文本**（与老 `world_map_render_svg`、以及 HTTP 版 `/api/render` 一致）。
///
/// `mode` 支持 `2d` / `3d`；`charts=true` 会画角落的数据卡片；`zoom`/`layers` 只影响
/// SVG 里的类名与 `data-zoom`，显隐交给前端 CSS。
#[tauri::command]
pub async fn world_map_render(
    app: AppHandle,
    layout: Option<Value>,
    key: Option<String>,
    area: Option<String>,
    size: Option<i32>,
    seed: Option<u64>,
    style: Option<String>,
    mode: Option<String>,
    zoom: Option<i32>,
    charts: Option<bool>,
    animate: Option<bool>,
    layers: Option<bool>,
    width: Option<f64>,
    height: Option<f64>,
    pad: Option<f64>,
) -> Result<String, String> {
    let lay = resolve_layout(&app, layout, key, area, size, seed)?;
    let opts = render::Opts {
        style: style.unwrap_or_else(|| "gaode".to_string()),
        width: width.unwrap_or(900.0),
        height: height.unwrap_or(900.0),
        pad: pad.unwrap_or(48.0),
        zoom: zoom.unwrap_or(3),
        animate: animate.unwrap_or(true),
        layers: layers.unwrap_or(true),
        mode: mode.unwrap_or_else(|| "2d".to_string()),
        charts: charts.unwrap_or(false),
    };
    Ok(render::render_svg(&lay, &opts))
}

/// 行政区划 SVG（`render_geo`）：全国 (`100000`) / 省 / 市 / 区县，区划带 adcode 可点击下钻
///
/// 与老命令 `world_map_render_svg` 是同一个渲染器，只是这里多暴露了 `zoom`/`dots`/`stats`。
/// `stats=true` 会在图上标注顶点抽稀统计（开发用）。
#[tauri::command]
pub async fn world_map_geo_svg(
    app: AppHandle,
    ad: Option<String>,
    style: Option<String>,
    width: Option<f64>,
    height: Option<f64>,
    pad: Option<f64>,
    zoom: Option<i32>,
    labels: Option<bool>,
    dots: Option<bool>,
    stats: Option<bool>,
) -> Result<String, String> {
    let ad = ad.unwrap_or_else(|| "100000".to_string());
    let src = make_source(&app);
    let fc = src.fetch(&ad).await?;
    let opts = render_geo::GeoOpts {
        level: geo::level_of(&ad).to_string(),
        style: style.unwrap_or_else(|| "gaode".to_string()),
        width: width.unwrap_or(1000.0),
        height: height.unwrap_or(760.0),
        pad: pad.unwrap_or(26.0),
        zoom: zoom.unwrap_or(2),
        labels: labels.unwrap_or(true),
        dots: dots.unwrap_or(true),
        show_stats: stats.unwrap_or(false),
    };
    render_geo::render_geo_svg(&fc, &opts)
}

/// 小区统计指标（`stats.rs` + `details.rs`）
///
/// 取 layout 的方式与 `world_map_render` 相同（`layout` / `key` / `area`+`size`+`seed`）。
/// 返回 `{ detail, stats, series }`：
///   · `detail` = 街道细节计数（树/车位/路灯/人行道…，来自 `details::stats`）
///   · `stats`  = 建筑/道路/公园/水域的数量、面积、层数、人口估算（`stats::compute`）
///   · `series` = 按建筑类型聚合的序列，方便前端直接画饼图/柱图
#[tauri::command]
pub async fn world_map_stats(
    app: AppHandle,
    layout: Option<Value>,
    key: Option<String>,
    area: Option<String>,
    size: Option<i32>,
    seed: Option<u64>,
) -> Result<Value, String> {
    let lay = resolve_layout(&app, layout, key, area, size, seed)?;
    let detail = details::stats(&lay);
    let computed = stats::compute(&lay);
    let series = stats::type_series(&computed)
        .into_iter()
        .map(|(t, zh, count, area_m2)| {
            json!({ "type": t, "typeZh": zh, "count": count, "areaM2": area_m2 })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "detail": detail,
        "stats": computed,
        "series": series,
        "source": "rust",
    }))
}

/// 地图库统计（条数/体积/按类型与城市分布）
///
/// 根目录：`$WM_MAPLIB_DIR`（测试隔离用）→ 否则 `<data_dir>/world_map/maplib`。
#[tauri::command]
pub async fn world_map_maplib_stats(app: AppHandle) -> Result<Value, String> {
    let lib = maplib::MapLib::new(maplib_root(&app));
    Ok(lib.stats())
}

/// 地图库列表（可按 `kind` / `ad` 过滤，`sort` 支持 `recent` 等，见 `maplib.rs`）
///
/// 返回 `{ stats, entries }`，与 HTTP 版 `/api/maplib/list` 一致。
#[tauri::command]
pub async fn world_map_maplib_list(
    app: AppHandle,
    kind: Option<String>,
    ad: Option<String>,
    limit: Option<usize>,
    sort: Option<String>,
) -> Result<Value, String> {
    let lib = maplib::MapLib::new(maplib_root(&app));
    let kind = kind.filter(|s| !s.trim().is_empty());
    let ad = ad.filter(|s| !s.trim().is_empty());
    let sort = sort.filter(|s| !s.trim().is_empty());
    let items = lib.list(
        kind.as_deref(),
        ad.as_deref(),
        limit,
        sort.as_deref().unwrap_or("recent"),
    );
    Ok(json!({ "stats": lib.stats(), "entries": items }))
}

/// 地图库容量清理（LRU）。**默认干跑**，只有显式 `dryRun=false`（或 `dry=false`）才真删。
///
/// 为什么默认干跑：Python 侧在真实删除路径上误删过 119 张地图缓存，
/// 这个开关是那次事故换来的纪律 —— 想真删必须自己写清楚。
/// `maxMb` 不传就用地图库自己的上限（默认 300MB）。
#[tauri::command]
pub async fn world_map_maplib_cleanup(
    app: AppHandle,
    max_mb: Option<f64>,
    dry_run: Option<bool>,
    dry: Option<bool>,
) -> Result<Value, String> {
    let lib = maplib::MapLib::new(maplib_root(&app));
    let max_bytes = max_mb.map(|m| (m * 1048576.0) as u64);
    let dry_run = dry_run.or(dry).unwrap_or(true);
    Ok(lib.enforce_limit(max_bytes, dry_run))
}

/// 日程 → 角色此刻在做什么、该出现在地图的哪个位置（`schedule.rs`）
///
/// `now` 可以是 `"HH:MM"`、分钟数，或不传（用设备本地时间）。
/// `area` 只影响生成的地名文案；`facilities` 传了就能落到具体设施点
/// （不传就退化为「只有类型」，等 facilities 模块落地后由调用方接上）。
///
/// 数据源：LingChat 的 `<data_dir>/game_data/schedules.json`
/// （已存在的 `characters/settings.yml` 也会读，读不到就用内置默认日程，保证地图不空白）。
#[tauri::command]
pub async fn world_map_schedule(
    now: Option<Value>,
    area: Option<String>,
    facilities: Option<Vec<Value>>,
) -> Result<Value, String> {
    bridge_lingchat_data_dir();
    let now_min = match now {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(Value::String(s)) if s.trim().is_empty() => None,
        Some(other) => schedule::parse_time(&other),
    };
    Ok(schedule::payload(
        now_min,
        facilities.as_deref(),
        area.as_deref(),
    ))
}

/// 两点之间的交通方案（`transport.rs`，9 种交通工具 + 接驳）
///
/// 坐标两种给法都行（前端 `worldMap.ts` 用的是前一种）：
///   · `from`/`to` = `{lat, lng}`（也认 `{lat, lon}` 与 `[lng, lat]`）
///   · 或者平铺的 `fromLng`/`fromLat`/`toLng`/`toLat`（对应 HTTP 版 `?from_lng=…`）
/// `prefer` 是交通方式偏好（bus/subway/train/…，别名见 `transport.rs` 的 ALIASES）。
/// 返回 `{ ok, route, options, modes }`。
#[tauri::command]
pub async fn world_map_transport_plan(
    from: Option<Value>,
    to: Option<Value>,
    prefer: Option<String>,
    from_lng: Option<f64>,
    from_lat: Option<f64>,
    to_lng: Option<f64>,
    to_lat: Option<f64>,
    water: Option<bool>,
    urban: Option<bool>,
) -> Result<Value, String> {
    let mut q = serde_json::Map::new();
    let pick = |flat: Option<f64>, p: &Option<Value>, k: &str| -> Option<f64> {
        flat.or_else(|| p.as_ref().and_then(|v| point_part(v, k)))
    };
    let flng = pick(from_lng, &from, "lng");
    let flat = pick(from_lat, &from, "lat");
    let tlng = pick(to_lng, &to, "lng");
    let tlat = pick(to_lat, &to, "lat");
    if let Some(v) = flng {
        q.insert("from_lng".into(), json!(v));
    }
    if let Some(v) = flat {
        q.insert("from_lat".into(), json!(v));
    }
    if let Some(v) = tlng {
        q.insert("to_lng".into(), json!(v));
    }
    if let Some(v) = tlat {
        q.insert("to_lat".into(), json!(v));
    }
    if let Some(v) = prefer.filter(|s| !s.trim().is_empty()) {
        q.insert("prefer".into(), json!(v));
    }
    if let Some(v) = water {
        q.insert("water".into(), json!(v));
    }
    if let Some(v) = urban {
        q.insert("urban".into(), json!(v));
    }
    Ok(transport::api_plan(&Value::Object(q)))
}

/// 区域 OSM（Overpass）摘要（`osm.rs`）
///
/// **默认只读缓存**（按 200m 网格缓存，key = `grid_key(lat,lng,radius)`）：
/// 生成路径绝不能被 Overpass 的几十秒拖住，想真去抓必须显式 `force=true`。
/// `text=true` 时额外返回给 LLM 看的一段中文描述（`osm::describe_for_llm`）。
/// 返回 `{ ok, key, cached, count, summary, text, meta }`；
/// 没缓存且没 `force` 时 `ok=false` + `hint`，让调用方降级为纯 LLM 生成。
#[tauri::command]
pub async fn world_map_osm_summary(
    app: AppHandle,
    lat: f64,
    lng: f64,
    radius: Option<f64>,
    force: Option<bool>,
    kinds: Option<Vec<String>>,
    text: Option<bool>,
) -> Result<Value, String> {
    let dir = osm_dir(&app);
    let radius = radius.unwrap_or(300.0);
    let force = force.unwrap_or(false);
    let key = osm::grid_key(lat, lng, radius);
    let mut cached = false;
    let mut data = None;
    if !force {
        data = osm::load_cached(&dir, &key);
        cached = data.is_some();
    }
    if data.is_none() && force {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(25))
            .build()
            .map_err(|e| format!("HTTP 客户端创建失败: {e}"))?;
        data = osm::fetch_area(&client, &dir, lat, lng, radius, kinds, true).await;
    }
    match data {
        Some(v) => {
            let count = v
                .get("elements")
                .and_then(|e| e.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Ok(json!({
                "ok": true,
                "key": key,
                "cached": cached,
                "count": count,
                "summary": osm::summarize(&v),
                "text": if text.unwrap_or(false) {
                    Value::String(osm::describe_for_llm(&v))
                } else {
                    Value::Null
                },
                "meta": v.get("_meta").cloned().unwrap_or(Value::Null),
                "source": "rust",
            }))
        }
        None => Ok(json!({
            "ok": false,
            "key": key,
            "cached": false,
            "error": "本地没有这份 OSM 缓存",
            "hint": "要真去抓请传 force=true（走 Overpass，可能几十秒）",
            "source": "rust",
        })),
    }
}

/// 设备本地时间 + 时段（前端 `worldMapApi.time()` 调的就是它）
///
/// LingChat 用真实时间、没有游戏内时间，所以这里取的是**设备时区**的本地时间
/// （`chrono::Local`，与 `schedule.rs` 内部取的是同一份设备本地时间）。
#[tauri::command]
pub async fn world_map_time() -> Result<Value, String> {
    let now = chrono::Local::now();
    let h = now.hour() as i32;
    let weekday = now.weekday().num_days_from_monday();
    let names = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    Ok(json!({
        "iso": now.format("%Y-%m-%dT%H:%M:%S").to_string(),
        "date": now.format("%Y-%m-%d").to_string(),
        "time": now.format("%H:%M:%S").to_string(),
        "hour": h,
        "minute": now.minute(),
        "weekday": weekday,
        "weekday_name": names[weekday as usize],
        "period": period_of(h),
        "is_day": (6..18).contains(&h),
        "timestamp": now.timestamp(),
        "source": "rust",
    }))
}
