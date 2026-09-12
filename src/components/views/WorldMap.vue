<template>
  <div class="wm-root">
    <!-- 顶栏 -->
    <div class="wm-top">
      <button class="wm-btn" title="返回主菜单" @click="goBack">← 返回</button>
      <span class="wm-title">🧭 {{ mainName }}</span>
      <span v-if="radiusKm" class="wm-tag">范围 {{ radiusKm }} km</span>
      <span v-if="nowText" class="wm-tag">{{ nowText }}</span>
      <span v-if="weatherText" class="wm-tag">{{ weatherText }}</span>
      <span class="wm-spacer" />
      <select v-model="style" class="wm-sel" @change="reload">
        <option value="gaode">高德</option>
        <option value="dark">暗色</option>
        <option value="water">水系</option>
      </select>
      <button class="wm-btn" :disabled="loading" @click="reload">↻ 刷新</button>
      <button class="wm-btn" :disabled="loading" @click="relocate">📍 按定位</button>
      <button class="wm-btn" title="把这张地图叠到聊天界面上" @click="toOverlay">▤ 叠加为背景</button>
      <button class="wm-btn" title="缩成右下角小窗" @click="toCorner">▢ 角落小窗</button>
      <!-- 子页面入口：与其把主菜单塞满，不如从地图页进 -->
      <span class="wm-spacer" />
      <button class="wm-btn" title="看 AI 逐栋实时画出这个小区" @click="router.push('/world/district-live')">✎ 实时绘制</button>
      <button class="wm-btn" title="数据可视化 / 图层开关 / 伪 3D" @click="router.push('/world/district-viz')">▦ 可视化</button>
      <button class="wm-btn" title="地图库：缓存的地图与布局" @click="router.push('/world/maplib')">🗂 地图库</button>
      <button class="wm-btn" title="手机悬浮窗形态预览" @click="router.push('/world/phone-overlay')">📱 手机窗</button>
    </div>

    <!-- 地图舞台 -->
    <div class="wm-wrap">
      <div ref="stageEl" class="wm-stage" :style="stageStyle">
        <img class="wm-img" :src="mainImg" alt="世界地图" @error="onImgError" />
        <!-- 天气/时间叠加层 -->
        <canvas ref="fxCanvas" class="wm-fx" />
        <!-- 远处区块指示器 -->
        <div
          v-for="(r, i) in remotes"
          :key="r.adcode"
          class="wm-ind"
          :class="{ sel: selIndex === i }"
          :style="indStyle(i)"
          :title="`${r.name} · ${r.dir}方向 ${r.distance_km}km`"
          @click="openRemote(i)"
        >
          <span class="wm-ar" :style="{ transform: `rotate(${r.bearing}deg)` }">↑</span>
          <span class="wm-nm">{{ r.name }}</span>
          <span class="wm-ds">{{ r.dir }} {{ r.distance_km }}km</span>
        </div>
        <!-- 角色点（按日程落在地图上） -->
        <div
          v-for="(a, i) in actorDots"
          :key="'a' + i"
          class="wm-actor"
          :style="{ left: a.x + '%', top: a.y + '%' }"
          :title="`${a.name}：${a.content}`"
        >
          <span class="wm-dot" /><span class="wm-aname">{{ a.name }}</span>
        </div>
        <div v-if="loading" class="wm-load">加载中…（首次拼接大图较慢）</div>
        <div v-if="err" class="wm-err">{{ err }}</div>
      </div>
    </div>

    <!-- 角色此刻状态 -->
    <div v-if="roles.length" class="wm-roles">
      <div v-for="r in roles" :key="r.name" class="wm-role">
        <span class="wm-rname">{{ r.name }}</span>
        <span class="wm-rnow">{{ r.now ? r.now.content : '—' }}</span>
        <span class="wm-rplace">{{ placeLabel(r) }}</span>
        <span class="wm-rbar"><i :style="{ width: Math.round(r.progress * 100) + '%' }" /></span>
      </div>
    </div>

    <!-- 远处区块预览 -->
    <div v-if="selIndex >= 0" class="wm-lb" @click.self="selIndex = -1">
      <img v-if="lbImg" :src="lbImg" alt="" />
      <div class="wm-lbcap">
        {{ remotes[selIndex].name }} · 主块{{ remotes[selIndex].dir }}方向
        {{ remotes[selIndex].distance_km }} km
      </div>
      <div class="wm-lbops">
        <button class="wm-btn" @click="makeMain(selIndex)">设为主块</button>
        <button class="wm-btn" @click="selIndex = -1">关闭</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import worldMapApi, {
  MAP_SVG_DEFAULT_H,
  MAP_SVG_DEFAULT_W,
  mapSvgUrl,
  type BlocksPayload,
  type RemoteBlock,
  type ScheduleRole,
} from '@/api/services/worldMap'
import { useWorldMapLayer } from '@/composables/useWorldMapLayer'
import { bindWorldData } from '@/composables/useWorldMapBindings'

