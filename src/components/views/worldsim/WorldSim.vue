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
    </header>

    <!-- ── 定位来源 / 上次位置：如实告诉用户「这个位置是怎么来的」──────── -->
    <div v-if="locLabel || restored" class="ws-srcbar">
      <span class="ws-tag">{{ locLabel || '—' }}</span>
      <span v-if="areaLabel" class="ws-srcbar__now">当前：{{ areaLabel }}</span>
      <span v-if="restored" class="ws-srcbar__hint">（首次引导已走过，直接进上次位置）</span>
      <span v-if="note" class="ws-srcbar__note">{{ note }}</span>
      <!-- 「重新引导」是低频且带破坏性的动作（会清掉「上次位置」重走一遍），
           放在顶栏会把它挤到第二行（手机上实测 8 个元素超宽约 64px）；
           挪到这条信息行的最右边：同一条视觉带、不抢主操作，顶栏因此能保持**一行**
           —— 机主要求「手机跟电脑版一样」，桌面就是一行。 -->
      <button
        class="ws-btn ws-btn--ghost ws-srcbar__reset"
        type="button"
        title="清掉「上次位置」，从加载动画重新走一遍引导"
        @click="restart"
      >
        {{ t('worldsim.restart') }}
      </button>
    </div>

    <!-- ── 舞台 ─────────────────────────────────────────────────────── -->
    <main class="ws-stage">
      <!-- ① 初始化：加载动画「正在展开世界…」 -->
      <section v-if="step === 'boot'" class="ws-center">
        <!-- delay=0：这是首屏（底下没有任何内容可看），一进来就该有东西，不是「等 180ms」的场景 -->
        <WsLoading
          variant="init"
          :delay="0"
          :text="t('worldsim.loader.init.text')"
          :sub="t('worldsim.loader.init.sub')"
          :hint="t('worldsim.loader.init.longHint')"
        />
      </section>

      <!-- ② 定位中 -->
      <section v-else-if="step === 'locating'" class="ws-center">
        <WsLoading
          variant="locate"
          :text="t('worldsim.loader.locate.text')"
          :sub="gpsHint"
          :hint="t('worldsim.loader.locate.longHint')"
        />
      </section>

      <!-- ③ 定位失败 / 被拒：手动选城市 + 用 IP 估测，两条路都给 -->
      <section v-else-if="step === 'locateFailed'" class="ws-center">
        <div class="ws-card ws-locatefail">
          <WsLoading variant="locate" size="sm" :text="t('worldsim.loader.locateFailed.text')" :sub="note" />
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

      <!-- ④ 地图（国/省/市/区县）—— v-html 注入后端 SVG，点击靠事件委托
           手势（平移/捏合/滚轮/复位）只作用在 .ws-geo__pan 这一层变换容器上，
           SVG 本体不动 —— 详见 useWorldSimGestures 的说明。

           ⚠️ 这里**不能写裸的 `v-else`**（曾经就是）：`v-else` 只能判断"上面那串条件都不成立"，
           而它上面那串只覆盖 boot/locating/locateFailed —— 于是进入 `neighborhood` 之后
           它**照样渲染**，和 ⑦ 的小区图叠在一起：区县图占满整个舞台、小区图被挤到最下面
           只剩一条缝（真机复现：面包屑已经到「小区」了，屏幕上还是「区县 · 1 个区划」）。
           所以必须显式排除 neighborhood。`confirm` / `manual` 是**浮在地图上的弹层**，
           要保留地图打底，故不排除它们。 -->
      <section v-else-if="step !== 'neighborhood'" class="ws-mapwrap">
        <!-- `ws-sea-*`：舞台底色 = 当前**地图风格**的海色。
             地图是横的（≈1.32）、容器是竖的，用 contain 等比缩放必然在上下留两条；
             只有底色与 SVG 里的海色**逐字一致**，那两条才不可见（否则像地图被挤在中间）。
             ⚠️ 这三个色值必须与后端 `render_geo::style_of()` 的 `bg` 保持一致。 -->
        <div ref="geoHost" class="ws-geo" :class="`ws-sea-${style}`" @click="onGeoClick">
          <div
            ref="geoPan"
            class="ws-geo__pan"
            :class="{ 'is-drag': gsDragging || gsInstant }"
            :style="gsStyle"
          >
            <div v-if="stageMarkup" class="ws-geo__inner" v-html="stageMarkup" />
            <div v-else-if="!busy" class="ws-center ws-geo__empty">
              <div class="ws-note ws-note--err">这一级的地图没画出来</div>
              <button class="ws-btn" type="button" @click="retryStage">重试</button>
            </div>
          </div>
        </div>

        <!-- 缩放/复位：鼠标派与「不想捏合」的人的明路（也顺带让人看见缩放是有上限的） -->
        <div class="ws-zoomctl">
          <button type="button" title="放大" :disabled="gsScale >= 3.99" @click="zoomBy(1.35)">＋</button>
          <button type="button" title="缩小" :disabled="gsScale <= 0.61" @click="zoomBy(1 / 1.35)">－</button>
          <button type="button" title="复位（也支持双击地图 / 双指双击）" @click="gsReset(false)">⟲</button>
        </div>

        <!-- 区域切换 / 首次加载的加载动画（盖在舞台上，不销毁已有的图）
             不给进度条：这张图是后端现画的，前端算不出百分比 —— 算不出来就**不装**确定进度 -->
        <div v-if="busy" class="ws-mapmask">
          <WsLoading
            variant="map"
            :text="busyText || t('worldsim.loader.map.text')"
            :sub="t('worldsim.loader.map.sub')"
            :hint="t('worldsim.loader.map.longHint')"
          />
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

      <!-- ⑦ 小区地图（终点）
           ⚠️ 用 `v-if` 而不是 `v-else-if`：它上面紧邻的是 ⑥ 的 `v-if="step === 'manual'"`，
           写成 `v-else-if` 会把两条本不相干的链悄悄接在一起（读的人很难发现），
           而它的条件本身是自足的（step === 'neighborhood'），没必要挂靠。 -->
      <section v-if="step === 'neighborhood'" ref="neighWrap" class="ws-neighwrap">
        <WsDistrict
          ref="districtRef"
          :area="areaLabel || '未知区域'"
          :map-style="style"
          @back="backTo(path.length - 1)"
          @done="onWorldEntered"
        >
          <!-- P2-1：人物层钉在地图上（跟着手势一起缩放平移，见 WsDistrict 的 pin 插槽说明）
               `zoom` = 小区图自己的手势倍率（由 WsDistrict defineExpose 出来）：
               头像位置跟着地图缩放走，但尺寸始终是屏幕上那么大（不然放大 4× 会变成大饼）。
               P4-3：交通工具标记也铺在这一层（同一个手势变换容器里才会跟着地图动）。 -->
          <template #pin>
            <WsAvatarLayer
              :placed="placedActors"
              :grid="WS_GRID"
              :selected-id="wsPanel.targetId.value"
              :me-name="meName"
              :zoom="districtScale"
              :drag="true"
              :drag-pin="dragPin"
              @pick="onActorPick"
              @dragstart="onDragStart"
              @dragmove="onDragMove"
              @dragend="onDragEnd"
            />
            <WsVehicleMark
              v-for="t in movingTrips"
              :key="t.id"
              :trip="t"
              :grid="WS_GRID"
              :zoom="districtScale"
            />
            <!-- P5-2：事件气泡（bubble 通道）。铺在同一层里才会跟着地图平移缩放；
                 位置不在这里算（`WsEventBubble` 用 letterboxOf + 角色的 px/py 自己算）。
                 P5-5：低性能档下调同时显示的气泡上限、入场动画退化成纯淡入。 -->
            <WsEventBubble
              :bubbles="wsEventBubbles"
              :placed="placedActors"
              :grid="WS_GRID"
              :zoom="districtScale"
              :me-name="meName"
              :max="bubbleMax"
              :low="perfLow"
            />
          </template>
        </WsDistrict>

        <!-- P4-2：行程卡（浮在左下角）。只在真有行程时出现，绝不占着地方。 -->
        <WsTripCard
          v-if="shownTrip"
          floating
          :trip="shownTrip"
          :speedup="trips.speedup.value"
          :busy="tripBusy"
          @speedup="onTripSpeedup"
          @cancel="onTripCancel"
        />
      </section>

      <!-- ⑦.5 地图上的小人浮标（P2-1 的入口）：一眼看到「地图上有人」，
           点它也能直接把「自己」的面板打开（不依赖刚好点中那个小圆头像） -->
      <div v-if="step === 'neighborhood'" class="ws-people">
        <button class="ws-chip" type="button" @click="loadActors(true)">👥 {{ placedActors.length }}</button>
        <button class="ws-chip" type="button" :title="t('worldsim.me.title')" @click="openMe">🙂 {{ meName }}</button>
      </div>

      <!-- ⑦.6 P5-2：事件流面板（含三通道开关）。钉在右下角、小人浮标上方；
           开关的语义就是「这类事件要不要打扰我」，所以它跟事件列表在同一处。 -->
      <WsEventFeed
        v-if="step === 'neighborhood'"
        :events="wsEventItems"
        :channels="wsEventChannels"
        :pending="wsEventPending"
        :pending-after="wsEventPendingAfter"
        :speech-hint="wsEventSpeechHint"
        :now-ms="wsEventNowMs"
        :reason="wsEventReason"
        :next-ok-in-secs="wsEventNextOk"
        :supported="wsEventSupported"
        :running="wsEventRunning"
        :error="wsEventError"
        @toggle="wsEvents.setChannel"
        @open="onEventPanelOpen"
        @refresh="onEventPanelOpen"
      />

      <!-- ⑧ P2-3 / P2-4：角色面板（含立绘侧边栏）/ 自己的面板。
           挂在 .ws-stage 里而不是 <main> 里：定位是相对 stage 的，
           这样面板底部不会盖住底部动作条（「回到区县 / 进入这个世界」还要能点）。 -->
      <WsCharPanel
        v-if="wsPanel.open.value && !wsPanel.isMe.value && currentActor"
        :actor="currentActor"
        :data="actors"
        :area-text="areaLabel"
        :narrow="wsPanel.narrow.value"
        :open="wsPanel.open.value"
        :portrait-open="wsPanel.portraitOpen.value"
        :current-role-id="currentRoleId"
        @close="wsPanel.closePanel"
        @portrait="wsPanel.togglePortrait"
        @goto-chat="onGotoChat"
        @quick="onQuick"
        @direct="onDirect"
      />
      <WsMePanel
        v-else-if="wsPanel.open.value && wsPanel.isMe.value"
        :data="actors"
        :placed="placedActors"
        :grid="WS_GRID"
        :area-text="areaLabel"
        :narrow="wsPanel.narrow.value"
        :open="wsPanel.open.value"
        :selected-id="wsPanel.targetId.value"
        :me-name="meName"
        @close="wsPanel.closePanel"
        @pick="onActorPick"
        @refresh="actors.loadTimeWeather()"
      />
    </main>

    <!-- toast 容器（快捷动作「即将上线」这类提示必须有可见反应，绝不静默） -->
    <div class="ws-toasts" aria-live="polite">
      <div v-for="m in toastItems" :key="m.id" class="ws-toast" :class="`ws-toast--${m.kind}`">
        {{ m.text }}
      </div>
    </div>

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
/* ═══ 【L-SYuki 线专用 · 提 PR 时整段剔除（这 2 行）】 ═══════════════════════
 * B 部分：浏览器手势抑制（touch-action / overscroll-behavior / 长按选择）。
 * 机主的硬要求：抑制浏览器手势**不能进 PR**，所以它单独一个文件、单独一次 import，
 * 剔的时候把这两行删掉 + 删掉那个文件即可（详见文件头与 docs/world-map/09 的
 * 「九、PR 排除清单」，那里有精确行号）。
 * 注意：A 部分（地图手势本体，useWorldSimGestures.ts）**是功能，要留在 PR 里**。 */
