<template>
  <!--
    「世界模拟」P5-2：**事件流面板**（现实事件引擎的归处 + 三通道开关）

    为什么三通道开关放在这个面板里而不是设置页：
      开关的语义就是「这类事件要不要打扰我」，它跟事件列表是同一件事的两面；
      塞进设置页会让用户在两处之间来回跳。面板本身就是「事件」的归处。

    为什么挂在 `.ws-stage` 里（绝对定位在右下角、`.ws-people` 浮标上方）：
      它属于「世界模拟」这一页的浮层，不是地图的一部分 —— 放在手势变换容器里
      会被地图一起缩放（字会糊）。`data-no-gesture` 是保险：万一将来它被挪进
      地图容器，也不会把点击漏给地图手势。

    文案纪律：模板里**一个中文都不许有**，全部走 `t('worldsim.events.*')`。
  -->
  <div class="ws-ef" data-no-gesture>
    <section class="ws-card ws-ef__panel" :class="{ 'is-open': open }">
      <button class="ws-ef__head" type="button" :aria-expanded="open" @click="toggleOpen">
        <span class="ws-ef__ico" aria-hidden="true">🔔</span>
        <span class="ws-ef__t">{{ t('worldsim.events.title') }}</span>
        <span v-if="events.length" class="ws-tag">{{ events.length }}</span>
        <span class="ws-spacer" />
        <!-- P5-5：fps 只在用户手动开启时出现（默认关），所以这里不是常驻元素 -->
        <span v-if="fpsOn" class="ws-ef__fps" :title="t('worldsim.perf.fpsHint')">{{ fps }} fps</span>
        <span v-if="running" class="ws-ef__dot ws-anim-breathe" aria-hidden="true" />
        <span class="ws-ef__caret" aria-hidden="true">{{ open ? '▾' : '▸' }}</span>
      </button>

      <div v-show="open" class="ws-scroll ws-ef__body">
        <!-- 三通道开关（默认全开；见 useWorldEvents 的 setChannel 注释） -->
        <div class="ws-ef__chs">
          <button
            v-for="c in WS_CHANNEL_KEYS"
            :key="c"
            class="ws-chip"
            :class="{ 'is-on': channels[c] }"
            type="button"
            :aria-pressed="channels[c]"
            :title="t(`worldsim.events.channel.${c}Hint`)"
            @click="emit('toggle', c, !channels[c])"
          >
            <span aria-hidden="true">{{ CHANNEL_ICON[c] }}</span>
            {{ t(`worldsim.events.channel.${c}`) }}
          </button>
          <span class="ws-spacer" />
          <button class="ws-chip" type="button" :title="t('worldsim.events.refresh')" @click="emit('refresh')">
            ⟳
          </button>
        </div>
        <div class="ws-ef__hint">{{ t('worldsim.events.channelHint') }}</div>

        <!-- P5-5：性能档位（自动判定 + 手动覆盖 + fps 显示开关）。
             放这里是因为这块已经是「这一页的设置」的归处（三通道开关也在这儿），
             不用再开一个设置页。 -->
        <div class="ws-ef__perf">
          <span class="ws-ef__label">{{ t('worldsim.perf.title') }}</span>
          <button
            v-for="o in TIER_OPTIONS"
            :key="o"
            class="ws-chip"
            :class="{ 'is-on': tierPref === o }"
            type="button"
            :aria-pressed="tierPref === o"
            :title="t(`worldsim.perf.${o}Hint`)"
            @click="perf.setTier(o)"
          >
            {{ t(`worldsim.perf.${o}`) }}
          </button>
          <span class="ws-spacer" />
          <button
            class="ws-chip"
            :class="{ 'is-on': fpsOn }"
            type="button"
            :aria-pressed="fpsOn"
            :title="t('worldsim.perf.fpsHint')"
            @click="perf.setFps(!fpsOn)"
          >
            {{ t('worldsim.perf.fps') }}
          </button>
        </div>
        <div class="ws-ef__hint">{{ t('worldsim.perf.now', { tier: tierLabel, why: whyText }) }}</div>

        <!-- 事件流（最新在上） -->
        <div v-if="events.length" class="ws-ef__list">
          <article v-for="e in events" :key="eventKeyOf(e)" class="ws-ef__item">
            <span
              class="ws-ef__badge"
              :class="`ws-ef__badge--${categoryOf(e)}`"
              :title="t(categoryMetaOf(categoryOf(e)).labelKey)"
              aria-hidden="true"
              >{{ categoryMetaOf(categoryOf(e)).emoji }}</span
            >
            <div class="ws-ef__main">
              <div class="ws-ef__row">
                <span class="ws-ef__title">{{ e.title }}</span>
                <span class="ws-ef__time">{{ relTime(e) }}</span>
              </div>
              <div class="ws-ef__text">{{ e.text }}</div>
            </div>
          </article>
        </div>

        <!-- 空态（世界很平静的时候也得好看，不许是光秃秃一行字） -->
        <div v-else class="ws-empty ws-empty--big ws-ef__empty">
          <span class="ws-empty__ico" aria-hidden="true">🌤</span>
          <span class="ws-empty__t">{{ t('worldsim.events.empty') }}</span>
          <span class="ws-empty__s">{{ t('worldsim.events.emptySub') }}</span>
        </div>

        <!-- 角色口述（speech 通道）的前端预览：后端已经把事件注进「最近：…」 -->
        <div v-if="speechHint" class="ws-ef__speech">
          <span class="ws-ef__label">{{ t('worldsim.events.speechPreview') }}</span>
          <p class="ws-ef__speechtext">{{ speechHint }}</p>
        </div>

        <!-- 待写记忆（drain 取到的行；取走即清空，所以这里只显示「这一次取到的」） -->
        <section class="ws-ef__mem">
          <div class="ws-ef__memhead">
            <span class="ws-ef__ico" aria-hidden="true">🧠</span>
            <span class="ws-ef__t">{{ t('worldsim.events.memoryTitle') }}</span>
            <span v-if="pending.length" class="ws-tag">{{ pending.length }}</span>
            <span class="ws-spacer" />
            <span v-if="pendingAfter > 0" class="ws-ef__time">{{ t('worldsim.events.memoryAfter', { n: pendingAfter }) }}</span>
          </div>
          <ul v-if="pending.length" class="ws-ef__memlist">
            <li v-for="(m, i) in pending" :key="`${m.at}-${i}`" class="ws-ef__memrow">
              <span v-if="m.role" class="ws-ef__memwho">{{ m.role }}</span>
              <span class="ws-ef__memline">{{ m.line }}</span>
            </li>
          </ul>
          <div v-else class="ws-empty ws-ef__memempty">{{ t('worldsim.events.memoryEmpty') }}</div>
          <div class="ws-ef__hint">{{ t('worldsim.events.memoryHint') }}</div>
        </section>

        <!-- 状态行：如实告诉用户「为什么还没出事」/「下次最早什么时候」 -->
        <div class="ws-ef__state" :class="{ 'is-err': !!error }">{{ stateText }}</div>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  WS_CHANNEL_KEYS,
  categoryMetaOf,
  categoryOf,
  eventKeyOf,
  relTimeOf,
  type WsEventChannels,
  type WsEventItem,
  type WsPendingLine,
} from '@/composables/useWorldEvents'
// P5-5：性能档位（判定/持久化/fps 都在 wsPerf 里；这里只做界面）
import { readPerfEnv, useWsPerf, type PerfTierPref } from './wsPerf'

