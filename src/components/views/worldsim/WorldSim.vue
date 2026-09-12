<template>
  <!--
    「世界模拟」主页面（P1：入口 + 首次引导下钻主线 + 加载动画 + 小清新外壳）

    分工：
      · 状态与流转 → useWorldSim（composable，见那边的状态机注释）
      · 取图与缓存 → useWorldSimGeo
      · 本文件只做「把状态画出来 + 把用户动作转回去」
    层级：.ws-root 用 z-index:2 —— 高于 WorldMapLayer 的叠加背景层(1，pointer-events:none)，
          低于菜单/弹窗(60/1000)。机主给的层叠表里，立绘是 1、菜单是 1000，这里不越级。
  -->
  <div class="ws-root" :class="rootClass">
    <!-- ── 顶栏 ─────────────────────────────────────────────────────── -->
    <header class="ws-top">
      <button class="ws-btn ws-btn--ghost" type="button" title="回主菜单" @click="goMenu">←</button>
      <div class="ws-brand">
        <span class="ws-brand__ico" aria-hidden="true">🌏</span>
        <span class="ws-brand__name">{{ t('worldsim.title') }}</span>
        <span class="ws-tag ws-brand__stage">{{ stageLabel }}</span>
      </div>

      <WsCrumb
        v-if="path.length > 1"
        class="ws-top__crumb"
        :crumbs="crumbs"
        :cursor-index="cursor"
        @go="backTo"
      />
      <span class="ws-spacer" />

      <!-- 地图本体仍是现有三套主题（高德/暗色/水系），只有外壳走小清新 -->
      <select class="ws-sel" :value="style" @change="onStyle" title="地图主题">
        <option value="gaode">高德</option>
        <option value="dark">暗色</option>
        <option value="water">水系</option>
      </select>
      <button class="ws-btn ws-btn--ghost" type="button" :title="`皮肤：${themeName === 'mint' ? '薄荷奶油' : '现代简约·毛玻璃'}`" @click="cycleTheme">
        {{ themeName === 'mint' ? '🍃 薄荷' : '🧊 玻璃' }}
      </button>
      <button class="ws-btn ws-btn--ghost" type="button" :title="`深浅：${darkLabel}`" @click="cycleDark">{{ dark ? '🌙' : '☀️' }}</button>
      <button class="ws-btn ws-btn--ghost" type="button" title="清掉「上次位置」，从加载动画重新走一遍引导" @click="restart">重新引导</button>
    </header>

    <!-- ── 定位来源 / 上次位置：如实告诉用户「这个位置是怎么来的」──────── -->
    <div v-if="locLabel || restored" class="ws-srcbar">
      <span class="ws-tag">{{ locLabel || '—' }}</span>
      <span v-if="areaLabel" class="ws-srcbar__now">当前：{{ areaLabel }}</span>
      <span v-if="restored" class="ws-srcbar__hint">（首次引导已走过，直接进上次位置）</span>
      <span v-if="note" class="ws-srcbar__note">{{ note }}</span>
    </div>

    <!-- ── 舞台 ─────────────────────────────────────────────────────── -->
    <main class="ws-stage">
      <!-- ① 初始化：加载动画「正在展开世界…」 -->
      <section v-if="step === 'boot'" class="ws-center">
        <WsLoading variant="init" text="正在展开世界…" sub="准备全国省级轮廓（只画省界）…" />
      </section>

      <!-- ② 定位中 -->
      <section v-else-if="step === 'locating'" class="ws-center">
        <WsLoading variant="locate" text="正在定位" :sub="gpsHint" />
      </section>

      <!-- ③ 定位失败 / 被拒：手动选城市 + 用 IP 估测，两条路都给 -->
      <section v-else-if="step === 'locateFailed'" class="ws-center">
        <div class="ws-card ws-locatefail">
          <WsLoading variant="locate" size="sm" text="没能自动定位" :sub="note" />
          <div class="ws-note">可以走下面任一条路，都能继续往下玩：</div>
          <div class="ws-locatefail__ops">
            <button class="ws-btn ws-btn--primary" type="button" @click="openManual('手动选择省 / 市 / 区县')">
              🗺 手动选城市
            </button>
            <button class="ws-btn" type="button" :disabled="busy" @click="locate({ ipOnly: true })">
              📡 用 IP 估测
            </button>
          </div>
          <div class="ws-locatefail__tip">
            IP 估测只到城市级（实测常见结果就是「重庆市」这种），所以后面还会请你确认具体区县。
          </div>
        </div>
      </section>

      <!-- ④ 地图（国/省/市/区县）—— v-html 注入后端 SVG，点击靠事件委托 -->
      <section v-else class="ws-mapwrap">
        <div
          ref="geoHost"
          class="ws-geo"
          @click="onGeoClick"
          @dblclick="onGeoDbl"
        >
          <div v-if="stageMarkup" class="ws-geo__inner" v-html="stageMarkup" />
          <div v-else-if="!busy" class="ws-center ws-geo__empty">
            <div class="ws-note ws-note--err">这一级的地图没画出来</div>
            <button class="ws-btn" type="button" @click="retryStage">重试</button>
          </div>
        </div>

        <!-- 区域切换 / 首次加载的加载动画（盖在舞台上，不销毁已有的图） -->
        <div v-if="busy" class="ws-mapmask">
          <WsLoading variant="map" :text="busyText || '正在加载地图'" sub="后端渲染中…" />
        </div>
      </section>

      <!-- ⑤ 一次性确认 -->
      <WsConfirm
        v-if="step === 'confirm'"
        :area="confirmArea"
        :source-label="locLabel"
        :note="!hasDistrict ? '定位精度只到市，确认后还要挑一个区县' : ''"
        @yes="confirmYes"
        @no="confirmNo"
      />

      <!-- ⑥ 三级联动（定位失败 / 纠偏共用） -->
      <div v-if="step === 'manual'" class="ws-modal">
        <WsPicker
          :load-regions="geo.loadRegions"
          :initial="path"
          :note="note"
          @select="applyManual"
          @cancel="onPickerCancel"
        />
      </div>

      <!-- ⑦ 小区地图（终点） -->
      <section v-else-if="step === 'neighborhood'" class="ws-neighwrap">
        <WsDistrict
          :area="areaLabel || '未知区域'"
          :map-style="style"
          @back="backTo(path.length - 1)"
          @done="onWorldEntered"
        />
      </section>
    </main>

    <!-- ── 底部动作条（按状态给唯一的主按钮）──────────────────────────── -->
    <footer v-if="showFoot" class="ws-foot">
      <div class="ws-foot__hint">{{ footHint }}</div>
      <div class="ws-foot__ops">
        <button v-if="canEnter" class="ws-btn ws-btn--primary" type="button" @click="enterPick">
          进入 {{ targetName || '这一级' }}
        </button>
        <button
          v-if="step === 'district'"
          class="ws-btn ws-btn--primary"
          type="button"
          @click="generateNeighborhood"
        >
          🏘 生成小区地图
        </button>
        <button v-if="step === 'district'" class="ws-btn ws-btn--ghost" type="button" @click="openManual('换一个区县')">
          换个区县
        </button>
        <button v-if="step === 'city'" class="ws-btn ws-btn--ghost" type="button" @click="openConfirm">
          再确认一次位置
        </button>
        <button v-if="canEnter" class="ws-btn ws-btn--ghost" type="button" @click="openManual('手动选择位置')">
          选别的地方
        </button>
      </div>
      <div v-if="error" class="ws-foot__err">
        <div class="ws-note ws-note--err">
          <span>{{ error }}</span>
          <button class="ws-btn ws-btn--ghost" type="button" @click="retryStage">重试</button>
        </div>
      </div>
    </footer>
  </div>