import '@/assets/styles/worldsim-gesture-lock.css'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import WsLoading from './WsLoading.vue'
import WsCrumb from './WsCrumb.vue'
import WsConfirm from './WsConfirm.vue'
import WsPicker from './WsPicker.vue'
import WsDistrict from './WsDistrict.vue'
// P2：人物层 + 两个面板（面板与头像层都是这一页独有的，懒加载没有意义，直接静态 import）
import WsAvatarLayer from './WsAvatarLayer.vue'
import WsCharPanel from './WsCharPanel.vue'
import WsMePanel from './WsMePanel.vue'
// P4-2 / P4-3：行程卡 + 地图上的交通工具（样式由组件自己 import worldsim-trip.css）
import WsTripCard from './WsTripCard.vue'
import WsVehicleMark from './WsVehicleMark.vue'
// P5-2：事件通知三通道的地图气泡 + 事件流面板（数据层在 useWorldEvents）
import WsEventBubble from './WsEventBubble.vue'
import WsEventFeed from './WsEventFeed.vue'
import { wsToast, useWsToast } from './wsToast'
import type { PlacedActor } from './wsActors'
import {
  DEFAULT_CELL_M,
  dropGridAt,
  estimateTrip,
  facilityAt,
  gridPinAt,
  metersParts,
  minutesOf,
  useWsIntervene,
  type DropCtx,
} from './wsIntervene'
// P5-5：性能档位（低端机自动降级 + 手动覆盖）。判定/持久化/fps 表都在这个文件里。
import { bubbleMaxOf, quantizeZoom, useWsPerf } from './wsPerf'
import { geoLevelOf, levelLabel } from './wsGeo'
// P3-3 的接线点：把「现在在哪、谁站在哪」推给 Rust 的 MapRuntime。
// **不推的后果很隐蔽**：`world_sim_enabled()` 恒 false → 位置指令剥离器不启用、
// 注入摘要恒为空串（角色不知道自己在哪）。见该文件头部的说明。
import { clearRuntime, pushRuntime } from './wsRuntimePush'
import { useElementSize, useWorldSimGeo, useWorldSimTheme } from '@/composables/useWorldSimGeo'
import { useWorldSim } from '@/composables/useWorldSim'
import { useWorldSimGestures } from '@/composables/useWorldSimGestures'
import { useWsActors } from '@/composables/useWsActors'
import { useWsPanel } from '@/composables/useWsPanel'
import { useWorldTrips } from '@/composables/useWorldTrips'
import { popupKindOf, useWorldEvents } from '@/composables/useWorldEvents'
import { useGameStore } from '@/stores/modules/game'

