// 「世界模拟」小区增量画笔（自己写的一小份，不 import worldmap/** 里的页面与模块）
//
// 为什么自己写、而不是直接复用 `worldmap/DistrictLive.vue` 那套：
//   ① 那个文件是**页面**，import 页面等于把它的样式、状态、连接管理一起拖进来；
//   ② 它在 worldmap/** 目录下 —— 本次改动的文件所有权把那个目录划给了另一个 agent，
//      「不修改」也意味着不该依赖它的内部函数（它一改我就崩）；
//   ③ 我这边要的画法其实更简单：把流式收到的元素攒起来，重算一份 SVG 文本即可
//      （Vue 用 v-html 渲染，新增的图元靠 CSS 自己「弹」出来，见 worldsim.css 的 ws-pop）。
//
// 数据形状与后端 `DistrictStreamEvent` 完全一致（x/y/w/h/type/name/floors、道路两端点），
// 所以两条通路（Tauri Channel / 浏览器 EventSource）都直接吃。

/** 建筑 / 绿地 / 水域的图元（坐标系：0..size 的格子） */
export interface PaintItem {
  x?: number
  y?: number
  w?: number
  h?: number
  type?: string
  name?: string
  floors?: number | string
}

/** 道路图元 */
export interface PaintRoad {
  x1?: number
  y1?: number
  x2?: number
  y2?: number
  type?: string
  name?: string
}

/** 一套配色（值抄自 Rust `render.rs::style_of`，保证 AI 精绘与草图/行政区划是同一套视觉语言） */
export interface PaintPalette {
  bg: string
  blockbg: string
  park: string
  parkEdge: string
  water: string
  waterEdge: string
  roadMain: string
  roadSec: string
  roadPath: string
  roadEdge: string
  text: string
  textHalo: string
  bldEdge: string
  shadow: string
  /** 建筑类型 → 填充色 */
  bld: Record<string, string>
}

/** 三个主题的配色（键名与 Rust 侧 style 名一致：gaode / dark / water） */
export function paletteOf(style: string): PaintPalette {
  if (style === 'dark') {
    return {
      bg: '#121a26', blockbg: '#18222f',
      park: '#1e4637', parkEdge: '#2a5c48', water: '#19375a', waterEdge: '#2a5a8c',
      roadMain: '#3c5070', roadSec: '#2a3a52', roadPath: '#232f42', roadEdge: '#26364e',
      text: '#c8dcf0', textHalo: '#0e1620', bldEdge: '#5a8cbe', shadow: 'rgba(0,0,0,.35)',
      bld: {
        residential: '#283c58', office: '#374665', commercial: '#3c4b46', shop: '#46503f',
        restaurant: '#554442', cafe: '#504640', school: '#32506e', hospital: '#5a3c46',
        civic: '#3a4254', leisure: '#483c54',
      },
    }
  }
  if (style === 'water') {
    return {
      bg: '#f4efe3', blockbg: '#faf5e8',
      park: '#dce8cc', parkEdge: '#bcd0a8', water: '#cfe3ef', waterEdge: '#a8c8dc',
      roadMain: '#fdfaf2', roadSec: '#f6f0e2', roadPath: '#f0ead8', roadEdge: '#e0d8c4',
      text: '#6a6252', textHalo: '#fdfaf2', bldEdge: '#a89c88', shadow: 'rgba(120,110,90,.18)',
      bld: {
        residential: '#e0d8c8', office: '#d4d4cc', commercial: '#e8d8b8', shop: '#eee0c4',
        restaurant: '#e8d0b0', cafe: '#e4d4bc', school: '#d0dce4', hospital: '#e8d0d0',
        civic: '#dcd8d0', leisure: '#e0d4dc',
      },
    }
  }
  return {
    bg: '#e9e7dc', blockbg: '#f2f0e8',
    park: '#c8e1be', parkEdge: '#a9cf9c', water: '#bed7f0', waterEdge: '#93b8e0',
    roadMain: '#ffffff', roadSec: '#fbfaf5', roadPath: '#f5f3ec', roadEdge: '#e2dfd4',
    text: '#3c4b5f', textHalo: '#ffffff', bldEdge: '#9aa7b8', shadow: 'rgba(60,75,95,.18)',
    bld: {
      residential: '#d6dce6', office: '#d2dae8', commercial: '#e6d7be', shop: '#ebdec3',
      restaurant: '#eed6ba', cafe: '#e8d6c4', school: '#c8dcf0', hospital: '#f0d2d2',
      civic: '#d8d6d0', leisure: '#e2d0e2',
    },
  }
}

