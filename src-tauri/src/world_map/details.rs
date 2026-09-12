//! 街道细节生成（移植自 Python `details.py`）
//!
//! 人行道 / 斑马线 / 行道树 / 停车位 / 路灯 / 公交站 / 红绿灯 —— 全部由路网**规则推导**，
//! 不让 AI 画（AI 擅长语义、不擅长精确坐标）。纯几何，无外部依赖。
//!
//! 坐标系：与布局一致，网格坐标 0..size。
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use serde_json::{json, Map, Value};

/// 一格约多少米（与 Python 侧 GRID_M 对齐）
pub const GRID_M: f64 = 18.0;

type Rect = (f64, f64, f64, f64);

fn num(v: &Value, key: &str, dflt: f64) -> f64 {
    v.get(key).and_then(|x| x.as_f64()).unwrap_or(dflt)
}

fn arr(v: &Value, key: &str) -> Vec<Value> {
    v.get(key).and_then(|x| x.as_array()).cloned().unwrap_or_default()
}

/// 确定性随机源：同一布局永远得到同一批细节
fn rng_of(layout: &Value) -> StdRng {
    let name = layout.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let size = num(layout, "size", 20.0);
    let d = md5::compute(format!("{name}|{size}|detail").as_bytes());
    let mut seed = [0u8; 32];
    for i in 0..32 {
        seed[i] = d.0[i % 16];
    }
    StdRng::from_seed(seed)
}

fn rects_of(layout: &Value) -> Vec<Rect> {
    arr(layout, "buildings")
        .iter()
        .filter_map(|b| {
            Some((
                num(b, "x", 0.0),
                num(b, "y", 0.0),
                num(b, "w", 1.0),
                num(b, "h", 1.0),
            ))
        })
        .collect()
}

fn blocks_rects_of(layout: &Value) -> Vec<Rect> {
    let mut out = Vec::new();
    for key in ["parks", "water"] {
        for it in arr(layout, key) {
            out.push((
                num(&it, "x", 0.0),
                num(&it, "y", 0.0),
                num(&it, "w", 1.0),
                num(&it, "h", 1.0),
            ));
        }
    }
    out
}

fn hit(x: f64, y: f64, rects: &[Rect], pad: f64) -> bool {
    rects.iter().any(|(rx, ry, rw, rh)| {
        x >= rx - pad && x <= rx + rw + pad && y >= ry - pad && y <= ry + rh + pad
    })
}

fn seg_h(r: &Value) -> bool {
    (num(r, "y2", 0.0) - num(r, "y1", 0.0)).abs() < 1e-9
}

fn seg_v(r: &Value) -> bool {
    (num(r, "x2", 0.0) - num(r, "x1", 0.0)).abs() < 1e-9
}

/// 规范成 (x1,y1,x2,y2) 且 x1<=x2 / y1<=y2
fn norm(r: &Value) -> (f64, f64, f64, f64) {
    let (mut x1, mut y1) = (num(r, "x1", 0.0), num(r, "y1", 0.0));
    let (mut x2, mut y2) = (num(r, "x2", 0.0), num(r, "y2", 0.0));
    if x1 > x2 {
        std::mem::swap(&mut x1, &mut x2);
    }
    if y1 > y2 {
        std::mem::swap(&mut y1, &mut y2);
    }
    (x1, y1, x2, y2)
}

fn width_of(r: &Value) -> f64 {
    match r.get("type").and_then(|v| v.as_str()).unwrap_or("secondary") {
        "main" => 0.62,
        "secondary" => 0.40,
        _ => 0.20,
    }
}

fn is_paved(r: &Value) -> bool {
    r.get("type").and_then(|v| v.as_str()).unwrap_or("secondary") != "path"
}

