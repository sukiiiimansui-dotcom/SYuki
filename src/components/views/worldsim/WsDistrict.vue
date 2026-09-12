<template>
  <!--
    小区地图（引导主线的终点）
    两段式（机主定的 B-11 方案）：**本地草图毫秒级先出**，AI 精绘在后台流式生成、
    画完淡入替换。AI 的绘制过程本身就是「可视化」——每收到一栋楼就多一个方块，
    所以这里是**同一块画布**在长出来，而不是在角落里另开一个小窗。

    双兜底（绝不能白屏）：
      · 草图失败 → 提示 + 重试，并把已经画出来的 AI 内容留住
      · AI 失败  → 保留草图 + 说明原因（多半是没配 LLM），随时可重试
  -->
  <div class="ws-dist">
    <div ref="neighHost" class="ws-dist__stage ws-neigh">
      <!-- 手势的变换容器：草图层与 AI 层一起被平移/缩放。
           放在这一层（而不是各自的 SVG/img 上）的好处是——AI 流式重绘会反复重建
           内层节点，变换挂在外层就不会被重建冲掉。 -->
      <div
        ref="neighPan"
        class="ws-neigh__pan"
        :class="{ 'is-drag': gsDragging || gsInstant }"
        :style="gsStyle"
      >
        <!-- ① 本地草图层（后端 sketch：确定性、毫秒级、不调 LLM） -->
        <div class="ws-neigh__layer" :class="{ 'is-out': aiVisible }">
          <img v-if="sketchUrl" :src="sketchUrl" :alt="`${area} 小区草图`" />
          <!-- 草图通常 <1s 就回来了：靠 WsLoading 的 180ms 延迟兜住 ——
               快到看不见的时候**一个转圈都不该闪**（那比不显示更糟）。 -->
          <WsLoading
            v-else-if="sketchLoading"
            variant="map"
            :text="t('worldsim.loader.sketch.text')"
            :sub="t('worldsim.loader.sketch.sub')"
          />
          <div v-else class="ws-dist__fail">
            <div class="ws-note ws-note--err">草图没画出来：{{ sketchError || '未知原因' }}</div>
            <button class="ws-btn" type="button" @click="loadSketch(true)">重试草图</button>
          </div>
        </div>

        <!-- ② AI 精绘层（流式增量；第一条要素到了就淡入接管）
             注意是 v-html 注入的裸 <svg>：我这份画笔**不写 <style>**，
             所有颜色/线宽都是元素属性，所以注入真 DOM 也不会污染任何全局样式。 -->
        <div class="ws-neigh__layer ws-neigh__ai" :class="{ 'is-in': aiVisible }">
          <div v-if="aiSvgInner" class="ws-paint-host" v-html="aiSvgInner" />
        </div>

        <!-- ③ 人物层（P2-1）：由页面从外面插进来。
             为什么用插槽而不是在这里 import 组件：
               这一层的数据（角色/日程/头像）与面板状态都属于页面级别，
               而且它必须落在**同一个手势变换容器**里才会跟着地图一起缩放。
             `pin` 这个名字的意思是「钉在地图上的东西」——
               下一阶段（P4 的走动小人、事件气泡）也走同一个口子。 -->
        <slot name="pin" />
      </div>

      <!-- 缩放/复位（与行政区划舞台同一个 composable，手感一致） -->
      <div class="ws-zoomctl">
        <button type="button" title="放大" :disabled="gsScale >= 3.99" @click="zoomBy(1.35)">＋</button>
        <button type="button" title="缩小" :disabled="gsScale <= 0.61" @click="zoomBy(1 / 1.35)">－</button>
        <button type="button" title="复位（也支持双击 / 双指双击）" @click="gsReset(false)">⟲</button>
      </div>

      <!-- 进度卡：AI 精绘的实时状态。
           这是**全项目最长的等待**（实测 17 秒起、可能几十秒），所以它走 WsLoading 的
           长任务档：阶段条（构思布局→落建筑→铺路绿化→收尾）+ 已等多久 +
           预估剩余（**样本够才给，且写明是估算**）+ 始终可点的「停止」。

           注意：**同一屏只有一个加载指示器** —— 原来这里自己画的那条进度条已经删掉，
           统一由 WsLoading 画（两条一起动就是两个竞争的指示器）。
           那条进度条的机制没变：仍然是写一个 `--ws-prog`（0~1）、由 CSS 用
           `transform: scaleX()` 推进（**不动 width** —— width 是布局属性，流式绘制时
           每帧改一次会让整条卡片反复重排），只是现在画它的人换成了
           `worldsim-loading.css` 里的 `.ws-loading__bar`。
           下面的计数 / 已达上限 / 重画是**信息与操作**，不是加载指示器。 -->
      <div v-if="aiRunning || aiDone || aiError" class="ws-prog ws-card">
        <WsLoading
          variant="draw"
          size="sm"
          :text="progTitle"
          :sub="stageText"
          :stages="stageNames"
          :stage="stageIndex"
          :progress="progRatio"
          :eta-ms="etaInfo.etaMs"
          :rate="etaInfo.ratePerSec"
          :cancellable="aiRunning"
          :cancel-text="t('worldsim.loader.cancel')"
          :hint="t('worldsim.loader.draw.longHint')"
          @cancel="stopAi(true)"
        />
        <div class="ws-prog__row ws-dist__counts">
          <span>🏢 {{ counts.buildings }}</span>
          <span>🛣 {{ counts.roads }}</span>
          <span>🌳 {{ counts.parks }}</span>
          <span>💧 {{ counts.water }}</span>
        </div>
        <div v-if="dropped" class="ws-dist__dropped">{{ t('worldsim.loader.draw.dropped', { n: dropped }) }}</div>
        <div class="ws-prog__row ws-dist__ops">
          <button v-if="!aiRunning" class="ws-btn ws-btn--ghost" type="button" @click="startAi(true)">
            {{ t('worldsim.loader.draw.retry') }}
          </button>
        </div>
      </div>

      <!-- 状态角标：明确区分「本地草图」和「AI 精绘」。
           AI 正在画的时候**不显示**：那时进度卡已经把状态说全了，
           同一屏上两个「正在绘制」只会互相抢注意力（收敛成一个主导的）。 -->
      <div v-if="!aiRunning" class="ws-dist__badge" :class="{ 'is-ai': aiDone }">
        <span v-if="aiDone">{{ t('worldsim.loader.draw.badgeAi') }}</span>
        <span v-else>{{ t('worldsim.loader.draw.badgeSketch') }}</span>
      </div>
    </div>

    <div v-if="aiError" class="ws-note ws-note--warn ws-dist__err">
      <span>{{ aiError }}</span>
      <button class="ws-btn ws-btn--ghost" type="button" @click="startAi(true)">
        {{ t('worldsim.loader.draw.retryFull') }}
      </button>
    </div>

    <div class="ws-dist__foot">
      <div class="ws-dist__area">
        <span class="ws-dist__pin" aria-hidden="true">🏘</span>
        <span class="ws-dist__name">{{ area }}</span>
        <span class="ws-tag">小区</span>
      </div>
      <div class="ws-dist__ops2">
        <button class="ws-btn ws-btn--ghost" type="button" @click="emit('back')">← 回到区县</button>
        <button class="ws-btn" type="button" @click="emit('done')">进入这个世界</button>
      </div>
    </div>

    <!-- 流式日志（默认折叠：出问题时才有用，平时不干扰） -->
    <details v-if="logs.length" class="ws-dist__logs">
      <summary>绘制过程（{{ logs.length }} 条）</summary>
      <div class="ws-scroll ws-dist__logbox">
        <div v-for="(l, i) in logs" :key="i" class="ws-dist__logline">{{ l }}</div>
      </div>
    </details>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import WsLoading from './WsLoading.vue'
