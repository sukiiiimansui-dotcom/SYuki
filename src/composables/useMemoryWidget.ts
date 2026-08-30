import { ref } from 'vue'

/** 记忆悬浮窗的全局开关（模块级单例）。 */
const open = ref(false)

export function useMemoryWidget() {
  function toggle() {
    open.value = !open.value
  }
  function openWidget() {
    open.value = true
  }
  function close() {
    open.value = false
  }
  return { open, toggle, openWidget, close }
}
