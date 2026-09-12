// 「世界模拟」地图手势（A 部分：**属于功能，PR 里应该有**）
//
// 为什么单独一个 composable：
//   · 地图页（WorldSim）与小区图（WsDistrict）两块都要用同一套手感，
//     写两遍必然手感不一致；
//   · 变换数学（钳制、以某点为锚缩放）是**纯函数**，拆出来才能被 node 自检覆盖
//     （见 ~/rikka/Dsh-SYuki/world_map/frontend_selftest_worldsim_p1.mjs 的【G】段）。
//
// ── 设计要点（逐条对应机主的要求）──────────────────────────────────────────
// ① **Pointer Events 统一处理**：pointerdown/move/up/cancel 一套吃下触屏、鼠标、笔，
//    不再写 touch* + mouse* 两套（两套要各自处理 capture/多指/取消，最容易出 bug）。
// ② **变换只作用在「舞台内层的变换容器」上**（translate + scale），**绝不改 SVG 本体**：
//    那个 SVG 是后端渲染的文本、由 v-html 注入的，改它的 viewBox/坐标会与它打架
//    （而且每次换级/流式重绘都会把属性冲掉）。变换容器是普通 div，重绘不影响它。
// ③ **拖动阈值 4px**：位移小于阈值一律当点击 —— 区划下钻靠的是 click，
//    没有阈值的话手指一碰就被判成拖动，点击会被吞掉。
//    另外拖动结束后的 350ms 内，click 会被 `shouldSuppressClick()` 拦掉
//    （手指滑动结束时浏览器照样会补一发 click，不拦就变成「拖完还顺带下钻一级」）。
// ④ 缩放 0.6~4×、平移边界回夹：scale≥1 时画面必须盖满容器（不允许露白边），
//    scale<1 时强制居中（不然缩小后会飘在角落）。
// ⑤ 手感：拖动中禁用过渡（`is-drag`），松手立刻恢复（CSS 里那条 0.28s 缓动）——
//    拖动跟手、复位动画顺滑，两者不能互相干扰。
//
// ⚠️ B 部分（抑制浏览器手势：touch-action / overscroll-behavior / preventDefault 默认滚动）
//    **不在这里**，也不在 worldsim.css 里，而是单独一个文件：
//    `src/assets/styles/worldsim-gesture-lock.css`（提 PR 时整份剔除，见 docs/world-map/09）。
//    本文件只做一件事与 B 相关：挂载时给舞台所在的 `.ws-root` 打上 `ws-nogesture` 类
//    （B 的样式全部以这个类为前缀，所以那份 CSS 一旦删掉，这个类就是个没有任何规则的死类，
//    留在 PR 里也无害 —— 这是刻意设计成「删除点只有一个文件」）。

import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue'

/** 平移 + 缩放状态（容器坐标：以舞台左上角为原点） */
export interface PanState {
  scale: number
  tx: number
  ty: number
}

/** 缩放上下限：0.6× 能一眼看全，4× 够看清街道名 */
export const GESTURE_MIN_SCALE = 0.6
export const GESTURE_MAX_SCALE = 4

/** 拖动阈值（CSS 像素）：小于它算点击。4px 是触屏上「手抖但不至于误判」的经验值 */
export const DRAG_THRESHOLD = 4

/** 拖动结束后抑制 click 的时长（ms）：手指离开后浏览器补发的那一发 click 要拦掉 */
export const CLICK_SUPPRESS_MS = 350

/**
 * 落在这些元素上的指针/滚轮**不启动地图手势**：
 *   · 按钮/链接/输入框（HUD 上的「停止」「重画」「＋」「－」、搜索框…）
 *   · 可滚动容器（绘制日志那种列表）—— 在它上面滚轮应该是**滚列表**，不是缩地图
 * 「点在按钮上却把地图拖走了」这种别扭，几乎全是从这儿来的。
 */
const NO_GESTURE_SELECTOR = 'button, a, input, select, textarea, .ws-zoomctl, .ws-prog, .ws-scroll, [data-no-gesture]'

/** 事件是不是落在「不该被地图手势吃掉」的元素上 */
export function isNoGestureTarget(target: EventTarget | null): boolean {
  const el = target as Element | null
  if (!el || typeof el.closest !== 'function') return false
  try {
    return !!el.closest(NO_GESTURE_SELECTOR)
  } catch {
    return false
  }
}

/** 双击/双指双击复位的判定窗口 */
const DOUBLE_TAP_MS = 320
const DOUBLE_TAP_DIST = 30