import {
  districtRenderSvg,
  startDistrictStream,
  type DistrictStreamEvent,
} from '@/api/services/worldMap'
import { DistrictPaint, paletteOf, type PaintCounts } from './wsDistrictPaint'
import { hash32, svgToDataUrl } from './wsGeo'
// 加载态纯逻辑（分档 / 阶段推断 / 预估剩余）：抽出去是为了能被自检脚本直接跑，
// 见 wsLoadPlan.ts 的文件头说明 —— 「预估剩余」是最容易变成骗人的地方。
import { DRAW_STAGES, drawStageOf, etaOf, planBuildingsOf } from './wsLoadPlan'
import { useWorldSimGestures } from '@/composables/useWorldSimGestures'

const props = withDefaults(
  defineProps<{
    /** 区域名「广州市·越秀区」——既写进草图 seed，也作为 AI 提示词里的地段 */
    area: string
    /** 三套地图主题之一（gaode / dark / water），与行政区划图保持一致 */
    mapStyle?: string
  }>(),
  { mapStyle: 'gaode' },
)

const emit = defineEmits<{ (e: 'back'): void; (e: 'done'): void }>()

const { t } = useI18n()

/** 草图的网格规模：28×28（与 AI 的 expand=1 对齐，两边密度不至于差一倍） */
const SKETCH_SIZE = 28
const AI_EXPAND = 1

