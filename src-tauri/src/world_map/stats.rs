//! 小区数据统计（移植自 Python `stats.py`）
//!
//! 把布局算成一组有意义的指标：容积率、绿化率、建筑密度、人口估算、设施配比…
//! 既画在地图角落的数据卡片上，也供前端数据面板用。
use serde_json::{json, Map, Value};

/// 每格约多少米（与 details::GRID_M 对齐）
pub const GRID_M: f64 = 18.0;
/// 住宅人均建筑面积（㎡/人）
pub const SQ_M_PER_PERSON: f64 = 32.0;

/// 类型中文名
pub fn type_zh(t: &str) -> &'static str {
    match t {
        "residential" => "住宅",
        "office" => "写字楼",
        "commercial" => "商场",
        "shop" => "店铺",
        "restaurant" => "餐饮",
        "cafe" => "咖啡",
        "school" => "学校",
        "hospital" => "医院",
        "civic" => "市政",
        "leisure" => "娱乐",
        _ => "其它",
    }
}

/// 各类型的「人的密度」权重（估算人流强度）
fn type_weight(t: &str) -> f64 {
    match t {
        "residential" => 1.0,
        "office" => 1.6,
        "commercial" => 2.2,
        "shop" => 1.4,
        "restaurant" => 1.3,
        "cafe" => 0.9,
        "school" => 1.8,
        "hospital" => 1.5,
        "civic" => 0.8,
        "leisure" => 1.2,
        _ => 1.0,
    }
}

fn num(v: &Value, k: &str, d: f64) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(d)
}

fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k)
        .and_then(|x| x.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// 计算统计指标（全部为纯数值，便于画图）
pub fn compute(layout: &Value) -> Value {
    let size = num(layout, "size", 20.0);
    let cell_m = GRID_M;
    let area_m2 = ((size * cell_m) * (size * cell_m)).max(1.0);
    let grid_area = size * size;
    let to_m2 = cell_m * cell_m;

    let blds = arr(layout, "buildings");
    let roads = arr(layout, "roads");
    let parks = arr(layout, "parks");
    let water = arr(layout, "water");

    let mut by_type: Map<String, Value> = Map::new();
    let mut counts: std::collections::BTreeMap<String, (u32, f64, f64, Vec<f64>)> =
        std::collections::BTreeMap::new();
    let mut tot_footprint = 0.0;
    let mut tot_floor_area = 0.0;
    let mut floors_list: Vec<f64> = Vec::new();
    let mut named_buildings = 0u32;

    for b in blds {
        let t = b.get("type").and_then(|v| v.as_str()).unwrap_or("residential").to_string();
        if !b.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().is_empty() {
            named_buildings += 1;
        }
        let w = num(b, "w", 1.0).max(0.0);
        let h = num(b, "h", 1.0).max(0.0);
        let fp = w * h;
        let fl_raw = b.get("floors").cloned().unwrap_or(Value::Null);
        let mut fl = fl_raw
            .as_f64()
            .or_else(|| fl_raw.as_str().and_then(|s| s.parse::<f64>().ok()))
            .unwrap_or(3.0);
        fl = fl.clamp(1.0, 80.0);

        let e = counts.entry(t).or_insert((0, 0.0, 0.0, Vec::new()));
        e.0 += 1;
        e.1 += fp;
        e.2 += fp * fl;
        e.3.push(fl);
        tot_footprint += fp;
        tot_floor_area += fp * fl;
        floors_list.push(fl);
    }
    for (t, (c, fp, fa, fls)) in &counts {
        by_type.insert(
            t.clone(),
            json!({
                "count": c,
                "footprintM2": (fp * to_m2).round(),
                "floorAreaM2": (fa * to_m2).round(),
                "avgFloors": round1(fls.iter().sum::<f64>() / (fls.len().max(1) as f64)),
            }),
        );
    }

    let park_area: f64 = parks.iter().map(|p| num(p, "w", 0.0) * num(p, "h", 0.0)).sum();
    let water_area: f64 = water.iter().map(|p| num(p, "w", 0.0) * num(p, "h", 0.0)).sum();

    let seg_len = |r: &Value| -> f64 {
        let (x1, y1) = (num(r, "x1", 0.0), num(r, "y1", 0.0));
        let (x2, y2) = (num(r, "x2", 0.0), num(r, "y2", 0.0));
        if (y2 - y1).abs() < 1e-9 || (x2 - x1).abs() < 1e-9 {
            (x2 - x1).abs() + (y2 - y1).abs()
        } else {
            ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt()
        }
    };
    let road_len: f64 = roads.iter().map(seg_len).sum();
    let named_roads = roads
        .iter()
        .filter(|r| !r.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().is_empty())
        .count() as u32;

    // 人口：住宅建筑面积 ÷ 人均面积
    let res_floor = counts.get("residential").map(|c| c.2).unwrap_or(0.0);
    let pop = (res_floor * to_m2) / SQ_M_PER_PERSON;

    // 人流强度 0-100
    let intensity_raw: f64 = counts
        .iter()
        .map(|(t, c)| type_weight(t) * c.2)
        .sum();
    let crowd = (intensity_raw / grid_area.max(1.0) * 12.0).min(100.0);

    let det_stats = layout
        .get("details")
        .and_then(|d| d.get("stats"))
        .cloned()
        .unwrap_or(json!({}));
    let dn = |k: &str| det_stats.get(k).and_then(|v| v.as_u64()).unwrap_or(0);

    let max_floors = floors_list.iter().cloned().fold(0.0f64, f64::max);
    let avg_floors = if floors_list.is_empty() {
        0.0
    } else {
        floors_list.iter().sum::<f64>() / floors_list.len() as f64
    };

    json!({
        "size": size,
        "cellMeters": cell_m,
        "areaM2": round1(area_m2),
        "areaHa": round2(area_m2 / 10000.0),

        "buildingCount": blds.len(),
        "byType": Value::Object(by_type),

        "buildingDensity": round1(tot_footprint / grid_area * 100.0),
        "plotRatio": round2(tot_floor_area / grid_area),
        "greenRate": round1(park_area / grid_area * 100.0),
        "waterRate": round1(water_area / grid_area * 100.0),
        "roadKm": round2(road_len * cell_m / 1000.0),
        "roadDensity": round1(road_len * cell_m / grid_area / cell_m * 1000.0),

        "maxFloors": max_floors as i64,
        "avgFloors": round1(avg_floors),
        "totalFloorAreaM2": (tot_floor_area * to_m2).round(),

        "popEstimate": pop.round() as i64,
        "crowdIndex": round1(crowd),

        "trees": dn("trees"),
        "parking": dn("parking"),
        "lamps": dn("lamps"),
        "busStops": dn("busStops"),
        "signals": dn("signals"),
        "crosswalks": dn("crosswalkBars"),
        "intersections": dn("intersections"),

        "namedBuildings": named_buildings,
        "namedRoads": named_roads,
        "parks": parks.len(),
        "water": water.len(),
        "namingRate": (named_buildings as f64 / blds.len().max(1) as f64 * 100.0).round(),
        "roadNamingRate": (named_roads as f64 / roads.len().max(1) as f64 * 100.0).round(),
    })
}

/// 给柱状图用的序列（数量降序）：(type, 中文名, 数量, 建筑面积㎡)
pub fn type_series(st: &Value) -> Vec<(String, String, u32, f64)> {
    let mut rows: Vec<(String, String, u32, f64)> = st
        .get("byType")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        type_zh(k).to_string(),
                        v.get("count").and_then(|c| c.as_u64()).unwrap_or(0) as u32,
                        v.get("floorAreaM2").and_then(|c| c.as_f64()).unwrap_or(0.0),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    rows.sort_by(|a, b| b.2.cmp(&a.2));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lay() -> Value {
        let mut l = crate::world_map::sketch::make_sketch("统计测试", 20, Some(3), None);
        for (i, b) in l["buildings"].as_array_mut().unwrap().iter_mut().enumerate() {
            b["name"] = json!(format!("{}号楼", i + 1));
            b["floors"] = json!(6);
        }
        for r in l["roads"].as_array_mut().unwrap().iter_mut() {
            r["name"] = json!("中山路");
        }
        crate::world_map::details::enrich(&mut l);
        l
    }

    #[test]
    fn computes_core_indicators() {
        let s = compute(&lay());
        assert!(s["buildingCount"].as_u64().unwrap() > 0);
        assert!(s["plotRatio"].as_f64().unwrap() > 0.0, "容积率应 > 0");
        assert!(s["buildingDensity"].as_f64().unwrap() > 0.0);
        assert!(s["popEstimate"].as_i64().unwrap() > 0, "人口估算应 > 0");
        assert!(s["roadKm"].as_f64().unwrap() > 0.0, "道路长度应 > 0");
        assert_eq!(s["maxFloors"].as_i64().unwrap(), 6);
    }

    #[test]
    fn naming_rate_reflects_named_buildings() {
        let mut l = lay();
        let s1 = compute(&l);
        assert_eq!(s1["namingRate"].as_f64().unwrap(), 100.0);
        // 清掉一半名字
        let half = l["buildings"].as_array().unwrap().len() / 2;
        for b in l["buildings"].as_array_mut().unwrap().iter_mut().take(half) {
            b["name"] = json!("");
        }
        let s2 = compute(&l);
        let r = s2["namingRate"].as_f64().unwrap();
        assert!(r < 100.0 && r > 0.0, "命名率应下降，实际 {r}");
    }

    #[test]
    fn detail_counts_carried_over() {
        let s = compute(&lay());
        let d = crate::world_map::details::stats(&lay());
        assert_eq!(
            s["trees"].as_u64().unwrap(),
            d.get("trees").and_then(|v| v.as_u64()).unwrap_or(0),
            "统计里的树数量应与细节层一致"
        );
    }

    #[test]
    fn type_series_sorted_desc() {
        let s = compute(&lay());
        let rows = type_series(&s);
        assert!(!rows.is_empty());
        for w in rows.windows(2) {
            assert!(w[0].2 >= w[1].2, "类型序列应按数量降序");
        }
        assert_eq!(rows[0].1, "住宅", "草图中住宅应最多");
    }

    #[test]
    fn empty_layout_safe() {
        let empty = json!({"name":"空","size":20,"buildings":[],"roads":[],"parks":[],"water":[]});
        let s = compute(&empty);
        assert_eq!(s["buildingCount"].as_u64().unwrap(), 0);
        assert_eq!(s["popEstimate"].as_i64().unwrap(), 0);
        assert_eq!(s["plotRatio"].as_f64().unwrap(), 0.0);
        assert!(type_series(&s).is_empty());
    }

    #[test]
    fn zero_floors_clamped() {
        let mut l = lay();
        l["buildings"][0]["floors"] = json!(0);
        let s = compute(&l);
        // 0 层被夹到 1，容积率仍应为正
        assert!(s["plotRatio"].as_f64().unwrap() > 0.0);
    }
}