const router = useRouter()
const { t } = useI18n()

/* 皮肤 + 取图器 + 状态机 */
/* P5-5：性能档位先建（皮肤与人物层都要读它 —— `ws-perf-low` 的类、
   气泡上限、zoom 量化、错开精度必须来自**同一个判定**，不能各判各的）。 */
const perf = useWsPerf()
const perfLow = perf.low
const theme = useWorldSimTheme({ lowPerf: perfLow })
const geo = useWorldSimGeo()
const geoHost = ref<HTMLElement | null>(null)
const geoPan = ref<HTMLElement | null>(null)
// 舞台尺寸就量 .ws-geo 本身：后端 SVG 的宽高必须与它 1:1，字号才不会在手机上缩没
const { sizeForBackend } = useElementSize(geoHost, { w: 900, h: 620 })

// 地图手势（单指拖 / 双指捏 / 滚轮 / 双击复位）：只改变换容器的 transform，不碰 SVG 本体
const {
  scale: gsScale,
  dragging: gsDragging,
  instant: gsInstant,
  panStyle: gsStyle,
  reset: gsReset,
  zoomBy,
  shouldSuppressClick,
} = useWorldSimGestures({ target: geoHost, content: geoPan })
const sim = useWorldSim({ geo, getViewport: () => sizeForBackend.value, t })

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

/* ══ P2：地图上的「人」+ 面板 ══════════════════════════════════════════════
 * 数据（谁在哪、头像、日程）在 useWsActors；面板的开关/选中在 useWsPanel。
 * 这一层只做两件事：把人物层插进地图、把面板接上事件。
 * ⚠️ 立绘（≈73MB/张）的生命周期只归 WsCharPanel + useWsPortrait 管，
 *    这里**不碰**，也就不可能被这一页误加载。 */
const gameStore = useGameStore()
/** 小区图的网格边长：与 WsDistrict 的 SKETCH_SIZE / 后端 sketch 一致 */
const WS_GRID = 28
const wsPanel = useWsPanel()
const actors = useWsActors({ grid: ref(WS_GRID), areaLabel: sim.areaLabel, lowPerf: perfLow })
const { placed: placedActors } = actors
/** 面板当前对着的那个人（面板关着 / 对着自己时是 null） */
const currentActor = computed<PlacedActor | null>(() => {
  const id = wsPanel.targetId.value
  if (!id || id === 'me') return null
  return placedActors.value.find((a) => a.id === id) || null
})

/** 玩家显示名（gameStore 里没有就退回 i18n 的「我」） */
const meName = computed(() => gameStore.userName || t('worldsim.actor.me'))
/** 当前正在对话的角色（决定「去找他聊聊」是直连还是提醒会切人） */
const currentRoleId = computed(() => Number(gameStore.currentInteractRoleId ?? gameStore.mainRoleId) || 0)

const { items: toastItems, clear: clearToasts } = useWsToast()

