<!--
  AI 实时绘制小区（移植自 Python 原型的 district_live.html）

  和后端别的接口最大的不同：**这个页面看的是「过程」而不是「结果」**。
  数据统一从 startDistrictStream() 来：应用内走 Tauri 命令 + Channel（APK 里没有
  127.0.0.1:8791 那个 Rust 调试服务，EventSource 必然连不上），浏览器里退回 SSE；
  两条路的事件形状逐字段一致，AI 每吐出一栋楼就立刻画一栋，
  所以这里的核心约束是「增量」：绝不能每来一条事件就重渲染整张 SVG。

  增量怎么做的（对应下面 appendBuilding/appendRoad/appendArea 的调用点）：
    · 页面只建一次舞台（districtDraw.createStage），拿到 4 个分层 <g>；
    · 每条 building/road/park/water 事件 → 现场 createElementNS 造**这一个**节点，
      直接 append 进对应分层 —— 已画的节点不碰、不重建，生长动画也就能正常播；
    · 唯一会全量重画的情况：网格规模中途变了（旧坐标按旧格子算，不重画会错位），
      以及 done 时的对账（后端说画了 30 栋、我们只收到 28 条 → 用完整布局补齐）。
-->
<template>
  <div class="dl-root">
    <!-- 顶部状态条 -->
    <div class="dl-top">
      <button class="dl-btn" title="回到世界地图" @click="goBack">← 返回</button>
      <span class="dl-title">🎨 {{ title }}</span>
      <span class="dl-tag">已画 <b>{{ counts.buildings }}</b> 栋</span>
      <span v-if="counts.roads" class="dl-tag">路 <b>{{ counts.roads }}</b></span>
      <span v-if="counts.parks" class="dl-tag">绿 <b>{{ counts.parks }}</b></span>
      <span v-if="counts.water" class="dl-tag">水 <b>{{ counts.water }}</b></span>
      <span class="dl-tag">用时 <b>{{ elapsedText }}</b>s</span>
      <span class="dl-tag" :class="phaseClass">{{ phase }}</span>
      <span class="dl-spacer" />
      <input
        v-model="area"
        class="dl-inp"
        placeholder="区域，如 广州市·越秀区"
        :disabled="running"
        @keyup.enter="start"
      />
      <button v-for="p in PRESETS" :key="p" class="dl-btn tiny" :disabled="running" :title="p" @click="area = p">
        {{ p.split('·')[1] || p }}
      </button>
      <button class="dl-btn" :disabled="running" title="用设备定位反推区域名" @click="locate">📍 定位</button>
      <input v-model="context" class="dl-inp ctx" placeholder="剧情 / 风格提示（可选）" :disabled="running" />
      <select v-model.number="expand" class="dl-sel" :disabled="running" title="规模档位（后端 20 + expand×8）">
        <option :value="0">紧凑 20×20</option>
        <option :value="1">标准 28×28</option>
        <option :value="2">大型 36×36</option>
      </select>
      <button class="dl-btn" @click="logOpen = !logOpen">{{ logOpen ? '收起日志' : '📜 绘制日志' }}</button>
      <button class="dl-btn primary" :disabled="running" @click="start">▶ 开始绘制</button>
      <button v-if="running" class="dl-btn" @click="stop">■ 停止</button>
    </div>

    <div class="dl-progress"><i :style="{ width: progress + '%' }" /></div>

    <div class="dl-wrap">
      <div class="dl-stagebox">
        <!-- 画布挂载点：里面的 SVG 完全由 districtDraw 用原生 DOM 增量构建 -->
        <div ref="stageHost" class="dl-stage" @click="onStageClick" />
        <div v-if="err" class="dl-alert err">{{ err }}</div>
        <div v-else-if="emptyHint" class="dl-alert warn">{{ emptyHint }}</div>
        <div v-else-if="!started" class="dl-alert hint">点「▶ 开始绘制」，看 AI 一栋栋把小区画出来</div>
      </div>

      <!-- 绘制日志 -->
      <div v-if="logOpen" class="dl-log">
        <div class="dl-loghead">📜 绘制日志</div>
        <div class="dl-logbody">
          <div v-for="l in logs" :key="l.id" class="dl-e" :class="l.cls">
            <span class="dl-t">{{ l.t }}</span>{{ l.text }}
          </div>
        </div>
      </div>

      <!-- 选中建筑的信息面板 -->
      <div v-if="sel" class="dl-panel">
        <h3>{{ sel.name }}</h3>
        <div><span class="k">类型：</span>{{ sel.typeZh }}</div>
        <div><span class="k">楼层：</span>{{ sel.floors ? sel.floors + ' 层' : '—' }}</div>
        <div><span class="k">占地：</span>{{ sel.w || '—' }} × {{ sel.h || '—' }} 格</div>
        <button class="dl-btn" @click="sel = null">关闭</button>
      </div>
    </div>

    <!-- 底栏 -->
    <div class="dl-foot">
      <span class="dl-status" v-html="statusText" />
      <span class="dl-spacer" />
      <button class="dl-btn" :disabled="!hasSvg" title="把当前画面导出成 SVG 文件" @click="exportSvg">⬇ 导出 SVG</button>
      <button
        class="dl-btn"
        :disabled="counts.buildings === 0"
        title="把这次生成的布局存进地图库"
        @click="saveToLibrary"
      >
        💾 保存到地图库
      </button>
    </div>

    <!-- 同组页面导航（不改主菜单，先给个入口） -->
    <div class="dl-nav">
      <span class="dl-navtip">世界地图扩展</span>
      <button class="dl-btn tiny" @click="go('/world/district-live')">🎨 实时绘制</button>
      <button class="dl-btn tiny" @click="go('/world/district-viz')">📊 数据 / 图层</button>
      <button class="dl-btn tiny" @click="go('/world/maplib')">🗂 地图库</button>
      <button class="dl-btn tiny" @click="go('/world/phone-overlay')">📱 悬浮窗</button>
    </div>

    <div v-if="toastText" class="dl-toast">{{ toastText }}</div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import worldMapApi, {
  startDistrictStream,
  type DistrictItem,
  type DistrictLayout,
  type DistrictRoadItem,
  type DistrictStreamEvent,
} from '@/api/services/worldMap'
import {
  TYPE_ZH,
  appendArea,
  appendBuilding,
  appendRoad,
  createStage,
  makeScale,
  replayLayout,
  serializeStage,
  setInfo,
  setTitle,
  type Scale,
  type Stage,
} from './districtDraw'

