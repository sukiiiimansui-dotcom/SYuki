<!--
  小区可视化对照（数据 / 图层 / 伪 3D）

  三种视图共用**同一张后端渲染的 SVG**（渲染结果是一段 SVG 文本，拿到后塞进 v-html）：
    · 数据可视化：前端把这张 SVG 当数据源解析（g.wm-bld 上带着 data-type/data-floors/data-w/data-h），
      按后端 stats.rs 同一套口径现算指标 —— 所以面板数字和图里后端画的数据卡片可以互相印证；
      需要后端卡片时加 ?charts=1，卡片会被画进 SVG 右上角。
    · 图层开关：**纯 CSS** 显隐（SVG 里本来就带 layer-* 类名），勾选框不触发任何请求。
    · 伪 3D：mode=3d 由后端按楼层挤出立体块，这个只能重新请求（渲染发生在后端）。

  为什么要缓存 SVG：切视图 / 来回切 2D-3D 时同参数的图只请求一次，手机上等一次就够了。

  取图走**双通路**（见 api/services/worldMap.ts 的 districtRenderSvg）：
  真壳（APK / 桌面）`invoke('world_map_render')`，浏览器预览才走 HTTP `/api/render/probe`
  —— 打包后手机上并没有 8791 那个服务，写死 HTTP 的话这一页永远是「渲染失败」。