const router = useRouter()

const loading = ref(false)
const err = ref('')
const style = ref('gaode')
const data = ref<BlocksPayload | null>(null)
const roles = ref<ScheduleRole[]>([])
const selIndex = ref(-1)
const fxCanvas = ref<HTMLCanvasElement | null>(null)

let fxTimer: number | null = null
const nowText = ref('')
const weatherText = ref('')

const mainName = computed(() => data.value?.main?.name || '世界地图')
const radiusKm = computed(() => data.value?.main?.radius_km ?? 0)
const remotes = computed<RemoteBlock[]>(() => data.value?.remotes || [])

const stageEl = ref<HTMLElement | null>(null)
const imgTick = ref(Date.now())
const stageStyle = computed(() => ({ aspectRatio: '4 / 3' }))

/** 主图（data URL / HTTP 地址）：由 refreshMainImg() 异步取回，Tauri 下没有 HTTP 服务可拼 */
const mainImg = ref('')
/** 远处区块大图（点指示器弹的那张），同样是异步取回 */
const lbImg = ref('')

/**
 * 地图舞台的**真实 CSS 像素**尺寸。
 *
 * 为什么非要它：SVG 里的字号是**固定 px**（Rust 侧 render_geo.rs 的 level_style：
 * 全国 13.5 / 省 12 / 市 11 / 区县 10.5，再按各区面积占比乘 0.75~1.6，实测最小 8.2px），
 * 跟画布多大无关。后端默认按 1000×760 画，手机上被缩到 ~360px 宽显示时，
 * 字号实际只剩 3~4px，地名全看不清；传真实尺寸后画布与显示尺寸 1:1，
 * 字号是多少就显示多少（实测 8.2~16px，等于放大了约 2.7 倍）。
 * 0 表示「还没量到」（首帧布局未完成），此时先不请求，等 ResizeObserver 回调。
 */
const stageSize = ref({ w: 0, h: 0 })

const IMG_ERR = '主图还没生成好，点「刷新」重试（首次拼接较慢）'

/** 请求序号：尺寸/风格连点时会并发多个请求，只有**最后发出的那个**允许写回（防乱序覆盖） */
let imgSeq = 0
let lbSeq = 0

/**
 * 取主图。失败不抛给全局：把 src 置空 + 走页面已有的错误提示（onImgError），绝不白屏。
 * @param useDefaultSize 尺寸一直量不到时的兜底（按默认画布先画一版，总比空舞台强）
 */
async function refreshMainImg(useDefaultSize = false) {
  const m = data.value?.main
  if (!m) {
    mainImg.value = ''
    return
  }
  let w = stageSize.value.w
  let h = stageSize.value.h
  if (w <= 0 || h <= 0) {
    // 还没量到尺寸就先不画：等 ResizeObserver 第一次回调再画，避免先用兜底尺寸白画一版
    if (!useDefaultSize) return
    w = MAP_SVG_DEFAULT_W
    h = MAP_SVG_DEFAULT_H
  }
  const seq = ++imgSeq
  const ad = m.adcode
  try {
    const url = await mapSvgUrl(ad, style.value, w, h)
    if (seq !== imgSeq) return
    mainImg.value = url
    // 上一轮失败留下的提示，这轮成功了要收掉（不清别的错误：那是 blocks/定位 的事）
    if (err.value === IMG_ERR) err.value = ''
  } catch {
    if (seq !== imgSeq) return
    mainImg.value = ''
    onImgError()
  }
}

