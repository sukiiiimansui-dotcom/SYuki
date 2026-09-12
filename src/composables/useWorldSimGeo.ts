// 「世界模拟」的两件基础设施：主题/皮肤开关 + 行政区划图的取图与缓存
//
// 分成两个 composable 导出，是因为它们的生命周期不同：
//   · useWorldSimTheme() —— 跟整个页面同寿命（换主题不该重取图）
//   · useWorldSimGeo()   —— 取图/缓存/请求竞态都在这里，页面切换级别时反复用
// 另外 useElementSize() 是给舞台量尺寸用的（为什么必须量：见 worldMap.ts 里
// 「尺寸必须跟着容器走」那段 —— 后端 SVG 的字号是固定 px，容器尺寸不传下去手机上就看不清）。

import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue'
import { geoSvgText, MAP_SVG_DEFAULT_H, MAP_SVG_DEFAULT_W } from '@/api/services/worldMap'
import {
  detectLowPerf,
  parseGeoRegions,
  sanitizeGeoMarkup,
  systemPrefersDark,
  type GeoRegion,
} from '@/components/views/worldsim/wsGeo'

/* ══════════════════════════════════════════════════════════════════
 * 一、主题 / 皮肤
 * ══════════════════════════════════════════════════════════════════ */

export type WorldSimTheme = 'mint' | 'glass'

/** 主题与深色偏好的存储键（P1 只落 localStorage，不进设置界面 —— 机主明确说了后续再接） */
const K_THEME = 'wsm:v1:theme'
const K_DARK = 'wsm:v1:dark'

function readLS(key: string): string {
  try {
    return localStorage.getItem(key) || ''
  } catch {
    return '' // 隐私模式 / 禁用存储：读不到就用默认值，不影响功能
  }
}
function writeLS(key: string, val: string) {
  try {
    localStorage.setItem(key, val)
  } catch {
    /* 写不进去就算了，主题只是观感，不该让页面报错 */
  }
}

/**
 * 皮肤：两套主题（薄荷奶油 / 现代简约毛玻璃）+ 深色。
 *
 * 三件事必须一起给到根节点，所以合成一个 `rootClass`：
 *   · `theme-mint` / `theme-glass`  —— 换变量表（worldsim.css）
 *   · `ws-dark` / `ws-light`        —— 深色开关。注意：**跟随系统**时用的是
 *     `ws-sys-dark`（也由这里打上），这样「系统深色」和「用户显式选深色」在 CSS 里
 *     是同一个规则的两段选择器，不用把深色变量抄两遍
 *   · `ws-perf-low`                 —— 低端机降级：关毛玻璃/关阴影/关装饰性动画
 *
 * @param opts.lowPerf P5-5：外部（`wsPerf`）算好的性能档位。传进来时以它为准，
 *   本函数内部的 `detectLowPerf()` 只当兜底 —— **单一事实来源**：
 *   `.ws-perf-low` 这个类与 JS 侧的气泡上限 / zoom 量化 / 错开精度必须来自同一个判定，
 *   否则会出现「CSS 说降级了、JS 还在满速跑」的分裂。
 */
