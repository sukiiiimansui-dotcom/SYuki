//! 世界模拟的**移动状态机**（P4-2 / P4-3 的纯逻辑层）。
//!
//! 一条「行程」= 出发地 → 目的地 + 出行方式 + 出发时刻 + 速度。位置**不靠定时器
//! 累积步进**，而是每次查询时按时间戳重算：
//! ```text
//! progress = clamp((已走米数 + (now - anchor) * 速度) / 总距离, 0, 1)
//! pos      = lerp(出发地, 目的地, progress)
//! ```
//! 这是机主定的硬要求：手机会息屏 / 切后台 / 冻进程，定时器会被暂停甚至杀掉；
//! 只要"当前时刻"对得上，回到前台时位置立刻自洽 —— 不会出现"人还停在半路"
//! 或者"时间倒了位置也倒着走"。
//!
//! 线程模型：进程级 [`REGISTRY`]（`std::sync::Mutex`），**没有 async**。
//! 选 `std` 而不是 `tokio::RwLock`：这里全是"读一眼/写一笔"的微秒级操作，
//! 不涉及 IO，`std` 锁反而不会被 async 上下文坑到（也不需要在 `Drop` 里 await）。
//!
//! ## 为什么本文件不 `use tauri`
//!
//! 命令层在 [`super::move_cmd`]。拆开是为了让"速度表 / 插值 / 边界钳制 / 目的地
//! 解析"这些真正容易错的东西能**脱离 Tauri 工程单独 `rustc --test`** 跑单测
//! （手机上没有 40 分钟的编译预算，见 `~/chk/p4/wm_shim.rs`）。
//! 本文件只依赖 `serde_json` 和同层的纯函数模块 [`super::summary`]。
//!
//! ## 距离口径
//!
//! 复用 [`summary::distance_m`]（格点 × 格边长）与 [`summary::geo_distance_m`]
//! （经纬度 haversine，与 `coord::haversine_m` 是同一套公式）。**刻意不另起一套**：
//! 注入文本里「附近：咖啡馆(80m)」用的就是 `summary::distance_m`，行程耗时要是
//! 换一把尺子，AI 看到的距离和它走路要花的时间就会互相打脸。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};

use serde_json::{json, Value};

use super::summary;

/// 起点/终点都缺格点坐标时用的默认格边长（`facilities::DEFAULT_CELL_METERS` = 30）
pub const DEFAULT_CELL_M: f64 = 30.0;
/// 100× 加速开关的两个档位（用户可选）
pub const SPEEDUP_OFF: f64 = 1.0;
/// 100× 档
pub const SPEEDUP_FAST: f64 = 100.0;
/// 允许的最大加速倍率（命令层会夹到这个区间）
pub const SPEEDUP_MAX: f64 = 1000.0;
/// 短途默认走路的距离上限（米）——机主给的 2km
pub const WALK_MAX_M: f64 = 2_000.0;
/// `depart_in_secs` 的上限（一天）：防止前端传个天文数字把定时器撑爆
pub const MAX_DEPART_SECS: f64 = 86_400.0;

// ═══════════════════════════════════════════════════════════════════
//  出行方式（P4-3）
// ═══════════════════════════════════════════════════════════════════

/// 九种出行方式。速度按机主给的现实值（米/秒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveKind {
    Walk,
    Bike,
    Ebike,
    Bus,
    Subway,
    Taxi,
    Car,
    Train,
    Plane,
}

impl MoveKind {
    /// 全部九种（顺序 = 由慢到快，`auto_for` 的阶梯也按它排）
    pub const ALL: [MoveKind; 9] = [
        MoveKind::Walk,
        MoveKind::Bike,
        MoveKind::Ebike,
        MoveKind::Bus,
        MoveKind::Subway,
        MoveKind::Taxi,
        MoveKind::Car,
        MoveKind::Train,
        MoveKind::Plane,
    ];

    /// 英文键（写进指令、事件、前端契约的那个）
    pub fn key(self) -> &'static str {
        match self {
            MoveKind::Walk => "walk",
            MoveKind::Bike => "bike",
            MoveKind::Ebike => "ebike",
            MoveKind::Bus => "bus",
            MoveKind::Subway => "subway",
            MoveKind::Taxi => "taxi",
            MoveKind::Car => "car",
            MoveKind::Train => "train",
            MoveKind::Plane => "plane",
        }
    }

    /// 中文名（事件文案用）
    pub fn zh(self) -> &'static str {
        match self {
            MoveKind::Walk => "步行",
            MoveKind::Bike => "骑车",
            MoveKind::Ebike => "电动车",
            MoveKind::Bus => "公交",
            MoveKind::Subway => "地铁",
            MoveKind::Taxi => "打车",
            MoveKind::Car => "开车",
            MoveKind::Train => "高铁",
            MoveKind::Plane => "飞机",
        }
    }

    /// 现实速度（米/秒）。机主给的口径：
    /// 步行 1.4、自行车 4.2、电动车 6、城市道路 8~14、高铁 70、飞机 220。
    pub fn speed_mps(self) -> f64 {
        match self {
            MoveKind::Walk => 1.4,
            MoveKind::Bike => 4.2,
            MoveKind::Ebike => 6.0,
            // 公交要停站，取下限；地铁没红灯取中；出租/私家车取城市快速路一档
            MoveKind::Bus => 8.0,
            MoveKind::Subway => 11.0,
            MoveKind::Taxi => 10.0,
            MoveKind::Car => 12.0,
            MoveKind::Train => 70.0,
            MoveKind::Plane => 220.0,
        }
    }

    /// 认出行方式：英文键（大小写不敏感）+ 中文别名。
    /// 认不出来返回 `None` —— 调用方按"自动选"处理，**绝不瞎猜一个方式**。
    pub fn parse(raw: &str) -> Option<MoveKind> {
        let s = raw.trim().to_lowercase();
        if s.is_empty() {
            return None;
        }
        let hit = match s.as_str() {
            "walk" | "walking" | "foot" | "onfoot" | "步行" | "走路" | "走过去" => MoveKind::Walk,
            "bike" | "bicycle" | "cycling" | "自行车" | "单车" | "骑行" | "共享单车" => MoveKind::Bike,
            "ebike" | "e-bike" | "scooter" | "电动车" | "电瓶车" | "小电驴" => MoveKind::Ebike,
            "bus" | "coach" | "公交" | "巴士" | "公交车" | "坐公交" => MoveKind::Bus,
            "subway" | "metro" | "underground" | "地铁" | "坐地铁" => MoveKind::Subway,
            "taxi" | "cab" | "didi" | "出租" | "出租车" | "打车" | "叫车" => MoveKind::Taxi,
            "car" | "drive" | "driving" | "开车" | "自驾" | "私家车" | "坐车" => MoveKind::Car,
            "train" | "rail" | "hsr" | "高铁" | "火车" | "动车" | "坐高铁" => MoveKind::Train,
            "plane" | "airplane" | "flight" | "飞机" | "航班" | "坐飞机" => MoveKind::Plane,
            _ => return None,
        };
        Some(hit)
    }

    /// 距离 → 自动选出行方式（P4-3）。
    ///
    /// 阶梯：< 2km 走路（机主定的），往后按"这段路现实中人们会怎么走"排：
    /// 2~6km 骑车、6~25km 公交、25~120km 开车、120~800km 高铁、再远飞机。
    /// `kind` 显式给定时以指定为准 —— 这条判断在 [`dispatch`] 里，不在这里。
    pub fn auto_for(distance_m: f64) -> MoveKind {
        if !distance_m.is_finite() || distance_m < WALK_MAX_M {
            MoveKind::Walk
        } else if distance_m < 6_000.0 {
            MoveKind::Bike
        } else if distance_m < 25_000.0 {
            MoveKind::Bus
        } else if distance_m < 120_000.0 {
            MoveKind::Car
        } else if distance_m < 800_000.0 {
            MoveKind::Train
        } else {
            MoveKind::Plane
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  地点与坐标系
// ═══════════════════════════════════════════════════════════════════

/// 位置插值所在的坐标系。
///
/// 小区地图里设施和角色都只有**格点**（`gx/gy` × `cell_m`），跨城/长距离才可能
/// 只有经纬度。两套坐标**不能混着插值**（格点没有地理锚点就没法换算），
/// 所以行程一开始就把坐标系定死，之后一直在同一套里 lerp。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    Grid,
    Geo,
}

impl Space {
    pub fn key(self) -> &'static str {
        match self {
            Space::Grid => "grid",
            Space::Geo => "geo",
        }
    }
}

/// 一个地点：名字 + 可选的格点/经纬度。
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    /// 格点 x（列）
    pub gx: Option<f64>,
    /// 格点 y（行，向下增大）
    pub gy: Option<f64>,
    pub lng: Option<f64>,
    pub lat: Option<f64>,
    /// 格边长（米）：目的地自己的 `cell_meters` 优先，缺了用默认 30
    pub cell_m: f64,
}

