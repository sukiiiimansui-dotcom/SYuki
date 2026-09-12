<template>
  <!--
    单个头像标记（地图上的「人」）—— P2-1 的最小单元

    为什么拆成独立组件（而不是在列表里铺一堆 div）：
      · 每个头像要自己管「图片加载失败 → 退名字首字占位」这一份状态；
      · 悬停/选中的样式只在被hover的那一个上，拆开后 Vue 的更新粒度就是一个人。
    结构与视觉：外圈 = 情绪环（选中/自己用不同色），内圈 = 圆头像，下面 = 名字标签。
  -->
  <button
    class="ws-av"
    :class="[
      `ws-av--${size}`,
      {
        'is-me': actor.isMe,
        'is-on': selected,
        'is-crowd': actor.crowd > 1,
        'is-nopic': !picOk,
        'is-dragging': dragging,
        'is-draggable': drag,
      },
    ]"
    type="button"
    data-no-gesture
    :style="style"
    :title="title"
    :aria-label="label"
    @pointerdown.stop="onDown"
    @pointermove.stop="onMove"
    @pointerup.stop="onUp"
    @pointercancel.stop="onCancel"
    @dblclick.stop
    @wheel.stop
    @click.stop="onClick"
  >    <span class="ws-av__ring">
      <img
        v-if="actor.avatarUrl && picOk"
        class="ws-av__pic"
        :src="actor.avatarUrl"
        :alt="actor.name"
        draggable="false"
        loading="lazy"
        decoding="async"
        @error="picOk = false"
      />
      <span v-else class="ws-av__ph" aria-hidden="true">{{ initial }}</span>
      <span v-if="actor.isMe" class="ws-av__me" aria-hidden="true">★</span>
      <span v-if="actor.crowd > 1" class="ws-av__n" aria-hidden="true">{{ actor.crowd }}</span>
    </span>
    <span v-if="showName" class="ws-av__name">{{ label }}</span>
    <span v-if="showPlace && actor.place" class="ws-av__place">{{ actor.place }}</span>
  </button>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { letterboxOf, gridToBox, type PlacedActor } from './wsActors'
// P4-4：拖拽必须与地图手势**同一套口径** —— 阈值 4px、拖后抑制补发的 click（350ms）。
// 直接复用那两个常量/纯函数，绝不在这里另写一组数（两套数必然手感不一致）。
import { CLICK_SUPPRESS_MS, isDrag } from '@/composables/useWorldSimGestures'

const props = withDefaults(
  defineProps<{
    actor: PlacedActor
    /** 盒子尺寸（CSS 像素，未缩放的本地坐标）——由 WsAvatarLayer 统一量一次传下来 */
    boxW: number
    boxH: number
    grid: number
    selected?: boolean
    /** 'map' 地图大头像 / 'mini' 小地图上的小点 */
    size?: 'map' | 'mini'
    /** 玩家显示名（我的头像上要写名字） */
    meName?: string
    /**
     * 地图当前的缩放倍率（手势那套）。
     *
     * 为什么要传进来：头像层是**地图变换容器的子节点**，地图放大 4× 时
     * 头像也会被一起放大成 4 倍的大饼。这里用 `scale(1/k)` 反向抵消，
     * 让头像**位置跟着地图走、尺寸始终是屏幕上那么大** —— 这是地图类应用
     * （高德/Google Maps 的 POI）的通行做法，也是「地图上的人」能看清的前提。
     */
    zoom?: number
    /**
     * P4-4：这个头像能不能被**拖动**（把角色拖到地图别处 = 下一条「去那里」的指令）。
     *
     * 默认 **false**：小地图（`size='mini'`）等场景不该能拖，
     * 只有主地图那一层会打开它（WorldSim 的 `#pin` 插槽）。
     */
    drag?: boolean
  }>(),
  { selected: false, size: 'map', meName: '', zoom: 1, drag: false },
)

const emit = defineEmits<{
  (e: 'pick', a: PlacedActor): void
  /** 超过阈值、真的开始拖了（只发一次） */
  (e: 'dragstart', a: PlacedActor): void
  /** 拖动中（每次 pointermove 一次，坐标是 client 坐标） */
  (e: 'dragmove', p: { a: PlacedActor; clientX: number; clientY: number }): void
  /** 松手（`moved=false` 表示没超过阈值 = 一次点击，调用方别当拖拽处理） */
  (e: 'dragend', p: { a: PlacedActor; clientX: number; clientY: number; moved: boolean }): void
}>()

const { t } = useI18n()

