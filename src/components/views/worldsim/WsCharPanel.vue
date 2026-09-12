<template>
  <!--
    P2-2 立绘侧边栏 + P2-3 角色信息面板（7 项）

    为什么两者在**同一个组件**里：
      需求本身就是「点角色 → 从边上滑出立绘，同时能看 7 项信息」。
      拆成两个组件会多出一条「谁是当前角色 / 立绘展开没有」的同步链，
      而那条链一旦不同步，就会出现「面板关了立绘还在加载」这种最要命的性能 bug。

    ⚠️ 性能红线：立绘（3511×5242 webp，解码 ≈73MB）只在 `portraitOpen` 为 true 时
       才由 useWsPortrait 去取，关掉立刻把 src 置空。缩略图是另一张图（头像小方图），
       不碰立绘文件。

    ⚠️ 手势：整个面板挂 `data-no-gesture`（useWorldSimGestures 的 NO_GESTURE_SELECTOR
       认这个属性），并且内部滚动容器自己 stop 掉 pointer/wheel，
       保证在面板上拖动/滚动**不会**把地图拖走或缩放。
  -->
  <div
    class="ws-drawer"
    :class="{ 'is-narrow': narrow }"
    data-no-gesture
    @pointerdown.stop
    @pointermove.stop
    @pointerup.stop
    @wheel.stop
    @dblclick.stop
  >
    <!-- 窄屏：半透明遮罩（点它 = 关闭）；宽屏不要遮罩（抽屉式，背后地图还能看） -->
    <div v-if="narrow" class="ws-drawer__mask" @click="emit('close')" />

    <aside class="ws-drawer__panel" role="dialog" aria-modal="true" :aria-label="panelTitle">
      <header class="ws-drawer__head">
        <button class="ws-btn ws-btn--ghost" type="button" :title="t('worldsim.panel.close')" @click="emit('close')">←</button>
        <div class="ws-drawer__who">
          <div class="ws-drawer__name">{{ actor?.name || '—' }}</div>
          <div v-if="actor?.subtitle" class="ws-drawer__sub">{{ actor.subtitle }}</div>
        </div>
        <span class="ws-spacer" />
        <!-- 情绪：头像与立绘都跟着它走（与聊天里同一张映射表） -->
        <span v-if="actor && !actor.isMe" class="ws-tag">{{ emotionLabel }}</span>
        <span v-if="onStage" class="ws-tag ws-tag--live">{{ t('worldsim.panel.onStage') }}</span>
      </header>

      <div class="ws-drawer__body ws-scroll">
        <!-- ① 立绘 -->
        <section class="ws-sec ws-sec--por">
          <div class="ws-por" :class="{ 'is-loading': pot.loading.value }">
            <img
              v-if="portraitOpen && pot.url.value"
              class="ws-por__img"
              :src="pot.url.value"
              :alt="actor?.name || ''"
              decoding="async"
              @error="onPortraitError"
            />
            <div v-else-if="portraitOpen && pot.loading.value" class="ws-por__state">
              <WsLoading variant="init" size="sm" :text="t('worldsim.panel.portraitLoading')" />
            </div>
            <div v-else-if="portraitOpen" class="ws-por__state">
              <div class="ws-note ws-note--warn">{{ t('worldsim.panel.portraitNone') }}</div>
              <button class="ws-btn" type="button" @click="pot.reload()">{{ t('worldsim.retry') }}</button>
            </div>
            <!-- 未展开：只显示缩略图（绝不加载那张 73MB 的大图） -->
            <button v-else class="ws-por__thumb" type="button" @click="emit('portrait', true)">
              <img v-if="thumbUrl" :src="thumbUrl" :alt="actor?.name || ''" />
              <span v-else class="ws-por__ph">{{ (actor?.name || '?').slice(0, 1) }}</span>
              <span class="ws-por__hint">{{ t('worldsim.panel.portraitExpand') }}</span>
            </button>
            <button
              v-if="portraitOpen"
              class="ws-por__collapse ws-btn ws-btn--ghost"
              type="button"
              @click="emit('portrait', false)"
            >
              {{ t('worldsim.panel.portraitCollapse') }}
            </button>
          </div>

          <!-- 服装变体：角色目录下若有子目录（泳装/…）就在这里切换；取不到只留「默认」 -->
          <div v-if="clothes.length > 1" class="ws-por__clothes">
            <span class="ws-panel__k">{{ t('worldsim.panel.clothes') }}</span>
            <button
              v-for="c in clothes"
              :key="c"
              class="ws-chip"
              :class="{ 'is-on': c === clothesName }"
              type="button"
              @click="clothesName = c"
            >
              {{ c === 'default' ? t('worldsim.panel.clothesDefault') : c }}
            </button>
          </div>
        </section>

        <!-- ② 日程 -->
        <WsCollapse :title="t('worldsim.panel.schedule')" icon="🗓" :count="timeline.length">
          <div v-if="roleSchedule?.now" class="ws-line">
            <span class="ws-panel__k">{{ t('worldsim.panel.scheduleNow') }}</span>
            <span>{{ roleSchedule.now.time }} {{ roleSchedule.now.content || roleSchedule.now.name }}</span>
          </div>
          <div v-if="roleSchedule?.next" class="ws-line">
            <span class="ws-panel__k">{{ t('worldsim.panel.scheduleNext') }}</span>
            <span>{{ roleSchedule.next.time }} {{ roleSchedule.next.content || roleSchedule.next.name }}</span>
          </div>
          <div v-if="timeline.length" class="ws-time">
            <div v-for="(it, i) in timeline" :key="i" class="ws-time__row">
              <span class="ws-time__t">{{ it.time }}</span>
              <span class="ws-time__c">{{ it.content || it.name }}</span>
              <span class="ws-tag">{{ it.kindZh || it.kind }}</span>
            </div>
          </div>
          <div v-else class="ws-empty">{{ t('worldsim.empty.schedule') }}</div>
        </WsCollapse>

        <!-- ③ 位置 -->
        <WsCollapse :title="t('worldsim.panel.location')" icon="📍" :default-open="true">
          <div class="ws-line">
            <span class="ws-panel__k">{{ t('worldsim.panel.area') }}</span>
            <span>{{ areaText || t('worldsim.empty.location') }}</span>
          </div>
          <div class="ws-line">
            <span class="ws-panel__k">{{ t('worldsim.panel.place') }}</span>
            <span>{{ actor?.place || t('worldsim.empty.place') }}</span>
          </div>
          <div class="ws-line">
            <span class="ws-panel__k">{{ t('worldsim.panel.posSource') }}</span>
            <span class="ws-dim">{{ posSourceLabel }}</span>
          </div>
        </WsCollapse>

        <!-- ④ 记忆 -->
        <WsCollapse :title="t('worldsim.panel.memory')" icon="🧠">
          <div v-if="memLoading" class="ws-dim">{{ t('worldsim.loading') }}</div>
          <template v-else-if="mem">
            <div v-if="!mem.memory_enabled" class="ws-note ws-note--warn">{{ t('worldsim.panel.memoryOff') }}</div>
            <div v-for="blk in memBlocks" :key="blk.key" class="ws-mem">
              <div class="ws-mem__t">{{ blk.title }}</div>
              <div class="ws-mem__b ws-scroll" :class="{ 'is-clamped': !expanded[blk.key] }">{{ blk.text }}</div>
              <button
                v-if="blk.text.length > 120"
                class="ws-btn ws-btn--ghost ws-mem__more"
                type="button"
                @click="toggleExpand(blk.key)"
              >
                {{ expanded[blk.key] ? t('worldsim.panel.collapse') : t('worldsim.panel.expand') }}
              </button>
            </div>
            <div v-if="errorText" class="ws-note ws-note--warn">{{ errorText }}</div>
          </template>
          <div v-else class="ws-empty">{{ memoryEmptyText }}</div>
        </WsCollapse>

        <!-- ⑤ 关系 -->
        <WsCollapse :title="t('worldsim.panel.relation')" icon="💗">
          <div v-if="relation" class="ws-rel">
            <div class="ws-rel__bar"><i :style="{ width: relation.pct + '%' }" /></div>
            <div class="ws-rel__k">{{ relation.label }}</div>
            <div class="ws-rel__src">{{ relation.source }}</div>
          </div>
          <div v-else class="ws-empty">{{ t('worldsim.empty.relation') }}</div>
        </WsCollapse>

        <!-- ⑥ 对话 -->
        <section class="ws-sec">
          <button class="ws-btn ws-btn--primary ws-wide" type="button" @click="goChat">
            💬 {{ t('worldsim.panel.goto') }}
          </button>
          <div class="ws-panel__hint">{{ chatHint }}</div>
        </section>

        <!-- ⑦ 快捷动作：先做成按钮 + emit，后端能力后接；点了必须有反应 -->
        <WsCollapse :title="t('worldsim.panel.actions')" icon="⚡" :default-open="true">
          <div class="ws-acts">
            <button class="ws-btn" type="button" @click="quick('hi')">👋 {{ t('worldsim.action.hi') }}</button>
            <button class="ws-btn" type="button" @click="quick('gift')">🎁 {{ t('worldsim.action.gift') }}</button>
            <button class="ws-btn" type="button" @click="quick('outing')">🚶 {{ t('worldsim.action.outing') }}</button>
          </div>
          <div class="ws-panel__hint">{{ t('worldsim.action.hint') }}</div>
        </WsCollapse>
      </div>
    </aside>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import WsCollapse from './WsCollapse.vue'
