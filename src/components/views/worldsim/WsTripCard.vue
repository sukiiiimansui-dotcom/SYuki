<template>
  <!--
    「世界模拟」P4-2：行程卡（谁 → 去哪儿 / 走多远 / 还有多久 / 100× 加速 / 取消）

    三条设计约束（都对应用户明确提过的问题）：
      ① **进度条要平滑**：`requestAnimationFrame` 逐帧插值，但**位置真相仍是时间戳**
         （`tripProgress(trip, Date.now())`）—— 不是「每帧 +1%」那种累加。
         页面隐藏时（`visibilitychange`）停掉 rAF 省电，回到前台立刻按时间戳对齐。
         逐帧只改 `transform: scaleX()`（不触发布局），文字节流到 ~4 次/秒。
      ② **加速开关不做乐观更新**：按钮的选中态只认 `speedup` 这个 prop（后端权威值），
         拨动只发 `speedup` 事件出去，父组件调 `setSpeedup()` 后**重读**再传回来。
      ③ **文案全在组件里**：`ZH` 常量 + `strings` prop 覆盖（i18n 由接入方统一收，
         这一阶段先不碰 `src/locales/**`）。
  -->
  <div
    class="ws-trip"
    :class="[`is-${status}`, { 'is-floating': floating, 'is-right': floating && floatingSide === 'right', 'is-busy': busy }]"
    role="group"
    :aria-label="S.title"
  >
    <!-- 空态：默认不画（父组件一般 v-if 掉），显式要求时才给一行 -->
    <div v-if="!trip" class="ws-trip__empty">{{ S.empty }}</div>

    <template v-else>
      <div class="ws-trip__head">
        <img class="ws-trip__ico" :src="icon" :alt="kindText" draggable="false" decoding="async" />
        <div class="ws-trip__route">
          <div class="ws-trip__names">
            <span class="ws-trip__from">{{ fromName }}</span>
            <span class="ws-trip__arrow" aria-hidden="true">→</span>
            <span class="ws-trip__to">{{ toName }}</span>
          </div>
          <div class="ws-trip__sub">
            <span class="ws-trip__who">{{ trip.role }}</span>
            <span class="ws-trip__dot" aria-hidden="true">·</span>
            <span class="ws-trip__kind">{{ kindText }}</span>
            <span v-if="!trip.kind_explicit" class="ws-trip__auto">{{ S.autoKind }}</span>
          </div>
        </div>
        <span class="ws-trip__status">{{ statusText }}</span>
      </div>

      <!-- 进度条：外层是轨道，内层条由 rAF 直接改 transform（GPU 合成，不触发布局） -->
      <div
        class="ws-trip__bar"
        role="progressbar"
        :aria-valuemin="0"
        :aria-valuemax="100"
        :aria-valuenow="Math.round(pctShown)"
        :aria-label="S.progress"
      >
        <i ref="barEl" class="ws-trip__fill" />
      </div>

      <div class="ws-trip__stats">
        <span class="ws-trip__stat">
          <b>{{ pctShown }}%</b>
          <em>{{ S.progress }}</em>
        </span>
        <span class="ws-trip__stat">
          <b>{{ fmtDistance(remainShown) }}</b>
          <em>{{ S.remain }}</em>
        </span>
        <span class="ws-trip__stat">
          <b>{{ etaText }}</b>
          <em>{{ S.eta }}</em>
        </span>
        <span class="ws-trip__stat">
          <b>{{ fmtDistance(doneShown) }} / {{ fmtDistance(totalM) }}</b>
          <em>{{ S.mileage }}</em>
        </span>
      </div>

      <div class="ws-trip__foot">
        <!-- 100× 加速：选中态**只**来自 speedup（后端权威值） -->
        <button
          class="ws-trip__fast"
          :class="{ 'is-on': fast, 'is-ready': !fast }"
          type="button"
          :disabled="busy || !live"
          :aria-pressed="fast"
          :title="fastTip"
          @click="toggleFast"
        >
          <span aria-hidden="true">⚡</span>
          {{ fastText }}
        </button>

        <span v-if="fast" class="ws-trip__hint">{{ S.slowEta }} {{ fmtDuration(slowEtaSecs) }}</span>
        <span class="ws-trip__spacer" />

        <button
          v-if="live && showCancel"
          class="ws-trip__cancel"
          type="button"
          :disabled="busy"
          @click="emit('cancel')"
        >
          {{ busy ? S.cancelling : S.cancel }}
        </button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  tripDoneM,
  tripEtaSecs,
  tripProgress,
  tripRemainingM,
  type WsTrip,
} from '@/composables/useWorldTrips'
import { vehicleIconOf, kindLabelOf } from './wsVehicles'
// 样式走**独立 css 文件**（与 P1~P3 的 worldsim.css 分开，避免和并行改同一份样式的代理打架）
import '@/assets/styles/worldsim-trip.css'

