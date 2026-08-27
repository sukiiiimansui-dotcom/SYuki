import { onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

/**
 * 用户心跳上报（心跳/离开想念触发用）。
 *
 * 监听用户的交互活动（鼠标移动/点击/键盘/触摸），
 * 去抖后调用后端 `proactive_mark_active` 刷新"最近活跃"时间，
 * 供主动系统的"用户离开 → AI 想念搭话"判断。
 *
 * 只在真实交互后上报（有活动才代表用户在线），避免一直轮询空转。
 */
const DEBOUNCE_MS = 3000
const ACTIVITY_EVENTS = [
  'mousedown',
  'mousemove',
  'keydown',
  'touchstart',
  'wheel',
] as const

export function useHeartbeat() {
  let timer: ReturnType<typeof setTimeout> | null = null

  const report = () => {
    invoke('proactive_mark_active').catch((e) =>
      console.warn('[Heartbeat] 上报心跳失败:', e),
    )
  }

  const onActivity = () => {
    if (timer) clearTimeout(timer)
    timer = setTimeout(report, DEBOUNCE_MS)
  }

  onMounted(() => {
    ACTIVITY_EVENTS.forEach((evt) => window.addEventListener(evt, onActivity))
  })

  onUnmounted(() => {
    ACTIVITY_EVENTS.forEach((evt) => window.removeEventListener(evt, onActivity))
    if (timer) clearTimeout(timer)
  })
}
