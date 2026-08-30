import { invoke } from '@tauri-apps/api/core'

export interface ScheduleData {
  scheduleGroups?: Record<string, any>
  todoGroups?: Record<string, any>
  importantDays?: any[]
}

export const getSchedules = async (): Promise<ScheduleData> => {
  try {
    const data = await invoke<ScheduleData>('get_schedules')
    return data
  } catch (error: any) {
    console.error('获取日程信息错误:', error.message)
    throw error
  }
}

export const saveSchedules = async (data: ScheduleData): Promise<void> => {
  try {
    console.log('日程信息触发提醒')
    await invoke('save_schedules', { data })
  } catch (error: any) {
    console.error('保存日程信息错误:', error.message)
    throw error
  }
}

export const reloadProactiveSystem = async (): Promise<void> => {
  try {
    await invoke('reload_proactive_system')
  } catch (error: any) {
    console.error('重载主动系统错误:', error.message)
    throw error
  }
}

// ========== 主动系统状态快照（前端可视化「AI 主动状态与历史」） ==========

export interface ProactivePendingIntent {
  kind: string
  waited_secs: number
}

export interface ProactiveEvent {
  ts_ms: number
  kind: string
  preview: string
}

export interface ProactiveStatusSnapshot {
  enabled: boolean
  running: boolean
  can_deliver: boolean
  last_interaction_ago_secs: number
  away_delivered_count: number
  away_max_times: number
  away_timeout_secs: number
  interest: number
  interest_cap: number
  proactive_times: number
  max_proactive_count: number
  state: string
  description: string
  pending_intents: ProactivePendingIntent[]
  history: ProactiveEvent[]
}

export const getProactiveStatus = async (): Promise<ProactiveStatusSnapshot> => {
  try {
    const data = await invoke<ProactiveStatusSnapshot>('get_proactive_status')
    return data
  } catch (error: any) {
    console.error('获取主动系统状态错误:', error?.message || error)
    throw error
  }
}
