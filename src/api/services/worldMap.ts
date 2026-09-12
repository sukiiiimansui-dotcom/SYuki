// 世界地图数据层（T6-1）
//
// 数据来源集中在这里，组件不感知底层：
//   · 现在：HTTP → Python 侧车服务（127.0.0.1:8790，runit 常驻）
//   · T6-5 完成后：切到 Tauri invoke（Rust 本地实现）
// 切换只需把 USE_RUST 改 true 并补 world_map_* 命令。
import { Channel, invoke } from '@tauri-apps/api/core'

/** 是否已切到 Rust 本地实现（T6-5） */
export const USE_RUST = false

// 后端地址可用 VITE_WORLD_MAP_API 覆盖（默认指向 Rust 版，见 docs/world-map/07）。
// 两个实现返回结构完全一致，所以换端口不需要改任何组件：
//   · 8791 = Rust 版（world_map_rs，正在成为主线）
//   · 8790 = Python 侧车（原型，已停用，留作对照）
const API_BASE = (import.meta.env?.VITE_WORLD_MAP_API as string) || 'http://127.0.0.1:8791'

async function httpGet<T>(path: string, params?: Record<string, string | number | undefined>): Promise<T> {
  const q = new URLSearchParams()
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      if (v !== undefined && v !== null && v !== '') q.set(k, String(v))
    }
  }
  const url = `${API_BASE}${path}${q.toString() ? '?' + q.toString() : ''}`
  const res = await fetch(url, { cache: 'no-store' })
  if (!res.ok) throw new Error(`世界地图接口 ${path} 返回 ${res.status}`)
  return (await res.json()) as T
}

// ── 类型 ──
export interface BlockEdge { x: number; y: number }

export interface MainBlock {
  adcode: string
  name: string
  center: [number, number]
  bbox_lng: [number, number]
  bbox_lat: [number, number]
  radius_km: number
  img: string
  img_url: string
}

export interface RemoteBlock {
  adcode: string
  name: string
  center: [number, number]
  bearing: number
  dir: string
  distance_km: number
  edge: BlockEdge
  in_main: boolean
  radius_km: number
  img: string
}

export interface BlocksPayload {
  ok: boolean
  error?: string
  hint?: string
  style: string
  located?: boolean
  locSource?: string | null
  auto: boolean
  main: MainBlock
  remotes: RemoteBlock[]
}

export interface WorldLocation {
  lat: number
  lng: number
  accuracy?: number
  provider?: string
  source?: string
  precision?: string
  ip_city?: string
  ip_region?: string
  error?: string
  hint?: string
  path?: { adcode: string; name: string }[]
  leaf?: { adcode: string; name: string } | null
}

export interface WorldTime {
  [k: string]: unknown
}

export interface WorldWeather {
  [k: string]: unknown
}

export interface SchedulePlace {
  kind: string
  kindZh: string
  facilityId?: string
  name?: string
  label?: string
  grid?: number[]
}

export interface ScheduleItem {
  name: string
  time: string
  content: string
  kind: string
  kindZh: string
}

export interface ScheduleRole {
  name: string
  group: string
  title: string
  now: (ScheduleItem & { place: SchedulePlace | null }) | null
  next: ScheduleItem | null
  progress: number
  timeline: ScheduleItem[]
}

export interface LingChatCharacter {
  name: string
  folder: string
  subtitle: string
  avatarCount: number
  hasAvatar: boolean
  info: string
}

export interface SchedulePayload {
  ok: boolean
  source: 'lingchat' | 'default'
  path: string | null
  now: string
  nowMinutes: number
  roles: ScheduleRole[]
  characters: LingChatCharacter[]
  todos: Record<string, unknown>[]
  importantDays: Record<string, unknown>[]
  placeSource: string
}

export interface TransportPlan {
  ok: boolean
  error?: string
  route: Record<string, unknown>
  options: Record<string, unknown>[]
  modes: Record<string, unknown>[]
}

