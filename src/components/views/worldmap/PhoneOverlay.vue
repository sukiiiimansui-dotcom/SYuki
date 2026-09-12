<!--
  手机悬浮窗形态

  和别的页面最大的不同：这个页面**不自己造状态**。
  悬浮窗的位置/不透明度/模式/区域全部走 `useWorldMapLayer()` 那份模块级单例
  —— 聊天界面上的叠加层（overlay）和右下角小窗（corner）用的是同一份状态，
  在这里调位置/透明度就等于调聊天时的显示，两边不会各说各话。

  页面本身就是一块「手机屏幕」样机：底下一层假的聊天背景，
  上面浮着 260×380 的半透明地图窗 —— 这样才能看出叠加效果到底透不透、挡不挡字。
-->
<template>
  <div class="po-root">
    <div class="po-top">
      <button class="po-btn" title="回到世界地图" @click="goBack">← 返回</button>
      <span class="po-title">📱 悬浮窗</span>
      <span class="po-tag">{{ regionName || '未选区域' }}</span>
      <span class="po-tag">模式 {{ modeZh }}</span>
      <span class="po-tag">位置 {{ posText }}</span>
      <span class="po-spacer" />
      <span class="po-seg">
        <button class="po-tab" :class="{ on: state.mode === 'off' }" @click="setMode('off')">关闭</button>
        <button class="po-tab" :class="{ on: state.mode === 'overlay' }" @click="setMode('overlay')">叠加层</button>
        <button class="po-tab" :class="{ on: state.mode === 'corner' }" @click="setMode('corner')">角落小窗</button>
      </span>
      <label class="po-slider">
        <span>不透明度 {{ Math.round(state.opacity * 100) }}%</span>
        <input
          type="range"
          min="5"
          max="90"
          :value="Math.round(state.opacity * 100)"
          @input="onOpacity"
        />
      </label>
      <button class="po-btn" @click="resetPos">↺ 位置复位</button>
      <button class="po-btn" :disabled="loading" @click="loadAll(true)">↻ 刷新</button>
    </div>

    <!-- 手机样机：假的聊天界面 + 悬浮窗 -->
    <div class="po-wrap">
      <div class="po-phone">
        <div class="po-statusbar"><span>LingChat</span><span>9:41</span></div>
        <div class="po-chat">
          <div v-for="m in MOCK" :key="m.id" class="po-msg" :class="m.me ? 'me' : 'them'">
            <span class="po-bubble">{{ m.text }}</span>
          </div>
          <div class="po-msgnote">（上面是示意用的假聊天，只为了看清叠加层的透明度）</div>
        </div>

        <!-- 折叠后的角标 -->
        <button
          v-if="collapsed"
          class="po-badge"
          :style="badgeStyle"
          title="展开地图悬浮窗"
          @pointerdown="onDragStart"
          @click="onBadgeClick"
        >
          🗺
          <i v-if="roles.length" class="po-badgedot">{{ roles.length }}</i>
        </button>

        <!-- 悬浮窗本体 -->
        <div
          v-else
          class="po-win"
          :style="winStyle"
          :class="{ dragging }"
        >
          <div class="po-winhead" @pointerdown="onDragStart">
            <span class="po-wintitle">🗺 {{ regionName || '定位中…' }}</span>
            <span class="po-spacer" />
            <button class="po-mini" title="折叠成角标" @click.stop="collapsed = true">–</button>
            <button class="po-mini" title="收起（等于关闭叠加层）" @click.stop="setMode('off')">×</button>
          </div>

          <div ref="mapBox" class="po-map">
            <img
              v-if="imgUrl"
              class="po-img"
              :src="imgUrl"
              alt=""
              :style="{ opacity: state.opacity }"
              @error="markImgErr('这张区域图的数据坏了，点「↻ 刷新」重试')"
            />
            <div v-if="imgErr || !imgUrl" class="po-maperr">
              {{
                imgErr
                  ? imgErrMsg || '这张区域图还没生成好，点「↻ 刷新」重试'
                  : imgLoading
                    ? '区域图加载中…（首次较慢）'
                    : '没有选中区域'
              }}
            </div>
            <!-- 角色位置点：位置来自日程里的设施格子（和 WorldMap.vue 同一套算法） -->
            <div
              v-for="r in roles"
              :key="r.name"
              class="po-role"
              :style="{ left: r.x + '%', top: r.y + '%' }"
              :title="`${r.name}：${r.content}`"
            >
              <i class="po-dot" />
              <span class="po-rname">{{ r.name }}</span>
            </div>
            <div v-if="!loading && !roles.length" class="po-norole">此刻没有角色在地图上</div>
          </div>

          <div class="po-winbody">
            <div v-if="err" class="po-err">{{ err }}</div>
            <template v-else>
              <div v-for="r in roles.slice(0, 4)" :key="r.name" class="po-row">
                <span class="po-dot sm" />
                <span class="po-rowname">{{ r.name }}</span>
                <span class="po-rowtxt">{{ r.content || '—' }}</span>
              </div>
              <div v-if="!roles.length" class="po-row dim">没有角色的日程信息（后端 /api/schedule 为空）</div>
            </template>
          </div>
        </div>
      </div>

      <div class="po-side">
        <div class="po-h">这份状态是共享的</div>
        <div class="po-note">
          模式 / 不透明度 / 位置 / 区域都存在 <code>useWorldMapLayer()</code> 的模块级单例里，
          并写进 localStorage（键 <code>lsyuki.worldLayer</code>）。
          所以：在这里拖到哪、调多透，回到聊天界面看到的叠加层就是同一份设置 ——
          这个页面不做第二套状态，避免两处显示打架。
        </div>
        <div class="po-h mt">当前状态</div>
        <div class="po-kv"><span>mode</span><b>{{ state.mode }}</b></div>
        <div class="po-kv"><span>opacity</span><b>{{ state.opacity.toFixed(2) }}</b></div>
        <div class="po-kv"><span>x / y</span><b>{{ state.x }} / {{ state.y }}</b></div>
        <div class="po-kv"><span>adcode</span><b>{{ state.adcode || '（按定位）' }}</b></div>
        <div class="po-kv"><span>style</span><b>{{ state.style }}</b></div>
        <div class="po-h mt">折叠</div>
        <div class="po-note">
          「–」把窗口折成角标（角标可继续拖动），点角标展开。
          折叠状态是本页面的局部状态（<code>collapsed</code>），
          不写进共享 store —— store 的字段是叠加层用的，乱加会让聊天那边跟着变。
        </div>
        <div class="po-ops">
          <button class="po-btn tiny" @click="collapsed = !collapsed">{{ collapsed ? '展开窗口' : '折叠成角标' }}</button>
          <button class="po-btn tiny" @click="setPos(12, 12)">移到左上</button>
          <button class="po-btn tiny" @click="resetPos">默认位置</button>
        </div>
      </div>
    </div>

    <div class="po-foot">
      <span class="po-meta">
        {{ regionName || '未选区域' }} · 角色点 {{ roles.length }} 个 · 图源 {{ imgSource }}（{{ state.style }}）
      </span>
      <span class="po-spacer" />
      <span class="po-navtip">世界地图扩展</span>
      <button class="po-btn tiny" @click="go('/world/district-live')">🎨 实时绘制</button>
      <button class="po-btn tiny" @click="go('/world/district-viz')">📊 数据 / 图层</button>
      <button class="po-btn tiny" @click="go('/world/maplib')">🗂 地图库</button>
      <button class="po-btn tiny" @click="go('/world/phone-overlay')">📱 悬浮窗</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import worldMapApi, {
  isTauriRuntime,
  MAP_SVG_DEFAULT_H,
  MAP_SVG_DEFAULT_W,
  mapSvgUrl,
  type ScheduleRole,
} from '@/api/services/worldMap'
import { useWorldMapLayer } from '@/composables/useWorldMapLayer'