-->
<template>
  <div class="vz-root">
    <div class="vz-top">
      <button class="vz-btn" title="回到世界地图" @click="goBack">← 返回</button>
      <span class="vz-title">🏙 小区可视化</span>
      <span class="vz-tabs">
        <button
          v-for="t in TABS"
          :key="t.id"
          class="vz-tab"
          :class="{ on: tab === t.id }"
          :title="t.tip"
          @click="tab = t.id"
        >
          {{ t.label }}
        </button>
      </span>
      <span class="vz-spacer" />
      <select v-model="style" class="vz-sel">
        <option value="gaode">高德风</option>
        <option value="dark">暗色夜景</option>
        <option value="water">手绘水彩</option>
      </select>
      <select v-model.number="size" class="vz-sel" title="网格规模">
        <option :value="20">20×20</option>
        <option :value="28">28×28</option>
        <option :value="36">36×36</option>
      </select>
      <select v-model.number="zoom" class="vz-sel" title="缩放层级">
        <option :value="1">缩放 1</option>
        <option :value="2">缩放 2</option>
        <option :value="3">缩放 3</option>
      </select>
      <span class="vz-seg">
        <button class="vz-tab" :class="{ on: mode === '2d' }" @click="mode = '2d'">2D</button>
        <button class="vz-tab" :class="{ on: mode === '3d' }" @click="mode = '3d'">伪 3D</button>
      </span>
      <button class="vz-btn" :disabled="loading" @click="reload(true)">↻ 刷新</button>
    </div>

    <div class="vz-wrap">
      <div class="vz-stagebox">
        <div v-if="loading" class="vz-alert">渲染中…（首次较慢）</div>
        <div v-else-if="err" class="vz-alert err">{{ err }}</div>
        <!-- SVG 是后端产出的字符串，用 v-html 塞进来；图层显隐全部靠外层 class + CSS。
             注意：后端 SVG 自带一段 <style>（text{font-family} / .wm-bld / [data-zoom] 规则），
             它会作用到整个文档。这里同一时刻只挂一张 SVG，所以不会互相打架；
             以后若要同屏显示多张，需要先把这段 style 的选择器加上容器前缀。 -->
        <div
          v-else
          class="vz-stage"
          :class="stageClasses"
          v-html="svgText"
        />
      </div>

      <div class="vz-side">
        <!-- ① 数据可视化 -->
        <template v-if="tab === 'data'">
          <div class="vz-h">📊 数据可视化</div>
          <div v-if="stats" class="vz-kpis">
            <div v-for="k in kpis" :key="k.k" class="vz-kpi">
              <span class="k">{{ k.k }}</span>
              <span class="v">{{ k.v }}</span>
              <span v-if="k.sub" class="s">{{ k.sub }}</span>
            </div>
          </div>
          <div v-if="stats" class="vz-bars">
            <div class="vz-sub">建筑类型分布（栋）</div>
            <div v-for="b in stats.byType" :key="b.type" class="vz-bar">
              <span class="lb">{{ b.zh }}</span>
              <span class="track">
                <i :style="{ width: barWidth(b.count) + '%', background: colorOf(b.type) }" />
              </span>
              <span class="num">{{ b.count }}</span>
            </div>
            <div v-if="!stats.byType.length" class="vz-empty">这张图里没有建筑元素</div>
          </div>
          <div v-else-if="!loading" class="vz-empty">解析不出数据（SVG 结构可能变了）</div>
          <div class="vz-note">
            面板数字由前端从同一张 SVG 解析现算（口径对齐后端 stats.rs：18m/格、住宅 32㎡/人、人流 0-100）；
            开「后端卡片」时 SVG 右上角还有一张后端画的数据卡片，两边可互相印证。
            注：SVG 里的 data-w/data-h 是整数格，AI 若给出小数尺寸，这里会有极小出入。
          </div>
          <label class="vz-chk">
            <input type="checkbox" :checked="withCharts" @change="toggleCharts" />
            <span>让后端把数据卡片画进 SVG（?charts=1）</span>
          </label>
        </template>

        <!-- ② 图层开关 -->
        <template v-else-if="tab === 'layers'">
          <div class="vz-h">🧩 图层开关</div>
          <div class="vz-hint">只切 CSS 类名，**不会重新请求**。数字是这张图里该层的元素个数。</div>
          <div class="vz-ops">
            <button class="vz-btn tiny" @click="showAll">全开</button>
            <button class="vz-btn tiny" @click="hideAll">全关</button>
          </div>
          <label v-for="l in LAYERS" :key="l.id" class="vz-chk" :class="{ dim: layerCount(l.id) === 0 }">
            <input
              type="checkbox"
              :checked="!hidden.includes(l.id)"
              :disabled="layerCount(l.id) === 0"
              @change="toggleLayer(l.id)"
            />
            <span>{{ l.label }}</span>
            <span class="cnt">{{ layerCount(l.id) }}</span>
          </label>

          <div class="vz-sub mt">建筑类型（wm-bld[data-type]）</div>
          <label v-for="t in TYPES" :key="t.id" class="vz-chk" :class="{ dim: typeCount(t.id) === 0 }">
            <input
              type="checkbox"
              :checked="!hiddenTypes.includes(t.id)"
              :disabled="typeCount(t.id) === 0"
              @change="toggleType(t.id)"
            />
            <span class="dot" :style="{ background: t.color }" />
            <span>{{ t.zh }}</span>
            <span class="cnt">{{ typeCount(t.id) }}</span>
          </label>
        </template>

        <!-- ③ 伪 3D -->
        <template v-else>
          <div class="vz-h">🏗 伪 3D（按楼层挤出）</div>
          <div class="vz-hint">
            立体块由**后端渲染**：楼层越高挤出越高（hz = 楼层×1.7，限 3~44），
            所以切 2D/3D 必须重新请求一次 —— 这里没有前端能做的近似。
          </div>
          <div class="vz-seg big">
            <button class="vz-tab" :class="{ on: mode === '2d' }" @click="mode = '2d'">平面 2D</button>
            <button class="vz-tab" :class="{ on: mode === '3d' }" @click="mode = '3d'">立体 3D</button>
          </div>
          <div v-if="stats" class="vz-kpis mini">
            <div class="vz-kpi"><span class="k">楼栋</span><span class="v">{{ stats.buildings }}</span></div>
            <div class="vz-kpi"><span class="k">最高</span><span class="v">{{ stats.maxFloors }}</span><span class="s">层</span></div>
            <div class="vz-kpi"><span class="k">平均</span><span class="v">{{ stats.avgFloors }}</span><span class="s">层</span></div>
          </div>
          <div class="vz-note">
            当前模式：{{ mode === '3d' ? '伪 3D（立体挤出）' : '平面 2D' }}<br />
            图层勾选对 3D 同样生效（立体块也在 layer-bld 里）。
          </div>
          <div class="vz-ops">
            <button class="vz-btn tiny" @click="exportSvg">⬇ 导出这张 SVG</button>
            <button class="vz-btn tiny" @click="openRaw">↗ 新窗口打开</button>
          </div>
        </template>
      </div>
    </div>

    <div class="vz-foot">
      <span class="vz-meta">{{ metaText }}</span>
      <span class="vz-spacer" />
      <span class="vz-navtip">世界地图扩展</span>
      <button class="vz-btn tiny" @click="go('/world/district-live')">🎨 实时绘制</button>
      <button class="vz-btn tiny" @click="go('/world/district-viz')">📊 数据 / 图层</button>
      <button class="vz-btn tiny" @click="go('/world/maplib')">🗂 地图库</button>
      <button class="vz-btn tiny" @click="go('/world/phone-overlay')">📱 悬浮窗</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { districtRenderSvg, isTauriRuntime, renderProbeUrl, type RenderProbeOpts } from '@/api/services/worldMap'
