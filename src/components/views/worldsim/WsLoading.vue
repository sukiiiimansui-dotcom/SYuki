<template>
  <!--
    加载指示器（纯 CSS/SVG，不引第三方库）

    它现在管的不只是「转个圈」，而是**按等待时长分档**：
      · < delay（默认 180ms）→ 什么都不显示（一闪而过的指示器比不显示更糟，还会闪一下）
      · 180ms ~ 1s           → 轻提示：安静的小插画，**不转**、不给进度
      · 1s ~ 10s             → 明确的加载态 + 能算就给确定性进度条（算不出来就走不确定态）
      · > 10s（长任务）      → 加上「已等多久 / 约还需多久（估算）/ 速率」+ 安心话 + 停止按钮

    分档阈值与预估规则在 `wsLoadPlan.ts`（纯函数、有单测），这里只负责画。
    五个变体各有一套小插画：init 展开世界 / locate 雷达定位 / map 等高线扩开 /
    draw 画笔描线 / list 骨架屏（结构已知的列表，比如三级联动那三列）。
    配色一律走 --ws-* 变量（细线用 --ws-primary-ink：主色当线条时对比度只有 1.6~2.1）。
  -->
  <div
    class="ws-loading"
    :class="rootClass"
    role="status"
    aria-live="polite"
    :aria-busy="tier !== 'idle'"
  >
    <!-- ⑤ 骨架屏：结构已知 → 先占位（形状照抄真内容，内容回来不跳版） -->
    <template v-if="isSkeleton">
      <div class="ws-loading__sk" aria-hidden="true">
        <div v-for="i in skRows" :key="i" class="ws-sk ws-sk--row" :style="{ '--ws-i': String(i - 1) }">
          <span class="ws-sk__a" :style="{ '--ws-w': String(skWidth(i - 1)) }" />
          <span class="ws-sk__b" />
        </div>
      </div>
      <span class="ws-loading__sr">{{ srText }}</span>
    </template>

    <template v-else>
      <svg class="ws-loading__art" viewBox="0 0 100 100" role="img" :aria-label="srText">
        <!-- ① 初始化：一张正在展开的世界（外圈经线 + 内圈纬线反向转） -->
        <template v-if="variant === 'init'">
          <circle cx="50" cy="50" r="30" fill="none" stroke="var(--ws-primary-ink, var(--ws-primary))" stroke-width="2.4" opacity="0.55" />
          <ellipse cx="50" cy="50" rx="30" ry="12" fill="none" stroke="var(--ws-primary-ink, var(--ws-primary))" stroke-width="2" opacity="0.75" class="ws-anim-spin" />
          <ellipse cx="50" cy="50" rx="12" ry="30" fill="none" stroke="var(--ws-accent)" stroke-width="2" opacity="0.75" class="ws-anim-spin-rev" />
          <circle cx="50" cy="50" r="4.6" fill="var(--ws-primary-deep)" />
          <path d="M20 84 Q50 70 80 84" fill="none" stroke="var(--ws-accent)" stroke-width="2.2" stroke-linecap="round" opacity="0.7" />
        </template>

        <!-- ② 定位：雷达脉冲 + 定位针 -->
        <template v-else-if="variant === 'locate'">
          <circle class="ws-anim-radar" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-primary-ink, var(--ws-primary))" stroke-width="2" />
          <circle class="ws-anim-radar" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-accent)" stroke-width="2" style="animation-delay: 0.7s" />
          <circle class="ws-anim-radar" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-primary-ink, var(--ws-primary))" stroke-width="2" style="animation-delay: 1.4s" />
          <circle cx="50" cy="50" r="5" fill="var(--ws-primary)" />
          <path
            d="M50 26 c-7.5 0-13 5.6-13 13 0 9.5 13 25 13 25 s13-15.5 13-25 c0-7.4-5.5-13-13-13z"
            fill="var(--ws-primary-deep)"
            opacity="0.92"
          />
          <circle cx="50" cy="39.5" r="4.6" fill="var(--ws-bg)" />
        </template>

        <!-- ③ 地图加载 / 区域切换：等高线一圈圈扩开 -->
        <template v-else-if="variant === 'map'">
          <path d="M18 30 L38 22 L62 30 L82 22 L82 70 L62 78 L38 70 L18 78 Z" fill="none" stroke="var(--ws-accent)" stroke-width="2.2" stroke-linejoin="round" opacity="0.7" />
          <path d="M38 22 L38 70 M62 30 L62 78" fill="none" stroke="var(--ws-accent)" stroke-width="1.6" opacity="0.45" />
          <circle class="ws-anim-ring" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-primary-ink, var(--ws-primary))" stroke-width="2.4" />
          <circle class="ws-anim-ring" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-primary-ink, var(--ws-primary))" stroke-width="2.4" style="animation-delay: 0.8s" />
          <circle class="ws-anim-ring" cx="50" cy="50" r="34" fill="none" stroke="var(--ws-primary-ink, var(--ws-primary))" stroke-width="2.4" style="animation-delay: 1.6s" />
        </template>

        <!-- ④ AI 绘制：一支笔把虚线描成实线（长任务；进度/阶段/停止由下面那块给） -->
        <template v-else>
          <path
            d="M22 74 C34 40 48 88 60 46 C66 27 74 34 80 28"
            fill="none"
            stroke="var(--ws-primary-ink, var(--ws-primary))"
            stroke-width="2.6"
            stroke-linecap="round"
            stroke-dasharray="120"
            class="ws-anim-draw"
          />
          <path d="M22 74 L46 84 L78 82" fill="none" stroke="var(--ws-accent)" stroke-width="1.8" opacity="0.5" stroke-dasharray="4 5" />
          <circle cx="80" cy="28" r="4.2" fill="var(--ws-primary-deep)" class="ws-anim-float" />
        </template>
      </svg>

      <div v-if="text || sub" class="ws-loading__body">
        <!-- 文案自带省略号时不再叠动画点（否则会出现「… .」两个点） -->
        <div v-if="text" class="ws-loading__text">
          <span :class="{ 'ws-dots': !/…$/.test(text) }">{{ text }}</span>
        </div>
        <div v-if="sub" class="ws-loading__sub">{{ sub }}</div>
      </div>

      <!-- 附加信息：进度条 / 阶段条 / 已等与预估 / 安心话 / 停止。
           整块**始终渲染**（当这一档用得上时）—— 让高度先占住，出现时不会把内容顶一下。 -->
      <div v-if="hasExtra" class="ws-loading__extra">
        <ol v-if="stageList.length >= 2" class="ws-loading__stages">
          <li
            v-for="(s, i) in stageList"
            :key="s"
            class="ws-loading__stage"
            :class="i < stageIndex ? 'is-done' : i === stageIndex ? 'is-now' : 'is-todo'"
          >
            {{ s }}
          </li>
        </ol>

        <div v-if="barEnabled" class="ws-loading__bar" :class="{ 'is-indet': ratio === null, 'is-hidden': tier === 'brief' }">
          <i v-if="ratio !== null" :style="{ '--ws-prog': String(ratio) }" />
          <i v-else />
        </div>

        <div v-if="metaReady" class="ws-loading__meta">
          <span v-for="(m, i) in metaList" :key="i">{{ m }}</span>
        </div>

        <p v-if="hintText" class="ws-loading__hint">{{ hintText }}</p>

        <div v-if="cancellable" class="ws-loading__ops">
          <button class="ws-btn ws-loading__cancel" type="button" @click="emit('cancel')">{{ cancelLabel }}</button>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
