<template>
  <!-- 世界地图叠加层（T6-2）：既是半透明背景层，也能缩成角落小窗 -->
  <template v-if="mode !== 'off'">
    <!-- ① 背景层：铺满、半透明、不吃鼠标事件 -->
    <div v-if="mode === 'overlay'" ref="overlayBox" class="wml-overlay" :style="{ opacity }">
      <img v-if="img" class="wml-img" :src="img" alt="" @error="onErr" />
      <div class="wml-veil" />
      <div v-if="loading" class="wml-hint">世界地图加载中…</div>
    </div>

    <!-- ② 角落小窗：可拖动、可点开完整世界页 -->
    <div
      v-else
      class="wml-corner"
      :style="cornerStyle"
      @mousedown="startDrag"
      @touchstart.passive="startDrag"
    >
      <div class="wml-head">
        <span class="wml-title">🧭 {{ mainName }}</span>
        <div class="wml-ops" @mousedown.stop @touchstart.stop>
          <button class="wml-op" title="切成半透明背景层" @click="setMode('overlay')">▤</button>
          <button class="wml-op" title="打开世界地图" @click="openWorld">⛶</button>
          <button class="wml-op" title="关闭" @click="setMode('off')">✕</button>
        </div>
      </div>
      <div ref="bodyBox" class="wml-body">
        <img v-if="img" class="wml-img2" :src="img" alt="" @error="onErr" />
        <div v-if="loading" class="wml-hint sm">加载中…</div>
      </div>
      <div class="wml-foot">
        <span v-if="nowText">{{ nowText }}</span>
        <span v-if="roles.length">{{ roles.length }} 位角色在活动</span>
        <span v-if="remotes.length">远处块 {{ remotes.length }}</span>
      </div>
    </div>
  </template>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useWorldMapLayer } from '@/composables/useWorldMapLayer'
import worldMapApi, {
  MAP_SVG_DEFAULT_H,
  MAP_SVG_DEFAULT_W,
  mapSvgUrl,
  type RemoteBlock,
  type ScheduleRole,
} from '@/api/services/worldMap'
import { bindWorldData } from '@/composables/useWorldMapBindings'

const router = useRouter()
const { state, setMode, setPos } = useWorldMapLayer()

const mode = computed(() => state.value.mode)
const opacity = computed(() => state.value.opacity)
const loading = ref(false)
const imgErr = ref(false)
const tick = ref(Date.now())
const mainName = ref('世界地图')
const remotes = ref<RemoteBlock[]>([])
const roles = ref<ScheduleRole[]>([])
const nowText = ref('')
const adcode = ref('')

// ── 图层图片：异步取 data URL ──
//
// 原来是 `computed` 直接拼 `http://127.0.0.1:8791/api/bigmap?...`；
// 打包成 APK 后没有这个 HTTP 服务，图片必然加载不出来（叠加层就只剩一层灰色纱罩）。
// 现在统一走 mapSvgUrl()：真壳 invoke Rust 命令，浏览器走调试服务 —— 两边都能出图。
const img = ref('')
/** 叠加层 / 角落小窗的**真实 CSS 像素**尺寸（喂给后端渲染，手机上的字才看得清） */
const boxSize = ref({ w: 0, h: 0 })
const overlayBox = ref<HTMLElement | null>(null)
const bodyBox = ref<HTMLElement | null>(null)

/** 请求序号：切模式/改尺寸会连发多个请求，只有最后发出的那个允许写回 */
let imgSeq = 0

/** 当前模式下真正承载图片的那个盒子（两个分支是 v-if/v-else，同时只存在一个） */
function currentBox(): HTMLElement | null {
  return mode.value === 'overlay' ? overlayBox.value : bodyBox.value
}

/**
 * 取图层图。失败保持静默（叠加层是聊天界面的背景，不该弹错误打扰用户），
 * 只把 src 置空避免显示破图；imgErr 由 loadRegion() 负责复位。
 */
async function loadImg(useDefaultSize = false) {
  if (imgErr.value || !adcode.value) {
    img.value = ''
    return
  }
  let w = boxSize.value.w
  let h = boxSize.value.h
  if (w <= 0 || h <= 0) {
    if (!useDefaultSize) return // 还没量到尺寸：等 ResizeObserver
    w = MAP_SVG_DEFAULT_W
    h = MAP_SVG_DEFAULT_H
  }
  const seq = ++imgSeq
  try {
    const url = await mapSvgUrl(adcode.value, state.value.style, w, h)
    if (seq === imgSeq) img.value = url
  } catch {
    if (seq !== imgSeq) return
    img.value = ''
    imgErr.value = true
  }
}

