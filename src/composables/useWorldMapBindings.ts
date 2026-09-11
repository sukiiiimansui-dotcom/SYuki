// 把 LingChat 的角色 / 对话 / 记忆 / 日程数据接到世界地图上（T6-4）
//
// 目标（用户明确要求）：
//   · 地图上的 NPC 只有**LingChat 角色列表里的角色**才带头像，其余只是点
//   · 手机通讯录 = LingChat 角色；手机日程 = LingChat 日程（T4-5 已解析）
//   · 角色在地图上的位置由**日程**决定（在上班 → 出现在商业设施）
import { convertFileSrc } from '@tauri-apps/api/core'
import { characterGetAll, getCharacterFilePath } from '@/api/services/character'
import type { Character } from '@/types'
import worldMapApi, { type SchedulePayload } from '@/api/services/worldMap'
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
  } catch (e) {
    // 不在 Tauri 环境（纯浏览器调试）或接口异常：退回用后端的角色目录
    try {
      const r = await fetch(`${worldMapApi.apiBase}/api/schedule/chars`, { cache: 'no-store' })
      const d = await r.json()
      for (const c of d?.characters || []) {
        out.push({
          id: c.folder || c.name,
          name: c.name,
          persona: (c.info || '').slice(0, 200),
          avatarUrl: '',
          folder: c.folder || '',
        })
      }
    } catch {
      /* 两边都拿不到就保持空 */
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
  return {
    characters: Math.max(nNpc, nPhone),
    schedule: (sch?.roles || []).length,
    source,
  }
}