import { TYPE_COLOR, TYPE_ZH } from './districtDraw'

const router = useRouter()

type TabId = 'data' | 'layers' | '3d'
const TABS: { id: TabId; label: string; tip: string }[] = [
  { id: 'data', label: '📊 数据', tip: '容积率/密度/绿地率/人口/人流 + 类型条形图' },
  { id: 'layers', label: '🧩 图层', tip: '勾选显示哪几层（纯 CSS，不重新请求）' },
  { id: '3d', label: '🏗 伪 3D', tip: '按楼层挤出的立体块' },
]

/** 可勾选图层：id 必须与后端 render.rs 里 lay("...") 产出的 layer-* 类名一致 */
const LAYERS: { id: string; label: string }[] = [
  { id: 'park', label: '🌳 绿地' },
  { id: 'water', label: '💧 水域' },
  { id: 'road', label: '🛣 道路' },
  { id: 'bld', label: '🏢 建筑' },
  { id: 'tree', label: '🌲 行道树' },
  { id: 'lamp', label: '💡 路灯' },
  { id: 'parking', label: '🅿️ 车位' },
  { id: 'bus', label: '🚌 公交站' },
  { id: 'sidewalk', label: '🚶 人行道' },
  { id: 'crosswalk', label: '🦓 斑马线' },
  { id: 'signal', label: '🚦 信号灯' },
]

/** 建筑类型（对应 wm-bld[data-type]）——顺序按「常见 → 少见」，颜色与后端取色一致 */
const TYPES: { id: string; zh: string; color: string }[] = [
  { id: 'residential', zh: '住宅', color: TYPE_COLOR.residential },
  { id: 'office', zh: '写字楼', color: TYPE_COLOR.office },
  { id: 'commercial', zh: '商场', color: TYPE_COLOR.commercial },
  { id: 'shop', zh: '店铺', color: TYPE_COLOR.shop },
  { id: 'restaurant', zh: '餐饮', color: TYPE_COLOR.restaurant },
  { id: 'cafe', zh: '咖啡', color: TYPE_COLOR.cafe },
  { id: 'school', zh: '学校', color: TYPE_COLOR.school },
  { id: 'hospital', zh: '医院', color: TYPE_COLOR.hospital },
  { id: 'civic', zh: '市政', color: TYPE_COLOR.civic },
  { id: 'leisure', zh: '娱乐', color: TYPE_COLOR.leisure },
]

const style = ref('gaode')
const size = ref(20)
const zoom = ref(3)
const mode = ref<'2d' | '3d'>('2d')
const tab = ref<TabId>('data')
const withCharts = ref(true)

const loading = ref(false)
const err = ref('')
const svgText = ref('')
const stats = ref<VizStats | null>(null)
const hidden = ref<string[]>([])
const hiddenTypes = ref<string[]>([])
/** 同参数的 SVG 只向后端要一次：来回切视图/模式时秒开 */
const cache = new Map<string, string>()