impl Place {
    /// 从地图数据（设施 / 角色 / 玩家位置）里读一个地点 —— 字段名口径跟
    /// [`summary::cell_xy`] / [`summary::lng_lat`] 完全一致（`x|gx`、`grid|pos|cell` 数组都认）。
    pub fn from_value(name: &str, v: &Value, fallback_cell_m: f64) -> Self {
        let (gx, gy) = summary::cell_xy(v);
        let (lng, lat) = summary::lng_lat(v);
        let cell = num(v, "cell_meters")
            .or_else(|| num(v, "cell_m"))
            .filter(|c| *c > 0.0)
            .unwrap_or(if fallback_cell_m > 0.0 {
                fallback_cell_m
            } else {
                DEFAULT_CELL_M
            });
        Self {
            name: name.to_string(),
            gx,
            gy,
            lng,
            lat,
            cell_m: cell,
        }
    }

    pub fn has_grid(&self) -> bool {
        self.gx.is_some() && self.gy.is_some()
    }

    pub fn has_geo(&self) -> bool {
        self.lng.is_some() && self.lat.is_some()
    }

    /// 两者能共用哪套坐标：经纬度优先（haversine 比"格 × 边长"更贴真实距离）。
    pub fn space_with(&self, other: &Place) -> Option<Space> {
        if self.has_geo() && other.has_geo() {
            Some(Space::Geo)
        } else if self.has_grid() && other.has_grid() {
            Some(Space::Grid)
        } else {
            None
        }
    }

    /// 两点距离（米）：拿不到共同坐标系返回 `None`。
    /// 同一个点返回 `Some(0.0)`（不是 `None`）—— 那是"原地到达"，不是"算不出来"。
    pub fn distance_to(&self, other: &Place) -> Option<f64> {
        match self.space_with(other)? {
            Space::Geo => summary::geo_distance_m(
                (self.lng, self.lat),
                (other.lng, other.lat),
            ),
            Space::Grid => {
                let (ax, ay) = (self.gx?, self.gy?);
                let (bx, by) = (other.gx?, other.gy?);
                // 目的地的格边长更贴它的实际尺寸（设施自带 cell_meters）
                let cell = if other.cell_m > 0.0 { other.cell_m } else { self.cell_m };
                Some((ax - bx).hypot(ay - by) * cell)
            }
        }
    }

    /// 在指定坐标系里的坐标点。
    pub fn point(&self, space: Space) -> Option<(f64, f64)> {
        match space {
            Space::Grid => Some((self.gx?, self.gy?)),
            Space::Geo => Some((self.lng?, self.lat?)),
        }
    }
}

/// 读数字（`summary` 内部同名函数没导出，这里是三行的等价物）
fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

// ═══════════════════════════════════════════════════════════════════
//  行程
// ═══════════════════════════════════════════════════════════════════

/// 行程状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TripStatus {
    /// 已登记、还没到出发时刻（`depart_ms` 在未来）
    Pending,
    /// 在路上
    Moving,
    /// 已到达（终态）
    Arrived,
    /// 已取消（终态，位置冻结在取消那一刻）
    Cancelled,
}

impl TripStatus {
    pub fn key(self) -> &'static str {
        match self {
            TripStatus::Pending => "pending",
            TripStatus::Moving => "moving",
            TripStatus::Arrived => "arrived",
            TripStatus::Cancelled => "cancelled",
        }
    }
}

/// 规划失败的原因（命令层/派发层翻成人话日志）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanError {
    /// 不知道是谁在移动（`current_role` 为空）
    NoRole,
    /// 这个角色在地图上还没有位置（`actors` / `me` 都没有格点）
    NoOrigin,
    /// 目的地名字在地图数据里找不到
    NoDestination,
    /// 目的地和当前位置不在同一套坐标系（格点 ↔ 经纬度，没有锚点换不了）
    NoSharedSpace,
}

impl PlanError {
    pub fn zh(self) -> &'static str {
        match self {
            PlanError::NoRole => "不知道是谁在移动（current_role 为空）",
            PlanError::NoOrigin => "这个角色在地图上还没有位置",
            PlanError::NoDestination => "地图数据里没有这个目的地",
            PlanError::NoSharedSpace => "目的地和当前位置不在同一套坐标系里",
        }
    }
}

/// 一条行程。
///
/// 时间字段全是 **unix 毫秒**：100× 加速下 1 米只要 7ms，秒级精度不够；
/// 用 wall clock（不是 `Instant`）是刻意的 —— 进程被冻/被杀之后，
/// 只要时钟对得上，位置就还能算出来（见模块注释）。
#[derive(Debug, Clone)]
pub struct Trip {
    pub id: u64,
    pub role: String,
    pub from: Place,
    pub to: Place,
    pub kind: MoveKind,
    /// `kind` 是 AI 显式指定的（true）还是按距离自动选的（false）
    pub kind_explicit: bool,
    pub space: Space,
    /// 出发地到目的地的直线距离（米）
    pub distance_m: f64,
    /// 1× 的现实速度（米/秒）
    pub speed_mps: f64,
    /// 加速倍率（用户开关；只改推进速率，不改语义）
    pub speedup: f64,
    pub created_ms: u64,
    /// 出发时刻（unix 毫秒）；`= created_ms` 表示立刻出发
    pub depart_ms: u64,
    /// 上一次"重锚"的时刻：从这一刻起按 `speed_eff` 推进
    pub anchor_ms: u64,
    /// 锚点之前已经走掉的米数
    pub done_m: f64,
    /// 终态（`Pending`/`Moving` 是"还没结果"，由时间戳推算）
    pub status: TripStatus,
    pub arrived_ms: Option<u64>,
    pub cancelled_ms: Option<u64>,
}

impl Trip {
    /// 规划一条行程。`kind = None` 时按距离自动选（P4-3）。
    ///
    /// `depart_ms` 在未来 → 状态是 `Pending`（给"等两分钟再出发"留的口子）。
    #[allow(clippy::too_many_arguments)]
    pub fn plan(
        id: u64,
        role: impl Into<String>,
        from: Place,
        to: Place,
        kind: Option<MoveKind>,
        speedup: f64,
        now_ms: u64,
        depart_ms: u64,
    ) -> Result<Trip, PlanError> {
        let space = from.space_with(&to).ok_or(PlanError::NoSharedSpace)?;
        let distance_m = from.distance_to(&to).unwrap_or(0.0);
        let kind_explicit = kind.is_some();
        let kind = kind.unwrap_or_else(|| MoveKind::auto_for(distance_m));
        let depart_ms = depart_ms.max(now_ms);
        Ok(Trip {
            id,
            role: role.into(),
            from,
            to,
            kind,
            kind_explicit,
            space,
            distance_m,
            speed_mps: kind.speed_mps(),
            speedup: normalize_speedup(speedup),
            created_ms: now_ms,
            depart_ms,
            anchor_ms: depart_ms,
            done_m: 0.0,
            status: if now_ms < depart_ms {
                TripStatus::Pending
            } else {
                TripStatus::Moving
            },
            arrived_ms: None,
            cancelled_ms: None,
        })
    }

    /// 当前生效速度（米/秒）= 现实速度 × 加速倍率。
    pub fn speed_eff(&self) -> f64 {
        self.speed_mps * normalize_speedup(self.speedup)
    }

    /// 到 `now` 为止已经走掉的米数（终态分别冻结在终点/取消点）。
    pub fn covered_m(&self, now_ms: u64) -> f64 {
        match self.status {
            TripStatus::Arrived => self.distance_m.max(0.0),
            TripStatus::Cancelled => self.done_m.min(self.distance_m.max(0.0)),
            _ => {
                let elapsed = now_ms.saturating_sub(self.anchor_ms) as f64 / 1000.0;
                (self.done_m + elapsed * self.speed_eff()).min(self.distance_m.max(0.0))
            }
        }
    }

    /// 进度 0..=1（`now` 早于出发时刻恒为 0）。
    pub fn progress_at(&self, now_ms: u64) -> f64 {
        if self.status == TripStatus::Cancelled {
            return if self.distance_m > 0.0 {
                (self.done_m / self.distance_m).clamp(0.0, 1.0)
            } else {
                1.0
            };
        }
        if now_ms < self.depart_ms {
            return 0.0;
        }
        if self.distance_m <= 0.0 {
            return 1.0;
        }
        (self.covered_m(now_ms) / self.distance_m).clamp(0.0, 1.0)
    }

