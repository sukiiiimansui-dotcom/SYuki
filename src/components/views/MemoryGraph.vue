<template>
  <div class="mg-root" ref="rootRef">
    <!-- 无限沙盒：世界坐标系，可平移/缩放 -->
    <svg
      class="mg-svg"
      :viewBox="`0 0 ${dim.w} ${dim.h}`"
      preserveAspectRatio="xMidYMid meet"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
      @wheel="onWheel"
      @click="onBlankClick"
    >
      <g :transform="`translate(${view.tx},${view.ty}) scale(${view.s})`">
        <!-- 层与层：食物链主链 （L1→L2→L3→L4） -->
        <template v-for="(l, i) in LAYERS" :key="'chain'+i">
          <g v-if="i < LAYERS.length - 1">
            <path v-for="(p, k) in chainArrow(l, LAYERS[i+1])" :key="k" :d="p.d" :fill="p.fill" :opacity="p.op" />
          </g>
        </template>

        <!-- 各层大球 -->
        <g
          v-for="l in LAYERS" :key="l.key"
          class="mg-interactive hub"
          :class="{ active: activeLayer === l.key }"
          :transform="`translate(${l.x},${l.y})`" @click.stop="toggleLayer(l.key)"
        >
          <circle class="hub-ring" :r="l.hubR" :fill="l.color" :stroke="l.ring" :stroke-width="activeLayer === l.key ? 3 : 1.5" />
          <text class="hub-label" y="-6">{{ l.short }}</text>
          <text class="hub-sub" y="12">{{ l.label }}</text>
          <text class="hub-count" y="30">{{ nodeCount(l.key) }} 条</text>
        </g>

        <!-- 激活层的记忆小球 + 大球→小球 方向箭（食物链） -->
        <template v-if="activeLayer">
          <g v-for="n in activeNodes" :key="n.id">
            <path v-for="(p, k) in linkArrow(hubFor(activeLayer), n)" :key="'a'+k" :d="p.d" :fill="p.fill" :opacity="p.op" />
            <g
              class="mg-interactive node"
              :class="{ pick: selectedId === n.id, dim: selectedId !== null && selectedId !== n.id && !relatedIds.has(n.id) }"
              :transform="`translate(${n.x},${n.y})`" @click.stop="selectNode(n.id)"
            >
              <circle :r="radius(n)" :fill="selectedId === n.id ? hubFor(activeLayer).color : '#fff'" :stroke="hubFor(activeLayer).ring" :stroke-width="selectedId === n.id ? 0 : 2" />
              <text class="node-label" y="4">{{ short(n.text) }}</text>
            </g>
          </g>
        </template>
      </g>
    </svg>

    <!-- 右上角可拖拽「关联最强」浮窗 -->
    <div v-if="selected" class="rel-panel" :style="{ left: relPos.x + 'px', top: relPos.y + 'px' }">
      <div class="rel-head" @mousedown="startRelDrag" @touchstart.passive="startRelDragT">
        <span>🔗 关联最强</span>
        <button class="rel-close" @click="selectedId = null">✕</button>
      </div>
      <div class="rel-body">
        <div v-if="!related.length" class="rel-empty">没有明显关联的记忆</div>
        <div v-for="(r, i) in related" :key="r.id" class="rel-item" @click="jumpTo(r)">
          <span class="rel-tag" :style="{ background: layerColor(r.category) }">{{ layerShort(r.category) }}</span>
          <span class="rel-text">{{ short(r.text) }}</span>
          <span class="rel-strength">{{ r.score }}</span>
        </div>
      </div>
    </div>

    <!-- 底部工具条 -->
    <div class="mg-toolbar">
      <button v-if="activeLayer" class="mg-btn" @click="toggleLayer(null)">↺ 全览</button>
      <span v-else class="mg-hint">点大球展开该层 · 拖拽平移 · 滚轮缩放</span>
      <span class="mg-zoom">{{ Math.round(view.s * 100) }}%</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'

