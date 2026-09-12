<template>
  <!--
    P2-4 自己的面板（5 项）—— 与角色面板同一套外壳/皮肤/手势纪律。
    1 我的头像（可上传/更换）  2 我的位置  3 当前时间 + 天气
    4 众人小地图（点谁选谁）   5 我的日程 / 待办（空态也要好看）
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
    <div v-if="narrow" class="ws-drawer__mask" @click="emit('close')" />

    <aside class="ws-drawer__panel" role="dialog" aria-modal="true" :aria-label="t('worldsim.me.title')">
      <header class="ws-drawer__head">
        <button class="ws-btn ws-btn--ghost" type="button" :title="t('worldsim.panel.close')" @click="emit('close')">←</button>
        <div class="ws-drawer__who">
          <div class="ws-drawer__name">{{ meName || t('worldsim.actor.me') }}</div>
          <div class="ws-drawer__sub">{{ t('worldsim.me.title') }}</div>
        </div>
        <span class="ws-spacer" />
        <span class="ws-tag">{{ clockShort }}</span>
      </header>

      <div class="ws-drawer__body ws-scroll">
        <!-- ① 我的头像 -->
        <WsCollapse :title="t('worldsim.me.avatar')" icon="🙂" :default-open="true">
          <div class="ws-me__av">
            <div class="ws-me__pic">
              <img v-if="data.meAvatarUrl.value" :src="data.meAvatarUrl.value" :alt="t('worldsim.me.avatar')" />
              <span v-else class="ws-me__ph" aria-hidden="true">🙂</span>
            </div>
            <div class="ws-me__ops">
              <button class="ws-btn ws-btn--primary" type="button" @click="pickFile">
                {{ data.meAvatar.value.value ? t('worldsim.me.avatarChange') : t('worldsim.me.avatarUpload') }}
              </button>
              <button v-if="data.meAvatar.value.value" class="ws-btn ws-btn--ghost" type="button" @click="resetAvatar">
                {{ t('worldsim.me.avatarReset') }}
              </button>
            </div>
          </div>
          <div class="ws-panel__hint">{{ t('worldsim.me.avatarHint') }}</div>
          <!-- 隐藏的文件选择：不引第三方上传组件（项目里那份 useImageSourcePicker 是给
               「截图/相册」全流程用的，为一枚头像拖进它不划算）；选完立刻压到 256px 再存。 -->
          <input
            ref="fileEl"
            class="ws-me__file"
            type="file"
            accept="image/*"
            :aria-label="t('worldsim.me.avatar')"
            @change="onFile"
          />
        </WsCollapse>

        <!-- ② 我的位置 -->
        <WsCollapse :title="t('worldsim.me.location')" icon="📍" :default-open="true">
          <div class="ws-line">
            <span class="ws-panel__k">{{ t('worldsim.panel.area') }}</span>
            <span>{{ areaText || t('worldsim.empty.location') }}</span>
          </div>
          <div class="ws-line">
            <span class="ws-panel__k">{{ t('worldsim.panel.place') }}</span>
            <span>{{ me?.place || t('worldsim.empty.place') }}</span>
          </div>
        </WsCollapse>

        <!-- ③ 时间 + 天气 -->
        <WsCollapse :title="t('worldsim.me.timeWeather')" icon="🌤" :default-open="true">
          <div class="ws-line">
            <span class="ws-panel__k">{{ t('worldsim.me.time') }}</span>
            <span>{{ data.nowText.value || clockShort }}</span>
          </div>
          <div class="ws-line">
            <span class="ws-panel__k">{{ t('worldsim.me.weather') }}</span>
            <span>{{ data.weatherText.value || t('worldsim.empty.weather') }}</span>
          </div>
          <button class="ws-btn ws-btn--ghost" type="button" @click="emit('refresh')">{{ t('worldsim.refresh') }}</button>
        </WsCollapse>

        <!-- ④ 众人小地图（点谁就选中谁） -->
        <WsCollapse :title="t('worldsim.me.minimap')" icon="🗺" :default-open="true" :count="placed.length">
          <div class="ws-mini" data-no-gesture>
            <WsAvatarLayer
              class="ws-mini__layer"
              :placed="placed"
              :grid="grid"
              size="mini"
              :selected-id="selectedId"
              :me-name="meName"
              @pick="(a) => emit('pick', a)"
            />
          </div>
          <div class="ws-panel__hint">{{ t('worldsim.me.minimapHint') }}</div>
          <div class="ws-mini__list ws-scroll">
            <button
              v-for="a in placed"
              :key="a.id"
              class="ws-mini__row"
              :class="{ 'is-on': a.id === selectedId }"
              type="button"
              @click="emit('pick', a)"
            >
              <span class="ws-mini__dot" :class="{ 'is-me': a.isMe }" />
              <span class="ws-mini__n">{{ a.isMe ? meName || t('worldsim.actor.me') : a.name }}</span>
              <span class="ws-mini__p">{{ a.place || a.nowText || '—' }}</span>
            </button>
            <div v-if="!placed.length" class="ws-empty">{{ t('worldsim.empty.actors') }}</div>
          </div>
        </WsCollapse>

        <!-- ⑤ 我的日程 / 待办 -->
        <WsCollapse :title="t('worldsim.me.todo')" icon="✅" :count="todos.length">
          <div v-if="todos.length" class="ws-todo">
            <div v-for="(x, i) in todos" :key="i" class="ws-todo__row" :class="{ 'is-done': x.done }">
              <span class="ws-todo__box" aria-hidden="true">{{ x.done ? '✓' : '' }}</span>
              <span class="ws-todo__t">{{ x.title }}</span>
              <span v-if="x.time" class="ws-tag">{{ x.time }}</span>
            </div>
          </div>
          <!-- 空态要好看：给个插画感的图标 + 一句安慰 + 一个可做的动作 -->
          <div v-else class="ws-empty ws-empty--big">
            <div class="ws-empty__ico" aria-hidden="true">🍃</div>
            <div class="ws-empty__t">{{ t('worldsim.empty.todo') }}</div>
            <div class="ws-empty__s">{{ t('worldsim.empty.todoSub') }}</div>
          </div>
        </WsCollapse>
      </div>
    </aside>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import WsAvatarLayer from './WsAvatarLayer.vue'