import WsLoading from './WsLoading.vue'
import { useWsPortrait } from '@/composables/useWsPortrait'
import { emotionFile, type WsActors } from '@/composables/useWsActors'
import type { PlacedActor, ActorPosSource } from './wsActors'
import { wsToast, wsToastSoon } from './wsToast'

const props = withDefaults(
  defineProps<{
    /** 当前面板对着的人 */
    actor: PlacedActor | null
    /** 数据层（记忆/日程/服装/位置来源都从这儿取） */
    data: WsActors
    /** 当前行政区文案（「广州市·越秀区」） */
    areaText?: string
    /** 窄屏（手机）= 全屏 + 遮罩 */
    narrow?: boolean
    /** 面板是否打开（立绘只在打开时才加载） */
    open?: boolean
    /** 立绘是否已展开 */
    portraitOpen?: boolean
    /** 当前正在对话的角色 id（决定「去找他聊聊」是直连还是提醒） */
    currentRoleId?: number
  }>(),
  { areaText: '', narrow: false, open: false, portraitOpen: false, currentRoleId: 0 },
)

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'portrait', v: boolean): void
  (e: 'goto-chat', a: PlacedActor): void
  (e: 'quick', action: string, a: PlacedActor): void
}>()

const { t } = useI18n()

/* ── 立绘（按需加载 + 关闭释放，全在 useWsPortrait 里）────────────────────── */

