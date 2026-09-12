// 「世界模拟」P4-2 / P4-3：行程的**数据层** —— 轮询 + 按时间戳插值 + 事件订阅 + 位置回推
//
// 分工（与 P1/P2 的三件套保持一致）：
//   · 纯逻辑（按时间戳算进度/位置/ETA）→ 本文件顶部的**导出纯函数**（`tripProgress` 等）
//   · 本文件：轮询后端状态机、订阅 `world_map:trip` 事件、把插值位置推回 `actors`
//   · 组件只负责画（WsTripCard / WsVehicleMark）
//
// ══ 三条必须守住的纪律 ══════════════════════════════════════════════════════
//
// ① **必须轮询，不能等事件**：后端是「一次性定时器 + 读接口顺手收口」的模型
//    （`move_cmd.rs` 文件头写得很清楚），事件只是在**它醒着的时候**顺手推的；
//    手机息屏把 WebView 冻住、订阅晚了一步、事件丢了……都会让纯事件方案卡住。
//    所以：`world_map_trip_status` 每 1.5s 一次（页面隐藏时降到 10s）是主通路，
//    事件只用来「更快地反应」。
//
// ② **位置的真相是时间戳，不是定时器累加**：
//      progress = clamp((done_m + (now - anchor) / 1000 * speed_eff_mps) / distance_m, 0, 1)
//      pos      = lerp(from, to, progress)
//    `anchor` 是**上次快照的那一刻**（后端返回的 `now_ms`，这里存成 `trip.at_ms`）。
//    为什么不能靠定时器累加：手机切后台/息屏会把 rAF 与 setInterval 一起冻掉，
//    回到前台时累加值停在半路，人就会「原地站着」直到下一次快照 —— 而按时间戳算，
//    回到前台的那一帧就是**正确位置**（"回到前台立即对齐"）。
//
// ③ **位置回推要克制**（机主与 P2 的约定，改之前先读）：
//    后端**不写** `actors` —— 移动中注入给 AI 的「角色在哪」还是旧位置，直到前端推回去。
//    但 P2（`useWsActors`）也在推**静态**位置，两边各自定时推就会互相覆盖
//    （表现为「角色走两步被拉回去、在地图上抖」）。所以本文件：
//      · **只在有 active trip 时推**，且只在三个时刻推：**起程 / 到达前一帧 / 到达**
//        （起程与到达各一次，到达前一帧是「最后一个还在路上的坐标」）；
//      · 同一时刻的重复坐标会被跳过（`lastPush` 去重，省一次 IPC）；
//      · 暴露 `isOwned(role)` —— P2 在推某个角色的静态位置前先问一句
//        「这个人现在是不是归行程管？」，是就别推（见文件末尾的用法说明）。
//
// 不新增依赖：只用 vue / @tauri-apps/api。web 预览（`__LINGCHAT_WEB_MOCK__`）下
// 这些命令一律不存在，本 composable 直接进入 `supported = false` 的**静默停摆**状态
// （不轮询、不报错、不弹 toast）—— 与 `useWsActors` 对 `isTauriRuntime()` 的纪律一致。