// ── 接口（HTTP 版）──
const http = {
  blocks: (ad?: string, style = 'gaode', limit = 6) =>
    httpGet<BlocksPayload>('/api/blocks', { ad, style, limit }),
  blocksByLatLng: (lat: number, lng: number, style = 'gaode', limit = 6) =>
    httpGet<BlocksPayload>('/api/blocks', { lat, lng, style, limit }),
  location: (opts?: { force?: boolean; fast?: boolean; lat?: number; lng?: number }) =>
    httpGet<WorldLocation>('/api/location', {
      force: opts?.force ? 1 : undefined,
      fast: opts?.fast ? 1 : undefined,
      lat: opts?.lat,
      lng: opts?.lng,
    }),
  time: () => httpGet<WorldTime>('/api/time'),
  weather: (city?: string) => httpGet<WorldWeather>('/api/weather', { city }),
  schedule: (now?: string, area?: string) => httpGet<SchedulePayload>('/api/schedule', { now, area }),
  transportPlan: (a: { lat: number; lng: number }, b: { lat: number; lng: number }, prefer?: string) =>
    httpGet<TransportPlan>('/api/transport_plan', {
      from_lat: a.lat, from_lng: a.lng, to_lat: b.lat, to_lng: b.lng, prefer,
    }),
  mapImg: (ad: string, style = 'gaode') => `${API_BASE}/api/map?ad=${ad}&style=${style}`,
  bigmapImg: (ad: string, style = 'gaode', scale = 1) =>
    `${API_BASE}/api/bigmap_img?ad=${ad}&style=${style}&scale=${scale}`,
  maplibFile: (id: string) => `${API_BASE}/api/maplib/file?id=${encodeURIComponent(id)}`,
}

// ── 对外统一出口（Rust 版就绪后在这里分流）──
export const worldMapApi = {
  blocks: async (ad?: string, style = 'gaode', limit = 6): Promise<BlocksPayload> => {
    if (USE_RUST) {
      return invoke<BlocksPayload>('world_map_blocks', { ad, style, limit })
    }
    return http.blocks(ad, style, limit)
  },
  blocksByLatLng: async (lat: number, lng: number, style = 'gaode', limit = 6): Promise<BlocksPayload> => {
    if (USE_RUST) {
      return invoke<BlocksPayload>('world_map_blocks_at', { lat, lng, style, limit })
    }
    return http.blocksByLatLng(lat, lng, style, limit)
  },
  location: async (opts?: { force?: boolean; fast?: boolean; lat?: number; lng?: number }): Promise<WorldLocation> => {
    if (USE_RUST) return invoke<WorldLocation>('world_map_location', { ...opts })
    return http.location(opts)
  },
  time: async (): Promise<WorldTime> => (USE_RUST ? invoke<WorldTime>('world_map_time') : http.time()),
  weather: async (city?: string): Promise<WorldWeather> =>
    (USE_RUST ? invoke<WorldWeather>('world_map_weather', { city }) : http.weather(city)),
  schedule: async (now?: string, area?: string): Promise<SchedulePayload> =>
    (USE_RUST ? invoke<SchedulePayload>('world_map_schedule', { now, area }) : http.schedule(now, area)),
  transportPlan: async (
    a: { lat: number; lng: number },
    b: { lat: number; lng: number },
    prefer?: string,
  ): Promise<TransportPlan> => {
    if (USE_RUST) return invoke<TransportPlan>('world_map_transport_plan', { from: a, to: b, prefer })
    return http.transportPlan(a, b, prefer)
  },
  mapImg: http.mapImg,
  bigmapImg: http.bigmapImg,
  maplibFile: http.maplibFile,
  apiBase: API_BASE,
}

