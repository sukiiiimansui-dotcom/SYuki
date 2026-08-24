<template>
  <div class="nm-page">
    <div class="nm-inner">
      <header class="nm-header">
        <button class="back" @click="goBack">‹</button>
        <h1><img class="ico" src="/assets/openmoji/1F3B5.svg" alt="" /> 网易云音乐</h1>
        <span class="sub">搜索 · 心情推歌</span>
      </header>

      <div class="search">
        <input v-model="keyword" placeholder="搜歌 / 歌手…" @keyup.enter="doSearch" />
        <button class="btn" :disabled="loading" @click="doSearch">搜索</button>
      </div>

      <div class="moods">
        <span class="mood-label">按心情推：</span>
        <button v-for="m in moods" :key="m" class="pill" :disabled="loading" @click="recommend(m)">{{ m }}</button>
      </div>

      <p v-if="error" class="err">{{ error }}</p>
      <p v-if="loading" class="empty">加载中…</p>

      <section v-if="songs.length" class="panel">
        <ul class="song-list">
          <li v-for="s in songs" :key="s.url" class="song-row">
            <div>
              <div class="t">{{ s.title }}<span class="soft"> · {{ s.artist }}</span></div>
              <div class="m">{{ s.album }} · {{ fmt(s.duration) }}</div>
            </div>
            <a class="btn ghost" :href="s.url" target="_blank" rel="noopener">播放</a>
          </li>
        </ul>
      </section>
      <p v-else-if="!loading && !error" class="empty">搜一首歌，或点上面心情按钮让 SYuki 推荐</p>
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
  min-height: 100vh;
  background: #f4f6f8;
  color: #1c2530;
}
.nm-inner {
  max-width: 720px;
  margin: 0 auto;
  padding: 24px 20px 56px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.nm-header {
  display: flex;
  align-items: center;
  gap: 12px;
}
.back {
  background: #fff;
  border: 1px solid #e3e8ee;
  border-radius: 999px;
  padding: 6px 14px;
  cursor: pointer;
  color: #33445a;
}
h1 {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 26px;
  font-weight: 700;
  margin: 0;
  letter-spacing: 0.5px;
}
.ico {
  width: 24px;
  height: 24px;
}
.sub {
  font-size: 13px;
  color: #7c8aa0;
}
.search {
  display: flex;
  gap: 10px;
}
.search input {
  flex: 1;
  background: #fff;
  border: 1px solid #e3e8ee;
  border-radius: 12px;
  padding: 12px 16px;
  font-size: 15px;
  color: #1c2530;
  outline: none;
}
.search input:focus {
  border-color: #4a90d9;
}
.btn {
  background: #4a90d9;
  border: none;
  color: #fff;
  font-weight: 600;
  border-radius: 12px;
  padding: 0 22px;
  cursor: pointer;
}
.btn:disabled {
  opacity: 0.5;
}
.btn.ghost {
  background: transparent;
  border: 1px solid #dfe5ec;
  color: #54708f;
  padding: 6px 14px;
  border-radius: 999px;
  font-size: 13px;
  text-decoration: none;
}
.moods {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}
.mood-label {
  color: #7c8aa0;
  font-size: 13px;
}
.pill {
  background: #fff;
  border: 1px solid #e3e8ee;
  color: #42536b;
  border-radius: 999px;
  padding: 6px 15px;
  font-size: 13px;
  cursor: pointer;
}
.pill:hover {
  border-color: #4a90d9;
  color: #2b6bb0;
}
.pill:disabled {
  opacity: 0.5;
}
.err {
  color: #d9534f;
  font-size: 14px;
}
.empty {
  color: #9aa7b8;
  font-size: 13px;
}
.panel {
  background: #fff;
  border: 1px solid #e8edf3;
  border-radius: 16px;
  padding: 10px 16px;
}
.song-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.song-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 10px 2px;
  border-bottom: 1px solid #f0f3f7;
}
.song-row:last-child {
  border-bottom: none;
}
.t {
  font-size: 15px;
  font-weight: 600;
  color: #1c2530;
}
.soft {
  color: #8a97a9;
  font-weight: 400;
  font-size: 13px;
}
.m {
  font-size: 12px;
  color: #8a97a9;
  margin-top: 3px;
}
</style>
