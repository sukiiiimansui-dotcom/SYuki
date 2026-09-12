//! 离线可用性（P5-4）：**已缓存的区域在没网时也要能看**。
//!
//! ## 前提：这条通路本来就不该"问网络"
//! "离线吗"这种判断最忌讳 `ping` 一下再决定 —— 那正好在最需要离线的时刻
//! 多等一个超时。真正的判据只有一个：**本地有没有这份缓存**。
//! 本模块只做两件事：
//!   1. 数一数本地有什么（`geo/` 的 geojson 缓存 + `maplib/` 的地图与布局）；
//!   2. 判定"要看的这个区域"能不能离线命中（[`coverage`]）。
//! 全程 `std::fs` + 纯字符串，**没有任何网络调用**（本文件连 reqwest 都不 import）。
//!
//! ## 缓存命中链路（现状，已核实）
//! `geo::GeoSource::fetch()` 第一件事就是 `load_cached(adcode)`
//! —— 命中就直接返回，**根本不构造请求**；命中不了才走 client，而
//! `GeoSource::offline()`（client = None）会直接回 `"…当前为离线模式"`。
//! 所以"已缓存的区域离线可见"在实现上已经天然成立，本模块补的是
//! **让用户/前端知道哪些区域能离线看**（否则只能靠"点一下试试"）。
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

use super::geo;

/// 一条 geojson 缓存（`geo/` 目录下的 `{adcode}.json` 或 `{adcode}_full.json`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeoCacheEntry {
    /// 6 位行政区划码
    pub adcode: String,
    /// 文件字节数
    pub bytes: u64,
    /// 文件修改时间（unix 秒；拿不到就是 0）
    pub mtime: u64,
    /// 是 `{adcode}_full.json`（带下辖子级的那份）
    pub full: bool,
}

/// 要看的区域能不能离线命中。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coverage {
    /// 这个 adcode 自己有缓存
    Exact,
    /// 自己没有，但**上级**有（例：想看 440104 荔湾区，缓存里只有 440100 广州市
    /// —— 广州市那份 `_full` 里就带着荔湾区的边界）
    Ancestor(String),
    /// 一点都没有，离线看不了
    None,
}

impl Coverage {
    /// 能不能离线看到（`Exact` 或 `Ancestor` 都算能）。
    pub fn hit(&self) -> bool {
        !matches!(self, Coverage::None)
    }

    /// 命中方式（给前端显示用）：`"exact"` / `"ancestor"` / `"none"`
    pub fn kind(&self) -> &'static str {
        match self {
            Coverage::Exact => "exact",
            Coverage::Ancestor(_) => "ancestor",
            Coverage::None => "none",
        }
    }

    /// 命中的那一级 adcode（`Exact` 就是自己）。
    pub fn matched(&self, want: &str) -> Option<String> {
        match self {
            Coverage::Exact => Some(want.trim().to_string()),
            Coverage::Ancestor(a) => Some(a.clone()),
            Coverage::None => None,
        }
    }
}

/// 扫一遍 geojson 缓存目录（纯文件系统，不联网）。
///
/// 认 `{adcode}.json` 与 `{adcode}_full.json`（后者 `full = true`），
/// 其它文件名一律忽略；adcode 必须是纯数字（与 `GeoSource::cached_adcodes` 同口径）。
pub fn scan_geo_dir(dir: &Path) -> Vec<GeoCacheEntry> {
    let mut out: Vec<GeoCacheEntry> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        let (adcode, full) = match stem.strip_suffix("_full") {
            Some(a) => (a, true),
            None => (stem, false),
        };
        if adcode.is_empty() || !adcode.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        out.push(GeoCacheEntry {
            adcode: adcode.to_string(),
            bytes: meta.len(),
            mtime: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
            full,
        });
    }
    out.sort_by(|a, b| a.adcode.cmp(&b.adcode).then(b.full.cmp(&a.full)));
    out.dedup_by(|a, b| a.adcode == b.adcode && a.full == b.full);
    out
}

/// 离线命中判定（纯函数）：`cached` 是本地已有的 adcode 列表，`want` 是想看的那个。
///
/// 上级链走的是 `geo::parent_of`（**同一个实现**，不是抄一份）：
/// `440104 → 440100 → 440000 → 100000`。命中上级也算能看 —— 上级那份
/// `_full` 缓存里本来就带着下级的边界（`geo::features` 会把它拍平）。
pub fn coverage(cached: &[String], want: &str) -> Coverage {
    let want = want.trim();
    if want.is_empty() {
        return Coverage::None;
    }
    if cached.iter().any(|c| c.trim() == want) {
        return Coverage::Exact;
    }
    let mut cur = want.to_string();
    // `parent_of` 对非 6 位数字返回 None（脏数据直接判定为不命中）
    while let Some(parent) = geo::parent_of(&cur) {
        if cached.iter().any(|c| c.trim() == parent) {
            return Coverage::Ancestor(parent);
        }
        cur = parent;
    }
    Coverage::None
}

