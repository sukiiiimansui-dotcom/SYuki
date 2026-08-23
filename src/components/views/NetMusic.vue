<template>
  <div class="nm-page">
    <div class="nm-bg"></div>

    <div class="nm-inner">
      <div class="nm-head">
        <button class="nm-back" @click="goBack">‹ 返回</button>
        <h1 class="nm-title">网易云音乐</h1>
        <span class="nm-sub">搜索 · 心情推歌</span>
      </div>

      <div class="nm-search">
        <input v-model="keyword" placeholder="搜歌 / 歌手（如 周杰伦 晴天）" @keyup.enter="doSearch" />
        <button class="nm-btn" :disabled="loading" @click="doSearch">搜索</button>
      </div>

      <div class="nm-moods">
        <span class="nm-mood-label">按心情推：</span>
        <button v-for="m in moods" :key="m" class="nm-btn-sm" :disabled="loading" @click="recommend(m)">
          {{ m }}
        </button>
      </div>

      <p v-if="error" class="nm-error">{{ error }}</p>
      <p v-if="loading" class="nm-empty">加载中…</p>

      <section v-if="songs.length" class="nm-card">
        <ul class="nm-list">
          <li v-for="s in songs" :key="s.url" class="nm-item">
            <div class="nm-row-main">
              <div class="nm-row-title">{{ s.title }}<span class="nm-artist"> - {{ s.artist }}</span></div>
              <div class="nm-row-meta">{{ s.album }} · {{ fmt(s.duration) }}</div>
            </div>
            <a class="nm-btn-sm" :href="s.url" target="_blank" rel="noopener">播放</a>
          </li>
        </ul>
      </section>
      <p v-else-if="!loading && !error" class="nm-empty">搜一首歌，或点上面的心情按钮让 SYuki 推荐</p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { netmusicRecommend, netmusicSearch } from '@/api/services/netmusic'
import type { NetMusicSong } from '@/api/services/netmusic'

const router = useRouter()
const keyword = ref('')
const songs = ref<NetMusicSong[]>([])
const loading = ref(false)
const error = ref('')
const moods = ['happy', 'sad', 'relax', 'calm', 'study', 'music']

function goBack() {
  router.push('/')
}
function fmt(sec: number): string {
  if (!sec) return '—'
  const m = Math.floor(sec / 60)
  const s = sec % 60
  return `${m}:${s.toString().padStart(2, '0')}`
}
async function doSearch() {
  if (!keyword.value.trim()) return
  loading.value = true
  error.value = ''
  try {
    songs.value = await netmusicSearch(keyword.value.trim(), 8)
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : e?.message || '搜索失败'
  } finally {
    loading.value = false
  }
}
async function recommend(m: string) {
  loading.value = true
  error.value = ''
  try {
    songs.value = await netmusicRecommend(m, 10)
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : e?.message || '推荐失败'
  } finally {
    loading.value = false
  }
}
onMounted(() => {
  recommend('calm')
})
</script>

<style scoped>
.nm-page {
  position: relative;
  width: 100%;
  min-height: 100vh;
  overflow: hidden;
  color: #eef4fb;
}
.nm-bg {
  position: absolute;
  inset: 0;
  background: radial-gradient(1100px 560px at 20% 0%, #1b3a6b 0%, #0d1b33 48%, #05080f 100%);
  z-index: -1;
}
.nm-inner {
  max-width: 720px;
  margin: 0 auto;
  padding: 24px 20px 48px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.nm-head {
  display: flex;
  align-items: center;
  gap: 14px;
}
.nm-back {
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.15);
  color: #cfe3ff;
  border-radius: 999px;
  padding: 6px 14px;
  cursor: pointer;
}
.nm-title {
  font-size: 26px;
  font-weight: 700;
  margin: 0;
  background: linear-gradient(90deg, #7fd0ff, #b9e6ff);
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
}
.nm-sub {
  font-size: 13px;
  color: #8aa6c9;
}
.nm-search {
  display: flex;
  gap: 10px;
}
.nm-search input {
  flex: 1;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.14);
  border-radius: 12px;
  padding: 11px 14px;
  color: #eef4fb;
  outline: none;
}
.nm-search input::placeholder {
  color: #6f87a8;
}
.nm-btn,
.nm-btn-sm {
  background: linear-gradient(90deg, #2497d9, #44b7fe);
  border: none;
  color: #fff;
  font-weight: 600;
  border-radius: 999px;
  cursor: pointer;
  transition: filter 0.2s;
}
.nm-btn {
  padding: 0 22px;
}
.nm-btn-sm {
  padding: 6px 13px;
  font-size: 13px;
  text-decoration: none;
}
.nm-btn:disabled,
.nm-btn-sm:disabled {
  filter: grayscale(0.5);
  opacity: 0.6;
}
.nm-moods {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}
.nm-mood-label {
  color: #9cc4ee;
  font-size: 13px;
}
.nm-error {
  color: #ff9aa0;
  font-size: 14px;
}
.nm-empty {
  color: #6f87a8;
  font-size: 13px;
}
.nm-card {
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 18px;
  padding: 14px 16px;
  backdrop-filter: blur(10px);
}
.nm-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.nm-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  background: rgba(255, 255, 255, 0.04);
  border-radius: 12px;
  padding: 10px 12px;
}
.nm-row-main {
  flex: 1;
  min-width: 0;
}
.nm-row-title {
  font-size: 15px;
  font-weight: 600;
  color: #eef4fb;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.nm-artist {
  color: #9cc4ee;
  font-size: 13px;
  font-weight: 400;
}
.nm-row-meta {
  font-size: 12px;
  color: #8aa6c9;
  margin-top: 3px;
}
</style>
