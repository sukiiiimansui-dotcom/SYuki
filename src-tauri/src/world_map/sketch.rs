//! 小区布局「草图」生成（移植自 Python `sketch.py`）
//!
//! 两段式生成的第一段：**本地规则、毫秒级、不调 LLM**。
//! 用户点开立刻看到一张像那么回事的布局，AI 结果回来再整张替换。
//!
//! 设计要点：
//!   · 确定性：同区域 + 同 seed 永远同一张图（可缓存、可复现）
//!   · 有路网骨架（主/次干道）、沿街建筑、中心绿地，不是随机撒点
//!   · 名字留空，交给 AI 覆盖（前端可显示占位）
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use serde_json::{json, Value};

/// 采样半径内的真实地物密度（来自 OSM），用于让草图贴近真实
#[derive(Debug, Clone, Default)]
pub struct OsmHint {
    pub building_kinds: usize,
    pub highway_kinds: usize,
    pub has_water: bool,
    pub has_park: bool,
}

impl OsmHint {
    /// 由 OSM summarize 结果构造（字段缺失时按中位值处理）
    pub fn from_summary(v: &Value) -> Self {
        let len = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        };
        let has = |k: &str, want: &str| {
            v.get(k)
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter().any(|it| {
                        it.as_array()
                            .and_then(|p| p.first())
                            .and_then(|s| s.as_str())
                            .map(|s| s.contains(want))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        };
        Self {
            building_kinds: len("building_types"),
            highway_kinds: len("highway_types"),
            has_water: has("leisure", "water") || has("landuse", "water"),
            has_park: has("leisure", "park") || has("leisure", "garden"),
        }
    }

    /// 建筑排布密度 0.35~0.95
    fn density(&self) -> f64 {
        (0.35 + 0.08 * self.building_kinds as f64 + 0.03 * self.highway_kinds as f64).min(0.95)
    }
}

fn seed_of(area: &str, seed: Option<u64>) -> [u8; 32] {
    let d = md5::compute(format!("{}|{}", area, seed.map(|s| s.to_string()).unwrap_or_default()).as_bytes());
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = d.0[i % 16];
    }
    out
}