const props = withDefaults(
  defineProps<{
    /** 事件历史（最新在前） */
    events: WsEventItem[]
    /** 三通道开关（受控：由页面里的 composable 持有） */
    channels: WsEventChannels
    /** 待写记忆（drain 取到的行） */
    pending: WsPendingLine[]
    /** 队列里还剩几行（别人的） */
    pendingAfter?: number
    /** 角色口述提示（`speech` 关掉时是空串） */
    speechHint?: string
    /** 相对时间的参照时钟（跟着 tick 走） */
    nowMs?: number
    /** 上一条 tick 的判定（面板底部的状态行） */
    reason?: string
    nextOkInSecs?: number
    /** 当前环境有没有事件引擎（web 预览 = 没有） */
    supported?: boolean
    /** 轮询在不在跑 */
    running?: boolean
    /** 最近一条错误（有就显示出来，绝不静默） */
    error?: string
    /**
     * 初始是否展开。
     *
     * ⚠️ 2026-09-12 由 `true` 改成 **`false`**（真机截图后定的）：它常驻在小区图右下角，
     * 展开时约占**半个屏幕宽**，会把刚生成好的街区图盖掉一大块 —— 而"刚生成完想看看
     * 街区长什么样"恰恰是玩家最想看地图的时刻。关着只留一个小药丸，点一下才展开；
     * 事件发生时另有地图气泡 / 提示条 / 角色口述三条通道通知，不靠"面板必须敞着"提醒。
     */
    defaultOpen?: boolean
  }>(),
  {
    pendingAfter: 0,
    speechHint: '',
    nowMs: 0,
    reason: '',
    nextOkInSecs: 0,
    supported: true,
    running: false,
    error: '',
    defaultOpen: false,
  },
)