/** 小区图的手势倍率（WsDistrict 用 defineExpose 露出来）—— 头像层据此抵消缩放 */
const districtRef = ref<{ gsScale?: number } | null>(null)
/**
 * 传给头像层的倍率（P5-5）。
 *
 * ⚠️ 这里**必须量化**：`gsScale` 在捏合时逐帧变化，而头像/车辆/气泡每一个都要
 * 按它重算 style（`scale(1/zoom)`）—— 20 个人就是 20 次/帧的 Vue 更新 + DOM 写入，
 * 这正是「地图上有人就卡」的主因。量化后同一档内 props 不变 → computed 不重算、
 * DOM 不重写；低档步长更大（0.25），视觉误差 ≤ 半个步长，肉眼看不出。
 * 手势状态本身不受影响（`gsScale` 原样用于缩放按钮的禁用判定）。
 */
const districtScale = computed(() => quantizeZoom(Number(districtRef.value?.gsScale) || 1, perfLow.value))

/** P5-5：低档同时显示的气泡上限（高档沿用数据层的 WS_BUBBLE_MAX = 4） */
const bubbleMax = computed(() => bubbleMaxOf(perfLow.value))

/**
 * P4-4：拖拽落点换算要用的**小地图包裹层**。
 *
 * 为什么不去找 WsDistrict 多要 `tx/ty/舞台元素`：那会逼着那个组件多暴露三个内部状态，
 * 而页面自己就能算 —— 变换容器（`.ws-neigh__pan`）的 `getBoundingClientRect()` 返回的是
 * **变换之后**的视觉矩形：宽度里已经含了缩放、left/top 里已经含了平移。
 * 于是 `scale = rect.width / pan.offsetWidth`、原点取 `rect.left/top` 就够了，
 * 不必知道手势内部存了什么（`WsDistrict` 的 expose 保持原来那一个 `gsScale`）。
 */
const neighWrap = ref<HTMLElement | null>(null)

/* ══ P4：行程（谁在路上）════════════════════════════════════════════════════
 * 数据（轮询 + 按时间戳插值 + `world_map:trip` 事件 + 位置回推）全在 useWorldTrips；
 * 这里只做三件事：把车铺进地图、把行程卡摆出来、把到达事件接上 toast。
 * ⚠️ 位置回推的「让位」纪律：行程中的角色由 trips 推插值位置，P2 的静态推送必须让开，
 *    否则两边互相覆盖，表现是「角色走两步被拉回去」。见 syncRuntime 里的 filter。 */
const trips = useWorldTrips({
  // ⚠️ 参数名不能叫 `t`（会把上面 useI18n 的 `t` 遮蔽掉，vue-tsc 会报 TS2349）
  onArrive: (trip: { role?: string; to?: { name?: string } }) => {
    wsToast(t('worldsim.trip.arrivedToast', { name: trip.role || '', place: trip.to?.name || '' }), 'ok')
    // 到达后把 runtime 里的新位置读回来（否则 P2 重推的还是出发前的老位置，
    // 会出现「车开到了、头像还站在原地」）
    void loadActors(true)
  },
})
/** 地图上要画的那几条行程 */
const movingTrips = trips.movingTrips
/** 行程卡显示哪一条：优先当前角色的 active，没有就第一条在途的 */
const shownTrip = computed(() => trips.active.value || movingTrips.value[0] || null)
/** 拨开关/取消进行中（防连点） */
const tripBusy = ref(false)

async function onTripSpeedup(fast: boolean) {
  tripBusy.value = true
  try {
    const r = await trips.setSpeedup(fast) // 内部用后端权威值覆盖 + 重读，不做乐观更新
    wsToast(
      fast
        ? t('worldsim.trip.fastOn', { n: r.speedup })
        : t('worldsim.trip.fastOff'),
      r.ok ? 'ok' : 'warn',
    )
  } finally {
    tripBusy.value = false
  }
}

async function onTripCancel() {
  tripBusy.value = true
  try {
    const r = await trips.cancel()
    // 没有行程时后端返回 ok:false（不是异常），如实提示，绝不静默
    wsToast(
      r.ok
        ? t('worldsim.trip.cancelled')
        : t('worldsim.trip.nothingToCancel', { reason: r.message || '' }),
      r.ok ? 'ok' : 'warn',
    )
  } finally {
    tripBusy.value = false
  }
}

/* ── P3-3：把地图状态推给 Rust（AI 才知道「我在哪、附近有什么」）───────────
 * 推三样东西：`scene`（有没有它 = 世界模拟开没开）、`actors`（谁站在哪）、
 * `current_role`（现在是谁在说话 —— `get_my_location` / `move_to` 靠它认人）。
 * 纪律：**别每帧推**。只在「名单变了 / 进了小区图」时推一次，其余交给后端缓存。 */

/** 正在对话的角色名（= `settings.yml` 的 `ai_name`，也是 runtime 里 actors 的键） */
const currentRoleName = computed(() => {
  const id = currentRoleId.value
  const r = id ? (gameStore.gameRoles as Record<number, { roleName?: string } | undefined>)[id] : undefined
  return String(r?.roleName || '').trim()
})

/* ══ P5-2：现实事件（事件通知三通道）════════════════════════════════════════
 * 数据层（tick 轮询 + `world_map:event` 广播 + 三通道开关 + 记忆交接）全在
 * `useWorldEvents`；这里只做三件事：
 *   ① 把当前说话的角色名传进 tick（`role`）—— 事件挂到正确的人头上靠它；
 *   ② popup 通道：抽中时用 `wsToast` 弹一条（配色按事件类别）；
 *   ③ 把开关/面板的交互转回数据层。
 * bubble 通道由 composable 自己写 `bubbles`（组件 `WsEventBubble` 只负责画），
 * speech 通道**前端不做额外事**（后端已把事件注进「最近：…」，见 composable 文件头）。
 * ⚠️ 生命周期：进到小区图才 `start()`（引擎要 `scene` 才有意义），离开本页 `stop()`。 */
