// 世界地图数据层（T6-1）
//
// 数据来源集中在这里，组件不感知底层：
//   · Tauri 应用内（APK / 桌面）：`invoke` → Rust 本地实现（src-tauri/src/world_map/）
//   · 纯浏览器预览 / 局域网调试：HTTP → Rust 调试服务（127.0.0.1:8791）
// 两条通路的**分流是自动的**（见下面的 isTauriRuntime），不需要再手改常量：
// 打包成 APK 之后手机上根本没有 8791 那个进程，写死 HTTP 必然显示「世界地图服务未启动」。
import { Channel, invoke } from '@tauri-apps/api/core'

/**
 * 当前是不是**真的**跑在 Tauri 壳里（而不是浏览器预览）。
 *
 * 判定必须同时满足两条，缺一不可：
 *   ① `window.__TAURI_INTERNALS__` 存在 —— 真壳由 Rust 注入；
 *   ② 没有 `__LINGCHAT_WEB_MOCK__` 标记 —— `src/web-mock.ts` 在纯 web 预览时会**伪造**
 *      一份 `__TAURI_INTERNALS__`，而它的 invoke 对所有 `world_map_*` 一律返回 undefined
 *      （等价于「命令不存在」）。只看 ① 会把 web 预览误判成真壳，
 *      地图请求全打到 mock 上 → 页面永远空白。
 */
export function isTauriRuntime(): boolean {
  if (typeof window === 'undefined') return false // 非浏览器环境（SSR / 测试）兜底
  if (!window.__TAURI_INTERNALS__) return false
  return !window.__LINGCHAT_WEB_MOCK__
}

/**
 * 是否走 Rust 本地实现 —— 保留这个既有导出名（别处可能在引用），
 * 值就是上面的自动判别结果，而**不再是写死的 false**。
 *
 * 注意：这是「模块加载那一刻」的快照；接口内部一律用 `isTauriRuntime()` 实时判定，
 * 这样不受模块求值顺序影响（`src/main.ts` 第一行 `import "./web-mock"` 会先执行，
 * 但测试 / 别的入口未必这么排）。
 */
export const USE_RUST = isTauriRuntime()

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

// ── 降级工具 ──

/**
 * 给 invoke 套一层超时。
 *
 * 为什么需要：命令**没注册**时 Tauri 会立刻 reject（好办，catch 就行），
 * 但如果哪天命令注册了却在 Rust 侧卡住（比如等 GPS / 等网络），
 * 页面会永远停在「加载中…」—— 那比报错更难查。超时后走降级分支，至少页面能继续用。
 */
