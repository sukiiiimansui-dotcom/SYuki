//! osm.rs — 从 OpenStreetMap(Overpass) 拉取真实建筑/道路/设施（移植自 Python `osm_fetch.py`）
//!
//! 为什么需要它：小区/区县地图由 LLM 虚构生成，但纯虚构会「不像那个地方」。
//! 拉一份该坐标周边的真实 OSM 摘要塞进 prompt，AI 画出来的街廓就带上了当地特征
//! （南方骑楼密度、北方院子尺度、有没有水系），这是「LLM 虚构 + 真实地理参考」的核心。
//!
//! 为什么必须缓存：
//!   · Overpass 公共端点一次查询要 ~19 秒，还会 502/限流；
//!   · 而「同一个小区」在地图里会被反复打开 → 按 ~200m 网格做缓存键，同格复用。
//! 为什么多端点回退：三个公共镜像轮流试，全失败就返回 None，
//!   调用方降级为「纯 LLM 生成」——地图功能绝不能因为外部服务挂了就整条链路失败。
//!
//! 与 Python 的对应关系：`_grid_key` → [`grid_key`]、`_query` → [`overpass_query`]、
//! `fetch_area` → [`fetch_area`]、`summarize` → [`summarize`]、`describe_for_llm` → [`describe_for_llm`]。
//! 摘要的 JSON 结构必须与 Python 完全一致，因为 `sketch::OsmHint::from_summary` 直接消费它。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 三个公共 Overpass 端点，按顺序回退
pub const ENDPOINTS: [&str; 3] = [
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
    "https://overpass.osm.ch/api/interpreter",
];

/// 摘要里要分桶统计的 5 类标签（顺序即 Python 里的出现顺序）
pub const BUCKET_KEYS: [&str; 5] = ["building", "highway", "amenity", "leisure", "landuse"];

/// 默认关心这 5 类标签（建筑/道路/设施/休闲/用地）
pub const DEFAULT_KINDS: [&str; 5] = ["building", "highway", "amenity", "leisure", "landuse"];

/// 缓存网格边长：0.002° ≈ 200m。同一个小区反复打开时命中同一份缓存。
const GRID: f64 = 0.002;

/// Python 的 `round()` 是「四舍六入五取偶」，Rust 的 `f64::round()` 是「五入」。
/// 缓存键要和 Python 侧共用同一份文件，所以这里必须复刻 Python 的行为，
/// 否则恰好落在 .5 边界的坐标会在两边算出不同的键、各存一份缓存。
fn py_round(x: f64) -> f64 {
    let f = x.floor();
    let diff = x - f;
    if (diff - 0.5).abs() < 1e-12 {
        // 正好在半点 → 取偶数侧
        if (f as i64) % 2 == 0 {
            f
        } else {
            f + 1.0
        }
    } else if diff > 0.5 {
        f + 1.0
    } else {
        f
    }
}

/// 按 ~200m 网格生成缓存键（与 Python `_grid_key` 完全一致：`{lat}_{lng}_{radius}`）
pub fn grid_key(lat: f64, lng: f64, radius_m: f64) -> String {
    format!(
        "{}_{}_{}",
        py_round(lat / GRID) as i64,
        py_round(lng / GRID) as i64,
        radius_m as i64
    )
}

/// 构造 Overpass QL：半径内指定类型的 way/node 都取，带 center 便于画多边形
pub fn overpass_query(lat: f64, lng: f64, radius_m: f64, kinds: &[String]) -> String {
    let mut parts = String::new();
    for k in kinds {
        parts.push_str(&format!("way[\"{k}\"](around:{radius_m},{lat},{lng});"));
        parts.push_str(&format!("node[\"{k}\"](around:{radius_m},{lat},{lng});"));
    }
    format!("[out:json][timeout:40];({parts});out center tags;")
}

/// 极简 `application/x-www-form-urlencoded` 编码。
///
/// 为什么不用 `reqwest` 的 `.form()`：reqwest 0.13 把它挪到了 `form` feature 后面，
/// 而宿主工程（LingChat）没有开这个 feature —— 为一行请求去改官方依赖不值得，
/// 而 Overpass 的请求体本来就只有一个 `data=` 字段，自己编码更省事也更可控。
fn form_encode(pairs: &[(&str, &str)]) -> String {
    let pct = |s: &str| -> String {
        let mut o = String::new();
        for b in s.as_bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    o.push(*b as char)
                }
                b' ' => o.push('+'),
                _ => o.push_str(&format!("%{b:02X}")),
            }
        }
        o
    };
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", pct(k), pct(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// 缓存文件路径
pub fn cache_path(dir: impl AsRef<Path>, key: &str) -> PathBuf {
    dir.as_ref().join(format!("{key}.json"))
}