const router = useRouter()

/** 预设区域：后端只用区域名写提示词，所以这里给「市·区」就够 */
const PRESETS = ['广州市·越秀区', '广州市·天河区', '深圳市·南山区', '上海市·黄浦区', '北京市·朝阳区']

const area = ref('广州市·越秀区')
const context = ref('')
/** 0=20×20 / 1=28×28 / 2=36×36（后端 base = 20 + expand*8） */
const expand = ref(0)

const running = ref(false)
const started = ref(false)
const logOpen = ref(true)
const err = ref('')
const emptyHint = ref('')
const phase = ref('待启动')
const elapsed = ref(0)
const toastText = ref('')
const title = ref('AI 绘制小区')
const statusText = ref('点「开始绘制」，看 AI 一栋栋把小区画出来')
const counts = reactive({ buildings: 0, roads: 0, parks: 0, water: 0 })
const logs = ref<{ id: number; cls: string; t: string; text: string }[]>([])
const sel = ref<{ name: string; typeZh: string; floors: string; w: string; h: string } | null>(null)
const hasSvg = ref(false)

const stageHost = ref<HTMLElement | null>(null)
let stage: Stage | null = null
let scale: Scale = makeScale(20)
/**
 * 停止函数（startDistrictStream 返回，幂等）。
 * 不管这条流是 Tauri Channel 还是 EventSource，页面只认这一个句柄：
 * 重画/卸载/点停止都必须调它，否则后台那一轮生成会一直跑完。
 */