interface N { id: number; text: string; category: string; x: number; y: number }
interface Related { id: number; text: string; category: string; score: number }

const props = defineProps<{
  sections: { short_term: string; long_term: string; user_info: string; promises: string }
}>()

// 二次元清新风（粉彩轻快）
const LAYERS = [
  { key: 'short_term', short: 'L1', label: '近期回顾', color: '#7cc0f0', ring: '#5f9fd8', hubR: 46, x: 340, y: 170 },
  { key: 'long_term', short: 'L2', label: '长期经历', color: '#9a8ff0', ring: '#7f74d8', hubR: 46, x: 285, y: 430 },
  { key: 'user_info', short: 'L3', label: '用户信息', color: '#7fd4a8', ring: '#5fb98a', hubR: 46, x: 340, y: 690 },
  { key: 'promises', short: 'L4', label: '重要约定', color: '#f591b0', ring: '#d87395', hubR: 46, x: 285, y: 950 },
] as const
const layerMap = Object.fromEntries(LAYERS.map((l) => [l.key, l])) as Record<string, any>

interface Dim { w: number; h: number }
const rootRef = ref<HTMLElement | null>(null)
const dim = reactive<Dim>({ w: 360, h: 300 })

const nodes: N[] = []
const nodeMap: Record<number, N> = {}
const activeLayer = ref<string | null>(null)
const selectedId = ref<number | null>(null)
const relatedIds = ref<Set<number>>(new Set())
const related = ref<Related[]>([])
const view = reactive({ s: 1, tx: 0, ty: 0 })
const relPos = reactive({ x: 12, y: 44 })

// ── 构建节点（围绕各自大球径向排布） ──
function sec(key: string): string { return (props.sections as Record<string, string>)[key] || '' }
function split(text: string): string[] {
  return text.split(/[\n。；;]+/).map((t) => t.trim()).filter((t) => t.length >= 2)
}
function buildNodes() {
  let id = 0
  for (const l of LAYERS) {
    const list = split(sec(l.key))
    const n = list.length
    list.forEach((t, i) => {
      const ang = (-75 + (150 * i) / Math.max(1, n - 1)) * (Math.PI / 180)
      const R = 150 + (i % 2) * 34
      const node: N = {
        id: id++, text: t, category: l.key,
        x: l.x + Math.cos(ang) * R,
        y: l.y + Math.sin(ang) * R,
      }
      nodes.push(node)
      nodeMap[node.id] = node
    })
  }
}