const router = useRouter()
/** 共享状态：不是本页面私有的，聊天界面的叠加层读的是同一份 */
const { state, setMode, setOpacity, setPos, setRegion } = useWorldMapLayer()

const WIN_W = 260
const WIN_H = 380
const PAD = 8

const loading = ref(false)
const err = ref('')
const imgErr = ref(false)
const collapsed = ref(false)
const regionName = ref('')
const rawRoles = ref<ScheduleRole[]>([])
const tick = ref(0)

/** 假聊天内容：只为了让「半透明」这件事看得出来 */
const MOCK = [
  { id: 1, me: false, text: '今天天气不错，出去走走？' },
  { id: 2, me: true, text: '好呀，老地方见～' },
  { id: 3, me: false, text: '我看看地图，先到公园那边等你。' },
]

const modeZh = computed(() => ({ off: '关闭', overlay: '叠加层', corner: '角落小窗' })[state.value.mode] || state.value.mode)

/** 窗口位置：state.x/y < 0 表示「还没拖过」，用默认的右下角 */
const pos = computed(() => {
  const vw = window.innerWidth
  const vh = window.innerHeight
  if (state.value.x < 0 || state.value.y < 0) {
    return { x: Math.max(PAD, vw - WIN_W - 18), y: Math.max(PAD, vh - WIN_H - 90) }
  }
  return { x: clamp(state.value.x, 0, Math.max(0, vw - WIN_W)), y: clamp(state.value.y, 0, Math.max(0, vh - 60)) }
})

