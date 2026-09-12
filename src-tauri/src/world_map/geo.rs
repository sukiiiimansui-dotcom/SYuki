//! 地理数据与区块（移植自 Python `blocks.py` / `gen_hier.py` + LingChat 版 `geo.rs`）
//!
//! 与 LingChat 版的差别：**去掉 `AppHandle` 依赖**，改成显式传入缓存目录，
//! 这样同一个模块既能在独立验证项目里跑，也能原样搬进 Tauri 侧。
//!
//! 数据来源：阿里云 DataV GeoAtlas
//!   · 有子级的区域 `{adcode}_full.json`，无子级的区县 `{adcode}.json`
//! 缓存优先，缺失才联网（离线时只读缓存）。
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::world_map::coord::{self, BBox};

const DATA_V: &str = "https://geo.datav.aliyun.com/areas_v3/bound";

/// 一个行政区划要素
#[derive(Debug, Clone)]
pub struct Feature {
    pub name: String,
    pub adcode: String,
    pub points: Vec<(f64, f64)>,
    pub bbox: Option<BBox>,
}

impl Feature {
    pub fn lng_lat_range(&self) -> (f64, f64, f64, f64) {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &(lng, lat) in &self.points {
            if lng < x0 { x0 = lng; }
            if lat < y0 { y0 = lat; }
            if lng > x1 { x1 = lng; }
            if lat > y1 { y1 = lat; }
        }
        (x0, y0, x1, y1)
    }
    pub fn center(&self) -> (f64, f64) {
        let (x0, y0, x1, y1) = self.lng_lat_range();
        ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
    }
}

/// 递归收集 GeoJSON 坐标数组里的点
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

/// adcode → 行政级别（决定渲染参数）
pub fn level_of(adcode: &str) -> &'static str {
    let a = adcode.trim();
    if a.is_empty() || a == "100000" || a == "0" {
        "country"
    } else if a.ends_with("0000") {
        "province"
    } else if a.ends_with("00") {
        "city"
    } else {
        "district"
    }
}

/// 从 FeatureCollection 提取要素（跳过没名字的、以及自身）
pub fn features(fc: &Value, self_adcode: Option<&str>) -> Vec<Feature> {
    let mut out = Vec::new();
    let empty = vec![];
    let list = fc.get("features").and_then(|v| v.as_array()).unwrap_or(&empty);
    for f in list {
        let p = match f.get("properties") {
            Some(p) => p,
            None => continue,
        };
        let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let adcode = match p.get("adcode") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        if let Some(sa) = self_adcode {
            if adcode == sa {
                continue;
            }
        }
        let mut pts = Vec::new();
        if let Some(c) = f.get("geometry").and_then(|g| g.get("coordinates")) {
            walk_coords(c, &mut pts);
        }
        let bbox = coord::bbox_of_lng_lat(&pts);
        out.push(Feature { name, adcode, points: pts, bbox });
    }
    out
}

/// 地理数据源：本地缓存优先 + 远程兜底
pub struct GeoSource {
    pub cache: PathBuf,
    client: Option<reqwest::Client>,
}

