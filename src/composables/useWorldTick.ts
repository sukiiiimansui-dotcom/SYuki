// 世界时钟与心跳（T6-3）
//
// 设计要点：
//   · 世界时间 = **设备真实时间**（用户要求「时间跟 LingChat」），不自己另起一套时钟
//   · 节拍：每 TICK_MS 推进一次世界（NPC 活动、事件、时间色调），页面隐藏时暂停省电
//   · 世界里的重大事件会通过 Tauri 命令推给后端存盘，LingChat 的主动系统可据此搭话
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { loadWorldModules, worldModule } from './useWorldModules'

/** 一次世界推进的间隔（真实时间毫秒）。20s 足够让 NPC 动起来又不费电 */
const TICK_MS = 20_000
/** 天气/大范围数据刷新间隔 */
const SLOW_MS = 10 * 60_000

export interface WorldEvent {
  ts: number
  type: string
  title: string
  desc?: string
  placeName?: string
  major?: boolean
}

const events = ref<WorldEvent[]>([])
const running = ref(false)
const tickCount = ref(0)
const lastTickAt = ref(0)

let timer: number | null = null
let slowTimer: number | null = null

function pushEvents(list: WorldEvent[]) {
  if (!list?.length) return
  // 只留最近的 50 条
  events.value = [...list.slice(-50), ...events.value].slice(0, 50)
  // 推给后端（失败不影响世界继续跑；后端可能没实现该命令，忽略即可）
  invoke('world_map_push_events', { events: list }).catch(() => {})
}

function tick() {
  const NPC = worldModule<any>('NPC_SYS')
  tickCount.value++
  lastTickAt.value = Date.now()
  if (!NPC) return
  try {
    // 世界时间由 device 真实时间决定
    const ts = NPC.fetchTimeState ? NPC.fetchTimeState() : null
    if (NPC.tickNpcs) NPC.tickNpcs(ts || undefined)
    if (NPC.tickInteractions) NPC.tickInteractions()
    if (typeof NPC.getEvents === 'function') {
      const evs = NPC.getEvents() || []
      const majors = evs.filter((e: any) => e && e.major)
      if (majors.length) pushEvents(majors.slice(-5))
    }
  } catch {
    /* 单个模块出错不影响心跳 */
  }
}

function onVisibility() {
  if (document.hidden) stop()
  else start()
}

/** 启动/停止世界心跳（幂等） */
export function startWorldTick() {
  if (timer !== null) return
  running.value = true
  timer = window.setInterval(tick, TICK_MS)
  slowTimer = window.setInterval(() => {
    // 慢速刷新：交给订阅者（天气/日程），这里只发一个信号
    window.dispatchEvent(new CustomEvent('world:slow-refresh'))
  }, SLOW_MS)
  tick() // 立即跑一次，避免刚打开时世界是静止的
}

export function stopWorldTick() {
  if (timer !== null) {
    window.clearInterval(timer)
    timer = null
  }
  if (slowTimer !== null) {
    window.clearInterval(slowTimer)
    slowTimer = null
  }
  running.value = false
}

function start() {
  startWorldTick()
}
function stop() {
  stopWorldTick()
}

export function useWorldTick() {
  onMounted(async () => {
    await loadWorldModules()
    startWorldTick()
    document.addEventListener('visibilitychange', onVisibility)
  })
  onBeforeUnmount(() => {
    document.removeEventListener('visibilitychange', onVisibility)
    stopWorldTick()
  })
  return { events, running, tickCount, lastTickAt, startWorldTick, stopWorldTick }
}

/** 不依赖组件生命周期的版本（在 App.vue 里用） */
export function useWorldTickGlobal() {
  onMounted(async () => {
    await loadWorldModules()
    startWorldTick()
    document.addEventListener('visibilitychange', onVisibility)
  })
  onBeforeUnmount(() => {
    document.removeEventListener('visibilitychange', onVisibility)
    stopWorldTick()
  })
  return { events, running, tickCount }
}

export default useWorldTick