/// 汇总「现在有哪些区域可以离线看」（纯函数，给命令层用）。
///
/// `geo` 来自 [`scan_geo_dir`]，`maps` 来自 `maplib::MapLib::list`，
/// `layouts` 来自 `maplib::MapLib::list_layouts`。三者都只是本地文件，
/// 所以这个汇总本身**不代表任何联网判断**。
pub fn summary(geo: &[GeoCacheEntry], maps: &[Value], layouts: &[Value]) -> Value {
    let geo_bytes: u64 = geo.iter().map(|g| g.bytes).sum();
    let geo_adcodes: Vec<String> = geo.iter().map(|g| g.adcode.clone()).collect();

    let mut map_bytes: u64 = 0;
    let mut map_kinds: Map<String, Value> = Map::new();
    let mut map_styles: Map<String, Value> = Map::new();
    // adcode → (地图数, 类型集合, 样式集合, 最近访问)
    // 用 BTreeMap 而不是 HashMap：这一层的顺序会**原样出现在返回的 JSON 里**，
    // 每次调用顺序都变的话，前端 diff / 测试断言都成了掷骰子。
    let mut per_area: BTreeMap<String, (u64, Vec<String>, Vec<String>, u64)> = BTreeMap::new();
    for e in maps {
        let bytes = e.get("bytes").and_then(Value::as_u64).unwrap_or(0);
        map_bytes += bytes;
        let kind = str_of(e, "kind").unwrap_or_else(|| "other".to_string());
        bump(&mut map_kinds, &kind);
        let style = str_of(e, "style").unwrap_or_default();
        if !style.is_empty() {
            bump(&mut map_styles, &style);
        }
        let adcode = str_of(e, "adcode").unwrap_or_default();
        if adcode.is_empty() {
            continue;
        }
        let last = e.get("lastAccess").and_then(Value::as_u64).unwrap_or(0);
        let slot = per_area
            .entry(adcode)
            .or_insert_with(|| (0, Vec::new(), Vec::new(), 0));
        slot.0 += 1;
        if !kind.is_empty() && !slot.1.iter().any(|k| *k == kind) {
            slot.1.push(kind);
        }
        if !style.is_empty() && !slot.2.iter().any(|s| *s == style) {
            slot.2.push(style);
        }
        slot.3 = slot.3.max(last);
    }

    // 区域表 = geo 缓存里的 adcode ∪ 地图库里的 adcode
    let mut area_codes: Vec<String> = geo_adcodes.clone();
    for (ad, _) in per_area.iter() {
        if !area_codes.iter().any(|a| a == ad) {
            area_codes.push(ad.clone());
        }
    }
    area_codes.sort();
    area_codes.dedup();

    let mut areas: Vec<Value> = Vec::new();
    for ad in &area_codes {
        let geo_hit = geo.iter().find(|g| &g.adcode == ad);
        let (maps_n, kinds, styles, last_access) = per_area
            .get(ad)
            .cloned()
            .unwrap_or((0, Vec::new(), Vec::new(), 0));
        let covered = coverage(&geo_adcodes, ad);
        areas.push(json!({
            "adcode": ad,
            "level": geo::level_of(ad),
            // 离线能不能看这个区域：geo 自己有 / 上级有 / 都没有
            "offline": covered.hit(),
            "coverage": covered.kind(),
            "covered_by": covered.matched(ad),
            "has_geo": geo_hit.is_some(),
            "geo_full": geo_hit.map(|g| g.full).unwrap_or(false),
            "geo_bytes": geo_hit.map(|g| g.bytes).unwrap_or(0),
            "maps": maps_n,
            "kinds": kinds,
            "styles": styles,
            "last_access": last_access,
        }));
    }

    let layout_keys: Vec<String> = layouts
        .iter()
        .filter_map(|l| str_of(l, "key"))
        .take(200)
        .collect();

    json!({
        "offline": true,
        "note": "只读本地缓存（geo/ + maplib/），本命令不发任何网络请求",
        "geo": {
            "count": geo.len(),
            "bytes": geo_bytes,
            "mb": round2(geo_bytes as f64 / 1048576.0),
            "adcodes": geo.iter().map(|g| json!({
                "adcode": g.adcode,
                "level": geo::level_of(&g.adcode),
                "full": g.full,
                "bytes": g.bytes,
                "mtime": g.mtime,
            })).collect::<Vec<_>>(),
        },
        "maps": {
            "count": maps.len(),
            "bytes": map_bytes,
            "mb": round2(map_bytes as f64 / 1048576.0),
            "adcodes": per_area.keys().cloned().collect::<Vec<_>>(),
            "kinds": Value::Object(map_kinds),
            "styles": Value::Object(map_styles),
        },
        "layouts": { "count": layouts.len(), "keys": layout_keys },
        "areas": areas,
        "totals": {
            "areas": area_codes.len(),
            "offline_areas": areas.iter().filter(|a| a["offline"] == json!(true)).count(),
            "geojson": geo.len(),
            "maps": maps.len(),
            "layouts": layouts.len(),
        },
    })
}