export function useWorldSimTheme(opts: { lowPerf?: Ref<boolean> } = {}) {
  const theme = ref<WorldSimTheme>((readLS(K_THEME) as WorldSimTheme) === 'glass' ? 'glass' : 'mint')
  // 深色偏好：'' = 跟随系统；'dark' / 'light' = 用户显式指定
  const darkPref = ref<string>(readLS(K_DARK))
  const sysDark = ref(systemPrefersDark())
  const ownLow = ref(detectLowPerf())
  const lowPerf = opts.lowPerf || ownLow

  // 跟随系统时要能实时响应系统切换（用户在通知栏切深色模式，页面不该等刷新）
  let mq: MediaQueryList | null = null
  const onSys = (e: MediaQueryListEvent) => {
    sysDark.value = e.matches
  }
  if (typeof window !== 'undefined' && window.matchMedia) {
    try {
      mq = window.matchMedia('(prefers-color-scheme: dark)')
      // addEventListener 在很老的 Android WebView 上没有 → 退回废弃的 addListener
      if (mq.addEventListener) mq.addEventListener('change', onSys)
      else if ((mq as MediaQueryList & { addListener?: (f: (e: MediaQueryListEvent) => void) => void }).addListener) {
        ;(mq as MediaQueryList & { addListener: (f: (e: MediaQueryListEvent) => void) => void }).addListener(onSys)
      }
    } catch {
      /* 拿不到 matchMedia 就固定按亮色走，不影响功能 */
    }
  }
  onBeforeUnmount(() => {
    try {
      if (mq?.removeEventListener) mq.removeEventListener('change', onSys)
    } catch {
      /* 卸载时的清理失败无需处理 */
    }
  })

  /** 最终是不是深色 */
  const dark = computed(() => (darkPref.value ? darkPref.value === 'dark' : sysDark.value))

  const rootClass = computed(() => [
    `theme-${theme.value}`,
    darkPref.value === 'dark' ? 'ws-dark' : darkPref.value === 'light' ? 'ws-light' : sysDark.value ? 'ws-sys-dark' : 'ws-light',
    lowPerf.value ? 'ws-perf-low' : '',
  ])

  function setTheme(t: WorldSimTheme) {
    theme.value = t === 'glass' ? 'glass' : 'mint'
    writeLS(K_THEME, theme.value)
  }
  function cycleTheme() {
    setTheme(theme.value === 'mint' ? 'glass' : 'mint')
  }
  /** 深色三态循环：跟随系统 → 深 → 浅 → 跟随系统（不做设置界面，一个按钮够用） */
  function cycleDark() {
    const next = darkPref.value === '' ? 'dark' : darkPref.value === 'dark' ? 'light' : ''
    darkPref.value = next
    writeLS(K_DARK, next)
  }

  return { theme, dark, darkPref, sysDark, lowPerf, rootClass, setTheme, cycleTheme, cycleDark }
}

/* ══════════════════════════════════════════════════════════════════
 * 二、取图（行政区划 SVG）+ 区划清单
 * ══════════════════════════════════════════════════════════════════ */

/** 一次舞台取图的结果 */
export interface GeoStageData {
  adcode: string
  /** 裸 SVG 文本（已解析出 regions；清洗留给渲染时做） */
  svg: string
  /** 清洗过、可直接 v-html 的标记 */
  markup: string
  /** 图上可点击的区划（省 / 市 / 区县…） */
  regions: GeoRegion[]
  /** 后端耗时（进度提示用） */
  ms: number
}

/** 取图参数的默认值（与后端 render_geo 的默认画布一致） */
export const GEO_DEFAULT_W = MAP_SVG_DEFAULT_W
export const GEO_DEFAULT_H = MAP_SVG_DEFAULT_H

/**
 * 行政区划图取图器。
 *
 * 缓存策略：key = `ad|style|w桶|h桶|zoom`。尺寸进 key 是因为**后端按画布尺寸抽稀**
 * （容差 0.6px），尺寸变了顶点数就变；但没必要按像素缓存，按 128px 分桶足够
 * （同一台设备上容器尺寸基本固定，分桶只是防抖）。
 *
 * 竞态：用户连点面包屑时会同时发出多个请求 —— 用 `seq` 令牌，只有最后一次能被采用，
 * 先回来的旧结果直接丢掉（否则会出现「点回省级、画面却是市级的」错乱）。
 */
