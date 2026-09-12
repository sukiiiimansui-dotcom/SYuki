<template>
  <!--
    「世界模拟」P5-2：**地图气泡层**（事件通知三通道里的 bubble 那一路）

    为什么单独一个组件、而且**不去改** `WsAvatarLayer.vue` / `WsAvatarMark.vue`：
      那两位是 P2 的既有实现（人物层与头像本体），事件气泡只是「多贴一层」，
      把气泡的定位蹭进头像组件里会让「一个人怎么画」和「一件事怎么冒泡」互相牵连。
      所以这里自带一层 `inset:0` 的透明覆盖层，自己算位置。

    坐标系与定位三件套（与 `WsAvatarMark` / `WsVehicleMark` 完全同款，改之前先读那两处）：
      `left/top`（信箱折算后的盒子像素）→ `translate(-50%,-100%)`（气泡底边中心对准锚点）
      → `scale(1/zoom)`（地图放大时气泡不变大）→ `transform-origin: 50% 100%`
      （把「底边中心」钉死成缩放的不动点，这样**任何 zoom 下气泡尖都正好压在头像上**）。
      盒子尺寸用 `offsetWidth/offsetHeight` 量：**不是** getBoundingClientRect ——
      后者会被祖先的手势 transform 放大，量出来的盒子是错的（`WsVehicleMark` 踩过）。

    找不到对应角色（不在名单里 / 没坐标）时退回**屏幕中央**浮一个，并在气泡上写清是谁的
      —— 宁可占点地方，也绝不静默丢弃一条已经发生的事件。

    指针事件：整层 `pointer-events:none`，只有气泡自己 `pointer-events:auto` 且吃掉
      pointerdown/up/click/wheel（`data-no-gesture` + `.stop`），绝不把事件漏给地图手势。
  -->
  <div ref="host" class="ws-ebx" data-no-gesture>
    <div
      v-for="b in shown"
      :key="b.key"
      class="ws-eb"
      :class="{ 'is-center': !findActor(b.role), 'is-lite': low }"
      :style="styleOf(b)"
      @pointerdown.stop
      @pointerup.stop
      @dblclick.stop
      @wheel.stop
      @click.stop
    >
      <span class="ws-eb__ico" aria-hidden="true">💬</span>
      <span class="ws-eb__t">{{ b.text }}</span>
      <span v-if="!findActor(b.role)" class="ws-eb__who">{{ b.role || meName }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { bubbleAnchorOf, type WsBubbleItem } from '@/composables/useWorldEvents'
import type { PlacedActor } from './wsActors'

const props = withDefaults(
  defineProps<{
    /** 正在冒的气泡（`useWorldEvents` 的 `bubbles`） */
    bubbles: WsBubbleItem[]
    /** 地图上的人（`useWsActors` 的 `placed`）—— 用来把气泡挂到对应头像头上 */
    placed: PlacedActor[]
    /** 小区图的网格边长（与 WsDistrict / 后端 sketch 一致，默认 28） */
    grid?: number
    /** 地图手势的缩放倍率（`WsDistrict` defineExpose 的 gsScale） */
    zoom?: number
    /** 没有对应角色时，气泡上写的名字（玩家的称呼） */
    meName?: string
    /**
     * P5-5：**同时显示的气泡数上限**。
     * 数据层（`useWorldEvents`）自己有一层上限（`WS_BUBBLE_MAX = 4`），
     * 这里再收一道是给低端机用的：DOM 节点、`backdrop-filter`、
     * 每个气泡一次位置换算 —— 一次冒 4 个在低端机上就是一次小卡顿。
     * 多出来的气泡不是丢了，只是这一帧不画（到点仍会自己消失）。
     */
    max?: number
    /** P5-5：低性能档 —— 入场动画退化成纯淡入（不做位移+缩放） */
    low?: boolean
  }>(),
  { grid: 28, zoom: 1, meName: '', max: 4, low: false },
)

/** 只画前 `max` 条（顺序就是数据层的顺序，最新那条一定在） */
const shown = computed<WsBubbleItem[]>(() => {
  const cap = Math.max(1, Math.trunc(Number(props.max) || 1))
  return props.bubbles.length > cap ? props.bubbles.slice(0, cap) : props.bubbles
})

/* ── 盒子尺寸：自己量（offsetWidth 不受祖先 transform 影响）──────────────── */
const host = ref<HTMLElement | null>(null)
const boxW = ref(0)
const boxH = ref(0)
let ro: ResizeObserver | null = null

function measure() {
  const el = host.value
  if (!el) return
  const w = el.offsetWidth || el.clientWidth
  const h = el.offsetHeight || el.clientHeight
  if (w > 0) boxW.value = w
  if (h > 0) boxH.value = h
}

onMounted(() => {
  measure()
  if (typeof ResizeObserver !== 'undefined' && host.value) {
    // 只关心布局尺寸变化（转屏/面板开合）：手势缩放不会触发 RO，也不会把盒子量错
    ro = new ResizeObserver(measure)
    ro.observe(host.value)
  } else if (typeof window !== 'undefined') {
    window.addEventListener('resize', measure)
  }
})

onBeforeUnmount(() => {
  ro?.disconnect()
  ro = null
  if (typeof window !== 'undefined') window.removeEventListener('resize', measure)
})

/** 这个气泡该挂在谁头上：先按角色名，再退回角色目录名（两种键后端都可能给） */
function findActor(role: string): PlacedActor | null {
  const who = String(role || '').trim()
  if (!who) return null
  return props.placed.find((a) => a.name === who) || props.placed.find((a) => a.folder === who) || null
}

/** 位置：纯函数 `bubbleAnchorOf` 算锚点，这里只拼 CSS */
function styleOf(b: WsBubbleItem): Record<string, string> {
  const a = findActor(b.role)
  const anchor = bubbleAnchorOf(
    a ? { gx: a.gx, gy: a.gy, px: a.px, py: a.py } : null,
    boxW.value,
    boxH.value,
    props.grid,
    props.zoom,
  )
  if (!anchor.found) {
    // 没找到人：屏幕正中央浮一个（定位在盒子中心，缩放同样抵消）
    return {
      left: '50%',
      top: '50%',
      transform: `translate(-50%, -50%) scale(${anchor.scale.toFixed(4)})`,
    }
  }
  return {
    left: `${anchor.x.toFixed(2)}px`,
    top: `${anchor.y.toFixed(2)}px`,
    transform: `translate(-50%, -100%) scale(${anchor.scale.toFixed(4)})`,
  }
}
</script>

<style scoped>
.ws-ebx {
  position: absolute;
  inset: 0;
  /* 整层只是定位参照：绝不能吃掉地图手势（气泡自己是唯一的洞，见 .ws-eb） */
  pointer-events: none;
  z-index: 4;
}
.ws-eb {
  position: absolute;
  /* 底边中心 = 缩放不动点：气泡尖永远压在头像上（见组件头注释） */
  transform-origin: 50% 100%;
  display: inline-flex;
  align-items: center;
  gap: 0.25em;
  max-width: 12em;
  padding: 0.25em 0.55em;
  font-size: 0.9em;
  line-height: 1.45;
  color: var(--ws-fg);
  background: var(--ws-panel);
  border: 1px solid var(--ws-border);
  border-radius: var(--ws-radius-sm);
  box-shadow: var(--ws-shadow);
  backdrop-filter: blur(var(--ws-blur));
  -webkit-backdrop-filter: blur(var(--ws-blur));
  pointer-events: auto;
  animation: ws-pop 0.22s ease both;
  overflow-wrap: anywhere;
}
/* P5-5 低档：入场只淡入（位移+缩放那套在低端机上每次都要重新合成一层） */
.ws-eb.is-lite {
  animation: ws-fade-in 0.14s ease both;
}
.ws-eb__ico {
  flex: none;
}
.ws-eb__t {
  min-width: 0;
}
.ws-eb__who {
  flex: none;
  font-size: 0.82em;
  color: var(--ws-fg-dim);
}
/* 找不到人的兜底：屏幕中央那个用主题主色描一圈，一眼看出「这不是某个人的气泡」 */
.ws-eb.is-center {
  border-color: var(--ws-primary);
  background: var(--ws-panel-2);
}
</style>