interface VizStats {
  size: number
  buildings: number
  byType: { type: string; zh: string; count: number; footprint: number }[]
  plotRatio: number
  densityPct: number
  greenPct: number
  waterPct: number
  popEstimate: number
  crowdIndex: number
  avgFloors: number
  maxFloors: number
  roadKm: number
  roadCount: number
  layerCounts: Record<string, number>
  typeCounts: Record<string, number>
}

/** 各类型「人的密度」权重：与后端 stats.rs 的 type_weight 保持一致 */
const TYPE_WEIGHT: Record<string, number> = {
  residential: 1.0,
  office: 1.6,
  commercial: 2.2,
  shop: 1.4,
  restaurant: 1.3,
  cafe: 0.9,
  school: 1.8,
  hospital: 1.5,
  civic: 0.8,
  leisure: 1.2,
}
const GRID_M = 18
const SQ_M_PER_PERSON = 32

const stageClasses = computed(() => [
  ...hidden.value.map((l) => `hide-${l}`),
  ...hiddenTypes.value.map((t) => `hide-btype-${t}`),
])

const kpis = computed(() => {
  const s = stats.value
  if (!s) return []
  return [
    { k: '容积率', v: String(s.plotRatio), sub: '总建面 / 用地' },
    { k: '建筑密度', v: `${s.densityPct}%`, sub: '占地 / 用地' },
    { k: '绿地率', v: `${s.greenPct}%`, sub: '绿地 / 用地' },
    { k: '水域率', v: `${s.waterPct}%`, sub: '水面 / 用地' },
    { k: '人口估算', v: String(s.popEstimate), sub: '住宅面积 / 32㎡' },
    { k: '人流强度', v: String(s.crowdIndex), sub: '加权 0-100' },
    { k: '楼栋数', v: String(s.buildings), sub: `平均 ${s.avgFloors} 层` },
    { k: '道路', v: `${s.roadKm}`, sub: `km · ${s.roadCount} 条` },
  ]
})

const metaText = computed(() => {
  const bits = [`风格 ${style.value}`, `规模 ${size.value}×${size.value}`, `缩放 ${zoom.value}`, mode.value === '3d' ? '伪 3D' : '2D']
  if (stats.value) bits.push(`${stats.value.buildings} 栋 · 容积率 ${stats.value.plotRatio}`)
  if (hidden.value.length) bits.push(`隐藏 ${hidden.value.length} 层`)
  return bits.join(' · ')
})

function colorOf(t: string): string {
  return TYPE_COLOR[t] || '#9aa7b8'
}

function barWidth(n: number): number {
  const max = Math.max(1, ...(stats.value?.byType || []).map((b) => b.count))
  return Math.max(4, Math.round((n / max) * 100))
}

function layerCount(id: string): number {
  return stats.value?.layerCounts?.[id] ?? 0
}
function typeCount(id: string): number {
  return stats.value?.typeCounts?.[id] ?? 0
}

function toggleLayer(id: string) {
  const i = hidden.value.indexOf(id)
  if (i >= 0) hidden.value.splice(i, 1)
  else hidden.value.push(id)
}
function toggleType(id: string) {
  const i = hiddenTypes.value.indexOf(id)
  if (i >= 0) hiddenTypes.value.splice(i, 1)
  else hiddenTypes.value.push(id)
}
function showAll() {
  hidden.value = []
  hiddenTypes.value = []
}
function hideAll() {
  // 建筑留一层，否则整张图会空掉、看起来像坏了
  hidden.value = LAYERS.filter((l) => l.id !== 'bld').map((l) => l.id)
}

function toggleCharts(e: Event) {
  withCharts.value = (e.target as HTMLInputElement).checked
}

/**
 * 把后端渲染的 SVG 当数据源解析。
 *
 * 为什么不去要一份 JSON 布局：后端没有「给我布局 JSON」的只读接口
 * （只有流式生成和地图库），但 render 出来的 SVG 里建筑自带
 * data-type/data-floors/data-w/data-h，绿地水域是带 layer-* 的 rect，
 * 信息量足够按同一口径复算指标，还省一次请求。
 */