/// 生成草图布局（结构与 AI 版完全同构，可直接交给渲染器）
pub fn make_sketch(area_name: &str, size: i32, seed: Option<u64>, osm: Option<&OsmHint>) -> Value {
    let mut rng = StdRng::from_seed(seed_of(area_name, seed));
    let s = size.max(8);
    let dens = osm.map(|o| o.density()).unwrap_or(0.5);
    let has_water = osm.map(|o| o.has_water).unwrap_or(false);
    let has_park = osm.map(|o| o.has_park).unwrap_or(false);

    let mut buildings: Vec<Value> = Vec::new();
    let mut roads: Vec<Value> = Vec::new();
    let mut parks: Vec<Value> = Vec::new();
    let mut water: Vec<Value> = Vec::new();

    // ── 1) 路网骨架：井字主干道 + 次干道 ──
    let margin = 2;
    let lanes = if s <= 16 { 3 } else { 4 };
    let step = ((s - 2 * margin) / lanes).max(4);
    let mut main_rows: Vec<i32> = Vec::new();
    let mut main_cols: Vec<i32> = Vec::new();
    for i in 0..=lanes {
        let p = margin + i * step;
        if p > s - margin {
            break;
        }
        if i % 2 == 0 {
            main_rows.push(p);
        } else {
            main_cols.push(p);
        }
    }
    for (idx, y) in main_rows.iter().enumerate() {
        roads.push(json!({"x1":margin,"y1":y,"x2":s-margin,"y2":y,
                          "type": if idx < 2 {"main"} else {"secondary"}}));
    }
    for (idx, x) in main_cols.iter().enumerate() {
        roads.push(json!({"x1":x,"y1":margin,"x2":x,"y2":s-margin,
                          "type": if idx < 1 {"main"} else {"secondary"}}));
    }
    // 小巷
    let n_paths = rng.gen_range(2..=4);
    for _ in 0..n_paths {
        if rng.gen_bool(0.5) {
            let y = rng.gen_range((margin + 1)..=(s - margin - 1).max(margin + 1));
            roads.push(json!({"x1":margin,"y1":y,"x2":s-margin,"y2":y,"type":"path"}));
        } else {
            let x = rng.gen_range((margin + 1)..=(s - margin - 1).max(margin + 1));
            roads.push(json!({"x1":x,"y1":margin,"x2":x,"y2":s-margin,"type":"path"}));
        }
    }

    // ── 2) 中心绿地 / 水面 ──
    // 骨架必定带一块中心绿地（随机抛硬币会让"骨架"时有时无，不可复现也不好看）；
    // OSM 显示这一带真有公园时，绿地给大一号。
    let (cx, cy) = (s / 2, s / 2);
    {
        let w = if has_park { rng.gen_range(4..=6) } else { rng.gen_range(3..=5) };
        let h = rng.gen_range(2..=4);
        parks.push(json!({"x":(cx - w/2).max(1),"y":(cy - h/2).max(1),"w":w,"h":h}));
    }
    if has_water {
        water.push(json!({"x":margin,"y":s-margin-3,"w":rng.gen_range(4..=7),"h":3}));
    }

    // ── 3) 沿街建筑：在道路围成的街区里排 ──
    let mut grid_lines: Vec<i32> = main_rows.iter().chain(main_cols.iter()).cloned().collect();
    grid_lines.push(margin);
    grid_lines.push(s - margin);
    grid_lines.sort_unstable();
    grid_lines.dedup();

    let big = ["office", "commercial", "school", "hospital", "civic"];
    let small = ["shop", "restaurant", "cafe", "leisure"];
    let mut occ: Vec<(i32, i32, i32, i32)> = Vec::new();
    let overlaps = |occ: &Vec<(i32, i32, i32, i32)>, x: i32, y: i32, w: i32, h: i32| {
        occ.iter().any(|(ax, ay, aw, ah)| x < ax + aw && *ax < x + w && y < ay + ah && *ay < y + h)
    };

    let mut blocks: Vec<(i32, i32, i32, i32)> = Vec::new();
    for i in 0..grid_lines.len().saturating_sub(1) {
        for j in 0..grid_lines.len().saturating_sub(1) {
            blocks.push((
                grid_lines[i],
                grid_lines[j],
                grid_lines[i + 1] - grid_lines[i],
                grid_lines[j + 1] - grid_lines[j],
            ));
        }
    }
    // 打乱（与 Python 的 shuffle 等价语义）
    for i in (1..blocks.len()).rev() {
        let j = rng.gen_range(0..=i);
        blocks.swap(i, j);
    }

    // 候选位置与随机序列只由地形决定，密度只决定"保留前几栋"
    let per_block_cap = 5;
    let per_block = (2 + (dens * 3.0) as i32).clamp(1, per_block_cap);
    for (bx, by, bw, bh) in blocks {
        if bw < 2 || bh < 2 {
            continue;
        }
        let mut placed = 0;
        let mut tries = 0;
        while placed < per_block_cap && tries < 14 {
            tries += 1;
            let w = rng.gen_range(2..=bw.saturating_sub(1).max(2).min(4));
            let h = rng.gen_range(1..=bh.saturating_sub(1).max(1).min(3));
            if bw - w < 1 || bh - h < 1 {
                continue;
            }
            let x = rng.gen_range(bx..=(bx + bw - w - 1).max(bx));
            let y = rng.gen_range(by..=(by + bh - h - 1).max(by));
            // 别压到绿地/水面
            let clash = parks.iter().chain(water.iter()).any(|p| {
                let (px, py) = (
                    p.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    p.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                );
                let (pw, ph) = (
                    p.get("w").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    p.get("h").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                );
                x < px + pw && px < x + w && y < py + ph && py < y + h
            });
            if clash || overlaps(&occ, x, y, w, h) {
                continue;
            }
            // 主干道旁放大建筑，内部放住宅
            let near_main = main_rows.iter().any(|r| (y - r).abs() <= 2)
                || main_cols.iter().any(|c| (x - c).abs() <= 2);
            let typ = if near_main && rng.gen_bool(0.45) {
                if rng.gen_bool(0.35) {
                    big[rng.gen_range(0..big.len())]
                } else {
                    small[rng.gen_range(0..small.len())]
                }
            } else {
                "residential"
            };
            let floors = if typ == "office" {
                rng.gen_range(3..=18)
            } else {
                rng.gen_range(2..=7)
            };
            if placed >= per_block {
                // 配额已满：随机数照常消费，保证两个密度的候选流完全一致
                continue;
            }
            occ.push((x, y, w, h));
            placed += 1;
            buildings.push(json!({
                "x":x,"y":y,"w":w,"h":h,"type":typ,
                "name":"",           // 草图不取名，交给 AI 覆盖
                "floors":floors,
                "_sketch":true
            }));
        }
    }

    let nm_seed = u64::from_be_bytes(seed_of(area_name, seed)[0..8].try_into().unwrap()) % 997;
    json!({
        "name": area_name,
        "size": s,
        "buildings": buildings,
        "roads": roads,
        "parks": parks,
        "water": water,
        "_sketch": true,
        "_sketchSeed": nm_seed,
        "_density": (dens * 100.0).round() / 100.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a = make_sketch("广州市·越秀区", 20, None, None);
        let b = make_sketch("广州市·越秀区", 20, None, None);
        assert_eq!(a["buildings"], b["buildings"], "同输入必须同输出（可缓存可复现）");
        assert_eq!(a["roads"], b["roads"]);
    }

    #[test]
    fn different_area_differs() {
        let a = make_sketch("广州市·越秀区", 20, None, None);
        let b = make_sketch("深圳市·南山区", 20, None, None);
        assert_ne!(a["buildings"], b["buildings"]);
    }

    #[test]
    fn produces_skeleton() {
        let v = make_sketch("测试小区", 20, None, None);
        let b = v["buildings"].as_array().unwrap();
        let r = v["roads"].as_array().unwrap();
        assert!(b.len() >= 10, "至少要有十几栋楼，实际 {}", b.len());
        assert!(r.len() >= 4, "至少要有骨架路网，实际 {}", r.len());
        assert!(v["parks"].as_array().unwrap().len() >= 1, "应有中心绿地");
        // 建筑必须落在网格内
        for it in b {
            let (x, y, w, h) = (
                it["x"].as_i64().unwrap(),
                it["y"].as_i64().unwrap(),
                it["w"].as_i64().unwrap(),
                it["h"].as_i64().unwrap(),
            );
            assert!(x >= 0 && y >= 0 && x + w <= 20 && y + h <= 20, "建筑越界: {it}");
        }
    }

    #[test]
    fn buildings_do_not_overlap() {
        let v = make_sketch("重叠检查", 20, None, None);
        let b = v["buildings"].as_array().unwrap();
        for i in 0..b.len() {
            for j in i + 1..b.len() {
                let (x1, y1, w1, h1) = (
                    b[i]["x"].as_i64().unwrap(), b[i]["y"].as_i64().unwrap(),
                    b[i]["w"].as_i64().unwrap(), b[i]["h"].as_i64().unwrap(),
                );
                let (x2, y2, w2, h2) = (
                    b[j]["x"].as_i64().unwrap(), b[j]["y"].as_i64().unwrap(),
                    b[j]["w"].as_i64().unwrap(), b[j]["h"].as_i64().unwrap(),
                );
                let overlap = x1 < x2 + w2 && x2 < x1 + w1 && y1 < y2 + h2 && y2 < y1 + h1;
                assert!(!overlap, "建筑重叠: {i} 与 {j}");
            }
        }
    }

    #[test]
    fn osm_density_affects_layout() {
        let sparse = OsmHint { building_kinds: 1, highway_kinds: 1, ..Default::default() };
        let dense = OsmHint { building_kinds: 6, highway_kinds: 6, ..Default::default() };
        let a = make_sketch("对比", 20, None, Some(&sparse));
        let b = make_sketch("对比", 20, None, Some(&dense));
        let na = a["buildings"].as_array().unwrap().len();
        let nb = b["buildings"].as_array().unwrap().len();
        assert!(nb >= na, "密度高的区域建筑应不少于稀疏区（{nb} vs {na}）");
        assert!(b["_density"].as_f64().unwrap() > a["_density"].as_f64().unwrap());
    }

    #[test]
    fn osm_summary_parsing() {
        let v = json!({
            "building_types": [["yes", 145], ["commercial", 14]],
            "highway_types": [["footway", 76]],
            "leisure": [["park", 3], ["water", 1]],
        });
        let h = OsmHint::from_summary(&v);
        assert_eq!(h.building_kinds, 2);
        assert_eq!(h.highway_kinds, 1);
        assert!(h.has_park && h.has_water);
        assert!(h.density() > 0.35);
    }

    #[test]
    fn empty_osm_gives_default_density() {
        let h = OsmHint::default();
        assert!((h.density() - 0.35).abs() < 1e-9);
        let v = make_sketch("无 OSM", 20, None, Some(&h));
        assert!(!v["buildings"].as_array().unwrap().is_empty());
    }
}