/// 读缓存。小于 50 字节的文件视为「写坏了的半截文件」，当作没有。
/// （Python 侧同样判断 `getsize(cf) > 50`——曾因为一次中断留下 3 字节文件导致解析失败）
pub fn load_cached(dir: impl AsRef<Path>, key: &str) -> Option<Value> {
    let p = cache_path(dir, key);
    let meta = std::fs::metadata(&p).ok()?;
    if meta.len() <= 50 {
        return None;
    }
    let raw = std::fs::read_to_string(&p).ok()?;
    serde_json::from_str(&raw).ok()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 拉取区域 OSM 数据。缓存优先；所有端点都失败返回 None（调用方降级为纯 LLM）。
///
/// 返回值里会附 `_meta`（坐标、半径、时间戳、命中的端点）便于排障与增量刷新。
pub async fn fetch_area(
    client: &reqwest::Client,
    dir: impl AsRef<Path>,
    lat: f64,
    lng: f64,
    radius_m: f64,
    kinds: Option<Vec<String>>,
    force: bool,
) -> Option<Value> {
    let kinds = kinds.unwrap_or_else(|| DEFAULT_KINDS.iter().map(|s| s.to_string()).collect());
    let dir = dir.as_ref();
    let key = grid_key(lat, lng, radius_m);

    if !force {
        if let Some(v) = load_cached(dir, &key) {
            return Some(v);
        }
    }

    let q = overpass_query(lat, lng, radius_m, &kinds);
    for ep in ENDPOINTS {
        let resp = client
            .post(ep)
            .header("User-Agent", "LSYuki-maps/1.0")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form_encode(&[("data", q.as_str())]))
            .send()
            .await;
        let j = match resp {
            Ok(r) if r.status().is_success() => match r.json::<Value>().await {
                Ok(v) => v,
                Err(_) => continue,
            },
            _ => continue,
        };
        if j.get("elements").is_none() {
            continue;
        }
        let mut j = j;
        j["_meta"] = json!({
            "lat": lat, "lng": lng, "radius_m": radius_m,
            "ts": now_secs(), "endpoint": ep,
        });
        let _ = std::fs::create_dir_all(dir);
        if let Ok(txt) = serde_json::to_string(&j) {
            let _ = std::fs::write(cache_path(dir, &key), txt);
        }
        return Some(j);
    }
    None
}

/// 按出现次数排序取前 n（同数保持「首次出现」顺序，与 Python 的稳定排序一致）
fn top(counts: &HashMap<String, u32>, order: &[String], n: usize) -> Vec<Value> {
    let mut items: Vec<(&String, u32)> = order
        .iter()
        .filter_map(|k| counts.get(k).map(|c| (k, *c)))
        .collect();
    items.sort_by(|a, b| b.1.cmp(&a.1)); // 稳定排序：同数保持插入顺序
    items
        .into_iter()
        .take(n)
        .map(|(k, c)| json!([k, c]))
        .collect()
}

/// 把 OSM 原始数据压缩成 LLM 可读的摘要（控制 token）
///
/// 注意：这个结构是 `sketch::OsmHint::from_summary` 的输入契约，
/// `building_types` / `highway_types` 必须是 `[[名字, 次数], ...]` 形式。
pub fn summarize(osm: &Value) -> Option<Value> {
    let el = osm.get("elements").and_then(|e| e.as_array())?;

    let mut counts: HashMap<String, u32> = HashMap::new();
    // 每个桶单独记一份「首次出现顺序」，用于同数时的稳定排序
    let mut orders: HashMap<&str, Vec<String>> = HashMap::new();
    for key in BUCKET_KEYS {
        orders.insert(key, Vec::new());
    }

    for e in el {
        let t = match e.get("tags") {
            Some(t) => t,
            None => continue,
        };
        let obj = match t.as_object() {
            Some(o) => o,
            None => continue,
        };
        for key in BUCKET_KEYS {
            if let Some(v) = obj.get(key).and_then(|v| v.as_str()) {
                let ck = format!("{key}\u{1}{v}");
                let c = counts.entry(ck).or_insert(0);
                if *c == 0 {
                    orders.get_mut(key).unwrap().push(v.to_string());
                }
                *c += 1;
            }
        }
    }

    let bucket = |key: &str| -> Vec<Value> {
        let order = orders.get(key).cloned().unwrap_or_default();
        let scoped: HashMap<String, u32> = order
            .iter()
            .map(|v| (v.clone(), *counts.get(&format!("{key}\u{1}{v}")).unwrap_or(&0)))
            .collect();
        top(&scoped, &order, 6)
    };

    Some(json!({
        "total_elements": el.len(),
        "building_types": bucket("building"),
        "highway_types": bucket("highway"),
        "amenities": bucket("amenity"),
        "leisure": bucket("leisure"),
        "landuse": bucket("landuse"),
        "meta": osm.get("_meta").cloned().unwrap_or_else(|| json!({})),
    }))
}

