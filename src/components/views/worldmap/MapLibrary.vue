<!--
  地图库（列表 / 预览 / 容量清理）

  这个页面有一件必须守住的纪律：**清理默认干跑**。
  Python 原型阶段在这个接口上误删过 119 张缓存图，所以这里的交互刻意做成两步：
    ① 点「容量清理」→ 先弹窗、先干跑（dry=1），把「将删除哪些 id / 释放多少」摆出来；
    ② 用户看完列表、亲手勾「我确认按这个列表真删」，才允许发出 dry=0 的删除请求。
  任何一步缺了都发不出真删请求 —— 不靠默认值碰运气。

  数据通路是**双通路**（细节与签名见 api/services/worldMap.ts 末尾的 maplib*Auto）：
    · 真壳（APK / 桌面）：`invoke('world_map_maplib_list' / '_stats' / '_cleanup')`；
    · 浏览器 / 局域网调试：HTTP `/api/maplib/*`（老函数原样保留）。
  唯独**条目本体**（缩略图 / 原文件 / 布局 JSON 文本）在真壳里没有对应命令，
  见下面 `fileAccess` 那段注释里的降级说明。
-->
<template>
  <div class="ml-root">
    <div class="ml-top">
      <button class="ml-btn" title="回到世界地图" @click="goBack">← 返回</button>
      <span class="ml-title">🗂 地图库</span>
      <span class="ml-tabs">
        <button
          v-for="k in KINDS"
          :key="k.id"
          class="ml-tab"
          :class="{ on: kind === k.id }"
          @click="kind = k.id"
        >
          {{ k.label }}
          <b>{{ kindCount(k.id) }}</b>
        </button>
      </span>
      <span class="ml-spacer" />
      <input v-model="adFilter" class="ml-inp" placeholder="adcode 过滤，如 440100" @keyup.enter="reload()" />
      <select v-model="sort" class="ml-sel" @change="reload()">
        <option value="recent">最近优先</option>
        <option value="oldest">最旧优先</option>
        <option value="largest">体积优先</option>
      </select>
      <select v-model.number="limit" class="ml-sel" @change="reload()">
        <option :value="30">30 条</option>
        <option :value="60">60 条</option>
        <option :value="120">120 条</option>
      </select>
      <button class="ml-btn" :disabled="loading" @click="reload()">↻ 刷新</button>
      <button class="ml-btn danger" @click="openCleanup">🧹 容量清理</button>
    </div>

    <!-- 容量条 -->
    <div class="ml-cap">
      <div class="ml-capbar">
        <i :class="{ warn: usedRatio > 0.8 }" :style="{ width: Math.min(100, usedRatio * 100) + '%' }" />
      </div>
      <span class="ml-captext">
        已用 <b>{{ fmtBytes(stats?.bytes || 0) }}</b> / 上限 {{ stats?.max_mb ?? '—' }} MB
        （{{ ((usedRatio || 0) * 100).toFixed(1) }}%）
      </span>
      <span class="ml-captext dim">共 {{ stats?.count || 0 }} 条</span>
      <span v-if="stats" class="ml-captext dim">分类：{{ kindSummary }}</span>
    </div>

    <div v-if="err" class="ml-err">{{ err }}</div>

    <div class="ml-wrap">
      <div v-if="loading && !entries.length" class="ml-hint">读取中…</div>
      <div v-else-if="!entries.length" class="ml-hint">
        这里还没有条目。先去做几张图（区域主图 / 大图 / AI 小区），再回来刷新。
      </div>
      <div v-else class="ml-grid">
        <div v-for="e in entries" :key="e.id" class="ml-card" :title="e.id" @click="preview(e)">
          <div class="ml-thumb">
            <img v-if="isImage(e) && fileAccess" :src="maplibFileUrl(e.id)" alt="" loading="lazy" @error="onThumbError" />
            <!-- 真壳：没有取条目本体的命令，缩略图退化成占位（元数据照常显示，列表仍然可用） -->
            <span v-else-if="isImage(e)" class="ml-file" :title="NOFILE_TIP">🖼<br />需在桌面端查看</span>
            <span v-else class="ml-file">📄<br />JSON</span>
            <span class="ml-kind" :class="'k-' + e.kind">{{ e.kind }}</span>
          </div>
          <div class="ml-info">
            <div class="ml-name">{{ displayName(e) }}</div>
            <div class="ml-sub">
              <span>{{ e.adcode }}</span>
              <span v-if="e.style">· {{ e.style }}</span>
            </div>
            <div class="ml-sub dim">
              <span>{{ fmtBytes(e.bytes) }}</span>
              <span>· {{ fmtStamp(e.createdAt || e.mtime) }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div class="ml-foot">
      <span class="ml-meta">{{ entries.length }} 条 / 共 {{ stats?.count || 0 }} 条{{ more ? '（还有更多，调大条数或过滤）' : '' }}</span>
      <span class="ml-spacer" />
      <span class="ml-navtip">世界地图扩展</span>
      <button class="ml-btn tiny" @click="go('/world/district-live')">🎨 实时绘制</button>
      <button class="ml-btn tiny" @click="go('/world/district-viz')">📊 数据 / 图层</button>
      <button class="ml-btn tiny" @click="go('/world/maplib')">🗂 地图库</button>
      <button class="ml-btn tiny" @click="go('/world/phone-overlay')">📱 悬浮窗</button>
    </div>

    <!-- 预览大图 -->
    <div v-if="sel" class="ml-modal" @click.self="closePreview">
      <div class="ml-modalbox">
        <div class="ml-modalhead">
          <span>{{ displayName(sel) }}</span>
          <span class="ml-spacer" />
          <!-- 真壳里没有「取条目本体」的命令：原文件/下载按钮指向的 HTTP 地址在 APK 里是死的，
               与其点了没反应（用户会以为坏了），不如换成一句说明 -->
          <template v-if="fileAccess">
            <a class="ml-btn tiny" :href="maplibFileUrl(sel.id)" target="_blank" rel="noreferrer">↗ 原文件</a>
            <a class="ml-btn tiny" :href="maplibFileUrl(sel.id)" :download="fileName(sel)">⬇ 下载</a>
          </template>
          <span v-else class="ml-noopen" :title="NOFILE_TIP">🖼 本体需在桌面端查看</span>
          <button class="ml-btn tiny" @click="closePreview">关闭</button>
        </div>
        <div class="ml-modalbody">
          <img v-if="isImage(sel) && fileAccess" :src="maplibFileUrl(sel.id)" alt="" />
          <pre v-else-if="!isImage(sel)" class="ml-json">{{ jsonPreview }}</pre>
          <div v-else class="ml-nopreview">
            这张图存在地图库里（{{ sel.path }}，{{ fmtBytes(sel.bytes) }}），
            但应用内还没有读取条目本体的命令，所以看不到大图。<br />
            元数据仍然可用：adcode {{ sel.adcode }} · 风格 {{ sel.style || '—' }} ·
            生成 {{ fmtStamp(sel.createdAt) }}
          </div>
        </div>
        <div class="ml-modalfoot">
          <span>id <code>{{ sel.id }}</code></span>
          <span>· {{ sel.path }}</span>
          <span>· {{ fmtBytes(sel.bytes) }}</span>
          <span>· 生成 {{ fmtStamp(sel.createdAt) }} / 最后访问 {{ fmtStamp(sel.lastAccess) }}</span>
          <span v-if="sel.meta?.area">· {{ sel.meta.area }}</span>
          <span v-if="sel.meta?.buildings !== undefined">· {{ sel.meta.buildings }} 栋</span>
          <span v-if="sel.meta?.osmUsed !== undefined">· OSM {{ sel.meta.osmUsed ? '有' : '无' }}</span>
        </div>
      </div>
    </div>

    <!-- 容量清理：先干跑，再确认 -->
    <div v-if="cleanOpen" class="ml-modal" @click.self="cleanOpen = false">
      <div class="ml-modalbox narrow">
        <div class="ml-modalhead">
          <span>🧹 容量清理</span>
          <span class="ml-spacer" />
          <button class="ml-btn tiny" @click="cleanOpen = false">关闭</button>
        </div>
        <div class="ml-clean">
          <div class="ml-row">
            <span>保留上限（MB）</span>
            <input v-model.number="cleanMaxMb" class="ml-inp" type="number" min="1" step="10" />
            <button class="ml-btn" :disabled="cleanLoading" @click="doDryRun">① 干跑预览</button>
          </div>
          <div class="ml-note">
            干跑只读：把「按这个上限会被清掉哪些条目」列出来，**不会删任何东西**。
            真删需要你勾下面的确认框。
          </div>

          <div v-if="dryResult" class="ml-dry">
            <div class="ml-drysum">
              将删除 <b>{{ dryResult.removed }}</b> 个条目，释放 <b>{{ dryResult.freed_mb }} MB</b>
              <span class="dim">（模拟上限 {{ cleanMaxMb }} MB）</span>
            </div>
            <div v-if="dryResult.victims?.length" class="ml-victims">
              <div v-for="v in dryResult.victims" :key="v" class="ml-victim">{{ v }}</div>
            </div>
            <div v-else class="ml-note">没有需要清理的条目（当前用量已在上限内）。</div>

            <label class="ml-confirm">
              <input v-model="cleanConfirm" type="checkbox" />
              <span>我确认按上面这份列表真删（不可撤销）</span>
            </label>
            <div class="ml-row">
              <button class="ml-btn danger" :disabled="!canPurge" @click="doPurge">② 确认清理</button>
              <span v-if="!cleanConfirm" class="ml-note">先勾确认框</span>
            </div>
          </div>

          <div v-if="cleanMsg" class="ml-cleanmsg">{{ cleanMsg }}</div>
        </div>
      </div>
    </div>

    <div v-if="toastText" class="ml-toast">{{ toastText }}</div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import {
  fmtBytes,
  fmtStamp,
  isTauriRuntime,
  maplibCleanupAuto,
  maplibFileUrl,
  maplibListAuto,
  type MapLibCleanupResult,
  type MapLibEntry,
  type MapLibSort,
  type MapLibStats,
} from '@/api/services/worldMap'

const router = useRouter()

/**
 * 能不能直接读「条目本体」（缩略图 / 大图 / 布局 JSON 文本 / 原文件下载）。
 *
 * 为什么真壳里不行：本体是靠 **HTTP** 路由 `/api/maplib/file?id=…` 取的
 * （见 worldMap.ts 的 `maplibFileUrl()`），而 APK 里根本没有那个本地服务；
 * Rust 侧目前注册的命令只有 `world_map_maplib_list` / `_stats` / `_cleanup`
 * 三个（已逐一核对 src-tauri/src/lib.rs 的 generate_handler 注册表），
 * **没有**任何「按 id 取条目内容 / 取条目绝对路径」的命令，也没有暴露地图库根目录，
 * 所以前端拿不到能交给 `convertFileSrc()` 的路径 —— 这一条只能优雅降级：
 *   · 列表、容量条、分类统计、排序、过滤、清理（含干跑）：全部照常，走命令；
 *   · 缩略图 / 大图：换成「需在桌面端查看」占位，元数据（adcode / 风格 / 体积 / 时间）照常显示；
 *   · 原文件 / 下载：不渲染成死链接；
 *   · 布局 JSON 的文本预览：给一句说明，不再发那个必然失败的请求。
 *
 * TODO（要补的命令）：给 Rust 侧加一个二选一即可 ——
 *   ① `world_map_maplib_file(id: String) -> Result<{ mime: String, data_base64: String }, String>`
 *      前端拼成 data URL；体积小但要把图片塞进 IPC（大图会慢）。
 *   ② `world_map_maplib_file_path(id: String) -> Result<String, String>` 返回绝对路径，
 *      前端 `convertFileSrc(path)` 直接给 <img>（更快，推荐）。
 *   两个都实现前，这里的降级必须留着，别把 fileAccess 写死成 true。
 */
const fileAccess = !isTauriRuntime()
/** 提示语只写一处，占位与按钮共用（避免两处说法不一致） */
const NOFILE_TIP = '应用内还没有读取地图库条目本体的命令（world_map_maplib_file / _file_path），需在桌面端查看'

const KINDS = [
  { id: '', label: '全部' },
  { id: 'region', label: '区域图' },
  { id: 'bigmap', label: '大图' },
  { id: 'district', label: '小区' },
]

const kind = ref('')
const sort = ref<MapLibSort>('recent')
const limit = ref(60)
const adFilter = ref('')
const loading = ref(false)
const err = ref('')
const entries = ref<MapLibEntry[]>([])
const stats = ref<MapLibStats | null>(null)
const sel = ref<MapLibEntry | null>(null)
const jsonPreview = ref('')
const toastText = ref('')

const cleanOpen = ref(false)
const cleanLoading = ref(false)
const cleanMaxMb = ref(300)
const cleanConfirm = ref(false)
const dryResult = ref<MapLibCleanupResult | null>(null)
const cleanMsg = ref('')

const usedRatio = computed(() => {
  const s = stats.value
  if (!s || !s.max_bytes) return 0
  return s.bytes / s.max_bytes
})
const more = computed(() => (stats.value?.count || 0) > entries.value.length)
const kindSummary = computed(() => {
  const by = stats.value?.by_kind || {}
  const bits = Object.entries(by).map(([k, v]) => `${k} ${v}`)
  return bits.length ? bits.join(' / ') : '—'
})
/** 真删的双保险：干跑结果 + 用户勾确认 */
const canPurge = computed(() => !!dryResult.value && cleanConfirm.value && !cleanLoading.value)

function toast(msg: string, ms = 3000) {
  toastText.value = msg
  window.setTimeout(() => {
    if (toastText.value === msg) toastText.value = ''
  }, ms)
}

function kindCount(id: string): number {
  const by = stats.value?.by_kind || {}
  if (!id) return stats.value?.count ?? 0
  return by[id] ?? 0
}

function isImage(e: MapLibEntry): boolean {
  const p = (e.path || '').toLowerCase()
  if (p.endsWith('.json')) return false
  return /\.(png|jpe?g|webp|svg|gif)$/.test(p) || !p
}

function displayName(e: MapLibEntry): string {
  return e.meta?.name || e.meta?.area || `${e.adcode}${e.style ? ' · ' + e.style : ''}`
}

function fileName(e: MapLibEntry): string {
  const base = (e.path || e.id).split('/').pop() || e.id
  return base.replace(/[^\w.-]/g, '_')
}

function onThumbError(ev: Event) {
  // 图挂了（文件被清掉 / 格式不是图片）：换成占位，不要让浏览器显示破图图标
  const img = ev.target as HTMLImageElement
  img.style.display = 'none'
  const holder = img.parentElement
  if (holder && !holder.querySelector('.ml-file')) {
    const s = document.createElement('span')
    s.className = 'ml-file'
    s.textContent = '⚠️ 读不到'
    holder.appendChild(s)
  }
}

async function reload(keepErr = false) {
  loading.value = true
  if (!keepErr) err.value = ''
  try {
    const d = await maplibListAuto({
      kind: kind.value || undefined,
      ad: adFilter.value.trim() || undefined,
      limit: limit.value,
      sort: sort.value,
    })
    entries.value = d?.entries || []
    stats.value = d?.stats || null
  } catch (e) {
    // 提示要按通路说：真壳里没有 8791 服务，写「服务在 127.0.0.1:8791 吗」会把人带偏
    const where = isTauriRuntime() ? '本地命令 world_map_maplib_list' : '服务 127.0.0.1:8791'
    err.value = `读取地图库失败：${(e as Error)?.message || e}（${where}）`
    entries.value = []
  } finally {
    loading.value = false
  }
}

async function preview(e: MapLibEntry) {
  sel.value = e
  jsonPreview.value = ''
  if (isImage(e)) return
  // 真壳：`/api/maplib/file` 不存在（见 fileAccess 的注释），别发这个必然失败的请求，
  // 直接给一句可读说明；元数据在弹窗底部照常显示。
  if (!fileAccess) {
    jsonPreview.value = `（应用内暂不支持读取条目本体，布局 JSON 的文本预览需在桌面端查看）\n\n路径：${e.path}\n体积：${fmtBytes(e.bytes)}\n\n${NOFILE_TIP}`
    return
  }
  // JSON 条目（布局存档）：取回文本给个受控预览，别整份塞进 DOM
  try {
    const res = await fetch(maplibFileUrl(e.id), { cache: 'no-store' })
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const text = await res.text()
    jsonPreview.value = text.length > 6000 ? text.slice(0, 6000) + '\n…（已截断）' : text
  } catch (er) {
    jsonPreview.value = `读取失败：${(er as Error)?.message || er}`
  }
}

function closePreview() {
  sel.value = null
  jsonPreview.value = ''
}

function openCleanup() {
  cleanOpen.value = true
  cleanConfirm.value = false
  dryResult.value = null
  cleanMsg.value = ''
  cleanMaxMb.value = stats.value?.max_mb ?? 300
}

/** ① 干跑：dry 恒为 true，接口层也默认干跑，两条保险 */
async function doDryRun() {
  cleanLoading.value = true
  cleanMsg.value = ''
  dryResult.value = null
  cleanConfirm.value = false
  try {
    dryResult.value = await maplibCleanupAuto({ maxMb: cleanMaxMb.value, dry: true })
    cleanMsg.value = `干跑完成：将删除 ${dryResult.value.removed} 个，释放 ${dryResult.value.freed_mb} MB`
  } catch (e) {
    cleanMsg.value = `干跑失败：${(e as Error)?.message || e}`
  } finally {
    cleanLoading.value = false
  }
}

/** ② 真删：只有用户勾了确认才会走到这里 */
async function doPurge() {
  if (!canPurge.value) return
  cleanLoading.value = true
  try {
    const r = await maplibCleanupAuto({ maxMb: cleanMaxMb.value, dry: false })
    cleanMsg.value = `已清理 ${r.removed} 个，释放 ${r.freed_mb} MB`
    dryResult.value = null
    cleanConfirm.value = false
    toast(`已清理 ${r.removed} 个，释放 ${r.freed_mb} MB`)
    await reload(true)
  } catch (e) {
    cleanMsg.value = `清理失败：${(e as Error)?.message || e}`
  } finally {
    cleanLoading.value = false
  }
}

function go(path: string) {
  router.push(path)
}
function goBack() {
  router.push('/world')
}

onMounted(() => {
  void reload()
})
</script>

<style scoped>
.ml-root {
  position: fixed;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: linear-gradient(160deg, #0d1620, #16273d);
  color: #eaf3ff;
  z-index: 50;
}
.ml-top {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 7px 11px;
  background: rgba(13, 22, 32, 0.92);
  border-bottom: 1px solid rgba(121, 217, 255, 0.22);
  flex-wrap: wrap;
}
.ml-title {
  font-size: 15px;
  font-weight: 600;
  white-space: nowrap;
}
.ml-spacer {
  flex: 1;
}
.ml-btn,
.ml-sel,
.ml-inp {
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(121, 217, 255, 0.25);
  border-radius: 9px;
  padding: 6px 11px;
  color: #eaf3ff;
  font-size: 12.5px;
  cursor: pointer;
  text-decoration: none;
  display: inline-block;
}
.ml-inp {
  cursor: text;
  min-width: 140px;
}
.ml-inp::placeholder {
  color: rgba(180, 205, 235, 0.55);
}
.ml-btn:hover:not(:disabled) {
  background: rgba(121, 217, 255, 0.18);
}
.ml-btn:disabled {
  opacity: 0.45;
  cursor: default;
}
.ml-btn.tiny {
  padding: 4px 9px;
  font-size: 11.5px;
}
.ml-btn.danger {
  border-color: rgba(255, 150, 120, 0.5);
  color: #ffd0bd;
}
.ml-btn.danger:hover:not(:disabled) {
  background: rgba(255, 120, 90, 0.2);
}
.ml-tabs {
  display: inline-flex;
  gap: 4px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(121, 217, 255, 0.18);
  border-radius: 10px;
  padding: 3px;
}
.ml-tab {
  background: transparent;
  border: none;
  border-radius: 8px;
  color: #cfe6ff;
  font-size: 12.5px;
  padding: 5px 10px;
  cursor: pointer;
}
.ml-tab b {
  color: #79d9ff;
  font-weight: 600;
  margin-left: 4px;
}
.ml-tab.on {
  background: rgba(121, 217, 255, 0.26);
  color: #fff;
}
.ml-cap {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px;
  background: rgba(13, 22, 32, 0.86);
  border-bottom: 1px solid rgba(121, 217, 255, 0.14);
  font-size: 11.5px;
  color: #b9d4ec;
  flex-wrap: wrap;
}
.ml-capbar {
  width: 180px;
  height: 8px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 5px;
  overflow: hidden;
}
.ml-capbar i {
  display: block;
  height: 100%;
  background: linear-gradient(90deg, #79d9ff, #8fd6a0);
  transition: width 0.3s;
}
.ml-capbar i.warn {
  background: linear-gradient(90deg, #ffd166, #ff9b6a);
}
.ml-captext b {
  color: #79d9ff;
}
.ml-captext.dim {
  color: #7f97b0;
}
.ml-err {
  margin: 8px 12px 0;
  padding: 8px 12px;
  background: rgba(255, 170, 120, 0.12);
  border: 1px solid rgba(255, 170, 120, 0.45);
  border-radius: 10px;
  color: #ffe3c2;
  font-size: 12.5px;
}
.ml-wrap {
  flex: 1;
  overflow-y: auto;
  padding: 10px 12px 16px;
  min-height: 0;
}
.ml-hint {
  color: #8fa6bd;
  font-size: 12.5px;
  padding: 14px 4px;
  line-height: 1.7;
}
.ml-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(132px, 1fr));
  gap: 10px;
}
.ml-card {
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(121, 217, 255, 0.18);
  border-radius: 12px;
  overflow: hidden;
  cursor: pointer;
  transition: 0.16s;
}
.ml-card:hover {
  border-color: #79d9ff;
  background: rgba(121, 217, 255, 0.14);
  transform: translateY(-1px);
}
.ml-thumb {
  position: relative;
  height: 104px;
  background: #0a121b;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}
.ml-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.ml-file {
  color: #7f97b0;
  font-size: 11.5px;
  text-align: center;
  line-height: 1.5;
  padding: 0 6px;
}
/* 真壳里的降级占位（读不到条目本体）：卡片/弹窗都不能因此塌掉或留白 */
.ml-noopen {
  font-size: 11px;
  color: #8fa6bd;
  border: 1px dashed rgba(143, 166, 189, 0.45);
  border-radius: 8px;
  padding: 3px 8px;
  cursor: help;
}
.ml-nopreview {
  max-width: 520px;
  padding: 14px 16px;
  border: 1px dashed rgba(143, 166, 189, 0.4);
  border-radius: 10px;
  background: #0a121b;
  color: #a9c0d6;
  font-size: 12px;
  line-height: 1.7;
  text-align: center;
  word-break: break-all;
}
.ml-kind {
  position: absolute;
  left: 6px;
  top: 6px;
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 20px;
  background: rgba(13, 22, 32, 0.8);
  border: 1px solid rgba(121, 217, 255, 0.35);
  color: #cfe6ff;
}
.ml-kind.k-region {
  border-color: rgba(121, 217, 255, 0.5);
}
.ml-kind.k-bigmap {
  border-color: rgba(255, 209, 102, 0.55);
  color: #ffe9b8;
}
.ml-kind.k-district {
  border-color: rgba(143, 214, 160, 0.55);
  color: #b8f0c8;
}
.ml-info {
  padding: 6px 8px 8px;
  font-size: 11.5px;
}
.ml-name {
  font-weight: 600;
  color: #eaf3ff;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ml-sub {
  color: #9fb4cc;
  display: flex;
  gap: 3px;
  margin-top: 2px;
}
.ml-sub.dim {
  color: #7f97b0;
}
.ml-foot {
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
.ml-navtip {
  color: #6f8aa6;
}
.ml-modal {
  position: fixed;
  inset: 0;
  background: rgba(6, 11, 18, 0.9);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  z-index: 60;
}
.ml-modalbox {
  background: #101c29;
  border: 1px solid rgba(121, 217, 255, 0.3);
  border-radius: 14px;
  max-width: min(920px, 96vw);
  max-height: 92vh;
  width: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.ml-modalbox.narrow {
  max-width: min(560px, 96vw);
}
.ml-modalhead {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-bottom: 1px solid rgba(121, 217, 255, 0.22);
  font-size: 13px;
  font-weight: 600;
  color: #cfe6ff;
  flex-wrap: wrap;
}
.ml-modalbody {
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 10px;
  overflow: auto;
}
.ml-modalbody img {
  max-width: 100%;
  max-height: 66vh;
  object-fit: contain;
  border-radius: 10px;
}
.ml-json {
  width: 100%;
  max-height: 60vh;
  overflow: auto;
  margin: 0;
  padding: 10px;
  background: #0a121b;
  border-radius: 10px;
  color: #b9d4ec;
  font-size: 11px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-all;
}
.ml-modalfoot {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
  padding: 8px 12px;
  border-top: 1px solid rgba(121, 217, 255, 0.18);
  font-size: 11px;
  color: #8fa6bd;
}
.ml-modalfoot code {
  color: #79d9ff;
}
.ml-clean {
  padding: 12px 14px;
  font-size: 12.5px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.ml-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.ml-row .ml-inp {
  width: 90px;
  min-width: 0;
}
.ml-note {
  color: #8fa6bd;
  line-height: 1.65;
  font-size: 11.5px;
}
.ml-dry {
  border: 1px solid rgba(121, 217, 255, 0.22);
  border-radius: 10px;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.ml-drysum b {
  color: #ffd166;
}
.ml-drysum .dim {
  color: #7f97b0;
}
.ml-victims {
  max-height: 150px;
  overflow-y: auto;
  background: #0a121b;
  border-radius: 8px;
  padding: 6px 8px;
}
.ml-victim {
  font-size: 11px;
  color: #ffb99b;
  line-height: 1.6;
  word-break: break-all;
}
.ml-confirm {
  display: flex;
  align-items: center;
  gap: 7px;
  color: #ffe3c2;
}
.ml-cleanmsg {
  color: #b9d4ec;
}
.ml-toast {
  position: fixed;
  left: 50%;
  bottom: 18px;
  transform: translateX(-50%);
  z-index: 70;
  background: rgba(16, 26, 40, 0.96);
  border: 1px solid rgba(121, 217, 255, 0.22);
  border-radius: 11px;
  padding: 9px 16px;
  font-size: 12.5px;
  max-width: 88vw;
  text-align: center;
}
@media (max-width: 780px) {
  .ml-grid {
    grid-template-columns: repeat(auto-fill, minmax(108px, 1fr));
  }
  .ml-thumb {
    height: 88px;
  }
}
</style>