// 动画/骨架/进度条的样式都在这个文件里（跨组件复用，且必须全局 —— 低端机档位的
// 覆盖规则要能选中 `.ws-root.ws-perf-low` 这个**组件外面**的祖先）
import '@/assets/styles/worldsim-loading.css'
import { LOADING_TIERS, skeletonWidth, tierOf, type LoadingTier } from './wsLoadPlan'

const props = withDefaults(
  defineProps<{
    /** 场景：初始化 / 定位 / 地图 / AI 绘制 / 骨架列表 */
    variant?: 'init' | 'locate' | 'map' | 'draw' | 'list'
    text?: string
    sub?: string
    /** 横排（放在进度卡、按钮旁边那种小尺寸） */
    inline?: boolean
    size?: 'sm' | 'md'
    /** 显示延迟（毫秒）：没到点什么都不显示。不传 = 骨架 0、其它 180（见 wsLoadPlan） */
    delay?: number
    /** 进度：数字 = 确定态；null = **明确的不确定态**（算不出来就别装确定）；不传 = 不显示进度条 */
    progress?: number | null
    /** 预估剩余毫秒（null = 还估不准，只在 >10s 那一档显示，且一定写明「估算」） */
    etaMs?: number | null
    /** 速率（项/秒），只在 >10s 那一档显示 */
    rate?: number | null
    /** 阶段名（≥2 个才显示阶段条） */
    stages?: string[]
    /** 当前阶段下标（0 起） */
    stage?: number
    /** 可取消：长任务必须给出口，且按钮**始终可见可点** */
    cancellable?: boolean
    cancelText?: string
    /** 安心话，只在 >10s 那一档显示。**必须如实**：切走会中断就别写「回来还能接着看」 */
    hint?: string
    /** 骨架行数（list 变体） */
    rows?: number
    /** 是否显示「已等 N 秒」（长任务默认显示） */
    showElapsed?: boolean
  }>(),
  {
    variant: 'map',
    text: '',
    sub: '',
    inline: false,
    size: 'md',
    etaMs: null,
    rate: null,
    stages: () => [],
    stage: 0,
    cancellable: false,
    cancelText: '',
    hint: '',
    rows: 6,
    showElapsed: true,
  },
)