/* ══════════════════════════════════════════════════════════════════
 * 纯函数（自检直接调它们，不碰 DOM）
 * ══════════════════════════════════════════════════════════════════ */

/** 缩放钳制 */
export function clampScale(s: number, min = GESTURE_MIN_SCALE, max = GESTURE_MAX_SCALE): number {
  const n = Number.isFinite(s) && s > 0 ? s : 1
  return Math.min(max, Math.max(min, n))
}

/**
 * 平移回夹：别让图被拖出视野。
 *   · scale >= 1：画面比容器大，允许平移，但边缘不许进到容器里（min = 容器 - 内容）
 *   · scale < 1：画面比容器小，强制居中（否则缩小后会飘在角落，看着像 bug）
 */
export function clampPan(tx: number, ty: number, scale: number, w: number, h: number): { tx: number; ty: number } {
  const W = Math.max(1, Number(w) || 1)
  const H = Math.max(1, Number(h) || 1)
  const cx = (W - W * scale) / 2
  const cy = (H - H * scale) / 2
  if (scale <= 1) return { tx: cx, ty: cy }
  const minX = W - W * scale // 负值
  const minY = H - H * scale
  return {
    tx: Math.min(0, Math.max(minX, Number.isFinite(tx) ? tx : cx)),
    ty: Math.min(0, Math.max(minY, Number.isFinite(ty) ? ty : cy)),
  }
}

/**
 * 以容器坐标里的某个点为**不动的锚点**缩放到 nextScale。
 * 双指捏合（锚点=两指中点）与滚轮缩放（锚点=光标）都用它，保证「手指下的那块地不动」。
 */
export function zoomAtPoint(
  state: PanState,
  anchor: { x: number; y: number },
  nextScale: number,
  w: number,
  h: number,
  min = GESTURE_MIN_SCALE,
  max = GESTURE_MAX_SCALE,
): PanState {
  const s1 = Number.isFinite(state.scale) && state.scale > 0 ? state.scale : 1
  const s2 = clampScale(nextScale, min, max)
  const k = s2 / s1
  const tx = anchor.x - (anchor.x - state.tx) * k
  const ty = anchor.y - (anchor.y - state.ty) * k
  const p = clampPan(tx, ty, s2, w, h)
  return { scale: s2, tx: p.tx, ty: p.ty }
}

/** 位移是否够得上「拖动」（小于阈值当点击，交给区划下钻） */
export function isDrag(dx: number, dy: number, threshold = DRAG_THRESHOLD): boolean {
  return Math.hypot(dx, dy) >= threshold
}

/* ══════════════════════════════════════════════════════════════════
 * composable
 * ══════════════════════════════════════════════════════════════════ */

export interface UseWorldSimGesturesOptions {
  /** 事件源 + 边界参考（就是舞台，例如 `.ws-geo`） */
  target: Ref<HTMLElement | null>
  /** 真正被 translate/scale 的**内层变换容器** */
  content: Ref<HTMLElement | null>
  min?: number
  max?: number
}

