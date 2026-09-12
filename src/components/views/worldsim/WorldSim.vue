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

      <!-- ④ 地图（国/省/市/区县）—— v-html 注入后端 SVG，点击靠事件委托
           手势（平移/捏合/滚轮/复位）只作用在 .ws-geo__pan 这一层变换容器上，
           SVG 本体不动 —— 详见 useWorldSimGestures 的说明。 -->
      <section v-else class="ws-mapwrap">
        <div ref="geoHost" class="ws-geo" @click="onGeoClick">
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
              @pick="onActorPick"
            />
            <WsVehicleMark
              v-for="t in movingTrips"
              :key="t.id"
              :trip="t"
              :grid="WS_GRID"
              :zoom="districtScale"
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
import { wsToast, useWsToast } from './wsToast'
import type { PlacedActor } from './wsActors'
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
import { useGameStore } from '@/stores/modules/game'

const router = useRouter()
const { t } = useI18n()

/* 皮肤 + 取图器 + 状态机 */
const theme = useWorldSimTheme()
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

/* ══ P2：地图上的「人」+ 面板 ══════════════════════════════════════════════
 * 数据（谁在哪、头像、日程）在 useWsActors；面板的开关/选中在 useWsPanel。
 * 这一层只做两件事：把人物层插进地图、把面板接上事件。
 * ⚠️ 立绘（≈73MB/张）的生命周期只归 WsCharPanel + useWsPortrait 管，
 *    这里**不碰**，也就不可能被这一页误加载。 */
const gameStore = useGameStore()
/** 小区图的网格边长：与 WsDistrict 的 SKETCH_SIZE / 后端 sketch 一致 */
const WS_GRID = 28
const wsPanel = useWsPanel()
const actors = useWsActors({ grid: ref(WS_GRID), areaLabel: sim.areaLabel })
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
const districtScale = computed(() => Number(districtRef.value?.gsScale) || 1)

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
 * 快捷动作（打招呼/送礼物/约他出门）。
 *
 * 现在**没有任何后端能力**，所以这里只负责把事件转发出去（下一步接后端时
 * 直接在这个函数里换成真正调用即可），**可见的提示由 WsCharPanel 弹**
 * —— 提示只留一处，避免同一次点击弹两条 toast。
 */
function onQuick(_action: string, _a: PlacedActor) {
  /* 预留：P4/P5 接「打招呼 / 送礼物 / 约他出门」的真实能力时在此处调用 */
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
    if (s === 'neighborhood') void loadActors()
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

onBeforeUnmount(() => {
  // 离开页面：面板关掉（立绘随之释放）+ 清掉还在排队的 toast 定时器
  wsPanel.closePanel()
  clearToasts()
  if (syncTimer !== null) window.clearTimeout(syncTimer)
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
