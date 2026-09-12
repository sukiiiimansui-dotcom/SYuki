//! 小区 SVG 渲染器（移植自 Python `svg_render.py`）
//!
//! 为什么是 SVG 而不是 PNG：
//!   · 车流/人流动画、缩放不糊、文字交给 WebView 渲染（中文字体问题从根上消失）
//!   · 生成几毫秒（PNG 要 100ms+），还能悬停/点击
//!
//! 缩放分层（前端切 `svg[data-zoom]`，纯 CSS 控制，不重绘）：
//!   z1 远景：底色 / 绿地 / 水域 / 主干道 / 地标名
//!   z2 中景：+ 次干道 / 路名 / 人行道 / 建筑与建筑名
//!   z3 近景：+ 斑马线 / 行道树 / 停车位 / 路灯 / 公交站 / 红绿灯 / 楼层数
use serde_json::Value;

use crate::world_map::details;

// ── 配色（与 Python 侧 district_gen.STYLES 对齐；RPG 风已按需求移除）──
#[derive(Clone, Copy)]
pub struct Style {
    pub bg: &'static str,
    pub blockbg: &'static str,
    pub park: &'static str,
    pub park_edge: &'static str,
    pub water: &'static str,
    pub water_edge: &'static str,
    pub road_face: &'static str,
    pub road_edge: &'static str,
    pub road_line: &'static str,
    pub path_face: &'static str,
    pub sidewalk: &'static str,
    pub parking: &'static str,
    pub text: &'static str,
    pub text_halo: &'static str,
    pub road_text: &'static str,
    pub sub_text: &'static str,
    pub tree: &'static str,
    pub tree_dark: &'static str,
    pub lamp: &'static str,
    pub bld_edge: &'static str,
    pub shadow: &'static str,
    /// 10 种建筑类型的填充色
    pub bld: [(&'static str, &'static str); 10],
}

pub const STYLE_NAMES: [&str; 3] = ["gaode", "dark", "water"];

pub fn style_of(name: &str) -> Style {
    match name {
        "dark" => Style {
            bg: "#121a26", blockbg: "#18222f",
            park: "#1e4637", park_edge: "#2a5c48", water: "#19375a", water_edge: "#2a5a8c",
            road_face: "#3c5070", road_edge: "#26364e", road_line: "#5a7ba8", path_face: "#2a3a52",
            sidewalk: "#2b3a4e", parking: "#27344a",
            text: "#c8dcf0", text_halo: "#0e1620", road_text: "#8caacd", sub_text: "#6d87a8",
            tree: "#2f6b4e", tree_dark: "#245440", lamp: "#ffd98a", bld_edge: "#5a8cbe",
            shadow: "rgba(0,0,0,.35)",
            bld: [
                ("residential", "#283c58"), ("office", "#374665"), ("commercial", "#3c4b46"),
                ("shop", "#46503f"), ("restaurant", "#554442"), ("cafe", "#504640"),
                ("school", "#32506e"), ("hospital", "#5a3c46"), ("civic", "#3a4254"),
                ("leisure", "#483c54"),
            ],
        },
        "water" => Style {
            bg: "#f4efe3", blockbg: "#faf5e8",
            park: "#dce8cc", park_edge: "#bcd0a8", water: "#cfe3ef", water_edge: "#a8c8dc",
            road_face: "#fdfaf2", road_edge: "#e0d8c4", road_line: "#d8c89c", path_face: "#f6f0e2",
            sidewalk: "#ece6d6", parking: "#e6dfcd",
            text: "#6a6252", text_halo: "#fdfaf2", road_text: "#8c8270", sub_text: "#a29880",
            tree: "#b8d4a0", tree_dark: "#96b880", lamp: "#e8c878", bld_edge: "#a89c88",
            shadow: "rgba(120,110,90,.18)",
            bld: [
                ("residential", "#e0d8c8"), ("office", "#d4d4cc"), ("commercial", "#e8d8b8"),
                ("shop", "#eee0c4"), ("restaurant", "#e8d0b0"), ("cafe", "#e4d4bc"),
                ("school", "#d0dce4"), ("hospital", "#e8d0d0"), ("civic", "#dcd8d0"),
                ("leisure", "#e0d4dc"),
            ],
        },
        // 默认 gaode
        _ => Style {
            bg: "#e9e7dc", blockbg: "#f2f0e8",
            park: "#c8e1be", park_edge: "#a9cf9c", water: "#bed7f0", water_edge: "#93b8e0",
            road_face: "#ffffff", road_edge: "#e2dfd4", road_line: "#e8c85a", path_face: "#f5f3ec",
            sidewalk: "#e0ddd2", parking: "#dcd9ce",
            text: "#3c4b5f", text_halo: "#ffffff", road_text: "#6e7889", sub_text: "#8d97a6",
            tree: "#8fc47f", tree_dark: "#6aa85c", lamp: "#f5d67a", bld_edge: "#9aa7b8",
            shadow: "rgba(60,75,95,.18)",
            bld: [
                ("residential", "#d6dce6"), ("office", "#d2dae8"), ("commercial", "#e6d7be"),
                ("shop", "#ebdec3"), ("restaurant", "#eed6ba"), ("cafe", "#e8d6c4"),
                ("school", "#c8dcf0"), ("hospital", "#f0d2d2"), ("civic", "#d8d6d0"),
                ("leisure", "#e2d0e2"),
            ],
        },
    }
}