// ── 2-gram 相似度（关联最强） ──
function bigrams(t: string): Set<string> {
  const s = new Set<string>()
  const c = t.replace(/[\s，。；、！？,.!?;:\-—"“”"'()（）]/g, '')
  for (let i = 0; i < c.length - 1; i++) s.add(c.slice(i, i + 2))
  if (c.length === 1) s.add(c)
  return s
}
const grams: Set<string>[] = []
function computeRelated() {
  grams.length = 0
  for (const n of nodes) grams.push(bigrams(n.text))
}
function topRelated(id: number): Related[] {
  const g = grams[id]
  const arr: Related[] = []
  for (const other of nodes) {
    if (other.id === id) continue
    const og = grams[other.id]
    let w = 0
    const small = g.size < og.size ? g : og
    const big = g.size < og.size ? og : g
    small.forEach((x) => { if (big.has(x)) w++ })
    if (w >= 1) arr.push({ id: other.id, text: other.text, category: other.category, score: w })
  }
  arr.sort((a, b) => b.score - a.score)
  return arr.slice(0, 4)
}
function selectNode(id: number) {
  selectedId.value = id
  const top = topRelated(id)
  related.value = top
  relatedIds.value = new Set(top.map((r) => r.id))
  // 若选中节点不在当前层，跳到它所在层
  const cat = nodeMap[id].category
  if (activeLayer.value !== cat) activeLayer.value = cat
}

// ── 层操作 ──
function toggleLayer(key: string | null) {
  activeLayer.value = key
  if (selectedId.value !== null) { selectedId.value = null; related.value = []; relatedIds.value = new Set() }
}
function hubFor(key: string) { return layerMap[key] }
function nodeCount(key: string) { return split(sec(key)).length }

const activeNodes = computed(() => {
  if (!activeLayer.value) return []
  return nodes.filter((n) => n.category === activeLayer.value)
})

// ── 食物链箭头（锥形渐变 + 箭头） ──
function arrowShapes(x1: number, y1: number, x2: number, y2: number, color: string, w0 = 4.6, w1 = 1.2, ah = 14, op = 0.55) {
  const dx = x2 - x1, dy = y2 - y1
  const len = Math.max(0.01, Math.hypot(dx, dy))
  const ux = dx / len, uy = dy / len
  const px = -uy, py = ux
  const bx = x2 - ux * ah, by = y2 - uy * ah
  const body = `M${x1 + px * w0 / 2},${y1 + py * w0 / 2} L${bx + px * w1 / 2},${by + py * w1 / 2} L${bx - px * w1 / 2},${by - py * w1 / 2} L${x1 - px * w0 / 2},${y1 - py * w0 / 2} Z`
  const head = `M${bx + px * ah / 2},${by + py * ah / 2} L${x2},${y2} L${bx - px * ah / 2},${by - py * ah / 2} Z`
  return [
    { d: body, fill: color, op },
    { d: head, fill: color, op: 1 },
  ]
}
function chainArrow(a: any, b: any) {
  // 端点略缩短，避免压到大球
  const dx = b.x - a.x, dy = b.y - a.y
  const len = Math.max(0.01, Math.hypot(dx, dy))
  const sx = a.x + (dx / len) * a.hubR, sy = a.y + (dy / len) * a.hubR
  const ex = b.x - (dx / len) * (b.hubR + 6), ey = b.y - (dy / len) * (b.hubR + 6)
  return arrowShapes(sx, sy, ex, ey, a.color, 5.5, 1.6, 16, 0.5)
}
function linkArrow(hub: any, n: N) {
  return arrowShapes(hub.x, hub.y, n.x, n.y, hub.color, 3.6, 1, 11, 0.4)
}

function radius(n: N) {
  return 15 + Math.min(7, n.text.length / 9)
}
function short(t: string) { return t.length > 10 ? t.slice(0, 10) + '…' : t }
function layerColor(cat: string) { return (layerMap[cat] && layerMap[cat].color) || '#888' }
function layerShort(cat: string) { return (layerMap[cat] && layerMap[cat].short) || cat }
function jumpTo(r: Related) {
  activeLayer.value = r.category
  selectedId.value = r.id
  selectNode(r.id)
}

// ── 无限沙盒：平移 / 缩放 ──
let userMoved = false
let pan = { active: false, x: 0, y: 0, tx: 0, ty: 0 }
let pointers = new Map<number, { x: number; y: number }>()
let pinchStart: { d: number; s: number } | null = null

function clamp(v: number, lo: number, hi: number) { return Math.max(lo, Math.min(hi, v)) }
function onPointerDown(e: PointerEvent) {
  pointers.set(e.pointerId, { x: e.clientX, y: e.clientY })
  if (pointers.size === 1) {
    pan.active = true; pan.x = e.clientX; pan.y = e.clientY; pan.tx = view.tx; pan.ty = view.ty; userMoved = true
  } else if (pointers.size === 2) {
    pan.active = false
    const [a, b] = [...pointers.values()]
    pinchStart = { d: Math.hypot(a.x - b.x, a.y - b.y), s: view.s }
  }
}
function onPointerMove(e: PointerEvent) {
  if (pointers.has(e.pointerId)) pointers.set(e.pointerId, { x: e.clientX, y: e.clientY })
  if (pointers.size === 1 && pan.active) {
    view.tx = pan.tx + (e.clientX - pan.x)
    view.ty = pan.ty + (e.clientY - pan.y)
  } else if (pointers.size === 2 && pinchStart) {
    const [a, b] = [...pointers.values()]
    const d = Math.hypot(a.x - b.x, a.y - b.y)
    view.s = clamp(pinchStart.s * (d / Math.max(1, pinchStart.d)), 0.2, 4)
  }
}
function onPointerUp(e: PointerEvent) {
  pointers.delete(e.pointerId)
  if (pointers.size === 0) { pan.active = false; pinchStart = null }
}
function onWheel(e: WheelEvent) {
  e.preventDefault()
  const rect = (rootRef.value as any)?.querySelector('.mg-svg')?.getBoundingClientRect()
  if (!rect) return
  const mx = e.clientX - rect.left, my = e.clientY - rect.top
  const factor = Math.exp(-e.deltaY * 0.0016)
  const s = clamp(view.s * factor, 0.2, 4)
  const k = s / view.s
  view.tx = mx - (mx - view.tx) * k
  view.ty = my - (my - view.ty) * k
  view.s = s
  userMoved = true
}
function onBlankClick() {
  if (activeLayer.value) toggleLayer(null)
  else if (selectedId.value !== null) { selectedId.value = null; related.value = []; relatedIds.value = new Set() }
}
function fitView() {
  const pad = 90
  const xs = LAYERS.map((l) => l.x), ys = LAYERS.map((l) => l.y)
  const minX = Math.min(...xs) - pad, maxX = Math.max(...xs) + pad
  const minY = Math.min(...ys) - pad, maxY = Math.max(...ys) + pad
  const s = clamp(Math.min(dim.w / (maxX - minX), dim.h / (maxY - minY)), 0.3, 1.5)
  view.s = s
  view.tx = dim.w / 2 - ((minX + maxX) / 2) * s
  view.ty = dim.h / 2 - ((minY + maxY) / 2) * s
}

// ── 关联浮窗拖拽 ──
function startRelDrag(e: MouseEvent) {
  relPos.x = e.clientX - (rootRef.value?.getBoundingClientRect().left || 0) - 0
  // 记录相对偏移
  const rect = (rootRef.value as any)?.querySelector('.rel-panel')?.getBoundingClientRect()
  ;(window as any).__relDrag = { ox: e.clientX - (rect?.left || 0), oy: e.clientY - (rect?.top || 0) }
  const move = (ev: MouseEvent) => {
    const d = (window as any).__relDrag
    relPos.x = ev.clientX - d.ox - (rootRef.value?.getBoundingClientRect().left || 0)
    relPos.y = ev.clientY - d.oy - (rootRef.value?.getBoundingClientRect().top || 0)
  }
  const up = () => { window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up) }
  window.addEventListener('mousemove', move); window.addEventListener('mouseup', up)
}
function startRelDragT(e: TouchEvent) {
  const t = e.touches[0]
  const rect = (rootRef.value as any)?.querySelector('.rel-panel')?.getBoundingClientRect()
  ;(window as any).__relDrag = { ox: t.clientX - (rect?.left || 0), oy: t.clientY - (rect?.top || 0) }
  const move = (ev: TouchEvent) => {
    const d = (window as any).__relDrag
    const tt = ev.touches[0]
    relPos.x = tt.clientX - d.ox - (rootRef.value?.getBoundingClientRect().left || 0)
    relPos.y = tt.clientY - d.oy - (rootRef.value?.getBoundingClientRect().top || 0)
  }
  const up = () => { window.removeEventListener('touchmove', move); window.removeEventListener('touchend', up) }
  window.addEventListener('touchmove', move, { passive: true }); window.addEventListener('touchend', up)
}

const selected = computed(() => (selectedId.value !== null ? nodeMap[selectedId.value] : null))

let ro: ResizeObserver | null = null
onMounted(() => {
  buildNodes()
  computeRelated()
  if (rootRef.value) {
    const upd = () => {
      const r = rootRef.value?.getBoundingClientRect()
      if (r && r.width > 0) {
        dim.w = Math.round(r.width)
        dim.h = Math.round(r.height)
        if (!userMoved) fitView()
      }
    }
    upd()
    ro = new ResizeObserver(upd)
    ro.observe(rootRef.value)
  }
})
onBeforeUnmount(() => ro?.disconnect())
</script>

<style scoped>
.mg-root {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 260px;
  display: flex;
  flex-direction: column;
  border-radius: 12px;
  background: linear-gradient(160deg, #f4f7ff 0%, #fdf4f7 55%, #eefbf6 100%);
  overflow: hidden;
}
.mg-svg { flex: 1; width: 100%; min-height: 0; display: block; touch-action: none; cursor: grab; }
.mg-svg:active { cursor: grabbing; }
.hub-ring { transition: stroke .2s; }
.hub { cursor: pointer; }
.hub-label { fill: #fff; font-size: 15px; font-weight: 800; text-anchor: middle; font-family: -apple-system, "PingFang SC", sans-serif; }
.hub-sub { fill: #fff; font-size: 11.5px; text-anchor: middle; opacity: .95; font-family: -apple-system, "PingFang SC", sans-serif; }
.hub-count { fill: #fff; font-size: 10px; text-anchor: middle; opacity: .8; }
.node { cursor: pointer; transition: opacity .2s; }
.node.dim { opacity: .22; }
.node.pick { filter: drop-shadow(0 2px 8px rgba(0,0,0,.14)); }
.node-label { font-size: 11.5px; fill: #4a5568; text-anchor: middle; pointer-events: none; font-family: -apple-system, "PingFang SC", sans-serif; }

/* 关联最强浮窗（右上，可拖拽） */
.rel-panel {
  position: absolute;
  z-index: 5;
  width: 200px;
  background: rgba(255,255,255,.9);
  border: 1px solid #e8eefc;
  border-radius: 12px;
  box-shadow: 0 8px 24px rgba(120,140,200,.18);
  overflow: hidden;
  backdrop-filter: blur(4px);
}
.rel-head { display: flex; align-items: center; justify-content: space-between; padding: 7px 10px; background: linear-gradient(90deg,#8ecdf5,#9a8ff0); color: #fff; font-size: 12px; font-weight: 700; cursor: grab; user-select: none; }
.rel-close { background: transparent; border: none; color: #fff; cursor: pointer; font-size: 13px; }
.rel-body { padding: 6px 8px; display: flex; flex-direction: column; gap: 4px; max-height: 160px; overflow-y: auto; }
.rel-item { display: flex; align-items: center; gap: 6px; font-size: 11.5px; color: #444; cursor: pointer; padding: 3px 4px; border-radius: 6px; }
.rel-item:hover { background: #f0f4ff; }
.rel-tag { flex-shrink: 0; color: #fff; font-size: 10px; font-weight: 700; border-radius: 5px; padding: 1px 5px; }
.rel-text { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.rel-strength { color: #a0aec0; font-size: 11px; }
.rel-empty { color: #a0aec0; font-size: 12px; padding: 6px; }

.mg-toolbar { display: flex; align-items: center; gap: 10px; padding: 6px 10px; border-top: 1px solid rgba(0,0,0,.05); }
.mg-btn { background: #6c7cff; border: none; color: #fff; border-radius: 8px; padding: 4px 12px; font-size: 12px; font-weight: 600; cursor: pointer; }
.mg-hint { font-size: 11px; color: #9aa7c4; }
.mg-zoom { margin-left: auto; font-size: 11px; color: #9aa7c4; }
</style>
