<template>
  <div class="bili-page">
    <div class="bili-inner">
      <header class="bili-header">
        <button class="back" @click="goBack">‹</button>
        <h1>B站学习</h1>
        <span class="sub">热榜 · 搜索 · 学习时间轴</span>
      </header>

      <div class="search">
        <input v-model="keyword" placeholder="搜索B站视频…" @keyup.enter="doSearch" />
        <button class="btn" :disabled="loading" @click="doSearch">搜索</button>
      </div>

      <p v-if="error" class="err">{{ error }}</p>
      <p v-if="loading" class="empty">加载中…</p>

      <section class="panel">
        <div class="panel-head">
          <h2>热榜</h2>
          <button class="btn ghost" :disabled="loading" @click="loadHot">刷新</button>
        </div>
        <ul v-if="hot.length" class="hot-list">
          <li v-for="b in hot" :key="b" class="hot-row">
            <span class="bvid">{{ b }}</span>
            <button class="btn ghost" :disabled="loading" @click="learn(b)">学习</button>
          </li>
        </ul>
        <p v-else class="empty">点「刷新」加载热榜</p>
      </section>

      <section v-if="searchItems.length" class="panel">
        <div class="panel-head"><h2>搜索结果</h2></div>
        <ul class="res-list">
          <li v-for="s in searchItems" :key="s.bvid" class="res-row">
            <div>
              <div class="t">{{ s.title }}</div>
              <div class="m">UP {{ s.up }} · 播放 {{ s.play }} · 赞 {{ s.like }}</div>
            </div>
            <button class="btn ghost" :disabled="loading" @click="learn(s.bvid)">学习</button>
          </li>
        </ul>
      </section>

      <!-- 学习库：时间轴卡片 -->
      <section class="panel">
        <div class="panel-head">
          <h2>学习时间轴</h2>
          <button class="btn ghost" :disabled="loading" @click="loadKnowledge">刷新</button>
        </div>
        <p v-if="!knowledge.length" class="empty">还没学过，去热榜或搜索里学一个吧</p>
        <div v-else class="timeline">
          <div
            v-for="(k, i) in knowledge"
            :key="k.bvid"
            class="tl-item"
            :class="{ open: expanded === k.bvid }"
          >
            <div class="tl-marker">
              <span v-if="i === 0" class="tl-now">新</span>
              <span v-else class="tl-dot"></span>
            </div>
            <div class="tl-card" @click="toggle(k.bvid)">
              <div class="tl-top">
                <div class="t">{{ k.title }}<span class="soft"> · {{ k.up }}</span></div>
                <div class="m">{{ fmtTime(k.learned_at) }} · {{ k.tname }}</div>
              </div>
              <div v-if="k.repeat_danmaku" class="tag-row">
                <span class="tag">弹幕梗</span>{{ k.repeat_danmaku }}
              </div>
              <div v-if="k.top_comments" class="tag-row">
                <span class="tag">高赞评论</span>{{ k.top_comments }}
              </div>
              <div v-if="expanded === k.bvid" class="detail">
                <p v-if="k.vdesc" class="desc">{{ k.vdesc }}</p>
                <p v-if="k.culture" class="desc">文化：{{ k.culture }}</p>
              </div>
              <div class="expand">{{ expanded === k.bvid ? '收起' : '查看详情' }}</div>
            </div>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { biliHot, biliKnowledge, biliLearn, biliSearch } from '@/api/services/bilibili'
import type { BiliSearchItem, BiliVideo } from '@/api/services/bilibili'

const router = useRouter()
const keyword = ref('')
const hot = ref<string[]>([])
const searchItems = ref<BiliSearchItem[]>([])
const knowledge = ref<BiliVideo[]>([])
const loading = ref(false)
const error = ref('')
const expanded = ref('')

