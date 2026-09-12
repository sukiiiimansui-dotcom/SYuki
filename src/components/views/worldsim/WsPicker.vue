<template>
  <!--
    三级联动（省 → 市 → 区县）+ 搜索框纠偏
    两个入口共用这一个组件：
      · 定位失败 → 「手动选城市」
      · 确认弹窗点「否」→ 纠偏
    列数是**动态**的：直辖市（北京/上海/天津/重庆）的数据里省下面直接就是区，
    没有「市」这一级 —— 所以列标题按**实际子级的级别**取，而不是写死「市」。
  -->
  <div class="ws-pick">
    <div class="ws-pick__head">
      <div>
        <div class="ws-pick__title">选择你的位置</div>
        <div class="ws-pick__sub">{{ note || '定位不准？直接选：省 → 市 → 区县' }}</div>
      </div>
      <button class="ws-btn ws-btn--ghost" type="button" @click="emit('cancel')">返回</button>
    </div>

    <div class="ws-pick__search">
      <span class="ws-pick__ico" aria-hidden="true">🔍</span>
      <input
        v-model="query"
        class="ws-pick__input"
        type="search"
        placeholder="搜索省 / 市 / 区县（回车可在下一级里深搜）"
        @keydown.enter.prevent="deepSearch"
      />
      <button class="ws-btn ws-btn--ghost" type="button" :disabled="deepBusy || !query" @click="deepSearch">
        {{ deepBusy ? t('worldsim.loader.searching') : '深搜' }}
      </button>
    </div>
    <div v-if="searchHint" class="ws-note ws-pick__note">{{ searchHint }}</div>

    <!-- 三列（窄屏自动叠成一列，DOM 完全一样） -->
    <div class="ws-pick__cols">
      <div v-for="(col, ci) in columns" :key="ci" class="ws-pick__col ws-card">
        <div class="ws-pick__colhead">
          <span class="ws-pick__coltitle">{{ col.title }}</span>
          <!-- 数量只在真有数据时显示：加载中写个「0」是在撒谎（看起来像「这一级没有下级」） -->
          <span v-if="!col.loading" class="ws-tag">{{ visible(col).length }}</span>
        </div>
        <!-- `ws-stagger`：列表项错峰进场（每行 36ms，纯 CSS 的 --ws-i），
             不要「啪」地整列弹出来 -->
        <div class="ws-pick__list ws-scroll ws-stagger">
          <!-- 结构已知（就是 N 行「名字 + adcode」的按钮）→ 用**骨架屏**，不用转圈：
               形状照抄下面的 .ws-pick__item（行高/内边距/两栏分布），内容回来不跳版。
               骨架默认 delay=0（它负责占位，晚出现就等于跳版）。

               `ws-swap` = 骨架淡出 + 列表淡入的**交叉淡入**（不是硬切一下闪白）。
               ⚠️ 这里刻意把「加载中」写成**独立的 v-if**，而不是原来那条 v-else-if 链的
               第一支：Vue 的 <Transition> 只能包一个元素，包住链首会把链切断，
               编译器直接报「v-else-if has no adjacent v-if」（vite build 会失败，已踩过）。
               淡出期间骨架是 position:absolute（见 worldsim-loading.css 的
               `.ws-swap-leave-active`），所以它不会把真内容顶下去 —— 交叉淡入不跳版。 -->
          <Transition name="ws-swap">
            <WsLoading v-if="col.loading" variant="list" size="sm" :rows="6" :text="t('worldsim.loader.pickColumn')" />
          </Transition>
          <div v-if="!col.loading && col.error" class="ws-note ws-note--warn ws-pick__err">
            <span>{{ col.error }}</span>
            <button class="ws-btn ws-btn--ghost" type="button" @click="reloadCol(ci)">重试</button>
          </div>
          <button
            v-for="item in visible(col)"
            :key="item.adcode"
            class="ws-pick__item"
            :class="{ 'is-on': col.selected === item.adcode }"
            type="button"
            @click="pick(ci, item)"
          >
            <span class="ws-pick__name">{{ item.name }}</span>
            <span class="ws-pick__ad">{{ item.adcode }}</span>
          </button>
          <div v-if="!col.loading && !col.error && !visible(col).length" class="ws-pick__empty">
            {{ query ? '这一级没有匹配项' : '没有更多下级了' }}
          </div>
        </div>
      </div>
      <!-- 三列还没拿到（第一次拉列表）——同样是结构已知，走骨架 -->
      <div v-if="!columns.length" class="ws-pick__col ws-card">
        <WsLoading variant="list" size="sm" :rows="7" :text="t('worldsim.loader.pickList')" />
      </div>
    </div>

    <!-- 当前选择 + 确定 -->
    <div class="ws-pick__foot">
      <div class="ws-pick__now">
        <span class="ws-pick__nowlabel">已选</span>
        <span v-if="chain.length" class="ws-pick__chain">{{ chainText }}</span>
        <span v-else class="ws-pick__chain ws-pick__chain--dim">还没选</span>
      </div>
      <div class="ws-pick__ops">
        <button class="ws-btn ws-btn--ghost" type="button" :disabled="!chain.length" @click="clearAll">清空</button>
        <!-- 只选到市也能确定：某些市的下级列表拿不到（离线/未缓存），不该把用户堵死 -->
        <button class="ws-btn ws-btn--primary" type="button" :disabled="!canConfirm" @click="confirm">
          就这里（{{ chain.length >= 2 ? chain[chain.length - 1].name : '请继续选' }}）
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import WsLoading from './WsLoading.vue'
import { errTextKey } from './wsErr'
import { geoLevelOf, levelLabel, matchRegions, type GeoRegion } from './wsGeo'


