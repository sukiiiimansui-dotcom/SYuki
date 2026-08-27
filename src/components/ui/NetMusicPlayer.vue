<template>
  <div v-if="store.current" class="netmusic-player">
    <!-- 迷你播放条：常驻左下角，切页/玩游戏都不停 -->
    <div
      class="fixed bottom-[calc(64px+var(--safe-area-inset-bottom))] left-4 z-1000 flex items-center gap-3 pl-4 pr-3 py-2 bg-slate-900/50 backdrop-blur-md rounded-xl drop-shadow-[0_8px_12px_rgba(0,0,0,0.3)] text-white"
      :style="{ maxWidth: 'min(420px, 90vw)' }"
    >
      <!-- 音符图标 -->
      <Music :size="16" class="shrink-0 text-cyan-300" :class="{ 'animate-pulse': store.playing }" />

      <!-- 歌曲信息 + 进度 -->
      <div class="min-w-0 flex-1">
        <div class="text-sm font-medium truncate">{{ store.current.title }}<span class="opacity-60 ml-1">{{ store.current.artist }}</span></div>
        <!-- 进度条 -->
        <div class="mt-1 h-1 bg-white/20 rounded overflow-hidden">
          <div class="h-full bg-cyan-400 transition-all" :style="{ width: progressPct + '%' }"></div>
        </div>
      </div>

      <!-- 控制按钮 -->
      <div class="flex items-center gap-1.5 shrink-0">
        <button class="p-1.5 rounded-lg hover:bg-white/10" :title="store.playing ? '暂停' : '播放'" @click="togglePlay">
          <Pause v-if="store.playing" :size="16" />
          <Play v-else :size="16" />
        </button>
        <button class="p-1.5 rounded-lg hover:bg-white/10" title="下一首" @click="store.next()">
          <SkipForward :size="16" />
        </button>
        <button class="p-1.5 rounded-lg hover:bg-white/10" title="关闭" @click="stop">
          <X :size="16" />
        </button>
      </div>

      <!-- 独立音量 -->
      <input
        v-model.number="vol"
        type="range"
        min="0"
        max="100"
        class="hidden md:block w-16 accent-cyan-400"
        title="网易云音量（独立于游戏BGM）"
      />
    </div>

    <!-- 隐藏的音频播放器 -->
    <audio
      ref="audioEl"
      :src="currentUrl"
      :volume="store.volume / 100"
      @timeupdate="onTimeUpdate"
      @ended="store.next()"
      @play="store.playing = true"
      @pause="store.playing = false"
    ></audio>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { Music, Play, Pause, SkipForward, X } from 'lucide-vue-next'
import { useNetmusicStore } from '@/stores/modules/netmusic'

const store = useNetmusicStore()
const audioEl = ref<HTMLAudioElement | null>(null)

const currentUrl = computed(() => (store.current ? (store.current as any).url : ''))
const progressPct = computed(() => {
  if (!store.duration) return 0
  return Math.min(100, (store.progress / store.duration) * 100)
})

const vol = computed({
  get: () => store.volume,
  set: (v: number) => store.setVolume(Number(v)),
})

function onTimeUpdate() {
  if (audioEl.value) {
    store.progress = audioEl.value.currentTime
    store.duration = audioEl.value.duration || store.duration
  }
}

function togglePlay() {
  if (!audioEl.value) return
  if (store.playing) {
    audioEl.value.pause()
  } else {
    audioEl.value.play().catch((e) => console.warn('网易云播放被拦截:', e))
  }
}

function stop() {
  if (audioEl.value) {
    audioEl.value.pause()
    audioEl.value.src = ''
  }
  store.clear()
}

// 当前歌曲变化时重新加载 audio
watch(
  () => currentUrl.value,
  (url) => {
    if (audioEl.value && url) {
      audioEl.value.play().catch((e) => console.warn('网易云播放失败（可能限制外链）:', e))
    }
  },
)

// 音量变化同步
watch(
  () => store.volume,
  (v) => {
    if (audioEl.value) audioEl.value.volume = v / 100
  },
)

// 播放/暂停状态与 audio 同步
watch(
  () => store.playing,
  (playing) => {
    if (!audioEl.value) return
    if (playing) {
      audioEl.value.play().catch((e) => console.warn('网易云播放被拦截:', e))
    } else {
      audioEl.value.pause()
    }
  },
)

onMounted(() => {})
onUnmounted(() => {})
</script>

<style scoped></style>