    /// 位置：`lerp(出发地, 目的地, progress)` —— 全文件唯一一处位置来源。
    pub fn position_at(&self, now_ms: u64) -> (f64, f64) {
        let p = self.progress_at(now_ms);
        let a = self.from.point(self.space).unwrap_or((0.0, 0.0));
        let b = self.to.point(self.space).unwrap_or(a);
        (a.0 + (b.0 - a.0) * p, a.1 + (b.1 - a.1) * p)
    }

    /// 还剩多少米。
    pub fn remaining_m(&self, now_ms: u64) -> f64 {
        (self.distance_m - self.covered_m(now_ms)).max(0.0)
    }

    /// 1× 的现实耗时（秒）—— 给"预计 25 分钟"这种文案用。
    pub fn duration_secs(&self) -> f64 {
        if self.speed_mps > 0.0 {
            self.distance_m / self.speed_mps
        } else {
            0.0
        }
    }

    /// 按**当前加速倍率**到达的墙钟时刻（unix 毫秒）。
    ///
    /// 一次性定时器就用它来 sleep（不是每秒 tick）；加速开关一拨要重算。
    pub fn arrive_at_ms(&self, now_ms: u64) -> u64 {
        if matches!(self.status, TripStatus::Arrived | TripStatus::Cancelled) {
            return now_ms;
        }
        let base = now_ms.max(self.depart_ms);
        let remain = self.remaining_m(base);
        base + (remain / self.speed_eff() * 1000.0).round() as u64
    }

    /// 按时间戳推状态（`Pending`/`Moving`/`Arrived`；`Cancelled` 是终态不参与）。
    pub fn status_at(&self, now_ms: u64) -> TripStatus {
        match self.status {
            TripStatus::Cancelled => TripStatus::Cancelled,
            _ => {
                if now_ms < self.depart_ms {
                    TripStatus::Pending
                } else if self.progress_at(now_ms) >= 1.0 {
                    TripStatus::Arrived
                } else {
                    TripStatus::Moving
                }
            }
        }
    }

    /// 重锚：把"已经走的米数"折算进 `done_m`，后面按新倍率推进。
    ///
    /// 加速开关可以**半路拨**：重锚保证进度连续（不会因为拨开关而瞬移），
    /// 只把剩下的路按新速度跑。
    pub fn reanchor(&mut self, now_ms: u64, speedup: f64) {
        if matches!(self.status, TripStatus::Arrived | TripStatus::Cancelled) {
            return;
        }
        self.done_m = self.covered_m(now_ms);
        self.anchor_ms = now_ms.max(self.depart_ms);
        self.speedup = normalize_speedup(speedup);
    }

    /// 到点收口：真的越过终点时把状态落成 `Arrived`，返回 `true`（**只成功一次**）。
    ///
    /// 一次性定时器和前端的轮询都会调它，幂等靠这里的状态判断保证 ——
    /// 事件只会写一条（见 `move_cmd::publish_arrivals`）。
    pub fn refresh(&mut self, now_ms: u64) -> bool {
        if matches!(self.status, TripStatus::Arrived | TripStatus::Cancelled) {
            return false;
        }
        if self.status_at(now_ms) == TripStatus::Arrived {
            self.done_m = self.distance_m;
            self.anchor_ms = now_ms;
            self.status = TripStatus::Arrived;
            self.arrived_ms = Some(now_ms);
            // 距离为 0 的"原地到达"：到达时刻记成出发时刻更符合直觉
            if self.distance_m <= 0.0 {
                self.arrived_ms = Some(self.depart_ms.max(self.created_ms));
            }
            return true;
        }
        false
    }

    /// 取消行程（位置冻结在当前进度）。
    pub fn cancel(&mut self, now_ms: u64) -> bool {
        if matches!(self.status, TripStatus::Arrived | TripStatus::Cancelled) {
            return false;
        }
        self.done_m = self.covered_m(now_ms);
        self.anchor_ms = now_ms;
        self.status = TripStatus::Cancelled;
        self.cancelled_ms = Some(now_ms);
        true
    }

    /// 给前端/调试看的行程快照（字段就是接口契约，见交付报告）。
    pub fn view(&self, now_ms: u64) -> Value {
        let (x, y) = self.position_at(now_ms);
        let pos = match self.space {
            Space::Grid => json!({ "gx": round3(x), "gy": round3(y) }),
            Space::Geo => json!({ "lng": round6(x), "lat": round6(y) }),
        };
        let status = self.status_at(now_ms);
        let remain = self.remaining_m(now_ms);
        let eta_ms = self.arrive_at_ms(now_ms);
        json!({
            "id": self.id,
            "role": self.role,
            "status": status.key(),
            "kind": self.kind.key(),
            "kind_zh": self.kind.zh(),
            "kind_explicit": self.kind_explicit,
            "space": self.space.key(),
            "from": { "name": self.from.name, "gx": self.from.gx, "gy": self.from.gy,
                      "lng": self.from.lng, "lat": self.from.lat },
            "to": { "name": self.to.name, "gx": self.to.gx, "gy": self.to.gy,
                    "lng": self.to.lng, "lat": self.to.lat },
            "pos": pos,
            "distance_m": round1(self.distance_m),
            "done_m": round1(self.covered_m(now_ms)),
            "remaining_m": round1(remain),
            "progress": (self.progress_at(now_ms) * 1000.0).round() / 1000.0,
            "speed_mps": self.speed_mps,
            "speed_eff_mps": round3(self.speed_eff()),
            "speedup": self.speedup,
            "duration_secs": round1(self.duration_secs()),
            "eta_secs": round1(remain / self.speed_mps.max(f64::MIN_POSITIVE)),
            "eta_secs_eff": round1(remain / self.speed_eff().max(f64::MIN_POSITIVE)),
            "created_ms": self.created_ms,
            "depart_ms": self.depart_ms,
            "arrive_at_ms": eta_ms,
            "eta_1x_ms": self.depart_ms
                + (self.distance_m / self.speed_mps.max(f64::MIN_POSITIVE) * 1000.0).round() as u64,
            "arrived_ms": self.arrived_ms,
            "cancelled_ms": self.cancelled_ms,
        })
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}
fn round6(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}

/// 倍率归一：非有限值/≤0 一律当 1.0，上限 [`SPEEDUP_MAX`]。
pub fn normalize_speedup(v: f64) -> f64 {
    if !v.is_finite() || v <= 0.0 {
        SPEEDUP_OFF
    } else {
        v.clamp(SPEEDUP_OFF, SPEEDUP_MAX)
    }
}

/// `depart_in_secs` 归一：非有限值/≤0 当"立刻出发"，上限 [`MAX_DEPART_SECS`]。
pub fn normalize_depart_secs(v: f64) -> f64 {
    if !v.is_finite() || v <= 0.0 {
        0.0
    } else {
        v.min(MAX_DEPART_SECS)
    }
}

// ═══════════════════════════════════════════════════════════════════
//  事件文案（同时是注入文本里「最近：…」那一行的来源）
// ═══════════════════════════════════════════════════════════════════

/// 「小满动身去咖啡馆（步行，约 70m）」
///
/// 距离用 [`summary::fmt_dist`] —— 与注入里「附近：咖啡馆(80m)」同一套写法。
pub fn move_text(role: &str, to: &str, kind: MoveKind, distance_m: f64) -> String {
    format!(
        "{role}动身去{to}（{}，约 {}）",
        kind.zh(),
        summary::fmt_dist(distance_m)
    )
}

/// 「小满到了咖啡馆」
pub fn arrive_text(role: &str, to: &str) -> String {
    format!("{role}到了{to}")
}

/// 「小满不去咖啡馆了」
pub fn cancel_text(role: &str, to: &str) -> String {
    format!("{role}不去{to}了")
}

// ═══════════════════════════════════════════════════════════════════
//  目的地 / 出发地解析（纯函数，喂的是 MapRuntime 的快照 JSON）
// ═══════════════════════════════════════════════════════════════════

/// 地名里的**修饰前缀**："楼下便利店" → "便利店"、"去咖啡馆" → "咖啡馆"。
/// 模型很爱加这些词，而设施表里的名字是"星洲咖啡馆"这种 —— 不剥就一条都对不上。
const PLACE_MODIFIERS: [&str; 17] = [
    "楼下", "楼上", "附近的", "附近", "旁边的", "旁边", "门口的", "门口", "外面的", "那边", "这边",
    "那家", "这家", "我们", "这里的", "这里", "去",
];

/// 剥掉修饰前缀，露出地名主干（剥不动就原样返回）。
///
/// 每剥一次长度都会变短，所以循环一定收敛；剥到只剩 1 个字就停手
/// （再剥下去"便利店"会变成"店"，那种子串匹配等于乱命中）。
fn core_name(q: &str) -> &str {
    let mut s = q.trim();
    loop {
        let mut changed = false;
        for m in PLACE_MODIFIERS {
            if let Some(rest) = s.strip_prefix(m) {
                if rest.chars().count() >= 2 {
                    s = rest;
                    changed = true;
                }
            }
        }
        if !changed {
            return s;
        }
    }
}

/// 名字互相包含（长度不足 2 的不参与，避免"店"这种字把整张表都命中）。
fn name_contains(name_lower: &str, q_lower: &str) -> bool {
    if q_lower.chars().count() < 2 || name_lower.chars().count() < 2 {
        return false;
    }
    name_lower.contains(q_lower) || q_lower.contains(name_lower)
}

/// 特殊目的地别名 → 走设施类型匹配。
/// （"回家"落到最近的住宅楼；模型很爱说"回家"）
fn alias_type(query: &str) -> Option<&'static [&'static str]> {
    let q = query.trim();
    if q.contains('家') && !q.contains("大家") {
        Some(&["residential", "住宅"])
    } else if q.contains("公司") || q.contains("上班") {
        Some(&["office", "写字楼", "公司"])
    } else {
        None
    }
}