import WsCollapse from './WsCollapse.vue'
import { shrinkImageToDataUrl, type PlacedActor } from './wsActors'
import type { WsActors } from '@/composables/useWsActors'
import { wsToast } from './wsToast'

const props = withDefaults(
  defineProps<{
    data: WsActors
    placed: PlacedActor[]
    grid?: number
    areaText?: string
    narrow?: boolean
    open?: boolean
    selectedId?: string
    meName?: string
  }>(),
  { grid: 28, areaText: '', narrow: false, open: false, selectedId: '', meName: '' },
)

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'pick', a: PlacedActor): void
  (e: 'refresh'): void
}>()

const { t } = useI18n()

const me = computed(() => props.placed.find((a) => a.isMe) || null)
const fileEl = ref<HTMLInputElement | null>(null)

const clockShort = computed(() => props.data.nowText.value || '')
const todos = computed(() => {
  const raw = (props.data.schedule?.value?.todos || []) as Record<string, unknown>[]
  return raw
    .map((x) => ({
      title: String(x?.text ?? x?.content ?? x?.title ?? '').trim(),
      time: String(x?.deadline ?? x?.time ?? '').trim(),
      done: !!(x?.completed ?? x?.done),
    }))
    .filter((x) => !!x.title)
})

/* ── 头像上传 ─────────────────────────────────────────────────────────── */

function pickFile() {
  fileEl.value?.click()
}