function parseViz(text: string, gridSize: number): VizStats | null {
  try {
    const doc = new DOMParser().parseFromString(text, 'image/svg+xml')
    if (doc.querySelector('parsererror')) return null
    const svg = doc.documentElement

    // 格子边长：优先从「地块背景方块」量（宽 = cell*size），量不到再按后端默认 900/48 推算
    let cell = (900 - 2 * 48) / gridSize
    const bg = svg.querySelector('rect[rx="6"]')
    if (bg) {
      const w = Number(bg.getAttribute('width')) || 0
      if (w > 0) cell = w / gridSize
    }

    const typeCounts: Record<string, number> = {}
    const agg: Record<string, { count: number; footprint: number; floorArea: number }> = {}
    let footprintGrid = 0
    let floorAreaGrid = 0
    let resFloorGrid = 0
    let weighted = 0
    let floorsSum = 0
    let maxFloors = 0

    const blds = Array.from(svg.querySelectorAll('g.wm-bld'))
    for (const g of blds) {
      const type = g.getAttribute('data-type') || 'residential'
      const w = Number(g.getAttribute('data-w')) || 0
      const h = Number(g.getAttribute('data-h')) || 0
      // 楼层缺失时后端挤出用的是 3 层（render.rs unwrap_or(3.0)），这里跟着算
      const fl = Number(g.getAttribute('data-floors')) || 3
      const fp = w * h
      const fa = fp * fl
      footprintGrid += fp
      floorAreaGrid += fa
      if (type === 'residential') resFloorGrid += fa
      weighted += (TYPE_WEIGHT[type] ?? 1) * fa
      floorsSum += fl
      maxFloors = Math.max(maxFloors, fl)
      typeCounts[type] = (typeCounts[type] || 0) + 1
      const a = agg[type] || { count: 0, footprint: 0, floorArea: 0 }
      a.count++
      a.footprint += fp
      a.floorArea += fa
      agg[type] = a
    }

    // 绿地/水面在 SVG 里是 px 尺寸：水面本身是 rect，绿地是 <g class="layer-park"><rect/></g>，
    // 所以统一「取自身或内部第一个 rect」再算面积；最后把 px² 换算回格²
    const areaGrid = (sel: string): number => {
      let px = 0
      svg.querySelectorAll(sel).forEach((n) => {
        const r = n.tagName.toLowerCase() === 'rect' ? n : n.querySelector('rect')
        if (!r) return
        px += (Number(r.getAttribute('width')) || 0) * (Number(r.getAttribute('height')) || 0)
      })
      return cell > 0 ? px / (cell * cell) : 0
    }
    const parkGrid = areaGrid('.layer-park')
    const waterGrid = areaGrid('.layer-water')

    const layerCounts: Record<string, number> = {}
    for (const l of LAYERS) layerCounts[l.id] = svg.querySelectorAll(`.layer-${l.id}`).length

    // 道路长度：一个路组里的多条 line 端点相同，取第一条就够，避免重复计算
    let roadLenGrid = 0
    let roadCount = 0
    svg.querySelectorAll('g.layer-road').forEach((g) => {
      roadCount++
      const ln = g.querySelector('line')
      if (!ln) return
      const x1 = Number(ln.getAttribute('x1')) || 0
      const y1 = Number(ln.getAttribute('y1')) || 0
      const x2 = Number(ln.getAttribute('x2')) || 0
      const y2 = Number(ln.getAttribute('y2')) || 0
      const dx = Math.abs(x2 - x1)
      const dy = Math.abs(y2 - y1)
      // 与后端 stats.rs 的 seg_len 同口径：正交路段直接相加，斜路用欧氏距离
      roadLenGrid += dx < 1e-9 || dy < 1e-9 ? dx + dy : Math.sqrt(dx * dx + dy * dy)
    })

    const grid = gridSize * gridSize
    const r1 = (x: number) => Math.round(x * 10) / 10
    const r2 = (x: number) => Math.round(x * 100) / 100

    return {
      size: gridSize,
      buildings: blds.length,
      byType: Object.entries(agg)
        .map(([type, a]) => ({ type, zh: TYPE_ZH[type] || type, count: a.count, footprint: a.footprint }))
        .sort((a, b) => b.count - a.count),
      plotRatio: r2(floorAreaGrid / grid),
      densityPct: r1((footprintGrid / grid) * 100),
      greenPct: r1((parkGrid / grid) * 100),
      waterPct: r1((waterGrid / grid) * 100),
      popEstimate: Math.round((resFloorGrid * GRID_M * GRID_M) / SQ_M_PER_PERSON),
      crowdIndex: r1(Math.min(100, (weighted / grid) * 12)),
      avgFloors: blds.length ? r1(floorsSum / blds.length) : 0,
      maxFloors,
      roadKm: r2((roadLenGrid * GRID_M) / 1000),
      roadCount,
      layerCounts,
      typeCounts,
    }
  } catch {
    return null
  }
}

