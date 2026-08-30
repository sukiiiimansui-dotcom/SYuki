<template>
  <div class="mem-page">
    <div class="mem-inner">
      <header class="mem-header">
        <button class="back" @click="goBack">‹</button>
        <h1>记忆</h1>
        <span class="sub">{{ roleName || '角色记忆库' }}</span>
      </header>

      <div class="status-bar">
        <span class="chip" :class="{ off: !memory.memory_enabled }">
          {{ memory.memory_enabled ? '永久记忆 · 已开启' : '永久记忆 · 未开启' }}
        </span>
        <span v-if="memory.updated_at" class="ts">最近更新 {{ memory.updated_at }}</span>
      </div>

      <div class="search">
        <input v-model="keyword" placeholder="搜索记忆内容…" />
        <button class="btn" :disabled="loading" @click="load">刷新</button>
      </div>

      <p v-if="error" class="err">{{ error }}</p>
      <p v-if="loading" class="empty">加载中…</p>
      <p v-else-if="!memory.role_id" class="empty">当前没有已加载的角色记忆。</p>

      <template v-if="memory.role_id">
        <section v-for="sec in sections" :key="sec.key" class="panel">
          <div class="panel-head">
            <h2>{{ sec.label }}</h2>
            <span class="badge">{{ sec.key }}</span>
          </div>
          <div v-if="!filtered(sec).length" class="empty">{{ sec.empty }}</div>
          <ul v-else class="lines">
            <li v-for="(line, i) in filtered(sec)" :key="i" class="line">{{ line }}</li>
          </ul>
        </section>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { getRoleMemoryBank, type RoleMemoryView } from '@/api/services/memory'
import { useGameStore } from '@/stores/modules/game'

const router = useRouter()
const route = useRoute()
const gameStore = useGameStore()

const empty = (): RoleMemoryView => ({
  role_id: 0,
  role_name: '',
  memory_enabled: false,
  schema_version: 1,
  updated_at: '',
  short_term: '暂无近期对话摘要。',
  long_term: '暂无长期关键经历。',
  user_info: '暂无用户特征记录。',
  promises: '暂无未完成的约定。',
})

const memory = ref<RoleMemoryView>(empty())
const keyword = ref('')
const loading = ref(false)
const error = ref('')

type SectionKey = keyof RoleMemoryView

const sections = computed<{ key: SectionKey; label: string; empty: string }[]>(() => [
  { key: 'short_term', label: '📍 近期回顾', empty: memory.value.short_term },
  { key: 'long_term', label: '📖 长期经历', empty: memory.value.long_term },
  { key: 'user_info', label: '🧑 用户信息', empty: memory.value.user_info },
  { key: 'promises', label: '📌 重要约定', empty: memory.value.promises },
])

const roleName = computed(() => memory.value.role_name)

function roleId(): number {
  const q = Number(route.query.roleId)
  if (q && q > 0) return q
  // 预览：角色未选时回退到角色 1（浏览器 mock 预览能立即看到记忆）
  return gameStore.mainRoleId || gameStore.currentInteractRoleId || 1
}

function filtered(sec: { key: SectionKey }): string[] {
  const raw = String(memory.value[sec.key] || '')
  const lines = raw.split('\n').map((l) => l.trim()).filter(Boolean)
  const k = keyword.value.trim().toLowerCase()
  if (!k) return lines
  return lines.filter((l) => l.toLowerCase().includes(k))
}

async function load() {
  const id = roleId()
  if (!id) {
    memory.value = empty()
    return
  }
  loading.value = true
  error.value = ''
  try {
    memory.value = await getRoleMemoryBank(id)
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : '读取记忆失败'
  } finally {
    loading.value = false
  }
}

function goBack() {
  router.push('/')
}

onMounted(load)
</script>

<style scoped>
.mem-page {
  min-height: 100vh;
  background: #f4f6f8;
  color: #1c2530;
}
.mem-inner {
  max-width: 760px;
  margin: 0 auto;
  padding: 24px 20px 56px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}
.mem-header {
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
.status-bar {
  display: flex;
  gap: 12px;
  align-items: center;
  flex-wrap: wrap;
}
.chip {
  background: #eaf4ff;
  color: #2b6cb0;
  border-radius: 999px;
  padding: 5px 12px;
  font-size: 13px;
  font-weight: 600;
}
.chip.off {
  background: #f2f3f5;
  color: #8a93a3;
}
.ts {
  font-size: 12px;
  color: #9aa7b8;
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
  margin-bottom: 10px;
}
.panel-head h2 {
  font-size: 16px;
  margin: 0;
  color: #25405f;
}
.badge {
  font-size: 11px;
  color: #7c8aa0;
  background: #eef2f7;
  border-radius: 999px;
  padding: 2px 8px;
}
.lines {
  margin: 0;
  padding: 0;
  list-style: none;
}
.line {
  font-size: 14px;
  line-height: 1.75;
  color: #3a4a5f;
  padding: 4px 0;
  border-bottom: 1px dashed #eef2f5;
  white-space: pre-wrap;
  word-break: break-word;
}
.line:last-child {
  border-bottom: none;
}
</style>