const wsEvents = useWorldEvents({
  role: currentRoleName,
  onFired: (e) => {
    // 关掉这一路就该安静（气泡那一路由 composable 内部判，这里只管提示条）
    if (!wsEvents.channels.value.popup) return
    wsToast(e.popup || e.event?.title || '', popupKindOf(e.event?.category))
  },
})
// 解构出来给模板用：模板只对**顶层** ref 自动解包，`wsEvents.xxx` 这种嵌套的不会解
const {
  events: wsEventItems,
  bubbles: wsEventBubbles,
  channels: wsEventChannels,
  pendingLines: wsEventPending,
  pendingAfter: wsEventPendingAfter,
  speechHint: wsEventSpeechHint,
  nowMs: wsEventNowMs,
  lastReason: wsEventReason,
  nextOkInSecs: wsEventNextOk,
  lastError: wsEventError,
  supported: wsEventSupported,
  running: wsEventRunning,
} = wsEvents

/** 面板展开 / 点刷新：两个自然检查点，顺手把事件历史与待写记忆各拉一次 */
function onEventPanelOpen() {
  void wsEvents.refreshHistory()
  void wsEvents.drainMemory()
}

/** 组一次 patch 并推上去（幂等；失败只记日志，绝不打断界面） */
async function syncRuntime(reason: string): Promise<void> {
  if (!placedActors.value.length && !areaLabel.value) return
  const r = await pushRuntime({
    area: areaLabel.value,
    // scene.place 只在地点确实比 area 更具体时才填（area 已经是「…·东山口」了，
    // 再补一个同名 place 只会在注入里变成「…·东山口·东山口里」）
    adcode: leaf.value?.adcode,
    actors: placedActors.value.filter((a) => !trips.isOwned(a.name)),
    meSource: locSource.value || undefined,
    currentRole: currentRoleName.value || undefined,
  })
  if (!r.ok) console.warn('[worldsim] 地图状态回推失败（AI 上下文会少一段）：', reason, r.error)
}

/** 名单/坐标变了就补齐（同一 tick 内多次变化只推一次） */
let syncTimer: number | null = null
function scheduleSyncRuntime(reason: string) {
  if (syncTimer !== null) window.clearTimeout(syncTimer)
  syncTimer = window.setTimeout(() => {
    syncTimer = null
    void syncRuntime(reason)
  }, 120)
}

/** 首次真正进到小区图时装配「地图上的人」（+ 时间天气） */
let actorsLoaded = false
async function loadActors(force = false) {
  if (actorsLoaded && !force) return
  actorsLoaded = true
  await actors.load()
  void actors.loadTimeWeather()
  // 名单刚装配好 → 立刻推一次（顺序很重要：先 load 再推，否则推上去的是空名单）
  await syncRuntime('loadActors')
}

/** 点头像/小地图里的某个人 → 打开对应面板 */
function onActorPick(a: PlacedActor) {
  wsPanel.openPanel(a.isMe ? 'me' : a.id)
}

function openMe() {
  wsPanel.openPanel('me')
}

/**
 * 「去找他聊聊」。
 *
 * ⚠️ 这里**故意不调用** `select_character` 命令：Rust 侧它会走 `init_game_status()`，
 * 把当前对话（line_list / 在场角色）整份重置 —— 从地图上点一下就清空聊天记录，
 * 是绝不能做的破坏性操作。所以：
 *   · 已经在跟这个角色聊 → 直接跳 /chat（零副作用）
 *   · 不是 → 先说清楚「切角色会开一段新对话」，用户确认了再跳
 */
function onGotoChat(a: PlacedActor) {
  if (!a?.roleId) {
    wsToast(t('worldsim.chat.noRole'), 'warn')
    return
  }
  if (a.roleId === currentRoleId.value) {
    void router.push('/chat')
    return
  }
  const ok = window.confirm(t('worldsim.chat.switchWarn', { name: a.name }))
  if (ok) void router.push('/chat')
}

/**
 * 快捷动作分派（P2-5：三个按钮各自的**真**行为，一处看全）。
 *
 *   · 打招呼   → 复用「去找他聊聊」那条路（跳 /chat，零副作用；见 onGotoChat）
 *   · 约他出门 → `inviteOut()`：调 `world_map_trip_start` 让**角色动身来找你**
 *   · 送礼物   → **需求未澄清**（礼物从哪来 / 送完发生什么，机主还没定）：
 *                这里只留接入点，**绝不自己发明一套礼物系统**。
 *                说明弹层由 `WsCharPanel` 打开（两个候选方案 + 「还没定」）。
 *                接入点长这样（方案定了再填）：
 *                  `world_map_trip_start` 那种 `{req}` 风格的命令，
 *                  或后端新增 `world_map_gift_send{role,item}` + 好感度/记忆写入。
 */
async function onQuick(action: string, a: PlacedActor) {
  if (action === 'hi') {
    onGotoChat(a)
    return
  }
  if (action === 'outing') {
    await inviteOut(a)
    return
  }
  if (action === 'gift') {
    /* 需求未澄清：什么都不做（面板已经弹了说明层，不会被误当成「点了没反应」） */
    return
  }
}

/* ══ P4-4：玩家干预（下指令 / 把他拖到别处）═════════════════════════════════
 * 两条入口共用同一条下游：`world_map_trip_start` → `Trip::plan` 起一条行程。
 *   · 对话指挥（聊天里说「你去便利店」）：模型吐 `⟦wm:…⟧` → Rust 侧
 *     `directive.rs` 剥离 → `move::dispatch` → 同样的 `Trip::plan`。
 *     前端**不新增任何后台能力**，这里只是把同一条下游做成地图上的可见入口。
 *   · 拖拽：屏幕坐标 → 格点 → 最近的设施 → 同一个 `world_map_trip_start`。
 * 纪律：**开关默认关**（尊重角色自主性），关着时拖拽无效但要给一次提示。 */
const { on: interveneOn } = useWsIntervene()

/** 拖动中的落点预览（信箱盒子坐标；null = 没在拖） */
const dragPin = ref<{ x: number; y: number } | null>(null)

