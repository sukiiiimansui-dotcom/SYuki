//! 地理数据与区块（T1-2/T1-3 的 Rust 版）
//!
//! 数据来源：阿里云 DataV GeoAtlas（与 Python 侧一致）
//!   · 有子级的区域：`{adcode}_full.json`
//!   · 无子级的区县：`{adcode}.json`
//! 缓存优先级：本地缓存 → 远程下载 → 写入缓存
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::coord::{self, BBox};

const DATA_V: &str = "https://geo.datav.aliyun.com/areas_v3/bound";

/// 一个行政区划要素
#[derive(Debug, Clone)]
pub struct Feature {
    pub name: String,
    pub adcode: String,
    /// 该要素的全部坐标点（lng,lat）
    pub points: Vec<(f64, f64)>,
    /// 世界坐标 bbox
    pub bbox: Option<BBox>,
}

impl Feature {
    /// 经纬度 bbox
    pub fn lng_lat_range(&self) -> (f64, f64, f64, f64) {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &(lng, lat) in &self.points {
            if lng < x0 {
                x0 = lng;
            }
            if lat < y0 {
                y0 = lat;
            }
            if lng > x1 {
                x1 = lng;
            }
            if lat > y1 {
                y1 = lat;
            }
        }
        (x0, y0, x1, y1)
    }

    /// 中心点（经纬度）
    pub fn center(&self) -> (f64, f64) {
        let (x0, y0, x1, y1) = self.lng_lat_range();
        ((x0 + x1) / 2.0, (y0 + y1) / 2.0)
    }
}

/// 递归收集 GeoJSON 坐标数组里的点
fn walk_coords(c: &Value, out: &mut Vec<(f64, f64)>) {
    match c {
        Value::Array(arr) => {
            // [lng, lat] 形式
            if arr.len() >= 2 && arr[0].is_number() && arr[1].is_number() {
                out.push((
                    arr[0].as_f64().unwrap_or(0.0),
                    arr[1].as_f64().unwrap_or(0.0),
                ));
                return;
            }
            for x in arr {
                walk_coords(x, out);
            }
        }
        _ => {}
    }
}

/// 从 FeatureCollection 提取要素（跳过没有名字的）
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
        let adcode = p
            .get("adcode")
            .map(|v| {
                if let Some(s) = v.as_str() {
                    s.to_string()
                } else if let Some(n) = v.as_i64() {
                    n.to_string()
                } else {
                    String::new()
                }
            })
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        if let Some(sa) = self_adcode {
            if adcode == sa {
                continue; // 跳过自身
            }
        }
        let mut pts = Vec::new();
        if let Some(g) = f.get("geometry") {
            if let Some(c) = g.get("coordinates") {
                walk_coords(c, &mut pts);
            }
        }
        let bbox = coord::bbox_of_lng_lat(&pts);
        out.push(Feature {
            name,
            adcode,
            points: pts,
            bbox,
        });
    }
    out
}

/// 地理数据源：本地缓存目录 + 远程下载兜底
pub struct GeoSource {
    pub cache: PathBuf,
    client: reqwest::Client,
}