// ═══════════════════════════════════════════════════════════════════
// 世界地图「新页面组」补充接口（src/components/views/worldmap/ 下的 4 个页面用）
//
// 约定：这一段**只追加**，不改上面任何既有导出 ——
//   WorldMap.vue / useWorldMapLayer 取用的对象与函数签名保持原样。
//
// 为什么另起一组函数、而不是往 worldMapApi 里塞：
//   ① 这些接口只有新页面用（SSE 事件流、SVG 探针、地图库），
//      塞进公共出口会让老页面共享的类型跟着变，没必要担风险；
//   ② SSE 是「取 URL 交给 EventSource」而不是「fetch JSON」，
//      形状本来就和 worldMapApi 的其它成员不同。
// ═══════════════════════════════════════════════════════════════════

// ── AI 实时绘制小区（/api/district_stream，SSE）──

/** 小区里的一栋建筑 / 一块绿地水域（后端逐条推的就是这个） */
export interface DistrictItem {
  x?: number
  y?: number
  w?: number
  h?: number
  /** residential / office / commercial / shop / restaurant / cafe / school / hospital / civic / leisure */
  type?: string
  name?: string
  /** 楼层：后端可能给数字也可能给字符串（AI 输出不稳定，两边都容忍） */
  floors?: number | string
}

/** 道路：两端点 + 等级 */
export interface DistrictRoadItem {
  x1?: number
  y1?: number
  x2?: number
  y2?: number
  /** main / secondary / path */
  type?: string
  name?: string
}

/** 完整布局（done 事件里那份，也是「保存到地图库」将来要 POST 的东西） */
export interface DistrictLayout {
  name?: string
  size?: number
  buildings?: DistrictItem[]
  roads?: DistrictRoadItem[]
  parks?: DistrictItem[]
  water?: DistrictItem[]
  details?: { stats?: Record<string, number> }
  /** 后端标注：这份布局是流式攒出来的 */
  _streamed?: boolean
}

export interface DistrictCounts {
  buildings: number
  roads: number
  parks: number
  water: number
}

/** SSE 事件（每行 `data: <json>`；流结束是 `data: [DONE]`） */
export interface DistrictStreamEvent {
  type:
    | 'start'
    | 'meta'
    | 'size'
    | 'building'
    | 'road'
    | 'park'
    | 'water'
    | 'warn'
    | 'debug'
    | 'done'
    | 'error'
  /** start */
  area?: string
  size?: number
  model?: string
  /** meta，也用作 done.layout.name */
  name?: string
  /** building / road / park / water */
  item?: DistrictItem & DistrictRoadItem
  index?: number
  elapsed?: number
  /** warn / error */
  message?: string
  /** debug */
  stats?: { chunks?: number; chars?: number; lines?: number }
  /** done */
  layout?: DistrictLayout
  counts?: DistrictCounts
}

export interface DistrictStreamOpts {
  /** 区域名，如「广州市·越秀区」（后端只拿它写进提示词，不做地理换算） */
  area?: string
  /** 剧情 / 风格提示，可选 */
  context?: string
  /** 规模档位：0=20×20、1=28×28、2=36×36（后端 base = 20 + expand*8） */
  expand?: number
}

/**
 * 拼 SSE 地址给 `new EventSource(...)`。
 *
 * 为什么不在服务层直接开 EventSource：EventSource 是**长连接 + 多次回调**的东西，
 * 生命周期属于组件（组件卸载要 close，重画要先 close 再开），
 * 服务层只负责「URL 怎么拼」，不许持有连接，否则页面切走连接还在漏。
 */
export function districtStreamUrl(opts: DistrictStreamOpts = {}): string {
  const q = new URLSearchParams()
  q.set('area', opts.area || '广州市·越秀区')
  if (opts.context) q.set('context', opts.context)
  if (opts.expand) q.set('expand', String(opts.expand))
  return `${API_BASE}/api/district_stream?${q.toString()}`
}