const BIG_TYPES: [&str; 9] = [
    "office", "commercial", "shop", "restaurant", "cafe", "school", "hospital", "civic", "leisure",
];

fn fill_of(st: &Style, typ: &str) -> &'static str {
    st.bld
        .iter()
        .find(|(k, _)| *k == typ)
        .map(|(_, v)| *v)
        .unwrap_or(st.bld[0].1)
}

/// 把 #rrggbb 调亮/调暗（用于 3D 三面着色）
fn shade(hexcolor: &str, delta: i32) -> String {
    let h = hexcolor.trim_start_matches('#');
    if h.len() < 6 {
        return hexcolor.to_string();
    }
    let p = |i: usize| i32::from_str_radix(&h[i..i + 2], 16).unwrap_or(128);
    let f = |v: i32| (v + delta).clamp(0, 255);
    format!("#{:02x}{:02x}{:02x}", f(p(0)), f(p(2)), f(p(4)))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 估算文本宽度：中文按 1.0em、ASCII 按 0.55em
fn est_width(text: &str, size: f64) -> f64 {
    text.chars()
        .map(|c| if (c as u32) > 0x2E80 { size } else { size * 0.55 })
        .sum()
}

/// 按可用宽度裁剪文字（放不下返回空串）
fn fit(text: &str, size: f64, max_w: f64, min_chars: usize) -> String {
    let t = text.trim();
    if t.is_empty() {
        return String::new();
    }
    if est_width(t, size) <= max_w {
        return t.to_string();
    }
    let chars: Vec<char> = t.chars().collect();
    let mut n = chars.len();
    while n > min_chars && est_width(&chars[..n].iter().collect::<String>(), size) > max_w {
        n -= 1;
    }
    let out: String = chars[..n].iter().collect();
    if est_width(&out, size) <= max_w {
        out
    } else {
        String::new()
    }
}

fn num(v: &Value, k: &str, d: f64) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(d)
}

fn strs(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn arr(v: &Value, k: &str) -> Vec<Value> {
    v.get(k).and_then(|x| x.as_array()).cloned().unwrap_or_default()
}

/// 渲染选项
pub struct Opts {
    pub style: String,
    pub width: f64,
    pub height: f64,
    pub pad: f64,
    /// 初始缩放层级（1/2/3），只写进根元素 data-zoom，显隐由 CSS 决定
    pub zoom: i32,
    /// 是否画车流/人流/水波动画
    pub animate: bool,
    /// 是否给元素加 layer-* 类名（供前端图层开关）
    pub layers: bool,
    /// 2d 平面 / 3d 伪立体（按楼层挤出）
    pub mode: String,
    /// 是否画角落的数据卡片
    pub charts: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            style: "gaode".into(),
            width: 900.0,
            height: 900.0,
            pad: 48.0,
            zoom: 3,
            animate: true,
            layers: true,
            mode: "2d".into(),
            charts: false,
        }
    }
}

