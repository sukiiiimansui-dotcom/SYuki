// AI 实时绘制小区 —— SVG 增量绘制模块（DistrictLive.vue 的画笔）
//
// 为什么把画笔从组件里拆出来：
//   ① DistrictLive 要求「收到一条事件就画一个元素」，只能直接操作 DOM 节点：
//      走 v-for + 响应式数组会每条事件全量 diff/重渲染，几十栋楼就开始闪，
//      而且「生长动画」会因为节点复用而播不出来；
//   ② 这里全是纯函数 + 原生 DOM，不依赖 Vue 运行时 —— 可以用 Node + DOM 桩
//      离线回放真实 SSE 样本验证（scripts 里那套自检就是干这个的）。
//
// 坐标约定与后端 render.rs / Python 原型完全一致：
//   布局用「格」为单位，格子边长 cell = (900 - 2*48) / size，
//   左上角留白 ox/oy 居中 —— 这样前端增量画的图和后端出的成品图能对上。

import type { DistrictItem, DistrictRoadItem } from '@/api/services/worldMap'

const NS = 'http://www.w3.org/2000/svg'

/** 画布尺寸：与后端 render_probe 默认值一致，方便「导出 SVG」后能直接对照 */
export const STAGE_W = 900
export const STAGE_H = 900
export const STAGE_PAD = 48

/** 建筑类型 → 中文名（原型里同一张表，点选建筑时的信息面板用） */
export const TYPE_ZH: Record<string, string> = {
  residential: '住宅',
  office: '写字楼',
  commercial: '商场',
  shop: '店铺',
  restaurant: '餐饮',
  cafe: '咖啡',
  school: '学校',
  hospital: '医院',
  civic: '市政',
  leisure: '娱乐',
}

/** 建筑类型 → 填充色（浅色底图上的低饱和色，和原型一致） */
export const TYPE_COLOR: Record<string, string> = {
  residential: '#d6dce6',
  office: '#d2dae8',
  commercial: '#e6d7be',
  shop: '#ebdec3',
  restaurant: '#eed6ba',
  cafe: '#e8d6c4',
  school: '#c8dcf0',
  hospital: '#f0d2d2',
  civic: '#d8d6d0',
  leisure: '#e2d0e2',
}

/** 大尺度建筑：标注字号更大、字重更粗 */
const BIG_TYPES = ['office', 'commercial', 'shop', 'restaurant', 'cafe', 'school', 'hospital', 'civic', 'leisure']

/** 缩放上下文：格 → 像素 */
export interface Scale {
  size: number
  cell: number
  ox: number
  oy: number
}

/** 依据网格规模算缩放（size 变了要重算，否则图会画歪） */
export function makeScale(size: number): Scale {
  const s = Math.max(8, Number(size) || 20)
  const cell = Math.min((STAGE_W - 2 * STAGE_PAD) / s, (STAGE_H - 2 * STAGE_PAD) / s)
  return { size: s, cell, ox: (STAGE_W - cell * s) / 2, oy: (STAGE_H - cell * s) / 2 }
}

export const gx = (x: unknown, sc: Scale): number => sc.ox + (Number(x) || 0) * sc.cell
export const gy = (y: unknown, sc: Scale): number => sc.oy + (Number(y) || 0) * sc.cell

/** XML 转义：建筑/道路名是 AI 生成的，直接拼进 SVG 会破坏结构 */
export function escapeXml(s: unknown): string {
  return String(s ?? '').replace(/[&<>"']/g, (c) => {
    return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c] as string
  })
}

/** 舞台引用：四个分层 group + 标题/信息文本，增量绘制就往这些 group 里塞 */
export interface Stage {
  svg: SVGSVGElement
  gParks: SVGGElement
  gWater: SVGGElement
  gRoads: SVGGElement
  gBld: SVGGElement
  title: SVGTextElement
  info: SVGTextElement
}