/// 目的地解析：在**当前地图数据**里找一个地点。
///
/// 顺序（前一条命中就不看后面的）：
/// 1. 设施名**完全一致**（忽略大小写与首尾空白；原词与剥掉修饰前缀的"主干"都算）
/// 2. 设施名互相包含（"咖啡馆" → "星洲咖啡馆"；"楼下便利店" → "便利店"）
/// 3. 别名（"回家" → 住宅楼）与类型名（`type_zh` / `type` / `group`）
/// 4. 其他角色名（去谁那儿）→ `actors`
/// 5. "用户/玩家/我" → `me`
///
/// 多条命中时取**离 `origin` 最近**的一条（"去便利店"当然去楼下那家）。
pub fn resolve_destination(snapshot: &Value, query: &str, origin: Option<&Place>) -> Option<Place> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let cell = num(snapshot, "cell_m").unwrap_or(DEFAULT_CELL_M);
    let ql = q.to_lowercase();
    // "楼下咖啡馆" → "咖啡馆"：剥完再比一次，模型的口语化说法也能落到具体设施上
    let core = core_name(q).to_lowercase();
    let core = if core.is_empty() { ql.clone() } else { core };

    // ① / ② 设施
    let mut best: Option<(u8, f64, Place)> = None;
    if let Some(list) = snapshot.get("facilities").and_then(Value::as_array) {
        let alias = alias_type(q);
        for f in list {
            let name = f.get("name").and_then(Value::as_str).unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            let nl = name.to_lowercase();
            let type_keys: Vec<String> = ["type", "type_zh", "group"]
                .iter()
                .filter_map(|k| f.get(*k).and_then(Value::as_str))
                .map(|s| s.to_lowercase())
                .collect();

            let score = if nl == ql || nl == core {
                5u8
            } else if name_contains(&nl, &ql) || name_contains(&nl, &core) {
                4
            } else if let Some(types) = alias {
                if type_keys
                    .iter()
                    .any(|t| types.iter().any(|want| want.to_lowercase() == *t))
                {
                    3
                } else {
                    0
                }
            } else if type_keys.iter().any(|t| *t == ql || t.contains(&ql) || ql.contains(t)) {
                2
            } else {
                0
            };
            if score == 0 {
                continue;
            }
            let place = Place::from_value(name, f, cell);
            if !place.has_grid() && !place.has_geo() {
                continue;
            }
            let dist = origin
                .and_then(|o| o.distance_to(&place))
                .unwrap_or(f64::INFINITY);
            let better = match &best {
                None => true,
                Some((bs, bd, _)) => score > *bs || (score == *bs && dist < *bd),
            };
            if better {
                best = Some((score, dist, place));
            }
        }
    }
    if let Some((_, _, p)) = best {
        return Some(p);
    }

    // ④ 其他角色（"去阿离那儿"也能落到人身上；同样先剥修饰前缀）
    if let Some(actors) = snapshot.get("actors").and_then(Value::as_object) {
        for (name, v) in actors {
            let nl = name.to_lowercase();
            if nl == ql || nl == core || name.contains(q) || name.contains(&core) {
                let p = Place::from_value(name, v, cell);
                if p.has_grid() || p.has_geo() {
                    return Some(p);
                }
            }
        }
    }

    // ⑤ 玩家位置
    if matches!(ql.as_str(), "用户" | "玩家" | "我" | "me" | "user" | "你") {
        if let Some(me) = snapshot.get("me") {
            let p = Place::from_value("用户", me, cell);
            if p.has_grid() || p.has_geo() {
                return Some(p);
            }
        }
    }

    None
}

/// 出发地解析：角色当前在地图上的位置（角色没落位就退到玩家位置）。
///
/// 与注入 `summary::render` 的口径一致（那里也是"角色没有位置就用玩家的"）。
pub fn resolve_origin(snapshot: &Value, role: &str) -> Option<Place> {
    let cell = num(snapshot, "cell_m").unwrap_or(DEFAULT_CELL_M);
    if let Some(v) = snapshot
        .get("actors")
        .and_then(|a| a.get(role))
        .filter(|v| !v.is_null())
    {
        let p = Place::from_value(role, v, cell);
        if p.has_grid() || p.has_geo() {
            return Some(p);
        }
    }
    let me = snapshot.get("me").filter(|v| !v.is_null())?;
    let p = Place::from_value("用户位置", me, cell);
    if p.has_grid() || p.has_geo() {
        Some(p)
    } else {
        None
    }
}

/// 一次位置指令 → 一条行程（派发的核心，纯函数）。
///
/// `kind_raw` 显式给定时以它为准（认不出来就退回自动选，并记 `kind_explicit=false`）。
pub fn dispatch(
    snapshot: &Value,
    role: &str,
    to: &str,
    kind_raw: Option<&str>,
    now_ms: u64,
    speedup: f64,
) -> Result<Trip, PlanError> {
    let role = role.trim();
    if role.is_empty() {
        return Err(PlanError::NoRole);
    }
    let from = resolve_origin(snapshot, role).ok_or(PlanError::NoOrigin)?;
    let dest = resolve_destination(snapshot, to, Some(&from)).ok_or(PlanError::NoDestination)?;
    let kind = kind_raw.and_then(MoveKind::parse);
    let id = next_id();
    let mut trip = Trip::plan(id, role, from, dest, kind, speedup, now_ms, now_ms)?;
    // 显式方式认不出来时不假装"是指定的"：让前端能看到真实来源
    trip.kind_explicit = kind.is_some();
    Ok(trip)
}

// ═══════════════════════════════════════════════════════════════════
//  进程级注册表（一个角色同时只有一条行程）
// ═══════════════════════════════════════════════════════════════════

/// 墙钟毫秒（unix epoch）。设备时间异常（1970 之前）按 0 处理，不让它变负数。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct Registry {
    trips: HashMap<String, Trip>,
    next_id: u64,
    speedup: f64,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| {
    Mutex::new(Registry {
        trips: HashMap::new(),
        next_id: 1,
        speedup: SPEEDUP_OFF,
    })
});

/// 拿锁：**中毒也继续用**（地图这类旁路状态不值得因为一次 panic 把整条路都废掉）
fn lock() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// 当前加速倍率。
pub fn speedup() -> f64 {
    lock().speedup
}

/// 设置加速倍率（夹到 `1.0..=SPEEDUP_MAX`），并对**进行中的行程重锚**：
/// 已经走过的路不算白走，剩下的路按新倍率跑（进度连续，不瞬移）。返回生效值。
pub fn set_speedup(v: f64, now_ms: u64) -> f64 {
    let v = normalize_speedup(v);
    let mut g = lock();
    g.speedup = v;
    for trip in g.trips.values_mut() {
        trip.reanchor(now_ms, v);
    }
    v
}

/// 取一个新行程 id。
pub fn next_id() -> u64 {
    let mut g = lock();
    let id = g.next_id;
    g.next_id = g.next_id.saturating_add(1);
    id
}

/// 登记一条行程（同角色的旧行程被顶替并返回，调用方负责记一条事件）。
pub fn start(trip: Trip) -> Option<Trip> {
    lock().trips.insert(trip.role.clone(), trip)
}

/// 读某角色的行程（克隆一份，锁外使用）。
///
/// **只读，不改进程内状态**：状态是按时间戳推出来的（`view` 里用 `status_at`），
/// 这里要是顺手把存储的 `status` 翻成 `Arrived`，`refresh` 之后就再也返回不了
/// `Some` —— 那条"到达事件"就永远写不出来了。收口只能走 [`refresh`]。
pub fn get(role: &str, _now_ms: u64) -> Option<Trip> {
    lock().trips.get(role).cloned()
}

/// 某角色的行程视图（给命令用）。
pub fn view_of(role: &str, now_ms: u64) -> Option<Value> {
    get(role, now_ms).map(|t| t.view(now_ms))
}

