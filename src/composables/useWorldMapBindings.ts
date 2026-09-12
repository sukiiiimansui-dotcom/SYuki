// 把 LingChat 的角色 / 对话 / 记忆 / 日程数据接到世界地图上（T6-4）
//
// 目标（用户明确要求）：
//   · 地图上的 NPC 只有**LingChat 角色列表里的角色**才带头像，其余只是点
//   · 手机通讯录 = LingChat 角色；手机日程 = LingChat 日程（T4-5 已解析）
//   · 角色在地图上的位置由**日程**决定（在上班 → 出现在商业设施）
import { convertFileSrc } from '@tauri-apps/api/core'
import { characterGetAll, getCharacterFilePath } from '@/api/services/character'
import type { Character } from '@/types'
import worldMapApi, { isTauriRuntime, type SchedulePayload } from '@/api/services/worldMap'
import { loadWorldModules, worldModule } from './useWorldModules'

export interface WorldCharacter {
  id: string
  name: string
  persona: string
  avatarUrl: string
  folder: string
}

let cachedCharacters: WorldCharacter[] | null = null
let lastBind = 0

/** 读 LingChat 角色列表，并把头像转成可显示的 asset URL */
export async function loadWorldCharacters(force = false): Promise<WorldCharacter[]> {
  if (cachedCharacters && !force) return cachedCharacters
  const out: WorldCharacter[] = []
  try {
    const res = await characterGetAll(1, 50)
    for (const c of (res?.items || []) as Character[]) {
      const folder = c.resource_folder || c.title || c.name || ''
      let avatarUrl = ''
      try {
        const rel = (c as any).avatar_path
        if (rel) {
          const abs = await getCharacterFilePath(rel)
          if (abs) avatarUrl = convertFileSrc(abs)
        }
      } catch {
        avatarUrl = ''
      }
      out.push({
        id: String(c.character_id ?? folder),
        name: c.name || c.title || folder,
        persona: (c.info || '').slice(0, 200),
        avatarUrl,
        folder,
      })
    }
  } catch (e1) {
    // LingChat 自己的角色接口没拿到（不在 Tauri 环境 / 接口异常）：退回「后端角色目录」。
    // 两条通路：
    //   · 浏览器预览 → HTTP `/api/schedule/chars`（老路，调试服务上有这个路由）；
    //   · 真壳（APK / 桌面）→ **没有**这个 HTTP 路由，也没有对应的 Tauri 命令，
    //     但 `world_map_schedule` 的返回里本来就带 `characters`
    //     （Rust 侧 schedule::payload() 与 HTTP 版 /api/schedule 同形），
    //     直接复用它 —— 同一份数据，还少一次请求，也不用新加 Rust 命令。
    try {
      let chars: { name?: string; folder?: string; info?: string }[] = []
      if (isTauriRuntime()) {
        const s = await worldMapApi.schedule()
        chars = s?.characters || []
      } else {
        const r = await fetch(`${worldMapApi.apiBase}/api/schedule/chars`, { cache: 'no-store' })
        if (!r.ok) throw new Error(`/api/schedule/chars 返回 ${r.status}`)
        const d = (await r.json()) as { characters?: { name?: string; folder?: string; info?: string }[] }
        chars = d?.characters || []
      }
      for (const c of chars) {
        out.push({
          id: c.folder || c.name || '',
          name: c.name || c.folder || '',
          persona: (c.info || '').slice(0, 200),
          avatarUrl: '',
          folder: c.folder || '',
        })
      }
    } catch (e2) {
      // ⚠️ 这里**故意不再静默**：两条通路都失败时，地图上就没有带头像的 NPC，
      // 现象是「通讯录空的、NPC 全是点」——不打印的话真机上根本没法定位。
      // 用中文 warn（便于真机抓日志），并且不抛错：地图本身还要能用。
      console.warn(
        '[世界地图] 角色名单两条通路都没拿到，地图/通讯录里不会有 LingChat 角色：',
        isTauriRuntime() ? '通路=world_map_schedule（真壳）' : '通路=/api/schedule/chars（浏览器）',
        { 角色接口错误: e1, 降级接口错误: e2 },
      )
    }
  }
  cachedCharacters = out
  return out
}