// ── 区域主图（/api/bigmap）──
//
// 注意：上面 worldMapApi 里的 mapImg/bigmapImg 指向 `/api/map`、`/api/bigmap_img`，
// 那是 Python 侧车（8790）时代的路由，Rust 版（8791）实测 **404**
// （全仓 grep 过，目前没有调用方，属于死代码；但既有导出按约定不动），
// 所以这里补一个走 `/api/bigmap` 的正确地址给新页面用。
export function bigmapSvgUrl(ad: string, style = 'gaode', scale = 1): string {
  const q = new URLSearchParams({ ad: ad || '', style, scale: String(scale) })
  return `${API_BASE}/api/bigmap?${q.toString()}`
}

// ── 渲染探针（/api/render/probe，直接返回 SVG 文本）──
export interface RenderProbeOpts {
  style?: string
  /** 2d 平面 / 3d 伪立体（按楼层挤出，由后端渲染决定，所以切 3D 必须重新请求） */
  mode?: '2d' | '3d'
  zoom?: number
  /** 是否把「小区数据」卡片画进 SVG 角落 */
  charts?: boolean
  size?: number
  seed?: number
}

/** 拼 /api/render/probe 地址（也可直接给 <img> 或新窗口用） */
export function renderProbeUrl(o: RenderProbeOpts = {}): string {
  const q = new URLSearchParams()
  q.set('style', o.style || 'gaode')
  q.set('mode', o.mode || '2d')
  q.set('zoom', String(o.zoom ?? 3))
  if (o.charts) q.set('charts', '1')
  if (o.size) q.set('size', String(o.size))
  if (o.seed !== undefined) q.set('seed', String(o.seed))
  return `${API_BASE}/api/render/probe?${q.toString()}`
}

/**
 * 取 SVG 文本（给 v-html 用）。
 *
 * 为什么要判 `startsWith('<')`：后端出错时这个路由回的是 JSON 而不是 SVG，
 * 直接塞进 v-html 会渲染成一坨 JSON 文本 —— 这里提前翻译成可读错误。
 */
export async function renderProbeSvg(o: RenderProbeOpts = {}): Promise<string> {
  const res = await fetch(renderProbeUrl(o), { cache: 'no-store' })
  if (!res.ok) throw new Error(`渲染接口返回 ${res.status}`)
  const text = await res.text()
  if (!text.trimStart().startsWith('<')) {
    let msg = text.slice(0, 200)
    try {
      const j = JSON.parse(text) as { error?: string }
      if (j?.error) msg = j.error
    } catch {
      /* 不是 JSON 就原样显示前 200 字 */
    }
    throw new Error(msg || '渲染失败')
  }
  return text
}

// ── 地图库（/api/maplib/*）──

export interface MapLibMeta {
  area?: string
  name?: string
  size?: number
  buildings?: number
  roads?: number
  parks?: number
  context?: string
  scale?: number
  layoutKey?: string
  osmUsed?: boolean
  [k: string]: unknown
}

export interface MapLibEntry {
  id: string
  /** region / bigmap / district */
  kind: string
  adcode: string
  style: string
  /** 相对「项目根」的路径，取本体用 maplibFile() */
  path: string
  bytes: number
  /** 秒级时间戳 */
  mtime: number
  createdAt: number
  lastAccess: number
  version: number
  /** generated / user … */
  origin: string
  meta?: MapLibMeta
}

export interface MapLibStats {
  count: number
  bytes: number
  mb: number
  by_kind: Record<string, number>
  max_bytes: number
  max_mb: number
}

export interface MapLibListPayload {
  stats: MapLibStats
  entries: MapLibEntry[]
}

export type MapLibSort = 'recent' | 'oldest' | 'largest'

export interface MapLibListOpts {
  kind?: string
  ad?: string
  limit?: number
  sort?: MapLibSort
}

/** 地图库列表（顺带返回容量统计，省一次请求） */
export const maplibList = (o: MapLibListOpts = {}): Promise<MapLibListPayload> =>
  httpGet<MapLibListPayload>('/api/maplib/list', {
    kind: o.kind,
    ad: o.ad,
    limit: o.limit,
    sort: o.sort,
  })