</template>

<script setup lang="ts">
// 皮肤（主题变量 + 动画关键帧）在这里引入：全局只此一处，后续 P2~P5 的页面直接复用同一个文件
import '@/assets/styles/worldsim.css'
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import WsLoading from './WsLoading.vue'
import WsCrumb from './WsCrumb.vue'
import WsConfirm from './WsConfirm.vue'
import WsPicker from './WsPicker.vue'
import WsDistrict from './WsDistrict.vue'
import { geoLevelOf, levelLabel } from './wsGeo'
import { useElementSize, useWorldSimGeo, useWorldSimTheme } from '@/composables/useWorldSimGeo'
import { useWorldSim } from '@/composables/useWorldSim'

const router = useRouter()
const { t } = useI18n()

/* 皮肤 + 取图器 + 状态机 */
const theme = useWorldSimTheme()
const geo = useWorldSimGeo()
const geoHost = ref<HTMLElement | null>(null)
// 舞台尺寸就量 .ws-geo 本身：后端 SVG 的宽高必须与它 1:1，字号才不会在手机上缩没
const { sizeForBackend } = useElementSize(geoHost, { w: 900, h: 620 })
const sim = useWorldSim({ geo, getViewport: () => sizeForBackend.value })

// 解构出来给模板用：Vue 模板对**顶层** ref 会自动解包，比满篇 `sim.xxx.value` 稳得多
//（踩过一次：`:value="sim.style"` 忘了 .value，下拉框直接对不上任何选项）。
// 脚本内部仍然一律用 sim.xxx（保底不出错）。
const {
  style,
  step,
  path,
  cursor,
  hitAd,
  pickAd,
  busy,
  busyText,
  error,
  note,
  locSource,
  locLabel,
  restored,
  neighReady,
  regionsOfStage,
  crumbs,
  leaf,
  areaLabel,
  start,
  locate,
  enterPick,
  setPick,
  drillTo,
  openConfirm,
  confirmYes,
  confirmNo,
  openManual,
  applyManual,
  backTo,
  generateNeighborhood,
  restart,
  retryStage,
  changeMapStyle,
} = sim
const { rootClass, theme: themeName, dark, darkPref, cycleTheme, cycleDark } = theme