/** 服装：默认那套；用户切换后重取 */
const clothesName = ref('default')
const clothes = ref<string[]>(['default'])

// 换人 → 服装选择与展开的记忆都要重置（不然会拿上一个人的「泳装」去取新角色的图）
watch(
  () => props.actor?.id,
  () => {
    clothesName.value = 'default'
    clothes.value = ['default']
    expanded.value = {}
    void loadClothes()
  },
)

const pot = useWsPortrait({
  folder: computed(() => props.actor?.folder || ''),
  emotion: computed(() => props.actor?.emotion || ''),
  clothes: clothesName,
  // 红线①：只有「面板打开 + 立绘展开」同时成立才去取图
  open: computed(() => !!props.open && !!props.portraitOpen),
  mapEmotion: emotionFile,
})

// 拿到服装变体清单（拿不到就只剩「默认」，不报错）
async function loadClothes() {
  const folder = props.actor?.folder
  if (!folder) return
  try {
    clothes.value = await props.data.clothesVariants(folder)
  } catch {
    clothes.value = ['default']
  }
}

// 某个变体取不到图 → 自动退回「默认」（需求：取不到就只显示默认，不要报错）
watch(
  () => pot.missingClothes.value,
  (miss) => {
    if (!miss) return
    clothesName.value = 'default'
    wsToast(t('worldsim.panel.clothesMissing'), 'warn')
  },
)