let stopStream: (() => void) | null = null
let tick: number | null = null
let startedAt = 0
/** done 之后不再重复收尾（[DONE] 与 onerror 都会走到 finish） */
let settled = false
let evCount = 0
let logSeq = 0
/** 已收到的元素（对账 + 网格变化时重画用） */
let layout: Required<Pick<DistrictLayout, 'buildings' | 'roads' | 'parks' | 'water'>> = {
  buildings: [],
  roads: [],
  parks: [],
  water: [],
}
/** 后端 done 里报的最终耗时（本地秒表只是过程显示） */
let serverElapsed: number | null = null

const elapsedText = computed(() => (serverElapsed ?? elapsed.value).toFixed(1))
const progress = computed(() => {
  if (running.value) return Math.min(92, counts.buildings * 4)
  return started.value ? 100 : 0
})
const phaseClass = computed(() => ({
  spin: running.value,
  ok: phase.value === '完成',
  bad: phase.value === '出错' || phase.value === '中断',
}))

function toast(msg: string, ms = 3200) {
  toastText.value = msg
  window.setTimeout(() => {
    if (toastText.value === msg) toastText.value = ''
  }, ms)
}

function nowText(): string {
  return startedAt ? ((Date.now() - startedAt) / 1000).toFixed(1) : '0.0'
}

/** 日志行：为什么存数组而不是直接操作 DOM —— 组件销毁时能跟着一起消失，不会留孤儿节点 */
function pushLog(cls: string, text: string) {
  logs.value.push({ id: ++logSeq, cls, t: `${nowText()}s`, text })
  if (logs.value.length > 300) logs.value.splice(0, logs.value.length - 300)
}

function go(path: string) {
  router.push(path)
}

function goBack() {
  router.push('/world')
}

function stopTimer() {
  if (tick !== null) window.clearInterval(tick)
  tick = null
}

function closeStream() {
  if (stopStream) {
    try {
      stopStream()
    } catch {
      /* 已经断了就算了 */
    }
    stopStream = null
  }
}

function reset() {
  closeStream()
  stopTimer()
  settled = false
  evCount = 0
  serverElapsed = null
  logs.value = []
  counts.buildings = 0
  counts.roads = 0
  counts.parks = 0
  counts.water = 0
  layout = { buildings: [], roads: [], parks: [], water: [] }
  err.value = ''
  emptyHint.value = ''
  sel.value = null
  hasSvg.value = false
  elapsed.value = 0
  title.value = 'AI 绘制小区'
}

/** 开始绘制：先清场、建舞台，再开流（应用内 = Tauri Channel，浏览器 = SSE） */
function start() {
  if (!stageHost.value) return
  reset()
  started.value = true
  running.value = true
  phase.value = '连接中…'
  startedAt = Date.now()
  scale = makeScale(20 + expand.value * 8)
  stage = createStage(stageHost.value, '绘制中…')
  setInfo(stage, '')
  hasSvg.value = true
  statusText.value = '<span class="spin"></span>正在连接 AI…'
  pushLog('', `请求 ${area.value}（${scale.size}×${scale.size}）`)

  tick = window.setInterval(() => {
    elapsed.value = (Date.now() - startedAt) / 1000
  }, 200)

  // 事件处理与通路无关：onEvent 收流事件、onDone 正常收尾、onError 给可读原因。
  // 打开连接失败时 onError 会被**同步**调用（此时 stopStream 还没赋值），
  // 所以这里不用 try/catch —— 失败路径由 onStreamError → fail() 统一负责。
  stopStream = startDistrictStream(
    { area: area.value, context: context.value.trim(), expand: expand.value },
    handleEvent,
    () => finish(false),
    (msg) => onStreamError(msg),
  )
}

