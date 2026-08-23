<template>
  <div class="bili-page">
    <div class="bili-bg"></div>

    <div class="bili-inner">
      <!-- 标题 + 返回 -->
      <div class="bili-head">
        <button class="bili-back" @click="goBack">‹ 返回</button>
        <h1 class="bili-title">B站学习</h1>
        <span class="bili-sub">热榜 · 搜索 · 弹幕文化库</span>
      </div>

      <!-- 搜索 -->
      <div class="bili-search">
        <input
          v-model="keyword"
          placeholder="搜索B站视频（如 零基础编程 / 猫娘 / Live2D）"
          @keyup.enter="doSearch"
        />
        <button class="bili-btn" :disabled="loading" @click="doSearch">搜索</button>
      </div>

      <p v-if="error" class="bili-error">{{ error }}</p>

      <!-- 热榜 -->
      <section class="bili-card">
        <div class="bili-card-head">
          <h2>🔥 B站热榜</h2>
          <button class="bili-btn-sm" @click="loadHot">刷新</button>
        </div>
        <ul v-if="hot.length" class="bili-list">
          <li v-for="bvid in hot" :key="bvid" class="bili-row">
            <span class="bili-row-id">{{ bvid }}</span>
            <button class="bili-btn-sm" @click="learn(bvid)">学习</button>
          </li>
        </ul>
        <p v-else class="bili-empty">热榜为空，点「刷新」加载</p>
      </section>

      <!-- 搜索结果 -->
      <section v-if="searchItems.length" class="bili-card">
        <div class="bili-card-head"><h2>📺 搜索结果</h2></div>
        <ul class="bili-list">
          <li v-for="s in searchItems" :key="s.bvid" class="bili-row">
            <div class="bili-row-main">
              <div class="bili-row-title">{{ s.title }}</div>
              <div class="bili-row-meta">UP {{ s.up }} · 播放 {{ s.play }} · 赞 {{ s.like }}</div>
            </div>
            <button class="bili-btn-sm" @click="learn(s.bvid)">学习</button>
          </li>
        </ul>
      </section>

      <!-- 学习库 -->
      <section class="bili-card">
        <div class="bili-card-head">
          <h2>📚 已学知识库</h2>
          <button class="bili-btn-sm" @click="loadKnowledge">刷新</button>
        </div>
        <p v-if="loading" class="bili-empty">处理中…</p>
        <ul v-else-if="knowledge.length" class="bili-list">
          <li v-for="k in knowledge" :key="k.bvid" class="bili-item">
            <div class="bili-row-title">{{ k.title }}（UP {{ k.up }}）</div>
            <div class="bili-row-meta">分区 {{ k.tname }} · 学到 {{ k.learned_at }}</div>
            <div v-if="k.repeat_danmaku" class="bili-tag-box">
              <span class="bili-tag">弹幕梗</span>{{ k.repeat_danmaku }}
            </div>
            <div v-if="k.top_comments" class="bili-tag-box">
              <span class="bili-tag">高赞评论</span>{{ k.top_comments }}
            </div>
          </li>
        </ul>
        <p v-else class="bili-empty">学习库还是空的，去热榜或搜索里学一个吧</p>
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

function goBack() {
  router.push('/')
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
  position: relative;
  width: 100%;
  min-height: 100vh;
  overflow: hidden;
  color: #eef4fb;
}
.bili-bg {
  position: absolute;
  inset: 0;
  background: radial-gradient(1200px 600px at 20% 0%, #1b3a6b 0%, #0d1b33 45%, #05080f 100%);
  z-index: -1;
}
.bili-inner {
  max-width: 760px;
  margin: 0 auto;
  padding: 24px 20px 48px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}
.bili-head {
  display: flex;
  align-items: center;
  gap: 14px;
}
.bili-back {
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.15);
  color: #cfe3ff;
  border-radius: 999px;
  padding: 6px 14px;
  cursor: pointer;
}
.bili-title {
  font-size: 26px;
  font-weight: 700;
  margin: 0;
  background: linear-gradient(90deg, #7fd0ff, #b9e6ff);
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
}
.bili-sub {
  font-size: 13px;
  color: #8aa6c9;
}
.bili-search {
  display: flex;
  gap: 10px;
}
.bili-search input {
  flex: 1;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.14);
  border-radius: 12px;
  padding: 11px 14px;
  color: #eef4fb;
  outline: none;
}
.bili-search input::placeholder {
  color: #6f87a8;
}
.bili-btn,
.bili-btn-sm {
  background: linear-gradient(90deg, #2497d9, #44b7fe);
  border: none;
  color: #fff;
  font-weight: 600;
  border-radius: 999px;
  cursor: pointer;
  transition: filter 0.2s;
}
.bili-btn {
  padding: 0 22px;
}
.bili-btn-sm {
  padding: 6px 14px;
  font-size: 13px;
}
.bili-btn:disabled,
.bili-btn-sm:disabled {
  filter: grayscale(0.5);
  opacity: 0.6;
}
.bili-error {
  color: #ff9aa0;
  font-size: 14px;
}
.bili-card {
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 18px;
  padding: 16px 18px;
  backdrop-filter: blur(10px);
}
.bili-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}
.bili-card-head h2 {
  font-size: 17px;
  margin: 0;
}
.bili-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.bili-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  background: rgba(255, 255, 255, 0.04);
  border-radius: 12px;
  padding: 10px 12px;
}
.bili-row-id {
  font-family: monospace;
  color: #9cc4ee;
  font-size: 13px;
}
.bili-row-main {
  flex: 1;
  min-width: 0;
}
.bili-row-title {
  font-size: 15px;
  font-weight: 600;
  color: #eef4fb;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bili-row-meta {
  font-size: 12px;
  color: #8aa6c9;
  margin-top: 3px;
}
.bili-item {
  background: rgba(255, 255, 255, 0.04);
  border-radius: 12px;
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.bili-tag-box {
  font-size: 13px;
  color: #c9ddf5;
  line-height: 1.5;
}
.bili-tag {
  display: inline-block;
  background: rgba(68, 183, 254, 0.22);
  color: #7fd0ff;
  border-radius: 6px;
  padding: 1px 7px;
  margin-right: 7px;
  font-size: 12px;
}
.bili-empty {
  color: #6f87a8;
  font-size: 13px;
  margin: 6px 0;
}
</style>
