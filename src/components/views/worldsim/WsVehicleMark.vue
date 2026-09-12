<template>
  <!--
    「世界模拟」P4-3：地图上的**交通工具标记**（一个行程 === 一个标记）

    定位三件套（与 WsAvatarMark 完全同款思路，改之前先看那边的注释）：
      `left/top` 定位置 → `translate(-50%,-50%)` 把自己居中 → `scale(1/zoom)` 抵消地图手势，
    效果是「钉在地图上的那块地，但屏幕上始终是 size px」。
    少任何一条都会出问题：不抵消 → 地图放大 4× 时车变成 4 倍大饼。

    为什么自带一层 `inset:0` 的透明覆盖层（而不是像头像那样由层组件统一量盒子）：
      被限制「只能新建 5 个文件」，没有 WsVehicleLayer 可以承接「量一次盒子」这件事。
      所以这一层自己量（`offsetWidth/offsetHeight`，**不是** getBoundingClientRect —— 后者
      会被祖先的手势 transform 放大，量出来的盒子是错的）。
      代价只是每个在途行程一个空 div（同屏最多几条），换来的是接入方可以**直接铺**：
        <WsVehicleMark v-for="t in trips.movingTrips.value" :key="t.id"
                       :trip="t" :zoom="districtScale" :grid="WS_GRID" />
      不传 `pos` 时组件**自己**按 rAF（~30Hz）插值（位置真相仍是时间戳，见下面 selfDrive），
      所以车是连续走的、接入方不用管时钟；传了 `pos` 就以接入方给的为准。

    坐标系：
      · `space === 'grid'`（小区图）—— `pos.gx/gy` 经信箱折算成盒子像素，直接用；
      · `space === 'geo'`（真实经纬度）—— 小区草图**没有**地理投影，画不出来。
        给了 `project` 就按它算；没给就**不画**（宁可没有车，也不能把车画错地方）。
  -->
  <div
    ref="host"
    class="ws-veh"
    :class="[`ws-veh--${vehicle.kind}`, { 'is-flip': flipLeft, 'is-click': clickable, 'is-bare': vehicle.bare }]"
  >
    <img
      v-if="shown"
      class="ws-veh__ico"
      :class="{ 'ws-veh__ico--walk': vehicle.bare }"
      :src="icon"
      :alt="altText"
      :style="icoStyle"
      draggable="false"
      decoding="async"
      @click="onClick"
    />
    <span v-if="shown && showLabel" class="ws-veh__label" :style="labelStyle">{{ labelText }}</span>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { tripFacingLeft, tripPosition, type WsTrip, type WsTripPos } from '@/composables/useWorldTrips'
import { letterboxOf, gridToBox } from './wsActors'
import { kindLabelOf, vehicleIconOf, vehicleOf } from './wsVehicles'
import '@/assets/styles/worldsim-trip.css'

const props = withDefaults(
  defineProps<{
    /** 行程（`useWorldTrips` 的 trips / movingTrips 里的一条） */
    trip: WsTrip
    /**
     * 当前位置。**建议传 `trips.positionOf(trip)`**（按时间戳插值出来的，逐帧在动）；
     * 不传就用后端快照里的 `trip.pos`（只在下一次轮询时跳一下，不动画）。
     */
    pos?: WsTripPos | null
    /** 地图缩放倍率（小区图手势那套）—— 图标反向抵消，屏幕上始终 `size` px */
    zoom?: number
    /** 图标尺寸（屏幕 CSS 像素）。默认 30：比角色头像（26）略大，视觉上「车托着人」 */
    size?: number
    /** 小区图的网格边长（与 WsDistrict / 后端 sketch 一致，默认 28） */
    grid?: number
    /** 盒子尺寸（CSS 像素）。不传就自己量（见组件头注释） */
    boxW?: number
    boxH?: number
    /** 垂直微调（像素，正数往下）—— 想让车「在人的脚底下」时用得上 */
    offsetY?: number
    /** 步行要不要也画（默认 false：人自己走，脚下没有车） */
    showWalk?: boolean
    /** 图标下要不要写出行方式的中文名（默认 false，地图上够挤了） */
    showLabel?: boolean
    /** 到达/取消后是否隐藏（默认 true：人到了就不该还有一辆车在路上） */
    hideWhenDone?: boolean
    /** 方向翻转：'auto' 按运动方向水平镜像 / 'none' 永远朝右 */
    flipMode?: 'auto' | 'none'
    /** 能不能点（默认 false：不吃地图手势的事件） */
    clickable?: boolean
    /** geo 空间的投影函数（小区草图没有地理投影；给了才画得出经纬度行程） */
    project?: ((p: { lng: number; lat: number }) => { x: number; y: number } | null) | null
  }>(),
  {
    pos: null,
    zoom: 1,
    size: 30,
    grid: 28,
    boxW: 0,
    boxH: 0,
    offsetY: 0,
    showWalk: false,
    showLabel: false,
    hideWhenDone: true,
    flipMode: 'auto',
    clickable: false,
    project: null,
  },
)