/** 图片加载失败 → 退占位（不弹错、不空着） */
const picOk = ref(true)
// 换了头像 URL 要重新给一次机会（否则第一次失败以后永远画占位）
watch(
  () => props.actor.avatarUrl,
  () => {
    picOk.value = true
  },
)

const label = computed(() => {
  if (props.actor.isMe) return props.meName || t('worldsim.actor.me')
  return props.actor.name
})
const initial = computed(() => (label.value || '?').slice(0, 1))
const showName = computed(() => props.size === 'map')
const showPlace = computed(() => props.size === 'map' && !!props.actor.place)

const title = computed(() => {
  const who = label.value
  const what = props.actor.nowText || props.actor.place
  return what ? `${who} · ${what}` : who
})

/**
 * 定位：把格子坐标折成盒子内像素。
 *
 * `left/top` 定位置、`translate(-50%,-50%)` 把自己居中、
 * `scale(1/zoom)` 抵消地图手势的放大 —— 三条一起才等于「钉在地图的那块地上，
 * 但始终保持屏幕上这么大」。少任何一条都会出问题（不抵消 → 放大成巨饼；
 * 不居中 → 头像右下角压在那个点上；不用 left/top → 变换会被 CSS 覆盖）。
 */
const style = computed(() => {
  const lb = letterboxOf(props.boxW, props.boxH, props.grid)
  const p = gridToBox(props.actor.px, props.actor.py, lb)
  const k = Number.isFinite(props.zoom) && props.zoom > 0 ? props.zoom : 1
  return {
    left: `${p.x.toFixed(2)}px`,
    top: `${p.y.toFixed(2)}px`,
    transform: `translate(-50%, -50%) scale(${(1 / k).toFixed(4)})`,
  }
})

/* ── P4-4：把这个人拖到地图别处 ────────────────────────────────────────────
 *
 * 三条硬要求（机主给的口径，改之前先读）：
 *  ① **绝不触发地图平移缩放**：本元素是 `<button>`（在 `useWorldSimGestures` 的
 *     免手势名单里）**并且**挂了 `data-no-gesture`，指针事件在这里 `.stop` 掉 ——
 *     地图那套 `pointerdown` 收不到，`pointers` 表一直是空的，所以拖地图的数学
 *     一次都不会跑（不是「跑了但被忽略」，是根本没启动）。
 *  ② **阈值 4px**（复用 `isDrag`）：小于它一律当点击，否则「点一下就选中」会失灵。
 *  ③ 拖完必须**吃掉浏览器补发的那一发 click**（`CLICK_SUPPRESS_MS`）：
 *     不然松手会顺带触发「选中这个人」，把刚拖到的目的地又盖掉。
 *
 * 指针捕获挂在自己的元素上（`setPointerCapture`）：手指滑出这个几十像素的小圆
 * 以后事件仍然回到这里，否则往远处拖到一半就断了（拖拽最典型的 bug）。
 */
const dragging = ref(false)
let armed = false
let pid = -1
let startX = 0
let startY = 0
let suppressUntil = 0

function elOf(e: PointerEvent): HTMLElement | null {
  return (e.currentTarget as HTMLElement) || null
}

function onDown(e: PointerEvent) {
  if (!props.drag) return
  // 只认主键（鼠标右键/中键不参与拖动）；触屏/笔的 button 恒为 0
  if (e.pointerType === 'mouse' && e.button !== 0) return
  armed = true
  dragging.value = false
  pid = e.pointerId
  startX = e.clientX
  startY = e.clientY
  try {
    elOf(e)?.setPointerCapture(e.pointerId)
  } catch {
    /* 老 WebView 不支持捕获：退化也能用，只是拖出元素后可能断 */
  }
}

function onMove(e: PointerEvent) {
  if (!armed || e.pointerId !== pid) return
  if (!dragging.value) {
    // 阈值内：还是「可能的点击」，什么都不做（点击选中靠 click 那条路）
    if (!isDrag(e.clientX - startX, e.clientY - startY)) return
    dragging.value = true
    emit('dragstart', props.actor)
  }
  emit('dragmove', { a: props.actor, clientX: e.clientX, clientY: e.clientY })
}

function onUp(e: PointerEvent) {
  if (!armed || e.pointerId !== pid) return
  armed = false
  try {
    elOf(e)?.releasePointerCapture(e.pointerId)
  } catch {
    /* 已经释放/不支持捕获 */
  }
  if (!dragging.value) return // 没超过阈值 = 一次点击，交给 onClick
  dragging.value = false
  suppressUntil = Date.now() + CLICK_SUPPRESS_MS
  emit('dragend', { a: props.actor, clientX: e.clientX, clientY: e.clientY, moved: true })
}