const emit = defineEmits<{
  (e: 'toggle', key: keyof WsEventChannels, on: boolean): void
  /** 展开时通知页面（页面趁机刷新历史 + drain 一次记忆） */
  (e: 'open'): void
  (e: 'refresh'): void
}>()

const { t } = useI18n()

/** 通道按钮的图标（文案走 i18n，图标不进词条：emoji 不是文案） */
const CHANNEL_ICON: Record<keyof WsEventChannels, string> = {
  bubble: '💬',
  popup: '🔔',
  speech: '🗣',
}

const open = ref(!!props.defaultOpen)

function toggleOpen() {
  open.value = !open.value
  if (open.value) emit('open')
}

/** 相对时间：`useWorldEvents.relTimeOf` 只给描述符，文案在这一层过 i18n */
function relTime(e: WsEventItem): string {
  const r = relTimeOf(e.at_secs, props.nowMs || Date.now())
  return t(`worldsim.events.rel.${r.key}`, { n: r.n })
}

/** 底部状态行：为什么还没出事 / 下次最早什么时候（引擎自己节流 300 秒） */
const stateText = computed(() => {
  if (props.error) return t('worldsim.events.state.error', { msg: props.error })
  if (!props.supported) return t('worldsim.events.state.unsupported')
  if (!props.running) return t('worldsim.events.state.idle')
  const r = props.reason || 'no_candidate'
  if (r === 'throttled' && props.nextOkInSecs > 0) {
    return t('worldsim.events.state.throttled', { n: props.nextOkInSecs })
  }
  return t(`worldsim.events.state.${r}`)
})

/* ── P5-5：性能档位（模块级单例，页面与本面板读的是同一份）──────────────── */
const perf = useWsPerf()
const TIER_OPTIONS: PerfTierPref[] = ['auto', 'high', 'low']
const tierPref = computed(() => perf.prefs.value.tier)
const fpsOn = computed(() => perf.prefs.value.fps)
const fps = perf.fps
const tierLabel = computed(() => (perf.low.value ? t('worldsim.perf.tierLow') : t('worldsim.perf.tierHigh')))

/** 「凭什么这么判」——如实列出来（自动档列设备信号，手动档说是你指定的） */
const whyText = computed(() => {
  if (perf.prefs.value.tier !== 'auto') return t('worldsim.perf.whyManual')
  const env = readPerfEnv()
  const parts: string[] = []
  if (env.cores) parts.push(t('worldsim.perf.envCores', { n: env.cores }))
  if (env.memGB) parts.push(t('worldsim.perf.envMem', { n: env.memGB }))
  if (perf.fps.value > 0) parts.push(t('worldsim.perf.envFps', { n: perf.fps.value }))
  return parts.length ? `${t('worldsim.perf.whyAuto')} · ${parts.join(' · ')}` : t('worldsim.perf.envNone')
})
</script>