const { t } = useI18n()

const props = withDefaults(
  defineProps<{
    /** 取某一级的子级清单（来自 useWorldSimGeo.loadRegions，走的是同一份 geojson 缓存） */
    loadRegions: (ad: string) => Promise<GeoRegion[]>
    /** 预展开的路径（定位已经知道省/市时，直接把它们选中，用户只需补区县） */
    initial?: GeoRegion[]
    /** 顶部提示语（定位失败原因 / 「定位只到市级」等） */
    note?: string
  }>(),
  { initial: () => [], note: '' },
)

const emit = defineEmits<{
  (e: 'select', chain: GeoRegion[]): void
  (e: 'cancel'): void
}>()

interface Col {
  /** 这一列是谁的子级 */
  parent: string
  title: string
  items: GeoRegion[]
  loading: boolean
  error: string
  selected: string
}

const columns = ref<Col[]>([])
const query = ref('')
const searchHint = ref('')
const deepBusy = ref(false)
/** 记录每一列的 parent，重试时不用重新推 */
const parents = ref<string[]>([])

/** 子级的列标题：按**实际数据**判断级别（直辖市没有「市」这一级，硬写会错） */
function titleFor(children: GeoRegion[]): string {
  const lv = children.length ? geoLevelOf(children[0].adcode) : ''
  if (lv === 'district') return '区县'
  if (lv === 'city') return '市'
  if (lv === 'province') return '省'
  return '下级'
}

async function loadCol(parent: string, index: number) {
  parents.value[index] = parent
  // 列标题在取到数据前先显示「加载中」（走 i18n）；取到后用真实级别（省/市/区县）
  const col: Col = { parent, title: t('worldsim.loader.pickHead'), items: [], loading: true, error: '', selected: '' }
  columns.value = [...columns.value.slice(0, index), col]
  try {
    const items = await props.loadRegions(parent)
    const cur = columns.value[index]
    if (!cur || cur.parent !== parent) return // 用户已经换了一级，这份结果作废
    cur.items = items
    cur.title = titleFor(items)
    cur.loading = false
    if (!items.length) cur.error = '拿不到这一级的列表（可能没缓存且当前离线）'
  } catch (e) {
    const cur = columns.value[index]
    if (!cur) return
    cur.loading = false
    // 技术错误不外泄：`Failed to fetch` 这种要给成人话，英文原文只留在 detail 里
    const info = errTextKey(e)
    cur.error = t(info.key) + (info.showDetail && info.detail ? `（${info.detail}）` : '')
  }
}