const emit = defineEmits<{ (e: 'pick', trip: WsTrip): void }>()

const vehicle = computed(() => vehicleOf(props.trip?.kind))
const icon = computed(() => vehicleIconOf(props.trip?.kind))
const labelText = computed(() => kindLabelOf(props.trip?.kind, props.trip?.kind_zh))
const altText = computed(() => `${props.trip?.role || ''} ${labelText.value}`.trim())

/* ── 盒子尺寸：自己量（offsetWidth 不受祖先 transform 影响，见组件头注释）──── */
const host = ref<HTMLElement | null>(null)
const ownW = ref(0)
const ownH = ref(0)
let ro: ResizeObserver | null = null

function measure() {
  const el = host.value
  if (!el) return
  const w = el.offsetWidth || el.clientWidth
  const h = el.offsetHeight || el.clientHeight
  if (w > 0) ownW.value = w
  if (h > 0) ownH.value = h
}

onMounted(() => {
  measure()
  if (typeof ResizeObserver !== 'undefined' && host.value) {
    // 只关心**布局尺寸**变化（转屏/面板开合）；手势缩放不会触发 RO，也不会把盒子量错
    ro = new ResizeObserver(measure)
    ro.observe(host.value)
  } else if (typeof window !== 'undefined') {
    window.addEventListener('resize', measure)
  }
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', onVisibility)
  // 进来就在路上（页面刷新 / 中途打开地图）→ 立刻把位置算出来，别等下一次 rAF
  if (selfDrive.value) livePos.value = tripPosition(props.trip, Date.now())
  startRaf()
})

onBeforeUnmount(() => {
  ro?.disconnect()
  ro = null
  stopRaf()
  if (typeof window !== 'undefined') window.removeEventListener('resize', measure)
  if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onVisibility)
})

const boxW = computed(() => (props.boxW > 0 ? props.boxW : ownW.value))
const boxH = computed(() => (props.boxH > 0 ? props.boxH : ownH.value))

/* ── 位置（盒子内像素）──────────────────────────────────────────────────── */

/**
 * 自驱（自己按 rAF 插值）？
 *
 * 调用方**没传** `pos` 时才自驱 —— 传了就以调用方为准（那是它的时钟说话）。
 * 为什么要自驱：`useWorldTrips` 的 `now` 只有 ~5Hz（省电），车会一格一格地挪；
 * 这里用 rAF 按**同一个时间戳函数**补到 ~30Hz，肉眼看就是连续的。
 * 位置真相没变：还是 `tripProgress/tripPosition(trip, Date.now())`，不累加、不猜测。
 */
const SELF_TICK_MS = 33
const livePos = ref<WsTripPos | null>(null)
let rafId = 0
let lastTick = 0

const live = computed(() => {
  const s = String(props.trip?.status || '')
  return s === 'pending' || s === 'moving'
})
const selfDrive = computed(() => !props.pos && live.value)

function frame(ts: number) {
  if (!selfDrive.value) {
    rafId = 0
    return
  }
  rafId = window.requestAnimationFrame(frame)
  if (ts - lastTick < SELF_TICK_MS) return
  lastTick = ts
  livePos.value = tripPosition(props.trip, Date.now())
}

function startRaf() {
  if (rafId || typeof window === 'undefined' || !window.requestAnimationFrame) return
  lastTick = 0
  rafId = window.requestAnimationFrame(frame)
}
function stopRaf() {
  if (rafId) window.cancelAnimationFrame(rafId)
  rafId = 0
}

/** 页面隐藏停 rAF（省电）；回到前台立刻按时间戳对齐再重启 */
function onVisibility() {
  if (typeof document === 'undefined') return
  if (document.visibilityState === 'hidden') {
    stopRaf()
  } else {
    if (selfDrive.value) livePos.value = tripPosition(props.trip, Date.now())
    startRaf()
  }
}