/** 格边长：后端 runtime 里有就用它的，没有按 30 米（与 `trip::DEFAULT_CELL_M` 一致） */
const cellM = computed(() => Number(actors.runtime.value?.cell_m) || DEFAULT_CELL_M)

/** 拖拽换算上下文：变换容器的视觉矩形（含平移缩放）+ 信箱盒子尺寸 */
function dropCtx(): DropCtx | null {
  // 变换容器由页面自己查（见 neighWrap 的说明）—— 不越权访问 WsDistrict 的内部 ref
  const pan = neighWrap.value?.querySelector('.ws-neigh__pan') as HTMLElement | null
  if (!pan) return null
  const w = pan.offsetWidth
  const h = pan.offsetHeight
  const r = pan.getBoundingClientRect()
  // 量不到尺寸就不硬算（宁可提示失败，也不要按错的信箱把人挪到别处）
  if (!w || !h || !r.width || !r.height) return null
  return {
    // 视觉矩形的左上角已经含了平移，所以 tx/ty 一律给 0（换算数学见 wsIntervene.dropGridAt）
    rect: { left: r.left, top: r.top },
    scale: r.width / w,
    tx: 0,
    ty: 0,
    boxW: w,
    boxH: h,
    grid: WS_GRID,
  }
}

/** 这一次拖动里有没有已经提示过「开关没开」（避免同一次拖动弹两遍） */
let dragHinted = false

function onDragStart(a: PlacedActor) {
  dragPin.value = null
  dragHinted = false
  // 开关关着：**一进入拖拽就说清楚**（比等到松手才说体验好），并且不画落点
  // —— 拖拽在关闭状态下是无效的，画个图钉等于给假希望。
  if (!interveneOn.value) {
    wsToast(t('worldsim.intervene.offToast'), 'warn', 3800)
    dragHinted = true
    return
  }
  if (!a?.name) wsToast(t('worldsim.chat.noRole'), 'warn')
}

function onDragMove(p: { a: PlacedActor; clientX: number; clientY: number }) {
  // 开关关着：不画落点（拖完也只会得到一条「去哪儿开」的提示，别给假希望）
  if (!interveneOn.value) return
  const ctx = dropCtx()
  if (!ctx) return
  const pin = gridPinAt({ x: p.clientX, y: p.clientY }, ctx)
  dragPin.value = { x: pin.x, y: pin.y }
}

/** 「210 米 / 1.2 公里」——数字与单位都走 i18n（这里只给数） */
function distText(meters: number): string {
  const p = metersParts(meters)
  return p.km ? t('worldsim.intervene.km', { n: p.value }) : t('worldsim.intervene.m', { n: p.value })
}

/** 起一条「谁去哪」的行程（拖拽与下指令共用）。`g` 不给就让后端按地名在地图数据里找 */
async function startTripTo(a: PlacedActor, to: string, g?: { gx: number; gy: number }) {
  if (!a?.name) {
    wsToast(t('worldsim.chat.noRole'), 'warn')
    return
  }
  if (!trips.supported.value) {
    wsToast(t('worldsim.intervene.unsupported'), 'warn')
    return
  }
  tripBusy.value = true
  try {
    // `kind: 'walk'` 是**故意显式给的**：拖拽/指挥的语义是「在小区里走过去」，
    // 不显式给的话后端会按距离自动选（可能来一辆车），那样提示条里的
    // 「步行 X 分钟」就与实际不符 —— 提示与实际必须是同一个口径。
    const r = await trips.startTrip({ role: a.name, to, gx: g?.gx, gy: g?.gy, kind: 'walk', cell_m: cellM.value })
    wsToast(
      r.ok
        ? t('worldsim.intervene.started', { name: a.name, place: to })
        : t('worldsim.intervene.failed', { reason: r.message || '' }),
      r.ok ? 'ok' : 'warn',
    )
  } finally {
    tripBusy.value = false
  }
}

/** 面板里下一条「让他去某地」的指令（对话指挥那条路在地图上的可见入口） */
async function onDirect(to: string, a: PlacedActor) {
  await startTripTo(a, to)
}

/** 送礼物那个按钮的说明层由面板负责；这里不做事（见 onQuick 的注释） */

/** 拖拽松手：反查落点 → 报距离/预计时间 → 起一条「走过去」的行程 */
async function onDragEnd(p: { a: PlacedActor; clientX: number; clientY: number; moved: boolean }) {
  dragPin.value = null
  if (!p.moved) return // 没超过 4px 阈值 = 一次点击（选中由 click 那条路负责）
  // 默认关：拖拽无效，但要**说清楚去哪儿开**（绝不静默；已在 onDragStart 说过的就不再重复）
  if (!interveneOn.value) {
    if (!dragHinted) wsToast(t('worldsim.intervene.offToast'), 'warn', 3800)
    return
  }
  const ctx = dropCtx()
  if (!ctx) {
    wsToast(t('worldsim.intervene.noMap'), 'warn')
    return
  }
  const g = dropGridAt({ x: p.clientX, y: p.clientY }, ctx)
  const fac = facilityAt(actors.runtime.value?.facilities, g.gx, g.gy)
  // 落到设施附近就报设施名；空地上就如实说「移动到这里」（先用占位，不编地名）
  const to = fac?.name || t('worldsim.intervene.moveHere')
  const est = estimateTrip({ gx: p.a.gx, gy: p.a.gy }, g, cellM.value)
  // ⚠️ 先报「多远 / 走多久」再起程：需求原文「拖到远处要提示距离/预计时间，不要静默瞬移」。
  //    行程本身就是「走过去」（地图上会画车、行程卡上有进度），所以不存在瞬移。
  wsToast(
    t('worldsim.intervene.estimate', { name: p.a.name, place: to, dist: distText(est.meters), min: minutesOf(est.secs) }),
    'info',
    3400,
  )
  await startTripTo(p.a, to, g)
}