/** 取「远处区块」的大图（原来拼的是 /api/map，那条路由在 Rust 服务上不存在，必然 404） */
async function refreshLbImg() {
  const r = remotes.value[selIndex.value]
  if (!r) {
    lbImg.value = ''
    return
  }
  const seq = ++lbSeq
  try {
    const w = stageSize.value.w || MAP_SVG_DEFAULT_W
    const h = stageSize.value.h || MAP_SVG_DEFAULT_H
    const url = await mapSvgUrl(r.adcode, style.value, w, h)
    if (seq === lbSeq) lbImg.value = url
  } catch {
    if (seq === lbSeq) lbImg.value = ''
  }
}

/**
 * 影响主图的全部输入合成一个 key：区域 / 风格 / 刷新计数 / 舞台尺寸。
 * 用 key 而不是逐个 watch，是为了让「同一次操作引起的多处变化」只触发一次请求。
 */
const imgKey = computed(() => {
  const ad = data.value?.main?.adcode || ''
  return `${ad}|${style.value}|${imgTick.value}|${stageSize.value.w}x${stageSize.value.h}`
})
watch(imgKey, () => {
  void refreshMainImg()
})

const lbKey = computed(() => `${selIndex.value}|${style.value}`)
watch(lbKey, () => {
  void refreshLbImg()
})

// ── 舞台尺寸监听 ──
let stageRo: ResizeObserver | null = null
let stageTimer: number | null = null
let fallbackTimer: number | null = null
let stageRoFired = false

function measureStage() {
  const el = stageEl.value
  if (!el) return
  const r = el.getBoundingClientRect()
  const w = Math.round(r.width)
  const h = Math.round(r.height)
  const cur = stageSize.value
  // 抖动过滤：变化不到 8px 不重画（拖窗口/转屏时 ResizeObserver 会连发几十次）
  if (Math.abs(w - cur.w) < 8 && Math.abs(h - cur.h) < 8) return
  stageSize.value = { w, h }
}

/** 兜底：尺寸一直量不到（老 WebView 的 RO 不回调 / 元素被隐藏）也要出图 —— 宁可字小，也别空着舞台 */
function armSizeFallback() {
  if (fallbackTimer !== null) return
  fallbackTimer = window.setTimeout(() => {
    fallbackTimer = null
    if (stageSize.value.w <= 0) void refreshMainImg(true)
  }, 1500)
}

function bindStageObserver() {
  armSizeFallback()
  const el = stageEl.value
  if (!el) return // 理论上 onMounted 时 DOM 已就绪；真为空就交给上面的兜底
  if (typeof ResizeObserver === 'undefined') {
    // 老 WebView（Chromium < 64，即很旧的 Android System WebView）没有 ResizeObserver：
    // 退回「量一次 + 监听 window.resize」，不能因为一个 API 缺失就让地图永远不画
    measureStage()
    window.addEventListener('resize', measureStage)
    return
  }
  stageRo = new ResizeObserver(() => {
    // 首次回调 = 布局已就绪，立刻量（别等防抖，否则首屏要多等 200ms）
    if (!stageRoFired) {
      stageRoFired = true
      measureStage()
      return
    }
    if (stageTimer !== null) window.clearTimeout(stageTimer)
    stageTimer = window.setTimeout(measureStage, 220)
  })
  stageRo.observe(el)
}

function unbindStageObserver() {
  if (stageTimer !== null) {
    window.clearTimeout(stageTimer)
    stageTimer = null
  }
  // 兜底计时器也要清：组件都卸载了就别再发这一枪
  if (fallbackTimer !== null) {
    window.clearTimeout(fallbackTimer)
    fallbackTimer = null
  }
  window.removeEventListener('resize', measureStage)
  stageRo?.disconnect()
  stageRo = null
}