async function reload(force = false) {
  loading.value = true
  err.value = ''
  const opts: RenderProbeOpts = {
    style: style.value,
    mode: mode.value,
    zoom: zoom.value,
    size: size.value,
    charts: tab.value === 'data' && withCharts.value,
    // 固定 seed：切图层/切视图时图不变，用户改的才是「显隐」而不是「换了一张图」
    seed: 7,
  }
  // 缓存键只要求「同参数 → 同键」，与走哪条通路无关：
  // 浏览器路径沿用它原来的 HTTP 地址（对着日志好排查），真壳里没有地址，用参数序列化。
  const key = isTauriRuntime()
    ? `rust|${opts.style}|${opts.mode}|${opts.zoom}|${opts.size}|${opts.charts ? 1 : 0}|${opts.seed}`
    : renderProbeUrl(opts)
  try {
    let text = cache.get(key)
    if (!text || force) {
      // 双通路：真壳 invoke `world_map_render`（APK 里没有 8791 服务），浏览器仍走 HTTP 探针。
      text = await districtRenderSvg(opts)
      cache.set(key, text)
    }
    svgText.value = text
    stats.value = parseViz(text, size.value)
  } catch (e) {
    // 后端出错时给可读提示，不留白屏
    err.value = `渲染失败：${(e as Error)?.message || e}`
    svgText.value = ''
    stats.value = null
  } finally {
    loading.value = false
  }
}