const emit = defineEmits<{ (e: 'cancel'): void }>()
const { t } = useI18n()

/** 骨架屏必须**立刻**出现（它负责占位，晚出现就等于跳版），所以它的默认延迟是 0 */
const isSkeleton = computed(() => props.variant === 'list')
const delayMs = computed(() => {
  const d = props.delay
  if (typeof d === 'number' && Number.isFinite(d) && d >= 0) return d
  return isSkeleton.value ? 0 : LOADING_TIERS.delayMs
})

/* 计时：只在**分档边界**上定三个闹钟，到点才起一个 500ms 的心跳。
 * 不用常驻 rAF/interval：一个「还没到显示时间」的指示器不该占任何帧。 */
const elapsed = ref(0)
let startedAt = 0
let ticker: number | null = null
const marks: number[] = []

onMounted(() => {
  startedAt = Date.now()
  const step = () => {
    elapsed.value = Date.now() - startedAt
  }
  marks.push(window.setTimeout(step, delayMs.value))
  marks.push(window.setTimeout(step, LOADING_TIERS.briefMs))
  marks.push(window.setTimeout(step, LOADING_TIERS.longMs))
  marks.push(
    window.setTimeout(
      () => {
        step()
        ticker = window.setInterval(step, 500)
      },
      Math.max(0, delayMs.value),
    ),
  )
})
onBeforeUnmount(() => {
  for (const m of marks) window.clearTimeout(m)
  if (ticker !== null) window.clearInterval(ticker)
})

const tier = computed<LoadingTier>(() => tierOf(elapsed.value, delayMs.value))

const srText = computed(() => props.text || t('worldsim.loader.busy'))
const skRows = computed(() => Math.min(20, Math.max(1, Math.round(props.rows))))
const skWidth = (i: number) => skeletonWidth(i)

const stageList = computed(() => (props.stages || []).filter((s) => typeof s === 'string' && s.length > 0))
const stageIndex = computed(() => Math.min(stageList.value.length - 1, Math.max(0, Math.round(props.stage))))

const ratio = computed<number | null>(() => {
  const p = props.progress
  if (typeof p !== 'number' || !Number.isFinite(p)) return null
  return p < 0 ? 0 : p > 1 ? 1 : p
})
/** 传了 progress（哪怕是 null）才画进度条：null = 不确定态（一层扫过去的高光） */
const barEnabled = computed(() => props.progress !== undefined)

/** 元信息行：1~10s 给「已等 N 秒」；>10s 再加「约还需 N 秒（估算）」和速率 */
const metaList = computed<string[]>(() => {
  const out: string[] = []
  if (tier.value !== 'normal' && tier.value !== 'long') return out
  if (props.showElapsed) out.push(t('worldsim.loader.elapsed', { n: Math.max(0, Math.round(elapsed.value / 1000)) }))
  if (tier.value === 'long') {
    const eta = props.etaMs
    if (typeof eta === 'number' && Number.isFinite(eta)) {
      // eta = 0 是「已达目标量」的约定，这时不报剩余时间（报「0 秒」是噪音）
      if (eta > 0) out.push(t('worldsim.loader.eta', { n: Math.round(eta / 1000) }))
    } else {
      out.push(t('worldsim.loader.etaUnknown'))
    }
    const r = props.rate
    if (typeof r === 'number' && Number.isFinite(r) && r > 0) out.push(t('worldsim.loader.rate', { n: r.toFixed(1) }))
  }
  return out
})
const metaReady = computed(() => barEnabled.value || props.showElapsed || props.cancellable)

const hintText = computed(() => (tier.value === 'long' ? props.hint : ''))
const cancelLabel = computed(() => props.cancelText || t('worldsim.loader.cancel'))
const hasExtra = computed(
  () => stageList.value.length >= 2 || barEnabled.value || metaReady.value || !!props.hint || props.cancellable,
)

const rootClass = computed(() => [
  `ws-loading--${props.size}`,
  `ws-loading--${tier.value}`,
  {
    'ws-loading--inline': props.inline,
    'ws-loading--enter': props.variant === 'init',
    'ws-loading--in': tier.value !== 'idle',
  },
])
</script>

<style scoped>
/* 布局只管这里（动画/关键帧/降级覆盖在 worldsim-loading.css：那些必须全局）*/
.ws-loading__body {
  display: flex;
  flex-direction: column;
  gap: 0.24em;
}
.ws-loading--inline .ws-loading__body {
  align-items: flex-start;
}
</style>