/// 横竖铺装道路的交点（网格坐标）
pub fn intersections(layout: &Value) -> Vec<(f64, f64)> {
    let roads: Vec<Value> = arr(layout, "roads").into_iter().filter(is_paved).collect();
    let hs: Vec<_> = roads.iter().filter(|r| seg_h(r)).map(norm).collect();
    let vs: Vec<_> = roads.iter().filter(|r| seg_v(r)).map(norm).collect();
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (hx1, hy, hx2, _) in &hs {
        for (vx, vy1, _, vy2) in &vs {
            if *hx1 - 0.01 <= *vx && *vx <= *hx2 + 0.01 && *vy1 - 0.01 <= *hy && *hy <= *vy2 + 0.01 {
                let p = ((vx * 1000.0).round() / 1000.0, (hy * 1000.0).round() / 1000.0);
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// 生成全部街道细节，返回可直接交给渲染层的 JSON
pub fn build(layout: &Value) -> Value {
    let mut rng = rng_of(layout);
    let size = num(layout, "size", 20.0);
    let roads = arr(layout, "roads");
    let paved: Vec<Value> = roads.iter().cloned().filter(is_paved).collect();
    let main_roads: Vec<Value> = roads
        .iter()
        .filter(|r| r.get("type").and_then(|v| v.as_str()) == Some("main"))
        .cloned()
        .collect();
    let brects = rects_of(layout);
    let grects = blocks_rects_of(layout);
    let mut blockers = brects.clone();
    blockers.extend_from_slice(&grects);

    let mut sidewalks: Vec<Value> = Vec::new();
    let mut crosswalks: Vec<Value> = Vec::new();
    let mut trees: Vec<Value> = Vec::new();
    let mut parking: Vec<Value> = Vec::new();
    let mut lamps: Vec<Value> = Vec::new();
    let mut bus_stops: Vec<Value> = Vec::new();
    let mut signals: Vec<Value> = Vec::new();

    const SW: f64 = 0.26; // 人行道宽度（网格）

    // ── 1) 人行道：沿铺装道路两侧各偏移一条带 ──
    for r in &paved {
        let (x1, y1, x2, y2) = norm(r);
        let off = width_of(r) / 2.0 + SW / 2.0;
        if seg_h(r) {
            sidewalks.push(json!({"x1":x1,"y1":y1-off,"x2":x2,"y2":y1-off,"w":SW,"side":"n"}));
            sidewalks.push(json!({"x1":x1,"y1":y1+off,"x2":x2,"y2":y1+off,"w":SW,"side":"s"}));
        } else if seg_v(r) {
            sidewalks.push(json!({"x1":x1-off,"y1":y1,"x2":x1-off,"y2":y2,"w":SW,"side":"w"}));
            sidewalks.push(json!({"x1":x1+off,"y1":y1,"x2":x1+off,"y2":y2,"w":SW,"side":"e"}));
        }
    }

    // ── 2) 交叉口：斑马线 + 红绿灯 ──
    let inter = intersections(layout);
    for (ix, iy) in &inter {
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            for k in 0..5 {
                let t = -0.35 + k as f64 * 0.175;
                if dx != 0.0 {
                    let px = ix + dx * 0.62;
                    let py = iy + t;
                    crosswalks.push(json!({"x1":px,"y1":py-0.06,"x2":px,"y2":py+0.06}));
                } else {
                    let px = ix + t;
                    let py = iy + dy * 0.62;
                    crosswalks.push(json!({"x1":px-0.06,"y1":py,"x2":px+0.06,"y2":py}));
                }
            }
        }
        signals.push(json!({"x":ix,"y":iy}));
    }

    // ── 3) 行道树：沿人行道等距、两侧交替、避开建筑 ──
    const STEP: f64 = 1.35;
    for r in &paved {
        let (x1, y1, x2, y2) = norm(r);
        let off = width_of(r) / 2.0 + SW + 0.22;
        if seg_h(r) {
            let n = ((x2 - x1) / STEP).max(2.0) as i32;
            for i in 0..=n {
                let px = x1 + i as f64 * (x2 - x1) / n as f64;
                for sy in [-1.0, 1.0] {
                    let py = y1 + sy * off;
                    if hit(px, py, &blockers, 0.08) {
                        continue;
                    }
                    if px > 0.4 && px < size - 0.4 && py > 0.4 && py < size - 0.4 {
                        let rr: f64 = rng.gen_range(0.17..0.24);
                        trees.push(json!({"x":(px*100.0).round()/100.0,"y":(py*100.0).round()/100.0,"r":(rr*1000.0).round()/1000.0}));
                    }
                }
            }
        } else if seg_v(r) {
            let n = ((y2 - y1) / STEP).max(2.0) as i32;
            for i in 0..=n {
                let py = y1 + i as f64 * (y2 - y1) / n as f64;
                for sx in [-1.0, 1.0] {
                    let px = x1 + sx * off;
                    if hit(px, py, &blockers, 0.08) {
                        continue;
                    }
                    if px > 0.4 && px < size - 0.4 && py > 0.4 && py < size - 0.4 {
                        let rr: f64 = rng.gen_range(0.17..0.24);
                        trees.push(json!({"x":(px*100.0).round()/100.0,"y":(py*100.0).round()/100.0,"r":(rr*1000.0).round()/1000.0}));
                    }
                }
            }
        }
    }

    // ── 4) 路灯：沿道路单侧，间距比树大 ──
    for r in paved.iter().chain(main_roads.iter()) {
        let (x1, y1, x2, y2) = norm(r);
        let off = width_of(r) / 2.0 + SW + 0.55;
        const LSTEP: f64 = 3.0;
        if seg_h(r) {
            let n = ((x2 - x1) / LSTEP).max(1.0) as i32;
            for i in 0..=n {
                let px = x1 + i as f64 * (x2 - x1) / n as f64;
                let py = y1 - off;
                if !hit(px, py, &blockers, 0.05) && py > 0.3 && py < size - 0.3 {
                    lamps.push(json!({"x":(px*100.0).round()/100.0,"y":(py*100.0).round()/100.0}));
                }
            }
        } else if seg_v(r) {
            let n = ((y2 - y1) / LSTEP).max(1.0) as i32;
            for i in 0..=n {
                let py = y1 + i as f64 * (y2 - y1) / n as f64;
                let px = x1 + off;
                if !hit(px, py, &blockers, 0.05) && px > 0.3 && px < size - 0.3 {
                    lamps.push(json!({"x":(px*100.0).round()/100.0,"y":(py*100.0).round()/100.0}));
                }
            }
        }
    }

    // ── 5) 公交站：主干道每 ~8 格一个 ──
    for r in &main_roads {
        let (x1, y1, x2, y2) = norm(r);
        let off = width_of(r) / 2.0 + SW + 0.30;
        if seg_h(r) {
            let n = ((x2 - x1) / 8.0).max(1.0) as i32;
            for i in 1..=n {
                let px = x1 + i as f64 * (x2 - x1) / (n + 1) as f64;
                let py = y1 + off;
                if !hit(px, py, &blockers, 0.1) && py > 0.5 && py < size - 0.5 {
                    bus_stops.push(json!({"x":(px*100.0).round()/100.0,"y":(py*100.0).round()/100.0,"dir":"s"}));
                }
            }
        } else if seg_v(r) {
            let n = ((y2 - y1) / 8.0).max(1.0) as i32;
            for i in 1..=n {
                let py = y1 + i as f64 * (y2 - y1) / (n + 1) as f64;
                let px = x1 - off;
                if !hit(px, py, &blockers, 0.1) && px > 0.5 && px < size - 0.5 {
                    bus_stops.push(json!({"x":(px*100.0).round()/100.0,"y":(py*100.0).round()/100.0,"dir":"w"}));
                }
            }
        }
    }

    // ── 6) 路边停车位：贴建筑外侧的空地 ──
    const SLOT: f64 = 0.52;
    for (bx, by, bw, bh) in &brects {
        let cands = [
            (*bx + 0.15, *by + *bh + 0.28, true),
            (*bx + *bw + 0.28, *by + 0.15, false),
        ];
        for (ax, ay, horiz) in cands {
            let span = if horiz { *bw - 0.3 } else { *bh - 0.3 };
            if span < SLOT * 1.5 {
                continue;
            }
            let n = (span / SLOT) as i32;
            for i in 0..n.min(4) {
                let (px, py) = if horiz {
                    (ax + i as f64 * SLOT, ay)
                } else {
                    (ax, ay + i as f64 * SLOT)
                };
                let (cx, cy) = if horiz {
                    (px + SLOT * 0.5, py)
                } else {
                    (px, py + SLOT * 0.5)
                };
                if hit(cx, cy, &brects, 0.06) || hit(cx, cy, &grects, 0.02) {
                    continue;
                }
                // 不能压在道路上
                let mut onroad = false;
                for r in &paved {
                    let (rx1, ry1, rx2, ry2) = norm(r);
                    if seg_h(r) && rx1 - 0.3 <= cx && cx <= rx2 + 0.3 && (cy - ry1).abs() < 0.55 {
                        onroad = true;
                        break;
                    }
                    if seg_v(r) && ry1 - 0.3 <= cy && cy <= ry2 + 0.3 && (cx - rx1).abs() < 0.55 {
                        onroad = true;
                        break;
                    }
                }
                if onroad || !(cx > 0.3 && cx < size - 0.3 && cy > 0.3 && cy < size - 0.3) {
                    continue;
                }
                let (w, h) = if horiz {
                    (SLOT - 0.06, SLOT * 0.62)
                } else {
                    (SLOT * 0.62, SLOT - 0.06)
                };
                parking.push(json!({
                    "x":(px*100.0).round()/100.0, "y":(py*100.0).round()/100.0,
                    "w":(w*100.0).round()/100.0, "h":(h*100.0).round()/100.0
                }));
            }
        }
    }

    let stats = json!({
        "sidewalkSegs": sidewalks.len(),
        "crosswalkBars": crosswalks.len(),
        "trees": trees.len(),
        "parking": parking.len(),
        "lamps": lamps.len(),
        "busStops": bus_stops.len(),
        "signals": signals.len(),
        "intersections": inter.len(),
    });
    json!({
        "sidewalks": sidewalks, "crosswalks": crosswalks, "trees": trees,
        "parking": parking, "lamps": lamps, "busStops": bus_stops,
        "signals": signals, "stats": stats,
    })
}

/// 原地把细节挂到 layout 上（渲染层直接可用）
pub fn enrich(layout: &mut Value) {
    let d = build(layout);
    if let Some(o) = layout.as_object_mut() {
        o.insert("details".into(), d);
    }
}

/// 取某个细节类别的数组（渲染层用）
pub fn list<'a>(layout: &'a Value, kind: &str) -> &'a [Value] {
    layout
        .get("details")
        .and_then(|d| d.get(kind))
        .and_then(|v| v.as_array())
        .map(|v| v.as_slice())
        .unwrap_or(&[])
}

/// 统计摘要
pub fn stats(layout: &Value) -> Map<String, Value> {
    layout
        .get("details")
        .and_then(|d| d.get("stats"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        // 一个 20×20 的井字路网 + 几栋楼
        json!({
            "name": "测试小区", "size": 20,
            "buildings": [
                {"x":3,"y":3,"w":3,"h":2,"type":"residential","name":"1号楼"},
                {"x":11,"y":3,"w":4,"h":3,"type":"office","name":"云顶大厦"},
                {"x":3,"y":12,"w":3,"h":3,"type":"shop","name":"便利店"},
            ],
            "roads": [
                {"x1":2,"y1":2,"x2":18,"y2":2,"type":"main","name":"中山路"},
                {"x1":2,"y1":10,"x2":18,"y2":10,"type":"secondary","name":"文明路"},
                {"x1":2,"y1":2,"x2":2,"y2":18,"type":"main","name":"解放大道"},
                {"x1":10,"y1":2,"x2":10,"y2":18,"type":"secondary","name":"寺贝通津"},
            ],
            "parks": [{"x":13,"y":12,"w":4,"h":4,"name":"中心公园"}],
            "water": [],
        })
    }

    #[test]
    fn generates_all_detail_kinds() {
        let d = build(&sample());
        for k in ["sidewalks", "crosswalks", "trees", "lamps", "signals"] {
            let n = d.get(k).and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
            assert!(n > 0, "{k} 应该有内容，实际 {n}");
        }
        let inter = d.get("stats").and_then(|s| s.get("intersections")).and_then(|v| v.as_u64()).unwrap_or(0);
        assert_eq!(inter, 4, "井字路网应有 4 个交叉口，实际 {inter}");
    }

    #[test]
    fn trees_never_inside_buildings() {
        let lay = sample();
        let d = build(&lay);
        let blds = rects_of(&lay);
        for t in d["trees"].as_array().unwrap() {
            let (x, y) = (num(t, "x", 0.0), num(t, "y", 0.0));
            assert!(!hit(x, y, &blds, -0.02), "树不该压在建筑上: {x},{y}");
        }
    }

    #[test]
    fn deterministic_same_input_same_output() {
        let a = build(&sample());
        let b = build(&sample());
        assert_eq!(a["trees"], b["trees"], "同输入必须得到同一批树（可复现）");
        assert_eq!(a["stats"], b["stats"]);
    }

    #[test]
    fn different_layout_different_trees() {
        let mut other = sample();
        other["name"] = json!("另一个小区");
        let a = build(&sample());
        let b = build(&other);
        assert_ne!(a["trees"], b["trees"], "不同小区应有不同的随机细节");
    }

    #[test]
    fn crosswalk_bars_multiple_of_20_per_intersection() {
        let d = build(&sample());
        let bars = d["crosswalks"].as_array().unwrap().len();
        // 每交叉口 4 方向 × 5 道 = 20
        assert_eq!(bars, 4 * 20, "斑马线条数应为 交叉口×20，实际 {bars}");
    }

    #[test]
    fn enrich_attaches_details() {
        let mut lay = sample();
        enrich(&mut lay);
        assert!(lay.get("details").is_some());
        assert!(!list(&lay, "trees").is_empty());
        assert!(stats(&lay).contains_key("trees"));
    }

    #[test]
    fn empty_layout_does_not_panic() {
        let empty = json!({"name": "空", "size": 20, "buildings": [], "roads": [], "parks": [], "water": []});
        let d = build(&empty);
        assert_eq!(d["trees"].as_array().unwrap().len(), 0);
        assert_eq!(intersections(&empty).len(), 0);
    }
}