/**
 * 影响图片的全部输入：区域 / 风格 / 模式（决定盒子尺寸）/ 刷新计数 / 盒子尺寸 / 错误标记。
 * 把 `imgErr` 也算进来，是为了保留原来的语义：loadRegion() 复位 imgErr 后图片会再试一次
 * （它一变成 true 就只触发一次「空转」，不会自旋）。
 */
const imgKey = computed(
  () =>
    `${adcode.value}|${state.value.style}|${mode.value}|${tick.value}|${boxSize.value.w}x${boxSize.value.h}|${
      imgErr.value ? 'e' : 'k'
    }`,
)
watch(imgKey, () => {
  void loadImg()
})

// ── 盒子尺寸监听（模式切换会换元素，必须重新 observe）──
let boxRo: ResizeObserver | null = null
let boxTimer: number | null = null
let fallbackTimer: number | null = null
let boxRoFired = false

function measureBox() {
  const el = currentBox()
  if (!el) return
  const r = el.getBoundingClientRect()
  const w = Math.round(r.width)
  const h = Math.round(r.height)
  const cur = boxSize.value
  // 抖动过滤：变化不到 8px 不重画（拖窗口/转屏时 ResizeObserver 会连发几十次）
  if (Math.abs(w - cur.w) < 8 && Math.abs(h - cur.h) < 8) return
  boxSize.value = { w, h }
}

function detachBoxObserver() {
  if (boxTimer !== null) {
    window.clearTimeout(boxTimer)
    boxTimer = null
  }
  // 兜底计时器也要清：叠加层都关掉了就别再发这一枪
  if (fallbackTimer !== null) {
    window.clearTimeout(fallbackTimer)
    fallbackTimer = null
  }
  window.removeEventListener('resize', measureBox)
  boxRo?.disconnect()
  boxRo = null
  boxRoFired = false
}

/** 兜底：尺寸一直量不到（老 WebView 的 RO 不回调 / 元素被隐藏）也要出图 —— 宁可字小，也别空着 */
function armSizeFallback() {
  if (fallbackTimer !== null) return
  fallbackTimer = window.setTimeout(() => {
    fallbackTimer = null
    if (boxSize.value.w <= 0) void loadImg(true)
  }, 1500)
}

function attachBoxObserver() {
  detachBoxObserver()
  const el = currentBox()
  if (!el) return
  armSizeFallback() // 每次（重新）挂监听都重新武装：切模式会 detach 掉上一个
  if (typeof ResizeObserver === 'undefined') {
    // 老 WebView（Chromium < 64）没有 ResizeObserver：退回「量一次 + 监听 window.resize」
    measureBox()
    window.addEventListener('resize', measureBox)
    return
  }
  boxRo = new ResizeObserver(() => {
    // 首次回调 = 布局就绪，立刻量；之后（拖拽/转屏连发）才走防抖
    if (!boxRoFired) {
      boxRoFired = true
      measureBox()
      return
    }
    if (boxTimer !== null) window.clearTimeout(boxTimer)
    boxTimer = window.setTimeout(measureBox, 220)
  })
  boxRo.observe(el)
}

// 叠加层 ↔ 角落小窗 切换时 DOM 元素换了，监听目标要跟着换（等 DOM 更新完再挂）
watch(mode, async () => {
  await nextTick()
  attachBoxObserver()
})

const cornerStyle = computed(() => {
  const s: Record<string, string> = {}
  if (state.value.x >= 0 && state.value.y >= 0) {
    s.left = state.value.x + 'px'
    s.top = state.value.y + 'px'
    s.right = 'auto'
    s.bottom = 'auto'
  }
  return s
})

function onErr() {
  imgErr.value = true
}

async function loadRegion() {
  loading.value = true
  imgErr.value = false
  try {
    const d = await worldMapApi.blocks(state.value.adcode || undefined, state.value.style, 6)
    if (d?.ok) {
      adcode.value = d.main.adcode
      mainName.value = d.main.name
      remotes.value = d.remotes || []
    }
  } catch {
    /* 服务没起时保持静默，不打扰聊天 */
  } finally {
    loading.value = false
  }
}

async function loadRoles() {
  try {
    const s = await worldMapApi.schedule()
    roles.value = s.roles || []
  } catch {
    roles.value = []
  }
  try {
    const t: any = await worldMapApi.time()
    const hh = t?.hour ?? t?.clock?.hour
    const mm = t?.minute ?? t?.clock?.minute
    if (hh !== undefined) nowText.value = `🕐 ${String(hh).padStart(2, '0')}:${String(mm ?? 0).padStart(2, '0')}`
  } catch {
    nowText.value = ''
  }
}