function el<K extends keyof SVGElementTagNameMap>(tag: K): SVGElementTagNameMap[K] {
  return document.createElementNS(NS, tag)
}

function attr(node: Element, o: Record<string, string | number>): void {
  for (const [k, v] of Object.entries(o)) node.setAttribute(k, String(v))
}

/**
 * 动画/交互样式**内嵌进 SVG 本身**（不是写在组件 <style scoped> 里）。
 *
 * 原因：这些节点是 JS 直接创建的，拿不到 scoped 的 data-v 属性，
 * scoped 选择器根本命中不了；内嵌还顺带让「导出 SVG」拿到的是自带样式的成品。
 */
const SVG_CSS = `
.wm-bld{cursor:pointer}
.wm-bld rect{transition:filter .14s}
.wm-bld:hover rect{filter:brightness(1.14)}
.wm-bld.on rect{stroke:#ffd166;stroke-width:2.6}
@keyframes wmGrow{from{opacity:0;transform:scale(.4);transform-origin:center}to{opacity:1;transform:scale(1)}}
@keyframes wmFade{from{opacity:0}to{opacity:1}}
.wm-bld{animation:wmGrow .42s cubic-bezier(.34,1.56,.64,1) both}
.wm-road{animation:wmFade .3s ease both}
.wm-area{animation:wmFade .35s ease both}
`

/**
 * 建空舞台（每次重画调用一次）。
 * 骨架全部用 createElementNS 而不是 innerHTML —— 避开 SVG 片段解析的命名空间坑。
 */
export function createStage(host: HTMLElement, titleText = '绘制中…'): Stage {
  host.textContent = ''
  const svg = el('svg')
  attr(svg, {
    xmlns: NS,
    viewBox: `0 0 ${STAGE_W} ${STAGE_H}`,
    width: STAGE_W,
    height: STAGE_H,
    'data-zoom': 3,
  })

  const style = el('style')
  style.textContent = SVG_CSS
  svg.appendChild(style)

  const bg = el('rect')
  attr(bg, { width: STAGE_W, height: STAGE_H, fill: '#e9e7dc' })
  svg.appendChild(bg)

  const grid = el('rect')
  attr(grid, {
    x: STAGE_PAD,
    y: STAGE_PAD,
    width: STAGE_W - 2 * STAGE_PAD,
    height: STAGE_H - 2 * STAGE_PAD,
    fill: '#f2f0e8',
    rx: 6,
  })
  svg.appendChild(grid)

  const gParks = el('g')
  const gWater = el('g')
  const gRoads = el('g')
  const gBld = el('g')
  for (const g of [gParks, gWater, gRoads, gBld]) svg.appendChild(g)

  // 标题与右下角统计：画在最后 → 永远压在图形上面
  const title = el('text')
  attr(title, {
    x: STAGE_PAD - 8,
    y: STAGE_PAD - 18,
    'font-size': 17,
    'font-weight': 700,
    fill: '#3c4b5f',
    stroke: '#ffffff',
    'stroke-width': 3,
    'paint-order': 'stroke',
  })
  title.textContent = titleText
  const info = el('text')
  attr(info, {
    x: STAGE_W - STAGE_PAD + 8,
    y: STAGE_PAD - 18,
    'font-size': 12,
    'text-anchor': 'end',
    fill: '#8d97a6',
  })
  svg.appendChild(title)
  svg.appendChild(info)

  host.appendChild(svg)
  return { svg, gParks, gWater, gRoads, gBld, title, info }
}

export function setTitle(st: Stage, text: string): void {
  st.title.textContent = text || '小区'
}

export function setInfo(st: Stage, text: string): void {
  st.info.textContent = text
}

/** 序列化整张 SVG（导出/预览用） */
export function serializeStage(st: Stage): string {
  return new XMLSerializer().serializeToString(st.svg)
}

// ── 增量绘制：以下三个函数每次只创建「这一条事件」对应的节点 ──

