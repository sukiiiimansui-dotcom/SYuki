<template>
  <!--
    面包屑：国 → 省 → 市 → 区县 → 小区
    机主的要求是「可任意回退，点哪级跳哪级」——所以**每一级都可点**，包括比当前更深的
    那几级（路径不删，用户退回省看一眼还能一键跳回区县）。用 › 分隔，符合「下钻」的直觉。
  -->
  <nav class="ws-crumb" aria-label="位置层级">
    <template v-for="(c, i) in crumbs" :key="c.key">
      <span v-if="i > 0" class="ws-crumb__sep" aria-hidden="true">›</span>
      <button
        class="ws-crumb__item"
        :class="{ 'is-active': c.active, 'is-here': i === cursorIndex, 'is-neigh': c.kind === 'neigh' }"
        :title="i === cursorIndex ? '当前位置' : `回到「${c.name}」`"
        type="button"
        @click="emit('go', c.index)"
      >
        {{ c.name }}
      </button>
    </template>
  </nav>
</template>

<script setup lang="ts">
import type { Crumb } from '@/composables/useWorldSim'

const props = defineProps<{
  crumbs: Crumb[]
  /** 当前停在第几级（用来把「当前这一级」画得不一样） */
  cursorIndex: number
}>()

const emit = defineEmits<{ (e: 'go', index: number): void }>()

// 这里不写逻辑，纯粹是渲染 —— 状态与流转全在 useWorldSim 里（单一职责）
void props
</script>

<style scoped>
.ws-crumb {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 0.15em;
  min-width: 0;
}
.ws-crumb__sep {
  color: var(--ws-fg-dim);
  opacity: 0.6;
  padding: 0 0.1em;
}
.ws-crumb__item {
  font: inherit;
  font-size: 0.94em;
  color: var(--ws-fg-dim);
  background: transparent;
  border: 1px solid transparent;
  border-radius: 999px;
  padding: 0.16em 0.6em;
  cursor: pointer;
  transition: background-color 0.16s ease, color 0.16s ease, transform 0.16s ease;
  max-width: 9em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 已知但因为回退而变暗的那几级：可点，提示“还能跳回去” */
.ws-crumb__item:hover {
  color: var(--ws-fg);
  background: var(--ws-primary-soft);
  transform: translateY(-1px);
}
.ws-crumb__item.is-active {
  color: var(--ws-fg);
}
.ws-crumb__item.is-here {
  color: var(--ws-on-primary);
  background: var(--ws-primary);
  font-weight: 600;
}
/* 「小区」那一级不是行政区划，用虚线边框区分 */
.ws-crumb__item.is-neigh {
  border-style: dashed;
  border-color: var(--ws-border);
}
.ws-crumb__item.is-neigh.is-here {
  border-style: solid;
}
</style>
