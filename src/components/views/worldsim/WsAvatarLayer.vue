<template>
  <!--
    地图上的「人」这一层（P2-1）—— 覆盖在小区图上、**跟着地图一起缩放平移**。

    为什么放在手势的变换容器（.ws-neigh__pan / .ws-geo__pan）里面：
      它是地图的一部分，不是悬浮 HUD。放进变换容器后，平移/缩放由手势那一层负责，
      这里一个 transform 都不用写；否则每次手势都要在这里重算一遍，必然对不齐。

    为什么还要再套一层 .ws-avs__box：
      地图 SVG 的 viewBox 是正方形（`0 0 size size`），而盒子常常是长方形的，
      CSS 用 `object-fit: contain` 居中留白（信箱）。头像要落在**图上**而不是盒子上，
      就必须复刻同一套信箱折算 —— 纯数学在 wsActors.ts 的 letterboxOf()，
      盒子尺寸由 useElementSize 量出来（与后端 SVG 的 1:1 约定一致）。
  -->
  <div class="ws-avs" :class="{ 'is-mini': size === 'mini' }">
    <div ref="host" class="ws-avs__box">
      <WsAvatarMark
        v-for="a in placed"
        :key="a.id"
        :actor="a"
        :box-w="w"
        :box-h="h"
        :grid="grid"
        :size="size"
        :zoom="zoom"
        :drag="drag"
        :selected="a.id === selectedId"
        :me-name="meName"
        @pick="(x) => emit('pick', x)"
        @dragstart="(x) => emit('dragstart', x)"
        @dragmove="(p) => emit('dragmove', p)"
        @dragend="(p) => emit('dragend', p)"
      />
      <!-- P4-4：拖动中的落点预览（一枚「目的地」图钉）。
           放在同一个信箱盒子里，坐标由页面算好（页面才知道手势变换）。
           `pointer-events:none` + `data-no-gesture`：它是纯显示，绝不参与任何交互。 -->
      <span
        v-if="dragPin"
        class="ws-avs__pin"
        data-no-gesture
        :style="{ left: `${dragPin.x.toFixed(2)}px`, top: `${dragPin.y.toFixed(2)}px`, transform: `translate(-50%, -100%) scale(${(1 / (zoom > 0 ? zoom : 1)).toFixed(4)})` }"
        aria-hidden="true"
      >📍</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import WsAvatarMark from './WsAvatarMark.vue'
import { useElementSize } from '@/composables/useWorldSimGeo'
import type { PlacedActor } from './wsActors'

withDefaults(
  defineProps<{
    placed: PlacedActor[]
    grid?: number
    selectedId?: string
    meName?: string
    size?: 'map' | 'mini'
    /**
     * 地图当前的缩放倍率（手势那套）。
     *
     * 头像层在变换容器内部，所以位置会自动跟着缩放走；
     * 但**尺寸**也会被一起放大 —— 传进来让每个头像 `scale(1/zoom)` 抵消，
     * 效果就是「钉在地图上的那块地，但始终是屏幕上这么大」。
     * 小地图（size='mini'）没有手势，保持 1。
     */
    zoom?: number
    /** P4-4：这一层的人能不能拖（默认不能；只有主地图打开） */
    drag?: boolean
    /** P4-4：拖动中的落点预览（**信箱盒子坐标**，由页面算；null = 没在拖） */
    dragPin?: { x: number; y: number } | null
  }>(),
  { grid: 28, selectedId: '', meName: '', size: 'map', zoom: 1, drag: false, dragPin: null },
)

const emit = defineEmits<{
  (e: 'pick', a: PlacedActor): void
  (e: 'dragstart', a: PlacedActor): void
  (e: 'dragmove', p: { a: PlacedActor; clientX: number; clientY: number }): void
  (e: 'dragend', p: { a: PlacedActor; clientX: number; clientY: number; moved: boolean }): void
}>()

// 量「信箱盒子」的尺寸：它就是用来复刻 object-fit: contain 的参照物
const host = ref<HTMLElement | null>(null)
const { w, h } = useElementSize(host, { w: 320, h: 320 })
</script>

<style scoped>
.ws-avs {
  position: absolute;
  inset: 0;
  /* 这一层只是个定位参照：绝不能吃掉地图手势的事件 */
  pointer-events: none;
  z-index: 3;
}
.ws-avs__box {
  position: absolute;
  inset: 0;
}
/* 拖动中的落点图钉：纯显示（不吃事件），只动 transform */
.ws-avs__pin {
  position: absolute;
  transform-origin: 50% 100%;
  font-size: 1.6em;
  line-height: 1;
  pointer-events: none;
  filter: drop-shadow(0 2px 4px rgba(0, 0, 0, 0.35));
  opacity: 0.95;
  z-index: 5;
}
</style>