/** 单条流事件（JSON 已经由数据层解析好；[DONE] 走 onDone） */
function handleEvent(ev: DistrictStreamEvent) {
  evCount++
  const st = stage
  if (!st) return

  switch (ev.type) {
    case 'start': {
      phase.value = 'AI 思考中'
      if (ev.size && ev.size !== scale.size) {
        scale = makeScale(ev.size)
        replayLayout(st, layout, scale)
      }
      pushLog('', `开始 · 网格 ${ev.size}×${ev.size}${ev.model ? ' · ' + ev.model : ''}`)
      statusText.value = `<span class="spin"></span>AI 正在构思（${ev.area || area.value}）…`
      break
    }
    case 'meta': {
      if (ev.name) {
        title.value = ev.name
        setTitle(st, ev.name)
      }
      pushLog('g', `小区名：${ev.name || '（未命名）'}`)
      break
    }
    case 'size': {
      const s = Number(ev.size) || 0
      if (s && s !== scale.size) {
        const drawn = counts.buildings + counts.roads + counts.parks + counts.water
        scale = makeScale(s)
        if (drawn > 0) {
          // 已经有元素了：旧坐标按旧格子算的，必须整体重画才不会错位
          replayLayout(st, layout, scale)
          pushLog('', `网格调整为 ${s}×${s}（已按新格距重画）`)
        } else {
          pushLog('', `规模：${s}×${s}`)
        }
      }
      break
    }
    case 'building': {
      const item = (ev.item || {}) as DistrictItem
      counts.buildings++
      layout.buildings.push(item)
      appendBuilding(st, item, scale)
      phase.value = '正在画建筑'
      pushLog('b', `🏠 ${item.name || '(未命名)'} +${nowText()}s`)
      statusText.value = `<span class="spin"></span>已画 <b>${counts.buildings}</b> 栋…`
      break
    }
    case 'road': {
      const item = (ev.item || {}) as DistrictRoadItem
      counts.roads++
      layout.roads.push(item)
      appendRoad(st, item, scale)
      pushLog('r', `🛣 ${item.name || '(无名路)'}`)
      break
    }
    case 'park':
    case 'water': {
      const item = (ev.item || {}) as DistrictItem
      if (ev.type === 'park') {
        counts.parks++
        layout.parks.push(item)
        appendArea(st, item, 'park', scale)
        pushLog('g', `🌳 ${item.name || '绿地'}`)
      } else {
        counts.water++
        layout.water.push(item)
        appendArea(st, item, 'water', scale)
        pushLog('g', `💧 ${item.name || '水面'}`)
      }
      break
    }
    case 'warn': {
      pushLog('', `⚠️ ${ev.message || ''}`)
      break
    }
    case 'debug': {
      const s = ev.stats || {}
      pushLog('', `流统计：${s.chunks ?? 0} 块 / ${s.chars ?? 0} 字符 / ${s.lines ?? 0} 行`)
      break
    }
    case 'done': {
      serverElapsed = typeof ev.elapsed === 'number' ? ev.elapsed : null
      reconcile(st, ev)
      finish(false)
      break
    }
    case 'error': {
      pushLog('', `❌ ${ev.message || '生成失败'}`)
      fail(ev.message || '生成失败')
      break
    }
    default: {
      pushLog('', `（未知事件 ${String((ev as { type?: string }).type)}）`)
    }
  }
}

/**
 * done 对账：后端给的 counts 是权威值。
 * 我们按事件逐条累加，正常情况两者相等；不等就说明有事件丢了
 * （网络抖动 / 解析失败），此时用 done.layout 这份完整布局重画一次，保证图和数据一致。
 */
function reconcile(st: Stage, ev: DistrictStreamEvent) {
  const c = ev.counts
  const mine = counts.buildings + counts.roads + counts.parks + counts.water
  const theirs = c ? c.buildings + c.roads + c.parks + c.water : 0
  if (ev.layout) {
    layout = {
      buildings: ev.layout.buildings || layout.buildings,
      roads: ev.layout.roads || layout.roads,
      parks: ev.layout.parks || layout.parks,
      water: ev.layout.water || layout.water,
    }
    if (ev.layout.name) {
      title.value = ev.layout.name
      setTitle(st, ev.layout.name)
    }
  }
  if (c && theirs !== mine) {
    pushLog('', `对账：本地收到 ${mine} 个元素，后端报告 ${theirs} 个 → 按后端完整布局重画`)
    replayLayout(st, layout, scale)
    counts.buildings = c.buildings
    counts.roads = c.roads
    counts.parks = c.parks
    counts.water = c.water
  }
}