/** 画一栋建筑 */
export function appendBuilding(st: Stage, item: DistrictItem, sc: Scale): SVGGElement {
  const typ = item.type || 'residential'
  const x = gx(item.x, sc)
  const y = gy(item.y, sc)
  const w = Math.max(6, (Number(item.w) || 1) * sc.cell)
  const h = Math.max(6, (Number(item.h) || 1) * sc.cell)
  const isBig = BIG_TYPES.indexOf(typ) >= 0 || (w > sc.cell * 2.4 && h > sc.cell * 1.8)
  const fill = TYPE_COLOR[typ] || TYPE_COLOR.residential

  const g = el('g')
  g.setAttribute('class', 'wm-bld')
  g.dataset.type = typ
  g.dataset.name = item.name || ''
  g.dataset.floors = item.floors === undefined || item.floors === null ? '' : String(item.floors)
  g.dataset.w = String(item.w ?? '')
  g.dataset.h = String(item.h ?? '')

  const shadow = el('rect')
  attr(shadow, { x: x + 2, y: y + 3, width: w, height: h, rx: 3, fill: 'rgba(60,75,95,.18)' })
  const face = el('rect')
  attr(face, { x, y, width: w, height: h, rx: 3, fill, stroke: '#9aa7b8', 'stroke-width': 1.4 })
  g.appendChild(shadow)
  g.appendChild(face)

  const nm = (item.name || '').trim()
  if (nm) {
    const fs = isBig ? 13 : 11
    const maxw = w - 4
    let label = nm
    // 标不完就截断：宁可少两个字，也不要字压到邻居楼上
    while (label.length > 2 && label.length * fs > maxw) label = label.slice(0, -1)
    if (label.length * fs <= maxw || label.length <= 2) {
      const t = el('text')
      attr(t, {
        x: x + w / 2,
        y: y + h / 2 + fs * 0.36,
        'font-size': fs,
        'text-anchor': 'middle',
        fill: '#3c4b5f',
        stroke: '#ffffff',
        'stroke-width': 2.6,
        'paint-order': 'stroke',
        'font-weight': isBig ? 700 : 500,
      })
      t.textContent = label
      g.appendChild(t)
    }
  }
  if (item.floors && sc.cell > 16) {
    const t = el('text')
    attr(t, {
      x: x + w - 3,
      y: y + h - 3,
      'font-size': 9.5,
      'text-anchor': 'end',
      fill: '#8d97a6',
      stroke: '#ffffff',
      'stroke-width': 2,
      'paint-order': 'stroke',
    })
    t.textContent = `${item.floors}F`
    g.appendChild(t)
  }

  st.gBld.appendChild(g)
  return g
}

