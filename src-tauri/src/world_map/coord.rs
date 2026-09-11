//! 统一坐标系统（T1-1 的 Rust 版，与 world_coord.py / world_coord.js 算法一致）
//!
//! 四层坐标：地理(lng,lat) ↔ 世界(Web Mercator 0..1) ↔ 画布(像素) ↔ 网格(小区)
use std::f64::consts::PI;

pub const D2R: f64 = PI / 180.0;
pub const R2D: f64 = 180.0 / PI;
pub const METERS_PER_DEG_LAT: f64 = 110_574.0;
pub const METERS_PER_DEG_LNG_EQ: f64 = 111_320.0;

/// 世界坐标包围盒 (minx, miny, maxx, maxy)
pub type BBox = (f64, f64, f64, f64);

/// 经纬度 → 世界坐标（Web Mercator，0..1）
pub fn lng_lat_to_world(lng: f64, lat: f64) -> (f64, f64) {
    let x = (lng + 180.0) / 360.0;
    let s = (lat * D2R).sin();
    let y = 0.5 - ((1.0 + s) / (1.0 - s)).ln() / (4.0 * PI);
    (x, y)
}

/// 世界坐标 → 经纬度
pub fn world_to_lng_lat(x: f64, y: f64) -> (f64, f64) {
    let lng = x * 360.0 - 180.0;
    let n = PI * (1.0 - 2.0 * y);
    let lat = R2D * n.sinh().atan();
    (lng, lat)
}

/// 一组经纬度的世界坐标 bbox
pub fn bbox_of_lng_lat(points: &[(f64, f64)]) -> Option<BBox> {
    if points.is_empty() {
        return None;
    }
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(lng, lat) in points {
        let (x, y) = lng_lat_to_world(lng, lat);
        if x < minx {
            minx = x;
        }
        if y < miny {
            miny = y;
        }
        if x > maxx {
            maxx = x;
        }
        if y > maxy {
            maxy = y;
        }
    }
    Some((minx, miny, maxx, maxy))
}

/// 适配缩放：把 bbox 铺进 w×h 的画布（pad 是画布内边距，不是世界坐标跨度）
pub fn fit_scale(bbox: BBox, w: f64, h: f64, pad: f64) -> (f64, f64, f64) {
    let (minx, miny, maxx, maxy) = bbox;
    let bw = (maxx - minx).max(1e-12);
    let bh = (maxy - miny).max(1e-12);
    let aw = (w - 2.0 * pad).max(1.0);
    let ah = (h - 2.0 * pad).max(1.0);
    let scale = (aw / bw).min(ah / bh);
    let ox = pad + (aw - bw * scale) / 2.0;
    let oy = pad + (ah - bh * scale) / 2.0;
    (scale, ox, oy)
}

pub fn world_to_canvas(x: f64, y: f64, bbox: BBox, w: f64, h: f64, pad: f64) -> (f64, f64) {
    let (scale, ox, oy) = fit_scale(bbox, w, h, pad);
    ((x - bbox.0) * scale + ox, (y - bbox.1) * scale + oy)
}

pub fn canvas_to_world(cx: f64, cy: f64, bbox: BBox, w: f64, h: f64, pad: f64) -> (f64, f64) {
    let (scale, ox, oy) = fit_scale(bbox, w, h, pad);
    ((cx - ox) / scale + bbox.0, (cy - oy) / scale + bbox.1)
}

pub fn lng_lat_to_canvas(
    lng: f64,
    lat: f64,
    bbox: BBox,
    w: f64,
    h: f64,
    pad: f64,
) -> (f64, f64) {
    let (wx, wy) = lng_lat_to_world(lng, lat);
    world_to_canvas(wx, wy, bbox, w, h, pad)
}

pub fn canvas_to_lng_lat(
    cx: f64,
    cy: f64,
    bbox: BBox,
    w: f64,
    h: f64,
    pad: f64,
) -> (f64, f64) {
    let (wx, wy) = canvas_to_world(cx, cy, bbox, w, h, pad);
    world_to_lng_lat(wx, wy)
}

pub fn meters_per_deg_lng(lat: f64) -> f64 {
    METERS_PER_DEG_LNG_EQ * (lat * D2R).cos()
}

/// 网格坐标 → 世界坐标
pub fn grid_to_world(gx: f64, gy: f64, anchor_lng: f64, anchor_lat: f64, m_per_cell: f64) -> (f64, f64) {
    let mlng = meters_per_deg_lng(anchor_lat);
    let lng = anchor_lng + (gx * m_per_cell) / mlng;
    let lat = anchor_lat - (gy * m_per_cell) / METERS_PER_DEG_LAT;
    lng_lat_to_world(lng, lat)
}

/// 世界坐标 → 网格坐标
pub fn world_to_grid(x: f64, y: f64, anchor_lng: f64, anchor_lat: f64, m_per_cell: f64) -> (f64, f64) {
    let (lng, lat) = world_to_lng_lat(x, y);
    let mlng = meters_per_deg_lng(anchor_lat);
    let gx = ((lng - anchor_lng) * mlng) / m_per_cell;
    let gy = ((anchor_lat - lat) * METERS_PER_DEG_LAT) / m_per_cell;
    (gx, gy)
}

/// 两点直线距离（米，haversine）
pub fn haversine_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    const EARTH_R: f64 = 6_371_008.8;
    let (lng1, lat1) = a;
    let (lng2, lat2) = b;
    let p1 = lat1 * D2R;
    let p2 = lat2 * D2R;
    let dp = (lat2 - lat1) * D2R;
    let dl = (lng2 - lng1) * D2R;
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_R * h.sqrt().min(1.0).asin()
}