/** 只取容量统计（容量条用） */
export const maplibStats = (): Promise<MapLibStats> => httpGet<MapLibStats>('/api/maplib/stats')

export interface MapLibCleanupResult {
  removed: number
  freed: number
  freed_mb: number
  dry_run: boolean
  /** 被删（或将被删）的条目 id */
  victims?: string[]
  error?: string
}

/**
 * 容量清理。
 *
 * **默认干跑**：只有显式传 `{ dry: false }` 才真删 ——
 * Python 侧曾在这里误删 119 张缓存图，所以「真删」必须由调用方写出来，
 * 而不是靠某个默认值碰运气。参数顺序也刻意让 dry 是必填语义。
 */
export const maplibCleanup = (o: { maxMb?: number; dry: boolean }): Promise<MapLibCleanupResult> =>
  httpGet<MapLibCleanupResult>('/api/maplib/cleanup', {
    max_mb: o.maxMb,
    // 后端约定 dry=0 才是真删；干跑时干脆不带这个参数（少一个出错机会）
    dry: o.dry ? 1 : 0,
  })

/** 条目本体（图片 / JSON）：复用已有出口，避免两处拼 URL 走偏 */
export const maplibFileUrl = (id: string): string => worldMapApi.maplibFile(id)

// ── 小工具（新页面共用，纯函数，不碰网络）──

