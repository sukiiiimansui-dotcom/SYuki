// 「世界模拟」P5-2：现实事件引擎的**前端数据层** —— tick 轮询 + 广播订阅 + 三通道开关 + 记忆交接
//
// 分工（与 P1/P2/P4 三件套保持一致）：
//   · 纯逻辑（分类 / 历史 / 轮询节奏 / 气泡换算 / 回包分支）→ 本文件顶部的**导出纯函数**
//     （不 import vue、不碰 DOM、不碰 Tauri，自检里能直接跑断言）
//   · 本文件下半部分：composable（定时器、invoke、listen、localStorage）
//   · 组件只负责画：`WsEventFeed.vue`（面板 + 三通道开关）、`WsEventBubble.vue`（地图气泡）
//
// ══ 后端契约（逐字照抄 `src-tauri/src/world_map/event_cmd.rs`，别猜）══════════
//
// ① `world_map_tick({ role? })` —— 事件引擎的**驱动器**（兼心跳），**永远返回 Ok**：
//      { ok:true, fired:true,  at, role, next_ok_in_secs,
//        event:{id,category,title,text,weight_used,at_secs},
//        popup, bubble, speech_hint, memory_line }
//      { ok:true, fired:false, reason:"no_candidate"|"throttled"|"no_scene", at, role, next_ok_in_secs }
//    引擎自己的全局节流是 300 秒（`events::MIN_GAP_SECS`），所以调得再快也不会更密。
//    ⚠️ `no_scene` = 前端没把场景推上去（`world_map_update_runtime` 的 `scene`），
//       这时后端**不写任何事件/记忆**（守「别污染对话记忆」的纪律）。
//
// ② Tauri 事件 `world_map:event`（与 tick 同时广播，抽中时才发）：
//      载荷 = { event, popup, bubble, speech_hint, memory_line, at, role }
//    —— 与 tick 的返回值**是同一件事的两条通路**，所以两边必须去重（见 `seenKeys`）。
//    ⚠️ 只在真壳里 `listen`（`isTauriRuntime()` 守卫）：web 预览会抛。
//
// ③ `world_map_events_recent({limit?})` → `{ ok, items:[PlannedEvent], count, limit }`
//    默认 20、上限 100，**时间正序的尾巴**（最早在前）—— 要「最新在上」得自己 reverse。
//
// ④ `world_map_take_pending_memory({role?})` → `{ ok, role, lines:[String], count,
//    items:[{role,line,at}], pending_after }` —— **drain 语义（取走即清空）**，
//    `role` 省略 = 全取。
//
// ══ 三条纪律 ═══════════════════════════════════════════════════════════════
//
// ① **轮询是主通路，广播只是「更快」**：手机切后台/息屏会把 WebView 冻住、订阅晚一步、
//    事件也可能丢 —— 所以 `world_map_tick` 的定时链才是发动机，listen 只是让它更跟手。
//    定时器一律 `setTimeout` 链（**不用 `setInterval`**）：切换前后台改频率时不会
//    「旧频率还排着一发」。
//
// ② **降频不暂停**：页面隐藏时降到 60 秒（完全停掉会让角色在你回来时「错过一段人生」），
//    回到前台**立刻对齐**（马上 tick 一次 + drain 一次）。tick 同时兼心跳，所以任何
//    睡眠都封顶 60 秒 —— 拿到 `next_ok_in_secs` 只能「顺势多睡一会儿」，不能睡到 300 秒。
//
// ③ **`speech`（角色口述）前端不做额外事**：事件文本已经由后端写进 `MapRuntime.events`，
//    `summary.rs` 的注入会把它变成对话上下文里的「最近：…」，角色下一轮自然知道。
//    所以这个开关**只**控制前端展示（面板里「角色会这样说」的预览），
//    绝不自己发明一个后台字段去假装能拦住注入（详见 `setChannel` 的注释）。

