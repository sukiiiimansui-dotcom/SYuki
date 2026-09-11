// 世界地图前端模块加载器（T6-1/T6-4）
//
// 这些模块（transport/npc/world_time/world_weather/phone）是纯前端的渲染与交互逻辑，
// 挂在 window 上（window.TRANSPORT / window.NPC_SYS / ...）。它们**不需要**移植到 Rust：
// Rust 侧负责数据与几何，这些负责画和点。
const FILES = [
  'world_time.js',
  'world_weather.js',
  'transport.js',
  'npc.js',
  'phone.js',
] as const

let loading: Promise<void> | null = null

function inject(src: string): Promise<void> {
  return new Promise<void>((resolve) => {
    if (document.querySelector(`script[data-wm="${src}"]`)) return resolve()
    const s = document.createElement('script')
    s.src = src
    s.async = false
    s.dataset.wm = src
    s.onload = () => resolve()
    s.onerror = () => resolve() // 加载失败也不阻塞界面
    document.head.appendChild(s)
  })
}

/** 幂等加载：多次调用只会真正加载一次 */
export function loadWorldModules(): Promise<void> {
  if (!loading) {
    loading = (async () => {
      for (const f of FILES) await inject(`/world_map/${f}`)
    })()
  }
  return loading
}

/** 取已加载模块（如 'NPC_SYS' / 'TRANSPORT' / 'PHONE'） */
export function worldModule<T = any>(name: string): T | null {
  return ((window as any)[name] as T) ?? null
}

/** 模块是否就绪 */
export function worldModulesReady(): boolean {
  const w = window as any
  return !!(w.NPC_SYS && w.TRANSPORT && w.WORLD_TIME && w.WORLD_WEATHER && w.PHONE)
}
