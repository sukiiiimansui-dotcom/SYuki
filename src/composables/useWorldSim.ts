// 「世界模拟」首次引导的状态机（P1 的核心）
//
// 为什么单独抽一个 composable：
//   · 引导流程有 **10 个状态**、还要落 localStorage（只走一次），塞进组件里会变成
//     一堆互相纠缠的 ref；这里把「状态怎么流转」与「页面长什么样」彻底分开；
//   · 页面（WorldSim.vue）只做两件事：把 state 画出来、把用户动作转发进来。
//
// ── 状态流转（与 docs/world-map/13 的 P1 一节逐条对应）──────────────────────
//
//                         ┌──────────────────────────────┐
//                         │ boot 加载动画「正在展开世界…」│←──────────┐
//                         └───────────────┬──────────────┘           │
//                    已引导过 │                     │ 首次             │ restart()
//                            ▼                     ▼                 │
//                 ┌────────────────┐      ┌──────────────────┐        │
//                 │ neighborhood   │      │ locating 定位中   │        │
//                 │ 直接进上次位置 │      └────────┬─────────┘        │
//                 └────────────────┘       成功 │      │ 被拒/失败    │
//                                               ▼      ▼             │
//                              ┌──────────────────┐  ┌──────────────────────────┐
//                              │ country 全国省级 │  │ locateFailed             │
//                              │ 轮廓（只高亮，等│  │ 手动选城市 / 用 IP 估测  │
//                              │ 用户点「进入」）│  └───┬──────────────┬───────┘
//                              └────────┬─────────┘      │手动           │IP
//                                 点「进入 省」           ▼              │
//                                       ▼          ┌───────────┐        │
//                              ┌────────────────┐  │ manual    │        │
//                              │ province 省图  │  │ 三级联动+ │        │
//                              │ 定位到市→高亮  │  │ 搜索框纠偏│        │
//                              └────────┬───────┘  └─────┬─────┘        │
//                                 点「进入 市」           │选到区县       │
//                                       ▼                │              │
//                              ┌────────────────┐        │              │
//                              │ city 市图      │        │              │
//                              └────────┬───────┘        │              │
//                                 自动弹一次确认          │              │
//                                       ▼                │              │
//                              ┌────────────────┐  否    │              │
//                              │ confirm        ├────────┘              │
//                              │ 「你现在在 X 吗」│                      │
//                              └────────┬───────┘                       │
//                                    是 │                               │
//                                       ▼                               │
//                              ┌────────────────────┐                   │
//                              │ district 下钻到区县│                   │
//                              └────────┬───────────┘                   │
//                                 点「生成小区地图」                     │
//                                       ▼                               │
//                              ┌──────────────────────────┐             │
//                              │ neighborhood 小区地图    │             │
//                              │ 草图秒出 → AI 精绘淡入   │             │
//                              └──────────────────────────┘             │
//
// 面包屑（国→省→市→区县→小区）**任意一级都能点回去**：backTo(i) 只改 cursor，
// 不删掉更深的路径 —— 用户退回省级看一眼，还能一键再跳回区县。

import { computed, ref, type Ref } from 'vue'
import { errTextKey } from '@/components/views/worldsim/wsErr'
import worldMapApi, { type WorldLocation } from '@/api/services/worldMap'
import {
  adminChain,
  buildAreaLabel,
  dedupePath,
  geoLevelOf,
  type GeoLevel,
  type GeoRegion,
} from '@/components/views/worldsim/wsGeo'
import type { GeoStageData, useWorldSimGeo } from '@/composables/useWorldSimGeo'

/** 状态机的全部状态 */
export type SimStep =
  | 'boot'
  | 'locating'
  | 'locateFailed'
  | 'country'
  | 'province'
  | 'city'
  | 'confirm'
  | 'manual'
  | 'district'
  | 'neighborhood'

/** 面包屑的一项（`kind='neigh'` 是虚拟的「小区」那一级，不是行政区划） */
export interface Crumb {
  key: string
  adcode: string
  name: string
  index: number
  level: GeoLevel
  active: boolean
  kind: 'geo' | 'neigh'
}

