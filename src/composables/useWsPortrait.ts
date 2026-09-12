// 「世界模拟」P2-2：立绘的**按需加载器**（性能红线的守门人）
//
// 机主给的红线（必须逐条守住，改之前先读）：
//   ① 只在侧边栏**真正打开**时才加载 —— 本 composable 的 `open` 为 false 时，一个命令都不发；
//   ② 关闭时**释放** —— 把 url 置空（`<img src="">` 会让浏览器丢掉那份解码位图），
//      并且把 in-flight 的结果作废（回到面板晚了不能把图又贴上去）；
//   ③ 绝不预加载别人的立绘 —— 这里只认「当前这一个 folder」，
//      而且每次只有一次请求在飞（`seq` 令牌）。
//
// 为什么立绘那么贵：素材是 3511×5242 的 webp，解码后约 73MB（与聊天里的立绘同源同价）。
// 所以「宁可慢一点、也不能同时存在两张」是这套代码的第一原则。

import { ref, watch, type Ref } from 'vue'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'

export interface UseWsPortraitOptions {
  /** 角色目录名（取立绘的 key） */
  folder: Ref<string>
  /** 情绪→文件名用的情绪（`GameRole.emotion`；空则「正常」） */
  emotion: Ref<string>
  /** 服装（'default' 或角色目录下的子目录名） */
  clothes: Ref<string>
  /** 侧边栏是否展开：false = 完全不动 */
  open: Ref<boolean>
  /** 情绪→文件名的映射函数（由调用方注入，保持与 GameScene 同一张表） */
  mapEmotion: (e: string) => string
}

export function useWsPortrait(opts: UseWsPortraitOptions) {
  /** 立绘 URL；空串 = 没图/已释放（组件据此画占位或空态） */
  const url = ref('')
  const loading = ref(false)
  const error = ref('')
  /** 当前这套服装取不到图（调用方据此把变体选择退回「默认」，而不是报错） */
  const missingClothes = ref(false)

  /** 请求令牌：只有最后一次的结果允许落地（防止快速切换时贴错人的立绘） */
  let seq = 0

  async function resolve() {
    const my = ++seq
    // ③ 关着就一个命令都不发
    if (!opts.open.value) return
    const folder = String(opts.folder.value || '').trim()
    if (!folder) {
      url.value = ''
      error.value = ''
      return
    }
    loading.value = true
    error.value = ''
    missingClothes.value = false
    const clothesName = String(opts.clothes.value || 'default') || 'default'
    const emotion = opts.mapEmotion(opts.emotion.value)
    try {
      const p = await invoke<string>('get_avatar_file', {
        characterFolder: folder,
        emotion,
        clothesName,
      })
      if (my !== seq || !opts.open.value) return // 迟到的结果丢掉（顺手避免贴上已关闭的面板）
      url.value = convertFileSrc(p)
    } catch (e) {
      if (my !== seq) return
      url.value = ''
      // 这套服装拿不到图：调用方会退回「默认」，所以这里不当成致命错误
      if (clothesName !== 'default') {
        missingClothes.value = true
        error.value = ''
      } else {
        error.value = e instanceof Error ? e.message : String(e)
      }
    } finally {
      if (my === seq) loading.value = false
    }
  }

  /** 释放：把 src 置空 —— 这是「关闭时释放 73MB」的唯一开关 */
  function release() {
    seq++ // 让所有在飞的请求作废
    url.value = ''
    loading.value = false
    error.value = ''
    missingClothes.value = false
  }

  watch(
    () => [opts.open.value, opts.folder.value, opts.emotion.value, opts.clothes.value] as const,
    ([isOpen]) => {
      if (!isOpen) {
        release() // ② 关掉就还内存
        return
      }
      void resolve()
    },
    { immediate: true },
  )

  return { url, loading, error, missingClothes, release, reload: resolve }
}
