<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="mfw"
      :class="{ fullscreen }"
      :style="{
        left: pos.x + 'px',
        top: pos.y + 'px',
        width: pos.w + 'px',
        height: minimized ? '40px' : pos.h + 'px',
      }"
    >
      <!-- 标题栏（拖动区） -->
      <div class="mfw-head" @mousedown="startDrag" @touchstart.passive="startDragT">
        <span class="mfw-title">🧠 记忆 · {{ roleName || '角色' }}</span>
        <div class="mfw-ops" @mousedown.stop @touchstart.stop>
          <button class="mfw-op" @click="toggleFullscreen" :title="fullscreen ? '退出全屏' : '全屏'">{{ fullscreen ? '🗗' : '⛶' }}</button>
          <button class="mfw-op" @click="minimized = !minimized">{{ minimized ? '▢' : '—' }}</button>
          <button class="mfw-op" @click="close">✕</button>
        </div>
      </div>

      <template v-if="!minimized">
        <div class="mfw-tabs">
          <button
            v-for="t in tabs" :key="t.key"
            class="mfw-tab" :class="{ active: tab === t.key }"
            @click="tab = t.key"
          >{{ t.label }}</button>
        </div>

        <div v-if="loading" class="mfw-empty">加载中…</div>
        <div v-else-if="error" class="mfw-empty err">{{ error }}</div>
        <div v-else-if="!memory.role_id" class="mfw-empty">当前没有已加载角色的记忆。</div>

        <template v-else>
          <!-- 图谱 -->
          <div v-show="tab === 'graph'" class="mfw-body">
            <MemoryGraph v-if="graphReady" :sections="memory" />
          </div>
          <!-- 情绪 -->
          <div v-show="tab === 'emotion'" class="mfw-body scroll">
            <EmotionRadar />
          </div>
          <!-- 类别（长条卡片，点击展开看记忆）-->
          <div v-show="tab === 'category'" class="mfw-body scroll">
            <MemoryCategoryCards :sections="memory" />
          </div>
          <!-- 文本 -->
          <div v-show="tab === 'text'" class="mfw-body scroll">
            <div class="mfg-text">
              <section v-for="sec in textSections" :key="sec.key" class="mfg-sec">
                <div class="mfg-sec-title" :style="{ color: sec.color }">{{ sec.label }}</div>
                <div class="mfg-sec-body">{{ sec.value }}</div>
              </section>
              <div class="mfw-meta">更新于 {{ memory.updated_at || '—' }} · 永久记忆{{ memory.memory_enabled ? '开' : '关' }}</div>
            </div>
          </div>
        </template>

        <button class="mfw-refresh" @click="load">刷新</button>
      </template>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useGameStore } from '@/stores/modules/game'
import { getRoleMemoryBank, type RoleMemoryView } from '@/api/services/memory'
import { useMemoryWidget } from '@/composables/useMemoryWidget'
import MemoryGraph from './MemoryGraph.vue'
import EmotionRadar from './EmotionRadar.vue'
import MemoryCategoryCards from './MemoryCategoryCards.vue'

const { open, close } = useMemoryWidget()
const gameStore = useGameStore()

const pos = reactive({ x: 0, y: 72, w: 360, h: 480 })
const minimized = ref(false)
const fullscreen = ref(false)

function toggleFullscreen() {
  fullscreen.value = !fullscreen.value
  if (!fullscreen.value) {
    // 退出全屏后回到小窗位置
    if (pos.x + pos.w > window.innerWidth) pos.x = Math.max(0, window.innerWidth - pos.w - 16)
    if (pos.y > window.innerHeight - 60) pos.y = 72
  }
}
const tab = ref<'graph' | 'emotion' | 'category' | 'text'>('graph')
const tabs = [
  { key: 'graph' as const, label: '图谱' },
  { key: 'emotion' as const, label: '情绪' },
  { key: 'category' as const, label: '类别' },
  { key: 'text' as const, label: '文本' },
]
const memory = ref<RoleMemoryView>(emptyMemory())
const loading = ref(false)
const error = ref('')
const graphReady = ref(false)

function emptyMemory(): RoleMemoryView {
  return {
    role_id: 0, role_name: '', memory_enabled: false, schema_version: 1,
    updated_at: '', short_term: '', long_term: '', user_info: '', promises: '',
  }
}

const roleName = computed(() => memory.value.role_name)