/* ── 文案：全部是组件内常量（i18n 由接入方统一收，这里不碰 locales）─────── */
const ZH = {
  title: '行程',
  empty: '现在没有行程',
  progress: '进度',
  remain: '还剩',
  eta: '预计',
  mileage: '已走 / 全程',
  autoKind: '（按距离自动选）',
  fastLabel: '{n}× 加速中',
  fastLabelOff: '{n}× 加速',
  fastOnTip: '打开 {n}× 加速（后端会重锚行程，剩余时间立刻缩短）',
  fastOffTip: '关掉加速，回到真实速度',
  slowEta: '常速还需',
  cancel: '取消行程',
  cancelling: '正在取消…',
  departing: '等待出发',
  moving: '在路上',
  arriving: '即将到达',
  arrived: '已到达',
  cancelled: '已取消',
  unknownPlace: '未知地点',
  lessThanSec: '不到 1 秒',
  sec: '秒',
  min: '分',
  hour: '小时',
  meter: '米',
  km: '公里',
  clockArrive: '到达',
}

/** 文案表（`strings` prop 覆盖时用的键集；`<script setup>` 不许 export，接入方按需自取） */
type WsTripCardStrings = Record<keyof typeof ZH, string>

const props = withDefaults(
  defineProps<{
    /** 要显示的行程（`useWorldTrips` 的 `active`；没有就传 null） */
    trip?: WsTrip | null
    /** 后端权威的加速倍率（1 = 常速，100 = 100×）—— 组件的选中态只认它 */
    speedup?: number
    /** 父组件正在拨开关 / 取消（禁用按钮，防连点） */
    busy?: boolean
    /** 「加速」的目标倍率（默认 100，与后端 `SPEEDUP_FAST` 一致） */
    fastSpeedup?: number
    /** 是否浮在地图某个角上（默认 false：当普通卡片，由父组件决定放哪） */
    floating?: boolean
    /** floating 时靠哪边（默认左；`.ws-people` 在右下角，行程卡默认放左下角不打架） */
    floatingSide?: 'left' | 'right'
    /** 是否给「取消行程」按钮（只读展示时关掉） */
    showCancel?: boolean
    /** 覆盖任意文案（i18n 接进来时用，缺的键自动落回内置中文） */
    strings?: Partial<WsTripCardStrings>
  }>(),
  {
    trip: null,
    speedup: 1,
    busy: false,
    fastSpeedup: 100,
    floating: false,
    floatingSide: 'left',
    showCancel: true,
    strings: () => ({}),
  },
)

const emit = defineEmits<{
  (e: 'cancel'): void
  /** 拨加速开关：true = 要 100×，false = 回常速（父组件调 setSpeedup 并重读） */
  (e: 'speedup', fast: boolean): void
}>()

const S = computed<WsTripCardStrings>(() => ({ ...ZH, ...(props.strings || {}) }))

/* ── 时钟状态（先声明：下面的派生量要依赖 `textNow` 才会随文字刷新重算）────── */

/** 文本重算间隔（毫秒）：逐帧改文字在手机上纯属浪费 */
const TEXT_MS = 240

const barEl = ref<HTMLElement | null>(null)
/** 文字用的时间（节流后的 Date.now()）—— 进度条的**逐帧**插值不经过它 */
const textNow = ref(Date.now())
/** 进度条的百分比（节流后给文字/aria 用；逐帧的条宽直接写 DOM，不走响应式） */
const pctShown = ref(0)
const remainShown = ref(0)
const doneShown = ref(0)
const slowEtaSecs = ref(0)

let rafId = 0
let lastText = 0

/* ── 展示派生量 ─────────────────────────────────────────────────────────── */

const status = computed(() => String(props.trip?.status || 'pending'))
const live = computed(() => status.value === 'pending' || status.value === 'moving')
const fast = computed(() => Number(props.speedup) > 1)
/** 按钮文字：`{n}` 用 `fastSpeedup` 填（默认 100，与后端 `SPEEDUP_FAST` 一致） */
const fastText = computed(() => fill(fast.value ? S.value.fastLabel : S.value.fastLabelOff))
/** 按钮 tooltip：同一个 `{n}` 口径 */
const fastTip = computed(() => fill(fast.value ? S.value.fastOffTip : S.value.fastOnTip))
function fill(tpl: string): string {
  return String(tpl).replace('{n}', String(props.fastSpeedup))
}

const icon = computed(() => vehicleIconOf(props.trip?.kind))
const kindText = computed(() => kindLabelOf(props.trip?.kind, props.trip?.kind_zh))

const fromName = computed(() => String(props.trip?.from?.name || '').trim() || S.value.unknownPlace)
const toName = computed(() => String(props.trip?.to?.name || '').trim() || S.value.unknownPlace)
const totalM = computed(() => Math.max(0, Number(props.trip?.distance_m) || 0))

const statusText = computed(() => {
  void textNow.value // 依赖节流时钟：进度跨过 99.9% 时文案要跟着从「在路上」变「即将到达」
  if (status.value === 'arrived') return S.value.arrived
  if (status.value === 'cancelled') return S.value.cancelled
  if (status.value === 'pending') return S.value.departing
  // 还剩最后一帧（≥99.9%）时换个说法，避免「进度 100% 还在路上」
  return progressNow() >= 0.999 ? S.value.arriving : S.value.moving
})