import { computed, getCurrentScope, onScopeDispose, ref, type Ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { isTauriRuntime } from '@/api/services/worldMap'

/* ══════════════════════════════════════════════════════════════════════════
 * 一、类型：与后端 `Trip::view()` 逐字对齐（字段名就是接口契约）
 * ══════════════════════════════════════════════════════════════════════════ */

export type WsTripStatus = 'pending' | 'moving' | 'arrived' | 'cancelled'
export type WsTripSpace = 'grid' | 'geo'
/** 事件载荷的 phase（后端 `move_cmd.rs` 的 EVENT_TRIP 只会推这三种） */
export type WsTripPhase = 'start' | 'arrived' | 'cancelled'

/** 出发点/目的地：grid 用 gx/gy，geo 用 lng/lat；两者都带 name */
export interface WsTripPlace {
  name?: string
  gx?: number
  gy?: number
  lng?: number
  lat?: number
}

/** 当前位置：形状与 `space` 绑定（grid → gx/gy；geo → lng/lat） */
export interface WsTripPos {
  gx?: number
  gy?: number
  lng?: number
  lat?: number
}

/** 后端 `world_map_trip_status` 返回的行程视图 */
export interface WsTrip {
  id: number
  role: string
  status: WsTripStatus
  /** walk / bike / ebike / bus / subway / taxi / car / train / plane */
  kind: string
  /** 后端给的中文名（步行/骑车/…）—— UI 优先用它 */
  kind_zh?: string
  /** 出行方式是 AI/调用方显式指定的，还是按距离自动选的 */
  kind_explicit?: boolean
  space: WsTripSpace
  from: WsTripPlace
  to: WsTripPlace
  pos: WsTripPos
  distance_m: number
  done_m: number
  remaining_m: number
  progress: number
  speed_mps: number
  /** 实际速度 = speed_mps × speedup（加速开关拨上去后，ETA 看这个） */
  speed_eff_mps: number
  speedup: number
  duration_secs: number
  eta_secs: number
  eta_secs_eff: number
  created_ms: number
  depart_ms: number
  arrive_at_ms: number
  eta_1x_ms: number
  arrived_ms: number | null
  cancelled_ms: number | null
  /**
   * ⚠️ **前端加的**字段（后端没有）：拿到这份快照时的后端时间（响应里的 `now_ms`）。
   * 插值的锚点就是它 —— 没有它就只能用后端算好的 `progress`（外推会算错，见 tripProgress）。
   */
  at_ms?: number
}

export interface WsTripStatusResp {
  ok?: boolean
  now_ms?: number
  speedup?: number
  /** 这次查询针对的角色（没传 role 时后端用 `current_role`） */
  role?: string | null
  active?: WsTrip | null
  trips?: WsTrip[]
  /** 本次查询**刚刚收口**的到达（后端 `publish_arrivals` 的产物） */
  arrived_now?: WsTrip[]
}

/** `world_map:trip` 事件载荷 */
export interface WsTripEventPayload {
  phase: WsTripPhase
  trip: WsTrip
  at: number
}

/** `world_map_trip_start` 的请求体（参数名与后端注释逐字一致） */
export interface WsTripStartReq {
  role?: string
  to: string
  kind?: string
  gx?: number
  gy?: number
  lng?: number
  lat?: number
  cell_m?: number
  from?: { gx?: number; gy?: number; lng?: number; lat?: number }
  depart_in_secs?: number
  speedup?: number
}

/* ══════════════════════════════════════════════════════════════════════════
 * 二、纯函数：按时间戳算一切（可单独自检，不依赖 Vue / Tauri）
 * ══════════════════════════════════════════════════════════════════════════ */

function num(v: unknown, d = 0): number {
  const n = Number(v)
  return Number.isFinite(n) ? n : d
}
function clamp01(v: number): number {
  return v < 0 ? 0 : v > 1 ? 1 : v
}
/** 线性插值（进度 → 坐标） */
function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t
}

/** 行程是不是「还在路上」（pending 也算：人还没动，但行程是活的） */
export function isTripLive(t: WsTrip | null | undefined): boolean {
  return !!t && (t.status === 'pending' || t.status === 'moving')
}

/**
 * 插值锚点：**从这一刻起**才按速度往前推；**没有快照时间就返回 0**（= 不外推）。
 *
 * 取 `max(at_ms, depart_ms)`：
 *   · `at_ms` —— 快照时刻（正常情况就是它，`done_m` 是那一刻的已走路程）；
 *   · `depart_ms` —— 出发时刻。快照是在「还没出发」时拍的情况下（`at_ms < depart_ms`，
 *     比如轮询赶在出发前），等待的那段不能算成走路，否则 `done_m + 等待时长×速度`
 *     会凭空多走一截。
 *
 * ⚠️ 只有 `depart_ms`、没有 `at_ms` 时**故意返回 0**（不自作主张拿出发时刻当锚点）：
 * 那份 `done_m` 可能是「半路拍的」，再按「从出发算起」推一遍就是**双倍计数**，
 * 人会提前飞到头。宁可画面不动（退回后端算好的 progress），也不要算错。
 */
export function tripAnchorMs(t: WsTrip): number {
  const at = num(t.at_ms)
  if (!(at > 0)) return 0
  return Math.max(at, num(t.depart_ms))
}

/**
 * 进度（0~1）—— **界面上的位置只认这个函数**。
 *
 * `progress = clamp((done_m + (now - anchor)/1000 × speed_eff_mps) / distance_m, 0, 1)`
 *
 * 几个边角：
 *   · `arrived` → 恒为 1；`cancelled` → 冻结在后端最后给的 `progress`（人停在半路）；
 *   · 没有 `at_ms`（直接用后端原始对象、没经过本 composable 标注）→ **不外推**，
 *     返回后端算好的 `progress`。宁可画面不动，也不要拿错误锚点瞎推。
 */