/** 落盘的「上次位置」 */
interface SavedState {
  onboarded: boolean
  path: GeoRegion[]
  hitAd: string
  source: string
  ts: number
}

const K_STATE = 'wsm:v1:state'

/** 首屏加载动画的最短时长：太快一闪而过同样难受，给个下限让「正在展开世界」看得见 */
const MIN_BOOT_MS = 900
/** 浏览器定位（GPS）的等待上限：超过就按「没有系统定位」走 IP，绝不死等 */
const GPS_TIMEOUT_MS = 6000
function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, Math.max(0, ms)))
}

function readSaved(): SavedState | null {
  try {
    const raw = localStorage.getItem(K_STATE)
    if (!raw) return null
    const o = JSON.parse(raw) as SavedState
    if (!o || !Array.isArray(o.path) || o.path.length < 2) return null
    return { onboarded: !!o.onboarded, path: dedupePath(o.path), hitAd: String(o.hitAd || ''), source: String(o.source || ''), ts: Number(o.ts || 0) }
  } catch {
    return null // 存储坏了就当没引导过，绝不让它把页面卡死
  }
}

function writeSaved(s: SavedState) {
  try {
    localStorage.setItem(K_STATE, JSON.stringify(s))
  } catch {
    /* 写不进去不影响本次使用，只是下次还要重新引导 */
  }
}

function clearSaved() {
  try {
    localStorage.removeItem(K_STATE)
  } catch {
    /* 同上 */
  }
}

/** 一级行政区划该用什么 zoom：国/省/市 用 2，区县用 3（尽量把街镇那一层也画出来） */
function zoomOf(adcode: string): number {
  const lv = geoLevelOf(adcode)
  return lv === 'district' ? 3 : 2
}

/** 行政级别 → 状态（面包屑回退时靠它决定回哪一步） */
function stepOf(level: GeoLevel): SimStep {
  switch (level) {
    case 'country':
      return 'country'
    case 'province':
      return 'province'
    case 'city':
      return 'city'
    default:
      return 'district'
  }
}

export interface UseWorldSimOptions {
  /** 取图器（useWorldSimGeo 的返回值） */
  geo: {
    style: Ref<string>
    setStyle: (s: string) => void
    loadStage: (ad: string, opts?: { w?: number; h?: number; zoom?: number; fresh?: boolean }) => Promise<GeoStageData | null>
    loadRegions: (ad: string) => Promise<GeoRegion[]>
    invalidate: () => void
  }
  /** 舞台当前尺寸（CSS 像素）——每次取图都要带上，字号才能看得清 */
  getViewport: () => { w: number; h: number }
  /**
   * i18n 翻译函数。**注入而不是内部 `useI18n()`** —— 这个 composable 会被 node 自检
   * 直接调用（没有组件实例），`useI18n()` 在那种场合会抛
   * `Must be called at the top of a setup function`。注入后两边都能用：
   * 组件传 vue-i18n 的 `t`，自检传一个 `key => key` 的桩。
   * 不传时退化成"原样返回键名"（可读性差但绝不崩）。
   */
  t?: (key: string, params?: Record<string, unknown>) => string
}