function finish(isErr: boolean) {
  if (settled) return
  settled = true
  running.value = false
  stopTimer()
  closeStream()
  elapsed.value = serverElapsed ?? (Date.now() - startedAt) / 1000
  const total = counts.buildings + counts.roads + counts.parks + counts.water

  if (stage) setInfo(stage, `${counts.buildings} 栋 · ${counts.roads} 路 · ${elapsedText.value}s`)

  if (isErr) {
    phase.value = '中断'
    statusText.value = total
      ? `⚠️ 连接中断，已保留画好的 <b>${total}</b> 个元素`
      : '⚠️ 连接中断，什么都没画出来'
    pushLog('', '⚠️ 连接中断')
    return
  }

  phase.value = '完成'
  if (total === 0) {
    // 真实踩到过：流正常 done、但一条元素都没解析出来（模型输出格式漂移）
    emptyHint.value = '这一轮 AI 没吐出可解析的元素（流正常结束）。可直接点「开始绘制」重试，或换个区域/提示词。'
    statusText.value = '⚠️ 完成，但没有解析出任何元素 —— 建议重试'
    pushLog('', '⚠️ 完成但元素数为 0，建议重试')
    toast('AI 没画出可解析的元素，建议重试')
    return
  }
  emptyHint.value = ''
  statusText.value = `✅ 画完了 · <b>${counts.buildings}</b> 栋 · <b>${elapsedText.value}s</b>`
  pushLog('g', `✅ 完成：${counts.buildings} 栋建筑 / ${counts.roads} 条路 / ${counts.parks} 绿地 / ${counts.water} 水面`)
  toast(`✅ 完成 ${counts.buildings} 栋，用时 ${elapsedText.value}s`)
}

/** 出错：给可读提示，绝不白屏 */
function fail(msg: string) {
  err.value = `生成失败：${msg}`
  phase.value = '出错'
  running.value = false
  stopTimer()
  closeStream()
  settled = true
  statusText.value = '⚠️ 出错了，可改区域或稍后重试'
  toast(`生成失败：${msg}`)
}

/**
 * 流出错回调（数据层已经把「为什么」翻译成一句人话了，见 worldMap.ts）。
 *
 * 这里只做「按已收到的量分流」：
 *   · 一条都没收到 → 这一轮白开了，直接给出原因（含「后端未配置 LLM」那条 JSON 提示）；
 *   · 收到过元素 → 当作中断收尾，保留已经画出来的部分。
 * 主动 close/停止之后迟到的错误不再处理（数据层也会拦掉）。
 */
function onStreamError(msg: string) {
  if (settled) return
  const received = evCount
  closeStream()
  if (received > 0) {
    finish(true)
    return
  }
  running.value = false
  stopTimer()
  fail(msg)
}

function stop() {
  if (!running.value) return
  pushLog('', '■ 用户手动停止')
  finish(true)
}

/** 点画布上的建筑 → 信息面板（事件委托：增量画的节点没法逐个绑监听） */
function onStageClick(e: MouseEvent) {
  const target = e.target as Element | null
  const g = target?.closest?.('.wm-bld') as SVGGElement | null
  if (!g || !stage) {
    sel.value = null
    return
  }
  stage.svg.querySelectorAll('.wm-bld.on').forEach((n) => n.classList.remove('on'))
  g.classList.add('on')
  const d = g.dataset
  sel.value = {
    name: d.name || '（未命名）',
    typeZh: TYPE_ZH[d.type || ''] || d.type || '其它',
    floors: d.floors || '',
    w: d.w || '',
    h: d.h || '',
  }
}