function goBack() {
  router.push('/')
}
function toggle(id: string) {
  expanded.value = expanded.value === id ? '' : id
}
function fmtTime(ts: string): string {
  const n = Number(ts)
  if (!n) return ts || '—'
  const d = new Date(n * 1000)
  const p = (x: number) => x.toString().padStart(2, '0')
  return `${d.getMonth() + 1}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}
async function loadHot() {
  loading.value = true
  error.value = ''
  try {
    hot.value = await biliHot(10)
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : e?.message || '加载热榜失败'
  } finally {
    loading.value = false
  }
}
async function doSearch() {
  if (!keyword.value.trim()) return
  loading.value = true
  error.value = ''
  try {
    searchItems.value = await biliSearch(keyword.value.trim(), 8)
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : e?.message || '搜索失败'
  } finally {
    loading.value = false
  }
}
async function learn(bvid: string) {
  loading.value = true
  error.value = ''
  try {
    const r = await biliLearn(bvid)
    if (r.ok) {
      await loadKnowledge()
    } else {
      error.value = '学习失败：视频不存在或网络错误'
    }
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : e?.message || '学习失败'
  } finally {
    loading.value = false
  }
}
async function loadKnowledge() {
  try {
    knowledge.value = await biliKnowledge('', 20)
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : e?.message || '加载学习库失败'
  }
}
onMounted(() => {
  loadHot()
  loadKnowledge()
})
</script>

<style scoped>
.bili-page {
  min-height: 100vh;
  background: #f4f6f8;
  color: #1c2530;
}
.bili-inner {
  max-width: 760px;
  margin: 0 auto;
  padding: 24px 20px 56px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}
.bili-header {
  display: flex;
  align-items: center;
  gap: 14px;
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
  font-size: 26px;
  font-weight: 700;
  margin: 0;
  letter-spacing: 0.5px;
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
}
.err {
  color: #d9534f;
  font-size: 14px;
}
.empty {
  color: #9aa7b8;
  font-size: 13px;
  margin: 6px 0;
}
.panel {
  background: #fff;
  border: 1px solid #e8edf3;
  border-radius: 16px;
  padding: 16px 18px;
}
.panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}
.panel-head h2 {
  font-size: 16px;
  margin: 0;
  font-weight: 700;
}
.hot-list,
.res-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.hot-row,
.res-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  background: #f8fafc;
  border-radius: 12px;
  padding: 10px 12px;
}
.bvid {
  font-family: monospace;
  color: #6d84a3;
  font-size: 13px;
}
.t {
  font-size: 15px;
  font-weight: 600;
  color: #1c2530;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.m {
  font-size: 12px;
  color: #8a97a9;
  margin-top: 3px;
}
/* 学习时间轴 */
.timeline {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.tl-item {
  display: flex;
  gap: 12px;
}
.tl-marker {
  width: 20px;
  display: flex;
  justify-content: center;
  padding-top: 10px;
}
.tl-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: #b9c7d6;
  border: 2px solid #fff;
  box-shadow: 0 0 0 2px #e8edf3;
}
.tl-now {
  background: #4a90d9;
  color: #fff;
  font-size: 10px;
  font-weight: 700;
  border-radius: 999px;
  padding: 3px 9px;
}
.tl-card {
  flex: 1;
  background: #f8fafc;
  border: 1px solid #eef2f6;
  border-radius: 14px;
  padding: 12px 14px;
  cursor: pointer;
  transition: box-shadow 0.2s;
}
.tl-card:hover {
  box-shadow: 0 4px 18px rgba(20, 40, 70, 0.06);
}
.tl-top .t {
  font-size: 15px;
}
.tl-top .m {
  font-size: 12px;
  color: #8a97a9;
  margin-top: 3px;
}
.soft {
  color: #8a97a9;
  font-weight: 400;
  font-size: 13px;
}
.tag-row {
  font-size: 13px;
  color: #42536b;
  line-height: 1.6;
  margin-top: 9px;
}
.tag {
  display: inline-block;
  background: #eaf1fb;
  color: #2b6bb0;
  border-radius: 6px;
  padding: 1px 7px;
  margin-right: 7px;
  font-size: 12px;
}
.detail {
  border-top: 1px dashed #e3e8ee;
  margin-top: 10px;
  padding-top: 10px;
}
.desc {
  font-size: 13px;
  color: #54657c;
  line-height: 1.6;
  margin: 0 0 6px;
}
.expand {
  margin-top: 8px;
  font-size: 12px;
  color: #4a90d9;
}
</style>