/** 指示器位置：后端给的边缘坐标 + 前端避让 */
const indPos = computed(() => {
  const placed: { x: number; y: number }[] = []
  return remotes.value.map((r) => {
    let x = r.edge.x
    let y = r.edge.y
    for (let k = 0; k < 8; k++) {
      let hit = false
      for (const p of placed) {
        if (Math.abs(p.x - x) < 0.13 && Math.abs(p.y - y) < 0.075) {
          hit = true
          if (y > 0.5) y = Math.min(0.965, y + 0.085)
          else y = Math.max(0.035, y - 0.085)
          if (y > 0.9 || y < 0.1) {
            if (x > 0.5) x = Math.max(0.06, x - 0.1)
            else x = Math.min(0.94, x + 0.1)
          }
          break
        }
      }
      if (!hit) break
    }
    placed.push({ x, y })
    return { x, y }
  })
})

function indStyle(i: number) {
  const p = indPos.value[i] || { x: 0.5, y: 0.5 }
  return { left: p.x * 100 + '%', top: p.y * 100 + '%' }
}

/** 角色点：按日程里的设施格子铺在地图上（没有格子时沿下方排开） */
const actorDots = computed(() => {
  const out: { name: string; x: number; y: number; content: string }[] = []
  roles.value.forEach((r, i) => {
    const g = r.now?.place?.grid
    if (Array.isArray(g) && g.length >= 2) {
      out.push({
        name: r.name,
        x: 8 + ((Number(g[0]) % 20) / 20) * 84,
        y: 8 + ((Number(g[1]) % 20) / 20) * 84,
        content: r.now?.content || '',
      })
    } else if (r.now?.place?.kind === 'transit') {
      out.push({ name: r.name, x: 50, y: 90 - i * 4, content: r.now?.content || '' })
    } else {
      out.push({ name: r.name, x: 12 + i * 9, y: 92, content: r.now?.content || '' })
    }
  })
  return out
})

function placeLabel(r: ScheduleRole) {
  const p = r.now?.place
  if (!p) return ''
  return p.label || p.name || p.kindZh || ''
}

function onImgError() {
  err.value = IMG_ERR
}

async function loadRoles() {
  try {
    const s = await worldMapApi.schedule()
    roles.value = s.roles || []
  } catch {
    roles.value = []
  }
}

async function loadTimeWeather() {
  try {
    const t: any = await worldMapApi.time()
    const hh = t?.hour ?? t?.clock?.hour
    const mm = t?.minute ?? t?.clock?.minute
    nowText.value = hh !== undefined ? `🕐 ${String(hh).padStart(2, '0')}:${String(mm ?? 0).padStart(2, '0')}` : ''
  } catch {
    nowText.value = ''
  }
  try {
    const w: any = await worldMapApi.weather()
    const cur = w?.current || w?.weather?.current || w
    const temp = cur?.temp_c ?? cur?.tempC ?? cur?.temp
    const desc = cur?.desc || cur?.weather_desc || cur?.description
    weatherText.value = temp !== undefined ? `🌤 ${desc ? desc + ' ' : ''}${temp}°C` : ''
    // 喂给特效层：startFx() 读 window.__WM_WEATHER__ 决定下不下雨/雪/雾。
    // 之前全仓只有"读"没有"写"，所以 T4-2 的粒子永远不会出现 —— 这一行就是那条断掉的线。
    ;(window as any).__WM_WEATHER__ = w || null
  } catch {
    weatherText.value = ''
    ;(window as any).__WM_WEATHER__ = null
  }
}

async function reload() {
  loading.value = true
  err.value = ''
  selIndex.value = -1
  try {
    data.value = await worldMapApi.blocks(undefined, style.value, 6)
    if (data.value && data.value.ok === false) err.value = data.value.error || '加载失败'
    else if (data.value?.hint) err.value = data.value.hint
  } catch (e: any) {
    err.value = '世界地图服务未启动：' + (e?.message || e)
  } finally {
    // 刷新计数放在 data 赋值之后（同一个 tick 内）：watch 只触发一次，
    // 否则会先用「旧区域 + 新计数」白白多渲染一张图
    imgTick.value = Date.now()
    loading.value = false
  }
}