// 小区图也要能拖能缩（它就是玩家待得最久的那张图）——与行政区划舞台共用同一套手势
const neighHost = ref<HTMLElement | null>(null)
const neighPan = ref<HTMLElement | null>(null)
const {
  scale: gsScale,
  dragging: gsDragging,
  instant: gsInstant,
  panStyle: gsStyle,
  reset: gsReset,
  zoomBy,
} = useWorldSimGestures({ target: neighHost, content: neighPan })

const sketchUrl = ref('')
const sketchLoading = ref(true)
const sketchError = ref('')
const aiSvgInner = ref('')
const aiVisible = ref(false)
const aiDone = ref(false)
const aiRunning = ref(false)
const aiError = ref('')
const counts = ref<PaintCounts>({ buildings: 0, roads: 0, parks: 0, water: 0 })
const dropped = ref(0)
const elapsed = ref(0)
const stageText = ref('')
const logs = ref<string[]>([])

/** 当前这轮的画笔（换区县/重画就换一个新实例，旧的连同它的连接一起丢掉） */
let paint = new DistrictPaint(SKETCH_SIZE)
let stopStream: (() => void) | null = null
let rafId = 0
let timer: number | null = null
let startedAt = 0
/**
 * 已收到的**要素**数（建筑 + 路 + 树 + 水；控制事件 start/meta/size 不算，见 onEvent）。
 * 它有两个用途：**速率**（项/秒，实测值）和「每栋楼伴随多少条消息」的换算。
 * ⚠️ 不要再拿它当进度分母：原来写死 420，而 28×28 档后端只让模型画 ~20 栋，
 *    于是进度条永远只走到 5% 就停住 —— 那不是进度，那是装饰。
 */
const received = ref(0)
/** 后端自报的网格边长（Event::Start / Event::Size）—— 计划建筑数由它推出来 */
const gridSize = ref(SKETCH_SIZE)
/** 最近一条要素的时间戳 + 「现在」（由既有的 500ms 计时器推进，不新增定时器） */
const lastItemAt = ref(0)
const nowTs = ref(0)

const palette = computed(() => paletteOf(props.mapStyle))

/* ── 长任务的进度 / 阶段 / 预估（规则全在 wsLoadPlan.ts，这里只喂数据）──────
 * 阶段由**要素类型**推断：后端是流式推的，先建筑、后道路/绿化，最后 done，
 * 所以不需要后端多告诉我们任何东西。 */