/** 点某一列的某一项：选中 + 展开它的下一级（区县没有下一级，就到头了） */
async function pick(ci: number, r: GeoRegion) {
  const cur = columns.value[ci]
  if (!cur) return
  cur.selected = r.adcode
  searchHint.value = ''
  // 更深的旧选择作废
  columns.value = columns.value.slice(0, ci + 1)
  const lv = geoLevelOf(r.adcode)
  if (lv === 'district') return // 区县是叶子
  await loadCol(r.adcode, ci + 1)
}

async function reloadCol(ci: number) {
  const parent = parents.value[ci]
  if (parent) await loadCol(parent, ci)
}

const chain = computed<GeoRegion[]>(() =>
  columns.value
    .map((c) => c.items.find((i) => i.adcode === c.selected))
    .filter((x): x is GeoRegion => !!x),
)

const chainText = computed(() => chain.value.map((c) => c.name).join(' · '))

const canConfirm = computed(() => {
  if (chain.value.length >= 2) return true
  // 直辖市特例：省下面直接是区，选到区也算选全了
  if (chain.value.length === 1) return geoLevelOf(chain.value[0].adcode) === 'district'
  return false
})

function visible(col: Col): GeoRegion[] {
  const q = query.value.trim()
  if (!q) return col.items
  return matchRegions(col.items, q, 200)
}

/** 清空重选 */
function clearAll() {
  columns.value = []
  query.value = ''
  searchHint.value = ''
  void loadCol('100000', 0)
}

/**
 * 深搜（「搜索框纠偏」的加强版）。
 *
 * 为什么不是「一次搜遍全国」：那要 31 个省 × 各自的下级请求，几十次网络往返，
 * 手机上等不起。这里的策略是**只往已选中的那一支再走一层**（最多 1~2 次请求）：
 *   · 已选省未选市 → 把该市列表拉出来搜
 *   · 已选市未选区 → 把该区列表拉出来搜
 *   · 什么都没选 → 先在省列里找
 * 找不到就明说，不假装搜过了。
 */
async function deepSearch() {
  const q = query.value.trim()
  if (!q) return
  searchHint.value = ''
  if (columns.value.some((c) => visible(c).length)) {
    searchHint.value = `已在展开的层级里找到匹配项（共 ${columns.value.reduce((n, c) => n + visible(c).length, 0)} 项）`
    return
  }
  const deepestIdx = chain.value.length ? chain.value.length - 1 : -1
  const deepest = deepestIdx >= 0 ? chain.value[deepestIdx] : null
  if (deepest && geoLevelOf(deepest.adcode) !== 'district' && !columns.value[deepestIdx + 1]) {
    deepBusy.value = true
    try {
      await loadCol(deepest.adcode, deepestIdx + 1)
      const col = columns.value[deepestIdx + 1]
      const hit = col ? matchRegions(col.items, q, 200) : []
      searchHint.value = hit.length
        ? `在「${deepest.name}」下面找到 ${hit.length} 项`
        : `「${deepest.name}」下面没有匹配「${q}」的项`
    } catch (e) {
      searchHint.value = `深搜失败：${e instanceof Error ? e.message : String(e)}`
    } finally {
      deepBusy.value = false
    }
    return
  }
  searchHint.value = `没找到「${q}」：先在上面选中省（或市），再点「深搜」往下找一级`
}

function confirm() {
  if (!canConfirm.value) return
  emit('select', chain.value)
}

onMounted(async () => {
  await loadCol('100000', 0)
  // 定位已经知道省/市时，把它们预选中（用户只需要补一个区县，少点两下）
  for (const step of props.initial || []) {
    if (!step?.adcode || step.adcode === '100000') continue
    const idx = columns.value.findIndex((c) => c.items.some((i) => i.adcode === step.adcode))
    if (idx < 0) break
    await pick(idx, { adcode: step.adcode, name: step.name })
  }
})
</script>