/// 渲染成自包含 SVG 字符串
pub fn render_svg(layout: &Value, o: &Opts) -> String {
    let st = style_of(&o.style);
    let is3d = o.mode.eq_ignore_ascii_case("3d");
    let size = num(layout, "size", 20.0).max(8.0);
    let (w, h, pad) = (o.width, o.height, o.pad);
    let cell = ((w - 2.0 * pad) / size).min((h - 2.0 * pad) / size);
    let ox = (w - cell * size) / 2.0;
    let oy = (h - cell * size) / 2.0;
    let gx = |x: f64| ox + x * cell;
    let gy = |y: f64| oy + y * cell;
    let lay = |name: &str| -> String {
        if o.layers {
            format!("layer-{name} ")
        } else {
            String::new()
        }
    };

    let mut p: Vec<String> = Vec::with_capacity(512);
    p.push(format!(r#"<rect width="{w}" height="{h}" fill="{}"/>"#, st.bg));
    p.push(format!(
        r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{}" rx="6"/>"#,
        ox, oy, cell * size, cell * size, st.blockbg
    ));

    // ── 绿地（z1）──
    for pk in arr(layout, "parks") {
        let (x, y) = (gx(num(&pk, "x", 0.0)), gy(num(&pk, "y", 0.0)));
        let (bw, bh) = (num(&pk, "w", 2.0) * cell, num(&pk, "h", 2.0) * cell);
        p.push(format!(
            r#"<g class="z1 {}"><rect x="{x:.1}" y="{y:.1}" width="{bw:.1}" height="{bh:.1}" rx="8" fill="{}" stroke="{}" stroke-width="1.5"/>"#,
            lay("park"), st.park, st.park_edge
        ));
        // 公园里的树丛（近景才显示）
        let n = ((bw * bh) / (cell * cell) * 1.2) as i32;
        if n > 0 {
            let mut dots = String::new();
            for i in 0..n.min(14) {
                let t = (i as f64 + 1.0) / (n.min(14) as f64 + 1.0);
                let tx = x + t * bw * 0.72 + bw * 0.14;
                let ty = y + ((i as f64 * 0.618).fract()) * bh * 0.7 + bh * 0.15;
                let rr = 3.0 + ((i % 3) as f64) * 1.1;
                dots.push_str(&format!(
                    r#"<circle cx="{tx:.1}" cy="{ty:.1}" r="{rr:.1}" fill="{}" opacity=".7"/>"#,
                    st.tree_dark
                ));
            }
            p.push(format!(r#"<g class="z3">{dots}</g>"#));
        }
        p.push("</g>".into());
    }

    // ── 水域（z1）──
    for wt in arr(layout, "water") {
        let (x, y) = (gx(num(&wt, "x", 0.0)), gy(num(&wt, "y", 0.0)));
        let (bw, bh) = (num(&wt, "w", 2.0) * cell, num(&wt, "h", 2.0) * cell);
        p.push(format!(
            r#"<rect class="z1 {} wm-water" x="{x:.1}" y="{y:.1}" width="{bw:.1}" height="{bh:.1}" rx="10" fill="{}" stroke="{}" stroke-width="1.5"/>"#,
            lay("water"), st.water, st.water_edge
        ));
    }

    // ── 人行道（z2）──
    for sw in details::list(layout, "sidewalks") {
        let (x1, y1) = (gx(num(sw, "x1", 0.0)), gy(num(sw, "y1", 0.0)));
        let (x2, y2) = (gx(num(sw, "x2", 0.0)), gy(num(sw, "y2", 0.0)));
        let lw = (num(sw, "w", 0.26) * cell).max(2.0);
        p.push(format!(
            r#"<line class="z2 {}" x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{}" stroke-width="{lw:.1}" stroke-linecap="butt"/>"#,
            lay("sidewalk"), st.sidewalk
        ));
    }

    // ── 道路（主干道 z1 / 其余 z2）──
    let roads = arr(layout, "roads");
    let mut road_anim: Vec<(f64, f64, f64, f64, f64, usize, String)> = Vec::new();
    for (i, r) in roads.iter().enumerate() {
        let (x1, y1) = (gx(num(r, "x1", 0.0)), gy(num(r, "y1", 0.0)));
        let (x2, y2) = (gx(num(r, "x2", 0.0)), gy(num(r, "y2", 0.0)));
        let typ = strs(r, "type");
        let typ = if typ.is_empty() { "secondary".to_string() } else { typ };
        let z = if typ == "main" { "z1" } else { "z2" };
        let ratio = match typ.as_str() {
            "main" => 0.62,
            "secondary" => 0.40,
            _ => 0.20,
        };
        let lw = (cell * ratio).max(4.0);
        if typ == "path" {
            p.push(format!(
                r#"<line class="{z} {}" x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{}" stroke-width="{lw:.1}" stroke-linecap="round" stroke-dasharray="6 5"/>"#,
                lay("road"), st.path_face
            ));
            continue;
        }
        let mut g = format!(r#"<g class="{z} {} wm-road" data-road="{}" data-type="{}">"#, lay("road"), esc(&strs(r, "name")), typ);
        g.push_str(&format!(
            r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{}" stroke-width="{:.1}" stroke-linecap="round"/>"#,
            st.road_edge, lw + 3.0
        ));
        g.push_str(&format!(
            r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{}" stroke-width="{lw:.1}" stroke-linecap="round"/>"#,
            st.road_face
        ));
        if typ == "main" {
            g.push_str(&format!(
                r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{}" stroke-width="1.6" stroke-dasharray="9 7" opacity=".85"/>"#,
                st.road_line
            ));
        }
        g.push_str("</g>");
        p.push(g);
        if o.animate && (typ == "main" || typ == "secondary") {
            road_anim.push((x1, y1, x2, y2, lw, i, typ));
        }
    }

    // ── 斑马线（z3）──
    for c in details::list(layout, "crosswalks") {
        let (x1, y1) = (gx(num(c, "x1", 0.0)), gy(num(c, "y1", 0.0)));
        let (x2, y2) = (gx(num(c, "x2", 0.0)), gy(num(c, "y2", 0.0)));
        p.push(format!(
            r##"<line class="z3 {}" x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="#ffffff" stroke-width="{:.1}" opacity=".92"/>"##,
            lay("crosswalk"), (cell * 0.10).max(2.0)
        ));
    }

    // ── 停车位（z3）──
    for pk in details::list(layout, "parking") {
        let (x, y) = (gx(num(&pk, "x", 0.0)), gy(num(&pk, "y", 0.0)));
        let (bw, bh) = (num(&pk, "w", 0.5) * cell, num(&pk, "h", 0.5) * cell);
        p.push(format!(
            r##"<rect class="z3 {}" x="{x:.1}" y="{y:.1}" width="{bw:.1}" height="{bh:.1}" rx="2" fill="{}" stroke="#ffffff" stroke-width="1" opacity=".95"/>"##,
            lay("parking"), st.parking
        ));
    }

    // ── 建筑（z2；3d 模式按楼层挤出）──
    let mut blds = arr(layout, "buildings");
    if is3d {
        // 伪 3D 要按 y 从小到大画（后面的先画，前面的覆盖上去）
        blds.sort_by(|a, b| num(a, "y", 0.0).partial_cmp(&num(b, "y", 0.0)).unwrap_or(std::cmp::Ordering::Equal));
    }
    for b in &blds {
        let (x, y) = (gx(num(b, "x", 0.0)), gy(num(b, "y", 0.0)));
        let (bw, bh) = (num(b, "w", 2.0) * cell, num(b, "h", 2.0) * cell);
        let typ = {
            let t = strs(b, "type");
            if t.is_empty() { "residential".to_string() } else { t }
        };
        let fill = fill_of(&st, &typ);
        let is_big = BIG_TYPES.contains(&typ.as_str()) || (bw > cell * 2.4 && bh > cell * 1.8);
        let nm = strs(b, "name");
        let fl_raw = b.get("floors").cloned().unwrap_or(Value::Null);
        let fl = fl_raw.as_f64().or_else(|| fl_raw.as_str().and_then(|s| s.parse().ok())).unwrap_or(3.0);

        p.push(format!(
            r#"<g class="z2 {} wm-bld" data-type="{}" data-name="{}" data-floors="{}" data-w="{}" data-h="{}" role="button" tabindex="0">"#,
            lay("bld"), esc(&typ), esc(&nm),
            fl_raw.as_str().map(|s| s.to_string()).unwrap_or_else(|| if fl_raw.is_null() { String::new() } else { format!("{fl:.0}") }),
            num(b, "w", 1.0) as i64, num(b, "h", 1.0) as i64
        ));
        if is3d {
            let hz = (fl * 1.7).clamp(3.0, 44.0);
            let (dx, dy) = (hz * 0.40, hz * 0.62);
            // 右侧面（暗）
            p.push(format!(
                r#"<polygon points="{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}" fill="{}" stroke="{}" stroke-width="1"/>"#,
                x + bw, y, x + bw + dx, y - dy, x + bw + dx, y + bh - dy, x + bw, y + bh,
                shade(fill, -34), st.bld_edge
            ));
            // 下侧面（更暗）
            p.push(format!(
                r#"<polygon points="{x:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}" fill="{}" stroke="{}" stroke-width="1"/>"#,
                y + bh, x + dx, y + bh - dy, x + bw + dx, y + bh - dy, x + bw, y + bh,
                shade(fill, -52), st.bld_edge
            ));
            // 顶面（亮）
            p.push(format!(
                r#"<rect x="{:.1}" y="{:.1}" width="{bw:.1}" height="{bh:.1}" rx="2" fill="{}" stroke="{}" stroke-width="1"/>"#,
                x + dx, y - dy, shade(fill, 22), st.bld_edge
            ));
            // 正面
            p.push(format!(
                r#"<rect x="{x:.1}" y="{y:.1}" width="{bw:.1}" height="{bh:.1}" rx="2" fill="{fill}" stroke="{}" stroke-width="1.2"/>"#,
                st.bld_edge
            ));
        } else {
            p.push(format!(
                r#"<rect x="{:.1}" y="{:.1}" width="{bw:.1}" height="{bh:.1}" rx="3" fill="{}"/>"#,
                x + 2.0, y + 3.0, st.shadow
            ));
            p.push(format!(
                r#"<rect x="{x:.1}" y="{y:.1}" width="{bw:.1}" height="{bh:.1}" rx="3" fill="{fill}" stroke="{}" stroke-width="1.4"/>"#,
                st.bld_edge
            ));
            if bh > 14.0 {
                p.push(format!(
                    r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{}" stroke-width="1" opacity=".5"/>"#,
                    x + bw * 0.18, y + bh * 0.34, x + bw * 0.82, y + bh * 0.34, st.bld_edge
                ));
            }
        }
        // 名称（分级：重要建筑全名 / 住宅栋号）
        if !nm.trim().is_empty() && cell > 10.0 {
            let fs = if is_big { (cell * 0.46).clamp(10.0, 14.0) } else { (cell * 0.38).clamp(10.0, 11.5) };
            let room = if is_big { bw - 6.0 } else { bw - 4.0 };
            let maxw = if is_big { room.max(26.0) } else { room };
            let label = fit(&nm, fs, maxw, 2);
            if !label.is_empty() {
                let tw = est_width(&label, fs);
                p.push(format!(
                    r#"<text x="{:.1}" y="{:.1}" font-size="{fs:.1}" text-anchor="middle" fill="{}" stroke="{}" stroke-width="2.6" paint-order="stroke" font-weight="{}">{}</text>"#,
                    x + bw / 2.0 - tw / 2.0, y + bh / 2.0 + fs * 0.36,
                    st.text, st.text_halo, if is_big { 700 } else { 500 }, esc(&label)
                ));
            }
        }
        // 楼层数（z3，近景）
        if !fl_raw.is_null() && cell > 16.0 {
            p.push(format!(
                r#"<text class="z3" x="{:.1}" y="{:.1}" font-size="{:.1}" text-anchor="end" fill="{}" stroke="{}" stroke-width="2" paint-order="stroke">{}F</text>"#,
                x + bw - 3.0, y + bh - 3.0, (cell * 0.26).max(8.5), st.sub_text, st.text_halo, fl as i64
            ));
        }
        p.push("</g>".into());
    }

    // ── 行道树（z3）──
    for t in details::list(layout, "trees") {
        let (cx, cy) = (gx(num(t, "x", 0.0)), gy(num(t, "y", 0.0)));
        let r = (num(t, "r", 0.2) * cell).max(3.0);
        p.push(format!(
            r#"<g class="z3 {}"><circle cx="{cx:.1}" cy="{cy:.1}" r="{r:.1}" fill="{}" opacity=".9"/><circle cx="{:.1}" cy="{:.1}" r="{:.1}" fill="{}" opacity=".95"/></g>"#,
            lay("tree"), st.tree_dark, cx - r * 0.25, cy - r * 0.25, r * 0.62, st.tree
        ));
    }

    // ── 路灯（z3，带光晕）──
    for lp in details::list(layout, "lamps") {
        let (cx, cy) = (gx(num(lp, "x", 0.0)), gy(num(lp, "y", 0.0)));
        p.push(format!(
            r#"<g class="z3 {}"><circle cx="{cx:.1}" cy="{cy:.1}" r="{:.1}" fill="{}" opacity=".22"/><circle cx="{cx:.1}" cy="{cy:.1}" r="{:.1}" fill="{}"/></g>"#,
            lay("lamp"), (cell * 0.20).max(4.5), st.lamp, (cell * 0.075).max(1.8), st.lamp
        ));
    }

    // ── 红绿灯（z3）──
    for sg in details::list(layout, "signals") {
        let (cx, cy) = (gx(num(sg, "x", 0.0)), gy(num(sg, "y", 0.0)));
        let r = (cell * 0.09).max(2.2);
        p.push(format!(
            r##"<g class="z3 {}"><circle cx="{:.1}" cy="{cy:.1}" r="{r:.1}" fill="#e05a5a"/><circle cx="{cx:.1}" cy="{cy:.1}" r="{r:.1}" fill="#e0c05a"/><circle cx="{:.1}" cy="{cy:.1}" r="{r:.1}" fill="#5ac07a"/></g>"##,
            lay("signal"), cx - r * 1.6, cx + r * 1.6
        ));
    }

    // ── 公交站（z3）──
    for bs in details::list(layout, "busStops") {
        let (cx, cy) = (gx(num(bs, "x", 0.0)), gy(num(bs, "y", 0.0)));
        let (bw, bh) = ((cell * 0.72).max(10.0), (cell * 0.34).max(7.0));
        let (bx, by) = (cx - bw / 2.0, cy - bh / 2.0);
        let lbus = lay("bus");
        p.push(format!(
            r##"<g class="z3 {lbus}"><rect x="{bx:.1}" y="{by:.1}" width="{bw:.1}" height="{bh:.1}" rx="2.5" fill="#4a90d9" stroke="#ffffff" stroke-width="1"/></g>"##
        ));
    }

    // ── 路名（z2）──
    for r in &roads {
        let nm = strs(r, "name");
        if nm.trim().is_empty() {
            continue;
        }
        let (x1, y1) = (gx(num(r, "x1", 0.0)), gy(num(r, "y1", 0.0)));
        let (x2, y2) = (gx(num(r, "x2", 0.0)), gy(num(r, "y2", 0.0)));
        let (mx, my) = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
        let horiz = (x2 - x1).abs() >= (y2 - y1).abs();
        let fs = (cell * 0.42).clamp(10.0, 13.0);
        let span = if horiz { (x2 - x1).abs() } else { (y2 - y1).abs() };
        let label = fit(&nm, fs, span * 0.82, 2);
        if label.is_empty() {
            continue;
        }
        let tw = est_width(&label, fs);
        let rot = if horiz { 0 } else { 90 };
        p.push(format!(
            r#"<g class="z2" transform="translate({mx:.1},{my:.1}) rotate({rot})"><rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="4" fill="{}" opacity=".82"/><text x="0" y="{:.1}" font-size="{fs:.1}" text-anchor="middle" fill="{}" font-weight="600">{}</text></g>"#,
            -tw / 2.0 - 4.0, -fs * 0.78, tw + 8.0, fs * 1.5,
            st.bg, fs * 0.36, st.road_text, esc(&label)
        ));
    }

    // ── 公园/水体名（z1）──
    for key in ["parks", "water"] {
        for it in arr(layout, key) {
            let nm = strs(&it, "name");
            if nm.trim().is_empty() {
                continue;
            }
            let (x, y) = (gx(num(&it, "x", 0.0)), gy(num(&it, "y", 0.0)));
            let bw = num(&it, "w", 2.0) * cell;
            let bh = num(&it, "h", 2.0) * cell;
            let fs = (cell * 0.4).clamp(10.0, 13.0);
            let label = fit(&nm, fs, bw - 4.0, 2);
            if label.is_empty() {
                continue;
            }
            p.push(format!(
                r#"<text class="z1" x="{:.1}" y="{:.1}" font-size="{fs:.1}" text-anchor="middle" fill="{}" stroke="{}" stroke-width="2.4" paint-order="stroke" font-weight="600">{}</text>"#,
                x + bw / 2.0, y + bh / 2.0 + fs * 0.36, st.text, st.text_halo, esc(&label)
            ));
        }
    }

    // ── 动画：车流（主干道）+ 人流（反向慢走）──
    if o.animate && !road_anim.is_empty() {
        let mut cars = String::new();
        for (x1, y1, x2, y2, lw, i, typ) in road_anim.iter().take(16) {
            let dist = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
            let dur = (dist / if typ == "main" { 60.0 } else { 42.0 }).clamp(4.0, 16.0);
            let delay = (*i % 9) as f64 * 0.55;
            let color = ["#ffd166", "#8fd6ff", "#ffffff"][*i % 3];
            let rdot = (lw * 0.19).max(2.4);
            cars.push_str(&format!(
                r#"<circle r="{rdot:.1}" fill="{color}" opacity=".9"><animateMotion dur="{dur:.1}s" repeatCount="indefinite" begin="{delay:.1}s" path="M{x1:.1},{y1:.1} L{x2:.1},{y2:.1}"/></circle>"#
            ));
        }
        p.push(format!(r#"<g class="z2 wm-cars">{cars}</g>"#));

        let mut peds = String::new();
        for (x1, y1, x2, y2, lw, i, _) in road_anim.iter().take(8) {
            let dist = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
            let dur = (dist / 26.0).clamp(7.0, 22.0);
            let delay = (*i % 5) as f64 * 1.1;
            let rped = (lw * 0.11).max(1.6);
            peds.push_str(&format!(
                r##"<circle r="{rped:.1}" fill="#ffffff" opacity=".72"><animateMotion dur="{dur:.1}s" repeatCount="indefinite" begin="{delay:.1}s" path="M{x2:.1},{y2:.1} L{x1:.1},{y1:.1}"/></circle>"##
            ));
        }
        p.push(format!(r#"<g class="z3 wm-peds">{peds}</g>"#));
    }

    // ── 数据卡片（可选）──
    if o.charts {
        p.push(charts_svg(layout, &st, w, pad));
    }

    // ── 标题 ──
    let title = layout.get("name").and_then(|v| v.as_str()).unwrap_or("小区");
    p.push(format!(
        r#"<text class="z1" x="{:.0}" y="{:.0}" font-size="17" font-weight="700" fill="{}" stroke="{}" stroke-width="3" paint-order="stroke">{}</text>"#,
        pad - 8.0, pad - 18.0, st.text, st.text_halo, esc(title)
    ));
    let nb = arr(layout, "buildings").len();
    let nr = arr(layout, "roads").len();
    let kind = if layout.get("_sketch").and_then(|v| v.as_bool()).unwrap_or(false) { "草图" } else { "AI 精细版" };
    let stt = details::stats(layout);
    let extra = if o.layers && !stt.is_empty() {
        format!(" · {}🌳 {}🅿", stt.get("trees").and_then(|v| v.as_u64()).unwrap_or(0), stt.get("parking").and_then(|v| v.as_u64()).unwrap_or(0))
    } else {
        String::new()
    };
    p.push(format!(
        r#"<text class="z1" x="{:.0}" y="{:.0}" font-size="12" text-anchor="end" fill="{}">{nb} 栋 · {nr} 路 · {kind}{extra}</text>"#,
        w - pad + 8.0, pad - 18.0, st.sub_text
    ));

    // ── CSS ──
    let mut css = String::from(
        "text{font-family:system-ui,-apple-system,'PingFang SC','Noto Sans CJK SC',sans-serif}\
.wm-bld{cursor:pointer}.wm-bld rect{transition:filter .14s,stroke-width .14s}\
.wm-bld:hover rect{filter:brightness(1.14)}.wm-bld.on rect{stroke:#ffd166;stroke-width:2.6}\
.wm-road:hover line{filter:brightness(1.1)}\
svg[data-zoom=\"1\"] .z2,svg[data-zoom=\"1\"] .z3{display:none}\
svg[data-zoom=\"2\"] .z3{display:none}",
    );
    if o.animate {
        css.push_str(".wm-water{animation:wmwave 6s ease-in-out infinite}@keyframes wmwave{0%,100%{opacity:.9}50%{opacity:1}}");
    }

    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.0} {h:.0}" data-zoom="{}" font-family="system-ui,sans-serif"><style>{css}</style>{}</svg>"#,
        o.zoom, p.join("")
    )
}

/// 角落数据卡片（容积率/建筑密度/人口 + 建筑类型条形图）
fn charts_svg(layout: &Value, st: &Style, w: f64, pad: f64) -> String {
    let s = crate::world_map::stats::compute(layout);
    let rows = crate::world_map::stats::type_series(&s);
    let n = rows.len().min(6);
    let (cw, ch) = (214.0, 26.0 + 40.0 + n as f64 * 15.0 + 14.0);
    let (cx0, cy0) = (w - pad - cw + 8.0, pad + 6.0);
    let mut o = String::new();
    o.push_str(r#"<g class="z1 wm-charts">"#);
    o.push_str(&format!(
        r#"<rect x="{cx0:.0}" y="{cy0:.0}" width="{cw:.0}" height="{ch:.0}" rx="10" fill="{}" opacity=".93" stroke="{}" stroke-width="1.2"/>"#,
        st.bg, st.bld_edge
    ));
    o.push_str(&format!(
        r#"<text x="{:.0}" y="{:.0}" font-size="12.5" font-weight="700" fill="{}">📊 小区数据</text>"#,
        cx0 + 10.0, cy0 + 18.0, st.text
    ));
    let kpis = [
        ("容积率", format!("{}", s["plotRatio"])),
        ("建筑密度", format!("{}%", s["buildingDensity"])),
        ("人口估算", format!("{}", s["popEstimate"])),
    ];
    for (i, (k, v)) in kpis.iter().enumerate() {
        let bx = cx0 + 10.0 + i as f64 * 68.0;
        o.push_str(&format!(
            r#"<text x="{bx:.0}" y="{:.0}" font-size="9.5" fill="{}">{k}</text>"#,
            cy0 + 36.0, st.sub_text
        ));
        o.push_str(&format!(
            r#"<text x="{bx:.0}" y="{:.0}" font-size="13" font-weight="700" fill="{}">{v}</text>"#,
            cy0 + 49.0, st.text
        ));
    }
    let maxc = rows.iter().map(|r| r.2).max().unwrap_or(1).max(1);
    let mut by = cy0 + 58.0;
    for (typ, zh, c, _fa) in rows.iter().take(6) {
        let col = fill_of(st, typ);
        let bw = ((*c as f64 / maxc as f64) * 96.0).max(4.0);
        o.push_str(&format!(
            r#"<text x="{:.0}" y="{:.0}" font-size="10" fill="{}">{}</text>"#,
            cx0 + 10.0, by + 9.0, st.text, esc(zh)
        ));
        o.push_str(&format!(
            r#"<rect x="{:.0}" y="{by:.0}" width="{bw:.1}" height="10" rx="2" fill="{col}" stroke="{}" stroke-width=".8"/>"#,
            cx0 + 52.0, st.bld_edge
        ));
        o.push_str(&format!(
            r#"<text x="{:.0}" y="{:.0}" font-size="9.5" fill="{}">{c}</text>"#,
            cx0 + 52.0 + bw + 5.0, by + 9.0, st.sub_text
        ));
        by += 15.0;
    }
    o.push_str("</g>");
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lay() -> Value {
        let mut l = crate::world_map::sketch::make_sketch("测试小区", 20, Some(7), None);
        for (i, b) in l["buildings"].as_array_mut().unwrap().iter_mut().enumerate() {
            b["name"] = json!(if b["type"] == "residential" { format!("{}号楼", i + 1) } else { "云顶大厦".to_string() });
            b["floors"] = json!(if b["type"] == "office" { 18 } else { 6 });
        }
        for (i, r) in l["roads"].as_array_mut().unwrap().iter_mut().enumerate() {
            r["name"] = json!(["中山路", "解放大道", "文明路"][i % 3]);
        }
        for p in l["parks"].as_array_mut().unwrap().iter_mut() {
            p["name"] = json!("中心公园");
        }
        crate::world_map::details::enrich(&mut l);
        l
    }

    #[test]
    fn renders_valid_svg() {
        let svg = render_svg(&lay(), &Opts::default());
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("viewBox=\"0 0 900 900\""));
        assert!(svg.contains("data-zoom=\"3\""));
    }

    #[test]
    fn contains_buildings_roads_and_details() {
        let svg = render_svg(&lay(), &Opts::default());
        assert!(svg.contains("wm-bld"), "应有建筑组");
        assert!(svg.contains("wm-road"), "应有道路组");
        assert!(svg.contains("layer-tree"), "应有行道树图层");
        assert!(svg.contains("layer-parking"), "应有停车位图层");
        assert!(svg.contains("layer-crosswalk"), "应有斑马线图层");
    }

    #[test]
    fn layers_flag_toggles_classnames() {
        let on = render_svg(&lay(), &Opts { layers: true, ..Default::default() });
        let off = render_svg(&lay(), &Opts { layers: false, ..Default::default() });
        assert!(on.contains("layer-tree"));
        assert!(!off.contains("layer-tree"));
    }

    #[test]
    fn three_d_adds_polygons() {
        let flat = render_svg(&lay(), &Opts::default());
        let d3 = render_svg(&lay(), &Opts { mode: "3d".into(), ..Default::default() });
        assert_eq!(flat.matches("<polygon").count(), 0, "2D 不该有多边形");
        assert!(d3.matches("<polygon").count() > 10, "3D 应有侧面多边形");
        assert_eq!(flat.matches("wm-bld").count(), d3.matches("wm-bld").count(), "建筑数应一致");
    }

    #[test]
    fn animation_present_and_removable() {
        let with = render_svg(&lay(), &Opts { animate: true, ..Default::default() });
        let without = render_svg(&lay(), &Opts { animate: false, ..Default::default() });
        assert!(with.contains("animateMotion"), "应有车流/人流动画");
        assert!(!without.contains("animateMotion"));
        assert!(!without.contains("@keyframes"));
    }

    #[test]
    fn charts_card_optional() {
        let off = render_svg(&lay(), &Opts::default());
        let on = render_svg(&lay(), &Opts { charts: true, ..Default::default() });
        assert!(!off.contains("wm-charts"));
        assert!(on.contains("wm-charts"));
        assert!(on.contains("容积率") && on.contains("建筑密度") && on.contains("人口估算"));
    }

    #[test]
    fn all_three_styles_distinct() {
        let mut bgs = std::collections::HashSet::new();
        for s in STYLE_NAMES {
            let svg = render_svg(&lay(), &Opts { style: s.into(), ..Default::default() });
            let st = style_of(s);
            assert!(svg.contains(st.bg), "风格 {s} 的底色应出现");
            bgs.insert(st.bg);
        }
        assert_eq!(bgs.len(), 3, "三种风格底色应各不相同");
    }

    #[test]
    fn names_escaped() {
        let mut l = lay();
        l["buildings"][0]["name"] = json!("A&B<C>");
        let svg = render_svg(&l, &Opts::default());
        assert!(svg.contains("A&amp;B&lt;C&gt;"), "名称里的 XML 特殊字符必须转义");
    }

    #[test]
    fn empty_layout_does_not_panic() {
        let empty = json!({"name":"空","size":20,"buildings":[],"roads":[],"parks":[],"water":[]});
        let svg = render_svg(&empty, &Opts::default());
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
    }

    #[test]
    fn zoom_level_written_to_root() {
        for z in 1..=3 {
            let svg = render_svg(&lay(), &Opts { zoom: z, ..Default::default() });
            assert!(svg.contains(&format!("data-zoom=\"{z}\"")));
        }
    }
}