/* ── rAF：逐帧插值（真相同一时间戳函数）────────────────────────────────── */

function progressNow(): number {
  const t = props.trip
  if (!t) return 0
  return tripProgress(t, Date.now())
}

/** 逐帧：只改 bar 的 transform（合成层）+ 按节流刷新文字 */
function frame() {
  rafId = window.requestAnimationFrame(frame)
  const t = props.trip
  if (!t) return
  const nowMs = Date.now()
  const p = tripProgress(t, nowMs)
  const el = barEl.value
  if (el) el.style.transform = `scaleX(${p.toFixed(4)})`
  if (nowMs - lastText >= TEXT_MS) {
    lastText = nowMs
    textNow.value = nowMs
    pctShown.value = Math.round(p * 100)
    remainShown.value = tripRemainingM(t, nowMs)
    doneShown.value = tripDoneM(t, nowMs)
    slowEtaSecs.value = tripEtaSecs(t, nowMs, false)
  }
}

/** 立刻对齐一次（回到前台 / 换了一条行程 / 倍率变了，都要马上反映，不等下一帧节流） */
function realign() {
  const t = props.trip
  const nowMs = Date.now()
  lastText = nowMs
  textNow.value = nowMs
  if (!t) {
    pctShown.value = 0
    remainShown.value = 0
    doneShown.value = 0
    slowEtaSecs.value = 0
    if (barEl.value) barEl.value.style.transform = 'scaleX(0)'
    return
  }
  const p = tripProgress(t, nowMs)
  pctShown.value = Math.round(p * 100)
  remainShown.value = tripRemainingM(t, nowMs)
  doneShown.value = tripDoneM(t, nowMs)
  slowEtaSecs.value = tripEtaSecs(t, nowMs, false)
  if (barEl.value) barEl.value.style.transform = `scaleX(${p.toFixed(4)})`
}

function startRaf() {
  if (rafId || typeof window === 'undefined' || !window.requestAnimationFrame) return
  rafId = window.requestAnimationFrame(frame)
}
function stopRaf() {
  if (rafId) window.cancelAnimationFrame(rafId)
  rafId = 0
}

/** 页面隐藏 → 停 rAF；回到前台 → 立刻按时间戳对齐并重启（手机息屏冻定时器也不怕） */
function onVisibility() {
  if (typeof document === 'undefined') return
  if (document.visibilityState === 'hidden') {
    stopRaf()
  } else {
    realign()
    startRaf()
  }
}

onMounted(() => {
  realign()
  startRaf()
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', onVisibility)
})

onBeforeUnmount(() => {
  stopRaf()
  if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onVisibility)
})

// 换行程 / 换倍率（后端重锚过）：立刻对齐，避免旧进度闪一下
watch(() => [props.trip?.id, props.trip?.status, props.speedup, props.trip?.at_ms], () => {
  realign()
  if (typeof document === 'undefined' || document.visibilityState !== 'hidden') startRaf()
})

/* ── 动作 ───────────────────────────────────────────────────────────────── */

function toggleFast() {
  if (props.busy || !live.value) return
  // 只发意图，不改本地状态：真正的倍率由父组件 setSpeedup() 后从后端重读回来
  emit('speedup', !fast.value)
}

/* ── 格式化（纯展示，不参与任何计算）────────────────────────────────────── */

function fmtDistance(m: number): string {
  const v = Math.max(0, Number(m) || 0)
  if (v < 1000) return `${Math.round(v)} ${S.value.meter}`
  if (v < 10_000) return `${(v / 1000).toFixed(1)} ${S.value.km}`
  return `${Math.round(v / 1000)} ${S.value.km}`
}

function fmtDuration(secs: number): string {
  const v = Math.max(0, Number(secs) || 0)
  if (v < 1) return S.value.lessThanSec
  if (v < 60) return `${Math.ceil(v)} ${S.value.sec}`
  if (v < 3600) {
    const m = Math.floor(v / 60)
    const s = Math.round(v % 60)
    return s > 0 ? `${m} ${S.value.min} ${s} ${S.value.sec}` : `${m} ${S.value.min}`
  }
  const h = Math.floor(v / 3600)
  const m = Math.round((v % 3600) / 60)
  return m > 0 ? `${h} ${S.value.hour} ${m} ${S.value.min}` : `${h} ${S.value.hour}`
}

/** ETA 文案：加速时按**有效速度**算（100× 下「还要 47 秒」是错的），并附到达钟点 */
const etaText = computed(() => {
  void textNow.value // 依赖节流时钟：每次文字刷新都重算
  const t = props.trip
  if (!t) return '—'
  if (status.value === 'arrived') return S.value.arrived
  if (status.value === 'cancelled') return S.value.cancelled
  const secs = tripEtaSecs(t, textNow.value, true)
  const clock = fmtClock(textNow.value + secs * 1000)
  return `${fmtDuration(secs)} · ${S.value.clockArrive} ${clock}`
})

function fmtClock(ms: number): string {
  const d = new Date(ms)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}`
}
</script>