/// 取一个非空字符串字段。
fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 计数 +1（`by_kind` 那种 `{"region": 3}` 的累加）。
fn bump(map: &mut Map<String, Value>, key: &str) {
    let n = map.get(key).and_then(Value::as_u64).unwrap_or(0) + 1;
    map.insert(key.to_string(), json!(n));
}

/// 保留两位小数（与 `maplib::stats` 的 `mb` 口径一致）。
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 测试用隔离目录（/tmp 在本机不可写：优先 TMPDIR）
    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let base = std::env::var("TMPDIR").unwrap_or_else(|_| ".".to_string());
        let dir = std::path::Path::new(&base).join(format!("wm_offline_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn geo_entry(adcode: &str, full: bool) -> GeoCacheEntry {
        GeoCacheEntry {
            adcode: adcode.to_string(),
            bytes: 100,
            mtime: 1,
            full,
        }
    }

    /// 扫目录：认 `{adcode}.json` 与 `{adcode}_full.json`，其它文件一概不算。
    #[test]
    fn scan_counts_only_adcode_cache_files() {
        let dir = tmp_dir("scan");
        std::fs::write(dir.join("440100.json"), br#"{"features":[]}"#).unwrap();
        std::fs::write(dir.join("440300_full.json"), br#"{"features":[1,2,3]}"#).unwrap();
        std::fs::write(dir.join("index.json"), b"{}").unwrap();
        std::fs::write(dir.join("readme.txt"), b"hello").unwrap();
        std::fs::write(dir.join("_full.json"), b"{}").unwrap();

        let got = scan_geo_dir(&dir);
        assert_eq!(got.len(), 2, "只该认出两个 adcode 缓存: {got:?}");
        assert_eq!(got[0].adcode, "440100");
        assert!(!got[0].full);
        assert!(got[0].bytes > 0);
        assert_eq!(got[1].adcode, "440300");
        assert!(got[1].full, "_full 那份要标出来");
        // 目录不存在 = 空表（不 panic）
        assert!(scan_geo_dir(&dir.join("nope")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 命中判定：自己有 = exact；只有上级 = ancestor；都没有 = none。
    #[test]
    fn coverage_prefers_exact_then_walks_up_the_chain() {
        let cached = vec!["440000".to_string(), "440100".to_string(), "440300".to_string()];
        assert_eq!(coverage(&cached, "440100"), Coverage::Exact);
        // 区县没缓存，但市（440100）有 → 能离线看
        assert_eq!(coverage(&cached, "440104"), Coverage::Ancestor("440100".into()));
        assert!(coverage(&cached, "440104").hit());
        // 市没缓存但省（440000）有 → 还是能看
        assert_eq!(coverage(&cached, "440200"), Coverage::Ancestor("440000".into()));
        // 广东省自己是省级：缓存里有就是 exact
        assert_eq!(coverage(&cached, "440000"), Coverage::Exact);
        // 一点都没有
        assert_eq!(coverage(&cached, "110108"), Coverage::None);
        assert!(!coverage(&cached, "110108").hit());
        assert_eq!(coverage(&cached, "110108").matched("110108"), None);
        // 空/脏输入不命中，也不 panic
        assert_eq!(coverage(&cached, "   "), Coverage::None);
        assert_eq!(coverage(&[], "440100"), Coverage::None);
        assert_eq!(coverage(&cached, "4401"), Coverage::None);
    }

    /// 判定与 `geo::parent_of` 同源：命中上级链上的**最近**一级。
    #[test]
    fn coverage_reports_the_nearest_cached_ancestor() {
        let cached = vec!["440000".to_string()];
        // 440104 → 440100（没缓存）→ 440000（有）
        assert_eq!(coverage(&cached, "440104"), Coverage::Ancestor("440000".into()));
        assert_eq!(coverage(&cached, "440104").matched("440104").as_deref(), Some("440000"));
        assert_eq!(coverage(&cached, "440104").kind(), "ancestor");
        assert_eq!(coverage(&cached, "440000").kind(), "exact");
        assert_eq!(coverage(&[], "440000").kind(), "none");
        // 带空白的缓存项也能命中（本地文件名的宽容度）
        let messy = vec![" 440100 ".to_string()];
        assert_eq!(coverage(&messy, "440100"), Coverage::Exact);
    }

    /// 汇总：区域表 = geo ∪ 地图库，字段齐全；空输入不炸。
    #[test]
    fn summary_merges_geo_and_maps_into_areas() {
        let geo = vec![geo_entry("440100", true), geo_entry("440300", false)];
        let maps = vec![
            json!({"id":"region:440100:night","kind":"region","adcode":"440100","style":"night","bytes":2048,"lastAccess":100}),
            json!({"id":"district:440100:x","kind":"district","adcode":"440100","style":"day","bytes":1024,"lastAccess":300}),
            json!({"id":"region:440300:day","kind":"region","adcode":"440300","style":"day","bytes":512,"lastAccess":200}),
        ];
        let layouts = vec![json!({"key":"abc"}), json!({"key":"def"})];
        let s = summary(&geo, &maps, &layouts);

        assert_eq!(s["offline"], json!(true));
        assert_eq!(s["geo"]["count"], json!(2));
        assert_eq!(s["geo"]["bytes"], json!(200));
        assert_eq!(s["maps"]["count"], json!(3));
        assert_eq!(s["maps"]["bytes"], json!(3584));
        assert_eq!(s["maps"]["kinds"]["region"], json!(2));
        assert_eq!(s["maps"]["kinds"]["district"], json!(1));
        assert_eq!(s["maps"]["styles"]["night"], json!(1));
        assert_eq!(s["layouts"]["count"], json!(2));
        assert_eq!(s["totals"]["areas"], json!(2));
        assert_eq!(s["totals"]["offline_areas"], json!(2));

        let areas = s["areas"].as_array().unwrap();
        assert_eq!(areas.len(), 2);
        assert_eq!(areas[0]["adcode"], json!("440100"));
        assert_eq!(areas[0]["level"], json!("city"));
        assert_eq!(areas[0]["offline"], json!(true));
        assert_eq!(areas[0]["coverage"], json!("exact"));
        assert_eq!(areas[0]["covered_by"], json!("440100"));
        assert_eq!(areas[0]["geo_full"], json!(true));
        assert_eq!(areas[0]["maps"], json!(2));
        assert_eq!(areas[0]["last_access"], json!(300));
        assert_eq!(areas[1]["adcode"], json!("440300"));
        assert_eq!(areas[1]["coverage"], json!("exact"));
        assert_eq!(areas[1]["geo_bytes"], json!(100));
    }

    /// 只有地图、没有 geojson 的区域也要列出来，但「离线看」为 false。
    #[test]
    fn summary_marks_map_only_areas_as_not_offline() {
        let maps = vec![json!({"id":"x","kind":"bigmap","adcode":"330100","bytes":10,"lastAccess":5})];
        let s = summary(&[], &maps, &[]);
        let areas = s["areas"].as_array().unwrap();
        assert_eq!(areas.len(), 1);
        assert_eq!(areas[0]["adcode"], json!("330100"));
        assert_eq!(areas[0]["offline"], json!(false));
        assert_eq!(areas[0]["coverage"], json!("none"));
        assert_eq!(areas[0]["has_geo"], json!(false));
        assert_eq!(areas[0]["maps"], json!(1));
        assert_eq!(s["totals"]["offline_areas"], json!(0));
        // 地图项没有 adcode 时不该凭空造一个区域出来
        let s2 = summary(&[], &[json!({"id":"y","kind":"district","bytes":1})], &[]);
        assert!(s2["areas"].as_array().unwrap().is_empty());
        assert_eq!(s2["maps"]["count"], json!(1));
    }

    /// 一个都没有时：计数全 0、区域表空，但结构齐全（前端不用判 null）。
    #[test]
    fn summary_of_an_empty_library_is_still_well_formed() {
        let s = summary(&[], &[], &[]);
        assert_eq!(s["offline"], json!(true));
        assert_eq!(s["geo"]["count"], json!(0));
        assert_eq!(s["maps"]["count"], json!(0));
        assert_eq!(s["layouts"]["count"], json!(0));
        assert_eq!(s["totals"]["areas"], json!(0));
        assert_eq!(s["areas"], json!([]));
        assert!(s["geo"]["adcodes"].as_array().unwrap().is_empty());
        assert_eq!(s["maps"]["bytes"], json!(0));
    }
}