function exportSvg() {
  if (!stage) return
  try {
    const text = serializeStage(stage)
    const blob = new Blob([text], { type: 'image/svg+xml;charset=utf-8' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `${title.value || 'district'}.svg`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    // 立刻 revoke 会让部分 WebView 下载失败，留一点时间
    window.setTimeout(() => URL.revokeObjectURL(url), 8000)
    toast('已导出 SVG')
  } catch (e) {
    toast(`导出失败：${(e as Error)?.message || e}`)
  }
}

function saveToLibrary() {
  // 后端还没有「存布局」的写接口（只有 /api/maplib 的读与清理），
  // 所以这里如实告诉用户「记录下来了，等接口」，不做假成功。
  pushLog('g', `💾 本次布局已记录在页面内存（${counts.buildings} 栋），保存接口待接入`)
  toast('已记录本次布局，保存到地图库的接口待接入')
}

/** 定位 → 用行政区路径拼出「省·市·区」当区域名（后端只吃字符串） */
async function locate() {
  statusText.value = '<span class="spin"></span>定位中…'
  try {
    const d = await worldMapApi.location({ fast: true })
    if (d?.error) throw new Error(d.hint || d.error)
    const names = (d?.path || []).map((p) => p.name).filter(Boolean)
    if (!names.length) throw new Error('定位结果没有行政区信息')
    // 只保留最后两级（市·区），太长的前缀对提示词没帮助
    area.value = names.slice(-2).join('·')
    statusText.value = `📍 已定位：${names.join(' · ')}`
    toast(`已切到 ${area.value}`)
  } catch (e) {
    statusText.value = '⚠️ 定位失败，可手动填区域名'
    toast(`定位失败：${(e as Error)?.message || e}`)
  }
}

onBeforeUnmount(() => {
  closeStream()
  stopTimer()
})
</script>

<style scoped>
.dl-root {
  position: fixed;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: linear-gradient(160deg, #0d1620, #16273d);
  color: #eaf3ff;
  z-index: 50;
}
.dl-top {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 7px 11px;
  background: rgba(13, 22, 32, 0.92);
  border-bottom: 1px solid rgba(121, 217, 255, 0.22);
  flex-wrap: wrap;
  z-index: 10;
}
.dl-title {
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
}
.dl-tag {
  font-size: 11.5px;
  background: rgba(255, 255, 255, 0.07);
  border: 1px solid rgba(121, 217, 255, 0.22);
  border-radius: 20px;
  padding: 3px 10px;
  color: #cfe6ff;
  white-space: nowrap;
}
.dl-tag b {
  color: #79d9ff;
}
.dl-tag.spin::before {
  content: '';
  display: inline-block;
  width: 9px;
  height: 9px;
  margin-right: 5px;
  border: 2px solid rgba(121, 217, 255, 0.3);
  border-top-color: #79d9ff;
  border-radius: 50%;
  animation: dlsp 0.7s linear infinite;
  vertical-align: -1px;
}
@keyframes dlsp {
  to {
    transform: rotate(360deg);
  }
}
.dl-tag.ok {
  border-color: rgba(143, 214, 160, 0.6);
  color: #b8f0c8;
}
.dl-tag.bad {
  border-color: rgba(255, 170, 120, 0.6);
  color: #ffd7b0;
}
.dl-spacer {
  flex: 1;
}
.dl-btn,
.dl-sel,
.dl-inp {
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(121, 217, 255, 0.25);
  border-radius: 9px;
  padding: 6px 11px;
  color: #eaf3ff;
  font-size: 12.5px;
  cursor: pointer;
}
.dl-inp {
  cursor: text;
  min-width: 130px;
}
.dl-inp.ctx {
  min-width: 150px;
}
.dl-inp::placeholder {
  color: rgba(180, 205, 235, 0.55);
}
.dl-btn:hover:not(:disabled) {
  background: rgba(121, 217, 255, 0.18);
}
.dl-btn:disabled {
  opacity: 0.45;
  cursor: default;
}
.dl-btn.tiny {
  padding: 4px 8px;
  font-size: 11.5px;
}
.dl-btn.primary {
  background: rgba(121, 217, 255, 0.26);
  border-color: rgba(121, 217, 255, 0.5);
}
.dl-progress {
  height: 3px;
  background: rgba(255, 255, 255, 0.08);
  overflow: hidden;
}
.dl-progress i {
  display: block;
  height: 100%;
  background: linear-gradient(90deg, #79d9ff, #8fd6a0);
  transition: width 0.3s;
}
.dl-wrap {
  flex: 1;
  display: flex;
  min-height: 0;
}
.dl-stagebox {
  flex: 1;
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 10px;
  min-height: 0;
}
.dl-stage {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}
/* SVG 是 JS 直接创建的节点，拿不到 scoped 的 data-v 属性，所以必须用 :deep() */
.dl-stage :deep(svg) {
  max-width: 100%;
  max-height: 100%;
  width: auto;
  height: auto;
  border-radius: 14px;
  box-shadow: 0 10px 34px rgba(0, 0, 0, 0.45);
  background: #fff;
}
.dl-alert {
  position: absolute;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  background: rgba(13, 22, 32, 0.92);
  border: 1px solid rgba(121, 217, 255, 0.3);
  border-radius: 12px;
  padding: 10px 18px;
  font-size: 13px;
  max-width: 86%;
  text-align: center;
  line-height: 1.6;
  pointer-events: none;
  z-index: 5;
}
.dl-alert.err {
  top: auto;
  bottom: 12px;
  transform: translateX(-50%);
  border-color: rgba(255, 170, 120, 0.5);
  color: #ffe3c2;
}
.dl-alert.warn {
  top: auto;
  bottom: 12px;
  transform: translateX(-50%);
  border-color: rgba(255, 209, 102, 0.5);
  color: #ffe9b8;
}
.dl-log {
  width: 250px;
  display: flex;
  flex-direction: column;
  background: rgba(13, 22, 32, 0.94);
  border-left: 1px solid rgba(121, 217, 255, 0.22);
  font-size: 11.5px;
}
.dl-loghead {
  padding: 9px 12px;
  border-bottom: 1px solid rgba(121, 217, 255, 0.22);
  color: #9fe0ff;
  font-weight: 600;
  font-size: 12px;
}
.dl-logbody {
  flex: 1;
  overflow-y: auto;
  padding: 7px 10px;
  line-height: 1.7;
}
.dl-e {
  color: #8fa6bd;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.dl-e.b {
  color: #cfe6ff;
}
.dl-e.r {
  color: #ffd166;
}
.dl-e.g {
  color: #8fd6a0;
}
.dl-t {
  color: #5f7a96;
  margin-right: 5px;
}
.dl-panel {
  width: 230px;
  background: rgba(13, 22, 32, 0.94);
  border-left: 1px solid rgba(121, 217, 255, 0.22);
  padding: 12px 14px;
  font-size: 12.5px;
  line-height: 1.8;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.dl-panel h3 {
  margin: 0 0 6px;
  font-size: 14px;
  color: #ffd166;
}
.dl-panel .k {
  color: #8fa6bd;
}
.dl-panel .dl-btn {
  margin-top: 10px;
  align-self: flex-start;
}
.dl-foot,
.dl-nav {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 7px 12px;
  background: rgba(13, 22, 32, 0.9);
  border-top: 1px solid rgba(121, 217, 255, 0.22);
  font-size: 12px;
  color: #9fb4cc;
  flex-wrap: wrap;
}
.dl-foot b {
  color: #ffd166;
}
.dl-status :deep(.spin) {
  display: inline-block;
  width: 11px;
  height: 11px;
  border: 2px solid rgba(121, 217, 255, 0.3);
  border-top-color: #79d9ff;
  border-radius: 50%;
  animation: dlsp 0.7s linear infinite;
  vertical-align: -1px;
  margin-right: 5px;
}
.dl-nav {
  padding: 5px 12px 7px;
  border-top: none;
}
.dl-navtip {
  font-size: 11px;
  color: #6f8aa6;
}
.dl-toast {
  position: fixed;
  left: 50%;
  bottom: 18px;
  transform: translateX(-50%);
  z-index: 60;
  background: rgba(16, 26, 40, 0.96);
  border: 1px solid rgba(121, 217, 255, 0.22);
  border-radius: 11px;
  padding: 9px 16px;
  font-size: 12.5px;
  max-width: 88vw;
  text-align: center;
}
@media (max-width: 760px) {
  .dl-log {
    width: 152px;
  }
  .dl-panel {
    width: 168px;
  }
  .dl-inp.ctx {
    min-width: 110px;
  }
}
</style>