<style scoped>
.ws-ef {
  position: absolute;
  /* 钉在右下角、`.ws-people` 浮标（bottom:3.7em）上面 —— 与缩放按钮三兄弟叠着排 */
  right: 0.7em;
  bottom: 6.4em;
  width: min(19em, 78vw);
  z-index: 6;
}
.ws-ef__panel {
  display: flex;
  flex-direction: column;
  max-height: min(60vh, 26em);
  overflow: hidden;
}
.ws-ef__head {
  display: flex;
  align-items: center;
  gap: 0.4em;
  width: 100%;
  padding: 0.45em 0.6em;
  border: 0;
  background: none;
  color: var(--ws-fg);
  font: inherit;
  font-weight: 600;
  text-align: left;
  cursor: pointer;
}
.ws-ef__ico {
  font-size: 1.02em;
}
.ws-ef__t {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ws-ef__caret {
  color: var(--ws-fg-dim);
  font-size: 0.9em;
}
/* 心跳指示灯：轮询在跑就呼吸（用的是 worldsim.css 里的公共动画） */
.ws-ef__dot {
  width: 0.5em;
  height: 0.5em;
  border-radius: 50%;
  background: var(--ws-ok);
}
/* P5-5：fps 读数（只在手动开启时渲染）。等宽数字，读数跳动时宽度不抖。 */
.ws-ef__fps {
  flex: none;
  padding: 0 0.35em;
  font-size: 0.8em;
  font-variant-numeric: tabular-nums;
  color: var(--ws-fg-dim);
  border: 1px solid var(--ws-border);
  border-radius: 999px;
}
.ws-ef__perf {
  display: flex;
  align-items: center;
  gap: 0.3em;
  flex-wrap: wrap;
  padding-top: 0.15em;
  border-top: 1px solid var(--ws-border);
}
.ws-ef__body {
  display: flex;
  flex-direction: column;
  gap: 0.45em;
  /* 展开后的内容整体滚动（事件多时不能让「待写记忆 / 状态行」被裁掉）：
     用的是全局的 .ws-scroll（细滚动条 + overscroll-behavior: contain），
     它同时也在 useWorldSimGestures 的免手势名单里。 */
  min-height: 0;
  padding: 0 0.6em 0.6em;
  font-size: 0.92em;
}
.ws-ef__chs {
  display: flex;
  align-items: center;
  gap: 0.3em;
  flex-wrap: wrap;
}
.ws-ef__hint {
  font-size: 0.82em;
  line-height: 1.5;
  color: var(--ws-fg-dim);
}
.ws-ef__list {
  display: flex;
  flex-direction: column;
  gap: 0.35em;
  min-height: 3em;
  padding-right: 0.2em;
}
.ws-ef__item {
  display: flex;
  align-items: flex-start;
  gap: 0.45em;
  padding: 0.35em 0.45em;
  border-radius: var(--ws-radius-sm);
  background: var(--ws-panel-2);
  animation: ws-fade-up 0.24s ease both;
}
/* 类别徽章：底色取主题变量（三套主题 + 深浅都跟着走）。
   先给一档兜底底色，再用 color-mix 叠一层 —— 老 WebView 上没有 color-mix 也不会透明。 */
.ws-ef__badge {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.7em;
  height: 1.7em;
  font-size: 0.95em;
  line-height: 1;
  border-radius: 50%;
  background: var(--ws-primary-soft);
  border: 1px solid var(--ws-border);
}
.ws-ef__badge--weather {
  background: color-mix(in srgb, var(--ws-accent) 26%, transparent);
  border-color: color-mix(in srgb, var(--ws-accent) 55%, transparent);
}
.ws-ef__badge--traffic {
  background: color-mix(in srgb, var(--ws-warn) 26%, transparent);
  border-color: color-mix(in srgb, var(--ws-warn) 55%, transparent);
}
.ws-ef__badge--social {
  background: color-mix(in srgb, var(--ws-primary) 26%, transparent);
  border-color: color-mix(in srgb, var(--ws-primary) 55%, transparent);
}
.ws-ef__badge--work {
  background: color-mix(in srgb, var(--ws-accent) 20%, transparent);
  border-color: color-mix(in srgb, var(--ws-accent) 45%, transparent);
}
.ws-ef__badge--health {
  background: color-mix(in srgb, var(--ws-err) 22%, transparent);
  border-color: color-mix(in srgb, var(--ws-err) 50%, transparent);
}
.ws-ef__badge--money {
  background: color-mix(in srgb, var(--ws-warn) 20%, transparent);
  border-color: color-mix(in srgb, var(--ws-warn) 45%, transparent);
}
.ws-ef__badge--accident {
  background: color-mix(in srgb, var(--ws-err) 28%, transparent);
  border-color: color-mix(in srgb, var(--ws-err) 60%, transparent);
}
.ws-ef__badge--festival {
  background: color-mix(in srgb, var(--ws-accent-2) 28%, transparent);
  border-color: color-mix(in srgb, var(--ws-accent-2) 60%, transparent);
}
.ws-ef__badge--mood {
  background: color-mix(in srgb, var(--ws-primary) 20%, transparent);
  border-color: color-mix(in srgb, var(--ws-primary) 45%, transparent);
}
.ws-ef__badge--luck {
  background: color-mix(in srgb, var(--ws-ok) 26%, transparent);
  border-color: color-mix(in srgb, var(--ws-ok) 55%, transparent);
}
.ws-ef__main {
  min-width: 0;
  flex: 1;
}
.ws-ef__row {
  display: flex;
  align-items: baseline;
  gap: 0.4em;
}
.ws-ef__title {
  min-width: 0;
  flex: 1;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ws-ef__time {
  flex: none;
  font-size: 0.8em;
  color: var(--ws-fg-dim);
}
.ws-ef__text {
  font-size: 0.88em;
  line-height: 1.5;
  color: var(--ws-fg-dim);
  overflow-wrap: anywhere;
}
.ws-ef__empty {
  padding: 0.9em 0.4em;
}
.ws-ef__speech {
  padding: 0.4em 0.5em;
  border-radius: var(--ws-radius-sm);
  background: var(--ws-primary-soft);
}
.ws-ef__label {
  font-size: 0.8em;
  font-weight: 600;
  color: var(--ws-fg);
}
.ws-ef__speechtext {
  margin: 0.2em 0 0;
  font-size: 0.86em;
  line-height: 1.5;
  color: var(--ws-fg-dim);
  overflow-wrap: anywhere;
}
.ws-ef__mem {
  padding: 0.45em 0.5em;
  border: 1px solid var(--ws-border);
  border-radius: var(--ws-radius-sm);
  background: var(--ws-panel-2);
}
.ws-ef__memhead {
  display: flex;
  align-items: center;
  gap: 0.35em;
  font-weight: 600;
}
.ws-ef__memlist {
  margin: 0.3em 0 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 0.2em;
}
.ws-ef__memrow {
  display: flex;
  gap: 0.35em;
  font-size: 0.86em;
  line-height: 1.5;
}
.ws-ef__memwho {
  flex: none;
  color: var(--ws-fg-dim);
}
.ws-ef__memline {
  min-width: 0;
  overflow-wrap: anywhere;
}
.ws-ef__memempty {
  font-size: 0.84em;
  padding: 0.3em 0;
}
.ws-ef__state {
  font-size: 0.8em;
  line-height: 1.5;
  color: var(--ws-fg-dim);
  overflow-wrap: anywhere;
}
.ws-ef__state.is-err {
  color: var(--ws-err);
}
</style>