function exportSvg() {
  try {
    const blob = new Blob([svgText.value], { type: 'image/svg+xml;charset=utf-8' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `district_${style.value}_${mode.value}_${size.value}.svg`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    window.setTimeout(() => URL.revokeObjectURL(url), 8000)
  } catch (e) {
    err.value = `导出失败：${(e as Error)?.message || e}`
  }
}

/** 新窗口看原图：部分 WebView 会拦 window.open，所以要给失败提示而不是静默 */
function openRaw() {
  // 真壳里没有 8791 那个 HTTP 服务，`renderProbeUrl()` 拼出来的地址打不开（会开出一张空白页，
  // 用户只会以为「图坏了」）。所以真壳直接引导到本页已有的「导出这张 SVG」——
  // 那条路用的是已经拿到的 svgText，不依赖任何服务。
  if (isTauriRuntime()) {
    err.value = '应用内没有本地网页服务，打不开裸图地址；请用「⬇ 导出 SVG」保存后用外部工具查看'
    return
  }
  try {
    const w = window.open(renderProbeUrl({ style: style.value, mode: mode.value, zoom: zoom.value, size: size.value }), '_blank')
    if (!w) err.value = '新窗口被拦截了，可以改用「导出这张 SVG」'
  } catch (e) {
    err.value = `打不开新窗口：${(e as Error)?.message || e}`
  }
}

function go(path: string) {
  router.push(path)
}
function goBack() {
  router.push('/world')
}

// 影响渲染结果的参数变了就重新取图；图层/类型勾选不在依赖里 —— 它们只改 CSS
watch([style, size, zoom, mode, tab, withCharts], () => {
  void reload()
})

onMounted(() => {
  void reload()
})
</script>

<style scoped>
.vz-root {
  position: fixed;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: linear-gradient(160deg, #0d1620, #16273d);
  color: #eaf3ff;
  z-index: 50;
}
.vz-top {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 7px 11px;
  background: rgba(13, 22, 32, 0.92);
  border-bottom: 1px solid rgba(121, 217, 255, 0.22);
  flex-wrap: wrap;
}
.vz-title {
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
}
.vz-spacer {
  flex: 1;
}
.vz-btn,
.vz-sel {
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(121, 217, 255, 0.25);
  border-radius: 9px;
  padding: 6px 11px;
  color: #eaf3ff;
  font-size: 12.5px;
  cursor: pointer;
}
.vz-btn:hover:not(:disabled) {
  background: rgba(121, 217, 255, 0.18);
}
.vz-btn:disabled {
  opacity: 0.45;
  cursor: default;
}
.vz-btn.tiny {
  padding: 4px 9px;
  font-size: 11.5px;
}
.vz-tabs,
.vz-seg {
  display: inline-flex;
  gap: 4px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(121, 217, 255, 0.18);
  border-radius: 10px;
  padding: 3px;
}
.vz-seg.big {
  width: 100%;
  margin: 6px 0 10px;
}
.vz-seg.big .vz-tab {
  flex: 1;
}
.vz-tab {
  background: transparent;
  border: none;
  border-radius: 8px;
  color: #cfe6ff;
  font-size: 12.5px;
  padding: 5px 10px;
  cursor: pointer;
}
.vz-tab.on {
  background: rgba(121, 217, 255, 0.26);
  color: #fff;
}
.vz-wrap {
  flex: 1;
  display: flex;
  min-height: 0;
}
.vz-stagebox {
  flex: 1;
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 10px;
  min-height: 0;
}
.vz-stage {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}
.vz-stage :deep(svg) {
  max-width: 100%;
  max-height: 100%;
  width: auto;
  height: auto;
  border-radius: 14px;
  box-shadow: 0 10px 34px rgba(0, 0, 0, 0.45);
  background: #fff;
}
/* ── 图层显隐：全部是纯 CSS，勾选不触发请求 ── */
.vz-stage.hide-park :deep(.layer-park) {
  display: none;
}
.vz-stage.hide-water :deep(.layer-water) {
  display: none;
}
.vz-stage.hide-road :deep(.layer-road) {
  display: none;
}
.vz-stage.hide-bld :deep(.layer-bld) {
  display: none;
}
.vz-stage.hide-tree :deep(.layer-tree) {
  display: none;
}
.vz-stage.hide-lamp :deep(.layer-lamp) {
  display: none;
}
.vz-stage.hide-parking :deep(.layer-parking) {
  display: none;
}
.vz-stage.hide-bus :deep(.layer-bus) {
  display: none;
}
.vz-stage.hide-sidewalk :deep(.layer-sidewalk) {
  display: none;
}
.vz-stage.hide-crosswalk :deep(.layer-crosswalk) {
  display: none;
}
.vz-stage.hide-signal :deep(.layer-signal) {
  display: none;
}
/* ── 建筑类型显隐：wm-bld[data-type] ── */
.vz-stage.hide-btype-residential :deep(.wm-bld[data-type='residential']) {
  display: none;
}
.vz-stage.hide-btype-office :deep(.wm-bld[data-type='office']) {
  display: none;
}
.vz-stage.hide-btype-commercial :deep(.wm-bld[data-type='commercial']) {
  display: none;
}
.vz-stage.hide-btype-shop :deep(.wm-bld[data-type='shop']) {
  display: none;
}
.vz-stage.hide-btype-restaurant :deep(.wm-bld[data-type='restaurant']) {
  display: none;
}
.vz-stage.hide-btype-cafe :deep(.wm-bld[data-type='cafe']) {
  display: none;
}
.vz-stage.hide-btype-school :deep(.wm-bld[data-type='school']) {
  display: none;
}
.vz-stage.hide-btype-hospital :deep(.wm-bld[data-type='hospital']) {
  display: none;
}
.vz-stage.hide-btype-civic :deep(.wm-bld[data-type='civic']) {
  display: none;
}
.vz-stage.hide-btype-leisure :deep(.wm-bld[data-type='leisure']) {
  display: none;
}
.vz-alert {
  background: rgba(13, 22, 32, 0.92);
  border: 1px solid rgba(121, 217, 255, 0.3);
  border-radius: 12px;
  padding: 10px 18px;
  font-size: 13px;
  max-width: 86%;
  text-align: center;
}
.vz-alert.err {
  border-color: rgba(255, 170, 120, 0.5);
  color: #ffe3c2;
}
.vz-side {
  width: 268px;
  padding: 10px 12px 16px;
  background: rgba(13, 22, 32, 0.94);
  border-left: 1px solid rgba(121, 217, 255, 0.22);
  overflow-y: auto;
  font-size: 12px;
}
.vz-h {
  font-size: 13.5px;
  font-weight: 600;
  color: #9fe0ff;
  margin-bottom: 6px;
}
.vz-hint {
  color: #8fa6bd;
  line-height: 1.6;
  margin-bottom: 8px;
}
.vz-sub {
  color: #cfe6ff;
  font-weight: 600;
  margin: 10px 0 6px;
}
.vz-sub.mt {
  margin-top: 14px;
}
.vz-kpis {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
}
.vz-kpis.mini {
  grid-template-columns: 1fr 1fr 1fr;
}
.vz-kpi {
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(121, 217, 255, 0.18);
  border-radius: 10px;
  padding: 6px 8px;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.vz-kpi .k {
  font-size: 10.5px;
  color: #8fa6bd;
}
.vz-kpi .v {
  font-size: 15px;
  font-weight: 700;
  color: #ffd166;
}
.vz-kpi .s {
  font-size: 10px;
  color: #7f97b0;
}
.vz-bars {
  margin-top: 4px;
}
.vz-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 4px;
}
.vz-bar .lb {
  width: 46px;
  color: #cfe6ff;
  flex: none;
}
.vz-bar .track {
  flex: 1;
  height: 10px;
  background: rgba(255, 255, 255, 0.09);
  border-radius: 3px;
  overflow: hidden;
}
.vz-bar .track i {
  display: block;
  height: 100%;
  border-radius: 3px;
  transition: width 0.25s;
}
.vz-bar .num {
  width: 22px;
  text-align: right;
  color: #9fb4cc;
  flex: none;
}
.vz-note {
  margin-top: 12px;
  color: #7f97b0;
  line-height: 1.65;
  font-size: 11px;
  border-left: 2px solid rgba(121, 217, 255, 0.3);
  padding-left: 8px;
}
.vz-chk {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 4px 2px;
  cursor: pointer;
  color: #dbe9f8;
}
.vz-chk.dim {
  opacity: 0.45;
}
.vz-chk .cnt {
  margin-left: auto;
  color: #8fa6bd;
  font-size: 11px;
}
.vz-chk .dot {
  width: 9px;
  height: 9px;
  border-radius: 2px;
  display: inline-block;
}
.vz-ops {
  display: flex;
  gap: 6px;
  margin: 8px 0;
  flex-wrap: wrap;
}
.vz-empty {
  color: #8fa6bd;
  padding: 6px 0;
}
.vz-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 11px;
  background: rgba(13, 22, 32, 0.9);
  border-top: 1px solid rgba(121, 217, 255, 0.22);
  font-size: 11.5px;
  color: #9fb4cc;
  flex-wrap: wrap;
}
.vz-meta {
  color: #b9d4ec;
}
@media (max-width: 780px) {
  .vz-side {
    width: 190px;
    font-size: 11.5px;
  }
  .vz-kpi .v {
    font-size: 13.5px;
  }
}
</style>