export function tripProgress(t: WsTrip, nowMs: number): number {
  const status = String(t.status || '')
  if (status === 'arrived') return 1
  const dist = num(t.distance_m)
  const anchor = tripAnchorMs(t)
  if (status === 'cancelled' || !(dist > 0) || !(anchor > 0)) {
    // 没有锚点 / 没有距离（0 米行程）：只能用后端算好的那份，不外推
    return clamp01(num(t.progress))
  }
  const v = Math.max(0, num(t.speed_eff_mps, num(t.speed_mps)))
  const done = num(t.done_m) + (Math.max(0, nowMs - anchor) / 1000) * v
  return clamp01(done / dist)
}

/** 当前位置（形状跟着 `space` 走：grid → {gx,gy}；geo → {lng,lat}） */
export function tripPosition(t: WsTrip, nowMs: number): WsTripPos {
  const p = tripProgress(t, nowMs)
  const from = t.from || {}
  const to = t.to || {}
  if (t.space === 'geo') {
    const a = num(from.lng, NaN)
    const b = num(to.lng, NaN)
    const c = num(from.lat, NaN)
    const d = num(to.lat, NaN)
    if (!Number.isFinite(a) || !Number.isFinite(b) || !Number.isFinite(c) || !Number.isFinite(d)) {
      // 起终点缺经纬度 → 退回后端快照里的 pos（有就用），绝不编一个 0,0
      return { lng: num(t.pos?.lng, NaN), lat: num(t.pos?.lat, NaN) }
    }
    return { lng: lerp(a, b, p), lat: lerp(c, d, p) }
  }
  const a = num(from.gx, NaN)
  const b = num(to.gx, NaN)
  const c = num(from.gy, NaN)
  const d = num(to.gy, NaN)
  if (!Number.isFinite(a) || !Number.isFinite(b) || !Number.isFinite(c) || !Number.isFinite(d)) {
    return { gx: num(t.pos?.gx, NaN), gy: num(t.pos?.gy, NaN) }
  }
  return { gx: lerp(a, b, p), gy: lerp(c, d, p) }
}

/** 剩余距离（米）：距离 × (1 − 进度)，与进度同源，不会出现「进度 100% 还剩 3 米」 */
export function tripRemainingM(t: WsTrip, nowMs: number): number {
  const dist = num(t.distance_m)
  if (dist > 0) return Math.max(0, dist * (1 - tripProgress(t, nowMs)))
  return Math.max(0, num(t.remaining_m))
}

/** 已走距离（米） */
export function tripDoneM(t: WsTrip, nowMs: number): number {
  const dist = num(t.distance_m)
  if (dist > 0) return dist * tripProgress(t, nowMs)
  return Math.max(0, num(t.done_m))
}

/**
 * 预计剩余时间（秒）。
 *
 * `effective = true` 用 `speed_eff_mps`（= 现实速度 × 加速倍率）—— 开了 100× 之后
 * 「还要 47 秒」这种话是错的，应该说 0.5 秒。默认就按有效速度算。
 */
export function tripEtaSecs(t: WsTrip, nowMs: number, effective = true): number {
  const remain = tripRemainingM(t, nowMs)
  const v = effective
    ? Math.max(num(t.speed_eff_mps, num(t.speed_mps)), 0)
    : Math.max(num(t.speed_mps), 0)
  if (!(v > 0)) return remain > 0 ? num(effective ? t.eta_secs_eff : t.eta_secs) : 0
  return remain / v
}

/** 预计到达的**墙钟时刻**（毫秒 unix）：现在 + 剩余时间（比后端 `arrive_at_ms` 更耐时钟漂移） */
export function tripArriveAtMs(t: WsTrip, nowMs: number, effective = true): number {
  if (String(t.status) === 'arrived') return num(t.arrived_ms, nowMs)
  return nowMs + tripEtaSecs(t, nowMs, effective) * 1000
}

/**
 * 该不该水平镜像（车头朝左）。
 *
 * 只看**运动方向**：`to.x < from.x` = 往左走 → 镜像。往右/原地/上下都不镜像
 * （这些立绘都是「朝右」画的，上下移动时镜像反而像倒着开）。
 */
export function tripFacingLeft(t: WsTrip): boolean {
  const f = t.from || {}
  const to = t.to || {}
  if (t.space === 'geo') {
    const a = num(f.lng, NaN)
    const b = num(to.lng, NaN)
    return Number.isFinite(a) && Number.isFinite(b) && b < a
  }
  const a = num(f.gx, NaN)
  const b = num(to.gx, NaN)
  return Number.isFinite(a) && Number.isFinite(b) && b < a
}

