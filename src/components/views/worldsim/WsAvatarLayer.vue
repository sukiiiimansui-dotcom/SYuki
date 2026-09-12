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
        :selected="a.id === selectedId"
        :me-name="meName"
        @pick="(x) => emit('pick', x)"
      />
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
  }>(),
  { grid: 28, selectedId: '', meName: '', size: 'map', zoom: 1 },
)

const emit = defineEmits<{ (e: 'pick', a: PlacedActor): void }>()

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
</style>