function onPortraitError() {
  wsToast(t('worldsim.panel.portraitFailed'), 'err')
}

/** 缩略图：就用地图上那张头像小方图（不碰立绘文件） */
const thumbUrl = computed(() => props.actor?.avatarUrl || '')

/* ── ② 日程 ──────────────────────────────────────────────────────────────── */
const roleSchedule = computed(() => (props.actor ? props.data.scheduleOf(props.actor.name) : null))
const timeline = computed(() => roleSchedule.value?.timeline || [])

/* ── ③ 位置 ──────────────────────────────────────────────────────────────── */
const posSourceLabel = computed(() => {
  const map: Record<ActorPosSource, string> = {
    runtime: t('worldsim.pos.runtime'),
    schedule: t('worldsim.pos.schedule'),
    scatter: t('worldsim.pos.scatter'),
    me: t('worldsim.pos.me'),
  }
  return map[(props.actor?.posSource || 'scatter') as ActorPosSource] || ''
})

/* ── ④ 记忆 ──────────────────────────────────────────────────────────────── */
const mem = ref<Awaited<ReturnType<WsActors['loadMemory']>>>(null)
const memLoading = ref(false)
const errorText = ref('')
const expanded = ref<Record<string, boolean>>({})

const memBlocks = computed(() => {
  const m = mem.value
  if (!m) return [] as { key: string; title: string; text: string }[]
  return [
    { key: 'short', title: t('worldsim.panel.memShort'), text: String(m.short_term || '').trim() },
    { key: 'long', title: t('worldsim.panel.memLong'), text: String(m.long_term || '').trim() },
    { key: 'user', title: t('worldsim.panel.memUser'), text: String(m.user_info || '').trim() },
    { key: 'promise', title: t('worldsim.panel.memPromise'), text: String(m.promises || '').trim() },
  ].filter((b) => !!b.text)
})

const memoryEmptyText = computed(() =>
  props.actor?.roleId ? t('worldsim.empty.memory') : t('worldsim.panel.memoryNoId'),
)

watch(
  () => [props.actor?.roleId, props.open] as const,
  async ([roleId, isOpen]) => {
    mem.value = null
    errorText.value = ''
    if (!isOpen || !roleId) return
    memLoading.value = true
    try {
      mem.value = await props.data.loadMemory(Number(roleId))
      if (!mem.value) errorText.value = ''
    } finally {
      memLoading.value = false
    }
  },
  { immediate: true },
)

function toggleExpand(k: string) {
  expanded.value = { ...expanded.value, [k]: !expanded.value[k] }
}

/* ── ⑤ 关系（后端没有好感度接口 → 从记忆里**如实推断**，并标明来源）──────── */
const relation = computed(() => {
  const m = mem.value
  if (!m) return null
  const text = `${m.user_info || ''}\n${m.short_term || ''}\n${m.long_term || ''}`
  if (!text.replace(/\s/g, '')) return null
  const POS = ['喜欢', '爱', '亲近', '信任', '开心', '温柔', '依赖', '想你', '关心', '亲密', '好感', '宠']
  const NEG = ['讨厌', '生气', '吵架', '误会', '冷淡', '疏远', '不满', '失望', '害怕你', '抱歉']
  let score = 50
  for (const w of POS) score += Math.min(3, text.split(w).length - 1) * 3
  for (const w of NEG) score -= Math.min(3, text.split(w).length - 1) * 3
  if (m.promises && String(m.promises).trim()) score += 6 // 有约定 = 关系更近一步
  const pct = Math.max(5, Math.min(100, score))
  const label =
    pct >= 85
      ? t('worldsim.rel.deep')
      : pct >= 70
        ? t('worldsim.rel.good')
        : pct >= 55
          ? t('worldsim.rel.normal')
          : pct >= 40
            ? t('worldsim.rel.cool')
            : t('worldsim.rel.tense')
  return { pct, label, source: t('worldsim.rel.fromMemory') }
})

