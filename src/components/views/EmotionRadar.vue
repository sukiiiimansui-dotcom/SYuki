<template>
  <div class="er-root">
    <div class="er-head">
      <span class="er-title">心情雷达 <small>（近 30 条回复情绪）</small></span>
      <span v-if="rawCount" class="er-count">样本 {{ rawCount }}</span>
    </div>

    <div v-if="rawCount">
      <svg class="er-svg" viewBox="0 0 200 200">
        <g v-for="a in AXES" :key="a.key">
          <line :x1="100" :y1="100" :x2="100 + Math.cos(a.angle) * 86" :y2="100 + Math.sin(a.angle) * 86"
            stroke="rgba(255,255,255,.12)" stroke-width="1" />
          <text :x="100 + Math.cos(a.angle) * 99" :y="100 + Math.sin(a.angle) * 99"
            transform="translate(0,0)" text-anchor="middle" dominant-baseline="middle" class="er-axis">
            {{ a.label }}
          </text>
        </g>
        <polygon
          :points="polygon(100)"
          fill="rgba(0,0,0,.08)"
          stroke="#111"
          stroke-width="1.5"
        />
        <polygon :points="polygon(60)" fill="rgba(0,0,0,.05)" stroke="none" />
        <polygon :points="polygon(20)" fill="rgba(0,0,0,.04)" stroke="none" />
        <circle v-for="a in AXES" :key="'p'+a.key" cx="100" cy="100"
          :r="4" fill="#111" />
      </svg>
      <div class="er-chips">
        <span v-for="(v, k) in dims" :key="k" class="chip">
          {{ labelOf(k) }} {{ Math.round(v) }}
        </span>
      </div>
      <div class="er-timeline">
        <span v-for="(c, i) in timeline" :key="i" class="tl-item" :style="{ background: c.color }" :title="c.text">
          {{ c.text[0] }}
        </span>
      </div>
    </div>
    <div v-else class="er-empty">进入聊天、有情绪判定的回复后，这里会统计心情雷达。</div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useGameStore } from '@/stores/modules/game'

const gameStore = useGameStore()

// 19 种情绪 → 五维（喜悦/亲近/害羞/烦躁/活力）映射
const MAP: Record<string, string[]> = {
  joy: ['高兴', '兴奋', '调皮', '自信', '心动'],
  close: ['心动', '平静', '认真'],
  shy: ['害羞', '难为情', '紧张'],
  agitated: ['生气', '厌恶', '无奈', '担心', '慌张', '害怕', '哭泣'],
  vitality: ['兴奋', '调皮', '自信'],
}
const DIM_LABEL: Record<string, string> = {
  joy: '喜悦', close: '亲近', shy: '害羞', agitated: '烦躁', vitality: '活力',
}
const AXES = [
  { key: 'joy', label: '喜悦', angle: -Math.PI / 2 },
  { key: 'close', label: '亲近', angle: -Math.PI / 2 + (2 * Math.PI) / 5 },
  { key: 'shy', label: '害羞', angle: -Math.PI / 2 + (2 * Math.PI * 2) / 5 },
  { key: 'agitated', label: '烦躁', angle: -Math.PI / 2 + (2 * Math.PI * 3) / 5 },
  { key: 'vitality', label: '活力', angle: -Math.PI / 2 + (2 * Math.PI * 4) / 5 },
]

// 从对话历史取最近情绪
const recentEmotions = computed(() => {
  const hs = gameStore.dialogHistory || []
  const list = hs
    .filter((m) => m.emotion && m.emotion !== '正常' && m.emotion !== '未知')
    .map((m) => m.emotion as string)
  return list.slice(-30)
})
const rawCount = computed(() => recentEmotions.value.length)

const counts = computed(() => {
  const c: Record<string, number> = {}
  for (const e of recentEmotions.value) c[e] = (c[e] || 0) + 1
  return c
})

const dims = computed<Record<string, number>>(() => {
  const d: Record<string, number> = {}
  let max = 0
  // 先把每条情绪归到维度
  // 每条情绪只算一个维度（按第一个命中的维度）
  const perDim: Record<string, number> = {}
  for (const label of Object.keys(counts.value)) {
    for (const dim of Object.keys(MAP)) {
      if (MAP[dim].includes(label)) { perDim[dim] = (perDim[dim] || 0) + counts.value[label]; break }
    }
  }
  for (const dim of Object.keys(DIM_LABEL)) max = Math.max(max, perDim[dim] || 0)
  for (const dim of Object.keys(DIM_LABEL)) {
    d[dim] = max ? ((perDim[dim] || 0) / max) * 100 : 0
  }
  return d
})

const timeline = computed(() =>
  recentEmotions.value.map((t) => {
    let color = '#7aa2f7'
    for (const dim of Object.keys(MAP)) {
      if (MAP[dim].includes(t)) {
        color = { joy: '#9ece6a', close: '#7aa2f7', shy: '#e0af68', agitated: '#f7768e', vitality: '#f9a8d4' }[dim] || '#7aa2f7'
        break
      }
    }
    return { text: t, color }
  }),
)

function polygon(r: number): string {
  const pts = AXES.filter((a) => dims.value[a.key] !== undefined).map((a) => {
    const v = dims.value[a.key] * (r / 100)
    return `${100 + Math.cos(a.angle) * v},${100 + Math.sin(a.angle) * v}`
  })
  return pts.join(' ')
}
function labelOf(k: string) { return DIM_LABEL[k] || k }
</script>

<style scoped>
.er-root { padding: 8px 4px; color: #333; }
.er-head { display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 6px; }
.er-title { font-size: 14px; font-weight: 700; color: #111; }
.er-title small { font-weight: 400; color: #999; }
.er-count { font-size: 11px; color: #999; }
.er-svg { width: 210px; height: 210px; margin: 0 auto; display: block; }
.er-axis { font-size: 11px; fill: #888; }
.er-chips { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 4px; }
.chip { background: #f4f4f4; color: #333; border-radius: 8px; padding: 3px 9px; font-size: 12px; }
.er-timeline { display: flex; gap: 3px; margin-top: 10px; flex-wrap: wrap; }
.tl-item { width: 14px; height: 14px; border-radius: 3px; font-size: 9px; display: flex; align-items: center; justify-content: center; color: #fff; }
.er-empty { color: #999; font-size: 13px; margin-top: 8px; }
</style>
