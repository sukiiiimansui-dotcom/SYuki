// 「世界模拟」P2：面板/侧边栏的**视图状态**（谁被选中、面板开不开、立绘展开没有）
//
// 为什么单独一个 composable、还用模块级单例：
//   · 「点地图上的头像 → 开面板」与「面板里点别人 → 换人」是**两个组件之间**的通信，
//     全靠 props 透传会把 WorldSim.vue 变成一个巨大的中转站；
//   · 模块级单例 = 同一次会话里任何组件读到的都是同一份状态（与 useWorldMapLayer 同款做法），
//     而且不引入 pinia store（这套东西只在世界模拟里活着，没必要进全局 store）。
//
// ⚠️ 这里**只有 UI 状态**，没有任何数据请求 —— 谁的数据谁去取（面板自己去拉记忆/日程），
//    这样「关掉面板就没人再发请求」，性能红线才守得住。

import { computed, ref, onBeforeUnmount } from 'vue'

/** 面板当前对着谁：null = 关着；'me' = 自己；其余是角色 id */
export type PanelTarget = 'me' | (string & {})

/** 关掉面板时把「展开的立绘」一起收掉（它才是吃内存的那个） */
const open = ref(false)
const targetId = ref<string>('')
/** 立绘是否已展开到侧边栏（没展开时只显示缩略图，不加载大图） */
const portraitOpen = ref(false)

/**
 * 窄屏判定阈值（CSS 像素）。
 *
 * 为什么同时看 `uiStore.isMobile` 和这个阈值：
 *   · UIStore 里 `isMobile` 判的是 <500px（LingChat 全局那套，跟随它才不会两边不一致）；
 *   · 但 500~760px 的手机横屏/小平板，抽屉式右侧栏只剩一条缝，也该走「全屏 + 遮罩」。
 * 两个条件取或：**任一**说窄屏就按窄屏布局。
 */
export const WS_NARROW_PX = 760

const winW = ref(typeof window === 'undefined' ? 1024 : window.innerWidth)
let bound = false
function onResize() {
  winW.value = typeof window === 'undefined' ? 1024 : window.innerWidth
}

export function useWsPanel() {
  if (!bound && typeof window !== 'undefined') {
    bound = true
    window.addEventListener('resize', onResize)
  }

  /** 是不是窄屏（手机）：窄屏 = 全屏 + 遮罩，宽屏 = 右侧抽屉 */
  const narrow = computed(() => winW.value < WS_NARROW_PX)

  function openPanel(id: string) {
    // 换人时先把立绘收掉：下一个人的立绘必须**重新**按需加载，
    // 不能沿用上一个人的（否则会在切换的一瞬间同时存在两张 73MB 的大图）
    if (targetId.value !== id) portraitOpen.value = false
    targetId.value = id
    open.value = true
  }

  function closePanel() {
    open.value = false
    portraitOpen.value = false
    targetId.value = ''
  }

  function togglePortrait(v?: boolean) {
    portraitOpen.value = typeof v === 'boolean' ? v : !portraitOpen.value
  }

  /** 选中的是不是玩家自己 */
  const isMe = computed(() => targetId.value === 'me')

  onBeforeUnmount(() => {
    // 组件卸载（离开世界模拟页）时把面板关掉：留着一个开着的面板会让
    // 立绘的 <img> 一直挂在 DOM 上（那 73MB 就不还了）
    if (open.value) closePanel()
  })

  return {
    open,
    targetId,
    portraitOpen,
    narrow,
    winW,
    isMe,
    openPanel,
    closePanel,
    togglePortrait,
  }
}

export type WsPanel = ReturnType<typeof useWsPanel>
