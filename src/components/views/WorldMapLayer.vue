<template>
  <!-- 世界地图叠加层（T6-2）：既是半透明背景层，也能缩成角落小窗 -->
  <template v-if="mode !== 'off'">
    <!-- ① 背景层：铺满、半透明、不吃鼠标事件 -->
    <div v-if="mode === 'overlay'" class="wml-overlay" :style="{ opacity }">
      <img v-if="img" class="wml-img" :src="img" alt="" @error="onErr" />
      <div class="wml-veil" />
      <div v-if="loading" class="wml-hint">世界地图加载中…</div>
    </div>

    <!-- ② 角落小窗：可拖动、可点开完整世界页 -->
    <div
      v-else
      class="wml-corner"
      :style="cornerStyle"
      @mousedown="startDrag"
      @touchstart.passive="startDrag"
    >
      <div class="wml-head">
        <span class="wml-title">🧭 {{ mainName }}</span>
        <div class="wml-ops" @mousedown.stop @touchstart.stop>
          <button class="wml-op" title="切成半透明背景层" @click="setMode('overlay')">▤</button>
          <button class="wml-op" title="打开世界地图" @click="openWorld">⛶</button>
          <button class="wml-op" title="关闭" @click="setMode('off')">✕</button>
        </div>
      </div>
      <div class="wml-body">
        <img v-if="img" class="wml-img2" :src="img" alt="" @error="onErr" />
        <div v-if="loading" class="wml-hint sm">加载中…</div>
      </div>
      <div class="wml-foot">
        <span v-if="nowText">{{ nowText }}</span>
        <span v-if="roles.length">{{ roles.length }} 位角色在活动</span>
        <span v-if="remotes.length">远处块 {{ remotes.length }}</span>
      </div>
    </div>
  </template>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useWorldMapLayer } from '@/composables/useWorldMapLayer'
import worldMapApi, { type RemoteBlock, type ScheduleRole } from '@/api/services/worldMap'
import { bindWorldData } from '@/composables/useWorldMapBindings'

const router = useRouter()
const { state, setMode, setPos } = useWorldMapLayer()

const mode = computed(() => state.value.mode)
const opacity = computed(() => state.value.opacity)
const loading = ref(false)
const imgErr = ref(false)
const tick = ref(Date.now())
const mainName = ref('世界地图')
const remotes = ref<RemoteBlock[]>([])
const roles = ref<ScheduleRole[]>([])
const nowText = ref('')
const adcode = ref('')

const img = computed(() => {
  if (imgErr.value || !adcode.value) return ''
  return `${worldMapApi.apiBase}/api/bigmap?ad=${adcode.value}&style=${state.value.style}&scale=1&_t=${tick.value}`
})

const cornerStyle = computed(() => {
  const s: Record<string, string> = {}
  if (state.value.x >= 0 && state.value.y >= 0) {
    s.left = state.value.x + 'px'
    s.top = state.value.y + 'px'
    s.right = 'auto'
    s.bottom = 'auto'
  }
  return s
})

function onErr() {
  imgErr.value = true
}

async function loadRegion() {
  loading.value = true
  imgErr.value = false
  try {
    const d = await worldMapApi.blocks(state.value.adcode || undefined, state.value.style, 6)
    if (d?.ok) {
      adcode.value = d.main.adcode
      mainName.value = d.main.name
      remotes.value = d.remotes || []
    }
  } catch {
    /* 服务没起时保持静默，不打扰聊天 */
  } finally {
    loading.value = false
  }
}

async function loadRoles() {
  try {
    const s = await worldMapApi.schedule()
    roles.value = s.roles || []
  } catch {
    roles.value = []
  }
  try {
    const t: any = await worldMapApi.time()
    const hh = t?.hour ?? t?.clock?.hour
    const mm = t?.minute ?? t?.clock?.minute
    if (hh !== undefined) nowText.value = `🕐 ${String(hh).padStart(2, '0')}:${String(mm ?? 0).padStart(2, '0')}`
  } catch {
    nowText.value = ''
  }
}

function openWorld() {
  router.push('/world')
}