/* ══════════════════════════════════════════════════════════════════════════
 * 三、位置回推（`world_map_update_runtime`）—— 只在三个时刻发生
 * ══════════════════════════════════════════════════════════════════════════ */

/**
 * 推回时刻：
 *   · `start`  —— 起程（让后端与别的界面马上知道人已经离开原地）
 *   · `final`  —— 到达前一帧（`progress ≥ FINAL_PUSH_AT`）的最后一个在途坐标
 *   · `cancel` —— 取消（位置冻结在半路）。**默认不推**（见 UseWorldTripsOptions.pushOnCancel）
 * 到达那一刻会再推一次**终点精确坐标**（`pushExact`），与 `final` 去重。
 */
export type WsTripPushPhase = 'start' | 'final' | 'cancel'

/** 到达前一帧的判定阈值（0.999 ≈ 还剩 0.1% 路程时推最后一个在途坐标） */
export const FINAL_PUSH_AT = 0.999

/**
 * 组装 `actors` 的 patch。
 *
 * 键必须是**角色显示名**（后端 `MapRuntime.actors` 的约定）。
 * 坐标**同时写 `x/y` 与 `gx/gy`**：这不是冗余 —— 后端 `summary::grid_xy` 先读 `x/y`
 * 再退 `gx/gy`，而前端 `wsActors.readRuntimePos` **只读 `x/y`**；
 * 两种拼法都写上，两边读到的才是同一个位置。
 *
 * ⚠️ 只推坐标，**不碰** `facility` / `place` / `since` 等字段（那些归 P2 管）——
 * `state.rs` 的 `apply_patch` 是按角色名逐字段合并，没给的字段不会被抹掉。
 */
export function actorPatchOf(t: WsTrip, pos: WsTripPos): Record<string, unknown> {
  const rec: Record<string, number> = {}
  if (t.space === 'geo') {
    const lng = num(pos.lng, NaN)
    const lat = num(pos.lat, NaN)
    if (Number.isFinite(lng)) rec.lng = lng
    if (Number.isFinite(lat)) rec.lat = lat
  } else {
    const gx = num(pos.gx, NaN)
    const gy = num(pos.gy, NaN)
    if (Number.isFinite(gx)) {
      rec.gx = gx
      rec.x = gx
    }
    if (Number.isFinite(gy)) {
      rec.gy = gy
      rec.y = gy
    }
  }
  return { actors: { [t.role]: rec } }
}

/* ══════════════════════════════════════════════════════════════════════════
 * 四、composable
 * ══════════════════════════════════════════════════════════════════════════ */

export interface UseWorldTripsOptions {
  /** 查/取消哪个角色的行程；不传 = 让后端用 `current_role` */
  role?: string | Ref<string>
  /** 前台轮询间隔（毫秒）。默认 1500 —— 后端是一次性定时器，这个频率只是「收口」 */
  pollMs?: number
  /** 页面隐藏时的轮询间隔（毫秒）。默认 10000 —— 降频但**不暂停**：
   *  到达回调要尽量准（切回来时不会漏掉「早就到了」这件事） */
  hiddenPollMs?: number
  /** `now`（插值时钟）的重算间隔（毫秒）。默认 200 —— 进度条另有 rAF 做逐帧平滑 */
  tickMs?: number
  /** 建好就自动开始轮询（默认 true）；关掉就得自己调 `start()` */
  autoStart?: boolean
  /** 是否把插值位置推回 `actors`（默认 true；三个时刻，见文件头纪律③） */
  pushRuntime?: boolean
  /** 取消时也推一次「冻结位置」（默认 false —— 交还给 P2 的静态推送更省事） */
  pushOnCancel?: boolean
  /** 起程回调（事件或轮询发现新行程时各触发一次，按行程 id 去重） */
  onStart?: (t: WsTrip) => void
  /** 到达回调（**给上层弹 toast 用的口子**；同一行程只回调一次） */
  onArrive?: (t: WsTrip) => void
  /** 取消回调 */
  onCancel?: (t: WsTrip) => void
  /** 出错回调（轮询失败不中断，只上报） */
  onError?: (e: unknown) => void
}

