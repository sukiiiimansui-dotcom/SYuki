// 「世界模拟」P4-4：玩家干预（指挥他 / 把他拖到别处）——纯逻辑 + 一个开关
//
// ── 三条口径（改之前先读）──────────────────────────────────────────────────
//  ① **默认关闭**（`wsm:v1:intervene` 缺省 = false）：角色是有自主性的，
//     「能不能被玩家拖着走」必须由玩家显式打开，不能默认就能拖。
//  ② 拖拽落点要**反查**：屏幕坐标 → 地图内容坐标 → 格点 → 最近的设施。
//     这三步全是纯数学（本文件），组件只负责「什么时候调」。
//  ③ 不许静默瞬移：落点算出距离/预计时间 → 先如实提示，再起行程
//     （行程本身就是「走过去」，地图上会画车，不存在瞬移）。
//
// ── 与「对话指挥」的关系（为什么不在这里做指令通道）────────────────────────
//   玩家在聊天里说「你去便利店」→ 模型吐 `⟦wm:{"to":"便利店"}⟧` → Rust 侧
//   `directive.rs` 剥离 + `move::dispatch` → `Trip::plan` 起一条行程。
//   地图页这个入口走的是**同一条下游**：`world_map_trip_start` → 同样 `Trip::plan`。
//   前端不新增任何后台能力，也不自己发明指令语法（见 WorldSim 里的注释）。
//
// 依赖：`./wsActors`（信箱数学与格点换算）与 `vue`，**不 import 任何 `@/` 别名**，
//       这样自检里 `bundle()` 不需要额外别名（同 wsPerf.ts 的纪律）。

import { ref, type Ref } from 'vue'
import { letterboxOf, screenToContent, type Letterbox } from './wsActors'

/* ══════════════════════════════════════════════════════════════════
 * 一、开关（默认关 + 落 localStorage）
 * ══════════════════════════════════════════════════════════════════ */

export const WS_INTERVENE_KEY = 'wsm:v1:intervene'

/** 默认关：尊重角色自主性（需求原文） */
export const INTERVENE_DEFAULT = false

/** 读开关：只认显式 `'1'` / `'0'`，坏值一律当默认（关） */
export function parseIntervene(raw: string | null | undefined): boolean {
  const s = String(raw ?? '').trim()
  if (s === '1' || s === 'true' || s === 'on') return true
  if (s === '0' || s === 'false' || s === 'off') return false
  return INTERVENE_DEFAULT
}

/** 写开关：存 `'1'` / `'0'`（比 JSON 少一层解析，人肉看存储也一目了然） */
export function serializeIntervene(on: boolean): string {
  return on ? '1' : '0'
}

function readLS(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}
function writeLS(key: string, val: string) {
  try {
    localStorage.setItem(key, val)
  } catch {
    /* 写不进去就只在本次会话生效 */
  }
}

const on = ref<boolean>(parseIntervene(readLS(WS_INTERVENE_KEY)))

/** 干预开关（模块级单例：角色面板与地图页读到的是同一份） */
export function useWsIntervene(): {
  on: Ref<boolean>
  setOn: (v: boolean) => void
  toggle: () => void
} {
  function setOn(v: boolean) {
    on.value = !!v
    writeLS(WS_INTERVENE_KEY, serializeIntervene(on.value))
  }
  return { on, setOn, toggle: () => setOn(!on.value) }
}

/* ══════════════════════════════════════════════════════════════════
 * 二、屏幕 → 格点（拖拽落点反查）
 * ══════════════════════════════════════════════════════════════════ */

/** 拖拽落点的上下文：舞台矩形 + 当前手势变换 + 头像层的信箱盒子 */
export interface DropCtx {
  /** 手势**舞台**（`.ws-dist__stage`）的 getBoundingClientRect() */
  rect: { left: number; top: number }
  /** 手势当前倍率 */
  scale: number
  /** 手势当前平移 */
  tx: number
  ty: number
  /** 头像层盒子的 CSS 尺寸（用 offsetWidth/offsetHeight 量，**不是** getBoundingClientRect） */
  boxW: number
  boxH: number
  /** 网格边长（WS_GRID = 28） */
  grid: number
}

/**
 * 屏幕坐标 → 格点坐标。
 *
 * 顺序：client → 内容坐标（逆变换）→ 信箱盒子像素 → 格点。
 * 与 `WsAvatarMark` 的正向换算（格子 → 信箱 → left/top + translate(-50%,-50%)）严格互逆，
 * 否则「拖到哪、人到哪」会对不上（这类偏移只在非正方形盒子上出现，最难查）。
 */
export function dropGridAt(client: { x: number; y: number }, ctx: DropCtx): { gx: number; gy: number } {
  const grid = Math.max(1, Number(ctx.grid) || 28)
  const content = screenToContent(client, ctx.rect, { scale: ctx.scale, tx: ctx.tx, ty: ctx.ty })
  const lb: Letterbox = letterboxOf(ctx.boxW, ctx.boxH, grid)
  const s = lb.scale > 0 ? lb.scale : 1
  return {
    gx: clampGrid((content.x - lb.padX) / s, grid),
    gy: clampGrid((content.y - lb.padY) / s, grid),
  }
}

/** 夹回图内（0.8 ~ grid-0.8，与 spreadCrowd 的边界一致 —— 人不能站到图外） */
export function clampGrid(v: number, grid = 28): number {
  const hi = Math.max(0.8, (Number(grid) || 28) - 0.8)
  const n = Number.isFinite(v) ? v : (Number(grid) || 28) / 2
  return Math.min(hi, Math.max(0.8, n))
}