impl GeoSource {
    pub fn new(cache: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&cache);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .unwrap_or_default();
        Self { cache, client }
    }

    pub fn cached_path(&self, adcode: &str) -> PathBuf {
        self.cache.join(format!("{adcode}.json"))
    }

    /// 已有缓存就直接用（不联网）
    pub fn load_cached(&self, adcode: &str) -> Option<Value> {
        let p = self.cached_path(adcode);
        let raw = std::fs::read_to_string(p).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// 缓存优先，没有就下载（先 `_full` 再普通，与 Python 侧相反：
    /// Rust 侧先试普通文件是因为区县级没有 `_full`，而省市级两者都有）
    pub async fn fetch(&self, adcode: &str) -> Result<Value, String> {
        if let Some(v) = self.load_cached(adcode) {
            return Ok(v);
        }
        let mut last_err = String::from("未知错误");
        for suffix in ["_full", ""] {
            let url = format!("{DATA_V}/{adcode}{suffix}.json");
            match self.client.get(&url).send().await {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        last_err = format!("{url} → HTTP {}", resp.status());
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
                            last_err = format!("{url} → 不是 FeatureCollection");
                        }
                        Err(e) => last_err = format!("{url} → 解析失败 {e}"),
                    }
                }
                Err(e) => last_err = format!("{url} → {e}"),
            }
        }
        Err(format!("无法获取 {adcode} 的地理数据：{last_err}"))
    }

    /// 列出本地缓存里已有的 adcode（用于自动挑选远处块）
    pub fn cached_adcodes(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.cache) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if let Some(stem) = name.strip_suffix(".json") {
                    let ad = stem.strip_suffix("_full").unwrap_or(stem);
                    if ad.chars().all(|c| c.is_ascii_digit()) && !ad.is_empty() && ad != "100000" {
                        out.push(ad.to_string());
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// 远处块信息（给前端的结构）
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
            "adcode": self.adcode,
            "name": self.name,
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

/// 主块信息
#[derive(Debug, Clone)]
pub struct MainBlock {
    pub adcode: String,
    pub name: String,
    pub center: (f64, f64),
    pub lng_range: (f64, f64),
    pub lat_range: (f64, f64),
    pub radius_km: f64,
    pub feature_count: usize,
}

impl MainBlock {
    pub fn to_json(&self, style: &str, scale: u32) -> Value {
        serde_json::json!({
            "adcode": self.adcode,
            "name": self.name,
            "center": [self.center.0, self.center.1],
            "bbox_lng": [self.lng_range.0, self.lng_range.1],
            "bbox_lat": [self.lat_range.0, self.lat_range.1],
            "radius_km": (self.radius_km * 10.0).round() / 10.0,
            "regions": self.feature_count,
            "img": format!("/api/bigmap?ad={}&style={}&scale={}", self.adcode, style, scale),
            "img_url": format!("/api/bigmap_img?ad={}&style={}&scale={}", self.adcode, style, scale),
            "source": "rust",
        })
    }
}

/// 组装主块 + 远处块（T1-3 的核心逻辑）
pub async fn build_blocks(
    src: &GeoSource,
    main_ad: &str,
    remote_ads: Option<Vec<String>>,
    limit: usize,
    min_km: f64,
) -> Result<Value, String> {
    let main_fc = src.fetch(main_ad).await?;
    let main_features = features(&main_fc, Some(main_ad));
    if main_features.is_empty() {
        return Err(format!("{main_ad} 没有有效区划"));
    }
    let mut all_pts: Vec<(f64, f64)> = Vec::new();
    for f in &main_features {
        all_pts.extend_from_slice(&f.points);
    }
    let main_bbox = coord::bbox_of_lng_lat(&all_pts).ok_or("主块没有坐标")?;
    let (x0, y0, x1, y1) = main_bbox;
    let main_center = coord::world_to_lng_lat((x0 + x1) / 2.0, (y0 + y1) / 2.0);

    let main_lng = (
        all_pts.iter().map(|p| p.0).fold(f64::MAX, f64::min),
        all_pts.iter().map(|p| p.0).fold(f64::MIN, f64::max),
    );
    let main_lat = (
        all_pts.iter().map(|p| p.1).fold(f64::MAX, f64::min),
        all_pts.iter().map(|p| p.1).fold(f64::MIN, f64::max),
    );
    let diag = coord::haversine_m(
        (main_lng.0, main_lat.0),
        (main_lng.1, main_lat.1),
    );
    let main = MainBlock {
        adcode: main_ad.to_string(),
        name: main_features
            .first()
            .map(|_| String::new())
            .unwrap_or_default(),
        center: main_center,
        lng_range: main_lng,
        lat_range: main_lat,
        radius_km: diag / 2000.0,
        feature_count: main_features.len(),
    };

    let auto = remote_ads.is_none();
    let cands: Vec<String> = match remote_ads {
        Some(v) => v,
        None => src.cached_adcodes(),
    };

    let inside = |lng: f64, lat: f64| -> bool {
        lng >= main_lng.0 && lng <= main_lng.1 && lat >= main_lat.0 && lat <= main_lat.1
    };

    let mut remotes: Vec<RemoteBlock> = Vec::new();
    for ad in cands {
        if ad == main_ad {
            continue;
        }
        let fc = match src.load_cached(&ad) {
            Some(v) => v,
            None => continue, // 自动挑选时只认本地缓存，避免联网拉一堆
        };
        let feats = features(&fc, Some(&ad));
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
    if auto && limit > 0 {
        remotes.truncate(limit);
    }

    Ok(serde_json::json!({
        "main": main.to_json("gaode", 1),
        "remotes": remotes.iter().map(|r| r.to_json("gaode")).collect::<Vec<_>>(),
        "auto": auto,
        "source": "rust",
    }))
}

/// 找缓存目录里离给定坐标最近的区域（用于「按定位定主块」）
pub fn nearest_cached(src: &GeoSource, lng: f64, lat: f64) -> Option<(String, f64)> {
    let mut best: Option<(String, f64)> = None;
    for ad in src.cached_adcodes() {
        let fc = match src.load_cached(&ad) {
            Some(v) => v,
            None => continue,
        };
        let feats = features(&fc, Some(&ad));
        for f in feats {
            // 先用 bbox 粗筛，再精确判点在多边形内
            if let Some(bb) = f.bbox {
                let (wx, wy) = coord::lng_lat_to_world(lng, lat);
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
                best = Some((ad.clone(), d));
            }
        }
    }
    best
}

/// 判断某路径是否是地理缓存目录（供上层选择数据源）
pub fn is_geo_cache(p: &Path) -> bool {
    p.join("worlddata").join("cn").is_dir()
}
