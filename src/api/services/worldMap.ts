// 世界地图数据层（T6-1）
//
// 数据来源集中在这里，组件不感知底层：
//   · 现在：HTTP → Python 侧车服务（127.0.0.1:8790，runit 常驻）
//   · T6-5 完成后：切到 Tauri invoke（Rust 本地实现）
// 切换只需把 USE_RUST 改 true 并补 world_map_* 命令。
import { invoke } from '@tauri-apps/api/core'

/** 是否已切到 Rust 本地实现（T6-5） */
export const USE_RUST = false

const API_BASE = 'http://127.0.0.1:8790'

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

export default worldMapApi