watch(selfDrive, (on) => {
  if (on) startRaf()
  else {
    stopRaf()
    livePos.value = null // 交回给调用方给的 pos / 后端快照
  }
})

// 换了行程（id 变了）：立刻对齐一次，别让新车从上一辆的位置滑过去
watch(
  () => props.trip?.id,
  () => {
    if (selfDrive.value) livePos.value = tripPosition(props.trip, Date.now())
  },
)

/** 生效的位置：调用方的 `pos` > 自驱的逐帧位置 > 后端快照 */
const at = computed<WsTripPos>(() => props.pos || livePos.value || props.trip?.pos || {})

/** 折算到盒子像素；算不出来（geo 没投影 / 坐标缺失 / 盒子还没量到）就是 null */
const place = computed<{ x: number; y: number } | null>(() => {
  const t = props.trip
  if (!t) return null
  const p = at.value
  if (t.space === 'geo') {
    const lng = Number(p?.lng)
    const lat = Number(p?.lat)
    if (!Number.isFinite(lng) || !Number.isFinite(lat) || !props.project) return null
    const hit = props.project({ lng, lat })
    if (!hit || !Number.isFinite(hit.x) || !Number.isFinite(hit.y)) return null
    return { x: hit.x, y: hit.y }
  }
  const gx = Number(p?.gx)
  const gy = Number(p?.gy)
  if (!Number.isFinite(gx) || !Number.isFinite(gy)) return null
  if (!(boxW.value > 0) || !(boxH.value > 0)) return null // 还没量到盒子，这一帧先不画
  const lb = letterboxOf(boxW.value, boxH.value, props.grid)
  return gridToBox(gx, gy, lb)
})

/** 到底画不画 */
const shown = computed(() => {
  if (!props.trip) return false
  if (props.hideWhenDone && !live.value) return false
  if (vehicle.value.bare && !props.showWalk) return false // 步行：人自己走，脚下不画车
  return !!place.value
})

/** 方向翻转：往左走 → 水平镜像（立绘默认朝右） */
const flipLeft = computed(() => props.flipMode === 'auto' && !!props.trip && tripFacingLeft(props.trip))

/**
 * 图标样式：定位 + 居中 + **抵消地图缩放**（+ 反向时的水平镜像）。
 * 镜像用 `scale(-1/k, 1/k)` 完成，不去改图片本身。
 */
const icoStyle = computed(() => {
  const p = place.value
  const k = Number(props.zoom)
  const z = Number.isFinite(k) && k > 0 ? k : 1
  const sx = (flipLeft.value ? -1 : 1) / z
  const sy = 1 / z
  return {
    left: p ? `${p.x.toFixed(2)}px` : '0px',
    top: p ? `${(p.y + props.offsetY).toFixed(2)}px` : '0px',
    width: `${props.size}px`,
    height: `${props.size}px`,
    transform: `translate(-50%, -50%) scale(${sx.toFixed(4)}, ${sy.toFixed(4)})`,
  }
})

/** 标签跟着图标走（同一个抵消，只是往下偏一点） */
const labelStyle = computed(() => {
  const p = place.value
  const k = Number(props.zoom)
  const z = Number.isFinite(k) && k > 0 ? k : 1
  const dy = props.size / 2 + 2
  return {
    left: p ? `${p.x.toFixed(2)}px` : '0px',
    top: p ? `${(p.y + props.offsetY + dy / z).toFixed(2)}px` : '0px',
    transform: `translate(-50%, 0) scale(${(1 / z).toFixed(4)})`,
  }
})

function onClick(e: MouseEvent) {
  if (!props.clickable) return
  e.stopPropagation()
  emit('pick', props.trip)
}

/**
 * 兜底导出：把「这一帧该画在哪」也算一遍（给将来要在 canvas/别的层里画的人用）。
 * 纯函数，不进响应式。
 */
function markPointOf(t: WsTrip, nowMs: number, boxW: number, boxH: number, grid = 28) {
  const p = tripPosition(t, nowMs)
  if (t.space !== 'grid') return null
  const lb = letterboxOf(boxW, boxH, grid)
  return gridToBox(Number(p.gx) || 0, Number(p.gy) || 0, lb)
}
defineExpose({ markPointOf })
</script>