/** 画一条路（含路名）；道路永远压在建筑下面 —— 所以只 append，不提层 */
export function appendRoad(st: Stage, item: DistrictRoadItem, sc: Scale): SVGGElement {
  const typ = item.type || 'secondary'
  const x1 = gx(item.x1, sc)
  const y1 = gy(item.y1, sc)
  const x2 = gx(item.x2, sc)
  const y2 = gy(item.y2, sc)
  const w = Math.max(4, sc.cell * (typ === 'main' ? 0.62 : typ === 'secondary' ? 0.4 : 0.2))
  const edge = typ === 'path' ? '#f5f3ec' : '#e2dfd4'
  const face = typ === 'path' ? '#f5f3ec' : '#ffffff'

  const g = el('g')
  g.setAttribute('class', 'wm-road')
  if (typ !== 'path') {
    const l = el('line')
    attr(l, { x1, y1, x2, y2, stroke: edge, 'stroke-width': w + 3, 'stroke-linecap': 'round' })
    g.appendChild(l)
  }
  const l2 = el('line')
  attr(l2, { x1, y1, x2, y2, stroke: face, 'stroke-width': w, 'stroke-linecap': 'round' })
  if (typ === 'path') l2.setAttribute('stroke-dasharray', '6 5')
  g.appendChild(l2)
  if (typ === 'main') {
    const l3 = el('line')
    attr(l3, {
      x1,
      y1,
      x2,
      y2,
      stroke: '#e8c85a',
      'stroke-width': 1.6,
      'stroke-dasharray': '9 7',
      opacity: 0.85,
    })
    g.appendChild(l3)
  }
  st.gRoads.appendChild(g)

  const nm = (item.name || '').trim()
  if (nm) {
    const mx = (x1 + x2) / 2
    const my = (y1 + y2) / 2
    const horiz = Math.abs(x2 - x1) >= Math.abs(y2 - y1)
    const fs = 12
    const tw = nm.length * fs
    const tg = el('g')
    tg.setAttribute('class', 'wm-road')
    tg.setAttribute('transform', `translate(${mx},${my}) rotate(${horiz ? 0 : 90})`)
    const bg = el('rect')
    attr(bg, { x: -tw / 2 - 4, y: -fs * 0.78, width: tw + 8, height: fs * 1.5, rx: 4, fill: '#e9e7dc', opacity: 0.82 })
    const t = el('text')
    attr(t, { x: 0, y: fs * 0.36, 'font-size': fs, 'text-anchor': 'middle', fill: '#6e7889', 'font-weight': 600 })
    t.textContent = nm
    tg.appendChild(bg)
    tg.appendChild(t)
    st.gRoads.appendChild(tg)
  }
  return g
}

/** 画一块绿地 / 水面 */
export function appendArea(st: Stage, item: DistrictItem, kind: 'park' | 'water', sc: Scale): SVGGElement {
  const x = gx(item.x, sc)
  const y = gy(item.y, sc)
  const w = Math.max(6, (Number(item.w) || 2) * sc.cell)
  const h = Math.max(6, (Number(item.h) || 2) * sc.cell)
  const isPark = kind === 'park'

  const g = el('g')
  g.setAttribute('class', 'wm-area')
  const r = el('rect')
  attr(r, {
    x,
    y,
    width: w,
    height: h,
    rx: isPark ? 8 : 10,
    fill: isPark ? '#c8e1be' : '#bed7f0',
    stroke: isPark ? '#a9cf9c' : '#93b8e0',
    'stroke-width': 1.5,
  })
  g.appendChild(r)
  const nm = (item.name || '').trim()
  if (nm) {
    const t = el('text')
    attr(t, {
      x: x + w / 2,
      y: y + h / 2 + 5,
      'font-size': 12,
      'text-anchor': 'middle',
      fill: '#3c4b5f',
      stroke: '#ffffff',
      'stroke-width': 2.4,
      'paint-order': 'stroke',
      'font-weight': 600,
    })
    t.textContent = nm
    g.appendChild(t)
  }
  ;(isPark ? st.gParks : st.gWater).appendChild(g)
  return g
}

/**
 * 整份布局重画。
 *
 * 唯一的「全量」路径，只在**网格规模中途变了**的时候用：
 * 那时已画的元素坐标是按旧 cell 算的，不重画就会错位；
 * 重画会重播一次生长动画，属于可接受的代价（正常情况 size 事件早于任何元素）。
 */
export function replayLayout(st: Stage, layout: { buildings?: DistrictItem[]; roads?: DistrictRoadItem[]; parks?: DistrictItem[]; water?: DistrictItem[] }, sc: Scale): number {
  st.gParks.textContent = ''
  st.gWater.textContent = ''
  st.gRoads.textContent = ''
  st.gBld.textContent = ''
  let n = 0
  for (const p of layout.parks || []) {
    appendArea(st, p, 'park', sc)
    n++
  }
  for (const p of layout.water || []) {
    appendArea(st, p, 'water', sc)
    n++
  }
  for (const r of layout.roads || []) {
    appendRoad(st, r, sc)
    n++
  }
  for (const b of layout.buildings || []) {
    appendBuilding(st, b, sc)
    n++
  }
  return n
}