const planBuildings = computed(() => planBuildingsOf(gridSize.value))
const drawStage = computed(() =>
  drawStageOf(counts.value, { done: aiDone.value, planBuildings: planBuildings.value }),
)
const stageIndex = computed(() => Math.max(0, DRAW_STAGES.indexOf(drawStage.value)))
const stageNames = computed(() => DRAW_STAGES.map((s) => t(`worldsim.loader.draw.stage.${s}`)))
const etaInfo = computed(() =>
  etaOf({
    done: counts.value.buildings,
    target: planBuildings.value,
    received: received.value,
    elapsedMs: elapsed.value,
    sinceLastItemMs: lastItemAt.value ? Math.max(0, nowTs.value - lastItemAt.value) : 0,
  }),
)
/**
 * 进度条的确定值：**只有建筑阶段、且样本足够**时才给。
 * 进入铺路/收尾后剩下的要素数量后端没告诉我们 —— 这时退回不确定态，
 * 不拿一个 100% 假装快完了。
 */
const progRatio = computed<number | null>(() => {
  if (drawStage.value !== 'buildings') return null
  const info = etaInfo.value
  return info.confident ? info.ratio : null
})
const progTitle = computed(() => {
  if (aiDone.value) return t('worldsim.loader.draw.done')
  if (!aiRunning.value) return t('worldsim.loader.draw.undone')
  return t(`worldsim.loader.draw.stageText.${drawStage.value}`)
})

function log(msg: string) {
  const now = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  logs.value = [...logs.value.slice(-60), `${p(now.getMinutes())}:${p(now.getSeconds())} ${msg}`]
}

/* ── 草图（秒出）────────────────────────────────────────────────────────── */
async function loadSketch(fresh = false) {
  sketchLoading.value = true
  sketchError.value = ''
  try {
    // seed 用「区域名的 32 位哈希」：同一个区县永远同一张草图（可复现），换区县自动换图。
    // 为什么不用随机数：用户退出再进来看到另一张图会很出戏。
    const svg = await districtRenderSvg({
      style: props.mapStyle || 'gaode',
      mode: '2d',
      zoom: 3,
      size: SKETCH_SIZE,
      seed: hash32(props.area || 'world'),
    })
    if (fresh) log('重新生成草图')
    sketchUrl.value = svgToDataUrl(svg)
  } catch (e) {
    sketchError.value = e instanceof Error ? e.message : String(e)
  } finally {
    sketchLoading.value = false
  }
}

/* ── AI 精绘（流式）────────────────────────────────────────────────────── */
function scheduleRender() {
  if (rafId) return // 单飞：一帧最多重建一次 SVG 文本（一次流可能推几百条）
  rafId = requestAnimationFrame(() => {
    rafId = 0
    counts.value = paint.counts()
    dropped.value = paint.dropped
    aiSvgInner.value = paint.toSvg(palette.value)
  })
}

/**
 * 后端自报的网格规模（Event::Start / Event::Size 都会带）。
 * 只在本轮还没收到任何要素时采纳：一旦画上了，再改尺寸会让已画的坐标全部错位。
 */
function adoptSize(size?: number) {
  const n = Number(size)
  if (!Number.isFinite(n) || n < 8) return
  gridSize.value = n // 计划建筑数（预估剩余的分母）由它推出来，所以先采纳
  if (paint.total === 0 && n !== paint.size) {
    paint = new DistrictPaint(n)
    log(`网格规模：${n}×${n}`)
  }
}