const posText = computed(() => (state.value.x < 0 ? '默认' : `${Math.round(state.value.x)},${Math.round(state.value.y)}`))
const winStyle = computed(() => ({ left: pos.value.x + 'px', top: pos.value.y + 'px', width: WIN_W + 'px', height: WIN_H + 'px' }))
const badgeStyle = computed(() => ({ left: pos.value.x + 'px', top: pos.value.y + 'px' }))
const imgUrl = ref('')
const imgErrMsg = ref('')
/** 取图进行中：此时 imgUrl 还是空串，占位要显示「加载中」而不是「没有选中区域」（后者会误导） */
const imgLoading = ref(false)

// ── 区域图：双通路异步取 data URL ──
//
// 原来是 `computed` 直接拼 `/api/bigmap?...`（**纯 HTTP 地址**）：打包成 APK 后手机上
// 没有 8791 那个本地服务，图片必然加载不出来 —— 这一页就只剩一个破图占位。
// 现在统一走 `mapSvgUrl()`：真壳 `invoke('world_map_geo_svg')` 拿 SVG 转 data URL，
// 浏览器仍走调试服务，两边都能出图。
//
// 尺寸按 WorldMapLayer.vue 的老规矩：量容器的**真实 CSS 像素**再喂给后端。
// 悬浮窗只有 260×380，按默认的 1000×760 画完再缩下来，字会小成蚂蚁。
const mapBox = ref<HTMLElement | null>(null)
/** 请求序号：切区域/刷新会连发多个请求，只有最后发出的那个允许写回 */
let imgSeq = 0
/** 上一次真正发过请求的参数键：watch 与 loadAll() 都会触发取图，避免同参数连发两次 */
let lastKey = ''

/** 量容器：量不到（还没布局 / 已折叠 / 尺寸为 0）就用默认尺寸兜底 —— 宁可字小，也别空着 */
function measureBox(): { w: number; h: number } {
  const r = mapBox.value?.getBoundingClientRect()
  const w = Math.round(r?.width || 0)
  const h = Math.round(r?.height || 0)
  if (w < 32 || h < 32) return { w: MAP_SVG_DEFAULT_W, h: MAP_SVG_DEFAULT_H }
  return { w, h }
}

function markImgErr(msg = '') {
  imgErr.value = true
  imgErrMsg.value = msg
}

/**
 * 取图。失败**不留白屏**：清掉 src + 把可读原因写进 imgErrMsg（模板里会显示出来），
 * 同时 imgErr 保持 true，让「↻ 刷新」（tick++）能重新走一次。
 */
async function loadImg() {
  const key = `${state.value.adcode}|${state.value.style}|${tick.value}|${collapsed.value ? 'c' : 'o'}`
  if (!state.value.adcode) {
    lastKey = key
    imgUrl.value = ''
    imgLoading.value = false
    return
  }
  if (key === lastKey) return
  lastKey = key
  const seq = ++imgSeq
  imgLoading.value = true
  try {
    const { w, h } = measureBox()
    const url = await mapSvgUrl(state.value.adcode, state.value.style, w, h)
    if (seq !== imgSeq) return // 已经有更新的请求了，这次结果作废
    imgUrl.value = url
    imgErr.value = false
    imgErrMsg.value = ''
  } catch (e) {
    if (seq !== imgSeq) return
    imgUrl.value = ''
    markImgErr(`这张区域图没取到：${(e as Error)?.message || e}（点「↻ 刷新」重试）`)
  } finally {
    if (seq === imgSeq) imgLoading.value = false
  }
}