export function useWorldSimGeo() {
  const style = ref(readLS('wsm:v1:mapstyle') || 'gaode')
  const cache = new Map<string, GeoStageData>()
  let seq = 0

  function setStyle(s: string) {
    style.value = s === 'dark' || s === 'water' ? s : 'gaode'
    writeLS('wsm:v1:mapstyle', style.value)
  }

  const bucket = (n: number) => Math.round(Math.max(64, n || 0) / 128)

  function keyOf(ad: string, w: number, h: number, zoom: number) {
    return `${ad}|${style.value}|${bucket(w)}|${bucket(h)}|${zoom}`
  }

  /**
   * 取某一级行政区划图。
   *
   * @param ad    adcode（'100000' = 全国；省级轮廓就是这张图，见交付说明）
   * @param opts  w/h = 容器 CSS 像素；zoom 1~3；fresh=true 跳过缓存（重试用）
   */
  async function loadStage(
    ad: string,
    opts: { w?: number; h?: number; zoom?: number; fresh?: boolean } = {},
  ): Promise<GeoStageData | null> {
    const code = String(ad || '100000').trim() || '100000'
    const w = Math.max(120, Math.round(opts.w || GEO_DEFAULT_W))
    const h = Math.max(120, Math.round(opts.h || GEO_DEFAULT_H))
    const zoom = Math.min(3, Math.max(1, Math.round(opts.zoom ?? 2)))
    const key = keyOf(code, w, h, zoom)
    if (!opts.fresh) {
      const hit = cache.get(key)
      if (hit) return hit
    }
    const my = ++seq
    const t0 = Date.now()
    const svg = await geoSvgText(code, style.value, w, h, zoom)
    // 迟到的旧请求：丢弃（但结果仍然存缓存，下次点回来时秒开）
    const data: GeoStageData = {
      adcode: code,
      svg,
      markup: sanitizeGeoMarkup(svg),
      regions: parseGeoRegions(svg),
      ms: Date.now() - t0,
    }
    cache.set(key, data)
    if (my !== seq) return data // 调用方会判 adcode 是否还是当前所需
    return data
  }

  /**
   * 只要区划清单、不要图（三级联动列表用）。
   *
   * 用 320×240 的小画布：区划的 `<g data-adcode>` 一定都在，
   * 但顶点会被抽稀掉一大半 —— 列表场景不需要图，所以这一枪又快又小。
   */
  async function loadRegions(ad: string): Promise<GeoRegion[]> {
    const code = String(ad || '100000').trim() || '100000'
    const key = keyOf(code, 320, 240, 1)
    const hit = cache.get(key)
    if (hit) return hit.regions
    const svg = await geoSvgText(code, style.value, 320, 240, 1)
    const data: GeoStageData = {
      adcode: code,
      svg,
      markup: sanitizeGeoMarkup(svg),
      regions: parseGeoRegions(svg),
      ms: 0,
    }
    cache.set(key, data)
    return data.regions
  }

  /** 换主题色后旧图作废（三套地图主题的颜色是画进 SVG 里的，必须重取） */
  function invalidate() {
    cache.clear()
  }

  return { style, setStyle, loadStage, loadRegions, invalidate }
}

/* ══════════════════════════════════════════════════════════════════
 * 三、量舞台尺寸
 * ══════════════════════════════════════════════════════════════════ */

/**
 * 量元素的 CSS 像素尺寸。
 *
 * 与 WorldMap.vue 同样的兜底（那边踩过的坑这里不重踩）：
 *   · 首选 ResizeObserver（首次回调 = 布局就绪，立刻量，不等防抖）
 *   · 没有 RO 的老 WebView → 量一次 + 监听 window.resize
 *   · 一直量不到 → 超时后用默认画布出一张图（宁可字小，也别空着舞台）
 */
export function useElementSize(el: Ref<HTMLElement | null>, fallback = { w: GEO_DEFAULT_W, h: GEO_DEFAULT_H }) {
  const w = ref(0)
  const h = ref(0)
  let ro: ResizeObserver | null = null
  let timer: number | null = null
  let fired = false

  function measure() {
    const node = el.value
    if (!node) return
    const r = node.getBoundingClientRect()
    if (r.width > 0) w.value = Math.round(r.width)
    if (r.height > 0) h.value = Math.round(r.height)
  }

  function bind() {
    const node = el.value
    if (!node) return
    measure()
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', measure)
      return
    }
    ro = new ResizeObserver(() => {
      if (!fired) {
        fired = true
        measure()
        return
      }
      if (timer !== null) window.clearTimeout(timer)
      timer = window.setTimeout(measure, 200)
    })
    ro.observe(node)
  }

  function unbind() {
    if (timer !== null) {
      window.clearTimeout(timer)
      timer = null
    }
    window.removeEventListener('resize', measure)
    ro?.disconnect()
    ro = null
  }

  // 元素是 v-if 出来的（引导阶段舞台才出现）→ 出现/消失时自动挂载/卸载观察
  watch(el, (node, old) => {
    if (old) unbind()
    if (node) bind()
  }, { immediate: true })
  onBeforeUnmount(unbind)

  /** 给后端用的尺寸（量不到时用兜底值，绝不传 0 —— 0 会让 SVG 的 width/height 属性坏掉） */
  const sizeForBackend = computed(() => ({
    w: w.value > 0 ? w.value : fallback.w,
    h: h.value > 0 ? h.value : fallback.h,
  }))

  return { w, h, sizeForBackend, measure }
}