/**
 * 拖动中的落点预览：一次算出**格点**与**信箱盒子像素**。
 *
 * 为什么要两个：格点是要发给后端的（`gx/gy`），盒子像素是给图钉定位用的
 * （图钉和头像层在同一个信箱盒子里）。两处都用同一套数学，绝不在模板里再算一遍。
 */
export function gridPinAt(
  client: { x: number; y: number },
  ctx: DropCtx,
): { gx: number; gy: number; x: number; y: number } {
  const g = dropGridAt(client, ctx)
  const lb = letterboxOf(ctx.boxW, ctx.boxH, Math.max(1, Number(ctx.grid) || 28))
  return { gx: g.gx, gy: g.gy, x: lb.padX + g.gx * lb.scale, y: lb.padY + g.gy * lb.scale }
}

/* ══════════════════════════════════════════════════════════════════
 * 三、设施反查（落点附近有没有「便利店」这种点）
 * ══════════════════════════════════════════════════════════════════ */

export interface WsFacility {
  name: string
  type: string
  gx: number
  gy: number
}

/** 命中半径（格子）：0.5 格 ≈ 15 米，比头像的错开半径还大一点，手感上「丢上去就算」 */
export const FACILITY_HIT_CELLS = 1.6

/**
 * 读设施表。形状按 `wsRuntimePush` 的契约：
 * `{"name":"咖啡馆","type":"commercial","grid":[6,7]}` 或带平铺的 `x`/`y`。
 * 脏数据一律丢掉（没有名字或没有坐标的不算）。
 */
export function facilityList(facilities: unknown): WsFacility[] {
  if (!Array.isArray(facilities)) return []
  const out: WsFacility[] = []
  for (const raw of facilities) {
    if (!raw || typeof raw !== 'object') continue
    const o = raw as Record<string, unknown>
    const name = String(o.name ?? o.label ?? '').trim()
    if (!name) continue
    const grid = Array.isArray(o.grid) ? o.grid : []
    const gx = Number(grid.length >= 2 ? grid[0] : (o.x ?? o.gx))
    const gy = Number(grid.length >= 2 ? grid[1] : (o.y ?? o.gy))
    if (!Number.isFinite(gx) || !Number.isFinite(gy)) continue
    out.push({ name, type: String(o.type ?? '').trim(), gx, gy })
  }
  return out
}

/** 落点附近最近的设施（没有就返回 null —— 调用方用「移动到这里」占位） */
export function facilityAt(facilities: unknown, gx: number, gy: number, radius = FACILITY_HIT_CELLS): WsFacility | null {
  const list = facilityList(facilities)
  let best: WsFacility | null = null
  let bestD = Infinity
  for (const f of list) {
    const d = Math.hypot(f.gx - gx, f.gy - gy)
    if (d <= radius && d < bestD) {
      bestD = d
      best = f
    }
  }
  return best
}

/** 候选目的地清单（面板里的输入建议用；去重 + 保序） */
export function facilityNames(facilities: unknown): string[] {
  const out: string[] = []
  for (const f of facilityList(facilities)) {
    if (!out.includes(f.name)) out.push(f.name)
  }
  return out
}

/* ══════════════════════════════════════════════════════════════════
 * 四、距离 / 预计时间（拖到远处必须先说清楚，别静默瞬移）
 * ══════════════════════════════════════════════════════════════════ */

/** 格边长兜底（后端 `trip::DEFAULT_CELL_M` 也是 30） */
export const DEFAULT_CELL_M = 30
/** 步行速度（m/s）：1.35 ≈ 4.9 km/h，正常成年人的散步速度 */
export const WALK_MPS = 1.35

export interface TripEstimate {
  /** 直线距离（格） */
  cells: number
  /** 直线距离（米，按 cell_m 折算） */
  meters: number
  /** 步行预计秒数（至少 5 秒，避免「0 秒」这种不可信的提示） */
  secs: number
}

/** 从 A 格点到 B 格点的距离与步行耗时（纯函数，提示条的文案由调用方 i18n） */
export function estimateTrip(
  from: { gx: number; gy: number },
  to: { gx: number; gy: number },
  cellM = DEFAULT_CELL_M,
  mps = WALK_MPS,
): TripEstimate {
  const cell = Number(cellM) > 0 ? Number(cellM) : DEFAULT_CELL_M
  const speed = Number(mps) > 0 ? Number(mps) : WALK_MPS
  const cells = Math.hypot((Number(to.gx) || 0) - (Number(from.gx) || 0), (Number(to.gy) || 0) - (Number(from.gy) || 0))
  const meters = Math.round(cells * cell)
  const secs = Math.max(5, Math.round((cells * cell) / speed))
  return { cells, meters, secs }
}

/** 米 → 「210 米」/「1.2 公里」的两个数（文案在 i18n 里拼，这里只给数） */
export function metersParts(meters: number): { value: number; km: boolean } {
  const m = Math.max(0, Math.round(Number(meters) || 0))
  return m >= 1000 ? { value: Math.round(m / 100) / 10, km: true } : { value: m, km: false }
}

/** 秒 → 分钟（向上取整，最小 1 分钟：说「0 分钟」等于没说） */
export function minutesOf(secs: number): number {
  const s = Math.max(0, Number(secs) || 0)
  return Math.max(1, Math.round(s / 60))
}