import { computed, getCurrentScope, onScopeDispose, ref, type Ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { isTauriRuntime } from '@/api/services/worldMap'
import { letterboxOf, gridToBox } from '@/components/views/worldsim/wsActors'

/* ══════════════════════════════════════════════════════════════════════════
 * 一、类型：与后端逐字对齐（字段名就是契约）
 * ══════════════════════════════════════════════════════════════════════════ */

/** 十大类别（`events.rs::Category::key()`，也是事件 id 的 `.` 前缀） */
export type WsEventCategory =
  | 'weather'
  | 'traffic'
  | 'social'
  | 'work'
  | 'health'
  | 'money'
  | 'accident'
  | 'festival'
  | 'mood'
  | 'luck'

/** 认不出的类别（老事件/脏数据）也给它一个位置，界面不许开天窗 */
export type WsEventCategoryOrUnknown = WsEventCategory | 'unknown'

/** 事件本体（= `PlannedEvent` 的序列化形状） */
export interface WsEventItem {
  id: string
  category: WsEventCategoryOrUnknown
  title: string
  text: string
  weight_used?: number
  at_secs: number
}

/** 弹窗通道用的 kind（与 `wsToast` 的 `WsToastKind` 结构一致；这里不 import 组件层类型） */
export type WsPopupKind = 'info' | 'ok' | 'warn' | 'err'

/** 类别 → 徽章 emoji / i18n 词条 / 弹窗配色 */
export interface WsCategoryMeta {
  emoji: string
  /** i18n 键（`worldsim.events.category.*`） */
  labelKey: string
  /** 弹窗通道的 kind：好事 ok / 坏事 warn / 中性 info */
  kind: WsPopupKind
}

/**
 * 十类 + unknown 的元数据。
 *
 * 配色口径（**粗映射，宁可 info 也不误报**）：麻烦事（天气/交通/健康/意外）warn、
 * 喜事（节日）ok、其余（社交/工作学习/消费/情绪/小确幸）info —— 它们本来就正负混在一起，
 * 用一个明确的颜色去断言「这是好事」反而是错的。
 */
export const WS_CATEGORY_META: Record<WsEventCategoryOrUnknown, WsCategoryMeta> = {
  weather: { emoji: '🌦', labelKey: 'worldsim.events.category.weather', kind: 'warn' },
  traffic: { emoji: '🚦', labelKey: 'worldsim.events.category.traffic', kind: 'warn' },
  social: { emoji: '👥', labelKey: 'worldsim.events.category.social', kind: 'info' },
  work: { emoji: '💼', labelKey: 'worldsim.events.category.work', kind: 'info' },
  health: { emoji: '🩺', labelKey: 'worldsim.events.category.health', kind: 'warn' },
  money: { emoji: '💰', labelKey: 'worldsim.events.category.money', kind: 'info' },
  accident: { emoji: '⚡', labelKey: 'worldsim.events.category.accident', kind: 'warn' },
  festival: { emoji: '🎉', labelKey: 'worldsim.events.category.festival', kind: 'ok' },
  mood: { emoji: '🌤', labelKey: 'worldsim.events.category.mood', kind: 'info' },
  luck: { emoji: '🍀', labelKey: 'worldsim.events.category.luck', kind: 'info' },
  unknown: { emoji: '🔔', labelKey: 'worldsim.events.category.unknown', kind: 'info' },
}

/** 十类的 key 顺序（与后端 `Category::ALL` 一致；自检用它查「十类是否都有徽章」） */
export const WS_EVENT_CATEGORIES: WsEventCategory[] = [
  'weather',
  'traffic',
  'social',
  'work',
  'health',
  'money',
  'accident',
  'festival',
  'mood',
  'luck',
]

/** 类别元数据（认不出就给 unknown 那一份，绝不让界面空着） */
export function categoryMetaOf(cat: unknown): WsCategoryMeta {
  const k = String(cat || '') as WsEventCategoryOrUnknown
  return WS_CATEGORY_META[k] || WS_CATEGORY_META.unknown
}

/**
 * 一条事件属于哪一类：先看 `category` 字段，缺了就**从 id 前缀推**
 * （后端的 `Category::from_id` 同款约定：`"weather.rain"` → weather）。
 * 老事件可能只有 id，这条退路让它们也有徽章。
 */
export function categoryOf(ev: { category?: unknown; id?: unknown } | null | undefined): WsEventCategoryOrUnknown {
  const raw = String(ev?.category || '').trim().toLowerCase()
  if ((WS_EVENT_CATEGORIES as string[]).includes(raw)) return raw as WsEventCategory
  const prefix = String(ev?.id || '').trim().toLowerCase().split('.')[0]
  if ((WS_EVENT_CATEGORIES as string[]).includes(prefix)) return prefix as WsEventCategory
  return 'unknown'
}

/** 弹窗通道的 kind（好事 ok / 坏事 warn / 中性 info） */
export function popupKindOf(cat: unknown): WsPopupKind {
  return categoryMetaOf(cat).kind
}

/* ══════════════════════════════════════════════════════════════════════════
 * 二、纯函数：三通道开关（默认值 + 持久化）
 * ══════════════════════════════════════════════════════════════════════════ */

/** localStorage 键（与 `wsm:v1:state` / `wsm:v1:me-avatar` 同一套前缀纪律） */
export const WS_CHANNELS_KEY = 'wsm:v1:event-channels'

export interface WsEventChannels {
  /** 地图气泡：事件发生时在对应角色头上冒一下 */
  bubble: boolean
  /** 应用内提示条（`wsToast`） */
  popup: boolean
  /** 角色口述：只控前端展示（后端注入不受它影响，见文件头纪律③） */
  speech: boolean
}

/** 三路**默认全开**（机主定的：三种都要，且用户可选） */
export const WS_CHANNEL_DEFAULTS: WsEventChannels = { bubble: true, popup: true, speech: true }

/** 通道 key 的固定顺序（面板按它排按钮，自检按它遍历） */
export const WS_CHANNEL_KEYS: (keyof WsEventChannels)[] = ['bubble', 'popup', 'speech']

/**
 * 解析持久化的开关。
 *
 * 口径：**只认显式的 boolean**，坏 JSON / 缺字段 / 类型不对一律退回默认值（true）。
 * 为什么不「有值就是 true」：手改 localStorage 写成 `"false"` 这种字符串时，
 * 按 truthy 判会**打开**一个用户明确关掉的通道 —— 宁可回到默认。
 */
export function parseChannels(raw: string | null | undefined): WsEventChannels {
  const out: WsEventChannels = { ...WS_CHANNEL_DEFAULTS }
  if (!raw) return out
  let obj: unknown = null
  try {
    obj = JSON.parse(String(raw))
  } catch {
    return out
  }
  if (!obj || typeof obj !== 'object' || Array.isArray(obj)) return out
  const rec = obj as Record<string, unknown>
  for (const k of WS_CHANNEL_KEYS) {
    if (typeof rec[k] === 'boolean') out[k] = rec[k] as boolean
  }
  return out
}

/** 序列化（只写这三个键，别的键不落盘） */
export function serializeChannels(c: Partial<WsEventChannels> | null | undefined): string {
  const out: Record<string, boolean> = {}
  for (const k of WS_CHANNEL_KEYS) {
    out[k] = typeof c?.[k] === 'boolean' ? (c[k] as boolean) : WS_CHANNEL_DEFAULTS[k]
  }
  return JSON.stringify(out)
}

/* ══════════════════════════════════════════════════════════════════════════
 * 三、纯函数：事件历史（去重 / 倒序 / 容量上限）
 * ══════════════════════════════════════════════════════════════════════════ */

/** 内存里留最近多少条（面板列表用它；后端环形缓冲只有 20 条，所以回填也回不满） */
export const WS_EVENT_HISTORY_MAX = 50

/** 一条事件的身份：`id@时刻` —— tick 返回值与广播是同一件事的两条通路，靠它去重 */
export function eventKeyOf(ev: { id?: unknown; at_secs?: unknown } | null | undefined): string {
  const id = String(ev?.id || '').trim()
  const at = Math.trunc(Number(ev?.at_secs) || 0)
  return `${id}@${at}`
}

/** 任意 JSON → 事件（认不出返回 null：宁可少一条，也不显示半条垃圾） */
export function normalizeEvent(raw: unknown): WsEventItem | null {
  if (!raw || typeof raw !== 'object') return null
  const o = raw as Record<string, unknown>
  const id = typeof o.id === 'string' ? o.id.trim() : ''
  const title = typeof o.title === 'string' ? o.title.trim() : ''
  const text = typeof o.text === 'string' ? o.text.trim() : ''
  const at = Math.trunc(Number(o.at_secs) || 0)
  // id 都没有的条目对「去重 / 冷却」毫无意义（与后端 `recent_one` 同款判断）
  if (!id) return null
  const w = Number(o.weight_used)
  return {
    id,
    category: categoryOf({ category: o.category, id }),
    title: title || text,
    text,
    ...(Number.isFinite(w) ? { weight_used: w } : {}),
    at_secs: at,
  }
}

/**
 * 把一条事件并进历史（**最新在前**）。
 *
 * 返回值有三种情况，调用方据此省掉无意义的重渲染：
 *   · 没变化（重复事件 / 空数据）→ **原数组**（引用不变，Vue 不会重渲染）；
 *   · 有新事件 → 新数组（`[ev, ...旧]`，按容量裁尾）。
 * 「重复」的判据是 [`eventKeyOf`]：同一条事件经 tick 与广播各来一次时只算一条。
 */
export function pushEventHistory(
  list: WsEventItem[],
  raw: unknown,
  max = WS_EVENT_HISTORY_MAX,
): WsEventItem[] {
  const cap = Math.max(1, Math.trunc(Number(max) || WS_EVENT_HISTORY_MAX))
  const ev = normalizeEvent(raw)
  if (!ev) return list
  const key = eventKeyOf(ev)
  if ((list || []).some((x) => eventKeyOf(x) === key)) return list
  return [ev, ...(list || [])].slice(0, cap)
}

/**
 * `world_map_events_recent` 的回包 → 历史（最新在前）。
 *
 * 后端给的是**时间正序的尾巴**（最早在前），所以这里 reverse 一次；顺手去重 + 裁到上限。
 * 回包坏掉（不是对象 / items 不是数组）→ 返回空数组，**不抛**（面板显示空态比崩了强）。
 */
export function historyFromRecent(raw: unknown, max = WS_EVENT_HISTORY_MAX): WsEventItem[] {
  const cap = Math.max(1, Math.trunc(Number(max) || WS_EVENT_HISTORY_MAX))
  const items = (raw as { items?: unknown } | null | undefined)?.items
  if (!Array.isArray(items)) return []
  const seen = new Set<string>()
  const out: WsEventItem[] = []
  for (const raw of items) {
    const ev = normalizeEvent(raw)
    if (!ev) continue
    const k = eventKeyOf(ev)
    if (seen.has(k)) continue
    seen.add(k)
    out.push(ev)
  }
  out.reverse() // 后端的「最早在前」→ 界面的「最新在上」
  return out.slice(0, cap)
}

/**
 * 回填的历史与本地已收到的事件**合并**（最新在前，按 `id@时刻` 去重）。
 *
 * 为什么不能直接 `events = serverList`：`refreshHistory()` 可能在一条事件刚落地的
 * 同一瞬间返回（面板展开、开始轮询时都会拉历史），而后端那份快照是**读之前**拍的 ——
 * 直接覆盖会把刚冒出来的那条从列表里抹掉；更糟的是它已经进过 `seenKeys`，
 * 下一次 tick/广播会被当成重复而不再补回来，表现为「事件一闪就没了」。
 * 所以：服务端给的是权威的尾巴，本地那份（更新鲜）覆盖同名条目，再按时刻倒序裁到上限。
 */
export function mergeHistory(
  server: WsEventItem[] | null | undefined,
  local: WsEventItem[] | null | undefined,
  max = WS_EVENT_HISTORY_MAX,
): WsEventItem[] {
  const cap = Math.max(1, Math.trunc(Number(max) || WS_EVENT_HISTORY_MAX))
  const map = new Map<string, WsEventItem>()
  // 两份都过一遍 normalizeEvent：认不出的条目直接丢掉（合并是最后一道闸，
  // 脏数据不该从这里混进界面）
  for (const raw of server || []) {
    const e = normalizeEvent(raw)
    if (e) map.set(eventKeyOf(e), e)
  }
  for (const raw of local || []) {
    const e = normalizeEvent(raw)
    if (e) map.set(eventKeyOf(e), e)
  }
  return [...map.values()]
    .sort((a, b) => (Math.trunc(Number(b.at_secs) || 0) - Math.trunc(Number(a.at_secs) || 0)))
    .slice(0, cap)
}

/* ══════════════════════════════════════════════════════════════════════════
 * 四、纯函数：tick 回包的三种形态 → 一个确定的判定
 * ══════════════════════════════════════════════════════════════════════════ */

/** `fired` / 三种「没抽中」的 reason / 回包本身坏掉 */
export type WsTickReason = 'fired' | 'no_candidate' | 'throttled' | 'no_scene' | 'unknown' | 'error'

export interface WsTickOutcome {
  reason: WsTickReason
  fired: boolean
  /** 距「下一次最早可能出事」还有多少秒（可用来顺势多睡一会儿） */
  nextOkInSecs: number
  atSecs: number
  /** 这条事件是给谁的（后端 `resolve_role` 的结果；空 = 占位符渲染成「你」） */
  role: string
  event: WsEventItem | null
  popup: string
  bubble: string
  speechHint: string
  memoryLine: string
  error: string
}

/** 一次「真的出事了」的全部文案（tick 与广播归一成同一个形状） */
export interface WsFiredEvent {
  event: WsEventItem | null
  popup: string
  bubble: string
  speechHint: string
  memoryLine: string
  atSecs: number
  role: string
  /** 从哪条通路来的（排障用：正常一次事件会同时被两条通路看到，去重后只剩先到的那个） */
  source: 'tick' | 'event'
}

function str(v: unknown): string {
  return typeof v === 'string' ? v : ''
}
function numOr(v: unknown, d = 0): number {
  const n = Number(v)
  return Number.isFinite(n) ? n : d
}

/**
 * `world_map_tick` 的回包 → 判定（**纯函数**，自检的主战场）。
 *
 * 覆盖三种形态 + 两种坏情况：
 *   · `fired:true`  → reason='fired'，文案与事件都取出来；
 *   · `fired:false` → reason 取后端给的（`no_candidate` / `throttled` / `no_scene`），
 *                     认不出的一律 'unknown'（不假装知道原因）；
 *   · `ok!==true` / 不是对象 → reason='error'，把原因写进 `error` 字段（面板如实显示）；
 *   · `fired:true` 但 `event` 是垃圾（缺 id）→ 只要有文案就仍算一条事件（`event:null`，
 *     气泡/提示条照常弹），文案也空才算 error —— 「说不清但确实出事了」不该静默丢掉。
 */
export function classifyTick(resp: unknown): WsTickOutcome {
  const empty = (reason: WsTickReason, error = ''): WsTickOutcome => ({
    reason,
    fired: false,
    nextOkInSecs: 0,
    atSecs: 0,
    role: '',
    event: null,
    popup: '',
    bubble: '',
    speechHint: '',
    memoryLine: '',
    error,
  })
  if (!resp || typeof resp !== 'object') return empty('error', 'world_map_tick 没有返回对象')
  const o = resp as Record<string, unknown>
  if (o.ok !== true) return empty('error', str(o.error) || 'world_map_tick 返回 ok:false')
  const base: WsTickOutcome = {
    ...empty('unknown'),
    atSecs: Math.trunc(numOr(o.at, 0)),
    role: str(o.role).trim(),
    nextOkInSecs: Math.max(0, Math.trunc(numOr(o.next_ok_in_secs, 0))),
  }
  const event = normalizeEvent(o.event)
  const popup = str(o.popup).trim()
  const bubble = str(o.bubble).trim()
  const speechHint = str(o.speech_hint).trim()
  const memoryLine = str(o.memory_line).trim()
  if (o.fired === true) {
    const hasSomething = !!event || !!popup || !!bubble || !!speechHint || !!memoryLine
    if (!hasSomething) return { ...base, reason: 'error', error: 'fired:true 但四条通道都是空的' }
    return { ...base, reason: 'fired', fired: true, event, popup, bubble, speechHint, memoryLine }
  }
  const reason = str(o.reason).trim()
  const known: WsTickReason[] = ['no_candidate', 'throttled', 'no_scene']
  return { ...base, reason: (known as string[]).includes(reason) ? (reason as WsTickReason) : 'unknown' }
}

/** `world_map:event` 的广播载荷 → 同一种「出事了」（形状坏掉返回 null，静默丢掉） */
export function firedFromPayload(raw: unknown): WsFiredEvent | null {
  if (!raw || typeof raw !== 'object') return null
  const o = raw as Record<string, unknown>
  const event = normalizeEvent(o.event)
  const popup = str(o.popup).trim()
  const bubble = str(o.bubble).trim()
  const speechHint = str(o.speech_hint).trim()
  const memoryLine = str(o.memory_line).trim()
  if (!event && !popup && !bubble && !speechHint && !memoryLine) return null
  return {
    event,
    popup,
    bubble,
    speechHint,
    memoryLine,
    atSecs: Math.trunc(numOr(o.at, 0)),
    role: str(o.role).trim(),
    source: 'event',
  }
}

/** tick 的判定 → 「出事了」（没出事就是 null） */
export function firedOfTick(o: WsTickOutcome | null | undefined): WsFiredEvent | null {
  if (!o || !o.fired) return null
  return {
    event: o.event,
    popup: o.popup,
    bubble: o.bubble,
    speechHint: o.speechHint,
    memoryLine: o.memoryLine,
    atSecs: o.atSecs,
    role: o.role,
    source: 'tick',
  }
}

/* ══════════════════════════════════════════════════════════════════════════
 * 五、纯函数：轮询节奏（前台 15s / 隐藏 60s / 顺势睡到 next_ok_in_secs）
 * ══════════════════════════════════════════════════════════════════════════ */

/** 前台轮询间隔（毫秒）。15 秒：比引擎的 300 秒节流密得多，但它同时是心跳 */
export const WS_POLL_MS = 15_000
/** 页面隐藏时的间隔（**降频不暂停**：完全停掉会让角色在你回来时「错过一段人生」） */
export const WS_HIDDEN_POLL_MS = 60_000
/** 任何一次睡眠的上限 = 心跳上限（`next_ok_in_secs` 再大也不能睡过头） */
export const WS_MAX_SLEEP_MS = 60_000
/** 睡眠下限：`next_ok_in_secs` 是 1 秒时也别把 tick 打成死循环 */
export const WS_MIN_SLEEP_MS = 1_000

export interface WsPollDelayInput {
  hidden?: boolean
  /** 上一条 tick 回包的 `next_ok_in_secs`（>0 = 引擎节流中，可以顺势多睡一会儿） */
  nextOkInSecs?: number
  pollMs?: number
  hiddenPollMs?: number
  maxSleepMs?: number
  minSleepMs?: number
}

/**
 * 下一次 tick 该在多长时间之后（**纯函数**）。
 *
 * 规则（顺序即优先级）：
 *   · 没拿到 `next_ok_in_secs`（≤0）→ 用基准间隔：前台 15s / 隐藏 60s；
 *   · 拿到了 → 睡到那个点，但**封顶 [`WS_MAX_SLEEP_MS`]（60s）**：tick 兼心跳，
 *     睡到 300 秒之后再回来，用户的「回来立刻对齐」就变成了「回来等五分钟」；
 *   · **隐藏时基准是下限**：`max(60s, 睡到那个点)` —— 息屏时省电优先，
 *     而引擎自己节流 300 秒，晚几分钟问一次也不会漏事件。
 */
export function nextPollDelayMs(input: WsPollDelayInput = {}): number {
  const pollMs = Math.max(250, Math.trunc(numOr(input.pollMs, WS_POLL_MS)))
  const hiddenMs = Math.max(pollMs, Math.trunc(numOr(input.hiddenPollMs, WS_HIDDEN_POLL_MS)))
  const maxSleep = Math.max(pollMs, Math.trunc(numOr(input.maxSleepMs, WS_MAX_SLEEP_MS)))
  const minSleep = Math.max(100, Math.trunc(numOr(input.minSleepMs, WS_MIN_SLEEP_MS)))
  const base = input.hidden ? hiddenMs : pollMs
  const next = Math.trunc(numOr(input.nextOkInSecs, 0))
  if (next <= 0) return base
  const want = Math.min(Math.max(next * 1000, minSleep), maxSleep)
  return input.hidden ? Math.max(base, want) : want
}

/* ══════════════════════════════════════════════════════════════════════════
 * 六、纯函数：地图气泡的锚点（复用 `wsActors` 的信箱折算）
 * ══════════════════════════════════════════════════════════════════════════ */

/** 气泡底边离头像中心多少 CSS 像素（屏幕上恒定；换算见 [`bubbleAnchorOf`]） */
export const WS_BUBBLE_GAP_PX = 16

/** 同屏最多几条气泡（再多就是刷屏；旧的会被挤掉） */
export const WS_BUBBLE_MAX = 4

export interface WsBubbleAnchor {
  /** 盒子内 CSS 像素（写进 left/top，父级手势 transform 会把它一起带走） */
  x: number
  y: number
  /** 元素自己的缩放：`1/zoom`，抵消地图手势的放大（头像同款做法） */
  scale: number
  /** 找到对应角色了吗（没找到 → 组件退回「屏幕中央浮一个」，绝不静默丢弃） */
  found: boolean
}

/**
 * 事件气泡该落在哪（**纯函数**）。
 *
 * 三处细节（少一个就对不上头像）：
 *   ① 信箱折算用 `wsActors.letterboxOf` —— 与 `WsAvatarMark` / `WsVehicleMark` 同一套数学，
 *      地图是正方形 viewBox 装在矩形盒子里，不算信箱就会整体偏移（只在非正方形盒子上偏）；
 *   ② 坐标优先用 `px/py`（**错开之后**的位置）：头像画的就是它，用原始 `gx/gy` 的话
 *      几个人挤在一起时气泡会飘到别人头上（gx/gy 是给「这个人在哪块地」用的）；
 *   ③ 上抬 `gap/zoom` 而不是 `gap`：元素自己缩了 `1/zoom`，写在**布局坐标**里的位移
 *      会被父级再放大 `zoom` 倍 —— 除一下，屏幕上才是恒定的 `gap` 像素。
 *
 * `found:false`（角色不在名单里 / 没坐标）时返回盒子中心，由调用方决定怎么画
 * （组件那边走「屏幕中央浮一个」的分支）。
 */
export function bubbleAnchorOf(
  actor: { gx?: unknown; gy?: unknown; px?: unknown; py?: unknown } | null | undefined,
  boxW: number,
  boxH: number,
  grid = 28,
  zoom = 1,
  gap = WS_BUBBLE_GAP_PX,
): WsBubbleAnchor {
  const w = Math.max(1, numOr(boxW, 1))
  const h = Math.max(1, numOr(boxH, 1))
  const k = Number.isFinite(Number(zoom)) && Number(zoom) > 0 ? Number(zoom) : 1
  const scale = 1 / k
  const gapPx = Math.max(0, numOr(gap, WS_BUBBLE_GAP_PX))
  const gx = numOr(actor?.px, NaN)
  const gy = numOr(actor?.py, NaN)
  const rx = Number.isFinite(gx) ? gx : numOr(actor?.gx, NaN)
  const ry = Number.isFinite(gy) ? gy : numOr(actor?.gy, NaN)
  if (!Number.isFinite(rx) || !Number.isFinite(ry)) {
    return { x: w / 2, y: h / 2, scale, found: false }
  }
  const lb = letterboxOf(w, h, Math.max(1, numOr(grid, 28)))
  const p = gridToBox(rx, ry, lb)
  return { x: p.x, y: p.y - gapPx / k, scale, found: true }
}

export interface WsBubbleItem {
  /** `id@时刻`（同一条事件只冒一个泡） */
  key: string
  /** 挂在谁头上（后端 `resolve_role` 的结果，空 = 没有指定人） */
  role: string
  text: string
  atSecs: number
}

/** 冒一个泡（去重 + 裁到 [`WS_BUBBLE_MAX`]；已有同 key 就原样返回，调用方不用再判） */
export function pushBubble(list: WsBubbleItem[], item: WsBubbleItem, max = WS_BUBBLE_MAX): WsBubbleItem[] {
  const cap = Math.max(1, Math.trunc(numOr(max, WS_BUBBLE_MAX)))
  if (!item || !item.text) return list
  const cur = list || []
  if (cur.some((b) => b.key === item.key)) return cur
  return [...cur, item].slice(-cap)
}

/* ══════════════════════════════════════════════════════════════════════════
 * 七、纯函数：相对时间 / drain 回包
 * ══════════════════════════════════════════════════════════════════════════ */

export type WsRelTimeKey = 'justNow' | 'minAgo' | 'hourAgo' | 'dayAgo'

/** 相对时间：**只给描述符，不给文案**（文案走 i18n，纯函数里不许出现中文） */
export function relTimeOf(atSecs: unknown, nowMs: number): { key: WsRelTimeKey; n: number } {
  const now = numOr(nowMs, Date.now()) / 1000
  const at = numOr(atSecs, 0)
  const diff = now - at
  // 设备时钟改到过去 / 事件时刻在未来：按「刚刚」显示，别出现「-3 分钟前」
  if (!Number.isFinite(diff) || diff < 60) return { key: 'justNow', n: 0 }
  if (diff < 3600) return { key: 'minAgo', n: Math.floor(diff / 60) }
  if (diff < 86400) return { key: 'hourAgo', n: Math.floor(diff / 3600) }
  return { key: 'dayAgo', n: Math.floor(diff / 86400) }
}

/** 一行待写记忆（`items[]` 的元素） */
export interface WsPendingLine {
  role: string
  line: string
  at: number
}

/**
 * `world_map_take_pending_memory` 的回包 → 行列表 + 队列剩余。
 *
 * 两种形状都吃：`items:[{role,line,at}]`（带归属的明细）与退化的 `lines:[String]`
 * （只有文本，角色名留空）。**回包里的 `lines` 是「这一次取走的」**——
 * drain 语义（取走即清空）在后端 `state.rs::take_memory`，前端只是把它显示出来。
 */
export function normalizeDrain(raw: unknown): { lines: WsPendingLine[]; pendingAfter: number } {
  const o = (raw || {}) as Record<string, unknown>
  const pendingAfter = Math.max(0, Math.trunc(numOr(o.pending_after, 0)))
  const items = o.items
  if (Array.isArray(items)) {
    const lines: WsPendingLine[] = []
    for (const it of items) {
      if (!it || typeof it !== 'object') continue
      const rec = it as Record<string, unknown>
      const line = str(rec.line).trim()
      if (!line) continue
      lines.push({ role: str(rec.role).trim(), line, at: Math.trunc(numOr(rec.at, 0)) })
    }
    return { lines, pendingAfter }
  }
  const flat = o.lines
  if (Array.isArray(flat)) {
    const lines: WsPendingLine[] = []
    for (const l of flat) {
      const line = str(l).trim()
      if (line) lines.push({ role: '', line, at: 0 })
    }
    return { lines, pendingAfter }
  }
  return { lines: [], pendingAfter }
}

/* ══════════════════════════════════════════════════════════════════════════
 * 八、composable：定时链 + 订阅 + 开关 + 记忆交接
 * ══════════════════════════════════════════════════════════════════════════ */

/** 定时 drain 的间隔（自然检查点之一：面板打开时、卸载前、以及每 60 秒一次） */
export const WS_DRAIN_EVERY_MS = 60_000
/** 气泡停留时长（毫秒）—— 3~5 秒之间，取 4.2 秒：够看清一句话，又不至于糊在图上 */
export const WS_BUBBLE_MS = 4_200

export interface UseWorldEventsOptions {
  /** 把当前说话的角色名传给 tick（`world_map_tick({role})`）；不传就让后端用 `current_role` */
  role?: string | Ref<string>
  pollMs?: number
  hiddenPollMs?: number
  maxHistory?: number
  /** 气泡停留时长（毫秒），默认 [`WS_BUBBLE_MS`] */
  bubbleMs?: number
  /** 建好就自动开始（默认 false：由页面在「真的进到小区图」时调 `start()`） */
  autoStart?: boolean
  /** 出事时的回调（页面拿它弹提示条 `wsToast`；三条通道的取舍在调用方，见 WorldSim.vue） */
  onFired?: (e: WsFiredEvent) => void
  /** 出错回调（轮询失败不中断，只上报） */
  onError?: (e: unknown) => void
}

function lsGet(key: string): string | null {
  try {
    if (typeof localStorage === 'undefined') return null
    return localStorage.getItem(key)
  } catch {
    return null // 隐私模式 / 存储被禁用：开关退回默认值，绝不因此崩掉
  }
}
function lsSet(key: string, value: string): void {
  try {
    if (typeof localStorage === 'undefined') return
    localStorage.setItem(key, value)
  } catch {
    /* 写不进去（配额满）不影响本次会话的开关状态 */
  }
}
function readRole(v: string | Ref<string> | undefined): string {
  const raw = typeof v === 'string' ? v : v?.value
  return String(raw || '').trim()
}

export function useWorldEvents(opts: UseWorldEventsOptions = {}) {
  const pollMs = Math.max(1000, numOr(opts.pollMs, WS_POLL_MS))
  const hiddenPollMs = Math.max(pollMs, numOr(opts.hiddenPollMs, WS_HIDDEN_POLL_MS))
  const maxHistory = Math.max(1, Math.trunc(numOr(opts.maxHistory, WS_EVENT_HISTORY_MAX)))
  const bubbleMs = Math.max(800, numOr(opts.bubbleMs, WS_BUBBLE_MS))

  /* ── 响应式状态 ─────────────────────────────────────────────────────── */
  /** 事件历史（最新在前，最多 `maxHistory` 条） */
  const events = ref<WsEventItem[]>([])
  /** 最近一条事件（面板高亮/预览用） */
  const latest = ref<WsEventItem | null>(null)
  /** 正在冒的气泡（组件按它画；到点自动消失） */
  const bubbles = ref<WsBubbleItem[]>([])
  /** 这一次 drain 取到的待写记忆行（**取走即清空**的后端语义在前端就是「替换」） */
  const pendingLines = ref<WsPendingLine[]>([])
  /** drain 之后队列里还剩几行（别人的） */
  const pendingAfter = ref(0)
  /** 上一条事件给模型的「你可以自然地提一句」提示（`speech` 开关关掉就清空） */
  const speechHint = ref('')
  const lastError = ref('')
  /** 上一条 tick 的判定（面板显示「为什么还没出事」） */
  const lastReason = ref<WsTickReason | ''>('')
  /** 上一条 tick 给的「下一次最早什么时候」 */
  const nextOkInSecs = ref(0)
  /** 相对时间的参照时钟（跟着 tick 走，不额外起定时器） */
  const nowMs = ref(Date.now())
  /** 页面是否隐藏（隐藏时降频，见 [`nextPollDelayMs`]） */
  const hidden = ref(false)
  /**
   * 这套命令在当前环境可不可用。web 预览（`__LINGCHAT_WEB_MOCK__`）里 `world_map_*`
   * 一律不存在 —— 那时**静默停摆**（不轮询、不报错、不弹 toast）。
   */
  const supported = ref(isTauriRuntime())
  const running = ref(false)
  /** 三通道开关（初值就地读 localStorage，坏数据退回全开） */
  const channels = ref<WsEventChannels>(parseChannels(lsGet(WS_CHANNELS_KEY)))

  /* ── 内部记账 ───────────────────────────────────────────────────────── */
  let pollTimer: number | null = null
  let drainTimer: number | null = null
  let unlisten: UnlistenFn | null = null
  let bound = false
  /** 已经处理过的事件（`id@时刻`）—— tick 与广播是同一件事的两条通路，必须去重 */
  const seenKeys = new Set<string>()
  /** 气泡的消失定时器 */
  const bubbleTimers = new Map<string, number>()

  /** 回调是上层写的，它炸了不该把数据层带走（与 useWorldTrips 同款） */
  function safe<T extends unknown[]>(fn: ((...a: T) => void) | undefined, ...a: T) {
    if (!fn) return
    try {
      fn(...a)
    } catch (e) {
      console.warn('[worldsim] event callback failed:', e)
    }
  }
  function errMsg(e: unknown): string {
    return e instanceof Error ? e.message : String(e)
  }

  /* ── 气泡 ───────────────────────────────────────────────────────────── */
  function dropBubble(key: string) {
    bubbles.value = bubbles.value.filter((b) => b.key !== key)
    const h = bubbleTimers.get(key)
    if (h !== undefined) {
      window.clearTimeout(h)
      bubbleTimers.delete(key)
    }
  }
  function clearBubbles() {
    for (const h of bubbleTimers.values()) window.clearTimeout(h)
    bubbleTimers.clear()
    bubbles.value = []
  }
  function showBubble(item: WsBubbleItem) {
    const next = pushBubble(bubbles.value, item)
    if (next === bubbles.value) return // 同一个泡已经在冒了
    bubbles.value = next
    // 被挤掉的那几条（超出同屏上限）也要把定时器清掉
    for (const k of [...bubbleTimers.keys()]) {
      if (!next.some((b) => b.key === k)) dropBubble(k)
    }
    const h = window.setTimeout(() => dropBubble(item.key), bubbleMs)
    bubbleTimers.set(item.key, h)
  }

  /* ── 一件事发生（tick 返回值与广播共用；去重后只处理先到的那个）────── */
  function applyFired(fired: WsFiredEvent | null): boolean {
    if (!fired) return false
    const key = fired.event
      ? eventKeyOf(fired.event)
      : `${fired.role}|${fired.atSecs}|${fired.bubble || fired.popup}`
    if (seenKeys.has(key)) return false
    seenKeys.add(key)
    // 记账表别无限长：超过 64 条就把早先的丢掉（丢弃只会让一条极老的重复事件重放一次，
    // 而那时它早就掉出历史窗口，界面上看不出差别）
    if (seenKeys.size > 64) {
      const first = seenKeys.values().next().value
      if (first !== undefined) seenKeys.delete(first)
    }

    if (fired.event) {
      events.value = pushEventHistory(events.value, fired.event, maxHistory)
      latest.value = events.value[0] || fired.event
    }
    nowMs.value = Date.now()
    // 口述提示：开关关掉就不留（前端展示层的取舍，后端注入不受影响）
    speechHint.value = channels.value.speech ? fired.speechHint : ''
    if (channels.value.bubble && fired.bubble) {
      showBubble({
        key,
        role: fired.role,
        text: fired.bubble,
        atSecs: fired.atSecs,
      })
    }
    safe(opts.onFired, fired)
    return true
  }

  /* ── 轮询 ───────────────────────────────────────────────────────────── */

  /** 打一次火（`invoke` 失败写进 lastError，**不抛给调用方**） */
  async function poll(): Promise<WsTickOutcome | null> {
    if (!supported.value) return null
    try {
      const args: Record<string, unknown> = {}
      const who = readRole(opts.role)
      if (who) args.role = who
      const r = await invoke<unknown>('world_map_tick', args)
      const o = classifyTick(r)
      lastReason.value = o.reason
      nextOkInSecs.value = o.nextOkInSecs
      lastError.value = o.reason === 'error' ? o.error : ''
      nowMs.value = Date.now()
      if (o.fired) applyFired(firedOfTick(o))
      return o
    } catch (e) {
      lastError.value = errMsg(e)
      lastReason.value = 'error'
      nextOkInSecs.value = 0
      safe(opts.onError, e)
      return null
    }
  }

  /**
   * 排下一次 tick。
   *
   * `setTimeout` 链 + 每次重算间隔（**不用 `setInterval`**）：切前后台时改频率，
   * 如果用的是 interval，旧频率的那一发还会排着，表现是「回到前台还是 60 秒一次」。
   */
  function schedule() {
    if (!running.value) return
    if (pollTimer !== null) window.clearTimeout(pollTimer)
    const ms = nextPollDelayMs({
      hidden: hidden.value,
      nextOkInSecs: nextOkInSecs.value,
      pollMs,
      hiddenPollMs,
    })
    pollTimer = window.setTimeout(() => {
      void poll().finally(schedule)
    }, ms)
  }

  /* ── 可见性：隐藏降频、回前台立刻对齐 ───────────────────────────────── */
  function onVisibility() {
    const isHidden = typeof document !== 'undefined' && document.visibilityState === 'hidden'
    hidden.value = isHidden
    if (!isHidden) {
      // 「回来立刻对齐」：马上 tick 一次（可能已经出过事了）+ 把待写记忆取回来
      nowMs.value = Date.now()
      void drainMemory()
      void poll().finally(schedule)
      return
    }
    schedule()
  }

  /* ── 事件历史回填 ───────────────────────────────────────────────────── */
  async function refreshHistory(): Promise<WsEventItem[]> {
    if (!supported.value) return events.value
    try {
      const r = await invoke<unknown>('world_map_events_recent', { limit: maxHistory })
      const list = historyFromRecent(r, maxHistory)
      // 合并而不是覆盖：拉历史与「刚抽中一条」可能撞在同一瞬间（见 mergeHistory 的说明）
      events.value = mergeHistory(list, events.value, maxHistory)
      if (events.value.length) latest.value = events.value[0]
      // 回填的历史也算「见过」：restart 之后同一批事件不会再弹一次气泡/提示条
      for (const e of list) seenKeys.add(eventKeyOf(e))
      lastError.value = ''
    } catch (e) {
      lastError.value = errMsg(e)
      safe(opts.onError, e)
    }
    return events.value
  }

  /* ── 记忆交接（drain）─────────────────────────────────────────────────
   * ⚠️ 前端**没有**「写记忆」命令可调，这里也**不猜**一个：当前后端的设计是
   *    事件文本已经通过 `runtime.events` 进了对话注入的「最近：…」那一行，
   *    所以角色本来就「知道」。drain 出来的行只做两件事：
   *      ① 让队列别攒满（`PENDING_MEMORY_MAX = 64`，满了丢最旧的）；
   *      ② 在面板的「待写记忆」区把「角色记下了什么」如实显示给用户看。
   */
  async function drainMemory(role?: string): Promise<WsPendingLine[]> {
    if (!supported.value) return []
    try {
      const args: Record<string, unknown> = {}
      const who = String(role || '').trim()
      if (who) args.role = who
      const r = await invoke<unknown>('world_map_take_pending_memory', args)
      const { lines, pendingAfter: after } = normalizeDrain(r)
      pendingLines.value = lines // 取走即清空 → 前端这份也整体替换（不是累加）
      pendingAfter.value = after
      lastError.value = ''
      return lines
    } catch (e) {
      lastError.value = errMsg(e)
      safe(opts.onError, e)
      return []
    }
  }

  /* ── 广播订阅（只是「更快」，不是主通路）────────────────────────────── */
  async function subscribe() {
    if (!supported.value || bound) return
    bound = true
    try {
      unlisten = await listen<unknown>('world_map:event', (ev) => {
        applyFired(firedFromPayload(ev?.payload))
      })
    } catch (e) {
      // web 预览 / 权限问题：订阅不上不影响轮询这条路
      unlisten = null
      safe(opts.onError, e)
    }
  }

  /* ── 三通道开关 ─────────────────────────────────────────────────────── */

  /**
   * 拨一个通道。
   *
   * ⚠️ **为什么这里没有 `world_map_update_runtime`**（原设计说「推一个标记」）：
   *    我逐字读了 Rust 侧认的 patch 键（`state.rs::apply_patch`：scene / me / actors /
   *    events / weather / facilities / cell_m / current_role / pending_move / clear_events）
   *    与事件引擎读的字段（`event_cmd.rs::build_context`），**没有任何一个能表达
   *    「别让角色提这件事」**：`scene` 虽然会字段级合并、塞什么键都存得下，
   *    但那就正是「自己发明后台字段」——后端不会读它，等于骗自己。
   *    而且 `clear_events` 这类现成键是**破坏性**的（会顺带清掉引擎的冷却统计，
   *    下一个 tick 可能立刻又出一条），用它实现「关掉口述」得不偿失。
   *    所以这个开关**只**控制前端展示（面板里的「角色会这样说」预览 + 气泡/提示条），
   *    后端注入不受影响 —— 这一点在面板的说明文字里也如实写了，报告里同样列成待补项。
   *    将来后端若加一个闸门（例如 `scene.speech_off`，在 `summary::render` 里判），
   *    这里补一行 `pushPatch({ scene: { speech_off: !on } })` 即可接上。
   */
  function setChannel(key: keyof WsEventChannels, on: boolean): void {
    if (!WS_CHANNEL_KEYS.includes(key)) return
    const next: WsEventChannels = { ...channels.value, [key]: !!on }
    channels.value = next
    lsSet(WS_CHANNELS_KEY, serializeChannels(next))
    // 关掉气泡：已经在冒的也收掉（否则「关了还冒」看起来像坏了）
    if (key === 'bubble' && !on) clearBubbles()
    if (key === 'speech' && !on) speechHint.value = ''
  }

  /* ── 生命周期 ───────────────────────────────────────────────────────── */
  function start() {
    if (running.value) return
    supported.value = isTauriRuntime()
    if (!supported.value) {
      // web 预览：命令不存在，静默停摆（不轮询、不报错）
      running.value = false
      return
    }
    running.value = true
    hidden.value = typeof document !== 'undefined' && document.visibilityState === 'hidden'
    if (typeof document !== 'undefined') document.addEventListener('visibilitychange', onVisibility)
    nowMs.value = Date.now()
    void subscribe()
    void refreshHistory()
    void drainMemory() // 进小区图就先把上次没取走的行收掉
    void poll().finally(schedule) // 第一发立刻打（同时兼心跳）
    if (drainTimer === null) {
      drainTimer = window.setInterval(() => {
        void drainMemory()
      }, WS_DRAIN_EVERY_MS)
    }
  }

  function stop() {
    running.value = false
    if (pollTimer !== null) window.clearTimeout(pollTimer)
    pollTimer = null
    if (drainTimer !== null) window.clearInterval(drainTimer)
    drainTimer = null
    if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onVisibility)
    if (unlisten) {
      try {
        unlisten()
      } catch {
        /* 卸载失败无所谓：Tauri 那边会随窗口一起清 */
      }
      unlisten = null
    }
    bound = false
    clearBubbles()
  }

  /* ── 派生量 ─────────────────────────────────────────────────────────── */
  const count = computed(() => events.value.length)
  /** 面板顶部的状态行要的那点信息（为什么还没出事 / 下次最早什么时候） */
  const status = computed(() => ({
    reason: lastReason.value,
    nextOkInSecs: nextOkInSecs.value,
    hidden: hidden.value,
    error: lastError.value,
  }))

  const api = {
    // 状态
    events,
    latest,
    bubbles,
    pendingLines,
    pendingAfter,
    speechHint,
    lastError,
    lastReason,
    nextOkInSecs,
    nowMs,
    hidden,
    supported,
    running,
    channels,
    count,
    status,
    // 生命周期
    start,
    stop,
    poll,
    refreshHistory,
    drainMemory,
    // 开关
    setChannel,
  }

  // 组件/作用域销毁时自动收摊（拿不到作用域就交给调用方自己 stop()，不报错）
  if (getCurrentScope()) onScopeDispose(stop)

  if (opts.autoStart === true) start()

  return api
}

export type WsWorldEvents = ReturnType<typeof useWorldEvents>