/* ── 展示用的派生量 ─────────────────────────────────────────────────── */
const stageLabel = computed(() => {
  switch (sim.step.value) {
    case 'boot':
      return '初始化'
    case 'locating':
      return '定位中'
    case 'locateFailed':
      return '待选位置'
    case 'manual':
      return '手动选择'
    case 'confirm':
      return '确认位置'
    case 'neighborhood':
      return '小区'
    default:
      break
  }
  const ad = sim.stage.value?.adcode || '100000'
  return levelLabel(geoLevelOf(ad))
})

const darkLabel = computed(() => (theme.darkPref.value === '' ? '跟随系统' : theme.darkPref.value === 'dark' ? '深色' : '浅色'))

const gpsHint = computed(() =>
  typeof navigator !== 'undefined' && navigator.geolocation
    ? '正在申请系统定位权限（被拒也没关系，会自动退到 IP 估测）'
    : '这台设备没有系统定位，直接用 IP 估测',
)

/** 当前选中/命中的那一级，名字是什么（按钮文案用） */
const targetAd = computed(() => sim.pickAd.value || sim.hitAd.value || '')
const targetName = computed(() => {
  const ad = targetAd.value
  if (!ad) return ''
  return sim.regionsOfStage.value.find((r) => r.adcode === ad)?.name || ad
})

/** 已知的最深一级是不是区县（决定确认弹窗要不要提醒「还得选区」） */
const hasDistrict = computed(() => {
  const last = sim.path.value[sim.path.value.length - 1]
  return !!last && geoLevelOf(last.adcode) === 'district'
})

/**
 * 确认弹窗里问的地名：只取**最后两级**（「广州市·越秀区」），与需求里的问法一致。
 * 为什么不直接用 areaLabel（含省）：省在图上已经高亮过了，再念一遍只会让问句变长。
 * 直辖市只有「省→区」两级时自然就是「北京市·朝阳区」；只知到市时就是「重庆市」。
 */
const confirmArea = computed(() => {
  const parts = sim.path.value.filter((p) => p.adcode !== '100000')
  const last2 = parts.slice(-2).map((p) => p.name)
  return last2.join('·') || targetName.value
})

/** 主按钮「进入 X」只在国/省/市三级出现；区县阶段给的是「生成小区地图」 */
const canEnter = computed(() => ['country', 'province', 'city'].includes(sim.step.value) && !!targetAd.value)

const showFoot = computed(() =>
  ['country', 'province', 'city', 'district', 'neighborhood'].includes(sim.step.value),
)

const footHint = computed(() => {
  switch (sim.step.value) {
    case 'country':
      return targetAd.value ? `定位命中：${targetName.value}（已在地图上高亮）——点「进入」才下钻` : '点地图上的省份选中，或直接点「选别的地方」'
    case 'province':
      return targetAd.value ? `定位到：${targetName.value}` : '点一个市继续下钻'
    case 'city':
      return '确认弹窗里选「是」就直接下钻到区县'
    case 'district':
      return '区县已就位，接下来生成小区地图（先出本地草图，AI 精绘随后淡入）'
    default:
      return ''
  }
})

/* ── 后端 SVG 落地后：打高亮 / 选中标记 ─────────────────────────────── */
function applyMarks() {
  const host = geoHost.value
  if (!host) return
  const groups = host.querySelectorAll<SVGGElement>('.geo-region')
  groups.forEach((g) => {
    const ad = g.getAttribute('data-adcode') || ''
    g.classList.toggle('ws-on', !!sim.hitAd.value && ad === sim.hitAd.value)
    g.classList.toggle('ws-pick', !!sim.pickAd.value && ad === sim.pickAd.value)
  })
}

// 图换了、或高亮/选中变了，都要重打一遍标记（v-html 重渲染会把 class 冲掉）
watch(
  () => [sim.stage.value?.adcode, sim.stage.value?.markup, sim.hitAd.value, sim.pickAd.value],
  () => void nextTick(applyMarks),
  { immediate: true },
)