async function relocate() {
  loading.value = true
  err.value = ''
  try {
    const loc = await worldMapApi.location({ force: true, fast: true })
    if (loc && !loc.error && loc.lat) {
      data.value = await worldMapApi.blocksByLatLng(loc.lat, loc.lng, style.value, 6)
    } else {
      // 定位不可用（真壳里 world_map_location 还没实现、HTTP 也不通）：
      // 提示必须写在 reload() **之后** —— reload() 开头会清空 err，写早了用户根本看不见
      const hint = loc?.hint || '定位失败，可稍后重试'
      await reload()
      if (!err.value) err.value = hint // 别盖掉 reload() 自己报的错（那个更要紧）
    }
  } catch (e: any) {
    err.value = '定位失败：' + (e?.message || e)
  } finally {
    loading.value = false
  }
}

function openRemote(i: number) {
  selIndex.value = i
}

async function makeMain(i: number) {
  const r = remotes.value[i]
  if (!r) return
  loading.value = true
  err.value = ''
  try {
    data.value = await worldMapApi.blocks(r.adcode, style.value, 6)
    imgTick.value = Date.now()
    selIndex.value = -1
  } catch (e: any) {
    err.value = '切换失败：' + (e?.message || e)
  } finally {
    loading.value = false
  }
}

function goBack() {
  router.push('/')
}

// 把当前区域带到叠加层，然后缩回主菜单，聊天时也能看到世界
const { setMode, setRegion } = useWorldMapLayer()
function toOverlay() {
  const m = data.value?.main
  if (m) setRegion(m.adcode, style.value)
  setMode('overlay')
  router.push('/')
}
function toCorner() {
  const m = data.value?.main
  if (m) setRegion(m.adcode, style.value)
  setMode('corner')
  router.push('/')
}

/** 天气粒子叠加（复用 world_weather.js） */
function startFx() {
  const loop = () => {
    const cv = fxCanvas.value
    const W = (window as any).WORLD_WEATHER
    if (!cv || !W) return
    const stage = cv.parentElement
    if (!stage) return
    const w = stage.clientWidth
    const h = stage.clientHeight
    if (cv.width !== w || cv.height !== h) {
      cv.width = w
      cv.height = h
    }
    const ctx = cv.getContext('2d')
    if (!ctx) return
    ctx.clearRect(0, 0, w, h)
    try {
      const st: any = (window as any).__WM_WEATHER__
      if (st && st.kind) W.drawWeather(ctx, w, h, 0.6, performance.now(), st)
    } catch {
      /* 天气模块失败不影响地图 */
    }
  }
  fxTimer = window.setInterval(loop, 1000 / 24)
}

onMounted(async () => {
  // 先挂尺寸监听：ResizeObserver 的首次回调会给出真实尺寸，主图按它来画（内含量不到尺寸的兜底）
  bindStageObserver()
  await reload()
  // T6-4：把 LingChat 的角色/日程接到地图与手机上（只有 LingChat 角色才带头像）
  try {
    await bindWorldData()
  } catch {
    /* 绑定失败不影响地图本身 */
  }
  await Promise.all([loadRoles(), loadTimeWeather()])
  const w = window as any
  if (!w.WORLD_WEATHER) {
    await new Promise<void>((resolve) => {
      const s = document.createElement('script')
      s.src = '/world_map/world_weather.js'
      s.onload = () => resolve()
      s.onerror = () => resolve()
      document.head.appendChild(s)
    })
  }
  startFx()
})

onBeforeUnmount(() => {
  unbindStageObserver()
  if (fxTimer !== null) window.clearInterval(fxTimer)
  fxTimer = null
})
</script>