function onCancel() {
  // 浏览器把手势抢走了（页面开始滚动之类）：干净退出，别留下「半拖着」的状态
  const wasDragging = dragging.value
  armed = false
  dragging.value = false
  if (wasDragging) {
    suppressUntil = Date.now() + CLICK_SUPPRESS_MS
    emit('dragend', { a: props.actor, clientX: startX, clientY: startY, moved: false })
  }
}

/** 点一下就选中 —— 拖过的那一发补发的 click 必须拦掉（见上面的 ③） */
function onClick() {
  if (Date.now() < suppressUntil) return
  emit('pick', props.actor)
}
</script>

<style scoped>
.ws-av {
  position: absolute;
  /* translate/scale 由组件按 zoom 动态算（见 style 计算属性），这里只留兜底 */
  transform: translate(-50%, -50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.12em;
  padding: 0;
  border: 0;
  background: none;
  font: inherit;
  color: var(--ws-fg);
  cursor: pointer;
  /* 自己是地图手势层里的一个洞：头像可点，其它地方的事件照旧穿透给手势 */
  pointer-events: auto;
  -webkit-tap-highlight-color: transparent;
}
.ws-av__ring {
  position: relative;
  display: block;
  width: 2.1em;
  height: 2.1em;
  border-radius: 50%;
  overflow: hidden;
  background: var(--ws-panel-2);
  border: 2px solid var(--ws-primary);
  box-shadow: var(--ws-shadow);
  transition: transform 0.16s ease, border-color 0.16s ease;
}
.ws-av__pic {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.ws-av__ph {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  font-size: 1em;
  font-weight: 700;
  color: var(--ws-on-primary);
  background: var(--ws-primary-soft);
}
.ws-av__me {
  position: absolute;
  right: 0;
  bottom: 0;
  font-size: 0.55em;
  line-height: 1;
  padding: 0.1em;
  color: var(--ws-accent-2);
  text-shadow: 0 0 3px rgba(0, 0, 0, 0.5);
}
.ws-av__n {
  position: absolute;
  left: -0.2em;
  top: -0.2em;
  min-width: 1.1em;
  padding: 0 0.2em;
  font-size: 0.6em;
  line-height: 1.1em;
  border-radius: 999px;
  background: var(--ws-warn);
  color: #3a2c10;
  text-align: center;
}
.ws-av__name {
  max-width: 6em;
  padding: 0.05em 0.4em;
  font-size: 0.78em;
  font-weight: 600;
  line-height: 1.4;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  border-radius: 999px;
  background: var(--ws-panel);
  border: 1px solid var(--ws-border);
  backdrop-filter: blur(var(--ws-blur));
  -webkit-backdrop-filter: blur(var(--ws-blur));
}
.ws-av__place {
  max-width: 7em;
  font-size: 0.7em;
  color: var(--ws-fg-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-shadow: 0 1px 2px var(--ws-bg);
}
/* 自己：用主题里那个专门的「自己头像点」色（--ws-accent-2） */
.ws-av.is-me .ws-av__ring {
  border-color: var(--ws-accent-2);
  border-width: 3px;
}
.ws-av.is-on .ws-av__ring {
  border-color: var(--ws-primary-deep);
  transform: scale(1.14);
  box-shadow: var(--ws-shadow-lg);
}
.ws-av.is-crowd .ws-av__ring {
  /* 一堆人挤在一起时加一点点描边，好区分 */
  outline: 1px solid var(--ws-border);
}
.ws-av:hover .ws-av__ring,
.ws-av:focus-visible .ws-av__ring {
  transform: scale(1.1);
}
/* P4-4：能被拖的时候给一点暗示（抓手光标 + 选中态放大），拖动中再加一圈高亮。
   注意：这里只动 transform / opacity / 颜色，不动 left/top/width（动画纪律）。 */
.ws-av.is-draggable {
  cursor: grab;
}
.ws-av.is-dragging {
  cursor: grabbing;
  z-index: 4;
}
.ws-av.is-dragging .ws-av__ring {
  transform: scale(1.22);
  border-color: var(--ws-accent);
  box-shadow: var(--ws-shadow-lg);
}
.ws-av.is-dragging .ws-av__name {
  opacity: 0.65;
}
/* 小地图上的点：只留圆点，不要名字 */
.ws-av--mini .ws-av__ring {
  width: 1.15em;
  height: 1.15em;
  border-width: 1.5px;
  box-shadow: none;
}
.ws-av--mini .ws-av__ph {
  font-size: 0.6em;
}
.ws-av--mini {
  pointer-events: auto;
}
</style>
