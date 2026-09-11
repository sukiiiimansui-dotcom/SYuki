//! 地图渲染（T1-2 的 Rust 版）
//!
//! 与 Python 侧不同，这里**输出 SVG 而不是 PNG**，原因：
//!   · Rust 侧渲染文字需要额外的字体库与字体文件，SVG 把文字交给 WebView 渲染，零依赖
//!   · 矢量在任意缩放下都清晰（用户要的「像高德/RPG 地图」本身就适合矢量）
//!   · 体积通常比 PNG 小一个量级，手机上省内存
//! 配色与 Python 的 render_map.py 保持一致（gaode / dark / water），另加 rpg。
use serde_json::Value;

use super::coord;

#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub bg: (u8, u8, u8),
    pub fill: (u8, u8, u8),
    pub stroke: (u8, u8, u8),
    pub line_width: f64,
    pub label: (u8, u8, u8),
    /// 相邻区块的填充微差（让拼接图不糊成一片）
    pub shade_step: i32,
}

pub fn style_of(name: &str) -> Style {
    match name {
        "dark" => Style {
            bg: (10, 22, 32),
            fill: (24, 42, 60),
            stroke: (90, 200, 255),
            line_width: 2.5,
            label: (185, 199, 218),
            shade_step: 8,
        },
        "water" => Style {
            bg: (244, 239, 227),
            fill: (250, 245, 232),
            stroke: (140, 150, 120),
            line_width: 1.8,
            label: (120, 110, 90),
            shade_step: 6,
        },
        "rpg" => Style {
            bg: (32, 28, 24),
            fill: (58, 50, 40),
            stroke: (214, 178, 108),
            line_width: 2.2,
            label: (238, 214, 160),
            shade_step: 10,
        },
        // 默认 gaode
        _ => Style {
            bg: (207, 227, 245),
            fill: (233, 231, 220),
            stroke: (255, 255, 255),
            line_width: 3.0,
            label: (51, 69, 95),
            shade_step: 6,
        },
    }
}

fn rgb(c: (u8, u8, u8)) -> String {
    format!("rgb({},{},{})", c.0, c.1, c.2)
}