/**
 * 约他出门（P2-5）。
 *
 * **语义选择**：让**角色动身到玩家所在的位置**（不是玩家自己跑过去）。理由两条：
 *   ① 玩家的移动：`trip_start` 的 role 认不出玩家（`resolve_origin` 对未知 role 会
 *      退回 `me`），确实能跑起来，但行程的位置回推是**以 role 为键**写进 runtime
 *      `actors` 的 → 会凭空多出一个「我」这个角色，污染 AI 上下文与事件引擎的角色表；
 *   ② 「约」的语义本来就是发起邀请（对方行动），而玩家自己动身 = 「去找他」，
 *      那件事已经有「去找他聊聊」那条路了。
 * 例外：如果玩家在对话里被问「你能来吗」，那是模型自己用 ⟦wm:⟧ 指令人物动 —— 不归这里管。
 */
async function inviteOut(a: PlacedActor) {
  if (!a?.name) {
    wsToast(t('worldsim.chat.noRole'), 'warn')
    return
  }
  const me = placedActors.value.find((x) => x.isMe)
  if (!me) {
    wsToast(t('worldsim.action.outingNoMe'), 'warn')
    return
  }
  const to = String(me.place || '').trim() || t('worldsim.action.outingHere')
  await startTripTo(a, to, { gx: me.gx, gy: me.gy })
}

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
  // 刚拖过地图（含双指缩放）：手指离开后浏览器会**补发**一发 click，
  // 不拦就会变成「拖完还顺带选中/下钻一个区划」——最难查的那类 bug。
  if (shouldSuppressClick()) return
  const hit = pickFromEvent(e)
  if (!hit) return
  sim.setPick(hit.adcode, hit.name)
}

// 说明：双击原来是「直接下钻」，现在按机主的要求改成**手势复位**（useWorldSimGestures 里
// 统一处理，鼠标双击 + 触屏双击/双指双击都算）。下钻仍然有两条路：
// 单击区划选中 → 底部「进入 XXX」按钮；或点面包屑。
// 换级/换区县时把缩放平移复位（不然从省级放大着钻到区县，画面还停在那个放大倍率上）
watch(
  () => sim.stage.value?.adcode,
  () => gsReset(false),
)

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
  // P2：真正进到小区图 → 把「地图上的人」装配出来（只装配一次，点浮标可刷新）
  void loadActors()
}

// 走到小区这一步就装配人物（面包屑回退再回来时不再重复装配 —— 数据没变）
watch(
  () => sim.step.value,
  (s) => {
    if (s === 'neighborhood') {
      void loadActors()
      // P5-2：进到小区图才给事件引擎打火（它要 `scene` 才有意义；start 幂等）
      wsEvents.start()
      // P5-5：性能档位在这里收口 —— 启动 fps 表（仅在用户开了显示时）+
      // 补一次自动采样（核数/内存/实测帧率三信号，只降不升，结果落 localStorage）。
      // 为什么等到这一步：采样必须在**地图真的在跑**的时候做，boot/定位阶段
      // 本来就有一堆首屏开销，那时候测出来的帧率会把好机器误判成低端机。
      perf.bootstrap()
    }
    // 离开小区图时把面板收掉：立绘的 73MB 必须在离开那一刻还回去
    if (s !== 'neighborhood' && wsPanel.open.value) wsPanel.closePanel()
  },
)

// 人物名单/坐标变化（换角色、走位、重算散点）→ 补齐 runtime。
// 用 deep 会在地图手势的每一帧触发，所以这里只看「个数 + 关键坐标」这个指纹。
watch(
  () => placedActors.value.map((a) => `${a.id}:${a.gx},${a.gy},${a.place}`).join('|'),
  () => scheduleSyncRuntime('actors-changed'),
)

// 行政区链路变了（面包屑回退/换了区县）也要推：scene.area 是注入的第一行
watch(
  () => `${areaLabel.value}|${leaf.value?.adcode || ''}`,
  () => scheduleSyncRuntime('area-changed'),
)

/* ── 画布尺寸要真的传下去（否则地图会被整体缩小 + 上下留大片空白）─────────────
 * 现象（真截图量的）：容器 407×759，但后端拿到的是**兜底** 900×620 →
 * SVG 按 900 宽排版、再被 `preserveAspectRatio: meet` 缩到 407 → **缩放 45%**：
 *   · 字缩到 45%，手机上几乎看不清（省名只有 4~5px）
 *   · 内容只占中间 ~280px 高，**上下各留 ~240px 空白**，屏幕白扔三分之一
 * 为什么会这样：`start()` 在 `onMounted` 里跑，那一刻舞台（`<section v-else>` 的
 * `.ws-geo`）**还没挂载** → `useElementSize` 量不到 → `sizeForBackend` 回落到兜底；
 * 等舞台真出现、量到 407×759 时，图已经按 900×620 画好并缓存了，**不会再取一次**。
 * 修法：视口尺寸**真的变了**就按新尺寸重取当前这一级（`retryStage` 用的就是当前视口；
 * `loadStage` 的缓存键本来就含 w/h，所以换尺寸 = 新的缓存项，不会串图）。
 * 只在已进入地图阶段时重取，并且防抖 250ms（旋屏/软键盘弹出会连续触发）。 */
let sizeResizeTimer: number | null = null
watch(
  () => `${sizeForBackend.value.w}x${sizeForBackend.value.h}`,
  (now, before) => {
    // 首次不触发：那时地图还没开始取（`before` 是兜底值，取了也是白取）
    if (!before || now === before) return
    // 只在**真的已经有一张图**的时候重取：引导/定位阶段本来就没图，重取是白跑一次网络
    if (!sim.stage.value) return
    if (sizeResizeTimer !== null) window.clearTimeout(sizeResizeTimer)
    sizeResizeTimer = window.setTimeout(() => {
      sizeResizeTimer = null
      void retryStage()
    }, 250)
  },
)