export function useWorldSimGestures(opts: UseWorldSimGesturesOptions) {
  const min = opts.min ?? GESTURE_MIN_SCALE
  const max = opts.max ?? GESTURE_MAX_SCALE

  const scale = ref(1)
  const tx = ref(0)
  const ty = ref(0)
  /** 正在拖动（拖动中禁用过渡，跟手才不粘） */
  const dragging = ref(false)
  /** 用代码改变换时临时禁过渡（换级复位用，别让用户看到一次「滑过去」） */
  const instant = ref(false)

  const pointers = new Map<number, { x: number; y: number }>()
  /** 一次手势的基准（按下那一刻的状态 + 锚点信息） */
  let base: { x: number; y: number; tx: number; ty: number; scale: number; dist: number; mid: { x: number; y: number }; rect: DOMRect } | null = null
  let moved = false
  let suppressUntil = 0
  let lastTap = { t: 0, x: 0, y: 0 }

  const panStyle = computed(() => ({
    transform: `translate3d(${tx.value.toFixed(2)}px, ${ty.value.toFixed(2)}px, 0) scale(${scale.value.toFixed(4)})`,
  }))

  function rectOf(): DOMRect | null {
    const el = opts.target.value
    if (!el) return null
    const r = el.getBoundingClientRect()
    if (r.width <= 0 || r.height <= 0) return null
    return r
  }

  function apply(next: PanState) {
    scale.value = next.scale
    tx.value = next.tx
    ty.value = next.ty
  }

  /** 复位到 1× 居中。`animate=false` 用于换级（不要有一次动画） */
  function reset(animate = true) {
    if (!animate) {
      instant.value = true
      requestAnimationFrame(() => {
        instant.value = false
      })
    }
    apply({ scale: 1, tx: 0, ty: 0 })
  }

  /** 以容器中心为锚点缩放（给「＋ / −」按钮用；滚轮与捏合走各自的锚点） */
  function zoomBy(factor: number) {
    const r = rectOf()
    if (!r) return
    apply(zoomAtPoint({ scale: scale.value, tx: tx.value, ty: ty.value }, { x: r.width / 2, y: r.height / 2 }, scale.value * factor, r.width, r.height, min, max))
  }

  /**
   * 拖动结束后浏览器会补发 click（手指/鼠标都一样）。
   * 页面在做「点击区划 → 选中/下钻」之前必须先问一句，否则拖完地图会顺带下钻一级。
   */
  function shouldSuppressClick(): boolean {
    return Date.now() < suppressUntil
  }

  /* ── 事件 ────────────────────────────────────────────────────────── */

  function onDown(e: PointerEvent) {
    const el = opts.target.value
    if (!el) return
    // 只认主键（鼠标右键/中键不参与拖动）；触屏/笔没有 button 语义，button 恒为 0
    if (e.pointerType === 'mouse' && e.button !== 0) return
    // 按在 HUD 控件/可滚动列表上：交给它们自己处理，别把地图拖走
    if (isNoGestureTarget(e.target)) return
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY })
    // 指针捕获：手指滑出舞台后事件仍然回到这里，否则拖到边缘就断
    try {
      el.setPointerCapture(e.pointerId)
    } catch {
      /* 老 WebView 不支持捕获：退化也能用，只是拖出元素会断 */
    }
    const r = rectOf()
    if (!r) return
    if (pointers.size === 1) {
      moved = false
      base = { x: e.clientX, y: e.clientY, tx: tx.value, ty: ty.value, scale: scale.value, dist: 0, mid: { x: 0, y: 0 }, rect: r }
    } else if (pointers.size === 2) {
      // 第二根手指落下：以「此刻」为新基准（双指缩放从这一刻算起）
      const [a, b] = [...pointers.values()]
      const mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }
      base = {
        x: mid.x,
        y: mid.y,
        tx: tx.value,
        ty: ty.value,
        scale: scale.value,
        dist: Math.hypot(a.x - b.x, a.y - b.y),
        mid: { x: mid.x - r.left, y: mid.y - r.top },
        rect: r,
      }
      moved = true // 双指一落下就是手势，不再等阈值
      dragging.value = true
    }
  }

  function onMove(e: PointerEvent) {
    if (!pointers.has(e.pointerId)) return
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY })
    const pts = [...pointers.values()]
    if (!base) return

    if (pts.length === 1) {
      const dx = pts[0].x - base.x
      const dy = pts[0].y - base.y
      // 阈值内：什么都不做（此时还是「可能的点击」）
      if (!moved && !isDrag(dx, dy)) return
      moved = true
      dragging.value = true
      const p = clampPan(base.tx + dx, base.ty + dy, scale.value, base.rect.width, base.rect.height)
      tx.value = p.tx
      ty.value = p.ty
      return
    }

    // 双指：缩放围绕两指中点，同时跟随中点平移
    const [a, b] = pts
    const d = Math.hypot(a.x - b.x, a.y - b.y)
    const midClient = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }
    const k = base.dist > 1 ? d / base.dist : 1
    const z = zoomAtPoint(
      { scale: base.scale, tx: base.tx, ty: base.ty },
      base.mid,
      base.scale * k,
      base.rect.width,
      base.rect.height,
      min,
      max,
    )
    const p = clampPan(
      z.tx + (midClient.x - base.x),
      z.ty + (midClient.y - base.y),
      z.scale,
      base.rect.width,
      base.rect.height,
    )
    apply({ scale: z.scale, tx: p.tx, ty: p.ty })
  }

  function onUp(e: PointerEvent) {
    if (!pointers.has(e.pointerId)) return
    pointers.delete(e.pointerId)
    const el = opts.target.value
    try {
      el?.releasePointerCapture(e.pointerId)
    } catch {
      /* 已经释放/不支持捕获 */
    }

    if (pointers.size >= 1) {
      // 双指抬掉一根：以剩下那根为新基准继续拖，避免「跳一下」
      const [a] = [...pointers.values()]
      const r = rectOf()
      if (a && r) {
        base = { x: a.x, y: a.y, tx: tx.value, ty: ty.value, scale: scale.value, dist: 0, mid: { x: 0, y: 0 }, rect: r }
        moved = true
      }
      return
    }

    if (moved) {
      // 真拖过：接下来 350ms 的 click 一律拦掉（那是浏览器补发的）
      suppressUntil = Date.now() + CLICK_SUPPRESS_MS
    } else if (e.pointerType !== 'mouse') {
      // 没拖过 = 一次轻点：触屏上用它凑「双指双击/双击复位」
      const now = Date.now()
      const d = Math.hypot(e.clientX - lastTap.x, e.clientY - lastTap.y)
      if (now - lastTap.t < DOUBLE_TAP_MS && d < DOUBLE_TAP_DIST) {
        lastTap = { t: 0, x: 0, y: 0 }
        reset()
      } else {
        lastTap = { t: now, x: e.clientX, y: e.clientY }
      }
    }
    dragging.value = false
    moved = false
    base = null
  }

  function onCancel(e: PointerEvent) {
    // 浏览器把手势抢走了（比如页面开始滚动）：干净退出，别留下半途的状态
    pointers.delete(e.pointerId)
    if (pointers.size === 0) {
      dragging.value = false
      moved = false
      base = null
    }
  }

  function onWheel(e: WheelEvent) {
    const el = opts.target.value
    if (!el) return
    const t = e.target as Node | null
    if (t && !el.contains(t)) return
    // 光标在列表/控件上滚：那是滚列表 / 操作控件，不是缩地图
    if (isNoGestureTarget(e.target)) return
    // 滚轮缩放**是手势本身**，必须吃掉默认滚动，否则页面会跟着滚（这条属于 A：
    // 没有它，「桌面滚轮缩放」这个功能根本没法实现）。B 部分管的是触屏那套。
    e.preventDefault()
    const r = rectOf()
    if (!r) return
    const anchor = { x: e.clientX - r.left, y: e.clientY - r.top }
    // deltaY 归一：指数缩放保证「滚一格的手感」在任何缩放级别都一致
    const factor = Math.exp(-e.deltaY * 0.0016)
    apply(zoomAtPoint({ scale: scale.value, tx: tx.value, ty: ty.value }, anchor, scale.value * factor, r.width, r.height, min, max))
  }

  /** 鼠标双击复位（触屏的双击在 onUp 里判） */
  function onDblClick() {
    reset()
  }

  function onResize() {
    // 容器尺寸变了：把当前变换按新边界回夹一次，免得出界
    const r = rectOf()
    if (!r) return
    const p = clampPan(tx.value, ty.value, scale.value, r.width, r.height)
    tx.value = p.tx
    ty.value = p.ty
  }

  /* ── 绑定 / 解绑（舞台是 v-if 出来的，元素会整块换掉，必须重挂）────── */
  let bound: { el: HTMLElement; root: HTMLElement | null } | null = null

  function unbind() {
    const b = bound
    if (!b) return
    b.el.removeEventListener('pointerdown', onDown)
    b.el.removeEventListener('pointermove', onMove)
    b.el.removeEventListener('pointerup', onUp)
    b.el.removeEventListener('pointercancel', onCancel)
    b.el.removeEventListener('wheel', onWheel)
    b.el.removeEventListener('dblclick', onDblClick)
    window.removeEventListener('resize', onResize)
    // 卸掉 B 的开关类（换个舞台时由新舞台重新打）
    b.root?.classList.remove('ws-nogesture')
    bound = null
  }

  function bind() {
    unbind()
    const el = opts.target.value
    if (!el) return
    el.addEventListener('pointerdown', onDown)
    el.addEventListener('pointermove', onMove)
    el.addEventListener('pointerup', onUp)
    el.addEventListener('pointercancel', onCancel)
    // passive:false —— 滚轮要 preventDefault（见 onWheel 的说明）
    el.addEventListener('wheel', onWheel, { passive: false })
    el.addEventListener('dblclick', onDblClick)
    window.addEventListener('resize', onResize)
    // 交给 B 的开关类（`worldsim-gesture-lock.css` 全部规则都以它为前提）
    const root = el.closest('.ws-root') as HTMLElement | null
    root?.classList.add('ws-nogesture')
    bound = { el, root }
  }

  watch(
    [opts.target, opts.content],
    () => {
      pointers.clear()
      base = null
      moved = false
      dragging.value = false
      bind()
    },
    { immediate: true },
  )
  onBeforeUnmount(unbind)

  return {
    scale,
    tx,
    ty,
    dragging,
    instant,
    panStyle,
    reset,
    zoomBy,
    shouldSuppressClick,
  }
}

export type WorldSimGestures = ReturnType<typeof useWorldSimGestures>