/** 影响图的全部输入：区域 / 风格 / 刷新计数 / 折叠状态（折叠时容器不在，取回来也没处放） */
watch(
  () => [state.value.adcode, state.value.style, tick.value, collapsed.value] as const,
  async () => {
    await nextTick() // 展开角标后容器刚挂回去，等 DOM 更新完再量尺寸
    if (collapsed.value) return
    void loadImg()
  },
)

/** 图源文案：真壳是 Rust 本地渲染（APK 里没有 HTTP 服务），浏览器才是 /api/bigmap */
const imgSource = computed(() => (isTauriRuntime() ? 'Rust 本地渲染' : '/api/bigmap'))

/**
 * 角色点坐标：与 WorldMap.vue 的 actorDots 同一套规则 ——
 * 有设施格子就用格子取模铺开，否则按序摆在底部。
 */
const roles = computed(() => {
  const out: { name: string; x: number; y: number; content: string }[] = []
  rawRoles.value.forEach((r, i) => {
    const g = r.now?.place?.grid
    const content = r.now?.content || ''
    if (Array.isArray(g) && g.length >= 2) {
      out.push({
        name: r.name,
        x: 8 + ((Number(g[0]) % 20) / 20) * 84,
        y: 8 + ((Number(g[1]) % 20) / 20) * 84,
        content,
      })
    } else if (r.now?.place?.kind === 'transit') {
      out.push({ name: r.name, x: 50, y: 90 - i * 4, content })
    } else {
      out.push({ name: r.name, x: 12 + i * 9, y: 92, content })
    }
  })
  return out
})

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v))
}

function onOpacity(e: Event) {
  setOpacity(Number((e.target as HTMLInputElement).value) / 100)
}

function resetPos() {
  setPos(-1, -1)
}

// ── 拖拽：用 pointer 事件 + window 监听（比在元素上监听稳，手指滑出窗口也不会丢）──
const dragging = ref(false)
/** 这次按下有没有真的拖动过：只是「点了一下」才允许展开，避免拖完手一松就展开 */
let dragMoved = false
let dragDX = 0
let dragDY = 0

function onDragStart(e: PointerEvent) {
  if (!e.isPrimary) return
  dragging.value = true
  dragMoved = false
  dragDX = e.clientX - pos.value.x
  dragDY = e.clientY - pos.value.y
  window.addEventListener('pointermove', onDragMove)
  window.addEventListener('pointerup', onDragEnd)
  window.addEventListener('pointercancel', onDragEnd)
}

function onDragMove(e: PointerEvent) {
  if (!dragging.value) return
  dragMoved = true
  // 拖拽中要实时跟手 → 直接写共享状态（watch 会落盘，不需要再存一次）
  setPos(e.clientX - dragDX, e.clientY - dragDY)
}

function onDragEnd() {
  dragging.value = false
  window.removeEventListener('pointermove', onDragMove)
  window.removeEventListener('pointerup', onDragEnd)
  window.removeEventListener('pointercancel', onDragEnd)
}

function onBadgeClick() {
  if (dragMoved) return
  collapsed.value = false
}

/** 折成角标后如果角标跑到屏幕外，就拉回来 */
function clampIntoView() {
  if (state.value.x < 0) return
  setPos(clamp(state.value.x, 0, Math.max(0, window.innerWidth - WIN_W)), clamp(state.value.y, 0, Math.max(0, window.innerHeight - 60)))
}

async function loadAll(manual = false) {
  loading.value = true
  if (manual) {
    err.value = ''
    imgErr.value = false
    tick.value++
  }
  // ① 区域：store 里没选过就按定位问后端要一个主块
  try {
    if (!state.value.adcode) {
      const b = await worldMapApi.blocks(undefined, state.value.style, 6)
      if (b?.ok === false) throw new Error(b.error || '拿不到主块')
      if (b?.main?.adcode) {
        setRegion(b.main.adcode, state.value.style)
        regionName.value = b.main.name || b.main.adcode
      }
    } else {
      const b = await worldMapApi.blocks(state.value.adcode, state.value.style, 1)
      regionName.value = b?.main?.name || state.value.adcode
    }
  } catch (e) {
    err.value = `区域读取失败：${(e as Error)?.message || e}`
  }
  // ② 角色位置：日程拿不到不算致命，只是没有点
  try {
    const s = await worldMapApi.schedule()
    rawRoles.value = s?.roles || []
  } catch {
    rawRoles.value = []
  }
  // ③ 区域图：等 DOM 稳定后按容器真实尺寸取。
  //    （setRegion 改了 adcode 时上面的 watch 也会触发，同一份参数由 loadImg 内部的
  //      lastKey 去重，所以这里补一枪不会变成两次请求。）
  loading.value = false
  await nextTick()
  if (!collapsed.value) void loadImg()
}