/// 把摘要转成一句中文描述，塞进 LLM prompt
pub fn describe_for_llm(osm: &Value) -> String {
    let s = match summarize(osm) {
        Some(s) => s,
        None => return String::new(),
    };
    let fmt = |k: &str| -> String {
        let arr = s.get(k).and_then(|v| v.as_array());
        match arr {
            Some(a) if !a.is_empty() => a
                .iter()
                .filter_map(|it| {
                    let name = it.get(0)?.as_str()?;
                    let n = it.get(1)?.as_u64()?;
                    Some(format!("{name}×{n}"))
                })
                .collect::<Vec<_>>()
                .join("、"),
            _ => "无".to_string(),
        }
    };
    format!(
        "该区域真实 OSM 数据：共 {} 个地物；建筑类型 {}；道路 {}；设施 {}；休闲 {}；用地 {}。",
        s.get("total_elements").and_then(|v| v.as_u64()).unwrap_or(0),
        fmt("building_types"),
        fmt("highway_types"),
        fmt("amenities"),
        fmt("leisure"),
        fmt("landuse"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用临时目录：Android/Termux 上 /tmp 不可写，优先 TMPDIR，退到 $HOME/.cache
    fn tmp(tag: &str) -> PathBuf {
        let base = if let Ok(t) = std::env::var("TMPDIR") {
            let p = PathBuf::from(t);
            if p.is_dir() {
                p
            } else {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache")
            }
        } else {
            PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache")
        };
        let d = base.join(format!("wm_osm_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn grid_key_matches_python_format() {
        // Python: f'{round(lat/0.002)}_{round(lng/0.002)}_{int(radius)}'
        // 29.7524 / 0.002 = 14876.2 → 14876；107.2779 / 0.002 = 53638.95 → 53639
        assert_eq!(grid_key(29.7524, 107.2779, 400.0), "14876_53639_400");
        assert_eq!(grid_key(23.1291, 113.2644, 500.0), "11565_56632_500");
    }

    #[test]
    fn grid_key_uses_bankers_rounding() {
        // 恰好在 .5 上：Python round(2.5)=2、round(3.5)=4（取偶），Rust 原生 round 会给 3、4
        assert_eq!(py_round(2.5), 2.0);
        assert_eq!(py_round(3.5), 4.0);
        assert_eq!(py_round(-0.5), 0.0);
        assert_eq!(py_round(-1.5), -2.0);
        assert_eq!(py_round(-2.5), -2.0);
        assert_eq!(py_round(2.4), 2.0);
        assert_eq!(py_round(2.6), 3.0);
    }

    #[test]
    fn query_contains_all_kinds_and_around() {
        let kinds: Vec<String> = DEFAULT_KINDS.iter().map(|s| s.to_string()).collect();
        let q = overpass_query(29.75, 107.28, 400.0, &kinds);
        assert!(q.starts_with("[out:json][timeout:40];("));
        assert!(q.ends_with(");out center tags;"));
        for k in DEFAULT_KINDS {
            assert!(q.contains(&format!("way[\"{k}\"](around:400,29.75,107.28);")), "缺 way {k}");
            assert!(q.contains(&format!("node[\"{k}\"](around:400,29.75,107.28);")), "缺 node {k}");
        }
    }

    fn fixture() -> Value {
        json!({
            "elements": [
                {"type": "way", "tags": {"building": "apartments"}},
                {"type": "way", "tags": {"building": "apartments"}},
                {"type": "way", "tags": {"building": "house"}},
                {"type": "way", "tags": {"highway": "residential"}},
                {"type": "node", "tags": {"amenity": "restaurant"}},
                {"type": "node", "tags": {"leisure": "park"}},
                {"type": "way", "tags": {"landuse": "grass"}},
                {"type": "node", "tags": {}},
                {"type": "node"}
            ],
            "_meta": {"lat": 29.75, "lng": 107.28, "radius_m": 400}
        })
    }

    #[test]
    fn summarize_counts_and_orders() {
        let s = summarize(&fixture()).unwrap();
        assert_eq!(s["total_elements"], json!(9));
        // apartments 出现 2 次排第一，house 1 次第二
        assert_eq!(s["building_types"][0], json!(["apartments", 2]));
        assert_eq!(s["building_types"][1], json!(["house", 1]));
        assert_eq!(s["highway_types"][0], json!(["residential", 1]));
        assert_eq!(s["amenities"][0], json!(["restaurant", 1]));
        assert_eq!(s["leisure"][0], json!(["park", 1]));
        assert_eq!(s["landuse"][0], json!(["grass", 1]));
        assert_eq!(s["meta"]["radius_m"], json!(400));
    }

    #[test]
    fn summarize_empty_and_malformed() {
        assert!(summarize(&json!({})).is_none(), "没有 elements 应返回 None");
        assert!(summarize(&json!({"elements": []})).is_some(), "空数组仍是有效摘要");
        let s = summarize(&json!({"elements": [{"type": "node"}]})).unwrap();
        assert_eq!(s["total_elements"], json!(1));
        assert_eq!(s["building_types"], json!([]));
    }

    #[test]
    fn describe_is_chinese_and_lists_types() {
        let d = describe_for_llm(&fixture());
        assert!(d.contains("该区域真实 OSM 数据"));
        assert!(d.contains("共 9 个地物"));
        assert!(d.contains("apartments×2"));
        assert!(d.contains("residential×1"));
        assert!(describe_for_llm(&json!({})).is_empty(), "无数据时给空串而不是半句话");
    }

    #[test]
    fn summary_feeds_sketch_osm_hint() {
        // 摘要结构的真正消费者是 sketch::OsmHint——契约在这里被钉住
        let s = summarize(&fixture()).unwrap();
        let hint = crate::world_map::sketch::OsmHint::from_summary(&s);
        assert_eq!(hint.building_kinds, 2, "apartments/house 两类建筑");
        assert_eq!(hint.highway_kinds, 1, "residential 一类道路");
        assert!(!hint.has_water, "样本里没有 water");
        assert!(hint.has_park, "leisure=park 应判定为有公园");
    }

    #[test]
    fn cache_roundtrip_and_tiny_file_ignored() {
        let dir = tmp("cache");
        let key = grid_key(29.7524, 107.2779, 400.0);
        assert!(load_cached(&dir, &key).is_none(), "还没写就该是 None");

        std::fs::write(cache_path(&dir, &key), serde_json::to_string(&fixture()).unwrap()).unwrap();
        let back = load_cached(&dir, &key).unwrap();
        assert_eq!(back["elements"].as_array().unwrap().len(), 9);

        // 半截文件（<50 字节）必须当作没有，否则解析失败会一路冒到 LLM 调用
        std::fs::write(cache_path(&dir, &key), "{\"elements\":[]}").unwrap();
        assert!(load_cached(&dir, &key).is_none(), "小于 50 字节的残file应忽略");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn form_encode_escapes_overpass_query() {
        // Overpass 的查询里有 [] " ; : , 这些字符，编码错就会被 400 拒掉
        let q = "[out:json][timeout:40];(way[\"building\"](around:400,29.75,107.28););out center tags;";
        let body = form_encode(&[("data", q)]);
        assert!(body.starts_with("data="));
        assert!(body.contains("%5Bout%3Ajson%5D"), "方括号与冒号要转义: {body}");
        assert!(body.contains("%22building%22"), "引号要转义");
        assert!(!body.contains(' '), "空格不能原样出现");
        // 空格按 form 规则编码成 +（%20 也合法，但 + 更省字节）
        assert_eq!(form_encode(&[("a", "b c")]), "a=b+c");
        assert_eq!(form_encode(&[("a", "1"), ("b", "2")]), "a=1&b=2");
    }

    #[test]
    fn cache_path_is_inside_dir() {
        let p = cache_path("/data/x/osm", "1_2_3");
        assert!(p.ends_with("1_2_3.json"));
        assert!(p.starts_with("/data/x/osm"));
    }
}