onBeforeUnmount(() => {
  // 离开页面：面板关掉（立绘随之释放）+ 清掉还在排队的 toast 定时器
  wsPanel.closePanel()
  clearToasts()
  if (syncTimer !== null) window.clearTimeout(syncTimer)
  if (sizeResizeTimer !== null) window.clearTimeout(sizeResizeTimer)
  // P5-2：卸载前把待写记忆收一遍（自然检查点），再停掉 tick 轮询、广播订阅与气泡
  void wsEvents.drainMemory()
  wsEvents.stop()
  // P5-5：fps 表也是 rAF —— 页面走了就必须停（与 useWorldTrips / WsTripCard 同款纪律）
  perf.teardown()
  // ⚠️ 必须清：注入的开关就是「runtime 里有没有 scene」，
  // 不清的话回到聊天页会继续带着上次的地图上下文跟模型说话。
  void clearRuntime()
})

onMounted(() => {
  void start()
})
</script>

<style scoped>
.ws-top {
  display: flex;
  align-items: center;
  gap: 0.5em;
  /* ⚠️ 曾经是 `wrap`：手机（430px）上 8 个元素放不下 → 折成两行，跟桌面对不上。
   * 机主的要求是「手机做成跟电脑版一样」——桌面就是一行。现在：① 低频的「重新引导」
   * 挪到下面那条信息行；② 这里改成 nowrap + 子项可收缩（min-width:0）；
   * ③ 极窄屏（<340px）兜底横向滚动，宁可滑也不要折行（折行会把整个舞台往下推）。 */
  flex-wrap: nowrap;
  overflow-x: auto;
  scrollbar-width: none;
  padding: 0.7em 0.9em 0.4em;
}
.ws-top::-webkit-scrollbar { display: none; }
.ws-top > * { min-width: 0; flex-shrink: 0; }
/* 只有品牌名允许被压缩（其余是按钮，压了会难点） */
.ws-top .ws-brand { flex-shrink: 1; overflow: hidden; }
.ws-top .ws-brand__name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
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

/* ══ 宽扁屏幕（横屏手机 / 超宽窗口）：chrome 收紧 + 底栏改悬浮 ══════════════════
 * 为什么需要单独一档（实测数据，915×412 横屏 20:9）：
 *   顶栏 83 + 信息行 30 + 底栏 82 = **195px，占屏高 47%**；留给地图的舞台只剩 889×241，
 *   后端按 contain 一缩，中国地图只有约 318px 宽、左右各空 300px。
 * 判据用 **min-aspect-ratio** 而不是 max-width：竖屏平板（768×1024）不该走这一档，
 * 而 915×412 的手机该走 —— 区别在**长宽比**，不在宽度。
 * 这一档做两件事：① 顶栏/信息行压到最紧（省约 50px）；② 底栏**脱离文档流浮在底部**
 * （再省 82px）。舞台因此从 241px 涨到约 370px，地图宽度约 +53%。 */
@media (min-aspect-ratio: 2/1) {
  .ws-top { padding: 0.28em 0.6em 0.1em; gap: 0.35em; }
  .ws-top .ws-btn { padding: 0.15em 0.5em; }
  .ws-brand__ico { display: none; }   /* 横屏省地方：图标让位给标题文字 */
  .ws-srcbar { padding: 0 0.6em 0.15em; gap: 0.35em; }
  .ws-stage { margin: 0 0.6em 0.4em; }
  /* 底栏浮起来：它只承载「主按钮 + 定位提示」，压在图上比占一条 82px 的带子划算 */
  .ws-foot {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 6;
    flex-direction: row;
    align-items: center;
    gap: 0.6em;
    padding: 0.3em 0.7em 0.5em;
    /* 渐隐托底：按钮压在浅蓝海面上也要看得清 */
    background: linear-gradient(to top, var(--ws-bg) 55%, transparent);
    pointer-events: none;   /* 空隙不许吃掉地图手势 */
  }
  .ws-foot > * { pointer-events: auto; }
  .ws-foot__hint { flex: 1; min-width: 0; }
  .ws-foot__ops { flex: 0 0 auto; }
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
  /* 顶栏改成一行之后，「重新引导」挪到了这里，所以这行也要 nowrap：
     它自己折行同样会把舞台往下推。极窄屏靠横向滚动兜底。 */
  flex-wrap: nowrap;
  overflow-x: auto;
  scrollbar-width: none;
  padding: 0 0.9em 0.4em;
  font-size: 0.86em;
  color: var(--ws-fg-dim);
}
.ws-srcbar::-webkit-scrollbar { display: none; }
.ws-srcbar > * { flex-shrink: 0; }
/* 「重新引导」推到最右：同一条视觉带里，不与定位信息抢注意力 */
.ws-srcbar__reset { margin-left: auto; }
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

/* ── P2：地图上的小人浮标 + toast ───────────────────────────────────────── */
.ws-people {
  position: absolute;
  /* 抬到 .ws-zoomctl（同样钉在右下角，bottom:0.7em）上面，别把缩放按钮盖住 */
  right: 0.7em;
  bottom: 3.7em;
  display: flex;
  gap: 0.4em;
  z-index: 5;
}
.ws-toasts {
  position: absolute;
  left: 50%;
  bottom: 5.2em;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.35em;
  pointer-events: none;
  z-index: 40;
}
.ws-toast {
  max-width: 22em;
  padding: 0.35em 0.85em;
  font-size: 0.94em;
  line-height: 1.6;
  border-radius: 999px;
  background: var(--ws-panel);
  border: 1px solid var(--ws-border);
  box-shadow: var(--ws-shadow);
  backdrop-filter: blur(var(--ws-blur));
  -webkit-backdrop-filter: blur(var(--ws-blur));
  animation: ws-fade-up 0.24s ease both;
}
.ws-toast--ok {
  border-color: var(--ws-ok);
}
.ws-toast--warn {
  border-color: var(--ws-warn);
}
.ws-toast--err {
  border-color: var(--ws-err);
}
</style>