/// 全部行程视图（按 id 升序，前端好做稳定渲染）。
pub fn views(now_ms: u64) -> Vec<Value> {
    let roles: Vec<String> = {
        let g = lock();
        let mut v: Vec<(u64, String)> = g.trips.iter().map(|(k, t)| (t.id, k.clone())).collect();
        v.sort();
        v.into_iter().map(|(_, k)| k).collect()
    };
    roles
        .into_iter()
        .filter_map(|r| view_of(&r, now_ms))
        .collect()
}

/// 到点收口（**幂等**）：真的跨过终点时返回那条行程的克隆，其余情况 `None`。
///
/// 事件只写一条就靠它：定时器到点调一次、前端轮询又调一次，第二次必然返回 `None`。
pub fn refresh(role: &str, now_ms: u64) -> Option<Trip> {
    let mut g = lock();
    let trip = g.trips.get_mut(role)?;
    if trip.refresh(now_ms) {
        Some(trip.clone())
    } else {
        None
    }
}

/// 全部行程的到点收口，返回**本次真的到达**的那些。
pub fn refresh_all(now_ms: u64) -> Vec<Trip> {
    let roles: Vec<String> = lock().trips.keys().cloned().collect();
    roles.into_iter().filter_map(|r| refresh(&r, now_ms)).collect()
}

/// 取消行程；返回取消后的行程（已到达/已取消/不存在都返回 `None`）。
pub fn cancel(role: &str, now_ms: u64) -> Option<Trip> {
    let mut g = lock();
    let trip = g.trips.get_mut(role)?;
    if trip.cancel(now_ms) {
        Some(trip.clone())
    } else {
        None
    }
}

/// 还在路上的行程（角色名, 预计到达毫秒）—— 命令层用它重挂一次性定时器。
pub fn moving_etas(now_ms: u64) -> Vec<(String, u64)> {
    let g = lock();
    let mut v: Vec<(String, u64)> = g
        .trips
        .values()
        .filter(|t| !matches!(t.status, TripStatus::Arrived | TripStatus::Cancelled))
        .map(|t| (t.role.clone(), t.arrive_at_ms(now_ms)))
        .collect();
    v.sort();
    v
}

