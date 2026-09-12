// 「世界模拟」纯逻辑工具（不 import vue、不碰网络）
//
// 为什么把这类东西单独拆一个文件：
//   ① 解析 SVG 文本、算 adcode 层级、比对搜索结果 —— 这些都是**纯函数**，
//      拆出来才能用 node 直接跑自检（见交付说明里的自检脚本），不必启动页面；
//   ② 组件与 composable 只负责「什么时候调」，逻辑本身不掺响应式，好读也好改。
//
// 依赖：只依赖后端 render_geo 的输出格式（`<g class="geo-region" data-adcode data-name>`）。
// 那个格式是 Rust 侧 `render_geo.rs` 与 Python 侧 `svg_geo.py` **两边一致**的契约
// （写在 render_geo.rs 开头的模块注释里：每个区划包在 geo-region 里，前端据此点击下钻）。

/** 一个可点击的行政区划（从 geo_svg 文本里读出来的） */
export interface GeoRegion {
  adcode: string
  name: string
}

/** 行政级别，取值规则与 Rust `geo.rs::level_of` **逐字对齐**（别处别再抄一遍规则） */
export type GeoLevel = 'country' | 'province' | 'city' | 'district' | 'street'

/**
 * adcode → 行政级别。
 *
 * 与 Rust 一致：空/100000 → country；`xx0000` → province；`xx0000` 之外的 `xxxx00` → city；
 * 其余 → district。注意**直辖市**是特例：北京市（110000）的子级直接就是区（110101…），
 * 所以流程里绝不能写死「省→市→区」三级，只能按「当前区域的子级」一层层走（见 useWorldSim）。
 */
export function geoLevelOf(adcode: string): GeoLevel {
  const a = String(adcode || '').trim()
  if (!a || a === '100000' || a === '0') return 'country'
  if (a.endsWith('0000')) return 'province'
  if (a.endsWith('00')) return 'city'
  return 'district'
}

export function levelLabel(level: GeoLevel): string {
  switch (level) {
    case 'country':
      return '全国'
    case 'province':
      return '省'
    case 'city':
      return '市'
    case 'district':
      return '区县'
    default:
      return '街镇'
  }
}

/**
 * 从 geo_svg 文本里抠出这张图上的所有区划（去重、丢掉没名字的）。
 *
 * 为什么要按 `<g ...>` 标签整体解析、而不是直接匹配
 * `data-adcode="..." data-name="..."` 这两个属性的相邻顺序：
 * 属性顺序是渲染器**当前**的写法，将来加一个字段就可能变；按标签取更耐改。
 *
 * 没名字的要素要丢掉：DataV 的全国数据里有一条 `100000_JD`（九段线），
 * 它不是一个省，点它没有任何意义（Python 侧渲染器会把它也包成 geo-region，
 * Rust 侧直接跳过，这里统一兜住）。
 */
export function parseGeoRegions(svg: string): GeoRegion[] {
  const out: GeoRegion[] = []
  const seen = new Set<string>()
  const tagRe = /<g\b[^>]*class="geo-region"[^>]*>/g
  let m: RegExpExecArray | null
  while ((m = tagRe.exec(String(svg || '')))) {
    const tag = m[0]
    const ad = /data-adcode="([^"]*)"/.exec(tag)
    const nm = /data-name="([^"]*)"/.exec(tag)
    const adcode = unescapeXml(ad ? ad[1] : '')
    const name = unescapeXml(nm ? nm[1] : '')
    if (!adcode || !name.trim()) continue
    if (seen.has(adcode)) continue
    seen.add(adcode)
    out.push({ adcode, name: name.trim() })
  }
  return out
}

/** XML 实体还原（渲染器对 &<>" 做了转义；地名里出现 `&` 的概率虽低，也不该显示成 `&amp;`） */
export function unescapeXml(s: string): string {
  return String(s || '')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&')
}

/**
 * 清洗后端 SVG，准备用 `v-html` 注进页面。
 *
 * **必须剥掉 `<style>`**：SVG 里那段 CSS 是给「整篇 SVG 文档」写的，
 * 一旦用 v-html 注入就变成**整页全局样式** —— 里面有 `text{font-family:...}`、
 * `.z1/.z2/.z3` 这类极短的类名，会顺着样式表漏到 LingChat 别的界面上。
 * 我们要的视觉效果（悬停、高亮、按 zoom 隐藏图层）全部改由 `worldsim.css`
 * 里的 `.ws-geo :deep(...)` 承担 —— 那才是**页面级、可审查**的作用域。
 *
 * `<script>` 同理（后端不会输出，但注入前一律清掉，这是纪律不是洁癖）。
 */
export function sanitizeGeoMarkup(svg: string): string {
  return String(svg || '')
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/<style[\s\S]*?<\/style>/gi, '')
}

/**
 * SVG 文本 → 可直接塞 `<img src>` 的 data URL。
 *
 * 与 `worldMap.ts` 里那个同名私有函数**逐字一致**（那边不导出，且本次改动纪律是
 * 「只在末尾追加、不改既有函数」，所以这里留一份 4 行的副本，注释指回源头）。
 * 用 encodeURIComponent 而不是 base64：`#`（颜色）、中文地名、`<>&` 都要转义，
 * 否则 data URL 会在第一个 `#` 处被截断成「只画了一半」的图。
 */
export function svgToDataUrl(svg: string): string {
  const text = String(svg || '').trim()
  if (!text.startsWith('<')) throw new Error('后端没有返回 SVG')
  return 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(text)
}