/// 初始方位角（0=正北，顺时针 0-360）
pub fn bearing_deg(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lng1, lat1) = a;
    let (lng2, lat2) = b;
    let p1 = lat1 * D2R;
    let p2 = lat2 * D2R;
    let dl = (lng2 - lng1) * D2R;
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    let d = y.atan2(x) * R2D;
    (d + 360.0) % 360.0
}

pub const DIRS: [&str; 8] = ["北", "东北", "东", "东南", "南", "西南", "西", "西北"];

/// 方位角 → 中文八方位
pub fn compass(b: f64) -> &'static str {
    let i = ((((b + 22.5) % 360.0) / 45.0).floor() as usize) % 8;
    DIRS[i]
}

/// 方位角 → 矩形边缘的相对位置（0..1），aspect = 宽/高
pub fn edge_point(b: f64, aspect: f64, inset: f64) -> (f64, f64) {
    let r = b * D2R;
    let dx = r.sin();
    let dy = -r.cos();
    let (tx, ty) = if aspect != 0.0 { (dx / aspect, dy) } else { (dx, dy) };
    let m = tx.abs().max(ty.abs()).max(1e-12);
    let (ux, uy) = (tx / m, ty / m);
    let x = (0.5 + ux * 0.5).clamp(inset, 1.0 - inset);
    let y = (0.5 + uy * 0.5).clamp(inset, 1.0 - inset);
    (x, y)
}

/// 点是否在多边形内（射线法），poly = [(lng,lat)...]
pub fn point_in_poly(pt: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let (x, y) = pt;
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn roundtrip_lng_lat() {
        for &(lng, lat) in &[(116.4074, 39.9042), (113.264, 23.129), (107.2779, 29.7524)] {
            let (x, y) = lng_lat_to_world(lng, lat);
            let (lng2, lat2) = world_to_lng_lat(x, y);
            assert!(close(lng, lng2, 1e-9), "lng {lng} -> {lng2}");
            assert!(close(lat, lat2, 1e-9), "lat {lat} -> {lat2}");
        }
    }

    #[test]
    fn grid_roundtrip() {
        let (alng, alat) = (113.264, 23.129);
        let (x, y) = grid_to_world(10.0, 10.0, alng, alat, 20.0);
        let (gx, gy) = world_to_grid(x, y, alng, alat, 20.0);
        assert!(close(gx, 10.0, 1e-6), "gx {gx}");
        assert!(close(gy, 10.0, 1e-6), "gy {gy}");
    }

    #[test]
    fn canvas_roundtrip() {
        let pts = [(113.0, 23.0), (114.0, 24.0)];
        let bb = bbox_of_lng_lat(&pts).unwrap();
        let (cx, cy) = lng_lat_to_canvas(113.5, 23.5, bb, 800.0, 600.0, 20.0);
        let (lng, lat) = canvas_to_lng_lat(cx, cy, bb, 800.0, 600.0, 20.0);
        assert!(close(lng, 113.5, 1e-6), "lng {lng}");
        assert!(close(lat, 23.5, 1e-6), "lat {lat}");
    }

    #[test]
    fn bbox_pads_within_canvas() {
        let pts = [(113.0, 23.0), (113.1, 23.1)];
        let bb = bbox_of_lng_lat(&pts).unwrap();
        let (x, y) = world_to_canvas(bb.0, bb.1, bb, 400.0, 300.0, 25.0);
        assert!(x >= 24.9 && y >= 24.9, "左上角应留出 pad: {x},{y}");
        let (x2, y2) = world_to_canvas(bb.2, bb.3, bb, 400.0, 300.0, 25.0);
        assert!(x2 <= 375.1 && y2 <= 275.1, "右下角应留出 pad: {x2},{y2}");
    }

    #[test]
    fn haversine_guangzhou_shenzhen() {
        let d = haversine_m((113.2644, 23.1291), (114.0579, 22.5431));
        assert!(d > 90_000.0 && d < 110_000.0, "广州→深圳约 100km，实际 {d}");
    }

    #[test]
    fn bearings_cardinal() {
        let c = (113.2644, 23.1291);
        assert_eq!(compass(bearing_deg(c, (113.2644, 24.1291))), "北");
        assert_eq!(compass(bearing_deg(c, (114.2644, 23.1291))), "东");
        assert_eq!(compass(bearing_deg(c, (113.2644, 22.1291))), "南");
        assert_eq!(compass(bearing_deg(c, (112.2644, 23.1291))), "西");
    }

    #[test]
    fn edge_point_cardinal() {
        let (nx, ny) = edge_point(0.0, 4.0 / 3.0, 0.06);
        assert!(close(nx, 0.5, 0.02) && ny < 0.1, "北应在顶部中间: {nx},{ny}");
        let (ex, ey) = edge_point(90.0, 4.0 / 3.0, 0.06);
        assert!(ex > 0.9 && close(ey, 0.5, 0.02), "东应在右侧中间: {ex},{ey}");
        let (sx, sy) = edge_point(180.0, 4.0 / 3.0, 0.06);
        assert!(close(sx, 0.5, 0.02) && sy > 0.9, "南应在底部中间: {sx},{sy}");
        let (wx, wy) = edge_point(270.0, 4.0 / 3.0, 0.06);
        assert!(wx < 0.1 && close(wy, 0.5, 0.02), "西应在左侧中间: {wx},{wy}");
    }

    #[test]
    fn point_in_poly_basic() {
        let sq = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)];
        assert!(point_in_poly((1.0, 1.0), &sq));
        assert!(!point_in_poly((3.0, 1.0), &sq));
        assert!(!point_in_poly((1.0, 1.0), &[(0.0, 0.0), (1.0, 1.0)]));
    }
}