<style scoped>
.ws-pick {
  display: flex;
  flex-direction: column;
  gap: 0.7em;
  width: min(46em, 94vw);
  max-height: 88vh;
  padding: 1em 1.1em 1.1em;
  background: var(--ws-panel);
  border: 1px solid var(--ws-border);
  border-radius: var(--ws-radius-lg);
  box-shadow: var(--ws-shadow-lg);
  backdrop-filter: blur(var(--ws-blur));
  -webkit-backdrop-filter: blur(var(--ws-blur));
  animation: ws-fade-up 0.28s ease both;
}
.ws-pick__head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 0.8em;
}
.ws-pick__title {
  font-size: 1.1em;
  font-weight: 600;
}
.ws-pick__sub {
  font-size: 0.86em;
  color: var(--ws-fg-dim);
}
.ws-pick__search {
  display: flex;
  align-items: center;
  gap: 0.5em;
  padding: 0.25em 0.5em;
  background: var(--ws-panel-2);
  border: 1px solid var(--ws-border);
  border-radius: var(--ws-radius-sm);
}
.ws-pick__ico {
  opacity: 0.7;
}
.ws-pick__input {
  flex: 1;
  min-width: 0;
  font: inherit;
  color: var(--ws-fg);
  background: transparent;
  border: none;
  outline: none;
  padding: 0.3em 0;
}
.ws-pick__note {
  font-size: 0.84em;
}
/* 三列：桌面并排 / 窄屏叠成一列（同一套 DOM，只改网格） */
.ws-pick__cols {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 0.6em;
  min-height: 0;
}
@media (max-width: 640px) {
  .ws-pick__cols {
    grid-template-columns: minmax(0, 1fr);
  }
}
.ws-pick__col {
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: 0.5em;
}
.ws-pick__colhead {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 0.2em 0.4em;
  font-size: 0.86em;
  color: var(--ws-fg-dim);
}
.ws-pick__coltitle {
  font-weight: 600;
  color: var(--ws-fg);
}
.ws-pick__list {
  display: flex;
  flex-direction: column;
  gap: 0.15em;
  max-height: 32vh;
  min-height: 6em;
  /* 骨架淡出时是 absolute（.ws-swap-leave-active），得有个定位祖先把它按在这块列表里 */
  position: relative;
}
@media (max-width: 640px) {
  .ws-pick__list {
    max-height: 20vh;
  }
}
.ws-pick__item {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 0.4em;
  font: inherit;
  font-size: 0.92em;
  text-align: left;
  color: var(--ws-fg);
  background: transparent;
  border: 1px solid transparent;
  border-radius: var(--ws-radius-sm);
  padding: 0.3em 0.5em;
  cursor: pointer;
}
.ws-pick__item:hover {
  background: var(--ws-primary-soft);
}
.ws-pick__item.is-on {
  color: var(--ws-on-primary);
  background: var(--ws-primary);
  font-weight: 600;
}
.ws-pick__ad {
  font-size: 0.78em;
  opacity: 0.55;
  font-variant-numeric: tabular-nums;
}
.ws-pick__empty {
  padding: 0.6em 0.4em;
  font-size: 0.84em;
  color: var(--ws-fg-dim);
}
.ws-pick__err {
  flex-direction: column;
  align-items: flex-start;
  gap: 0.3em;
  font-size: 0.84em;
}
.ws-pick__foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.8em;
  flex-wrap: wrap;
  padding-top: 0.6em;
  border-top: 1px solid var(--ws-border);
}
.ws-pick__now {
  display: flex;
  align-items: baseline;
  gap: 0.4em;
  min-width: 0;
}
.ws-pick__nowlabel {
  font-size: 0.8em;
  color: var(--ws-fg-dim);
}
.ws-pick__chain {
  font-weight: 600;
}
.ws-pick__chain--dim {
  font-weight: 400;
  color: var(--ws-fg-dim);
}
.ws-pick__ops {
  display: flex;
  gap: 0.5em;
}
</style>