const stageMarkup = computed(() => sim.stage.value?.markup || '')

/* ── 地图点击（v-html 的内容没有 Vue 事件，只能事件委托）───────────── */
function pickFromEvent(e: Event): { adcode: string; name: string } | null {
  const el = e.target as Element | null
  const g = el?.closest?.('.geo-region') as Element | null
  if (!g) return null
  const adcode = g.getAttribute('data-adcode') || ''
  const name = g.getAttribute('data-name') || ''
  return adcode ? { adcode, name } : null
}

function onGeoClick(e: MouseEvent) {
  const hit = pickFromEvent(e)
  if (!hit) return
  sim.setPick(hit.adcode, hit.name)
}

/** 双击 = 直接进（比「点一下再点按钮」快一档，误触代价也只是多下钻一级） */
function onGeoDbl(e: MouseEvent) {
  const hit = pickFromEvent(e)
  if (!hit) return
  void sim.drillTo({ adcode: hit.adcode, name: hit.name })
}

/* ── 杂项动作 ───────────────────────────────────────────────────────── */
function goMenu() {
  router.push('/')
}
async function onStyle(e: Event) {
  const v = (e.target as HTMLSelectElement).value
  await sim.changeMapStyle(v)
}
function onPickerCancel() {
  // 从确认弹窗进来的，退回确认；否则退回上一级地图（绝不让用户卡在空页面里）
  if (sim.path.value.length > 1) void sim.backTo(Math.max(0, sim.cursor.value))
  else sim.step.value = 'locateFailed'
}
function onWorldEntered() {
  // P1 到这里就是终点了：世界已就位（P2 起才有真正可逛的画面）
  sim.note.value = '世界已就位（P1 到此为止，人物/事件在后续阶段接）'
}

onMounted(() => {
  void start()
})
</script>

<style scoped>
.ws-top {
  display: flex;
  align-items: center;
  gap: 0.5em;
  flex-wrap: wrap;
  padding: 0.7em 0.9em 0.4em;
}
.ws-brand {
  display: flex;
  align-items: center;
  gap: 0.4em;
  font-size: 1.04em;
  font-weight: 700;
}
.ws-brand__ico {
  font-size: 1.1em;
}
.ws-brand__stage {
  font-weight: 400;
}
.ws-top__crumb {
  margin-left: 0.4em;
  min-width: 0;
}
.ws-spacer {
  flex: 1;
}
.ws-sel {
  font: inherit;
  color: var(--ws-fg);
  background: var(--ws-panel-2);
  border: 1px solid var(--ws-border);
  border-radius: var(--ws-radius-sm);
  padding: 0.3em 0.5em;
}
.ws-srcbar {
  display: flex;
  align-items: center;
  gap: 0.5em;
  flex-wrap: wrap;
  padding: 0 0.9em 0.4em;
  font-size: 0.86em;
  color: var(--ws-fg-dim);
}
.ws-srcbar__now {
  color: var(--ws-fg);
  font-weight: 600;
}
.ws-srcbar__hint,
.ws-srcbar__note {
  opacity: 0.9;
}
.ws-stage {
  position: relative;
  flex: 1;
  min-height: 0;
  margin: 0 0.9em;
}
.ws-center {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
}
.ws-mapwrap,
.ws-neighwrap {
  position: relative;
  height: 100%;
  min-height: 0;
}
.ws-geo__inner {
  width: 100%;
  height: 100%;
}
.ws-geo__empty {
  flex-direction: column;
  gap: 0.6em;
}
.ws-mapmask {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--ws-bg) 55%, transparent);
  backdrop-filter: blur(1.5px);
  -webkit-backdrop-filter: blur(1.5px);
  border-radius: var(--ws-radius);
}
.ws-modal {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0.6em;
  z-index: 4;
}
.ws-locatefail {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.8em;
  width: min(26em, 92vw);
  padding: 1.2em;
  text-align: center;
}
.ws-locatefail__ops {
  display: flex;
  gap: 0.6em;
  flex-wrap: wrap;
  justify-content: center;
}
.ws-locatefail__tip {
  font-size: 0.84em;
  color: var(--ws-fg-dim);
  line-height: 1.6;
}
.ws-foot {
  display: flex;
  flex-direction: column;
  gap: 0.4em;
  padding: 0.5em 0.9em 0.9em;
}
.ws-foot__hint {
  font-size: 0.86em;
  color: var(--ws-fg-dim);
}
.ws-foot__ops {
  display: flex;
  gap: 0.5em;
  flex-wrap: wrap;
}
.ws-foot__err {
  margin-top: 0.2em;
}
</style>