/**
 * 稳定 32 位哈希（FNV-1a 变体），给「小区草图」当 seed 用。
 *
 * 为什么不用 Date.now() 或 Math.random()：草图必须**同区县永远同一张**（可复现、可缓存），
 * 用户切走再回来不该看到另一张图。
 * 为什么是 32 位：Rust 侧 seed 是 u64，而 JSON 数字进 JS 只有 2^53 精度，
 * 超过就丢精度、两边算出的草图会悄悄不一样 —— 卡在 32 位绝对安全。
 */
export function hash32(s: string): number {
  let h = 0x811c9dc5
  const str = String(s || '')
  for (let i = 0; i < str.length; i++) {
    h ^= str.charCodeAt(i)
    h = Math.imul(h, 0x01000193)
  }
  return h >>> 0
}

/**
 * 面包屑/小区名用的文字：「广州市·越秀区」。
 *
 * 从第 2 项开始拼（第 1 项永远是「中国」，说「中国·广州市·越秀区」啰嗦且没信息量）。
 * 直辖市会自然变成「北京市·朝阳区」（只有两级），不用特判。
 */
export function buildAreaLabel(path: GeoRegion[], startAt = 1): string {
  const names = (path || [])
    .slice(startAt)
    // ⚠️ **纯 6 位数字的名字是 adcode，不是地名**：有些层级（例如直辖市那一跳）
    // 后端只给了编码没给名字，直接拼出来就是「重庆市·500100·500102」——
    // 把一串编码甩给用户既没用又吓人。宁可少一段，也不露编码。
    .map((p) => {
      const raw = p && p.name ? String(p.name).trim() : ''
      return /^\d{6}$/.test(raw) ? '' : raw
    })
    .filter(Boolean)
  return names.join('·')
}

/** 去掉路径里的重复与空项（后端给的 path 偶尔会带同名父级，拼出来会「广东省·广东省」） */
export function dedupePath(path: GeoRegion[]): GeoRegion[] {
  const out: GeoRegion[] = []
  for (const p of path || []) {
    if (!p || !p.adcode || !p.name) continue
    if (out.length && out[out.length - 1].adcode === p.adcode) continue
    out.push({ adcode: String(p.adcode), name: String(p.name) })
  }
  return out
}

/**
 * 搜索结果（三级联动上面的那个「搜索框纠偏」）。
 *
 * 排序规则（都是为了让「越秀」这种半截输入第一眼就命中）：
 *   ① 完全相等 > ② 前缀命中 > ③ 包含命中；同级内保持原始顺序（省市区本身的顺序有意义）。
 * 大小写不敏感；adcode 前缀也认（用户从别处抄来一串编码时能直接用）。
 */
export function matchRegions(list: GeoRegion[], query: string, limit = 12): GeoRegion[] {
  const q = String(query || '').trim().toLowerCase()
  if (!q) return []
  const score = (r: GeoRegion): number => {
    const n = r.name.toLowerCase()
    const a = r.adcode.toLowerCase()
    if (n === q || a === q) return 0
    if (n.startsWith(q) || a.startsWith(q)) return 1
    if (n.includes(q)) return 2
    return -1
  }
  const hit: { r: GeoRegion; s: number; i: number }[] = []
  list.forEach((r, i) => {
    const s = score(r)
    if (s >= 0) hit.push({ r, s, i })
  })
  hit.sort((x, y) => (x.s - y.s) || (x.i - y.i))
  return hit.slice(0, limit).map((h) => h.r)
}

/**
 * 这台机器算不算低端（决定要不要加 `.ws-perf-low`）。
 *
 * 说明：方案文档里提到「复用 LingChat 的 autoConfigurePerformance」，但**本仓库里没有**
 * 这个函数（全仓 grep 无命中，应该是官方更新版才有的东西），所以这里按同一思路做个轻量版：
 * 只看并发核数 / 设备内存这两个到处都有的信号，拿不到就当高端（宁可多开动画，也别误降级）。
 */
export function detectLowPerf(): boolean {
  if (typeof navigator === 'undefined') return false
  const nav = navigator as Navigator & { deviceMemory?: number }
  const cores = Number(nav.hardwareConcurrency || 0)
  const mem = Number(nav.deviceMemory || 0)
  if (cores > 0 && cores <= 4) return true
  if (mem > 0 && mem <= 3) return true
  return false
}

/** 系统是否要求「减少动态效果」 */
export function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  try {
    return window.matchMedia('(prefers-reduced-motion: reduce)').matches
  } catch {
    return false
  }
}

/** 系统当前是不是深色（跟随系统那一路就是靠它；LingChat 主题那一路由调用方显式传 dark） */
export function systemPrefersDark(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  try {
    return window.matchMedia('(prefers-color-scheme: dark)').matches
  } catch {
    return false
  }
}

/** 把毫秒数说成人话（进度卡上的「已用 1.4s」） */
export function fmtMs(ms: number): string {
  const n = Math.max(0, Number(ms) || 0)
  if (n < 1000) return `${Math.round(n)}ms`
  return `${(n / 1000).toFixed(1)}s`
}

/**
 * 从 adcode 反推上级链（与 Rust `live.rs::admin_chain` 同规则，供「手动选城市」时
 * 把用户选的区县补成一条完整路径用）。用的是行政区划编码的**层级前缀制**，
 * 不依赖任何查询表：440103 → 440000 / 440100 / 440103。
 */
export function adminChain(adcode: string): string[] {
  const a = String(adcode || '').trim()
  if (a.length !== 6 || !/^\d+$/.test(a)) return a ? [a] : []
  const province = a.slice(0, 2) + '0000'
  const city = a.slice(0, 4) + '00'
  const out = [province]
  if (city !== province) out.push(city)
  if (a !== city && a !== province) out.push(a)
  return out
}