function go(path: string) {
  router.push(path)
}
function goBack() {
  router.push('/world')
}

onMounted(() => {
  window.addEventListener('resize', clampIntoView)
  void loadAll()
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', clampIntoView)
  window.removeEventListener('pointermove', onDragMove)
  window.removeEventListener('pointerup', onDragEnd)
  window.removeEventListener('pointercancel', onDragEnd)
})
</script>

<style scoped>
.po-root {
  position: fixed;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: linear-gradient(160deg, #0d1620, #16273d);
  color: #eaf3ff;
  z-index: 50;
}
.po-top {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 7px 11px;
  background: rgba(13, 22, 32, 0.92);
  border-bottom: 1px solid rgba(121, 217, 255, 0.22);
  flex-wrap: wrap;
}
.po-title {
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
}
.po-tag {
  font-size: 11.5px;
  background: rgba(255, 255, 255, 0.07);
  border: 1px solid rgba(121, 217, 255, 0.22);
  border-radius: 20px;
  padding: 3px 10px;
  color: #cfe6ff;
  white-space: nowrap;
}
.po-spacer {
  flex: 1;
}
.po-btn {
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(121, 217, 255, 0.25);
  border-radius: 9px;
  padding: 6px 11px;
  color: #eaf3ff;
  font-size: 12.5px;
  cursor: pointer;
}
.po-btn:hover:not(:disabled) {
  background: rgba(121, 217, 255, 0.18);
}
.po-btn:disabled {
  opacity: 0.45;
  cursor: default;
}
.po-btn.tiny {
  padding: 4px 9px;
  font-size: 11.5px;
}
.po-seg {
  display: inline-flex;
  gap: 4px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(121, 217, 255, 0.18);
  border-radius: 10px;
  padding: 3px;
}
.po-tab {
  background: transparent;
  border: none;
  border-radius: 8px;
  color: #cfe6ff;
  font-size: 12.5px;
  padding: 5px 10px;
  cursor: pointer;
}
.po-tab.on {
  background: rgba(121, 217, 255, 0.26);
  color: #fff;
}
.po-slider {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11.5px;
  color: #cfe6ff;
}
.po-slider input {
  width: 92px;
}
.po-wrap {
  flex: 1;
  display: flex;
  gap: 14px;
  min-height: 0;
  padding: 14px;
}
.po-phone {
  position: relative;
  width: 360px;
  max-width: 46vw;
  flex: none;
  border-radius: 22px;
  border: 1px solid rgba(121, 217, 255, 0.28);
  background: linear-gradient(180deg, #16273d, #0f1a26);
  box-shadow: 0 14px 40px rgba(0, 0, 0, 0.5);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
.po-statusbar {
  display: flex;
  justify-content: space-between;
  padding: 6px 12px;
  font-size: 11px;
  color: #9fb4cc;
  border-bottom: 1px solid rgba(121, 217, 255, 0.16);
}
.po-chat {
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow: hidden;
}
.po-msg {
  display: flex;
}
.po-msg.me {
  justify-content: flex-end;
}
.po-bubble {
  max-width: 78%;
  padding: 7px 10px;
  border-radius: 12px;
  font-size: 12px;
  line-height: 1.5;
  background: rgba(255, 255, 255, 0.1);
  border: 1px solid rgba(121, 217, 255, 0.18);
}
.po-msg.me .po-bubble {
  background: rgba(121, 217, 255, 0.22);
  border-color: rgba(121, 217, 255, 0.4);
}
.po-msgnote {
  font-size: 10.5px;
  color: #6f8aa6;
  margin-top: 4px;
}
/* ── 悬浮窗本体：fixed 定位，坐标来自共享 store ── */
.po-win {
  position: fixed;
  border-radius: 14px;
  overflow: hidden;
  background: rgba(10, 18, 27, 0.55);
  border: 1px solid rgba(121, 217, 255, 0.45);
  box-shadow: 0 12px 30px rgba(0, 0, 0, 0.5);
  display: flex;
  flex-direction: column;
  z-index: 70;
  backdrop-filter: blur(2px);
}
.po-win.dragging {
  border-color: #79d9ff;
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.6);
}
.po-winhead {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  background: rgba(13, 22, 32, 0.85);
  border-bottom: 1px solid rgba(121, 217, 255, 0.25);
  cursor: grab;
  touch-action: none;
  user-select: none;
}
.po-winhead:active {
  cursor: grabbing;
}
.po-wintitle {
  font-size: 12px;
  font-weight: 600;
  color: #cfe6ff;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.po-mini {
  width: 20px;
  height: 20px;
  line-height: 1;
  border-radius: 6px;
  border: 1px solid rgba(121, 217, 255, 0.3);
  background: rgba(255, 255, 255, 0.08);
  color: #eaf3ff;
  font-size: 12px;
  cursor: pointer;
  padding: 0;
}
.po-map {
  position: relative;
  flex: 1;
  min-height: 0;
  background: #0a121b;
  overflow: hidden;
}
.po-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  transition: opacity 0.15s;
}
.po-maperr {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: center;
  font-size: 11.5px;
  color: #8fa6bd;
  padding: 16px;
  line-height: 1.6;
}
.po-role {
  position: absolute;
  transform: translate(-50%, -50%);
  display: flex;
  align-items: center;
  gap: 3px;
  font-size: 10px;
  pointer-events: none;
  z-index: 3;
}
.po-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #ffd166;
  box-shadow: 0 0 0 3px rgba(255, 209, 102, 0.28);
  flex: none;
}
.po-dot.sm {
  width: 6px;
  height: 6px;
  box-shadow: none;
}
.po-rname {
  background: rgba(0, 0, 0, 0.55);
  border-radius: 5px;
  padding: 1px 4px;
  white-space: nowrap;
}
.po-norole {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 6px;
  text-align: center;
  font-size: 10.5px;
  color: #6f8aa6;
}
.po-winbody {
  max-height: 112px;
  overflow-y: auto;
  padding: 6px 8px;
  background: rgba(13, 22, 32, 0.72);
  font-size: 11px;
}
.po-row {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 2px 0;
  color: #cfe6ff;
}
.po-row.dim {
  color: #7f97b0;
}
.po-rowname {
  font-weight: 600;
  white-space: nowrap;
}
.po-rowtxt {
  color: #9fb4cc;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.po-err {
  color: #ffc7a8;
  line-height: 1.6;
}
.po-badge {
  position: fixed;
  width: 46px;
  height: 46px;
  border-radius: 50%;
  border: 1px solid rgba(121, 217, 255, 0.55);
  background: rgba(13, 22, 32, 0.88);
  color: #eaf3ff;
  font-size: 20px;
  cursor: grab;
  z-index: 70;
  touch-action: none;
  box-shadow: 0 8px 20px rgba(0, 0, 0, 0.45);
}
.po-badge:active {
  cursor: grabbing;
}
.po-badgedot {
  position: absolute;
  right: -3px;
  top: -3px;
  min-width: 17px;
  height: 17px;
  border-radius: 9px;
  background: #ffd166;
  color: #201a00;
  font-size: 10.5px;
  font-weight: 700;
  font-style: normal;
  line-height: 17px;
  text-align: center;
}
.po-side {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  background: rgba(13, 22, 32, 0.6);
  border: 1px solid rgba(121, 217, 255, 0.18);
  border-radius: 14px;
  padding: 12px 14px;
  font-size: 12px;
}
.po-h {
  font-size: 13px;
  font-weight: 600;
  color: #9fe0ff;
  margin-bottom: 6px;
}
.po-h.mt {
  margin-top: 14px;
}
.po-note {
  color: #8fa6bd;
  line-height: 1.7;
}
.po-note code {
  color: #79d9ff;
}
.po-kv {
  display: flex;
  justify-content: space-between;
  gap: 10px;
  padding: 3px 0;
  border-bottom: 1px dashed rgba(121, 217, 255, 0.12);
  color: #b9d4ec;
}
.po-kv b {
  color: #ffd166;
}
.po-ops {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
  margin-top: 10px;
}
.po-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 11px;
  background: rgba(13, 22, 32, 0.9);
  border-top: 1px solid rgba(121, 217, 255, 0.22);
  font-size: 11.5px;
  color: #9fb4cc;
  flex-wrap: wrap;
}
.po-navtip {
  color: #6f8aa6;
}
@media (max-width: 780px) {
  .po-phone {
    max-width: 62vw;
  }
  .po-side {
    display: none;
  }
}
</style>
