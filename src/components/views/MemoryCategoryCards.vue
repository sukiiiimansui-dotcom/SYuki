<template>
  <div class="mcc">
    <div
      v-for="cat in cats"
      :key="cat.key"
      class="mcc-card"
      :class="{ open: expanded === cat.key }"
      :style="{ '--c': cat.color }"
      @click="toggle(cat.key)"
    >
      <div class="mcc-head">
        <span class="box" :style="{ background: cat.color }"></span>
        <span class="name">{{ cat.label }}</span>
        <span class="count">{{ items(cat.key).length }} 条</span>
        <span class="chev">{{ expanded === cat.key ? '▾' : '▸' }}</span>
      </div>
      <div v-if="expanded === cat.key" class="mcc-body">
        <div v-if="!items(cat.key).length" class="mcc-empty">该类别暂无记忆。</div>
        <div v-else class="mcc-list">
          <div v-for="(it, i) in items(cat.key)" :key="i" class="mcc-item">
            <sup>{{ i + 1 }}</sup>{{ it }}
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'

const props = defineProps<{
  sections: { short_term: string; long_term: string; user_info: string; promises: string }
}>()

const cats = [
  { key: 'short_term', label: '📍 近期回顾', color: '#FFB347' },
  { key: 'long_term', label: '📖 长期经历', color: '#7aa2f7' },
  { key: 'user_info', label: '🧑 用户信息', color: '#9ece6a' },
  { key: 'promises', label: '📌 重要约定', color: '#f7768e' },
] as const

const expanded = ref<string | null>(null)

function value(key: string): string {
  const s = props.sections as Record<string, string>
  return s[key] || ''
}
function items(key: string): string[] {
  return value(key)
    .split(/[\n。；;]+/)
    .map((t) => t.trim())
    .filter((t) => t.length >= 2)
}
function toggle(key: string) {
  expanded.value = expanded.value === key ? null : key
}
</script>

<style scoped>
.mcc { display: flex; flex-direction: column; gap: 10px; }
.mcc-card {
  border: 1px solid #ededed;
  border-left: 3px solid var(--c);
  border-radius: 12px;
  background: #fff;
  overflow: hidden;
  cursor: pointer;
  transition: background .15s;
}
.mcc-card.open { background: #fafafa; }
.mcc-head { display: flex; align-items: center; gap: 10px; padding: 11px 14px; }
.box { width: 12px; height: 12px; border-radius: 3px; flex-shrink: 0; }
.name { font-size: 14px; font-weight: 700; color: #111; }
.count { margin-left: auto; font-size: 12px; color: #999; }
.chev { font-size: 13px; color: #999; }
.mcc-body { padding: 2px 14px 14px 38px; animation: mccOpen .18s ease; }
.mcc-empty { color: #999; font-size: 12px; }
.mcc-list { display: flex; flex-direction: column; gap: 8px; }
.mcc-item { font-size: 12.5px; color: #444; line-height: 1.6; border-left: 2px solid #eee; padding-left: 10px; word-break: break-word; }
.mcc-item sup { color: #999; margin-right: 5px; font-size: 10px; }
@keyframes mccOpen { from { opacity: 0; transform: translateY(-6px); } to { opacity: 1; transform: none; } }
</style>
