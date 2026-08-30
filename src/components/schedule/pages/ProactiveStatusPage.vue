<template>
  <div v-if="uiStore.scheduleView === 'proactive_status'" class="space-y-6 p-1">
    <div
      v-for="s in statusCards"
      :key="s.label"
      class="bg-white/60 glass-panel border border-cyan-500/20 rounded-2xl p-5"
    >
      <div class="flex items-center justify-between mb-2">
        <div class="text-lg font-bold text-white">{{ s.label }}</div>
        <div class="text-xs text-white/70">{{ s.extra }}</div>
      </div>
      <div class="text-sm text-white/90 leading-relaxed whitespace-pre-wrap break-words">
        {{ s.value }}
      </div>
    </div>

    <div class="bg-white/60 glass-panel border border-cyan-500/20 rounded-2xl p-5">
      <div class="flex items-center justify-between mb-3">
        <div class="text-lg font-bold text-white">🔥 想念 / 主动历史</div>
        <button
          class="px-4 py-2 bg-cyan-500 hover:bg-cyan-600 text-white rounded-lg text-sm font-medium"
          :disabled="loading"
          @click="load"
        >
          {{ loading ? '刷新中…' : '刷新' }}
        </button>
      </div>
      <p v-if="error" class="text-sm text-red-300">{{ error }}</p>
      <div v-if="!history.length && !error" class="text-sm text-white/60">
        还没有主动投放记录。当 AI 想念你、主动搭话、或触发日程提醒时会出现在这里。
      </div>
      <ul v-else class="space-y-3">
        <li
          v-for="(e, i) in history"
          :key="i"
          class="border-l-2 pl-4 py-2 rounded-r-xl"
          :class="kindClass(e.kind)"
        >
          <div class="flex items-center gap-2 text-sm">
            <span class="px-2 py-0.5 rounded-full text-xs font-bold">{{ kindLabel(e.kind) }}</span>
            <span class="text-white/60">{{ fmtTime(e.ts_ms) }}</span>
          </div>
          <p class="text-sm text-white/85 mt-1 leading-relaxed">{{ e.preview }}</p>
        </li>
      </ul>
    </div>

    <div class="bg-white/60 glass-panel border border-cyan-500/20 rounded-2xl p-5">
      <div class="text-lg font-bold text-white mb-3">⏳ 等待投放的意图 (小本本)</div>
      <div v-if="!pending.length" class="text-sm text-white/60">暂无待投放意图。</div>
      <ul v-else class="space-y-2">
        <li v-for="(p, i) in pending" :key="i" class="text-sm text-white/85 flex justify-between">
          <span class="px-2 py-0.5 rounded-full text-xs font-bold bg-amber-500/20 text-amber-200">
            {{ kindLabel(p.kind) }}
          </span>
          <span class="text-white/50">已等 {{ p.waited_secs }}s</span>
        </li>
      </ul>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useUIStore } from '@/stores/modules/ui/ui'
import { getProactiveStatus, type ProactiveStatusSnapshot } from '@/api/services/schedule'

const uiStore = useUIStore()

const status = ref<ProactiveStatusSnapshot | null>(null)
const loading = ref(false)
const error = ref('')
let timer: number | undefined

const history = computed(() => status.value?.history ?? [])
const pending = computed(() => status.value?.pending_intents ?? [])

const statusCards = computed(() => {
  const s = status.value
  if (!s) return []
  return [
    {
      label: '🌱 主动系统',
      extra: s.running ? '后台轮询运行中' : '未运行',
      value: `${s.enabled ? '已开启' : '已关闭'} · 现在${s.can_deliver ? '可以' : '不适合'}投放 · 距上次交互 ${fmtAgo(s.last_interaction_ago_secs)}`,
    },
    {
      label: '💭 想念',
      extra: `本轮离开已想念 ${s.away_delivered_count}/${s.away_max_times} 次`,
      value: `离开 ${fmtAgo(s.away_timeout_secs)} 秒后触发想念。`,
    },
    {
      label: '📈 兴趣值',
      extra: `主动 ${s.proactive_times}/${s.max_proactive_count} 次`,
      value: `${s.interest.toFixed(1)} / ${s.interest_cap.toFixed(1)}`,
    },
    {
      label: '👀 当前感知',
      extra: s.state,
      value: s.description || '无描述',
    },
  ]
})

function fmtAgo(secs: number): string {
  if (secs < 60) return `${secs}s`
  const m = Math.floor(secs / 60)
  return `${m}分${secs % 60}s`
}
function fmtTime(ms: number): string {
  if (!ms) return '—'
  const d = new Date(ms)
  const p = (n: number) => n.toString().padStart(2, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
}
function kindLabel(kind: string): string {
  const map: Record<string, string> = {
    miss: '想念',
    alarm: '日程提醒',
    todo: '待办',
    important_day: '重要日子',
    screen: '屏幕感知',
    topic: '闲聊',
  }
  return map[kind] ?? kind
}
function kindClass(kind: string): string {
  if (kind === 'miss') return 'border-pink-400/60'
  if (kind === 'alarm') return 'border-amber-400/60'
  if (kind === 'todo' || kind === 'important_day') return 'border-cyan-400/60'
  return 'border-white/30'
}

async function load() {
  loading.value = true
  error.value = ''
  try {
    status.value = await getProactiveStatus()
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : '读取主动状态失败'
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  load()
  timer = window.setInterval(load, 30000)
})
onUnmounted(() => {
  if (timer) window.clearInterval(timer)
})
</script>