async function onFile(e: Event) {
  const el = e.target as HTMLInputElement
  const f = el.files?.[0]
  el.value = '' // 允许连续选同一张图（不清空的话第二次不会触发 change）
  if (!f) return
  try {
    // 先压到 256px 再进 localStorage：手机原图直接塞会把站点存储写爆
    const dataUrl = await shrinkImageToDataUrl(f, 256, 0.82)
    const r = props.data.setMeAvatarData(dataUrl)
    if (!r.ok) {
      wsToast(r.reason === 'toolarge' ? t('worldsim.me.avatarTooLarge') : t('worldsim.me.avatarBad'), 'err')
      return
    }
    wsToast(t('worldsim.me.avatarSaved'), 'ok')
  } catch (err) {
    // 压缩失败（老 WebView 没 canvas / 格式怪）→ 说清楚，绝不静默
    wsToast(`${t('worldsim.me.avatarBad')}：${err instanceof Error ? err.message : String(err)}`, 'err')
  }
}

function resetAvatar() {
  props.data.clearMeAvatar()
  wsToast(t('worldsim.me.avatarResetDone'), 'ok')
}
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
  animation: ws-fade-in 0.18s ease both;
}
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
}
.ws-drawer__sub {
  font-size: 0.8em;
  color: var(--ws-fg-dim);
}
.ws-drawer__body {
  flex: 1;
  min-height: 0;
  padding: 0.7em 0.8em 1.2em;
  display: flex;
  flex-direction: column;
  gap: 0.7em;
  overscroll-behavior: contain;
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

/* ── 头像 ─────────────────────────────────────────────────────────────── */
.ws-me__av {
  display: flex;
  align-items: center;
  gap: 0.8em;
}
.ws-me__pic {
  width: 4.6em;
  height: 4.6em;
  flex: 0 0 auto;
  border-radius: 50%;
  overflow: hidden;
  border: 3px solid var(--ws-accent-2);
  background: var(--ws-panel-2);
  display: flex;
  align-items: center;
  justify-content: center;
}
.ws-me__pic img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.ws-me__ph {
  font-size: 1.8em;
}
.ws-me__ops {
  display: flex;
  flex-direction: column;
  gap: 0.35em;
  align-items: flex-start;
}
.ws-me__file {
  display: none;
}

/* ── 小地图 ───────────────────────────────────────────────────────────── */
.ws-mini {
  position: relative;
  width: 100%;
  aspect-ratio: 1 / 1;
  border-radius: var(--ws-radius);
  border: 1px solid var(--ws-border);
  background: var(--ws-stage-bg);
  overflow: hidden;
}
/* 小地图是「缩略总览」：点谁选谁，绝不参与地图手势 */
.ws-mini__layer {
  position: absolute;
  inset: 0;
}
.ws-mini__list {
  margin-top: 0.4em;
  max-height: 11em;
  display: flex;
  flex-direction: column;
  gap: 0.15em;
}
.ws-mini__row {
  display: flex;
  align-items: center;
  gap: 0.45em;
  width: 100%;
  padding: 0.3em 0.4em;
  border: 0;
  border-radius: var(--ws-radius-sm);
  background: none;
  color: var(--ws-fg);
  font: inherit;
  font-size: 0.9em;
  text-align: left;
  cursor: pointer;
}
.ws-mini__row.is-on {
  background: var(--ws-primary-soft);
}
.ws-mini__dot {
  width: 0.55em;
  height: 0.55em;
  flex: 0 0 auto;
  border-radius: 50%;
  background: var(--ws-primary);
}
.ws-mini__dot.is-me {
  background: var(--ws-accent-2);
}
.ws-mini__n {
  font-weight: 600;
  max-width: 7em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ws-mini__p {
  flex: 1;
  min-width: 0;
  color: var(--ws-fg-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ── 待办 ─────────────────────────────────────────────────────────────── */
.ws-todo {
  display: flex;
  flex-direction: column;
  gap: 0.2em;
}
.ws-todo__row {
  display: flex;
  align-items: center;
  gap: 0.45em;
  font-size: 0.9em;
  line-height: 1.7;
}
.ws-todo__row.is-done {
  color: var(--ws-fg-dim);
  text-decoration: line-through;
}
.ws-todo__box {
  width: 1em;
  height: 1em;
  flex: 0 0 auto;
  border: 1px solid var(--ws-border);
  border-radius: 0.28em;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 0.8em;
  color: var(--ws-primary-deep);
}
.ws-todo__t {
  flex: 1;
  min-width: 0;
}
</style>