/** 注入到 npc.js：只有名单里的角色才升级为带头像的 named NPC */
export function bindCharactersToNpc(chars: WorldCharacter[]) {
  const NPC = worldModule<any>('NPC_SYS')
  if (!NPC || typeof NPC.setNamedCharacters !== 'function') return 0
  NPC.setNamedCharacters(
    chars.map((c) => ({
      id: c.id,
      name: c.name,
      avatarUrl: c.avatarUrl,
      persona: c.persona,
    })),
  )
  return chars.length
}

/** 注入到 phone.js：通讯录 = LingChat 角色 */
export function bindCharactersToPhone(chars: WorldCharacter[]) {
  const PHONE = worldModule<any>('PHONE')
  if (!PHONE || typeof PHONE.setContacts !== 'function') return 0
  PHONE.setContacts(
    chars.map((c) => ({
      name: c.name,
      avatar: c.avatarUrl,
      signature: c.persona.slice(0, 40),
    })),
  )
  return chars.length
}

/** 日程 → 手机日程界面（数据来自 T4-5 的 /api/schedule） */
export function bindScheduleToPhone(sch: SchedulePayload | null) {
  const PHONE = worldModule<any>('PHONE')
  if (!PHONE || typeof PHONE.setSchedule !== 'function' || !sch) return 0
  const list: any[] = []
  // 角色日程 → 按「时间 内容」排成待办样式
  for (const r of sch.roles || []) {
    for (const it of r.timeline || []) {
      list.push({ time: it.time, title: `${r.name}：${it.content || it.name}`, tag: it.kindZh, done: false })
    }
  }
  // LingChat 自己的待办
  for (const t of sch.todos || []) {
    list.push({
      time: (t as any).deadline || '',
      title: String((t as any).text ?? ''),
      tag: String((t as any).group ?? '待办'),
      done: !!(t as any).completed,
    })
  }
  list.sort((a, b) => String(a.time).localeCompare(String(b.time)))
  PHONE.setSchedule(list)
  return list.length
}

/**
 * 天气 → 手机天气界面。
 *
 * 为什么需要这一步：`public/world_map/phone.js` 自己写死了 `fetch('/api/weather')`，
 * 而 APK 里没有那个 HTTP 服务 —— 手机天气页会一直空着。数据层（worldMapApi）已经能
 * 双通路取到天气，这里负责把它喂给 phone.js 的界面。
 * 拿不到天气时返回 0（不注入），让手机页保持"暂无数据"而不是显示上一次的陈旧值。
 */
export function bindWeatherToPhone(w: any) {
  const PHONE = worldModule<any>('PHONE')
  if (!PHONE || typeof PHONE.setWeather !== 'function' || !w) return 0
  if (w.error) return 0
  const cur = w?.current || w?.weather?.current || w
  PHONE.setWeather({
    city: cur?.city ?? '',
    desc: cur?.desc ?? cur?.weather_desc ?? '',
    temp: cur?.temp_c ?? cur?.tempC ?? cur?.temp ?? null,
    humidity: cur?.humidity ?? null,
    wind: cur?.wind_kmph ?? null,
    isRain: !!(cur?.is_rain ?? cur?.isRain),
    isSnow: !!(cur?.is_snow ?? cur?.isSnow),
    isFog: !!(cur?.is_fog ?? cur?.isFog),
  })
  return 1
}

/** 一次性绑定（页面进来时调一次即可，内部有节流） */
export async function bindWorldData(opts?: { force?: boolean }): Promise<{
  characters: number
  schedule: number
  source: string
}> {
  await loadWorldModules()
  const now = Date.now()
  if (!opts?.force && now - lastBind < 4000) {
    return { characters: cachedCharacters?.length || 0, schedule: 0, source: 'throttled' }
  }
  lastBind = now
  const chars = await loadWorldCharacters(opts?.force)
  const nNpc = bindCharactersToNpc(chars)
  const nPhone = bindCharactersToPhone(chars)
  let sch: SchedulePayload | null = null
  let source = 'none'
  try {
    sch = await worldMapApi.schedule()
    source = sch?.source || 'unknown'
    bindScheduleToPhone(sch)
  } catch {
    sch = null
  }
  // 天气：phone.js 自己写死了 fetch('/api/weather')，真壳里拿不到 —— 由这里喂给它。
  // 失败不抛（拿不到天气不该让整轮 bind 失败），手机页会保持"暂无数据"。
  try {
    bindWeatherToPhone(await worldMapApi.weather())
  } catch {
    /* 天气拿不到就算了，不影响角色与日程绑定 */
  }
  return {
    characters: Math.max(nNpc, nPhone),
    schedule: (sch?.roles || []).length,
    source,
  }
}
