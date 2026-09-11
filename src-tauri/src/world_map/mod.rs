//! 世界地图 Rust 后端（T6-5）
//!
//! 分层：coord（坐标/几何） · geo（地理数据与区块） · 后续再有 render/facilities/transport/schedule
//! 前端通过 `world_map_*` 命令调用；Python 侧车仍可并存（前端 `USE_RUST` 决定走哪边）。
pub mod coord;
pub mod geo;
pub mod render;

use serde_json::Value;
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
    let mut out = geo::build_blocks(
        &src,
        &main_ad,
        remotes,
        limit.unwrap_or(6),
        80.0,
    )
    .await?;
    // 风格/缩放回填到图片地址里
    let style = style.unwrap_or_else(|| "gaode".to_string());
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
    render::render_svg(
        &fc,
        &style.unwrap_or_else(|| "gaode".to_string()),
        width.unwrap_or(1200.0),
        height.unwrap_or(900.0),
        pad.unwrap_or(30.0),
        labels.unwrap_or(true),
    )
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