/** 把秒级时间戳格式化成「MM-DD HH:mm」；0/空值给占位符 */
export function fmtStamp(sec: number, withTime = true): string {
  if (!sec || !isFinite(sec)) return '—'
  const d = new Date(sec * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  const md = `${p(d.getMonth() + 1)}-${p(d.getDate())}`
  return withTime ? `${md} ${p(d.getHours())}:${p(d.getMinutes())}` : md
}

/** 人类可读体积 */
export function fmtBytes(bytes: number): string {
  const b = Number(bytes) || 0
  if (b < 1024) return `${b} B`
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`
  return `${(b / 1048576).toFixed(2)} MB`
}

/** 稳定 id → 便于给列表做 key（后端 id 已含冒号，这里只做兜底转义） */
export function safeId(id: string): string {
  return String(id || '').replace(/[^\w:.-]/g, '_')
}

// ═══════════════════════════════════════════════════════════════════
// AI 实时绘制小区：**统一入口**（Tauri Channel / 浏览器 EventSource）
//
// 为什么需要它：DistrictLive 页面原来自己 `new EventSource(...)` 连
// http://127.0.0.1:8791/api/district_stream —— 那是独立 Rust 调试服务的地址，
// 打包成 APK 后手机上根本没有那个进程，那条路必然失败。应用内要走
// 「Tauri 命令 + IPC Channel」：Rust 侧 `world_map_district_stream` 用 LingChat
// 自己的 LLM 客户端（设置页里配的那个，支持热切换）流式生成，每抠出一个元素就
// send 一条。事件形状与 SSE 的 data 行**逐字段一致**（Rust 侧同一个
// `Event::to_json`），所以页面那套阶段条/日志/增量画/对账逻辑两条路都能用。
//
// 生命周期：返回「停止」函数，必须在组件卸载、重画、点「停止」时调用 ——
// EventSource 是长连接，Tauri 那边是一个后台任务，不停掉的话页面切走了还在画。
//
// 注：这里**不另立** `DistrictStreamEvent` —— 上面（`districtStreamUrl` 附近）已经
// 导出过它，两条通路的负载字段完全一致，直接复用；同名 interface 重复声明会
// TS2300，也会让页面拿到两套互不相容的类型。
// ═══════════════════════════════════════════════════════════════════

/** 强制指定实时绘制通路（调试/录屏用）：`window.__WM_DISTRICT_TRANSPORT__ = 'tauri' | 'http'` */
export type DistrictTransport = 'tauri' | 'http'

declare global {
  interface Window {
    __WM_DISTRICT_TRANSPORT__?: DistrictTransport
    /** 纯 web 预览标记，由 src/web-mock.ts 打上 */
    __LINGCHAT_WEB_MOCK__?: boolean
  }
}

/**
 * 这一轮该走哪条通路。
 *
 * ① 先看显式开关（`__WM_DISTRICT_TRANSPORT__`），调试时可强制走某一条；
 * ② 再看 `window.__TAURI_INTERNALS__`（真壳由 Rust 注入，项目里判断 Tauri 就是这么判的）；
 * ③ 但**纯 web 预览**时 `src/web-mock.ts` 会伪造一份 `__TAURI_INTERNALS__`，它的 invoke
 *    对 `world_map_*` 一律返回 undefined（等价于「命令不存在」）—— 那样页面会永远停在
 *    「连接中…」，所以用 mock 自己打的 `__LINGCHAT_WEB_MOCK__` 标记把它排除，让它退回 EventSource。
 */
function useTauriTransport(): boolean {
  if (typeof window === 'undefined') return false // 非浏览器环境（SSR / 测试）兜底
  const forced = window.__WM_DISTRICT_TRANSPORT__
  if (forced === 'tauri') return true
  if (forced === 'http') return false
  if (!window.__TAURI_INTERNALS__) return false
  return !window.__LINGCHAT_WEB_MOCK__
}

/** invoke 的 reject 有的是 string（Rust 的 `Err(String)`），有的是 Error，统一成人话 */
function errText(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return e ? String(e) : '未知错误'
}

/** Tauri 分支：命令 + Channel（应用内正路） */
function startTauriDistrictStream(
  opts: DistrictStreamOpts,
  onEvent: (ev: DistrictStreamEvent) => void,
  onDone?: () => void,
  onError?: (msg: string) => void,
): () => void {
  let stopped = false
  // 唯一 id：Rust 侧靠它把「停止」找回对应的后台任务（见 world_map_district_stream_cancel）。
  // 时间戳 + 随机串足够：同一页面连点「重画」也不会撞。
  const streamId = `district-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`
  const channel = new Channel<DistrictStreamEvent>()
  channel.onmessage = (ev) => {
    if (stopped) return
    onEvent(ev)
    // done = 这一轮正常收尾。放在 onEvent 之后，先让调用方把布局对账完再收摊。
    if (ev.type === 'done') {
      stopped = true
      onDone?.()
    }
  }

  // 注意：命令**立刻返回**，真正的生成在 Rust 后台任务里跑，事件全部走 channel ——
  // 所以绝不能拿这个 invoke 的 resolve 当「画完了」（它只是「任务挂起来了」）。
  invoke('world_map_district_stream', {
    area: opts.area,
    context: opts.context ?? null,
    expand: opts.expand ?? 0,
    streamId,
    onEvent: channel,
  }).catch((e) => {
    if (stopped) return
    stopped = true
    // 命令本身失败（旧版 APK 没注册这个命令 / Channel 传参不被识别 / 参数反序列化失败…）：
    // 转成可读文案交给页面，别让异常冒到控制台就没了
    onError?.(`实时绘制命令调用失败：${errText(e)}`)
  })

  return () => {
    if (stopped) return
    stopped = true
    // 先本地挂断（此后迟到的事件一律丢弃），再让 Rust 把这一轮停掉。
    // 取消是「尽力而为」：任务可能刚好自己跑完了，失败不影响任何东西。
    invoke('world_map_district_stream_cancel', { streamId }).catch(() => {})
  }
}

/** 浏览器分支：沿用 EventSource（Rust 调试服务 8791 / 将来别的 HTTP 后端） */
function startHttpDistrictStream(
  opts: DistrictStreamOpts,
  onEvent: (ev: DistrictStreamEvent) => void,
  onDone?: () => void,
  onError?: (msg: string) => void,
): () => void {
  let es: EventSource
  try {
    es = new EventSource(districtStreamUrl(opts))
  } catch (e) {
    onError?.(`无法打开实时连接：${errText(e)}`)
    return () => {} // 连都没连上，停止函数给个空的即可
  }
  let stopped = false
  let received = 0
  const close = () => {
    try {
      es.close()
    } catch {
      /* 已经关了就算了 */
    }
  }

  es.onmessage = (e) => {
    if (stopped) return
    const raw = String(e.data ?? '')
    if (raw === '[DONE]') {
      stopped = true
      close()
      onDone?.()
      return
    }
    let ev: DistrictStreamEvent
    try {
      ev = JSON.parse(raw) as DistrictStreamEvent
    } catch {
      // 坏片段照旧只记一条日志（页面把它当 warn 显示），不打断整条流
      onEvent({ type: 'warn', message: '收到无法解析的流片段（已跳过）' })
      return
    }
    received++
    onEvent(ev)
  }

  es.onerror = () => {
    if (stopped) return
    // 两种情况必须分开（原来在页面里判的，现在判完只交给调用方一句话）：
    //   ① 后端没配 LLM 时这个路由回的是 **JSON**（不是 text/event-stream），
    //      浏览器按规范把连接判死（readyState=CLOSED）→ 再 fetch 一次把 JSON 里的
    //      error 读出来给用户看（这条路径后端不会调 LLM，不花钱）；
    //   ② 网络/服务问题 → 浏览器会一直重连，必须我们主动 close，否则页面看起来卡死。
    const wasClosed = es.readyState === EventSource.CLOSED
    const got = received
    stopped = true
    close()
    if (got > 0) {
      // 已经画出一部分了：交给调用方按「中断」收尾（保留已画的内容）
      onError?.('连接中断')
      return
    }
    if (!wasClosed) {
      onError?.(`连不上实时绘制服务（${API_BASE} 未启动或被拦）`)
      return
    }
    void probeStreamError(opts).then((reason) => onError?.(reason))
  }

  return () => {
    if (stopped) return
    stopped = true
    close()
  }
}

/**
 * 读「为什么开不了流」：只取 JSON 错误；如果拿到的其实是 SSE，
 * 立刻 abort —— 否则等于白白多跑一次生成（后端一进这个路由就会调 LLM）。
 */
async function probeStreamError(opts: DistrictStreamOpts): Promise<string> {
  try {
    const ctrl = new AbortController()
    const res = await fetch(districtStreamUrl(opts), { cache: 'no-store', signal: ctrl.signal })
    const ct = res.headers.get('content-type') || ''
    if (!ct.includes('json')) {
      ctrl.abort()
      return '后端拒绝了流式连接（返回内容不是 SSE）'
    }
    const j = (await res.json()) as { error?: string; hint?: string }
    return j.error || j.hint || `后端返回 ${res.status}`
  } catch (e) {
    return `读取失败原因时又出错：${errText(e)}`
  }
}

/**
 * 实时绘制小区：**统一入口**，页面只调这一个函数。
 *
 * @param opts    区域 / 剧情提示 / 规模档位（base = 20 + expand×8）
 * @param onEvent 每一条流事件（形状见 `DistrictStreamEvent`，两条通路一致）
 * @param onDone  流正常收尾（浏览器版是 `[DONE]`；Tauri 版是 `done` 事件之后）
 * @param onError 连不上 / 中途断了 / 命令调用失败，参数是给人看的一句话
 * @returns 停止函数（幂等）：组件卸载、重画、点「停止」时都要调
 */
export function startDistrictStream(
  opts: { area: string; context?: string; expand?: number },
  onEvent: (ev: DistrictStreamEvent) => void,
  onDone?: () => void,
  onError?: (msg: string) => void,
): () => void {
  if (useTauriTransport()) return startTauriDistrictStream(opts, onEvent, onDone, onError)
  return startHttpDistrictStream(opts, onEvent, onDone, onError)
}

export default worldMapApi