function openWorld() {
  router.push('/world')
}

// ── 拖动小窗 ──
let dragging = false
let sx = 0
let sy = 0
let ox = 0
let oy = 0
function startDrag(e: MouseEvent | TouchEvent) {
  const t: any = 'touches' in e ? e.touches[0] : e
  if (!t) return
  const el = (e.currentTarget as HTMLElement).getBoundingClientRect()
  dragging = true
  sx = t.clientX
  sy = t.clientY
  ox = el.left
  oy = el.top
  window.addEventListener('mousemove', onDrag)
  window.addEventListener('mouseup', endDrag)
  window.addEventListener('touchmove', onDrag, { passive: false })
  window.addEventListener('touchend', endDrag)
}
function onDrag(e: MouseEvent | TouchEvent) {
  if (!dragging) return
  const t: any = 'touches' in e ? e.touches[0] : e
  if (!t) return
  const nx = Math.max(0, Math.min(window.innerWidth - 120, ox + (t.clientX - sx)))
  const ny = Math.max(0, Math.min(window.innerHeight - 60, oy + (t.clientY - sy)))
  setPos(nx, ny)
  if ('touches' in e) e.preventDefault()
}
function endDrag() {
  dragging = false
  window.removeEventListener('mousemove', onDrag)
  window.removeEventListener('mouseup', endDrag)
  window.removeEventListener('touchmove', onDrag)
  window.removeEventListener('touchend', endDrag)
}
onBeforeUnmount(endDrag)

let refreshTimer: number | null = null
onMounted(async () => {
  // 先挂尺寸监听（首次回调给真实尺寸），再去拿区域数据 —— 数据一到就能按正确尺寸出图
  attachBoxObserver()
  try {
    await bindWorldData()
  } catch {
    /* 静默 */
  }
  await loadRegion()
  await loadRoles()
  // 每 5 分钟刷一次画面（时间/角色位置会变），避免频繁请求
  refreshTimer = window.setInterval(() => {
    if (state.value.mode !== 'off') {
      tick.value = Date.now()
      void loadRoles()
    }
  }, 5 * 60 * 1000)
})
onBeforeUnmount(() => {
  detachBoxObserver()
  if (refreshTimer !== null) window.clearInterval(refreshTimer)
})
</script>

<style scoped>
/* 背景层：铺满 + 半透明 + 不吃事件 */
.wml-overlay {
  position: fixed;
  inset: 0;
  z-index: 1;
  pointer-events: none;
  overflow: hidden;
  transition: opacity 0.4s ease;
}
.wml-overlay .wml-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  filter: saturate(0.9) contrast(1.05);
}
.wml-veil {
  position: absolute;
  inset: 0;
  background: radial-gradient(circle at 50% 40%, rgba(13, 22, 32, 0.1), rgba(13, 22, 32, 0.75));
}
.wml-hint {
  position: absolute;
  left: 50%;
  bottom: 18px;
  transform: translateX(-50%);
  font-size: 12px;
  color: #9fd6ff;
  background: rgba(13, 22, 32, 0.8);
  border-radius: 10px;
  padding: 5px 12px;
}
.wml-hint.sm {
  bottom: 50%;
  transform: translate(-50%, 50%);
}

/* 角落小窗 */
.wml-corner {
  position: fixed;
  right: 16px;
  bottom: 84px;
  width: 232px;
  z-index: 60;
  background: rgba(13, 22, 32, 0.92);
  border: 1px solid rgba(121, 217, 255, 0.3);
  border-radius: 13px;
  overflow: hidden;
  box-shadow: 0 8px 26px rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(8px);
  cursor: move;
  user-select: none;
}
.wml-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 9px;
  font-size: 12px;
  color: #dff1ff;
  border-bottom: 1px solid rgba(121, 217, 255, 0.18);
}
.wml-title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.wml-ops {
  display: flex;
  gap: 4px;
}
.wml-op {
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(121, 217, 255, 0.2);
  border-radius: 7px;
  color: #cfe6ff;
  font-size: 11px;
  padding: 2px 6px;
  cursor: pointer;
}
.wml-op:hover {
  background: rgba(121, 217, 255, 0.22);
}
.wml-body {
  position: relative;
  aspect-ratio: 4 / 3;
  background: #0a121b;
}
.wml-img2 {
  width: 100%;
  height: 100%;
  object-fit: contain;
  display: block;
}
.wml-foot {
  display: flex;
  gap: 9px;
  padding: 5px 9px;
  font-size: 10.5px;
  color: rgba(180, 205, 235, 0.8);
  flex-wrap: wrap;
}
</style>
