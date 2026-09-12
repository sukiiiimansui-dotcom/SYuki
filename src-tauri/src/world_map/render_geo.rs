//! 行政区划级 SVG 渲染（移植自 Python `svg_geo.py`）
//!
//! 与 `render.rs`（小区级）共用同一套视觉语言，保证从全国缩到小区是连续的：
//!   country 全国  → 省界 + 省会点 + 省名
//!   province 省   → 市界 + 市中心点 + 市名
//!   city 市       → 区县界 + 区名
//!   district 区县 → 街镇边界
//!
//! 每个区划包在 `<g class="geo-region" data-adcode data-name>` 里 —— 前端据此做**点击下钻**。
use serde_json::Value;

// ── 三套配色（与 render.rs 对齐；RPG 风已按需求移除）──
pub struct GeoStyle {
    pub bg: &'static str,
    pub land: &'static str,
    pub land_alt: &'static str,
    pub border: &'static str,
    pub border2: &'static str,
    pub inner: &'static str,
    pub text: &'static str,
    pub text_halo: &'static str,
    pub sub_text: &'static str,
    pub dot: &'static str,
}

pub const STYLE_NAMES: [&str; 3] = ["gaode", "dark", "water"];

pub fn style_of(name: &str) -> GeoStyle {
    match name {
        "dark" => GeoStyle {
            bg: "#0a1420", land: "#182636", land_alt: "#1d2d3f",
            border: "#5a8cbe", border2: "#2a4055", inner: "rgba(0,0,0,.35)",
            text: "#c8dcf0", text_halo: "#0a1420", sub_text: "#6d87a8", dot: "#ff8a5a",
        },
        "water" => GeoStyle {
            bg: "#dfeaf2", land: "#f4efe3", land_alt: "#f8f4ea",
            border: "#b9c8d4", border2: "#d6cec0", inner: "rgba(140,130,110,.16)",
            text: "#6a6252", text_halo: "#faf6ec", sub_text: "#9a9080", dot: "#c88a6a",
        },
        _ => GeoStyle {
            bg: "#cfe3f5", land: "#e9e7dc", land_alt: "#eeece2",
            border: "#ffffff", border2: "#d8d4c6", inner: "rgba(120,130,150,.20)",
            text: "#3c4b5f", text_halo: "#ffffff", sub_text: "#7b8698", dot: "#e05a6a",
        },
    }
}

/// 各行政级别的线宽/字号基准
struct LevelStyle {
    border: f64,
    border2: f64,
    dot_r: f64,
    fs: f64,
    min_area: f64,
    show_dot: bool,
}