/* ── ⑥ 对话 ──────────────────────────────────────────────────────────────── */
const onStage = computed(() => !!props.actor && !props.actor.isMe && props.actor.roleId > 0 && props.actor.roleId === props.currentRoleId)
const chatHint = computed(() =>
  onStage.value ? t('worldsim.panel.gotoNow') : t('worldsim.panel.gotoWarn'),
)
function goChat() {
  if (!props.actor) return
  emit('goto-chat', props.actor)
}

/* ── ⑦ 快捷动作（先接到 emit；没有后端能力时**必须**有可见反应）──────────── */
function quick(action: string) {
  if (!props.actor) return
  emit('quick', action, props.actor)
  const label =
    action === 'hi' ? t('worldsim.action.hi') : action === 'gift' ? t('worldsim.action.gift') : t('worldsim.action.outing')
  wsToastSoon(`${props.actor.name} · ${label}`, t('worldsim.soon'))
}

const emotionLabel = computed(() => {
  const e = String(props.actor?.emotion || '').trim()
  return e || t('worldsim.emotionNormal')
})

const panelTitle = computed(() => props.actor?.name || t('worldsim.panel.title'))
</script>

<style scoped>
.ws-drawer {
  position: absolute;
  inset: 0;
  z-index: 30;
}
.ws-drawer__mask {
  position: absolute;
  inset: 0;
  background: rgba(20, 30, 32, 0.42);
  backdrop-filter: blur(1px);
  -webkit-backdrop-filter: blur(1px);
  animation: ws-fade-in 0.18s ease both;
}
/* 宽屏：右侧抽屉（不遮地图，方便边看边点别人） */
.ws-drawer__panel {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
  width: min(24em, 42vw);
  display: flex;
  flex-direction: column;
  background: var(--ws-panel);
  border-left: 1px solid var(--ws-border);
  box-shadow: var(--ws-shadow-lg);
  backdrop-filter: blur(var(--ws-blur));
  -webkit-backdrop-filter: blur(var(--ws-blur));
  animation: ws-slide-in 0.24s cubic-bezier(0.22, 0.61, 0.36, 1) both;
}
/* 窄屏：全屏（盖住地图，避免小手势区里再叠一层可滚动面板） */
.ws-drawer.is-narrow .ws-drawer__panel {
  width: 100%;
  border-left: 0;
}
.ws-drawer__head {
  display: flex;
  align-items: center;
  gap: 0.5em;
  padding: 0.6em 0.8em;
  border-bottom: 1px solid var(--ws-border);
}
.ws-drawer__who {
  min-width: 0;
}
.ws-drawer__name {
  font-weight: 700;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ws-drawer__sub {
  font-size: 0.8em;
  color: var(--ws-fg-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ws-drawer__body {
  flex: 1;
  min-height: 0;
  padding: 0.7em 0.8em 1.2em;
  display: flex;
  flex-direction: column;
  gap: 0.7em;
  /* 面板里滚动到底不该带动背后的地图 */
  overscroll-behavior: contain;
}
.ws-sec {
  display: flex;
  flex-direction: column;
  gap: 0.5em;
}
.ws-panel__k {
  display: inline-block;
  min-width: 4.2em;
  color: var(--ws-fg-dim);
  font-size: 0.88em;
}
.ws-panel__hint {
  font-size: 0.8em;
  color: var(--ws-fg-dim);
  line-height: 1.6;
}
.ws-line {
  display: flex;
  gap: 0.4em;
  align-items: baseline;
  font-size: 0.92em;
  line-height: 1.6;
}
.ws-dim {
  color: var(--ws-fg-dim);
}
.ws-wide {
  width: 100%;
}

/* ── 立绘 ─────────────────────────────────────────────────────────────── */
.ws-sec--por {
  gap: 0.4em;
}
.ws-por {
  position: relative;
  min-height: 12em;
  border-radius: var(--ws-radius);
  overflow: hidden;
  background: var(--ws-bg-2);
  border: 1px solid var(--ws-border);
}
.ws-por__img {
  display: block;
  width: 100%;
  height: 100%;
  max-height: 62vh;
  object-fit: contain;
}
.ws-por__state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.6em;
  min-height: 12em;
  padding: 0.8em;
  text-align: center;
}
.ws-por__thumb {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 0.5em;
  width: 100%;
  min-height: 12em;
  border: 0;
  background: none;
  color: var(--ws-fg);
  font: inherit;
  cursor: pointer;
}
.ws-por__thumb img {
  width: 7.5em;
  height: 7.5em;
  object-fit: cover;
  border-radius: 50%;
  border: 2px solid var(--ws-primary);
}
.ws-por__ph {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 7.5em;
  height: 7.5em;
  border-radius: 50%;
  font-size: 2em;
  background: var(--ws-primary-soft);
}
.ws-por__hint {
  font-size: 0.82em;
  color: var(--ws-fg-dim);
}
.ws-por__collapse {
  position: absolute;
  right: 0.5em;
  top: 0.5em;
}
.ws-por__clothes {
  display: flex;
  align-items: center;
  gap: 0.35em;
  flex-wrap: wrap;
}

/* ── 日程 ─────────────────────────────────────────────────────────────── */
.ws-time {
  display: flex;
  flex-direction: column;
  gap: 0.2em;
  max-height: 12em;
  overflow-y: auto;
  overscroll-behavior: contain;
}
.ws-time__row {
  display: flex;
  align-items: baseline;
  gap: 0.45em;
  font-size: 0.88em;
  line-height: 1.7;
}
.ws-time__t {
  min-width: 3.4em;
  color: var(--ws-fg-dim);
  font-variant-numeric: tabular-nums;
}
.ws-time__c {
  flex: 1;
  min-width: 0;
}

/* ── 记忆 ─────────────────────────────────────────────────────────────── */
.ws-mem {
  margin-top: 0.5em;
}
.ws-mem__t {
  font-size: 0.86em;
  font-weight: 600;
  color: var(--ws-fg-dim);
}
.ws-mem__b {
  margin-top: 0.2em;
  padding: 0.5em 0.6em;
  font-size: 0.88em;
  line-height: 1.7;
  white-space: pre-wrap;
  word-break: break-word;
  background: var(--ws-panel-2);
  border-radius: var(--ws-radius-sm);
  max-height: 14em;
}
/* 折叠：只显示前几行（滚动条 + 折叠，两个要求都要满足） */
.ws-mem__b.is-clamped {
  max-height: 5.4em;
  overflow: hidden;
  mask-image: linear-gradient(180deg, #000 62%, transparent 100%);
  -webkit-mask-image: linear-gradient(180deg, #000 62%, transparent 100%);
}
.ws-mem__more {
  margin-top: 0.25em;
}

/* ── 关系 ─────────────────────────────────────────────────────────────── */
.ws-rel__bar {
  height: 0.5em;
  border-radius: 999px;
  background: var(--ws-primary-soft);
  overflow: hidden;
}
.ws-rel__bar i {
  display: block;
  height: 100%;
  border-radius: 999px;
  background: linear-gradient(90deg, var(--ws-accent), var(--ws-accent-2));
}
.ws-rel__k {
  margin-top: 0.35em;
  font-weight: 600;
}
.ws-rel__src {
  font-size: 0.78em;
  color: var(--ws-fg-dim);
}

/* ── 快捷动作 ─────────────────────────────────────────────────────────── */
.ws-acts {
  display: flex;
  gap: 0.4em;
  flex-wrap: wrap;
}
</style>