export interface PaintCounts {
  buildings: number
  roads: number
  parks: number
  water: number
}

/** 上限：AI 输出不可控，攒到一定量就不再收（超出的只计数不画，进度卡上如实说明） */
const MAX_BUILDINGS = 900
const MAX_ROADS = 400
const MAX_PARK = 80
const MAX_WATER = 80

function esc(s: string): string {
  return String(s || '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function num(v: unknown, d = 0): number {
  const n = Number(v)
  return Number.isFinite(n) ? n : d
}

/**
 * 增量绘制模型：一条流对应一个实例。
 *
 * 用**可变对象 + 计数**而不是响应式数组：流可能一次推几百条，
 * 让 Vue 逐条 diff 反而慢；这里由页面按帧节流调用 `toSvg()` 整份重建
 * （SVG 文本本身很小，重建一次是微秒级）。
 */
export class DistrictPaint {
  /** 网格边长（后端 size，20/28/36…） */
  readonly size: number
  buildings: PaintItem[] = []
  roads: PaintRoad[] = []
  parks: PaintItem[] = []
  water: PaintItem[] = []
  /** 因为超过上限被丢掉的条数（如实报给用户，不假装全画了） */
  dropped = 0

  constructor(size = 28) {
    this.size = Math.max(8, num(size, 28))
  }

  addBuilding(it: PaintItem) {
    if (this.buildings.length >= MAX_BUILDINGS) { this.dropped++; return }
    this.buildings.push(it)
  }
  addRoad(it: PaintRoad) {
    if (this.roads.length >= MAX_ROADS) { this.dropped++; return }
    this.roads.push(it)
  }
  addPark(it: PaintItem) {
    if (this.parks.length >= MAX_PARK) { this.dropped++; return }
    this.parks.push(it)
  }
  addWater(it: PaintItem) {
    if (this.water.length >= MAX_WATER) { this.dropped++; return }
    this.water.push(it)
  }

  counts(): PaintCounts {
    return {
      buildings: this.buildings.length,
      roads: this.roads.length,
      parks: this.parks.length,
      water: this.water.length,
    }
  }

  get total(): number {
    return this.buildings.length + this.roads.length + this.parks.length + this.water.length
  }

  clear() {
    this.buildings = []
    this.roads = []
    this.parks = []
    this.water = []
    this.dropped = 0
  }

  /**
   * 换成一份完整布局（`done` 事件里的那份）。
   *
   * 为什么要换：流式收的过程中偶尔会丢片段（网络/解析），最终布局才是权威。
   * 直接替换而不是「合并」，避免出现两份重复的楼。
   */
  loadLayout(layout: { size?: number; buildings?: PaintItem[]; roads?: PaintRoad[]; parks?: PaintItem[]; water?: PaintItem[] }) {
    this.clear()
    for (const b of layout?.buildings || []) this.addBuilding(b)
    for (const r of layout?.roads || []) this.addRoad(r)
    for (const p of layout?.parks || []) this.addPark(p)
    for (const w of layout?.water || []) this.addWater(w)
  }

  /**
   * 生成整份 SVG 文本（给 v-html）。
   *
   * viewBox 用 `0 0 size size`，坐标 1:1 —— 这样流里给的格子坐标不用任何换算，
   * 缩放交给 CSS（`width:100%`），手机上也是清晰的矢量。
   */
  toSvg(p: PaintPalette, opts: { showNames?: boolean } = {}): string {
    const s = this.size
    const w0 = (k: number) => Math.max(0.05, s * k)
    const parts: string[] = []
    parts.push(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${s} ${s}" class="ws-paint" role="img">`)
    // 底色 + 街区底
    parts.push(`<rect x="0" y="0" width="${s}" height="${s}" fill="${p.bg}"/>`)
    parts.push(`<rect x="${s * 0.04}" y="${s * 0.04}" width="${s * 0.92}" height="${s * 0.92}" rx="${s * 0.02}" fill="${p.blockbg}"/>`)

    // 绿地 / 水域（先画，压在路和楼下面）
    for (const k of this.parks) {
      const x = num(k.x), y = num(k.y), w = Math.max(0.4, num(k.w, 1)), h = Math.max(0.4, num(k.h, 1))
      parts.push(`<rect class="ws-b" x="${x}" y="${y}" width="${w}" height="${h}" rx="${w0(0.05)}" fill="${p.park}" stroke="${p.parkEdge}" stroke-width="${w0(0.012)}"/>`)
    }
    for (const k of this.water) {
      const x = num(k.x), y = num(k.y), w = Math.max(0.4, num(k.w, 1)), h = Math.max(0.4, num(k.h, 1))
      parts.push(`<rect class="ws-b" x="${x}" y="${y}" width="${w}" height="${h}" rx="${w0(0.08)}" fill="${p.water}" stroke="${p.waterEdge}" stroke-width="${w0(0.012)}"/>`)
    }

    // 道路：先描边色打底（宽一点），再盖一层路面色 —— 就有「路缘」了
    const roadW = (t?: string) => (t === 'main' ? w0(0.055) : t === 'secondary' ? w0(0.034) : w0(0.018))
    for (const r of this.roads) {
      const x1 = num(r.x1), y1 = num(r.y1), x2 = num(r.x2), y2 = num(r.y2)
      const w = roadW(r.type)
      parts.push(`<line class="ws-t" x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${p.roadEdge}" stroke-width="${(w + w0(0.012)).toFixed(3)}" stroke-linecap="round"/>`)
      parts.push(`<line class="ws-t" x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${r.type === 'main' ? p.roadMain : r.type === 'secondary' ? p.roadSec : p.roadPath}" stroke-width="${w.toFixed(3)}" stroke-linecap="round"/>`)
    }

    // 建筑：投影 + 本体 + 名字（名字只给够大的楼，小楼上写字只会糊成一团）
    const showNames = opts.showNames !== false
    for (const b of this.buildings) {
      const x = num(b.x), y = num(b.y)
      const w = Math.max(0.35, num(b.w, 1))
      const h = Math.max(0.35, num(b.h, 1))
      const fill = p.bld[String(b.type || 'residential')] || p.bld.residential || '#d6dce6'
      parts.push(`<rect x="${(x + w0(0.012)).toFixed(3)}" y="${(y + w0(0.012)).toFixed(3)}" width="${w}" height="${h}" fill="${p.shadow}" rx="${w0(0.01)}"/>`)
      parts.push(`<rect class="ws-b" x="${x}" y="${y}" width="${w}" height="${h}" fill="${fill}" stroke="${p.bldEdge}" stroke-width="${w0(0.008)}" rx="${w0(0.01)}"/>`)
      const name = String(b.name || '').trim()
      if (showNames && name && w * h >= 5.5) {
        const fs = Math.max(0.42, Math.min(w0(0.032), w / (name.length * 0.62)))
        parts.push(
          `<text x="${(x + w / 2).toFixed(3)}" y="${(y + h / 2 + fs * 0.36).toFixed(3)}" font-size="${fs.toFixed(3)}" ` +
            `text-anchor="middle" fill="${p.text}" stroke="${p.textHalo}" stroke-width="${w0(0.006)}" paint-order="stroke">${esc(name.slice(0, 8))}</text>`,
        )
      }
    }

    parts.push('</svg>')
    return parts.join('')
  }
}