impl GeoSource {
    pub fn new(cache: impl AsRef<Path>) -> Self {
        let cache = cache.as_ref().to_path_buf();
        let _ = std::fs::create_dir_all(&cache);
        // ⚠️ LingChat 专有改动（同步真源时注意保留）：
        // 这里拉的是 https://geo.datav.aliyun.com/...，而 reqwest 0.13 默认用
        // rustls-platform-verifier，Android 上不预先初始化会直接 panic ——
        // 见 src/utils/tls.rs 的模块说明。真源工程（Termux/桌面调试服务）用裸 builder 没问题，
        // 但搬进 Tauri 必须走 build_tls_config()。
        let client = crate::utils::tls::build_tls_config()
            .ok()
            .and_then(|tls| {
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(20))
                    .tls_backend_preconfigured(tls)
                    .build()
                    .ok()
            });
        Self { cache, client }
    }

    /// 纯离线模式（只读缓存，绝不联网）—— 单元测试与断网环境用
    pub fn offline(cache: impl AsRef<Path>) -> Self {
        let cache = cache.as_ref().to_path_buf();
        let _ = std::fs::create_dir_all(&cache);
        Self { cache, client: None }
    }

    pub fn cached_path(&self, adcode: &str) -> PathBuf {
        self.cache.join(format!("{adcode}.json"))
    }

    pub fn load_cached(&self, adcode: &str) -> Option<Value> {
        let raw = std::fs::read_to_string(self.cached_path(adcode)).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// 缓存优先，缺失才下载（先 `_full` 再普通：省市级两者都有，区县级只有普通）
    pub async fn fetch(&self, adcode: &str) -> Result<Value, String> {
        if let Some(v) = self.load_cached(adcode) {
            return Ok(v);
        }
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| format!("{adcode} 无本地缓存，且当前为离线模式"))?;
        let mut last = String::from("未知错误");
        for suffix in ["_full", ""] {
            let url = format!("{DATA_V}/{adcode}{suffix}.json");
            match client.get(&url).send().await {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        last = format!("{url} → HTTP {}", resp.status());
                        continue;
                    }
                    match resp.json::<Value>().await {
                        Ok(v) => {
                            if v.get("features").is_some() {
                                let _ = std::fs::write(
                                    self.cached_path(adcode),
                                    serde_json::to_string(&v).unwrap_or_default(),
                                );
                                return Ok(v);
                            }
                            last = format!("{url} → 不是 FeatureCollection");
                        }
                        Err(e) => last = format!("{url} → 解析失败 {e}"),
                    }
                }
                Err(e) => last = format!("{url} → {e}"),
            }
        }
        Err(format!("无法获取 {adcode} 的地理数据：{last}"))
    }

    /// 本地缓存里已有的 adcode 列表
    pub fn cached_adcodes(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.cache) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if let Some(stem) = name.strip_suffix(".json") {
                    let ad = stem.strip_suffix("_full").unwrap_or(stem);
                    if !ad.is_empty() && ad.chars().all(|c| c.is_ascii_digit()) && ad != "100000" {
                        out.push(ad.to_string());
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// 该区域下辖的子级（用于判断能否继续下钻）
    pub fn children_of(&self, adcode: &str) -> Vec<(String, String)> {
        let fc = match self.load_cached(adcode) {
            Some(v) => v,
            None => return Vec::new(),
        };
        features(&fc, Some(adcode))
            .into_iter()
            .map(|f| (f.name, f.adcode))
            .collect()
    }
}

/// 远处块（给前端的结构）
#[derive(Debug, Clone)]
pub struct RemoteBlock {
    pub adcode: String,
    pub name: String,
    pub center: (f64, f64),
    pub bearing: f64,
    pub dir: String,
    pub distance_km: f64,
    pub edge_x: f64,
    pub edge_y: f64,
    pub radius_km: f64,
}

impl RemoteBlock {
    pub fn to_json(&self, style: &str) -> Value {
        serde_json::json!({
            "adcode": self.adcode, "name": self.name,
            "center": [self.center.0, self.center.1],
            "bearing": (self.bearing * 10.0).round() / 10.0,
            "dir": self.dir,
            "distance_km": (self.distance_km * 10.0).round() / 10.0,
            "edge": { "x": (self.edge_x * 10000.0).round() / 10000.0,
                      "y": (self.edge_y * 10000.0).round() / 10000.0 },
            "in_main": false,
            "radius_km": (self.radius_km * 10.0).round() / 10.0,
            "img": format!("/api/map?ad={}&style={}", self.adcode, style),
            "source": "rust",
        })
    }
}

/// 组装主块 + 远处块（T1-3 的核心逻辑）
///
/// `remote_ads` 为 None 时自动挑选：从本地缓存里按距离由近到远取 limit 个
/// （跳过落在主块范围内的、以及近于 min_km 的）。
pub async fn build_blocks(
    src: &GeoSource,
    main_ad: &str,
    remote_ads: Option<Vec<String>>,
    limit: usize,
    min_km: f64,
    style: &str,
) -> Result<Value, String> {
    let main_fc = src.fetch(main_ad).await?;
    let mfeats = features(&main_fc, Some(main_ad));
    if mfeats.is_empty() {
        return Err(format!("{main_ad} 没有有效区划"));
    }
    let mut all_pts: Vec<(f64, f64)> = Vec::new();
    for f in &mfeats {
        all_pts.extend_from_slice(&f.points);
    }
    let main_bbox = coord::bbox_of_lng_lat(&all_pts).ok_or("主块没有坐标")?;
    let main_center = coord::world_to_lng_lat((main_bbox.0 + main_bbox.2) / 2.0, (main_bbox.1 + main_bbox.3) / 2.0);
    let main_lng = (
        all_pts.iter().map(|p| p.0).fold(f64::MAX, f64::min),
        all_pts.iter().map(|p| p.0).fold(f64::MIN, f64::max),
    );
    let main_lat = (
        all_pts.iter().map(|p| p.1).fold(f64::MAX, f64::min),
        all_pts.iter().map(|p| p.1).fold(f64::MIN, f64::max),
    );
    let diag = coord::haversine_m((main_lng.0, main_lat.0), (main_lng.1, main_lat.1));
    // 主块的名字要问上一级：自己那份 geojson 里装的是下辖区域，不含自己的名字。
    // 查不到就退回 adcode —— 显示 "440100" 也比空白强，但正常路径应该给出「广州市」。
    let main_name = match parent_of(&main_ad) {
        Some(p) => match src.fetch(&p).await {
            Ok(fc) => features(&fc, Some(&p))
                .into_iter()
                .find(|f| f.adcode == main_ad)
                .map(|f| f.name)
                .unwrap_or_default(),
            Err(_) => String::new(),
        },
        None => String::new(),
    };
    let main_json = serde_json::json!({
        "adcode": main_ad,
        "name": if main_name.is_empty() { main_ad.to_string() } else { main_name },
        "center": [main_center.0, main_center.1],
        "bbox_lng": [main_lng.0, main_lng.1],
        "bbox_lat": [main_lat.0, main_lat.1],
        "radius_km": (diag / 2000.0 * 10.0).round() / 10.0,
        "regions": mfeats.len(),
        "img": format!("/api/bigmap?ad={main_ad}&style={style}&scale=1"),
        "img_url": format!("/api/bigmap_img?ad={main_ad}&style={style}&scale=1"),
        "style": style,
        "source": "rust",
    });

    let auto = remote_ads.is_none();
    let cands: Vec<String> = remote_ads.unwrap_or_else(|| {
        // 自动挑选只在**省/市**里选：缓存里还躺着区县级文件（如重庆的武隆区），
        // 把「武隆区 897km」当远处块显示出来毫无意义。
        src.cached_adcodes()
            .into_iter()
            .filter(|ad| ad.ends_with("0000") || ad.ends_with("00"))
            .collect()
    });
    let inside = |lng: f64, lat: f64| -> bool {
        lng >= main_lng.0 && lng <= main_lng.1 && lat >= main_lat.0 && lat <= main_lat.1
    };

    let mut remotes: Vec<RemoteBlock> = Vec::new();
    for ad in cands {
        if ad == main_ad {
            continue;
        }
        // 自动挑选时只认本地缓存，避免联网拉一堆
        let fc = match src.load_cached(&ad) {
            Some(v) => v,
            None => continue,
        };
        // 缓存文件有两种可能：①下辖子区域列表 ②该区域自身的边界。
        // 先按「排除自身」读，空的话说明文件存的就是它自己 → 退一步不过滤。
        let mut feats = features(&fc, Some(&ad));
        if feats.is_empty() {
            feats = features(&fc, None);
        }
        if feats.is_empty() {
            continue;
        }
        let mut pts: Vec<(f64, f64)> = Vec::new();
        for f in &feats {
            pts.extend_from_slice(&f.points);
        }
        let bb = match coord::bbox_of_lng_lat(&pts) {
            Some(b) => b,
            None => continue,
        };
        let c = coord::world_to_lng_lat((bb.0 + bb.2) / 2.0, (bb.1 + bb.3) / 2.0);
        if auto && inside(c.0, c.1) {
            continue;
        }
        let dist = coord::haversine_m(main_center, c);
        if auto && dist < min_km * 1000.0 {
            continue;
        }
        let b = coord::bearing_deg(main_center, c);
        let (ex, ey) = coord::edge_point(b, 4.0 / 3.0, 0.06);
        let (lx0, ly0, lx1, ly1) = (
            pts.iter().map(|p| p.0).fold(f64::MAX, f64::min),
            pts.iter().map(|p| p.1).fold(f64::MAX, f64::min),
            pts.iter().map(|p| p.0).fold(f64::MIN, f64::max),
            pts.iter().map(|p| p.1).fold(f64::MIN, f64::max),
        );
        remotes.push(RemoteBlock {
            adcode: ad.clone(),
            name: feats.first().map(|f| f.name.clone()).unwrap_or(ad.clone()),
            center: c,
            bearing: b,
            dir: coord::compass(b).to_string(),
            distance_km: dist / 1000.0,
            edge_x: ex,
            edge_y: ey,
            radius_km: coord::haversine_m((lx0, ly0), (lx1, ly1)) / 2000.0,
        });
    }
    remotes.sort_by(|a, b| a.distance_km.partial_cmp(&b.distance_km).unwrap_or(std::cmp::Ordering::Equal));
    let total = remotes.len();
    if auto && limit > 0 {
        remotes.truncate(limit);
    }

    Ok(serde_json::json!({
        "ok": true,
        "auto": auto,
        "style": style,
        "main": main_json,
        "remotes": remotes.iter().map(|r| r.to_json(style)).collect::<Vec<_>>(),
        "remoteTotal": total,
        "source": "rust",
    }))
}

/// 6 位 adcode 的上一级（前缀制：44 0000 广东省 → 4401 00 广州市 → 440103 荔湾区）
pub fn parent_of(ad: &str) -> Option<String> {
    if ad.len() != 6 || !ad.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if ad == "100000" {
        return None;
    }
    if ad.ends_with("0000") {
        return Some("100000".to_string());
    }
    if ad.ends_with("00") {
        return Some(format!("{}0000", &ad[..2]));
    }
    Some(format!("{}00", &ad[..4]))
}

/// 找缓存里离给定坐标最近的区域（按定位定主块）
pub fn nearest_cached(src: &GeoSource, lng: f64, lat: f64) -> Option<(String, f64)> {
    let (wx, wy) = coord::lng_lat_to_world(lng, lat);
    let mut best: Option<(String, f64)> = None;
    for ad in src.cached_adcodes() {
        let fc = match src.load_cached(&ad) {
            Some(v) => v,
            None => continue,
        };
        for f in features(&fc, Some(&ad)) {
            // bbox 粗筛
            if let Some(bb) = f.bbox {
                if wx < bb.0 || wx > bb.2 || wy < bb.1 || wy > bb.3 {
                    continue;
                }
            }
            let (x0, y0, x1, y1) = f.lng_lat_range();
            if lng < x0 || lng > x1 || lat < y0 || lat > y1 {
                continue;
            }
            let d = coord::haversine_m(f.center(), (lng, lat));
            if best.as_ref().map(|(_, bd)| d < *bd).unwrap_or(true) {
                // 返回**命中要素**的 adcode，而不是所在文件的 adcode：
                // 深圳的边界只在「广东省」那份文件里，若返回 440000 就会把深圳认成省级。
                let code = if f.adcode.is_empty() { ad.clone() } else { f.adcode.clone() };
                best = Some((code, d));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fc() -> Value {
        json!({
            "type": "FeatureCollection",
            "features": [
                {"type":"Feature","properties":{"name":"甲区","adcode":440103},
                 "geometry":{"type":"Polygon","coordinates":[[[113.20,23.10],[113.25,23.10],[113.25,23.15],[113.20,23.15],[113.20,23.10]]]}},
                {"type":"Feature","properties":{"name":"乙区","adcode":440104},
                 "geometry":{"type":"Polygon","coordinates":[[[113.25,23.10],[113.30,23.10],[113.30,23.15],[113.25,23.15],[113.25,23.10]]]}},
                {"type":"Feature","properties":{"name":"","adcode":440105},
                 "geometry":{"type":"Polygon","coordinates":[[[113.30,23.10],[113.35,23.10],[113.35,23.15],[113.30,23.15],[113.30,23.10]]]}}
            ]
        })
    }

    #[test]
    fn extract_features_skips_unnamed_and_self() {
        let f = features(&fc(), None);
        assert_eq!(f.len(), 2, "没名字的应被跳过");
        assert_eq!(f[0].name, "甲区");
        let f2 = features(&fc(), Some("440103"));
        assert_eq!(f2.len(), 1, "自身应被跳过");
        assert_eq!(f2[0].name, "乙区");
    }

    #[test]
    fn feature_geometry_and_center() {
        let f = features(&fc(), None);
        let (x0, y0, x1, y1) = f[0].lng_lat_range();
        assert!((x0 - 113.20).abs() < 1e-9 && (x1 - 113.25).abs() < 1e-9);
        assert!((y0 - 23.10).abs() < 1e-9 && (y1 - 23.15).abs() < 1e-9);
        let (cx, cy) = f[0].center();
        assert!((cx - 113.225).abs() < 1e-6 && (cy - 23.125).abs() < 1e-6);
        assert!(f[0].bbox.is_some());
    }

    #[test]
    fn level_inference() {
        assert_eq!(level_of("100000"), "country");
        assert_eq!(level_of("440000"), "province");
        assert_eq!(level_of("440100"), "city");
        assert_eq!(level_of("440103"), "district");
        assert_eq!(level_of(""), "country");
    }

    #[test]
    fn nearest_cached_returns_the_matched_feature_not_the_file() {
        // 只放一份「广东省」的文件，里面含深圳市 —— 返回的必须是 440300（要素），
        // 而不是 440000（文件）。否则深圳会被当成省级区划（实际踩过）。
        let dir = std::env::temp_dir().join(format!("wm_near_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let gd = json!({"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"name":"广州市","adcode":440100},
             "geometry":{"type":"Polygon","coordinates":[[[113.2,23.1],[113.3,23.1],[113.3,23.2],[113.2,23.2],[113.2,23.1]]]}},
            {"type":"Feature","properties":{"name":"深圳市","adcode":440300},
             "geometry":{"type":"Polygon","coordinates":[[[114.0,22.5],[114.1,22.5],[114.1,22.6],[114.0,22.6],[114.0,22.5]]]}}
        ]});
        std::fs::write(dir.join("440000.json"), serde_json::to_string(&gd).unwrap()).unwrap();
        let src = GeoSource::offline(&dir);
        let hit = nearest_cached(&src, 114.05, 22.55).expect("深圳该被命中");
        assert_eq!(hit.0, "440300", "应返回命中要素的 adcode");
        let hit2 = nearest_cached(&src, 113.25, 23.15).expect("广州该被命中");
        assert_eq!(hit2.0, "440100");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_source_reads_cache_only() {
        let dir = std::env::temp_dir().join(format!("wm_geo_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("440100.json"), serde_json::to_string(&fc()).unwrap()).unwrap();
        let src = GeoSource::offline(&dir);
        assert!(src.load_cached("440100").is_some());
        assert_eq!(src.cached_adcodes(), vec!["440100".to_string()]);
        let kids = src.children_of("440100");
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0].0, "甲区");
        // 离线时 fetch 未缓存区域应报错而不是挂起
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let err = rt.block_on(src.fetch("999999")).unwrap_err();
        assert!(err.contains("离线"), "应为离线错误，实际: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blocks_auto_pick_from_cache() {
        let dir = std::env::temp_dir().join(format!("wm_blk_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // 主块：广州（113.2~113.3, 23.1~23.15）
        std::fs::write(dir.join("440100.json"), serde_json::to_string(&fc()).unwrap()).unwrap();
        // 远处块：深圳（114.0~114.1, 22.5~22.6），距主块约 100km
        let far = json!({"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"name":"深圳市","adcode":440300},
             "geometry":{"type":"Polygon","coordinates":[[[114.0,22.5],[114.1,22.5],[114.1,22.6],[114.0,22.6],[114.0,22.5]]]}}]});
        std::fs::write(dir.join("440300.json"), serde_json::to_string(&far).unwrap()).unwrap();
        // 近处块：就在主块内部的区域，自动模式应被跳过
        let near = json!({"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"name":"内部区","adcode":440199},
             "geometry":{"type":"Polygon","coordinates":[[[113.22,23.11],[113.23,23.11],[113.23,23.12],[113.22,23.12],[113.22,23.11]]]}}]});
        std::fs::write(dir.join("440199.json"), serde_json::to_string(&near).unwrap()).unwrap();

        let src = GeoSource::offline(&dir);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let out = rt.block_on(build_blocks(&src, "440100", None, 6, 80.0, "gaode")).unwrap();
        assert_eq!(out["ok"], true);
        let remotes = out["remotes"].as_array().unwrap();
        let names: Vec<&str> = remotes.iter().filter_map(|r| r["name"].as_str()).collect();
        assert!(names.contains(&"深圳市"), "远处块应包含深圳，实际 {names:?}");
        assert!(!names.contains(&"内部区"), "主块内部的区域不该被当远处块");
        let d = remotes[0]["distance_km"].as_f64().unwrap();
        assert!(d > 80.0, "距离应大于 min_km，实际 {d}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blocks_explicit_remote_list() {
        let dir = std::env::temp_dir().join(format!("wm_blk2_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("440100.json"), serde_json::to_string(&fc()).unwrap()).unwrap();
        let far = json!({"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"name":"北京市","adcode":110000},
             "geometry":{"type":"Polygon","coordinates":[[[116.3,39.9],[116.5,39.9],[116.5,40.0],[116.3,40.0],[116.3,39.9]]]}}]});
        std::fs::write(dir.join("110000.json"), serde_json::to_string(&far).unwrap()).unwrap();
        let src = GeoSource::offline(&dir);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let out = rt.block_on(build_blocks(&src, "440100", Some(vec!["110000".into()]), 6, 80.0, "dark")).unwrap();
        assert_eq!(out["auto"], false);
        let r0 = &out["remotes"][0];
        assert_eq!(r0["name"], "北京市");
        assert_eq!(r0["dir"], "北", "北京在广州北面");
        assert!(r0["distance_km"].as_f64().unwrap() > 1000.0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