fn shade(c: (u8, u8, u8), delta: i32) -> String {
    let f = |v: u8| -> u8 {
        let x = v as i32 + delta;
        x.clamp(0, 255) as u8
    };
    rgb((f(c.0), f(c.1), f(c.2)))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 一个环 → SVG path 片段
fn ring_to_path(ring: &Value, bb: coord::BBox, w: f64, h: f64, pad: f64) -> String {
    let mut s = String::new();
    let empty = vec![];
    let pts = ring.as_array().unwrap_or(&empty);
    let mut first = true;
    for p in pts {
        let arr = match p.as_array() {
            Some(a) if a.len() >= 2 => a,
            _ => continue,
        };
        let lng = arr[0].as_f64().unwrap_or(0.0);
        let lat = arr[1].as_f64().unwrap_or(0.0);
        let (x, y) = coord::lng_lat_to_canvas(lng, lat, bb, w, h, pad);
        if first {
            s.push_str(&format!("M{:.2} {:.2}", x, y));
            first = false;
        } else {
            s.push_str(&format!("L{:.2} {:.2}", x, y));
        }
    }
    if !first {
        s.push('Z');
    }
    s
}

/// 收集所有坐标点（世界坐标）
fn collect_points(fc: &Value, out: &mut Vec<(f64, f64)>) {
    let empty = vec![];
    for f in fc.get("features").and_then(|v| v.as_array()).unwrap_or(&empty) {
        if let Some(coords) = f.get("geometry").and_then(|g| g.get("coordinates")) {
            walk(coords, out);
        }
    }
}

fn walk(c: &Value, out: &mut Vec<(f64, f64)>) {
    if let Some(arr) = c.as_array() {
        if arr.len() >= 2 && arr[0].is_number() && arr[1].is_number() {
            out.push((
                arr[0].as_f64().unwrap_or(0.0),
                arr[1].as_f64().unwrap_or(0.0),
            ));
            return;
        }
        for x in arr {
            walk(x, out);
        }
    }
}

/// 把一个 FeatureCollection 渲染成 SVG 字符串
pub fn render_svg(
    fc: &Value,
    style_name: &str,
    w: f64,
    h: f64,
    pad: f64,
    labels: bool,
) -> Result<String, String> {
    let st = style_of(style_name);
    let mut pts: Vec<(f64, f64)> = Vec::new();
    collect_points(fc, &mut pts);
    if pts.is_empty() {
        return Err("没有几何数据".into());
    }
    let bb = coord::bbox_of_lng_lat(&pts).ok_or("无法计算包围盒")?;

    let mut body = String::new();
    let empty = vec![];
    let features = fc
        .get("features")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);
    for (i, f) in features.iter().enumerate() {
        let geom = match f.get("geometry") {
            Some(g) => g,
            None => continue,
        };
        let gtype = geom.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let coords = match geom.get("coordinates") {
            Some(c) => c,
            None => continue,
        };
        // 统一成 Vec<Vec<ring>>
        let polys: Vec<Vec<&Value>> = match gtype {
            "Polygon" => vec![coords
                .as_array()
                .map(|a| a.iter().collect::<Vec<_>>())
                .unwrap_or_default()],
            "MultiPolygon" => coords
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|poly| poly.as_array().map(|r| r.iter().collect()).unwrap_or_default())
                        .collect()
                })
                .unwrap_or_default(),
            _ => continue,
        };

        let mut d = String::new();
        for poly in &polys {
            for ring in poly {
                d.push_str(&ring_to_path(ring, bb, w, h, pad));
            }
        }
        if d.is_empty() {
            continue;
        }
        let fill = shade(st.fill, (i as i32 % 3 - 1) * st.shade_step);
        body.push_str(&format!(
            r#"<path d="{}" fill="{}" fill-rule="evenodd" stroke="{}" stroke-width="{}" stroke-linejoin="round"/>"#,
            d,
            fill,
            rgb(st.stroke),
            st.line_width
        ));

        if labels {
            let name = f
                .get("properties")
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !name.is_empty() && name.chars().count() <= 6 {
                // 标签锚点：要素 bbox 中心
                let mut fpts = Vec::new();
                walk(coords, &mut fpts);
                if let Some(fbb) = coord::bbox_of_lng_lat(&fpts) {
                    let cx = (fbb.0 + fbb.2) / 2.0;
                    let cy = (fbb.1 + fbb.3) / 2.0;
                    let (x, y) = coord::world_to_canvas(cx, cy, bb, w, h, pad);
                    let fs = (w.min(h) / 34.0).clamp(9.0, 17.0);
                    body.push_str(&format!(
                        r#"<text x="{:.1}" y="{:.1}" fill="{}" font-size="{:.1}" font-family="system-ui,'PingFang SC',sans-serif" text-anchor="middle" dominant-baseline="middle" paint-order="stroke" stroke="{}" stroke-width="2.5" stroke-opacity="0.55">{}</text>"#,
                        x,
                        y,
                        rgb(st.label),
                        fs,
                        rgb(st.bg),
                        esc(name)
                    ));
                }
            }
        }
    }

    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{:.0}" height="{:.0}" viewBox="0 0 {:.0} {:.0}"><rect width="100%" height="100%" fill="{}"/>{}</svg>"#,
        w,
        h,
        w,
        h,
        rgb(st.bg),
        body
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "type": "FeatureCollection",
            "features": [
                { "type": "Feature",
                  "properties": { "name": "甲区", "adcode": 1 },
                  "geometry": { "type": "Polygon",
                    "coordinates": [[[113.0,23.0],[113.5,23.0],[113.5,23.5],[113.0,23.5],[113.0,23.0]]] } },
                { "type": "Feature",
                  "properties": { "name": "乙区", "adcode": 2 },
                  "geometry": { "type": "MultiPolygon",
                    "coordinates": [[[[113.5,23.0],[114.0,23.0],[114.0,23.5],[113.5,23.5],[113.5,23.0]]]] } }
            ]
        })
    }

    #[test]
    fn renders_valid_svg() {
        let svg = render_svg(&sample(), "gaode", 800.0, 600.0, 30.0, true).unwrap();
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("<path"));
        assert_eq!(svg.matches("<path").count(), 2, "两个要素应画两条 path");
        assert!(svg.contains("甲区") && svg.contains("乙区"));
        assert!(svg.contains("viewBox=\"0 0 800 600\""));
    }

    #[test]
    fn all_styles_produce_distinct_bg() {
        let mut bgs = std::collections::HashSet::new();
        for s in ["gaode", "dark", "water", "rpg"] {
            let svg = render_svg(&sample(), s, 400.0, 300.0, 20.0, false).unwrap();
            let st = style_of(s);
            assert!(svg.contains(&rgb(st.bg)), "风格 {s} 的底色没出现");
            bgs.insert(rgb(st.bg));
        }
        assert_eq!(bgs.len(), 4, "四种风格底色应各不相同");
    }

    #[test]
    fn geometry_stays_inside_canvas() {
        let svg = render_svg(&sample(), "dark", 400.0, 300.0, 25.0, false).unwrap();
        // 解析所有 M/L 坐标，确认落在画布内（含 pad 余量）
        let mut maxx: f64 = 0.0;
        let mut maxy: f64 = 0.0;
        let mut minx = f64::MAX;
        let mut miny = f64::MAX;
        for cap in svg.split(|c| c == 'M' || c == 'L').skip(1) {
            let seg: String = cap.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ' ' || *c == '-').collect();
            let mut it = seg.split_whitespace();
            if let (Some(a), Some(b)) = (it.next(), it.next()) {
                if let (Ok(x), Ok(y)) = (a.parse::<f64>(), b.parse::<f64>()) {
                    maxx = maxx.max(x);
                    maxy = maxy.max(y);
                    minx = minx.min(x);
                    miny = miny.min(y);
                }
            }
        }
        assert!(minx >= 24.0 && miny >= 24.0, "左上越界: {minx},{miny}");
        assert!(maxx <= 376.0 && maxy <= 276.0, "右下越界: {maxx},{maxy}");
    }

    #[test]
    fn empty_geojson_errors() {
        let empty = json!({ "type": "FeatureCollection", "features": [] });
        assert!(render_svg(&empty, "gaode", 100.0, 100.0, 10.0, false).is_err());
    }

    #[test]
    fn label_escaped() {
        let fc = json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "properties": { "name": "A&B<C>" },
                "geometry": { "type": "Polygon",
                  "coordinates": [[[113.0,23.0],[113.2,23.0],[113.2,23.2],[113.0,23.0]]] }
            }]
        });
        let svg = render_svg(&fc, "gaode", 200.0, 200.0, 10.0, true).unwrap();
        assert!(svg.contains("A&amp;B&lt;C&gt;"), "名称里的 XML 特殊字符必须转义");
    }
}