export function useWorldSim(opts: UseWorldSimOptions) {
  const { geo, getViewport } = opts
  // 错误文案要人话：技术信息先经 wsErr.classifyError 归类，再查 `worldsim.err.*`
  const tr = opts.t ?? ((k: string) => k)

  /* ── 状态 ────────────────────────────────────────────────────────────── */
  const step = ref<SimStep>('boot')
  /** 已知的完整路径（国 → … → 区县）；面包屑按它渲染，回退不删路径 */
  const path = ref<GeoRegion[]>([{ adcode: '100000', name: '中国' }])
  /** 当前停在路径的第几级 */
  const cursor = ref(0)
  /** 定位命中的 adcode（地图上高亮、等用户点「进入」） */
  const hitAd = ref('')
  /** 用户手动点的 adcode（比命中高亮更明确） */
  const pickAd = ref('')
  /** 舞台上正在显示的图 */
  const stage = ref<GeoStageData | null>(null)
  /** 舞台级加载中（区域切换的加载动画） */
  const busy = ref(false)
  const busyText = ref('')
  /** 舞台级错误：只显示成条状提示 + 重试，**不清空已有的图**（绝不白屏） */
  const error = ref('')
  /** 阶段性提示（比如「定位只到市级，请选一个区县」） */
  const note = ref('')
  /** 定位来源，用于如实告诉用户精度 */
  const locSource = ref<'' | 'gps' | 'ip' | 'manual' | 'restored'>('')
  /** 是否走了「上次位置」直进 */
  const restored = ref(false)
  /** 小区阶段是否已经生成过（面包屑上才会出现「小区」那一级） */
  const neighReady = ref(false)
  /** 首次引导中（决定确认弹窗弹不弹、以及是否显示引导步骤条） */
  const onboarding = ref(true)
  /** 确认弹窗是否已经弹过（一次引导只弹一次） */
  const confirmedOnce = ref(false)
  /** 重新取图用（点「重试」） */
  const lastStageReq = ref<{ ad: string; zoom: number } | null>(null)

  /* ── 派生 ────────────────────────────────────────────────────────────── */
  const leaf = computed<GeoRegion | null>(() => (path.value.length ? path.value[path.value.length - 1] : null))
  /** 「广州市·越秀区」（给确认弹窗、小区名用；直辖市自然只剩两级） */
  const areaLabel = computed(() => buildAreaLabel(path.value, 1))
  /** 定位/选择带来的精度说明 */
  const locLabel = computed(() => {
    switch (locSource.value) {
      case 'gps':
        return '系统定位'
      case 'ip':
        return 'IP 估测（城市级）'
      case 'manual':
        return '手动选择'
      case 'restored':
        return '上次位置'
      default:
        return ''
    }
  })

  const crumbs = computed<Crumb[]>(() => {
    // 只画**已经走到过的那几级**（0..cursor）：还没访问的级别画出来只会让用户迷惑。
    // 注意 path 本身**不删** —— 定位早就知道区县了，确认弹窗还要问它（见 drillTo 的注释）。
    const out: Crumb[] = path.value.slice(0, cursor.value + 1).map((p, i) => ({
      key: p.adcode,
      adcode: p.adcode,
      name: p.name,
      index: i,
      level: geoLevelOf(p.adcode),
      active: true,
      kind: 'geo' as const,
    }))
    // 虚拟的「小区」一级：只有生成过才出现（没生成过时它点不出任何东西，不该占位）。
    // index 用 -1 当哨兵：它不是 path 里的一级，backTo 收到负数就回小区。
    if (neighReady.value && path.value.length > 1) {
      out.push({
        key: 'neigh',
        adcode: path.value[path.value.length - 1].adcode,
        name: '小区',
        index: -1,
        level: 'street',
        active: step.value === 'neighborhood',
        kind: 'neigh',
      })
    }
    return out
  })

  /** 当前这一级已加载出来的子级清单（点「进入」用） */
  const regionsOfStage = computed<GeoRegion[]>(() => stage.value?.regions || [])

  /* ── 取图（所有网络都从这里走，错误一律兜住）────────────────────────── */
  async function goStage(ad: string, o: { zoom?: number; fresh?: boolean; silent?: boolean } = {}) {
    const zoom = o.zoom ?? zoomOf(ad)
    lastStageReq.value = { ad, zoom }
    if (!o.silent) {
      busy.value = true
      busyText.value = '正在展开地图…'
    }
    error.value = ''
    try {
      const vp = getViewport()
      const data = await geo.loadStage(ad, { w: vp.w, h: vp.h, zoom, fresh: o.fresh })
      // 竞态兜底：用户连点面包屑时，先回来的旧请求不许覆盖当前这一级
      if (data && data.adcode === ad) stage.value = data
      return data
    } catch (e) {
      {
        const info = errTextKey(e)
        error.value = `取「${ad}」的地图失败：${tr(info.key)}${info.detail ? `（${info.detail}）` : ''}`
      }
      return null
    } finally {
      if (!o.silent) busy.value = false
    }
  }

  /** 重试当前这一级的取图（错误条上的按钮） */
  async function retryStage() {
    const req = lastStageReq.value
    if (!req) return
    await goStage(req.ad, { zoom: req.zoom, fresh: true })
  }

  /* ── 定位 ────────────────────────────────────────────────────────────── */

  /**
   * 浏览器系统定位（GPS / 网络定位）。
   *
   * 为什么明明知道「Tauri Android WebView 里 navigator.geolocation 不可用」还要试一次：
   *   · 浏览器预览 / 桌面版 WebView 里它是**能用**的，能用就该用（精度比 IP 高一个量级）；
   *   · APK 里它会**立刻**走 error 回调（Rust 侧 live.rs 的注释解释过：Tauri 的 Android
   *     胶水没接 onGeolocationPermissionsShowPrompt），代价接近零，不会拖慢流程；
   *   · 无论成功失败都有超时兜底 —— 权限弹窗挂在那里也不会让页面卡住。
   */
  function gpsOnce(): Promise<{ lat: number; lng: number } | null> {
    return new Promise((resolve) => {
      const nav = typeof navigator !== 'undefined' ? navigator : undefined
      if (!nav?.geolocation?.getCurrentPosition) return resolve(null)
      let done = false
      const finish = (v: { lat: number; lng: number } | null) => {
        if (done) return
        done = true
        resolve(v)
      }
      const timer = setTimeout(() => finish(null), GPS_TIMEOUT_MS)
      try {
        nav.geolocation.getCurrentPosition(
          (pos) => {
            clearTimeout(timer)
            const lat = Number(pos?.coords?.latitude)
            const lng = Number(pos?.coords?.longitude)
            finish(Number.isFinite(lat) && Number.isFinite(lng) && (lat || lng) ? { lat, lng } : null)
          },
          () => {
            clearTimeout(timer)
            finish(null) // 被拒 / 不可用：静默降级到 IP，不打断流程
          },
          { timeout: GPS_TIMEOUT_MS - 500, maximumAge: 60000, enableHighAccuracy: false },
        )
      } catch {
        clearTimeout(timer)
        finish(null)
      }
    })
  }

  /**
   * 把定位结果变成一条「国 → 省 → [市 → 区县]」路径。
   *
   * 三种形状都要吃下（实测 IP 定位最常见的其实是第二种）：
   *   ① `path` 完整（省会反查到区县）→ 直接用；
   *   ② 只有 `leaf`（区县 adcode）→ 用行政编码前缀制推上级（与 Rust `admin_chain` 同规则）；
   *   ③ 只有坐标（反查超时）→ 由调用方再用 `blocksByLatLng` 补一个 adcode。
   */
  function pathFromLocation(loc: WorldLocation): GeoRegion[] {
    let p: GeoRegion[] = dedupePath(
      (loc.path || []).map((x) => ({ adcode: String(x.adcode || ''), name: String(x.name || '') })),
    )
    if (p.length < 2 && loc.leaf?.adcode) {
      const chain = adminChain(String(loc.leaf.adcode))
      p = chain.map((ad, i) => ({ adcode: ad, name: i === chain.length - 1 ? String(loc.leaf?.name || ad) : ad }))
    }
    if (!p.length) return []
    if (p[0].adcode !== '100000') p.unshift({ adcode: '100000', name: '中国' })
    return dedupePath(p)
  }

  /**
   * 把路径里「名字还是 adcode」的那几级补成真名。
   *
   * 什么时候会缺名字：定位只给了坐标 / 只给了区县 adcode 时，
   * 中间那几级（省、市）我们只有编码。补法是拿**上一级的子级清单**去查
   * （`loadRegions` 走的是同一个 geojson 缓存，命中时 0 网络开销）。
   * 查不到就保留 adcode —— 难看但**诚实**，总比编一个错名字强。
   */
  async function fillNames(p: GeoRegion[]): Promise<GeoRegion[]> {
    const out = p.map((x) => ({ ...x }))
    for (let i = 1; i < out.length; i++) {
      if (out[i].name && out[i].name !== out[i].adcode) continue
      try {
        const kids = await geo.loadRegions(out[i - 1].adcode)
        const hit = kids.find((k) => k.adcode === out[i].adcode)
        if (hit?.name) out[i].name = hit.name
      } catch {
        /* 补不到就留着 adcode，不影响流程 */
      }
    }
    return out
  }

  /**
   * 定位（首次引导的第 2 步）。
   *
   * @param o.ipOnly  跳过系统定位，直接用 IP 估测（「用 IP 估测」按钮走这条）
   */
  async function locate(o: { ipOnly?: boolean } = {}) {
    step.value = 'locating'
    error.value = ''
    note.value = ''
    confirmedOnce.value = false
    let coords: { lat: number; lng: number } | null = null
    if (!o.ipOnly) coords = await gpsOnce()

    let loc: WorldLocation | null = null
    try {
      // fast:true = 让 Rust 侧用短预算（4s）跑完并自己返回 {error,hint}，
      // 而不是撞上数据层 6s 的 withTimeout（那样 hint 会被前端兜底文案顶掉）。
      loc = coords
        ? await worldMapApi.location({ lat: coords.lat, lng: coords.lng, fast: true })
        : await worldMapApi.location({ fast: true, force: true })
    } catch (e) {
      loc = { lat: 0, lng: 0, error: '定位调用失败', hint: e instanceof Error ? e.message : String(e) }
    }

    let p = loc && !loc.error ? pathFromLocation(loc) : []
    // 只有坐标、反查没结果时：拿坐标换一个区县 adcode（blocks_at 走本地缓存 + 最近邻）
    if (!p.length && coords) {
      try {
        const blk = await worldMapApi.blocksByLatLng(coords.lat, coords.lng, geo.style.value, 1)
        const ad = blk?.main?.adcode
        if (ad) {
          const chain = adminChain(String(ad))
          p = dedupePath([
            { adcode: '100000', name: '中国' },
            ...chain.map((x, i) => ({ adcode: x, name: i === chain.length - 1 ? String(blk.main.name || x) : x })),
          ])
        }
      } catch {
        /* 拿不到就按失败处理，下面统一进 locateFailed */
      }
    }

    if (!p.length || p.length < 2) {
      // 定位失败 / 被拒：两条路都给（手动选城市 + 用 IP 估测），这是机主定的
      const hint = loc?.hint || loc?.error || '定位没有结果'
      step.value = 'locateFailed'
      note.value = hint
      locSource.value = ''
      return false
    }

    p = await fillNames(p)
    path.value = p
    // 游标停在「国」：此刻人在全国图上（更深的省/市/区县只是**已知**，还没走过去）。
    // 这几个已知级别不能丢 —— 后面每一步的「进入」和确认弹窗都要靠它。
    cursor.value = 0
    locSource.value = coords ? 'gps' : 'ip'
    // 命中「省」：只高亮，等用户点「进入」（机主明确要求**不要自动下钻**）
    hitAd.value = p[1]?.adcode || ''
    pickAd.value = ''
    // 首屏永远是全国那张省级轮廓图（命中省只是高亮，用户点了「进入」才下钻）
    await goStage('100000', { zoom: 2 })
    step.value = 'country'
    // 精度提示：IP 只到市级时，先把话说明白，免得用户以为是定位不准
    const leafLv = geoLevelOf(p[p.length - 1].adcode)
    note.value = leafLv === 'district' ? '' : '定位精度到市一级，下一步会请你确认具体区县'
    return true
  }

  /* ── 引导主流程 ──────────────────────────────────────────────────────── */

  /**
   * 入口：先放加载动画 + 同时把全国省级轮廓拉下来（两者并行，不浪费时间）。
   *
   * 整个过程包在 try/catch 里（「绝不能白屏」的最后一道闸）：里面有十几处网络与权限调用，
   * 任何一处抛出没预料到的异常，用户都会**永远停在加载动画**上——那是最糟的失败样子。
   * 兜底统一落到 locateFailed：那里有「手动选城市 / 用 IP 估测」两条现成的路，用户出得去。
   */
  async function start() {
    try {
      await startInner()
    } catch (e) {
      step.value = 'locateFailed'
      {
        const info = errTextKey(e)
        note.value = `初始化时出了点问题：${tr(info.key)}（可以手动选城市继续）`
      }
    }
  }

  async function startInner() {
    step.value = 'boot'
    error.value = ''
    note.value = ''
    restored.value = false
    onboarding.value = true
    neighReady.value = false
    path.value = [{ adcode: '100000', name: '中国' }]
    cursor.value = 0
    hitAd.value = ''
    pickAd.value = ''
    locSource.value = ''

    const saved = readSaved()
    const t0 = Date.now()
    // 全国图（省级轮廓）与加载动画**并行**：动画不是白等
    const stageP = goStage('100000', { zoom: 2, silent: true })
    await stageP
    await sleep(Math.max(0, MIN_BOOT_MS - (Date.now() - t0)))

    if (saved?.onboarded && saved.path.length > 1) {
      // 只走一次引导：直接进上次位置
      restored.value = true
      onboarding.value = false
      path.value = saved.path
      cursor.value = saved.path.length - 1
      hitAd.value = saved.hitAd || ''
      locSource.value = 'restored'
      neighReady.value = true
      step.value = 'neighborhood'
      // 背后的行政区划图也补上（面包屑要能点回去，回去时得有图）
      void goStage(saved.path[saved.path.length - 1].adcode, { zoom: 3, silent: true })
      return
    }
    await locate()
  }

  /** 用户点了某个区划（省/市/区县）——「进入」 */
  async function drillTo(region: GeoRegion) {
    if (!region?.adcode) return
    const lv = geoLevelOf(region.adcode)
    // 截断到当前 cursor 再追加（用户在某一级换了选择时，更深的旧路径必须作废）
    const known = path.value.findIndex((p) => p.adcode === region.adcode)
    if (known >= 0) {
      // ① 这一级**定位早就知道了**（比如定位给的是「广东省·广州市·越秀区」，
      //    用户点「进入 广东省」）：只把游标移过去，**更深的已知级别必须留着** ——
      //    丢了它，确认弹窗就只能问到「广州市」，用户还得手选一次区县。
      cursor.value = known
    } else {
      // ② 用户换了一支（点了别的省/别的市）：这时更深的旧路径才作废
      path.value = dedupePath([...path.value.slice(0, cursor.value + 1), { adcode: region.adcode, name: region.name }])
      cursor.value = path.value.length - 1
    }
    // 进入某一级后，把**路径上已知的下一级**高亮出来（定位到市 → 省图上就亮着那个市），
    // 这样用户一眼看到「定位到哪了」，而不是对着一张没标记的省图自己找
    hitAd.value = path.value[cursor.value + 1]?.adcode || ''
    pickAd.value = ''
    note.value = ''
    await goStage(region.adcode, { zoom: zoomOf(region.adcode) })
    step.value = stepOf(lv)
    // 到市一级（或直辖市直接到区）就弹一次性确认 —— 但只在首次引导里弹
    if (onboarding.value && !confirmedOnce.value && (lv === 'city' || lv === 'district')) {
      await openConfirm()
    }
  }

  /** 点「进入」：拿当前高亮/选中的那个子级下钻 */
  async function enterPick() {
    const ad = pickAd.value || hitAd.value
    if (!ad) return
    const hit = regionsOfStage.value.find((r) => r.adcode === ad)
    await drillTo(hit || { adcode: ad, name: ad })
  }

  /** 用户在地图上点了一个区划（只是选中，不立刻下钻） */
  function setPick(adcode: string, name?: string) {
    pickAd.value = adcode
    if (name && !path.value.some((p) => p.adcode === adcode)) {
      // 点到的不是已知路径上的区域：只记选中，等用户点「进入」再进路径
      note.value = `已选中：${name}`
    }
  }

  /** 弹确认（「你现在在 广州市·越秀区 吗？」） */
  async function openConfirm() {
    // 确认问的是「已知的最深一级」，所以先把它的名字补齐（IP 定位常常只给到市）
    if (path.value.length > 1) path.value = await fillNames(path.value)
    confirmedOnce.value = true
    step.value = 'confirm'
  }

  /** 确认「是」 */
  async function confirmYes() {
    const last = path.value[path.value.length - 1]
    const lv = last ? geoLevelOf(last.adcode) : 'country'
    if (lv === 'district') {
      // 已经是区县：直接进区县视图（下钻）
      onboarding.value = false
      await goStage(last!.adcode, { zoom: 3 })
      step.value = 'district'
      persist()
      return
    }
    // 只知道市（IP 定位的典型情况）：必须让用户选一个区县，否则没有「小区」可言
    note.value = '定位只到市级，请选一个区县'
    step.value = 'manual'
  }

  /** 确认「否」→ 进三级联动纠偏 */
  function confirmNo() {
    note.value = '请手动选择正确的省 / 市 / 区县'
    step.value = 'manual'
  }

  /** 打开三级联动（定位失败时也走它） */
  function openManual(msg = '') {
    note.value = msg
    step.value = 'manual'
  }

  /**
   * 三级联动选完（chain 是从省开始、一直到用户选中的那一级）。
   * 允许只到市级（该市拿不到区县列表时的兜底）——所以这里不强制长度。
   */
  async function applyManual(chain: GeoRegion[]) {
    const picked = dedupePath(chain).filter((c) => c.adcode && c.adcode !== '100000')
    if (!picked.length) return
    path.value = dedupePath([{ adcode: '100000', name: '中国' }, ...picked])
    cursor.value = path.value.length - 1
    hitAd.value = ''
    pickAd.value = ''
    locSource.value = 'manual'
    onboarding.value = false
    const leafRegion = path.value[path.value.length - 1]
    await goStage(leafRegion.adcode, { zoom: 3 })
    step.value = 'district'
    persist()
  }

  /** 面包屑：跳到第 i 级（不删更深的路径，用户还能再跳回去） */
  async function backTo(index: number) {
    if (index < 0) {
      // -1 = 虚拟的「小区」那一级（哨兵值，见 crumbs）
      if (neighReady.value) {
        step.value = 'neighborhood'
        cursor.value = path.value.length - 1
      }
      return
    }
    if (index >= path.value.length) return
    cursor.value = index
    onboarding.value = false
    const region = path.value[index]
    if (!region) return
    // 回到某一级时，把「再往下一级」的那个已知区域高亮出来 —— 用户一眼就知道上次选了哪
    hitAd.value = path.value[index + 1]?.adcode || ''
    pickAd.value = ''
    step.value = stepOf(geoLevelOf(region.adcode))
    if (!stage.value || stage.value.adcode !== region.adcode) {
      await goStage(region.adcode, { zoom: zoomOf(region.adcode) })
    }
  }

  /** 下钻到区县之后：生成小区地图（草图秒出 → AI 精绘淡入） */
  function generateNeighborhood() {
    if (!leaf.value || path.value.length < 2) return
    neighReady.value = true
    cursor.value = path.value.length - 1
    step.value = 'neighborhood'
    persist()
  }

  /** 落盘「上次位置」（下次直接进这里） */
  function persist() {
    if (path.value.length < 2) return
    writeSaved({
      onboarded: true,
      path: path.value.map((p) => ({ adcode: p.adcode, name: p.name })),
      hitAd: hitAd.value,
      source: locSource.value || 'manual',
      ts: Date.now(),
    })
  }

  /** 重新引导（清掉「上次位置」，从加载动画重来一遍） */
  async function restart() {
    clearSaved()
    await start()
  }

  /** 换地图主题（高德/暗色/水系）：图是后端画进 SVG 的，必须清缓存重取 */
  async function changeMapStyle(s: string) {
    geo.setStyle(s)
    geo.invalidate()
    const cur = stage.value?.adcode || '100000'
    await goStage(cur, { zoom: zoomOf(cur), fresh: true })
  }

  return {
    // 状态
    style: geo.style, // 地图本体主题（高德/暗色/水系）——外壳皮肤是另一回事（见 useWorldSimTheme）
    step,
    path,
    cursor,
    hitAd,
    pickAd,
    stage,
    busy,
    busyText,
    error,
    note,
    locSource,
    locLabel,
    restored,
    neighReady,
    onboarding,
    regionsOfStage,
    crumbs,
    leaf,
    areaLabel,
    // 动作
    start,
    locate,
    enterPick,
    setPick,
    drillTo,
    openConfirm,
    confirmYes,
    confirmNo,
    openManual,
    applyManual,
    backTo,
    generateNeighborhood,
    restart,
    retryStage,
    changeMapStyle,
  } as const
}

export type WorldSim = ReturnType<typeof useWorldSim>