// ── 拖动小窗 ──
let dragging = false
let sx = 0
let sy = 0
let ox = 0
let oy = 0
function startDrag(e: MouseEvent | TouchEvent) {
  const t: any = 'touches' in e ? e.touches[0] : e
  if (!t) return
  const el = (e.currentTarget as HTMLElement).getBoundingClientRect()
  dragging = true
  sx = t.clientX
  sy = t.clientY
  ox = el.left
  oy = el.top
  window.addEventListener('mousemove', onDrag)
  window.addEventListener('mouseup', endDrag)
  window.addEventListener('touchmove', onDrag, { passive: false })
  window.addEventListener('touchend', endDrag)
}
function onDrag(e: MouseEvent | TouchEvent) {
  if (!dragging) return
  const t: any = 'touches' in e ? e.touches[0] : e
  if (!t) return
  const nx = Math.max(0, Math.min(window.innerWidth - 120, ox + (t.clientX - sx)))
  const ny = Math.max(0, Math.min(window.innerHeight - 60, oy + (t.clientY - sy)))
  setPos(nx, ny)
  if ('touches' in e) e.preventDefault()
}
function endDrag() {
  dragging = false
  window.removeEventListener('mousemove', onDrag)
  window.removeEventListener('mouseup', endDrag)
  window.removeEventListener('touchmove', onDrag)
  window.removeEventListener('touchend', endDrag)
}
onBeforeUnmount(endDrag)

let refreshTimer: number | null = null
onMounted(async () => {
  try {
    await bindWorldData()
  } catch {
    /* 静默 */
  }
  await loadRegion()
  await loadRoles()
  // 每 5 分钟刷一次画面（时间/角色位置会变），避免频繁请求
  refreshTimer = window.setInterval(() => {
    if (state.value.mode !== 'off') {
      tick.value = Date.now()
      void loadRoles()
    }
  }, 5 * 60 * 1000)
})
onBeforeUnmount(() => {
  if (refreshTimer !== null) window.clearInterval(refreshTimer)
})
</script>

<style scoped>
/* 背景层：铺满 + 半透明 + 不吃事件 */
.wml-overlay {
  position: fixed;
  inset: 0;
  z-index: 1;
  pointer-events: none;
  overflow: hidden;
  transition: opacity 0.4s ease;
}
.wml-overlay .wml-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  filter: saturate(0.9) contrast(1.05);
}
.wml-veil {
  position: absolute;
  inset: 0;
  background: radial-gradient(circle at 50% 40%, rgba(13, 22, 32, 0.1), rgba(13, 22, 32, 0.75));
}
.wml-hint {
  position: absolute;
  left: 50%;
  bottom: 18px;
  transform: translateX(-50%);
  font-size: 12px;
  color: #9fd6ff;
  background: rgba(13, 22, 32, 0.8);
  border-radius: 10px;
  padding: 5px 12px;
}
.wml-hint.sm {
  bottom: 50%;
  transform: translate(-50%, 50%);
}

/* 角落小窗 */
.wml-corner {
  position: fixed;
  right: 16px;
  bottom: 84px;
  width: 232px;
  z-index: 60;
  background: rgba(13, 22, 32, 0.92);
  border: 1px solid rgba(121, 217, 255, 0.3);
  border-radius: 13px;
  overflow: hidden;
  box-shadow: 0 8px 26px rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(8px);
  cursor: move;
  user-select: none;
}
.wml-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 9px;
  font-size: 12px;
  color: #dff1ff;
  border-bottom: 1px solid rgba(121, 217, 255, 0.18);
}
.wml-title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.wml-ops {
  display: flex;
  gap: 4px;
}
.wml-op {
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(121, 217, 255, 0.2);
  border-radius: 7px;
  color: #cfe6ff;
  font-size: 11px;
  padding: 2px 6px;
  cursor: pointer;
}
.wml-op:hover {
  background: rgba(121, 217, 255, 0.22);
}
.wml-body {
  position: relative;
  aspect-ratio: 4 / 3;
  background: #0a121b;
}
.wml-img2 {
  width: 100%;
  height: 100%;
  object-fit: contain;
  display: block;
}
.wml-foot {
  display: flex;
  gap: 9px;
  padding: 5px 9px;
  font-size: 10.5px;
  color: rgba(180, 205, 235, 0.8);
  flex-wrap: wrap;
}
</style>
