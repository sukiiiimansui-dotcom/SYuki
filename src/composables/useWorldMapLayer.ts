import { ref, watch } from 'vue'

/** 世界地图叠加层的显示模式 */
export type WorldLayerMode = 'off' | 'overlay' | 'corner'

export interface WorldLayerState {
  /** off=不显示 / overlay=全屏半透明背景层 / corner=右下角小窗 */
  mode: WorldLayerMode
  /** overlay 模式的不透明度 0-1 */
  opacity: number
  /** corner 模式的小窗位置（px，左上角） */
  x: number
  y: number
  /** 显示哪个区域（adcode），空则按定位 */
  adcode: string
  style: string
}

const KEY = 'lsyuki.worldLayer'

function load(): WorldLayerState {
  const def: WorldLayerState = { mode: 'off', opacity: 0.28, x: -1, y: -1, adcode: '', style: 'gaode' }
  try {
    const raw = localStorage.getItem(KEY)
    if (!raw) return def
    const o = JSON.parse(raw)
    return {
      mode: o.mode === 'overlay' || o.mode === 'corner' ? o.mode : 'off',
      opacity: typeof o.opacity === 'number' ? Math.min(0.9, Math.max(0.05, o.opacity)) : def.opacity,
      x: typeof o.x === 'number' ? o.x : -1,
      y: typeof o.y === 'number' ? o.y : -1,
      adcode: typeof o.adcode === 'string' ? o.adcode : '',
      style: typeof o.style === 'string' ? o.style : 'gaode',
    }
  } catch {
    return def
  }
}

/** 模块级单例：任何组件/页面都能控制同一份状态 */
const state = ref<WorldLayerState>(load())

watch(
  state,
  (v) => {
    try {
      localStorage.setItem(KEY, JSON.stringify(v))
    } catch {
      /* 存储不可用不影响功能 */
    }
  },
  { deep: true },
)

export function useWorldMapLayer() {
  function setMode(m: WorldLayerMode) {
    state.value.mode = m
  }
  function cycleMode() {
    const order: WorldLayerMode[] = ['off', 'overlay', 'corner']
    const i = order.indexOf(state.value.mode)
    state.value.mode = order[(i + 1) % order.length]
  }
  function setOpacity(v: number) {
    state.value.opacity = Math.min(0.9, Math.max(0.05, v))
  }
  function setPos(x: number, y: number) {
    state.value.x = x
    state.value.y = y
  }
  function setRegion(adcode: string, style?: string) {
    state.value.adcode = adcode
    if (style) state.value.style = style
  }
  return { state, setMode, cycleMode, setOpacity, setPos, setRegion }
}