function onEvent(ev: DistrictStreamEvent) {
  // ⚠️ received 只数**要素**（建筑/路/树/水）：它是速率和「每栋楼伴随几条消息」的分母。
  // 把 start / meta / size 这些控制事件也算进去的话，一栋楼都还没画出来速率就不是 0 了
  // （实测显示成「0.1 项/秒」），预估也会被这一串常量事件带偏。
  if (ev.type === 'building' || ev.type === 'road' || ev.type === 'park' || ev.type === 'water') {
    received.value++
    // 每一条要素都刷新「最近活动时间」：预估剩余靠它判断有没有卡住
    // （卡住时还倒计时就是撒谎，见 wsLoadPlan 的 ETA_RULES.stallMs）
    lastItemAt.value = Date.now()
  }
  switch (ev.type) {
    case 'start':
      // 区域名不用再占一行：卡片下面就有「小区」标题，这里留给「小区名」更有信息量
      log(`开始生成${ev.model ? `（${ev.model}）` : ''}`)
      adoptSize(ev.size) // start 也会带 size（Rust 侧 Event::Start），和 size 事件等价
      break
    case 'meta':
      if (ev.name) stageText.value = t('worldsim.loader.draw.name', { name: ev.name })
      break
    case 'size':
      adoptSize(ev.size)
      break
    case 'building':
      if (ev.item) paint.addBuilding(ev.item)
      break
    case 'road':
      if (ev.item) paint.addRoad(ev.item)
      break
    case 'park':
      if (ev.item) paint.addPark(ev.item)
      break
    case 'water':
      if (ev.item) paint.addWater(ev.item)
      break
    case 'warn':
      if (ev.message) log(`⚠ ${ev.message}`)
      break
    case 'error':
      aiError.value = ev.message || 'AI 绘制失败'
      break
    case 'done':
      // 最终布局才是权威：用它整份替换（流式过程中偶尔会丢片段）
      if (ev.layout) paint.loadLayout(ev.layout)
      counts.value = paint.counts()
      aiDone.value = true
      log(`完成：${counts.value.buildings} 栋建筑 / ${counts.value.roads} 条路`)
      break
    default:
      break
  }
  // 第一条要素到达 = AI 层淡入、草图淡出（这就是「精绘版本淡入替换」）
  if (!aiVisible.value && paint.total > 0) {
    aiVisible.value = true
    log('第一条要素已到达，切换到 AI 图层')
  }
  scheduleRender()
}

function startAi(fresh = false) {
  stopAi(false)
  if (fresh) {
    paint = new DistrictPaint(SKETCH_SIZE)
    aiSvgInner.value = ''
    aiVisible.value = false
    aiDone.value = false
    aiError.value = ''
    received.value = 0
    counts.value = { buildings: 0, roads: 0, parks: 0, water: 0 }
    dropped.value = 0
    stageText.value = ''
  }
  aiRunning.value = true
  startedAt = Date.now()
  elapsed.value = 0
  nowTs.value = startedAt
  lastItemAt.value = 0 // 新一轮：还没收到任何要素
  if (timer !== null) window.clearInterval(timer)
  timer = window.setInterval(() => {
    elapsed.value = Date.now() - startedAt
    // nowTs 只用来算「距最近一条要素多久」（停滞判定），和 elapsed 同一个心跳
    nowTs.value = Date.now()
  }, 500)
  log(`开始 AI 精绘（expand=${AI_EXPAND}）`)

  // 生命周期归组件：卸载/换区县/点停止都必须 close，否则连接会漏（这是数据层的约定）
  stopStream = startDistrictStream(
    { area: props.area, expand: AI_EXPAND },
    onEvent,
    () => {
      aiRunning.value = false
      if (timer !== null) {
        window.clearInterval(timer)
        timer = null
      }
    },
    (msg) => {
      aiRunning.value = false
      if (timer !== null) {
        window.clearInterval(timer)
        timer = null
      }
      // 已经画出一部分就留住它（机主之前踩过「一断线全没了」的坑）
      aiError.value = paint.total > 0 ? `AI 绘制中断（已保留画出的 ${paint.total} 项）：${msg}` : `AI 精绘不可用：${msg}`
      log(`✖ ${msg}`)
      scheduleRender()
    },
  )
}

function stopAi(manual = false) {
  if (stopStream) {
    stopStream()
    stopStream = null
  }
  if (timer !== null) {
    window.clearInterval(timer)
    timer = null
  }
  if (manual) {
    aiRunning.value = false
    log('已手动停止')
    // 手动停止时把已画的留住（用户看得到自己停下来的那一刻）
    if (paint.total > 0) {
      aiVisible.value = true
      scheduleRender()
    }
  }
}