export function useWorldTrips(opts: UseWorldTripsOptions = {}) {
  const pollMs = Math.max(250, num(opts.pollMs, 1500))
  const hiddenPollMs = Math.max(pollMs, num(opts.hiddenPollMs, 10_000))
  const tickMs = Math.max(50, num(opts.tickMs, 200))
  const pushRuntime = opts.pushRuntime !== false
  const pushOnCancel = opts.pushOnCancel === true

  /* ── 响应式状态 ─────────────────────────────────────────────────────── */
  /** 全部在册行程（含刚到达/取消的，按后端给的原样） */
  const trips = ref<WsTrip[]>([])
  /** 当前角色的行程（后端的 `active`；没有就是 null） */
  const active = ref<WsTrip | null>(null)
  /** 后端权威的加速倍率（1 = 常速，100 = 100×） */
  const speedup = ref(1)
  /** 插值时钟（毫秒）—— 组件读它来算进度/位置，**不是**帧计数器 */
  const now = ref(Date.now())
  /** 本次查询针对的角色（后端回的 `role`） */
  const role = ref('')
  /** 正在请求（首次查询用，避免 UI 闪「没有行程」） */
  const loading = ref(false)
  const lastError = ref('')
  /** 页面是否隐藏（隐藏时降频 + 停 rAF） */
  const hidden = ref(false)
  /**
   * 这套命令在当前环境可不可用。
   * web 预览（`__LINGCHAT_WEB_MOCK__`）里 `world_map_trip_*` 一律不存在，
   * 此时**直接停摆**：不轮询、不报错（否则 devtools 里每 1.5s 一条红字）。
   * 初值就地判一次（`isTauriRuntime()` 任何时候都能安全调），`start()` 里再复核一次。
   */
  const supported = ref(isTauriRuntime())
  /** 轮询是否在跑 */
  const running = ref(false)

  let pollTimer: number | null = null
  let rafId: number | null = null
  let lastTick = 0
  let unlisten: UnlistenFn | null = null
  let bound = false

  /** 已回调过 start 的行程 id（事件 + 轮询两条通路都要去重） */
  const seenStart = new Set<number>()
  /** 已回调过 arrive 的行程 id（`arrived_now` 与事件会重复报同一件事） */
  const seenArrive = new Set<number>()
  const seenCancel = new Set<number>()
  /** 每个行程推过哪些时刻（key = `role#id`） */
  const pushMark = new Map<string, { start: boolean; final: boolean; cancel: boolean }>()
  /** 上次推的坐标（去重：同一坐标不重复 IPC） */
  const lastPush = new Map<string, string>()

  const roleRef: Ref<string> = typeof opts.role === 'string' ? ref(opts.role) : opts.role || ref('')

  /* ── 回调封装（任何回调抛异常都不该打断轮询）────────────────────────── */
  function safe<T extends unknown[]>(fn: ((...a: T) => void) | undefined, ...a: T) {
    if (!fn) return
    try {
      fn(...a)
    } catch (e) {
      // 回调是上层写的，它炸了不该把数据层带走
      console.warn('[worldsim] trip callback failed:', e)
    }
  }

  /* ── 位置回推 ───────────────────────────────────────────────────────── */

  async function pushPos(t: WsTrip, phase: WsTripPushPhase, pos?: WsTripPos): Promise<boolean> {
    if (!pushRuntime || !supported.value || !t?.role) return false
    if (phase === 'cancel' && !pushOnCancel) return false
    const at = num(t.at_ms, now.value) // 用快照锚点推，位置与服务端算的一致
    const p = pos || (phase === 'start' ? tripPosition(t, at) : tripPosition(t, now.value))
    const key = `${t.role}#${t.id}`
    const mark = pushMark.get(key) || { start: false, final: false, cancel: false }
    if (mark[phase]) return false
    const sig = `${phase}|${t.space}|${num(p.gx, NaN)},${num(p.gy, NaN)},${num(p.lng, NaN)},${num(p.lat, NaN)}`
    if (lastPush.get(key) === sig) return false
    mark[phase] = true
    pushMark.set(key, mark)
    lastPush.set(key, sig)
    try {
      await invoke('world_map_update_runtime', { patch: actorPatchOf(t, p) })
      return true
    } catch (e) {
      // 推失败要**允许重试**：把标记退回去（否则这一次位置就永远丢了）
      mark[phase] = false
      lastPush.delete(key)
      safe(opts.onError, e)
      return false
    }
  }

  /* ── 状态落地 ───────────────────────────────────────────────────────── */

  /** 把一份后端快照/事件里的 trip 标注上快照时间（插值锚点） */
  function stamp(t: WsTrip, atMs: number): WsTrip {
    return { ...t, at_ms: num(t.at_ms, atMs) }
  }

  /** 起程：新行程第一次被看见（事件或轮询都算） */
  function noteStart(t: WsTrip) {
    if (!t || seenStart.has(t.id)) return
    seenStart.add(t.id)
    safe(opts.onStart, t)
    void pushPos(t, 'start')
  }

  /** 到达：推一次**终点精确坐标**（progress = 1），然后交还给 P2 的静态推送 */
  function noteArrive(t: WsTrip) {
    if (!t || seenArrive.has(t.id)) return
    seenArrive.add(t.id)
    seenStart.add(t.id) // 到达的行程不必再报「起程」
    safe(opts.onArrive, t)
    if (pushRuntime && supported.value && t.role) {
      // 终点坐标是精确值（不是插值出来的），直接用它 —— 这一步推完就不再管这个人了
      const endPos: WsTripPos =
        t.space === 'geo'
          ? { lng: num(t.to?.lng, NaN), lat: num(t.to?.lat, NaN) }
          : { gx: num(t.to?.gx, NaN), gy: num(t.to?.gy, NaN) }
      const key = `${t.role}#${t.id}`
      const mark = pushMark.get(key) || { start: false, final: false, cancel: false }
      mark.final = false // 允许「到达」再推一次（可能是更精确的终点）
      pushMark.set(key, mark)
      lastPush.delete(key)
      void pushPos(t, 'final', endPos)
    }
  }

  function noteCancel(t: WsTrip) {
    if (!t || seenCancel.has(t.id)) return
    seenCancel.add(t.id)
    seenStart.add(t.id)
    safe(opts.onCancel, t)
    void pushPos(t, 'cancel')
  }

  /** 把一份状态响应合进本地状态，并跑「三时刻」的判定 */
  function applyStatus(r: WsTripStatusResp | null | undefined) {
    if (!r || r.ok !== true) throw new Error('world_map_trip_status 返回异常')
    const at = num(r.now_ms, Date.now())
    const list = Array.isArray(r.trips) ? r.trips.map((t) => stamp(t, at)) : []
    trips.value = list
    active.value = r.active ? stamp(r.active, at) : null
    speedup.value = num(r.speedup, speedup.value || 1)
    role.value = String(r.role || r.active?.role || roleRef.value || '')

    // ① 起程：列表里第一次出现的活行程
    for (const t of list) if (isTripLive(t)) noteStart(t)
    // ② 到达：后端「本次查询刚收口」的那批（这是最权威的到达信号）
    for (const t of Array.isArray(r.arrived_now) ? r.arrived_now : []) {
      noteArrive(stamp(t, at))
    }
    // ③ 兜底：状态已经是 arrived / cancelled 但我们还没回调过（事件丢了/错过了一轮）
    for (const t of list) {
      if (t.status === 'arrived') noteArrive(t)
      else if (t.status === 'cancelled') noteCancel(t)
    }
    // ④ 清理：行程从列表里消失（后端滚动窗口）后，把记账丢掉，避免长跑内存涨
    if (pushMark.size > 64) {
      const alive = new Set(list.map((t) => `${t.role}#${t.id}`))
      for (const k of [...pushMark.keys()]) if (!alive.has(k)) pushMark.delete(k)
      for (const k of [...lastPush.keys()]) if (!alive.has(k)) lastPush.delete(k)
    }
  }

  /* ── 轮询 ───────────────────────────────────────────────────────────── */

  /** 查一次状态（`invoke` 失败**不抛给调用方**，写进 lastError 并把旧数据留着） */
  async function refresh(): Promise<WsTrip[]> {
    if (!supported.value) return trips.value
    loading.value = true
    try {
      const args: Record<string, unknown> = {}
      if (roleRef.value) args.role = roleRef.value
      const r = await invoke<WsTripStatusResp>('world_map_trip_status', args)
      applyStatus(r)
      lastError.value = ''
    } catch (e) {
      lastError.value = e instanceof Error ? e.message : String(e)
      safe(opts.onError, e)
    } finally {
      loading.value = false
    }
    return trips.value
  }

  function schedule() {
    if (!running.value) return
    if (pollTimer !== null) window.clearTimeout(pollTimer)
    const ms = hidden.value ? hiddenPollMs : pollMs
    // 用 setTimeout 链而不是 setInterval：切换前后台频率时不会有「旧频率还排着一发」
    pollTimer = window.setTimeout(() => {
      void refresh().finally(schedule)
    }, ms)
  }

  /* ── 插值时钟（rAF，节流到 tickMs）────────────────────────────────────
   * 为什么还要 rAF：进度条的**逐帧**平滑归组件（它自己有 rAF），
   * 这里只要保证 `now` 在推进、且隐藏时**立刻停**（省电）。
   * 隐藏时停掉不影响正确性：位置是按时间戳算的，回到前台 `now` 立刻对齐。 */
  function tickLoop(ts: number) {
    if (!running.value || hidden.value) {
      rafId = null
      return
    }
    if (ts - lastTick >= tickMs) {
      lastTick = ts
      now.value = Date.now()
      // 到达前一帧：最后一个在途坐标推回后端（每个行程只推一次）
      for (const t of trips.value) {
        if (t.status !== 'moving') continue
        if (tripProgress(t, now.value) >= FINAL_PUSH_AT) void pushPos(t, 'final')
      }
    }
    rafId = window.requestAnimationFrame(tickLoop)
  }

  function tickStart() {
    if (rafId !== null || typeof window === 'undefined' || !window.requestAnimationFrame) return
    lastTick = 0
    rafId = window.requestAnimationFrame(tickLoop)
  }
  function tickStop() {
    if (rafId !== null) window.cancelAnimationFrame(rafId)
    rafId = null
  }

  /* ── 可见性：隐藏停 rAF、降频；回前台**立即对齐** ───────────────────── */
  function onVisibility() {
    const isHidden = typeof document !== 'undefined' && document.visibilityState === 'hidden'
    hidden.value = isHidden
    if (isHidden) {
      tickStop()
    } else {
      now.value = Date.now() // ← 「回到前台立即对齐」：这一帧就是正确位置
      tickStart()
      void refresh().finally(schedule) // 顺手立刻收口一次（可能已经到达了）
      return
    }
    schedule()
  }

  /* ── 事件订阅（只是「更快」，不是主通路）────────────────────────────── */
  async function subscribe() {
    if (!supported.value || bound) return
    bound = true
    try {
      unlisten = await listen<WsTripEventPayload>('world_map:trip', (ev) => {
        const p = ev?.payload
        const t = p?.trip
        if (!t || typeof t !== 'object') return
        const at = num(p.at, Date.now())
        const stamped = stamp(t, at)
        // 事件里的 trip 直接并进列表（同一个 id 覆盖），让 UI 立刻有反应
        const rest = trips.value.filter((x) => x.id !== stamped.id)
        trips.value = [stamped, ...rest].slice(0, 32)
        if (active.value?.id === stamped.id || !active.value) active.value = stamped
        if (p.phase === 'start') noteStart(stamped)
        else if (p.phase === 'arrived') noteArrive(stamped)
        else if (p.phase === 'cancelled') noteCancel(stamped)
        // 事件的 trip 可能没带 `now_ms` 的上下文，顺手再对齐一次轮询数据
        void refresh().finally(schedule)
      })
    } catch (e) {
      unlisten = null
      safe(opts.onError, e)
    }
  }

  /* ── 生命周期 ───────────────────────────────────────────────────────── */
  function start() {
    if (running.value) return
    running.value = true
    supported.value = isTauriRuntime()
    if (!supported.value) {
      // web 预览：命令不存在，静默停摆（不轮询、不报错）
      running.value = false
      return
    }
    hidden.value = typeof document !== 'undefined' && document.visibilityState === 'hidden'
    if (typeof document !== 'undefined') document.addEventListener('visibilitychange', onVisibility)
    now.value = Date.now()
    if (!hidden.value) tickStart()
    void subscribe()
    void refresh().finally(schedule)
  }

  function stop() {
    running.value = false
    if (pollTimer !== null) window.clearTimeout(pollTimer)
    pollTimer = null
    tickStop()
    if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onVisibility)
    if (unlisten) {
      try {
        unlisten()
      } catch {
        /* 卸载失败无所谓：Tauri 那边会随窗口一起清 */
      }
      unlisten = null
    }
    bound = false
  }

  /* ── 动作 ───────────────────────────────────────────────────────────── */

  /** 取消行程（没有行程时后端返回 `ok:false`，**不是异常** —— 如实回报给调用方） */
  async function cancel(target?: WsTrip | null): Promise<{ ok: boolean; message: string }> {
    const who = target?.role || active.value?.role || roleRef.value || ''
    if (!supported.value) return { ok: false, message: 'unsupported' }
    try {
      const args: Record<string, unknown> = {}
      if (who) args.role = who
      const r = await invoke<{ ok?: boolean; message?: string; trip?: WsTrip }>('world_map_trip_cancel', args)
      await refresh() // 取消后立刻重读（位置冻结在哪由后端说了算）
      return { ok: r?.ok === true, message: String(r?.message || '') }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      lastError.value = msg
      return { ok: false, message: msg }
    }
  }

  /**
   * 拨加速开关。
   *
   * ⚠️ **不本地乐观更新**：先把意图发给后端，再用返回值（`speedup` 权威值）
   * 与随后的**重读**（`world_map_trip_speedup` 会重锚行程、重挂定时器，
   * `changed`/`rearmed` 是这次拨动到底生效没有的唯一凭据）覆盖本地。
   * 传 `true/false` 走后端的 `enabled`（= 100×/1×），传数字走 `speedup`。
   */
  async function setSpeedup(next: number | boolean): Promise<{ ok: boolean; speedup: number; changed: boolean; rearmed: number }> {
    if (!supported.value) return { ok: false, speedup: speedup.value, changed: false, rearmed: 0 }
    try {
      const args: Record<string, unknown> = typeof next === 'boolean' ? { enabled: next } : { speedup: num(next, 1) }
      const r = await invoke<{ ok?: boolean; speedup?: number; changed?: boolean; rearmed?: unknown[] }>(
        'world_map_trip_speedup',
        args,
      )
      if (r && Number.isFinite(Number(r.speedup))) speedup.value = Number(r.speedup)
      await refresh() // 重锚之后必须重读：不然插值锚点还是旧的，进度会跳
      return {
        ok: r?.ok === true,
        speedup: speedup.value,
        changed: r?.changed === true,
        rearmed: Array.isArray(r?.rearmed) ? r.rearmed.length : 0,
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      lastError.value = msg
      return { ok: false, speedup: speedup.value, changed: false, rearmed: 0 }
    }
  }

  /** 起一条行程（调试 / 前端自己驱动；AI 那条路走后端 `dispatch_directives`） */
  async function startTrip(req: WsTripStartReq): Promise<{ ok: boolean; trip: WsTrip | null; message: string }> {
    if (!supported.value) return { ok: false, trip: null, message: 'unsupported' }
    try {
      const r = await invoke<{ ok?: boolean; trip?: WsTrip; message?: string }>('world_map_trip_start', { req })
      await refresh()
      return { ok: r?.ok === true, trip: r?.trip || null, message: String(r?.message || '') }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      lastError.value = msg
      return { ok: false, trip: null, message: msg }
    }
  }

  /* ── 派生量 ─────────────────────────────────────────────────────────── */
  /** 还在路上的行程（地图上要画车的就这一批） */
  const movingTrips = computed<WsTrip[]>(() => trips.value.filter((t) => isTripLive(t)))
  /** 归行程管的角色名（P2 推送前先查这里） */
  const ownerRoles = computed<string[]>(() => [...new Set(movingTrips.value.map((t) => t.role).filter(Boolean))])

  /**
   * **让位开关**：这个角色现在是不是归行程管。
   *
   * P2 的静态位置推送在推之前问一句 —— 为 true 就别推（否则会和插值位置互相覆盖，
   * 表现为「角色走两步被拉回去」）。到达/取消后自动变 false，静态推送随即接管。
   */
  function isOwned(roleName: string): boolean {
    const n = String(roleName || '').trim()
    if (!n) return false
    return movingTrips.value.some((t) => t.role === n)
  }

  /* ── 出口 ───────────────────────────────────────────────────────────── */
  const api = {
    // 状态
    trips,
    active,
    speedup,
    now,
    role,
    loading,
    lastError,
    hidden,
    supported,
    running,
    movingTrips,
    ownerRoles,
    // 生命周期
    start,
    stop,
    refresh,
    // 动作
    startTrip,
    cancel,
    setSpeedup,
    // 插值（绑定到响应式 now）
    progressOf: (t: WsTrip) => tripProgress(t, now.value),
    positionOf: (t: WsTrip) => tripPosition(t, now.value),
    remainingOf: (t: WsTrip) => tripRemainingM(t, now.value),
    doneOf: (t: WsTrip) => tripDoneM(t, now.value),
    etaOf: (t: WsTrip, effective = true) => tripEtaSecs(t, now.value, effective),
    arriveAtOf: (t: WsTrip, effective = true) => tripArriveAtMs(t, now.value, effective),
    // 让位开关
    isOwned,
  }

  // 组件/作用域销毁时自动收摊（拿不到作用域就交给调用方自己 stop()，不报错）
  if (getCurrentScope()) onScopeDispose(stop)

  if (opts.autoStart !== false) start()

  return api
}

export type WsTrips = ReturnType<typeof useWorldTrips>