/// 清空注册表与倍率（**只给单测/调试用**：进程内状态，没有持久化）。
pub fn reset() {
    let mut g = lock();
    g.trips.clear();
    g.next_id = 1;
    g.speedup = SPEEDUP_OFF;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 注册表是**进程级单例**，而 `cargo test` 是多线程跑的：
    /// 两个动注册表的用例必须串行，否则互相 `reset()` 会把对方的数据清掉。
    fn registry_guard() -> MutexGuard<'static, ()> {
        static L: Mutex<()> = Mutex::new(());
        L.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 一份典型的地图快照（字段口径抄 `state.rs` 单测里的真实形状）。
    fn snapshot() -> Value {
        json!({
            "scene": {"area": "广州市·越秀区·东山口"},
            "me": {"area": "广州市·越秀区", "gx": 13.0, "gy": 4.0},
            "actors": {"小满": {"facility": "便利店", "x": 3.0, "y": 4.0, "since": "19:20"}},
            "facilities": [
                {"id": "s001", "gx": 3, "gy": 4, "type": "commercial", "type_zh": "商业", "group": "life", "name": "便利店", "cell_meters": 30.0},
                {"id": "s002", "gx": 5, "gy": 5, "type": "commercial", "type_zh": "商业", "group": "life", "name": "星洲咖啡馆", "cell_meters": 30.0},
                {"id": "s003", "gx": 30, "gy": 40, "type": "commercial", "type_zh": "商业", "group": "life", "name": "便利店(南门)", "cell_meters": 30.0},
                {"id": "m001", "gx": 40, "gy": 30, "type": "medical", "type_zh": "医疗", "group": "life", "name": "市第一医院", "cell_meters": 30.0},
                {"id": "t001", "gx": 9, "gy": 6, "type": "subway", "type_zh": "地铁站", "group": "transport", "name": "东山口地铁站", "cell_meters": 30.0},
                {"id": "r001", "gx": 1, "gy": 1, "type": "residential", "type_zh": "住宅", "group": "life", "name": "3号楼", "cell_meters": 30.0}
            ],
            "cell_m": 30.0,
            "current_role": "小满"
        })
    }

    fn grid_place(name: &str, gx: f64, gy: f64) -> Place {
        Place {
            name: name.into(),
            gx: Some(gx),
            gy: Some(gy),
            lng: None,
            lat: None,
            cell_m: 30.0,
        }
    }

    // ── 出行方式 ──────────────────────────────────────────────

    /// 九种方式的速度表与中文名（机主给的口径，写死防回归）。
    #[test]
    fn move_kind_speed_table() {
        let want = [
            (MoveKind::Walk, 1.4),
            (MoveKind::Bike, 4.2),
            (MoveKind::Ebike, 6.0),
            (MoveKind::Bus, 8.0),
            (MoveKind::Subway, 11.0),
            (MoveKind::Taxi, 10.0),
            (MoveKind::Car, 12.0),
            (MoveKind::Train, 70.0),
            (MoveKind::Plane, 220.0),
        ];
        assert_eq!(MoveKind::ALL.len(), 9);
        for (k, v) in want {
            assert_eq!(k.speed_mps(), v, "{} 速度不对", k.key());
        }
        // 城市道路四件套必须落在 8~14 的区间里
        for k in [MoveKind::Bus, MoveKind::Subway, MoveKind::Taxi, MoveKind::Car] {
            assert!((8.0..=14.0).contains(&k.speed_mps()), "{} 不在 8~14", k.key());
        }
        assert_eq!(MoveKind::Walk.zh(), "步行");
        assert_eq!(MoveKind::Plane.zh(), "飞机");
    }

    /// 方式解析：英文键、大小写、中文别名、垃圾输入。
    #[test]
    fn move_kind_parse_aliases() {
        assert_eq!(MoveKind::parse("walk"), Some(MoveKind::Walk));
        assert_eq!(MoveKind::parse(" WALK "), Some(MoveKind::Walk));
        assert_eq!(MoveKind::parse("步行"), Some(MoveKind::Walk));
        assert_eq!(MoveKind::parse("走路"), Some(MoveKind::Walk));
        assert_eq!(MoveKind::parse("单车"), Some(MoveKind::Bike));
        assert_eq!(MoveKind::parse("电瓶车"), Some(MoveKind::Ebike));
        assert_eq!(MoveKind::parse("坐地铁"), Some(MoveKind::Subway));
        assert_eq!(MoveKind::parse("打车"), Some(MoveKind::Taxi));
        assert_eq!(MoveKind::parse("自驾"), Some(MoveKind::Car));
        assert_eq!(MoveKind::parse("高铁"), Some(MoveKind::Train));
        assert_eq!(MoveKind::parse("航班"), Some(MoveKind::Plane));
        assert_eq!(MoveKind::parse(""), None);
        assert_eq!(MoveKind::parse("传送门"), None);
        // 老工具里的 ferry 不在九种之内 → 认不出来，交给自动选（不瞎猜）
        assert_eq!(MoveKind::parse("ferry"), None);
    }

    /// 自动选：2km 是走路的分界，长途自动上车，超远上高铁/飞机。
    #[test]
    fn auto_kind_ladder() {
        assert_eq!(MoveKind::auto_for(0.0), MoveKind::Walk);
        assert_eq!(MoveKind::auto_for(1_999.0), MoveKind::Walk);
        assert_eq!(MoveKind::auto_for(2_000.0), MoveKind::Bike);
        assert_eq!(MoveKind::auto_for(5_999.0), MoveKind::Bike);
        assert_eq!(MoveKind::auto_for(6_000.0), MoveKind::Bus);
        assert_eq!(MoveKind::auto_for(24_999.0), MoveKind::Bus);
        assert_eq!(MoveKind::auto_for(25_000.0), MoveKind::Car);
        assert_eq!(MoveKind::auto_for(119_999.0), MoveKind::Car);
        assert_eq!(MoveKind::auto_for(120_000.0), MoveKind::Train);
        assert_eq!(MoveKind::auto_for(799_999.0), MoveKind::Train);
        assert_eq!(MoveKind::auto_for(800_000.0), MoveKind::Plane);
        assert_eq!(MoveKind::auto_for(f64::NAN), MoveKind::Walk);
    }

    // ── 地点与距离 ────────────────────────────────────────────

    /// 格点距离 = 格差 × 格边长；经纬度走 haversine；两套坐标不混。
    #[test]
    fn place_distance_uses_existing_helpers() {
        let a = grid_place("A", 3.0, 4.0);
        let b = grid_place("B", 5.0, 5.0);
        let d = a.distance_to(&b).unwrap();
        assert!((d - 5f64.sqrt() * 30.0).abs() < 1e-9, "格点距离算错了: {d}");

        // 同一个点 = 0 米（不是 None）
        assert_eq!(a.distance_to(&a), Some(0.0));

        // 经纬度：广州塔 → 深圳（与 coord.rs 单测同一组坐标，haversine 约 108km）
        let gz = Place {
            name: "广州".into(),
            gx: None,
            gy: None,
            lng: Some(113.2644),
            lat: Some(23.1291),
            cell_m: 30.0,
        };
        let sz = Place {
            name: "深圳".into(),
            gx: None,
            gy: None,
            lng: Some(114.0579),
            lat: Some(22.5431),
            cell_m: 30.0,
        };
        let dg = gz.distance_to(&sz).unwrap();
        // 广州塔 → 深圳 约 104km（coord.rs::haversine_guangzhou_shenzhen 的口径）
        assert!(dg > 90_000.0 && dg < 110_000.0, "haversine 距离不对: {dg}");
        // 与 coord.rs 的现成实现**逐位一致** —— 证明这里没另起一把尺子
        assert_eq!(
            dg,
            super::super::coord::haversine_m((113.2644, 23.1291), (114.0579, 22.5431)),
            "geo_distance_m 必须与 coord::haversine_m 同公式"
        );
        assert_eq!(gz.space_with(&sz), Some(Space::Geo));
        assert_eq!(a.space_with(&b), Some(Space::Grid));

        // 格点 ↔ 经纬度：没有锚点，算不了（不猜）
        assert_eq!(a.space_with(&gz), None);
        assert_eq!(a.distance_to(&gz), None);
    }

    /// `from_value` 认各种字段写法。
    #[test]
    fn place_from_value_reads_real_shapes() {
        let p = Place::from_value("A", &json!({"x": 3.0, "y": 4.0}), 30.0);
        assert!(p.has_grid() && !p.has_geo());
        assert_eq!(p.cell_m, 30.0);

        let p2 = Place::from_value("B", &json!({"grid": [5, 5], "cell_meters": 25.0}), 30.0);
        assert_eq!((p2.gx, p2.gy), (Some(5.0), Some(5.0)));
        assert_eq!(p2.cell_m, 25.0);

        let p3 = Place::from_value("C", &json!({"lng": 113.3, "lat": 23.1}), 30.0);
        assert!(p3.has_geo());

        // 什么都没有：名字还在，坐标全空
        let p4 = Place::from_value("D", &json!({}), 30.0);
        assert!(!p4.has_grid() && !p4.has_geo());
    }

    // ── 行程：插值 / 时间戳重算 ───────────────────────────────

    /// 步行 1400 米 = 1000 秒；半途进度 0.5；位置是线性插值。
    #[test]
    fn trip_interpolates_by_timestamp() {
        let from = grid_place("A", 0.0, 0.0);
        let to = grid_place("B", 10.0, 0.0); // 10 格 × 30m = 300m
        let t0 = 1_000_000u64;
        let trip = Trip::plan(1, "小满", from, to, Some(MoveKind::Walk), 1.0, t0, t0).unwrap();
        assert_eq!(trip.distance_m, 300.0);
        assert_eq!(trip.duration_secs(), 300.0 / 1.4);

        // 走了 100 秒 × 1.4 = 140 米 → 进度 140/300
        let p = trip.progress_at(t0 + 100_000);
        assert!((p - 140.0 / 300.0).abs() < 1e-9, "{p}");
        let (x, y) = trip.position_at(t0 + 100_000);
        assert!((x - 10.0 * p).abs() < 1e-9, "{x}");
        assert_eq!(y, 0.0);

        // 到点 → 正好落在目的地
        let end = t0 + 214_286; // 300m ÷ 1.4m/s = 214.286s
        assert_eq!(trip.progress_at(end), 1.0);
        assert_eq!(trip.position_at(end), (10.0, 0.0));
        assert_eq!(trip.status_at(end - 1), TripStatus::Moving);
        assert_eq!(trip.status_at(end), TripStatus::Arrived);
        // 超过也 clamp 在终点，不会跑到天上去
        assert_eq!(trip.position_at(end + 10_000_000), (10.0, 0.0));
        assert_eq!(trip.remaining_m(end + 1), 0.0);
    }

    /// 出发前：位置在起点、进度 0、状态 pending。
    #[test]
    fn pending_before_departure() {
        let t0 = 5_000u64;
        let trip = Trip::plan(
            1,
            "小满",
            grid_place("A", 0.0, 0.0),
            grid_place("B", 10.0, 0.0),
            Some(MoveKind::Walk),
            1.0,
            t0,
            t0 + 60_000, // 一分钟后出发
        )
        .unwrap();
        assert_eq!(trip.status, TripStatus::Pending);
        assert_eq!(trip.status_at(t0 + 59_999), TripStatus::Pending);
        assert_eq!(trip.position_at(t0 + 59_999), (0.0, 0.0));
        assert_eq!(trip.progress_at(t0 + 59_999), 0.0);
        assert_eq!(trip.status_at(t0 + 60_000), TripStatus::Moving);
        // 出发时刻起算：到点时刻 = 出发 + 耗时
        assert_eq!(trip.arrive_at_ms(t0), t0 + 60_000 + 214_286);
    }

    /// 时钟倒退（NTP 校时/用户改时间）：进度不许往回走。
    #[test]
    fn clock_going_backwards_does_not_rewind() {
        let t0 = 1_000_000u64;
        let trip = Trip::plan(1, "小满", grid_place("A", 0.0, 0.0), grid_place("B", 10.0, 0.0), None, 1.0, t0, t0)
            .unwrap();
        let ahead = trip.progress_at(t0 + 100_000);
        let back = trip.progress_at(t0 - 500_000);
        assert_eq!(back, 0.0, "时钟倒退时进度归 0 而不是负数");
        assert!(ahead > 0.0);
    }

    /// 100× 加速：只改推进速率，语义（方式/距离）不变。
    #[test]
    fn speedup_is_time_compression_only() {
        let t0 = 0u64;
        let slow = Trip::plan(1, "小满", grid_place("A", 0.0, 0.0), grid_place("B", 10.0, 0.0), Some(MoveKind::Walk), 1.0, t0, t0).unwrap();
        let fast = Trip::plan(2, "小满", grid_place("A", 0.0, 0.0), grid_place("B", 10.0, 0.0), Some(MoveKind::Walk), 100.0, t0, t0).unwrap();

        assert_eq!(slow.distance_m, fast.distance_m);
        assert_eq!(slow.kind, fast.kind);
        assert_eq!(fast.speed_eff(), 140.0);
        // 1× 要 300 秒；100× 只要 3 秒
        assert_eq!(slow.status_at(3_000), TripStatus::Moving);
        assert_eq!(fast.status_at(3_000), TripStatus::Arrived);
        assert_eq!(fast.arrive_at_ms(t0), 2_143); // 300m ÷ 140m/s
        // 1× 的"现实耗时"文案不受加速影响
        assert_eq!(fast.duration_secs(), slow.duration_secs());
    }

    /// 半路拨加速开关：进度连续（不瞬移），剩下的路按新倍率跑。
    #[test]
    fn toggling_speedup_midway_keeps_progress_continuous() {
        let t0 = 0u64;
        let mut trip = Trip::plan(1, "小满", grid_place("A", 0.0, 0.0), grid_place("B", 10.0, 0.0), Some(MoveKind::Walk), 1.0, t0, t0).unwrap();
        let mid = 100_000u64; // 走了 140 米
        let before = trip.progress_at(mid);
        trip.reanchor(mid, 100.0);
        let after = trip.progress_at(mid);
        assert!((before - after).abs() < 1e-9, "重锚后进度必须连续: {before} vs {after}");
        // 剩下的 160 米按 140m/s 跑 → 1.14 秒后到
        assert_eq!(trip.status_at(mid + 1_143), TripStatus::Arrived);
        assert!(trip.arrive_at_ms(mid) <= mid + 1_200, "{}", trip.arrive_at_ms(mid));
    }

    /// 原地到达（距离 0）：立刻 arrived，位置就是那一点。
    #[test]
    fn zero_distance_arrives_immediately() {
        let t0 = 42u64;
        let mut trip = Trip::plan(1, "小满", grid_place("A", 3.0, 4.0), grid_place("A", 3.0, 4.0), None, 1.0, t0, t0).unwrap();
        assert_eq!(trip.distance_m, 0.0);
        assert_eq!(trip.status_at(t0), TripStatus::Arrived);
        assert!(trip.refresh(t0));
        assert_eq!(trip.status, TripStatus::Arrived);
        assert_eq!(trip.position_at(t0 + 999_999), (3.0, 4.0));
        // 幂等：第二次不再"到达"
        assert!(!trip.refresh(t0 + 1));
    }

    /// `refresh` 幂等：定时器到点 + 前端轮询都调它，事件只许落一条。
    #[test]
    fn refresh_is_idempotent() {
        let t0 = 0u64;
        let mut trip = Trip::plan(1, "小满", grid_place("A", 0.0, 0.0), grid_place("B", 10.0, 0.0), None, 100.0, t0, t0).unwrap();
        assert!(!trip.refresh(t0 + 1_000));
        assert!(trip.refresh(t0 + 3_000));
        assert!(!trip.refresh(t0 + 4_000));
        assert!(!trip.refresh(t0 + 99_000));
        assert_eq!(trip.arrived_ms, Some(t0 + 3_000));
    }

    /// 取消：位置冻结在取消那一刻，不再随时间前进。
    #[test]
    fn cancel_freezes_position() {
        let t0 = 0u64;
        let mut trip = Trip::plan(1, "小满", grid_place("A", 0.0, 0.0), grid_place("B", 10.0, 0.0), None, 1.0, t0, t0).unwrap();
        let at_cancel = trip.position_at(t0 + 100_000);
        assert!(trip.cancel(t0 + 100_000));
        assert_eq!(trip.status, TripStatus::Cancelled);
        assert_eq!(trip.status_at(t0 + 999_000), TripStatus::Cancelled);
        assert_eq!(trip.position_at(t0 + 999_000), at_cancel);
        assert_eq!(trip.progress_at(t0 + 999_000), trip.progress_at(t0 + 100_000));
        // 二次取消/到达后再取消都无效
        assert!(!trip.cancel(t0 + 200_000));
        let mut arrived = Trip::plan(2, "小满", grid_place("A", 0.0, 0.0), grid_place("B", 0.0, 0.0), None, 1.0, t0, t0).unwrap();
        arrived.refresh(t0);
        assert!(!arrived.cancel(t0 + 1));
    }

    /// 规划失败的口径：坐标系对不上就别硬开行程。
    #[test]
    fn plan_rejects_mismatched_spaces() {
        let grid = grid_place("A", 0.0, 0.0);
        let geo = Place {
            name: "B".into(),
            gx: None,
            gy: None,
            lng: Some(113.0),
            lat: Some(23.0),
            cell_m: 30.0,
        };
        assert_eq!(
            Trip::plan(1, "小满", grid.clone(), geo, None, 1.0, 0, 0).unwrap_err(),
            PlanError::NoSharedSpace
        );
        assert_eq!(PlanError::NoSharedSpace.zh(), "目的地和当前位置不在同一套坐标系里");
    }

    /// 倍率归一化。
    #[test]
    fn speedup_normalization() {
        assert_eq!(normalize_speedup(1.0), 1.0);
        assert_eq!(normalize_speedup(100.0), 100.0);
        assert_eq!(normalize_speedup(0.0), 1.0);
        assert_eq!(normalize_speedup(-5.0), 1.0);
        assert_eq!(normalize_speedup(f64::NAN), 1.0);
        assert_eq!(normalize_speedup(f64::INFINITY), 1.0);
        assert_eq!(normalize_speedup(1e9), SPEEDUP_MAX);
        assert_eq!(normalize_speedup(0.5), 1.0, "比 1× 还慢没有意义，夹到 1");
    }

    /// 视图字段（前端契约）该有的都有，数值取整合理。
    #[test]
    fn view_exposes_frontend_contract() {
        let t0 = 1_000u64;
        let trip = Trip::plan(7, "小满", grid_place("便利店", 3.0, 4.0), grid_place("咖啡馆", 5.0, 5.0), Some(MoveKind::Walk), 1.0, t0, t0).unwrap();
        let v = trip.view(t0 + 30_000);
        assert_eq!(v["id"], json!(7));
        assert_eq!(v["role"], json!("小满"));
        assert_eq!(v["status"], json!("moving"));
        assert_eq!(v["kind"], json!("walk"));
        assert_eq!(v["kind_zh"], json!("步行"));
        assert_eq!(v["kind_explicit"], json!(true));
        assert_eq!(v["space"], json!("grid"));
        assert_eq!(v["to"]["name"], json!("咖啡馆"));
        assert_eq!(v["from"]["gx"], json!(3.0));
        assert_eq!(v["distance_m"], json!(67.1));
        assert!(v["pos"]["gx"].as_f64().unwrap() > 3.0);
        assert!(v["pos"]["gy"].as_f64().unwrap() > 4.0);
        assert!(v["pos"].get("lng").is_none(), "格点空间不该冒出 lng");
        assert_eq!(v["speedup"], json!(1.0));
        assert_eq!(v["arrived_ms"], json!(null));
        assert!(v["arrive_at_ms"].as_u64().unwrap() > t0 + 30_000);
        // 进度与剩余量自洽
        let p = v["progress"].as_f64().unwrap();
        assert!((0.0..1.0).contains(&p));
        assert!((v["done_m"].as_f64().unwrap() + v["remaining_m"].as_f64().unwrap() - 67.1).abs() < 0.2);
    }

    // ── 目的地解析 ────────────────────────────────────────────

    /// 设施名：完全一致优先于部分匹配；同名多家里取最近的。
    #[test]
    fn destination_name_matching_prefers_exact_then_nearest() {
        let snap = snapshot();
        let origin = resolve_origin(&snap, "小满").unwrap();
        assert_eq!(origin.name, "小满");
        assert_eq!((origin.gx, origin.gy), (Some(3.0), Some(4.0)));

        // 完全一致
        let p = resolve_destination(&snap, "星洲咖啡馆", Some(&origin)).unwrap();
        assert_eq!((p.gx, p.gy), (Some(5.0), Some(5.0)));
        // 部分匹配（模型爱加"楼下""附近的"）
        let p2 = resolve_destination(&snap, "楼下便利店", Some(&origin)).unwrap();
        assert_eq!((p2.gx, p2.gy), (Some(3.0), Some(4.0)), "该挑最近的那家便利店");
        // "楼下咖啡馆" → 星洲咖啡馆：修饰前缀剥掉后靠**名字互相包含**命中
        let p2b = resolve_destination(&snap, "楼下咖啡馆", Some(&origin)).unwrap();
        assert_eq!(p2b.name, "星洲咖啡馆");
        assert_eq!(resolve_destination(&snap, "去咖啡馆", Some(&origin)).unwrap().name, "星洲咖啡馆");
        assert_eq!(resolve_destination(&snap, "附近的咖啡馆", Some(&origin)).unwrap().name, "星洲咖啡馆");
        // 护栏：剥到只剩一个字就不许再当子串乱命中
        assert_eq!(core_name("楼下咖啡馆"), "咖啡馆");
        assert_eq!(core_name("便利店"), "便利店");
        assert_eq!(core_name("附近的"), "附近的");
        // 名字里带类型词
        let p3 = resolve_destination(&snap, "地铁站", Some(&origin)).unwrap();
        assert_eq!(p3.name, "东山口地铁站");
        // 类型名兜底
        let p4 = resolve_destination(&snap, "医疗", Some(&origin)).unwrap();
        assert_eq!(p4.name, "市第一医院");
        // 别名：回家 → 最近的住宅
        let p5 = resolve_destination(&snap, "回家", Some(&origin)).unwrap();
        assert_eq!(p5.name, "3号楼");
        // 大小写不敏感（英文名也认）
        assert!(resolve_destination(&snap, "便利店(南门)", Some(&origin)).is_some());
        // 找不到就是找不到
        assert!(resolve_destination(&snap, "不存在的地方", Some(&origin)).is_none());
        assert!(resolve_destination(&snap, "   ", Some(&origin)).is_none());
    }

    /// 找别的角色 / 找玩家；角色没落位时退到玩家位置。
    #[test]
    fn destination_actors_and_me_and_origin_fallback() {
        let mut snap = snapshot();
        snap["actors"]["阿离"] = json!({"facility": "公园", "x": 8.0, "y": 9.0});
        let me = resolve_origin(&snap, "小满").unwrap();
        let p = resolve_destination(&snap, "阿离", Some(&me)).unwrap();
        assert_eq!((p.gx, p.gy), (Some(8.0), Some(9.0)));
        let u = resolve_destination(&snap, "用户", Some(&me)).unwrap();
        assert_eq!((u.gx, u.gy), (Some(13.0), Some(4.0)));

        // 角色没在 actors 里 → 用 me 兜底
        let other = resolve_origin(&snap, "路人甲").unwrap();
        assert_eq!(other.name, "用户位置");
        assert_eq!((other.gx, other.gy), (Some(13.0), Some(4.0)));

        // 连 me 都没有 → None（不瞎给坐标）
        let empty = json!({});
        assert!(resolve_origin(&empty, "小满").is_none());
    }

    /// 派发：显式 kind > 自动选；认不出的 kind 退回自动选但标记来源。
    #[test]
    fn dispatch_respects_explicit_kind() {
        let snap = snapshot();
        let t0 = 1_000u64;
        // 60 米（便利店里走到咖啡馆）→ 自动选走路
        let auto = dispatch(&snap, "小满", "咖啡馆", None, t0, 1.0).unwrap();
        assert_eq!(auto.kind, MoveKind::Walk);
        assert!(!auto.kind_explicit);

        // 显式打车：距离很近也照办（以指定为准）
        let taxi = dispatch(&snap, "小满", "咖啡馆", Some("taxi"), t0, 1.0).unwrap();
        assert_eq!(taxi.kind, MoveKind::Taxi);
        assert!(taxi.kind_explicit);

        // 认不出的方式 → 自动选，且**不假装**是显式指定的
        let weird = dispatch(&snap, "小满", "咖啡馆", Some("传送门"), t0, 1.0).unwrap();
        assert_eq!(weird.kind, MoveKind::Walk);
        assert!(!weird.kind_explicit);

        // 长途（医院 1.3km 还是走路；把距离拉大一点看点：r001 3号楼在 (1,1)）
        let long = dispatch(&snap, "小满", "用户", None, t0, 1.0).unwrap();
        assert_eq!(long.kind, MoveKind::Walk, "300 米当然走路");
        // 直接构造一个远距离：从 (3,4) 到 (30,40) 是 √1333 格 ≈ 1095m → 走路
        let far = dispatch(&snap, "小满", "便利店(南门)", None, t0, 1.0).unwrap();
        assert_eq!(far.kind, MoveKind::Walk);
        assert!(far.distance_m > 1000.0, "{}", far.distance_m);
    }

    /// 派发的错误口径。
    #[test]
    fn dispatch_error_cases() {
        let snap = snapshot();
        assert_eq!(dispatch(&snap, "  ", "咖啡馆", None, 0, 1.0).unwrap_err(), PlanError::NoRole);
        assert_eq!(dispatch(&snap, "小满", "月亮", None, 0, 1.0).unwrap_err(), PlanError::NoDestination);
        let empty = json!({});
        assert_eq!(dispatch(&empty, "小满", "咖啡馆", None, 0, 1.0).unwrap_err(), PlanError::NoOrigin);
        assert_eq!(PlanError::NoOrigin.zh(), "这个角色在地图上还没有位置");
    }

    // ── 注册表 ────────────────────────────────────────────────

    /// 事件文案（注入里「最近：…」的来源）与 `summary::last_event` 的口径合得上。
    #[test]
    fn event_texts_read_naturally() {
        assert_eq!(
            move_text("小满", "咖啡馆", MoveKind::Walk, 67.08),
            "小满动身去咖啡馆（步行，约 70m）"
        );
        assert_eq!(
            summary::last_event(&[json!({"text": move_text("小满", "咖啡馆", MoveKind::Walk, 67.08)})]).unwrap(),
            "刚才小满动身去咖啡馆（步行，约 70m）"
        );
        assert_eq!(arrive_text("小满", "咖啡馆"), "小满到了咖啡馆");
        assert_eq!(cancel_text("小满", "咖啡馆"), "小满不去咖啡馆了");
        // 长途的距离文案走 km 那一档
        assert_eq!(move_text("小满", "北京", MoveKind::Train, 2_100_000.0), "小满动身去北京（高铁，约 2100km）");
    }

    /// `depart_in_secs` 的钳制。
    #[test]
    fn depart_secs_is_clamped() {
        assert_eq!(normalize_depart_secs(0.0), 0.0);
        assert_eq!(normalize_depart_secs(-3.0), 0.0);
        assert_eq!(normalize_depart_secs(f64::NAN), 0.0);
        assert_eq!(normalize_depart_secs(f64::INFINITY), 0.0);
        assert_eq!(normalize_depart_secs(60.0), 60.0);
        assert_eq!(normalize_depart_secs(1e9), MAX_DEPART_SECS);
    }

    /// `get`/`view_of` 是**只读**的：前端轮询读多少次都不能把"到达"吃掉
    /// （否则那条到达事件就永远写不进 `MapRuntime`）。
    #[test]
    fn reads_do_not_swallow_the_arrival() {
        let _serial = registry_guard();
        reset();
        let t0 = 1_000u64;
        let trip = dispatch(&snapshot(), "小满", "咖啡馆", Some("walk"), t0, 1.0).unwrap();
        let eta = trip.arrive_at_ms(t0);
        start(trip);
        // 前端轮询好几轮
        assert_eq!(view_of("小满", eta + 5).unwrap()["status"], json!("arrived"));
        assert_eq!(views(eta + 5).len(), 1);
        assert!(get("小满", eta + 5).is_some());
        // 收口：事件必须还在
        assert!(refresh("小满", eta + 5).is_some(), "读过之后到达事件不能丢");
        assert!(refresh("小满", eta + 6).is_none());
        assert_eq!(view_of("小满", eta + 6).unwrap()["arrived_ms"], json!(eta + 5));
        reset();
    }

    /// 注册表：登记 / 读 / 顶替 / 取消 / 视图；`refresh` 幂等贯穿注册表。
    #[test]
    fn registry_lifecycle() {
        let _serial = registry_guard();
        reset();
        assert_eq!(speedup(), SPEEDUP_OFF);

        let t0 = 1_000u64;
        let trip = dispatch(&snapshot(), "小满", "咖啡馆", Some("walk"), t0, speedup()).unwrap();
        let id = trip.id;
        assert!(start(trip).is_none());
        assert_eq!(views(t0).len(), 1);
        let v = view_of("小满", t0).unwrap();
        assert_eq!(v["id"], json!(id));
        assert_eq!(v["status"], json!("moving"));

        // 到点：第一次 true，第二次 false（事件只落一条）
        let eta = v["arrive_at_ms"].as_u64().unwrap();
        assert!(refresh("小满", eta).is_some());
        assert!(refresh("小满", eta + 1).is_none());
        assert!(refresh("小满", eta + 99_999).is_none());
        assert_eq!(view_of("小满", eta + 1).unwrap()["status"], json!("arrived"));

        // 顶替：新行程挤掉旧的（返回旧的那条给调用方记事件）
        let again = dispatch(&snapshot(), "小满", "市第一医院", Some("taxi"), t0, speedup()).unwrap();
        let old = start(again).expect("旧行程要被交出来");
        assert_eq!(old.id, id);
        assert_eq!(view_of("小满", t0).unwrap()["kind"], json!("taxi"));

        // 取消
        let cancelled = cancel("小满", t0 + 1_000).unwrap();
        assert_eq!(cancelled.status, TripStatus::Cancelled);
        assert!(cancel("小满", t0 + 2_000).is_none(), "取消是幂等的");
        assert!(refresh("小满", t0 + 999_999).is_none(), "取消后不会再'到达'");
        assert!(view_of("小满", t0).unwrap()["cancelled_ms"].is_u64());

        // 不存在的角色
        assert!(view_of("没人", t0).is_none());
        assert!(cancel("没人", t0).is_none());
        reset();
    }

    /// 加速开关：设置后**进行中的行程**重锚，新行程直接吃到新倍率。
    #[test]
    fn registry_speedup_applies_to_running_trip() {
        let _serial = registry_guard();
        reset();
        let t0 = 0u64;
        let trip = dispatch(&snapshot(), "小满", "便利店(南门)", Some("walk"), t0, speedup()).unwrap();
        let total = trip.distance_m;
        start(trip);

        let mid = 10_000u64;
        let p_before = view_of("小满", mid).unwrap()["progress"].as_f64().unwrap();
        assert_eq!(set_speedup(100.0, mid), 100.0);
        assert_eq!(speedup(), 100.0);
        let p_after = view_of("小满", mid).unwrap()["progress"].as_f64().unwrap();
        assert!((p_before - p_after).abs() < 1e-6, "拨开关不该瞬移: {p_before} vs {p_after}");

        // 剩下的路按 140m/s 跑
        let v = view_of("小满", mid).unwrap();
        assert_eq!(v["speedup"], json!(100.0));
        assert!((v["speed_eff_mps"].as_f64().unwrap() - 140.0).abs() < 1e-6);
        let arrive = v["arrive_at_ms"].as_u64().unwrap();
        assert!(arrive < mid + (total / 140.0 * 1000.0) as u64 + 5, "{arrive}");

        // 上限夹紧 + 非有限值归 1
        assert_eq!(set_speedup(9_999.0, mid), SPEEDUP_MAX);
        assert_eq!(set_speedup(f64::NAN, mid), SPEEDUP_OFF);

        // 新行程吃当前倍率
        set_speedup(SPEEDUP_FAST, mid);
        assert_eq!(moving_etas(mid).len(), 1);
        reset();
        assert!(views(mid).is_empty());
    }
}