function withTimeout<T>(p: Promise<T>, ms: number, what: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${what} 调用超时（${ms}ms）`)), ms)
    p.then(
      (v) => {
        clearTimeout(timer)
        resolve(v)
      },
      (e) => {
        clearTimeout(timer)
        reject(e)
      },
    )
  })
}

// ═══════════════════════════════════════════════════════════════════
// 为什么 `location` / `weather` 在 Tauri 下要**降级**而不是直接 invoke
//
// 现状（已核对 src-tauri/src/lib.rs 的 generate_handler 注册表）：
//   `world_map_location` 与 `world_map_weather` **两个命令都还没实现**。
//   Rust 侧目前有的是：blocks / blocks_at / render / render_svg / geo_svg / geo_status /
//   coord_selftest / stats / maplib_* / schedule / transport_plan / osm_summary / time /
//   push_events / recent_events / district_stream(_cancel)。
//   所以真壳里直接 invoke 这两个名字 → 命令不存在 → reject；
//   若没人接住，页面就只剩一句报错（甚至白屏）。
//
// 因此这里做**数据层降级**（页面一行都不用改）：
//   location：invoke（将来能用）→ HTTP 兜底 → `{error,hint}` 让页面走已有的「定位失败」路径
//   weather ：invoke（将来能用）→ HTTP 兜底 → 空对象，顶栏少个标签而已
//
// 后续要补的（Rust 侧，不在本次前端改动范围内）：
//   ① `world_map_location(opts)`：Android 走系统定位权限，桌面走系统 API；
//      明确**不要**照搬 termux-location（那是 Termux:API 的东西，装进 APK 必然失败）；
//   ② `world_map_weather(city)`：可以复用 Rust 侧的天气源，返回结构与 `/api/weather` 一致
//      （`{current:{temp_c,desc}}` 这一层，见 WorldMap.vue 的 loadTimeWeather）。
//   补完后前端**不用再改**：上面的 try 分支一命中，降级代码自然不再执行。
// ═══════════════════════════════════════════════════════════════════

// ── 对外统一出口（真壳 / 浏览器在这里自动分流）──
//
// 每个分支都用 `isTauriRuntime()` 实时判定（而不是读上面那个常量快照），
// 免得判早了或判晚了一格导致整页失效。
export const worldMapApi = {
  blocks: async (ad?: string, style = 'gaode', limit = 6): Promise<BlocksPayload> => {
    if (isTauriRuntime()) {
      return invoke<BlocksPayload>('world_map_blocks', { ad, style, limit })
    }
    return http.blocks(ad, style, limit)
  },
  blocksByLatLng: async (lat: number, lng: number, style = 'gaode', limit = 6): Promise<BlocksPayload> => {
    if (isTauriRuntime()) {
      return invoke<BlocksPayload>('world_map_blocks_at', { lat, lng, style, limit })
    }
    return http.blocksByLatLng(lat, lng, style, limit)
  },
  // ── 定位：Rust 侧**还没有** `world_map_location` 命令（降级原因见上方整段说明）──
  location: async (opts?: { force?: boolean; fast?: boolean; lat?: number; lng?: number }): Promise<WorldLocation> => {
    if (!isTauriRuntime()) return http.location(opts)
    // ① 先试真命令：Rust 侧一旦补上 `world_map_location`，这里自动就通了，前端不用再改。
    try {
      return await withTimeout(invoke<WorldLocation>('world_map_location', { ...opts }), 6000, '定位')
    } catch {
      /* 命令不存在 / 超时 → 往下走降级 */
    }
    // ② 降级一：HTTP 兜底（真机上是「自己连自己」，通常连不上；
    //    但局域网调试、桌面版同时起着 8791 调试服务时仍然可用）。
    try {
      return await http.location(opts)
    } catch {
      /* 两条都不通 → 往下走 ③ */
    }
    // ③ 降级二：返回一个**页面能读懂的结果**（不抛异常）。
    //    WorldMap.vue 里 relocate() 判的是 `loc.error`，拿到 error 就显示 hint 并回到默认城市，
    //    绝不会白屏；用户也可以直接用「刷新」走默认区域。
    return {
      lat: 0,
      lng: 0,
      error: '定位不可用',
      hint: '应用内暂不支持自动定位，请手动选择区域（或直接填区域名）',
      source: 'none',
    }
  },
  time: async (): Promise<WorldTime> => (isTauriRuntime() ? invoke<WorldTime>('world_map_time') : http.time()),
  // ── 天气：Rust 侧**还没有** `world_map_weather` 命令（降级原因见上方整段说明）──
  weather: async (city?: string): Promise<WorldWeather> => {
    if (!isTauriRuntime()) return http.weather(city)
    try {
      // 26000 > Rust 侧 25 秒：前端若先放弃，Rust 那边跑完虽然会写进 30 分钟缓存，
      // 但用户第一次进页面就是没天气。宁可多等 25 秒拿一次，之后 30 分钟都是秒回。
      return await withTimeout(invoke<WorldWeather>('world_map_weather', { city }), 26000, '天气')
    } catch {
      /* 命令不存在 / 超时 → 降级 */
    }
    try {
      return await http.weather(city)
    } catch {
      // 拿不到天气不是错误：WorldMap.vue 的 loadTimeWeather() 已经把 weatherText 留空，
      // 顶栏少一个标签而已，不打扰主流程。返回空对象比抛异常干净。
      return {}
    }
  },
  /**
   * 日程（谁现在该在哪）。
   *
   * ⚠️ `facilities` **必须传**（从 `world_map_runtime` 里那份设施表原样带回去）：
   * 后端 `schedule::role_place(kind, facilities, seed)` 只有在拿到设施表时才能把
   * "商业区"这种**类型**落到**具体设施点**上（给 `place.name`）。
   * 不传的话后端返回的是 `placeSource: "kind-only"`、`place.name` 为空，于是：
   *   · 地图上角色的「地点」永远是空的 → `wsRuntimePush` 推上去的 `facility` 也是空
   *   · 事件引擎（`event_cmd.rs`）按空 place 判室内外 → 恒为户外
   *     → **停电/停水/失眠/睡过头这批"仅室内"事件永远不会触发**，而且不报错
   *   · "消费场所 ×1.8 / 户外场所 ×1.4" 两条权重修正同样永不生效
   * 也就是说：少传这一个参数，事件引擎就"半边瘫"，而表面上一切正常。
   */
  schedule: async (
    now?: string,
    area?: string,
    facilities?: unknown[],
  ): Promise<SchedulePayload> =>
    isTauriRuntime()
      ? invoke<SchedulePayload>('world_map_schedule', {
          now,
          area,
          // 空数组与 undefined 等价（后端 `facilities.as_deref()` 拿不到就按 kind-only 走），
          // 但传 `[]` 会让后端多做一次无意义的匹配，所以空的时候就别传。
          facilities: facilities && facilities.length ? facilities : undefined,
        })
      : http.schedule(now, area),
  transportPlan: async (
    a: { lat: number; lng: number },
    b: { lat: number; lng: number },
    prefer?: string,
  ): Promise<TransportPlan> => {
    if (isTauriRuntime()) return invoke<TransportPlan>('world_map_transport_plan', { from: a, to: b, prefer })
    return http.transportPlan(a, b, prefer)
  },
  // 下面三个是**死代码**（全仓 grep 无调用方，仅为兼容保留）：
  // mapImg → `/api/map`、bigmapImg → `/api/bigmap_img` 都是 Python 侧车（8790）时代的路由，
  // Rust 服务（8791）与打包后的应用内**都没有**这两条路由（实测 404）。
  // 新代码请用文件末尾的 `mapSvgUrl()`（双通路）或 `bigmapSvgUrl()`（仅浏览器）。
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
//
// 注意②：这个函数拼的是**纯 HTTP 地址**，只适合「明确知道自己在浏览器里」的场景
// （如 worldmap/PhoneOverlay.vue 的形态预览）。要图片在 **Tauri 应用内**也能出来，
// 必须用文件末尾的 `mapSvgUrl()`（真壳走 invoke，不依赖任何本地端口）。
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
    /** Tauri 壳注入的运行时（真壳由 Rust 注入；web-mock 会伪造一份，见 isTauriRuntime） */
    __TAURI_INTERNALS__?: any
  }
}

/**
 * 这一轮该走哪条通路。
 *
 * ① 先看显式开关（`__WM_DISTRICT_TRANSPORT__`），调试时可强制走某一条；
 * ② 其余交给 `isTauriRuntime()`（真壳 vs web-mock 的判别全在那一个函数里，别处别再抄一遍）。
 */
function useTauriTransport(): boolean {
  if (typeof window === 'undefined') return false // 非浏览器环境（SSR / 测试）兜底
  const forced = window.__WM_DISTRICT_TRANSPORT__
  if (forced === 'tauri') return true
  if (forced === 'http') return false
  return isTauriRuntime()
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

// ═══════════════════════════════════════════════════════════════════
// 区域主图：**双通路**取图（Tauri 命令 / HTTP），给 `<img src>` 用
//
// 为什么要这层封装：WorldMap.vue / WorldMapLayer.vue 原来直接拼
// `http://127.0.0.1:8791/api/bigmap?...` 塞给 `<img>`。那是**独立调试服务**的地址，
// 打包成 APK 之后手机上没有任何进程监听 8791 → 图片必然加载失败，
// 地图页只剩一句「世界地图服务未启动」和一个空舞台。
//
// 真壳里改走 Tauri 命令 `world_map_geo_svg`（Rust 本地渲染，不需要端口、不需要网络），
// 拿回的 SVG 文本转成 data URL 再塞进 `<img>` —— 组件那套 @error / 刷新逻辑一行都不用改。
//
// **尺寸必须跟着容器走**（机主专门提过）：SVG 里字号是「固定 px」（见 Rust 侧
// render_geo.rs 的 level_style：全国 13.5 / 省 12 / 市 11 / 区县 10.5，再乘各区面积占比
// 0.75~1.6，实测最小 8.2px），跟画布尺寸无关。
// 若固定按 1000×760 画、在手机上被缩到 360px 显示，8~12px 的字实际只剩 3~4px，完全看不清；
// 把容器真实 CSS 像素传下去，画布与显示尺寸 1:1，字号是多少就显示多少（约放大 2.7 倍）。
// ═══════════════════════════════════════════════════════════════════

/** 拿不到容器尺寸时的兜底画布（与 Rust 渲染器 / `/api/bigmap` 的默认值一致） */
export const MAP_SVG_DEFAULT_W = 1000
export const MAP_SVG_DEFAULT_H = 760

/** SVG 文本 → 可直接塞进 `<img src>` 的 data URL */
function svgToDataUrl(svg: string): string {
  const text = String(svg || '').trim()
  if (!text.startsWith('<')) throw new Error('后端没有返回 SVG')
  // 用 encodeURIComponent：SVG 里的 `#`（颜色）、中文地名、`<>&` 都会被转义，
  // 否则 data URL 会在第一个 `#` 处被截断成「只画了一半」的图。
  return 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(text)
}

/**
 * 取一份 SVG 文本。后端出错时回的是 JSON（甚至 HTML），
 * 这里提前翻译成人话，免得把 JSON 当图片塞给 `<img>` 后只看到一句「加载失败」。
 */
async function fetchSvgText(url: string): Promise<string> {
  const res = await fetch(url, { cache: 'no-store' })
  if (!res.ok) throw new Error(`地图接口返回 ${res.status}`)
  const ct = (res.headers.get('content-type') || '').toLowerCase()
  if (ct.startsWith('image/') && !ct.includes('svg')) throw new Error('后端返回的是位图，不是可缩放的 SVG')
  const text = await res.text()
  // 只看开头一段就够：真正的 SVG 一定是 `<svg ...>` 打头（最多前面有 `<?xml ...?>`）
  if (!/<svg[\s>]/i.test(text.slice(0, 400))) {
    let msg = text.slice(0, 200)
    try {
      const j = JSON.parse(text) as { error?: string }
      if (j?.error) msg = j.error
    } catch {
      /* 不是 JSON 就原样显示前 200 字 */
    }
    throw new Error(msg || '地图渲染失败')
  }
  return text
}

/**
 * 取区域主图，返回**可直接给 `<img src>` 的 data URL**。
 *
 * @param ad    行政区划 adcode（全国 `100000`；空值时抛错，免得后面拿到一张别的图）
 * @param style gaode（默认）/ dark / water
 * @param w     容器实际 CSS 像素宽（`getBoundingClientRect().width`）
 * @param h     容器实际 CSS 像素高
 *
 * 失败一律 **throw**：调用方（组件）捕获后把 src 置空并走自己已有的错误提示路径，
 * 绝不让一个未捕获的 rejection 冒到控制台、也不让页面卡在「加载中…」。
 */
export async function mapSvgUrl(
  ad: string,
  style = 'gaode',
  w: number = MAP_SVG_DEFAULT_W,
  h: number = MAP_SVG_DEFAULT_H,
): Promise<string> {
  const code = String(ad || '').trim()
  if (!code) throw new Error('缺少 adcode，取不到地图')
  // 兜底 + 取整：0 / NaN 会让 SVG 的 width/height 属性坏掉，`<img>` 直接空白
  const width = Math.max(64, Math.round(Number(w) || MAP_SVG_DEFAULT_W))
  const height = Math.max(64, Math.round(Number(h) || MAP_SVG_DEFAULT_H))

  // ── ① 真壳：Tauri 命令 ──
  if (isTauriRuntime()) {
    // 命令名与参数名**逐一核对过** src-tauri/src/world_map/mod.rs 的：
    //   pub async fn world_map_geo_svg(app, ad: Option<String>, style: Option<String>,
    //        width: Option<f64>, height: Option<f64>, pad, zoom, labels, dots, stats) -> Result<String, String>
    // Tauri 的 JS 侧参数是 camelCase，这条命令的形参都是单词，所以原样传即可；
    // 其余可选参数（pad/zoom/labels/dots/stats）走 Rust 侧默认值，这里不传。
    const svg = await invoke<string>('world_map_geo_svg', { ad: code, style, width, height })
    return svgToDataUrl(svg)
  }

  // ── ② 浏览器 / 局域网调试：HTTP ──
  // 首选 `/api/geo_svg`：它是 `world_map_geo_svg` 命令在调试服务上的**孪生路由**，
  // 同样认 w/h，所以浏览器预览与真机看到的排版是一致的（字大小不会两套）。
  const geoUrl =
    `${API_BASE}/api/geo_svg?ad=${encodeURIComponent(code)}` +
    `&style=${encodeURIComponent(style)}&w=${width}&h=${height}&zoom=2`
  try {
    return svgToDataUrl(await fetchSvgText(geoUrl))
  } catch (e) {
    // 兜底：老一些的后端只有 `/api/bigmap`（Rust 调试服务两条都有；Python 侧车只有 bigmap，
    // 而且它回的是 **PNG** → 会在 fetchSvgText 里被识别成位图并报错，这也是预期内的降级终点）。
    // 注意 bigmap **不认 w/h**（只认 scale），所以这条兜底路径忽略尺寸、沿用旧行为。
    try {
      const bigUrl =
        `${API_BASE}/api/bigmap?ad=${encodeURIComponent(code)}` +
        `&style=${encodeURIComponent(style)}&scale=1`
      return svgToDataUrl(await fetchSvgText(bigUrl))
    } catch (e2) {
      throw new Error(`${errText(e2)}（/api/geo_svg：${errText(e)}）`)
    }
  }
}

export default worldMapApi

// ═══════════════════════════════════════════════════════════════════
// 新页面（worldmap/*）剩下的三处 HTTP 依赖：双通路收口
//
// 为什么**追加在文件末尾**、而不去改上面那些同名老函数：
//   `renderProbeSvg` / `maplibList` / `maplibStats` / `maplibCleanup` 是**纯 HTTP** 的历史出口，
//   浏览器预览（vite dev + 8791 调试服务）还在用它们，改掉会牵连那条已验证的路；
//   所以这里按 `mapSvgUrl()` 的同一个模式补一组「自动分流」版本，让组件不感知底层：
//     真壳（APK / 桌面）→ `invoke` Rust 本地命令，完全不依赖本地端口；
//     浏览器 / 局域网调试 → 原样调老函数走 HTTP。
//   每个分支都用 `isTauriRuntime()` **实时**判定（不读模块加载时的常量快照）。
//
// 参数名怎么定的：Rust 侧形参是 snake_case，Tauri 给 JS 侧转成 camelCase ——
// 单词参数两侧同名（ad / style / mode / zoom / size / seed / charts / limit / sort / kind），
// 多词参数 JS 必须写驼峰（`max_mb` → `maxMb`、`dry_run` → `dryRun`）。
// 下面每个函数都把对应的 Rust 签名抄在注释里，改命令时对着核。
// ═══════════════════════════════════════════════════════════════════

/**
 * 小区渲染（DistrictViz 页：数据 / 图层 / 伪 3D 共用的一张图），返回**裸 SVG 文本**。
 *
 * Tauri 侧命令（src-tauri/src/world_map/mod.rs）：
 *   pub async fn world_map_render(
 *     app: AppHandle,
 *     layout: Option<Value>, key: Option<String>, area: Option<String>,
 *     size: Option<i32>, seed: Option<u64>,
 *     style: Option<String>, mode: Option<String>, zoom: Option<i32>,
 *     charts: Option<bool>, animate: Option<bool>, layers: Option<bool>,
 *     width: Option<f64>, height: Option<f64>, pad: Option<f64>,
 *   ) -> Result<String, String>
 * 返回的就是 SVG 文本，与 HTTP 版 `/api/render/probe` 同形（v-html 直接吃）。
 *
 * 取 layout 的优先级是 `layout` → `key` → `area`+`size`+`seed`：
 * 这里三个都不给（与 HTTP 探针的默认行为一致，Rust 侧走 sketch 的默认区域），
 * 只把「会影响画面」的 style / mode / zoom / size / seed / charts 传下去；
 * `width`/`height`/`pad`/`animate`/`layers` 留给 Rust 默认值 —— 页面是靠 CSS
 * （`.vz-stage :deep(svg) { max-width/height:100% }`）缩放的，传尺寸反而会两套排版。
 *
 * 失败一律 **throw**（空串 / 不是 SVG 也算失败）：调用方已有 `catch` → 顶部红条提示，
 * 绝不让空串进 v-html 变成一片白。
 */
export async function districtRenderSvg(o: RenderProbeOpts = {}): Promise<string> {
  // ── ② 浏览器 / 局域网调试：保持老通路（行为与改造前逐字节一致）──
  if (!isTauriRuntime()) return renderProbeSvg(o)

  // ── ① 真壳：Tauri 命令 ──
  // 注意别传 `layout`/`key`/`area`：`layout: undefined` 会被 JSON 丢掉（等价于不传），
  // 但显式传 null 在 Rust 侧是 `Some(Value::Null)` —— `normalize_layout` 认不出就会掉到
  // 后面的分支，语义会变得难懂；干脆一个都不传。
  const svg = await invoke<string>('world_map_render', {
    style: o.style || 'gaode',
    mode: o.mode || '2d',
    zoom: o.zoom ?? 3,
    charts: !!o.charts,
    size: o.size,
    seed: o.seed,
  })
  // 双保险：命令注册错/参数名写错时 invoke 会 reject，但万一哪天回了空串，
  // 这里也要翻译成人话，而不是把 "" 塞进 v-html（页面会白）。
  if (!svg || !String(svg).trimStart().startsWith('<')) {
    throw new Error('world_map_render 没返回 SVG 文本（命令签名或参数可能对不上）')
  }
  return svg
}

/**
 * 地图库列表（含容量统计）：真壳 invoke，浏览器 HTTP。
 *
 * Tauri 侧命令：
 *   pub async fn world_map_maplib_list(
 *     app: AppHandle, kind: Option<String>, ad: Option<String>,
 *     limit: Option<usize>, sort: Option<String>,
 *   ) -> Result<Value, String>          // 返回 { stats, entries }，与 /api/maplib/list 一致
 *
 * 空串过滤交给 Rust 侧（它自己做 `filter(|s| !s.trim().is_empty())`），
 * 这里把 undefined 原样传：JSON 会丢掉 undefined 的键 → Rust 收到 None → 用默认值，
 * 与 HTTP 版 `httpGet` 丢掉空参数的行为一致。
 */
export async function maplibListAuto(o: MapLibListOpts = {}): Promise<MapLibListPayload> {
  if (!isTauriRuntime()) return maplibList(o)
  return await invoke<MapLibListPayload>('world_map_maplib_list', {
    kind: o.kind,
    ad: o.ad,
    limit: o.limit,
    sort: o.sort,
  })
}

/**
 * 地图库容量统计：真壳 invoke，浏览器 HTTP。
 *
 * Tauri 侧命令：`pub async fn world_map_maplib_stats(app: AppHandle) -> Result<Value, String>`
 * 返回 `{ count, bytes, mb, by_kind, max_bytes, max_mb }`（字段名与 HTTP 版一致）。
 */
export async function maplibStatsAuto(): Promise<MapLibStats> {
  if (!isTauriRuntime()) return maplibStats()
  return await invoke<MapLibStats>('world_map_maplib_stats')
}

/**
 * 地图库容量清理（LRU）：真壳 invoke，浏览器 HTTP。
 *
 * Tauri 侧命令：
 *   pub async fn world_map_maplib_cleanup(
 *     app: AppHandle, max_mb: Option<f64>, dry_run: Option<bool>, dry: Option<bool>,
 *   ) -> Result<Value, String>          // { removed, freed, freed_mb, dry_run, victims? }
 *
 * ⚠️ 干跑参数在 Rust 侧叫 `dry_run`，JS 侧必须写 **camelCase `dryRun`**。
 * 写错成 `dry_run` 不会「变成真删」——Tauri 忽略不认识的键，Rust 侧 `dry_run` 收到 None，
 * 于是走 `dry_run.or(dry).unwrap_or(true)` = **true**（干跑），属于失败安全；
 * 但那样「真删」按钮会永远删不掉东西、又没人发现，所以这里**显式**把布尔传全，
 * 绝不依赖「少传参数碰默认值」。语义对齐：本函数参数的 `dry: true` = 干跑。
 */
export async function maplibCleanupAuto(o: { maxMb?: number; dry: boolean }): Promise<MapLibCleanupResult> {
  if (!isTauriRuntime()) return maplibCleanup(o)
  return await invoke<MapLibCleanupResult>('world_map_maplib_cleanup', {
    maxMb: o.maxMb,
    dryRun: !!o.dry, // 只有调用方明写 dry:false 才会真删，与 HTTP 版纪律一致
  })
}

// ═══════════════════════════════════════════════════════════════════
// 「世界模拟」首屏要的是**裸 SVG 文本**，不是 data URL —— 所以这里补一个 geoSvgText()
//
// 为什么不能直接用上面的 `mapSvgUrl()`：
//   那个函数把 SVG 转成 data URL 就返回了，**文本就丢了**；而首屏需要从文本里读到
//   `render_geo` 写在每个区划上的 `<g class="geo-region" data-adcode data-name>`，
//   用它来：① 知道这张图上有哪些省/市/区县（三级联动列表的数据源，不必另开接口）；
//          ② 判断定位命中的那个省在图上的哪一块（高亮）。
//   一个请求同时拿到「图」和「图上的区划索引」，比再调一次接口省一次往返。
//
// 与 `mapSvgUrl()` 一样是**双通路**：真壳 invoke Rust 命令（不需要任何本地端口），
// 浏览器走调试服务的孪生路由 `/api/geo_svg`（8791 Rust 服务与 8790 Python 侧车都有这条，
// 都认 w/h/zoom，所以预览与真机排版一致）。
//
// 尺寸仍然必须跟着容器走（理由见 `mapSvgUrl` 上方那段）：字号是固定 px，
// 容器尺寸传下去画布与显示 1:1，手机上字才看得清。
// 另外尺寸还有个副作用是**好事**：抽稀容差是 0.6px，画布越小保留的顶点越少，
// 手机上返回的 SVG 反而更小（全国那张 1000×760 实测 695KB，见交付说明）。
// ═══════════════════════════════════════════════════════════════════

/**
 * 取行政区划 SVG 的**裸文本**（全国 `100000` / 省 / 市 / 区县）。
 *
 * Tauri 侧命令（src-tauri/src/world_map/mod.rs，已逐字核对）：
 *   pub async fn world_map_geo_svg(
 *     app: AppHandle, ad: Option<String>, style: Option<String>,
 *     width: Option<f64>, height: Option<f64>, pad: Option<f64>,
 *     zoom: Option<i32>, labels: Option<bool>, dots: Option<bool>, stats: Option<bool>,
 *   ) -> Result<String, String>
 * 形参都是单词，JS 侧原样传即可（`world_map_geo_svg` 一条命令就够，
 * pad/labels/dots/stats 留给 Rust 默认值）。
 *
 * @param ad    行政区划 adcode（空值按全国处理）
 * @param style gaode / dark / water（三套主题仍由 Rust 渲染器决定，外壳不干预）
 * @param w     容器实际 CSS 像素宽
 * @param h     容器实际 CSS 像素高
 * @param zoom  1=只留主轮廓（省界/市界），2=多一层内阴影，3=全
 *
 * 失败一律 **throw**（空串、不是 SVG 都算失败）：调用方已有 catch → 页面提示 + 重试，
 * 绝不把 JSON 塞进 v-html 变成一屏乱码、也不让页面卡在「加载中…」。
 */
export async function geoSvgText(
  ad: string,
  style = 'gaode',
  w: number = MAP_SVG_DEFAULT_W,
  h: number = MAP_SVG_DEFAULT_H,
  zoom = 2,
): Promise<string> {
  const code = String(ad || '').trim() || '100000'
  const width = Math.max(64, Math.round(Number(w) || MAP_SVG_DEFAULT_W))
  const height = Math.max(64, Math.round(Number(h) || MAP_SVG_DEFAULT_H))
  const z = Math.min(3, Math.max(1, Math.round(Number(zoom) || 2)))

  // ── ① 真壳：Tauri 命令（本地渲染，离线可用，只要有 geojson 缓存）──
  if (isTauriRuntime()) {
    const svg = await invoke<string>('world_map_geo_svg', { ad: code, style, width, height, zoom: z })
    const text = String(svg || '')
    if (!/<svg[\s>]/i.test(text.slice(0, 400))) {
      throw new Error(`world_map_geo_svg 没返回 SVG（ad=${code}）`)
    }
    return text
  }

  // ── ② 浏览器 / 局域网调试：/api/geo_svg ──
  // 这条路由**不认 zoom 之外的东西**也一样能用；出错时 fetchSvgText 会把 JSON 翻译成人话。
  const url =
    `${API_BASE}/api/geo_svg?ad=${encodeURIComponent(code)}` +
    `&style=${encodeURIComponent(style)}&w=${width}&h=${height}&zoom=${z}`
  return await fetchSvgText(url)
}