fn level_style(level: &str) -> LevelStyle {
    match level {
        "country" => LevelStyle { border: 1.6, border2: 0.8, dot_r: 3.4, fs: 13.5, min_area: 4e-6, show_dot: true },
        "province" => LevelStyle { border: 1.4, border2: 0.7, dot_r: 2.8, fs: 12.0, min_area: 2e-6, show_dot: true },
        "city" => LevelStyle { border: 1.1, border2: 0.55, dot_r: 2.2, fs: 11.0, min_area: 6e-7, show_dot: true },
        _ => LevelStyle { border: 0.9, border2: 0.45, dot_r: 1.8, fs: 10.5, min_area: 3e-7, show_dot: false },
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn est_width(text: &str, size: f64) -> f64 {
    text.chars().map(|c| if (c as u32) > 0x2E80 { size } else { size * 0.55 }).sum()
}

fn project(lng: f64, lat: f64) -> (f64, f64) {
    let x = (lng + 180.0) / 360.0;
    let s = (lat.to_radians()).sin().clamp(-0.9999, 0.9999);
    let y = 0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * std::f64::consts::PI);
    (x, y)
}

/// 把 geometry 统一成 [[ring, hole...], ...]
fn rings(geom: &Value) -> Vec<Vec<Vec<(f64, f64)>>> {
    let t = geom.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let c = match geom.get("coordinates").and_then(|v| v.as_array()) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let parse_ring = |r: &Value| -> Vec<(f64, f64)> {
        r.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        let pa = p.as_array()?;
                        Some((pa.first()?.as_f64()?, pa.get(1)?.as_f64()?))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    match t {
        "Polygon" => vec![c.iter().map(parse_ring).collect()],
        "MultiPolygon" => c
            .iter()
            .map(|poly| {
                poly.as_array().map(|rs| rs.iter().map(parse_ring).collect()).unwrap_or_default()
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// 像素级抽稀：相邻点太近就丢弃（0.6px 容差肉眼无感，但能砍掉大量冗余顶点）
fn simplify(ring: &[(f64, f64)], tol: f64) -> Vec<(f64, f64)> {
    if ring.len() < 4 {
        return ring.to_vec();
    }
    let mut out = Vec::with_capacity(ring.len());
    out.push(ring[0]);
    for pt in &ring[1..ring.len() - 1] {
        let last = out[out.len() - 1];
        if (pt.0 - last.0).abs() + (pt.1 - last.1).abs() >= tol {
            out.push(*pt);
        }
    }
    out.push(ring[ring.len() - 1]);
    if out.len() >= 3 { out } else { ring.to_vec() }
}

pub struct GeoOpts {
    pub level: String,
    pub style: String,
    pub width: f64,
    pub height: f64,
    pub pad: f64,
    pub zoom: i32,
    pub labels: bool,
    pub dots: bool,
    /// 是否标注顶点抽稀统计（开发用）
    pub show_stats: bool,
}

impl Default for GeoOpts {
    fn default() -> Self {
        Self {
            level: "province".into(), style: "gaode".into(),
            width: 1000.0, height: 760.0, pad: 26.0, zoom: 2,
            labels: true, dots: true, show_stats: false,
        }
    }
}

/// 渲染行政区划 SVG
pub fn render_geo_svg(fc: &Value, o: &GeoOpts) -> Result<String, String> {
    let st = style_of(&o.style);
    let ls = level_style(&o.level);
    let (w, h, pad) = (o.width, o.height, o.pad);

    struct Feat {
        name: String,
        adcode: String,
        polys: Vec<Vec<Vec<(f64, f64)>>>,
        wpts: Vec<(f64, f64)>,
    }
    // 一次遍历同时拿到「多边形几何」和「投影后的点」（避免两次解析导致索引错位）
    let empty = vec![];
    let raw = fc.get("features").and_then(|v| v.as_array()).unwrap_or(&empty);
    let mut feats: Vec<Feat> = Vec::new();
    let mut all: Vec<(f64, f64)> = Vec::new();
    for rf in raw {
        let props = match rf.get("properties") {
            Some(p) => p,
            None => continue,
        };
        let name = props.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if name.is_empty() {
            continue; // 没名字的要素不画（也点不中）
        }
        let adcode = match props.get("adcode") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        let polys = match rf.get("geometry") {
            Some(g) => rings(g),
            None => continue,
        };
        let mut wpts: Vec<(f64, f64)> = Vec::new();
        for poly in &polys {
            for ring in poly {
                for &(lng, lat) in ring {
                    wpts.push(project(lng, lat));
                }
            }
        }
        if wpts.is_empty() {
            continue;
        }
        all.extend_from_slice(&wpts);
        feats.push(Feat { name, adcode, polys, wpts });
    }
    if all.is_empty() {
        return Ok(r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"></svg>"#.to_string());
    }

    let minx = all.iter().map(|p| p.0).fold(f64::MAX, f64::min);
    let maxx = all.iter().map(|p| p.0).fold(f64::MIN, f64::max);
    let miny = all.iter().map(|p| p.1).fold(f64::MAX, f64::min);
    let maxy = all.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    let bw = (maxx - minx).max(1e-12);
    let bh = (maxy - miny).max(1e-12);
    let aw = (w - 2.0 * pad).max(1.0);
    let ah = (h - 2.0 * pad).max(1.0);
    let scale = (aw / bw).min(ah / bh);
    let oxp = pad + (aw - bw * scale) / 2.0;
    let oyp = pad + (ah - bh * scale) / 2.0;
    let s = |wx: f64, wy: f64| (oxp + (wx - minx) * scale, oyp + (wy - miny) * scale);
    let total_area = bw * bh;

    let mut p: Vec<String> = Vec::new();
    p.push(format!(r#"<rect width="{w}" height="{h}" fill="{}"/>"#, st.bg));
    let mut stats = (0usize, 0usize); // (原始顶点, 保留顶点)

    // 陆地 + 三层边界
    p.push(r#"<g class="z1">"#.to_string());
    for (i, f) in feats.iter().enumerate() {
        if f.polys.is_empty() {
            continue;
        }
        let mut d = String::new();
        for poly in &f.polys {
            for ring in poly {
                if ring.len() < 3 {
                    continue;
                }
                let px: Vec<(f64, f64)> = ring.iter().map(|&(lng, lat)| s(project(lng, lat).0, project(lng, lat).1)).collect();
                stats.0 += px.len();
                let px = simplify(&px, 0.6);
                stats.1 += px.len();
                for (k, (x, y)) in px.iter().enumerate() {
                    d.push(if k == 0 { 'M' } else { 'L' });
                    d.push_str(&format!("{x:.1} {y:.1}"));
                }
                if !px.is_empty() {
                    d.push('Z');
                }
            }
        }
        if d.is_empty() {
            continue;
        }
        let fill = if i % 2 == 0 { st.land } else { st.land_alt };
        p.push(format!(
            r#"<g class="geo-region" data-adcode="{}" data-name="{}" role="button" tabindex="0">"#,
            esc(&f.adcode), esc(&f.name)
        ));
        p.push(format!(
            r#"<path class="geo-fill" d="{d}" fill="{fill}" fill-rule="evenodd" stroke="{}" stroke-width="{}" stroke-linejoin="round"/>"#,
            st.border, ls.border
        ));
        p.push(format!(
            r#"<path class="z3 geo-inner" d="{d}" fill="none" fill-rule="evenodd" stroke="{}" stroke-width="2.4" stroke-linejoin="round"/>"#,
            st.inner
        ));
        p.push(format!(
            r#"<path class="z2 geo-border" d="{d}" fill="none" fill-rule="evenodd" stroke="{}" stroke-width="{}" stroke-linejoin="round" pointer-events="none"/>"#,
            st.border2, ls.border2
        ));
        p.push("</g>".to_string());
    }
    p.push("</g>".to_string());

    // 标签与中心点
    if o.labels || o.dots {
        for f in &feats {
            // 面积（用 bbox 面积近似）
            let (mut fx0, mut fy0, mut fx1, mut fy1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for &(x, y) in &f.wpts {
                if x < fx0 { fx0 = x; }
                if y < fy0 { fy0 = y; }
                if x > fx1 { fx1 = x; }
                if y > fy1 { fy1 = y; }
            }
            let area = ((fx1 - fx0) * (fy1 - fy0)).max(0.0);
            if area < ls.min_area {
                continue;
            }
            let (cx, cy) = s((fx0 + fx1) / 2.0, (fy0 + fy1) / 2.0);
            if cx <= 0.0 || cx >= w || cy <= 0.0 || cy >= h {
                continue;
            }
            if o.dots && ls.show_dot {
                p.push(format!(
                    r#"<g class="z1"><circle cx="{cx:.1}" cy="{cy:.1}" r="{:.1}" fill="{}" stroke="{}" stroke-width="1.2"/></g>"#,
                    ls.dot_r, st.dot, st.text_halo
                ));
            }
            if !o.labels || f.name.trim().is_empty() {
                continue;
            }
            // 字号随面积微调（大省名牌更大）
            let rel = ((area / total_area.max(1e-12)).powf(0.18)).clamp(0.75, 1.6);
            let fs = ls.fs * rel;
            let show = if rel > 1.05 { "z1" } else { "z2" };
            let ty = cy + if ls.show_dot { ls.dot_r + fs * 0.85 } else { fs * 0.36 };
            p.push(format!(
                r#"<text class="{show}" x="{cx:.1}" y="{ty:.1}" font-size="{fs:.1}" text-anchor="middle" fill="{}" stroke="{}" stroke-width="3" paint-order="stroke" font-weight="{}">{}</text>"#,
                st.text, st.text_halo, if rel > 1.05 { 700 } else { 600 }, esc(&f.name)
            ));
        }
    }

    // 标题
    let title = match o.level.as_str() {
        "country" => "全国",
        "province" => "省域",
        "city" => "市域",
        _ => "区县",
    };
    p.push(format!(
        r#"<text class="z1" x="{:.0}" y="{:.0}" font-size="16" font-weight="700" fill="{}" stroke="{}" stroke-width="3" paint-order="stroke">{title} · {} 个区划</text>"#,
        pad - 6.0, pad - 10.0, st.text, st.text_halo, feats.len()
    ));
    let keep = if stats.0 > 0 { stats.1 as f64 / stats.0 as f64 * 100.0 } else { 100.0 };
    let tail = if o.show_stats {
        format!(" · 顶点 {}/{}（保留 {keep:.0}%）", stats.1, stats.0)
    } else {
        String::new()
    };
    p.push(format!(
        r#"<text class="z2" x="{:.0}" y="{:.0}" font-size="11.5" text-anchor="end" fill="{}">{} · SVG{tail}</text>"#,
        w - pad + 6.0, pad - 10.0, st.sub_text, o.level
    ));

    let css = format!(
        "text{{font-family:system-ui,-apple-system,'PingFang SC','Noto Sans CJK SC',sans-serif}}\
.geo-fill{{transition:fill .16s,filter .16s;cursor:pointer}}\
.geo-region:hover .geo-fill{{filter:brightness(1.07) saturate(1.15)}}\
.geo-region:hover .geo-border{{stroke-width:2.2;stroke:#79d9ff}}\
svg[data-zoom=\"1\"] .z2,svg[data-zoom=\"1\"] .z3{{display:none}}\
svg[data-zoom=\"2\"] .z3{{display:none}}"
    );

    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" data-zoom="{}" font-family="system-ui,sans-serif"><style>{css}</style>{}</svg>"#,
        o.zoom, p.join("")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fc() -> Value {
        json!({"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"name":"甲区","adcode":440103},
             "geometry":{"type":"Polygon","coordinates":[[[113.20,23.10],[113.25,23.10],[113.25,23.15],[113.20,23.15],[113.20,23.10]]]}},
            {"type":"Feature","properties":{"name":"乙区","adcode":440104},
             "geometry":{"type":"MultiPolygon","coordinates":[[[[113.25,23.10],[113.30,23.10],[113.30,23.15],[113.25,23.15],[113.25,23.10]]]]}}
        ]})
    }

    #[test]
    fn renders_valid_svg_with_regions() {
        let o = GeoOpts { level: "city".into(), ..Default::default() };
        let svg = render_geo_svg(&fc(), &o).unwrap();
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        assert_eq!(svg.matches(r#"class="geo-region""#).count(), 2, "两个区划各一个可点击组");
        assert!(svg.contains(r#"data-adcode="440103""#), "必须带 adcode 供下钻");
        assert!(svg.contains("甲区") && svg.contains("乙区"));
    }

    #[test]
    fn zoom_layers_present() {
        let svg = render_geo_svg(&fc(), &GeoOpts::default()).unwrap();
        assert!(svg.contains("data-zoom="));
        assert!(svg.contains("svg[data-zoom=\"1\"] .z2"), "应有缩放分层的 CSS");
        assert!(svg.contains("class=\"z1\"") || svg.contains("class=\"z2\""));
    }

    #[test]
    fn all_three_styles_distinct() {
        let mut bgs = std::collections::HashSet::new();
        for s in STYLE_NAMES {
            let o = GeoOpts { style: s.into(), ..Default::default() };
            let svg = render_geo_svg(&fc(), &o).unwrap();
            assert!(svg.contains(style_of(s).bg), "风格 {s} 底色应出现");
            bgs.insert(style_of(s).bg);
        }
        assert_eq!(bgs.len(), 3);
    }

    #[test]
    fn simplification_reduces_points() {
        // 一条密集直线（100 个几乎重合的点）
        let ring: Vec<(f64, f64)> = (0..100).map(|i| (i as f64 * 0.01, 0.0)).collect();
        let out = simplify(&ring, 0.6);
        assert!(out.len() < ring.len(), "抽稀应减少顶点：{} → {}", ring.len(), out.len());
        assert!(out.len() >= 3, "抽稀后仍应是合法多边形");
        // 首尾必须保留（闭合形状不能变形）
        assert_eq!(out[0], ring[0]);
        assert_eq!(*out.last().unwrap(), *ring.last().unwrap());
    }

    #[test]
    fn stats_line_when_requested() {
        let a = render_geo_svg(&fc(), &GeoOpts::default()).unwrap();
        let b = render_geo_svg(&fc(), &GeoOpts { show_stats: true, ..Default::default() }).unwrap();
        assert!(!a.contains("顶点"));
        assert!(b.contains("顶点"));
    }

    #[test]
    fn labels_and_dots_toggle() {
        // 用真实尺度的省域轮廓（约 5°×4°）：fc() 里的 0.05° 小区划会被
        // min_area 防糊阈值过滤掉标签，那是刻意的策略，本测试只验开关
        let big = json!({"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"name":"甲区","adcode":440103},
             "geometry":{"type":"Polygon","coordinates":[[[110.0,20.0],[115.0,20.0],[115.0,24.0],[110.0,24.0],[110.0,20.0]]]}},
            {"type":"Feature","properties":{"name":"乙区","adcode":440104},
             "geometry":{"type":"Polygon","coordinates":[[[115.0,20.0],[120.0,20.0],[120.0,24.0],[115.0,24.0],[115.0,20.0]]]}}
        ]});
        let with = render_geo_svg(&big, &GeoOpts { level: "province".into(), ..Default::default() }).unwrap();
        let without = render_geo_svg(&big, &GeoOpts { level: "province".into(), labels: false, dots: false, ..Default::default() }).unwrap();
        assert!(with.contains(">甲区<"), "开标签时应画出区划名文字");
        // data-name 是点击下钻必需的属性，即便关标签也要保留；
        // 这里断言的是「不再画名字文字」
        assert!(!without.contains(">甲区<"), "关掉标签后不该有区划名文字");
        assert!(without.contains(r#"data-name="甲区""#), "下钻属性必须保留");
    }

    #[test]
    fn empty_fc_is_safe() {
        let empty = json!({"type":"FeatureCollection","features":[]});
        let svg = render_geo_svg(&empty, &GeoOpts::default()).unwrap();
        assert!(svg.contains("<svg"));
    }

    #[test]
    fn names_escaped() {
        let bad = json!({"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"name":"A&B<C>","adcode":1},
             "geometry":{"type":"Polygon","coordinates":[[[113.2,23.1],[113.3,23.1],[113.3,23.2],[113.2,23.1]]]}}]});
        let svg = render_geo_svg(&bad, &GeoOpts::default()).unwrap();
        assert!(svg.contains("A&amp;B&lt;C&gt;"));
    }
}
