<template>
  <!--
    面板里的「一节」（可折叠）。

    为什么要它：P2-3/P2-4 一共 12 个板块，每个都要「标题 + 图标 + 折叠 + 计数」，
    写 12 遍必然长歪；而它太薄，不值得引一个 UI 库（项目里也没有通用折叠组件）。

    手势：标题栏是 button（已在 useWorldSimGestures 的 NO_GESTURE_SELECTOR 里），
    内容区要滚动的话自己带 .ws-scroll。
  -->
  <section class="ws-cp" :class="{ 'is-open': isOpen }">
    <button class="ws-cp__head" type="button" :aria-expanded="isOpen" @click="isOpen = !isOpen">
      <span class="ws-cp__ico" aria-hidden="true">{{ icon }}</span>
      <span class="ws-cp__t">{{ title }}</span>
      <span v-if="typeof count === 'number' && count > 0" class="ws-tag">{{ count }}</span>
      <span class="ws-spacer" />
      <span class="ws-cp__caret" aria-hidden="true">{{ isOpen ? '▾' : '▸' }}</span>
    </button>
    <div v-show="isOpen" class="ws-cp__body">
      <slot />
    </div>
  </section>
</template>

<script setup lang="ts">
import { ref } from 'vue'

const props = withDefaults(
  defineProps<{
    title: string
    icon?: string
    /** 右上角的小计数（日程条目数那种） */
    count?: number
    defaultOpen?: boolean
  }>(),
  { icon: '•', count: 0, defaultOpen: false },
)

// defaultOpen 只在挂载时读一次：之后完全由用户控制（父组件重渲染不该把它折回去）
const isOpen = ref(!!props.defaultOpen)
</script>

<style scoped>
.ws-cp {
  border: 1px solid var(--ws-border);
  border-radius: var(--ws-radius);
  background: var(--ws-panel-2);
  overflow: hidden;
}
.ws-cp__head {
  display: flex;
  align-items: center;
  gap: 0.45em;
  width: 100%;
  padding: 0.5em 0.6em;
  border: 0;
  background: none;
  color: var(--ws-fg);
  font: inherit;
  font-weight: 600;
  text-align: left;
  cursor: pointer;
}
.ws-cp__ico {
  font-size: 1.05em;
}
.ws-cp__t {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ws-cp__caret {
  color: var(--ws-fg-dim);
  font-size: 0.9em;
}
.ws-cp__body {
  padding: 0 0.6em 0.6em;
  font-size: 0.94em;
}
</style>