const textSections = computed(() => [
  { key: 'short_term', label: '📍 近期回顾', color: '#FFB347', value: memory.value.short_term },
  { key: 'long_term', label: '📖 长期经历', color: '#7aa2f7', value: memory.value.long_term },
  { key: 'user_info', label: '🧑 用户信息', color: '#9ece6a', value: memory.value.user_info },
  { key: 'promises', label: '📌 重要约定', color: '#f7768e', value: memory.value.promises },
])

async function load() {
  // 预览/小窗：角色未选时回退到角色 1（浏览器 mock 预览能立即看到图谱）
  const id = gameStore.mainRoleId || gameStore.currentInteractRoleId || 1
  loading.value = true
  error.value = ''
  try {
    memory.value = await getRoleMemoryBank(id)
    graphReady.value = true
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : '读取记忆失败'
  } finally {
    loading.value = false
  }
}

// ── 拖动 ──
let drag = { on: false, ox: 0, oy: 0 }
function initPos() {
  pos.x = Math.max(0, window.innerWidth - pos.w - 16)
  pos.y = 72
}
function startDrag(e: MouseEvent) {
  drag.on = true
  drag.ox = e.clientX - pos.x
  drag.oy = e.clientY - pos.y
  window.addEventListener('mousemove', onMove)
  window.addEventListener('mouseup', onUp)
}
function startDragT(e: TouchEvent) {
  const t = e.touches[0]
  drag.ox = t.clientX - pos.x
  drag.oy = t.clientY - pos.y
  drag.on = true
  window.addEventListener('touchmove', onMoveT, { passive: true })
  window.addEventListener('touchend', onUp)
}
function onMove(e: MouseEvent) {
  if (!drag.on) return
  pos.x = e.clientX - drag.ox
  pos.y = e.clientY - drag.oy
}
function onMoveT(e: TouchEvent) {
  if (!drag.on) return
  const t = e.touches[0]
  pos.x = t.clientX - drag.ox
  pos.y = t.clientY - drag.oy
}
function onUp() {
  drag.on = false
  window.removeEventListener('mousemove', onMove)
  window.removeEventListener('touchmove', onMoveT)
  window.removeEventListener('touchend', onUp)
}

watch(open, (v) => {
  if (v) {
    initPos()
    load()
  }
})

onMounted(() => {
  initPos()
  if (open.value) load()
})
</script>

<style scoped>
.mfw {
  position: fixed;
  z-index: 9999;
  background: #fff;
  border: 1px solid #e3e3e3;
  border-radius: 14px;
  box-shadow: 0 12px 36px rgba(0,0,0,.16);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.mfw-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 9px 12px;
  cursor: grab;
  user-select: none;
  background: #fff;
  border-bottom: 1px solid #f0f0f0;
}
.mfw-title { font-size: 13px; font-weight: 700; color: #111; }
.mfw-ops { display: flex; gap: 2px; }
.mfw-op { background: transparent; border: none; color: #999; cursor: pointer; font-size: 15px; padding: 0 5px; }
.mfw-op:hover { color: #111; }
.mfw-tabs { display: flex; gap: 2px; padding: 6px 8px 0; }
.mfw-tab { background: transparent; border: none; color: #9a9a9a; padding: 6px 14px; cursor: pointer; font-size: 12px; font-weight: 600; border-bottom: 2px solid transparent; }
.mfw-tab.active { color: #111; border-bottom-color: #111; }
.mfw-body { flex: 1; min-height: 0; padding: 8px 10px; background: #fff; }
.mfw-body.scroll { overflow-y: auto; }
.mfw-refresh {
  margin: 8px 12px;
  align-self: flex-start;
  background: #111;
  border: none;
  color: #fff;
  border-radius: 8px;
  padding: 5px 14px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}
.mfw-empty { padding: 24px 12px; color: #999; font-size: 13px; text-align: center; }
.mfw-empty.err { color: #d33; }
.mfg-text { display: flex; flex-direction: column; gap: 12px; }
.mfg-sec-title { font-size: 13px; font-weight: 700; margin-bottom: 4px; color: #111; }
.mfg-sec-body { font-size: 12px; color: #444; line-height: 1.7; white-space: pre-wrap; word-break: break-word; }
.mfw-meta { font-size: 11px; color: #999; margin-top: 8px; }
.mfw.fullscreen {
  left: 0 !important;
  top: 0 !important;
  width: 100vw !important;
  height: 100vh !important;
  border-radius: 0;
  border: none;
}
.mfw.fullscreen .mfw-head { cursor: default; }
</style>