/* ── 生命周期 ─────────────────────────────────────────────────────────── */
function reset() {
  stopAi(false)
  gsReset(false) // 换区县：缩放平移一起归零，免得新图停在上一张的放大倍率上
  aiDone.value = false
  aiError.value = ''
  aiRunning.value = false
  received.value = 0
  logs.value = []
  paint = new DistrictPaint(SKETCH_SIZE)
  aiSvgInner.value = ''
  aiVisible.value = false
  counts.value = { buildings: 0, roads: 0, parks: 0, water: 0 }
  void loadSketch()
  startAi(true)
}

// 换区县才整轮重来（草图 + AI 重新生成）
watch(
  () => props.area,
  (v) => {
    if (!v) return
    reset()
  },
  { immediate: true },
)

// 换**地图主题**（高德/暗色/水系）只重画草图、用新配色重渲染 AI 图层：
// 主题是纯视觉的，为此再调一次大模型（几十秒 + 真金白银）完全没必要。
watch(
  () => props.mapStyle,
  () => {
    if (!props.area) return
    void loadSketch()
    scheduleRender()
  },
)

onBeforeUnmount(() => {
  stopAi(false)
  if (rafId) cancelAnimationFrame(rafId)
})

/**
 * 把手势的缩放倍率暴露给页面。
 *
 * 用途只有一个：地图上的头像要「位置跟着缩放、尺寸不变」，
 * 就得知道自己被放大了几倍（见 WsAvatarMark 的 zoom 说明）。
 * 这个倍率是**这个组件内部**的手势状态（neighHost/neighPan 都在这里），
 * 页面拿不到，所以只能用 defineExpose 露出来 —— 不去动 shared composable，
 * 也不给页面再塞一个手势实例（两个实例会互相打架）。
 *
 * ⚠️ P4-4 的拖拽落点**不需要**再暴露 tx/ty：页面拿变换容器的视觉矩形反推即可
 *    （`rect.width / pan.offsetWidth` 就是 scale，`rect.left/top` 里已经含了平移）。
 *    「内部状态少露一个是一个」比「多露三个」更值。
 */
defineExpose({ gsScale })
</script>

<style scoped>
.ws-dist {
  display: flex;
  flex-direction: column;
  gap: 0.6em;
  height: 100%;
  min-height: 0;
}
.ws-dist__stage {
  flex: 1;
  min-height: 14em;
}
.ws-dist__fail {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.6em;
  padding: 1em;
  text-align: center;
}
.ws-dist__counts {
  gap: 0.5em;
  font-size: 0.92em;
  color: var(--ws-fg-dim);
}
.ws-dist__dropped {
  margin-top: 0.3em;
  font-size: 0.8em;
  color: var(--ws-fg-dim);
}
.ws-dist__ops {
  justify-content: flex-end;
  margin-top: 0.4em;
}
.ws-dist__badge {
  position: absolute;
  left: 0.8em;
  top: 0.8em;
  padding: 0.16em 0.6em;
  font-size: 0.8em;
  color: var(--ws-fg-dim);
  background: var(--ws-panel);
  border: 1px solid var(--ws-border);
  border-radius: 999px;
  backdrop-filter: blur(var(--ws-blur));
  -webkit-backdrop-filter: blur(var(--ws-blur));
}
.ws-dist__badge.is-ai {
  color: var(--ws-on-primary);
  background: var(--ws-primary);
  border-color: transparent;
}
.ws-dist__err {
  flex-wrap: wrap;
}
.ws-dist__foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.6em;
  flex-wrap: wrap;
}
.ws-dist__area {
  display: flex;
  align-items: center;
  gap: 0.4em;
  min-width: 0;
}
.ws-dist__pin {
  font-size: 1.1em;
}
.ws-dist__name {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ws-dist__ops2 {
  display: flex;
  gap: 0.5em;
}
.ws-dist__logs {
  font-size: 0.84em;
  color: var(--ws-fg-dim);
}
.ws-dist__logbox {
  max-height: 7em;
  margin-top: 0.3em;
  padding: 0.4em 0.6em;
  background: var(--ws-panel-2);
  border-radius: var(--ws-radius-sm);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.ws-dist__logline {
  line-height: 1.6;
}
</style>