<style scoped>
.wm-root {
  position: fixed;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: linear-gradient(160deg, #0d1620, #16273d);
  color: #eaf3ff;
  z-index: 50;
}
.wm-top {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: rgba(13, 22, 32, 0.9);
  border-bottom: 1px solid rgba(121, 217, 255, 0.2);
  flex-wrap: wrap;
  z-index: 2;
}
.wm-title {
  font-size: 15px;
  font-weight: 600;
}
.wm-tag {
  font-size: 11.5px;
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(121, 217, 255, 0.2);
  border-radius: 20px;
  padding: 3px 10px;
  color: #cfe6ff;
}
.wm-spacer {
  flex: 1;
}
.wm-btn,
.wm-sel {
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(121, 217, 255, 0.25);
  border-radius: 9px;
  padding: 6px 11px;
  color: #eaf3ff;
  font-size: 12.5px;
  cursor: pointer;
}
.wm-btn:hover:not(:disabled) {
  background: rgba(121, 217, 255, 0.18);
}
.wm-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.wm-wrap {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 10px;
  min-height: 0;
}
.wm-stage {
  position: relative;
  width: min(100%, calc((100vh - 150px) * 4 / 3));
  border-radius: 14px;
  box-shadow: 0 10px 34px rgba(0, 0, 0, 0.45);
}
.wm-img {
  width: 100%;
  height: 100%;
  display: block;
  border-radius: 14px;
  background: #0a121b;
  object-fit: contain;
  border: 1px solid rgba(121, 217, 255, 0.2);
}
.wm-fx {
  position: absolute;
  inset: 0;
  border-radius: 14px;
  pointer-events: none;
}
.wm-ind {
  position: absolute;
  transform: translate(-50%, -50%);
  background: rgba(16, 26, 40, 0.92);
  border: 1px solid rgba(121, 217, 255, 0.45);
  border-radius: 11px;
  padding: 5px 9px;
  font-size: 11.5px;
  cursor: pointer;
  white-space: nowrap;
  display: flex;
  align-items: center;
  gap: 6px;
  transition: 0.16s;
  z-index: 3;
}
.wm-ind:hover,
.wm-ind.sel {
  background: rgba(121, 217, 255, 0.28);
  border-color: #79d9ff;
}
.wm-ar {
  color: #79d9ff;
  font-size: 13px;
  display: inline-block;
}
.wm-nm {
  font-weight: 600;
}
.wm-ds {
  color: rgba(180, 205, 235, 0.8);
  font-size: 10.5px;
}
.wm-actor {
  position: absolute;
  transform: translate(-50%, -50%);
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 10.5px;
  pointer-events: none;
  z-index: 4;
}
.wm-dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: #ffd166;
  box-shadow: 0 0 0 3px rgba(255, 209, 102, 0.25);
}
.wm-aname {
  background: rgba(0, 0, 0, 0.5);
  border-radius: 6px;
  padding: 1px 5px;
}
.wm-load,
.wm-err {
  position: absolute;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  background: rgba(13, 22, 32, 0.92);
  border: 1px solid rgba(121, 217, 255, 0.3);
  border-radius: 12px;
  padding: 10px 18px;
  font-size: 13px;
  z-index: 5;
}
.wm-err {
  top: auto;
  bottom: 12px;
  transform: translateX(-50%);
  border-color: rgba(255, 170, 120, 0.5);
  color: #ffe3c2;
  max-width: 90%;
}
.wm-roles {
  display: flex;
  gap: 10px;
  padding: 6px 12px 10px;
  overflow-x: auto;
  flex-wrap: wrap;
}
.wm-role {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 11.5px;
  background: rgba(255, 255, 255, 0.07);
  border: 1px solid rgba(121, 217, 255, 0.18);
  border-radius: 10px;
  padding: 5px 10px;
}
.wm-rname {
  font-weight: 600;
  color: #9fe0ff;
}
.wm-rplace {
  color: rgba(180, 205, 235, 0.75);
}
.wm-rbar {
  width: 46px;
  height: 4px;
  border-radius: 3px;
  background: rgba(255, 255, 255, 0.15);
  overflow: hidden;
}
.wm-rbar i {
  display: block;
  height: 100%;
  background: #79d9ff;
}
.wm-lb {
  position: fixed;
  inset: 0;
  background: rgba(6, 11, 18, 0.94);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 40px 16px 16px;
  z-index: 20;
}
.wm-lb img {
  max-width: 100%;
  max-height: 70vh;
  object-fit: contain;
  border-radius: 12px;
}
.wm-lbcap {
  font-size: 13px;
  color: #bfe4ff;
}
.wm-lbops {
  display: flex;
  gap: 10px;
}
</style>
