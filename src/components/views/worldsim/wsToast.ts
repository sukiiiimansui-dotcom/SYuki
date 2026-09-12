// 「世界模拟」的极简 toast（P2-3「快捷动作」明确要求：点不动就提示，绝不静默无反应）
//
// 为什么不复用 LingChat 现成的：
//   全仓扫过一遍 —— 只有 AchievementToast.vue（成就专用）、ImportProgressBar.vue（导入进度专用），
//   没有通用的 toast；App.vue 里的 notification 是**另一套**（头像/系统通知，样式与生命周期
//   都跟「世界模拟」的小清新外壳不搭），硬塞进去等于把两套皮肤搅在一起。
//   所以这里给「世界模拟」做一份自己的：一个响应式队列 + 一个容器，全部 ws- 前缀，
//   不进任何全局 store（世界模拟没打开时它什么也不做、也不占地方）。

import { readonly, ref } from 'vue'

export type WsToastKind = 'info' | 'ok' | 'warn' | 'err'

export interface WsToastItem {
  id: number
  text: string
  kind: WsToastKind
  /** 毫秒；默认 2600 */
  ms: number
}

const items = ref<WsToastItem[]>([])
let seq = 0
const timers = new Map<number, number>()

/** 同一条文案连续弹时只保留最新的一条（快捷动作连点不会刷屏） */
function dedupe(text: string): boolean {
  const last = items.value[items.value.length - 1]
  return !!last && last.text === text && Date.now() - (last.id || 0) < 100
}

/**
 * 弹一条提示。
 *
 * @param text 已经 i18n 过的文案（这里不做翻译：文案的上下文只有调用方知道）
 * @param kind info/ok/warn/err —— 配色走 worldsim.css 的 ws-note 同款变量
 * @param ms   停留时长（毫秒）
 */
export function wsToast(text: string, kind: WsToastKind = 'info', ms = 2600): void {
  const t = String(text || '').trim()
  if (!t) return
  if (dedupe(t)) return
  const id = ++seq
  items.value = [...items.value.slice(-3), { id, text: t, kind, ms }]
  // 定时移除：把 timer 存起来，组件卸载时可以一次性清干净
  const h = window.setTimeout(() => {
    items.value = items.value.filter((x) => x.id !== id)
    timers.delete(id)
  }, Math.max(600, ms))
  timers.set(id, h)
}

/** 「即将上线」这类占位动作的统一提示（文案由调用方 i18n 后传进来） */
export function wsToastSoon(label: string, soonText: string): void {
  wsToast(`${label}：${soonText}`, 'info', 2200)
}

export function useWsToast() {
  function clear() {
    for (const h of timers.values()) window.clearTimeout(h)
    timers.clear()
    items.value = []
  }
  return { items: readonly(items), clear, toast: wsToast }
}
