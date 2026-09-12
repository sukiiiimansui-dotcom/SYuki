// 「世界模拟」P2：地图上的「人」——纯逻辑部分（不 import vue / 不碰网络 / 不碰 DOM）
//
// 为什么单独一个文件（与 wsGeo.ts 同样的理由）：
//   ① 「谁站在哪、谁跟谁重叠、重叠了往哪挪」全是**纯函数**，拆出来才能用 node 直接跑自检
//      （见 ~/rikka/Dsh-SYuki/world_map/frontend_selftest_worldsim_p1.mjs 的【P2】段）；
//   ② 组件（WsAvatarLayer）与 composable（useWsActors）只负责「什么时候调」。
//
// ── 坐标约定（很重要，改之前先读）──────────────────────────────────────────
// 小区图的坐标系是**格子**：后端草图/AI 精绘的 viewBox 都是 `0 0 size size`
// （见 wsDistrictPaint.ts 的 toSvg），size 默认 28。地图用 `object-fit: contain`
// 显示在不一定正方形的盒子里 —— 所以「格子 → 屏幕」的换算必须先做**信箱（letterbox）**
// 折算：取 min(w,h)/size 当比例尺，再把短边居中。
// 本文件只做这套纯数学；谁来量那个盒子（ResizeObserver）在组件里。
//
// 后端 runtime 里 actors 的坐标（`x`/`y`）**就是这套格子坐标**
// （Rust 侧 world_map/state.rs 的注释与手机端 phone.js 的网格一致）。

/** 一个可画的「人」——角色或玩家 */
export interface MapActor {
  /** 稳定 id：角色是 `r<roleId>`，玩家固定 `me` */
  id: string
  /** 角色数据库主键（玩家为 0）——日程/记忆/对话都要用它 */
  roleId: number
  name: string
  subtitle: string
  /** 角色目录名（取头像/立绘用） */
  folder: string
  /** 原始情绪（`GameRole.emotion`，可能为空） */
  emotion: string
  /** 已解析出的头像 URL；空 = 还没解析出来（画占位） */
  avatarUrl: string
  isMe: boolean
  /** 当前所在的格子坐标（未错开） */
  gx: number
  gy: number
  /** 位置从哪来的（如实展示，别假装精确） */
  posSource: ActorPosSource
  /** 「便利店」这种地点/在做的事 */
  place: string
  /** 该角色此刻的日程条目（面板里要显示） */
  nowText: string
}

/** 位置可信度：runtime（后端实时）> schedule（日程推出的设施点）> scatter（本地散开） */
export type ActorPosSource = 'runtime' | 'schedule' | 'scatter' | 'me'

/** 错开之后的一个人（多了屏幕位置与是否被选中） */
export interface PlacedActor extends MapActor {
  /** 错开后的格子坐标 */
  px: number
  py: number
  /** 该位置压了几个人（>1 说明这一堆是叠着的） */
  crowd: number
}

/* ══════════════════════════════════════════════════════════════════
 * 一、格子 ↔ 屏幕（信箱折算）
 * ══════════════════════════════════════════════════════════════════ */

export interface Letterbox {
  /** 一个格子等于多少 CSS 像素 */
  scale: number
  /** 图在盒子里的左/上内边距（把正方形居中留出来的那两条） */
  padX: number
  padY: number
}

/**
 * 算出「正方形地图装在矩形盒子里」的信箱参数。
 *
 * 与 CSS `object-fit: contain` 的算法逐字一致 —— 两边不一致的话，
 * 头像会整体偏移（而且只在非正方形盒子上偏，最难查）。
 */
export function letterboxOf(boxW: number, boxH: number, grid = 28): Letterbox {
  const w = Math.max(1, Number(boxW) || 1)
  const h = Math.max(1, Number(boxH) || 1)
  const n = Math.max(1, Number(grid) || 1)
  const scale = Math.min(w, h) / n
  return { scale, padX: (w - n * scale) / 2, padY: (h - n * scale) / 2 }
}

/** 格子坐标 → 盒子内 CSS 像素（头像绝对定位用） */
export function gridToBox(gx: number, gy: number, lb: Letterbox): { x: number; y: number } {
  return { x: lb.padX + gx * lb.scale, y: lb.padY + gy * lb.scale }
}

/**
 * 视图变换的逆：屏幕上的一点 → 舞台内容坐标。
 *
 * 手势（useWorldSimGestures）让 SVG 与头像层**共用同一个变换容器**，
 * 所以头像不需要自己反算；这个函数留给「点空白处选中最近的人」那类交互用，
 * 也顺手把变换数学收在一处（自检能覆盖）。
 */
export function screenToContent(
  client: { x: number; y: number },
  rect: { left: number; top: number },
  tf: { scale: number; tx: number; ty: number },
): { x: number; y: number } {
  const s = Number.isFinite(tf.scale) && tf.scale > 0 ? tf.scale : 1
  return { x: (client.x - rect.left - tf.tx) / s, y: (client.y - rect.top - tf.ty) / s }
}

/* ══════════════════════════════════════════════════════════════════
 * 二、重叠错开（机主明确要求：同一坐标附近多个人，必须都看得见）
 * ══════════════════════════════════════════════════════════════════ */

/**
 * 黄金角（≈137.5°）。
 *
 * 为什么用黄金角而不是「一圈 6 个」：人数是不定的（1~20 人都有可能），
 * 固定环会把第 7 个人又堆回第 1 个人身上；黄金角散点是**无序增长**的，
 * 前 n 个点的分布对任意 n 都是均匀的，不用按人数分支。
 */
const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5))

/** 第 i 圈的半径（格子）。半径不能太大：人应该还在「他待的那个地方」附近 */
export const CROWD_STEP = 0.62

/**
 * 错开散点的降级参数（P5-5：低端机）。
 *
 * `candidates` 是「挤在一起时最多试几个候选点」——原来固定 64，是 O(N²) 里的那个 N；
 * 20 人以内的真实场景用 12 个候选点就够散开（试不到就留在原地，反正不会死循环）。
 * `crowd` 是「压了几个人」那个计数的算法：`exact` 两两比对（原样），
 * `bucket` 按 0.25 格分桶统计（O(N)，只在低端机上用 —— 这个数只是给角标看的）。
 */
export interface SpreadOpts {
  candidates?: number
  crowd?: 'exact' | 'bucket'
}

/** 默认候选点数（= 原来的行为；低端机走 wsPerf 的 SPREAD_CANDIDATES_LOW） */
export const SPREAD_CANDIDATES = 64

/**
 * 把一个点周围的人错开成螺旋散点（确定性）。
 *
 * @param pts  原始位置（格子坐标），顺序 = 参与顺序（同一份输入永远同一份输出）
 * @param grid 网格边长（散点会被夹在 [0.8, grid-0.8] 里，别跑出图外）
 * @param radius 命中半径（格子）：比它更近的两个点算「重叠」
 * @param opts 降级参数（**默认值 = 老行为**，不传就是原样；见 SpreadOpts）
 * @returns 错开后的坐标 + 每个位置压了几个人（crowd）
 *
 * 说明：这是**贪心**散点，不是全局最优，但足够稳定且可复现 —— 而且
 * 「同一个角色刷新前后站的位置不能变」比「数学上最优」重要得多。
 */
export function spreadCrowd(
  pts: { x: number; y: number }[],
  grid = 28,
  radius = 1.5,
  opts: SpreadOpts = {},
): { x: number; y: number; crowd: number }[] {
  const n = Math.max(1, Number(grid) || 28)
  const r = Math.max(0.2, Number(radius) || 1.5)
  // 候选点数：低端机上少试几次（默认 64 = 老行为，别改）
  const maxCand = Math.max(1, Math.trunc(Number(opts?.candidates) || SPREAD_CANDIDATES))
  const crowdMode: 'exact' | 'bucket' = opts?.crowd === 'bucket' ? 'bucket' : 'exact'
  const lo = 0.8
  const hi = n - 0.8
  const clamp = (v: number) => Math.min(hi, Math.max(lo, v))
  const taken: { x: number; y: number }[] = []
  const out: { x: number; y: number; crowd: number }[] = []

  for (const p of pts || []) {
    let x = clamp(Number(p?.x) || n / 2)
    let y = clamp(Number(p?.y) || n / 2)
    if (taken.some((t) => Math.hypot(t.x - x, t.y - y) < r)) {
      let placed = false
      // 最多找 maxCand 个候选点（20 人以内一定够；再多也只是继续叠着，不会死循环）
      for (let i = 1; i <= maxCand; i++) {
        const ang = i * GOLDEN_ANGLE
        const rad = CROWD_STEP * Math.sqrt(i)
        const cx = clamp(x + Math.cos(ang) * rad)
        const cy = clamp(y + Math.sin(ang) * rad)
        if (!taken.some((t) => Math.hypot(t.x - cx, t.y - cy) < r)) {
          x = cx
          y = cy
          placed = true
          break
        }
      }
      if (!placed) {
        // 实在挤不下：留在原地（宁可叠着，也不能把人挪到十万八千里外）
      }
    }
    taken.push({ x, y })
    out.push({ x, y, crowd: 1 })
  }

  // 统计每个落点周围挤了几个人（只在「同一个落点」这一档上算，给 UI 标数量用）。
  // 两种算法：exact = 两两比对（原样，O(N²)）；bucket = 按 0.25 格分桶（O(N)，低端机用）。
  // ⚠️ 只改**统计方式**，不动落点坐标 —— 坐标必须与高档逐字一致，
  //    否则同一台设备换个档位，地图上的人会整体挪位置（最难受的那种 bug）。
  if (crowdMode === 'bucket') {
    const buckets = new Map<string, number>()
    const keyOf = (p: { x: number; y: number }) => `${Math.round(p.x * 4)}:${Math.round(p.y * 4)}`
    for (const p of out) buckets.set(keyOf(p), (buckets.get(keyOf(p)) || 0) + 1)
    for (const p of out) p.crowd = buckets.get(keyOf(p)) || 1
    return out
  }
  for (let i = 0; i < out.length; i++) {
    let c = 1
    for (let j = 0; j < out.length; j++) {
      if (i === j) continue
      if (Math.hypot(out[i].x - out[j].x, out[i].y - out[j].y) < 0.25) c++
    }
    out[i].crowd = c
  }
  return out
}

/** 错开结果的最近 4 份快取（模块级：同一份输入在页面各处都命中） */
const spreadMemo = new Map<string, { x: number; y: number; crowd: number }[]>()

/**
 * 带缓存的错开（P5-5）。
 *
 * 为什么值得缓存：`placed` 每次名单/坐标变化都会重算一次错开，而实际触发点里
 * 有不少「名单没变也要刷新」的路径（到达后 `loadActors(true)`、点浮标刷新、
 * 面板里改头像）。输入完全没变时，重算是纯浪费 —— 而且返回**同一个数组引用**
 * 还能让下游（头像层/气泡层）的 props 不变，连带省掉一轮 diff。
 * 容量故意很小（4）：这只是「上次那份」的快取，不是缓存层。
 */
export function spreadCrowdMemo(
  pts: { x: number; y: number }[],
  grid = 28,
  radius = 1.5,
  opts: SpreadOpts = {},
): { x: number; y: number; crowd: number }[] {
  const list = pts || []
  const key = `${grid}|${radius}|${opts?.candidates ?? SPREAD_CANDIDATES}|${opts?.crowd || 'exact'}|${list
    .map((p) => `${p?.x},${p?.y}`)
    .join(';')}`
  const hit = spreadMemo.get(key)
  if (hit) return hit
  const val = spreadCrowd(list, grid, radius, opts)
  spreadMemo.set(key, val)
  if (spreadMemo.size > 4) {
    const oldest = spreadMemo.keys().next().value
    if (oldest !== undefined) spreadMemo.delete(oldest)
  }
  return val
}

/* ══════════════════════════════════════════════════════════════════
 * 三、位置与日程
 * ══════════════════════════════════════════════════════════════════ */

/**
 * 没有 runtime 坐标时的兜底落点：在一个圆环上按索引均匀铺开。
 *
 * 为什么不是随机：角色每次刷新都换地方会非常出戏；为什么不是都堆在中心：
 * 那样 3 个人以上就全叠在一起（虽然 spreadCrowd 也会挪开，但「本来就分开」
 * 比「靠散点硬挪」更自然）。
 */
export function scatterGrid(index: number, total: number, grid = 28): { x: number; y: number } {
  const n = Math.max(1, Number(grid) || 28)
  const c = n / 2
  const k = Math.max(0, Number(index) || 0)
  const t = Math.max(1, Number(total) || 1)
  if (t <= 1) return { x: c, y: c }
  // 内圈最多 6 个人（六边形那样铺开），第 7 个人起换到外圈（半径 0.30n → 0.42n）。
  // 内圈为什么是 6 而不是更多：内圈半径 0.30n，周长 2πr ≈ 1.88n，
  // 六个人之间已经隔了约 0.31n（≈8.7 格），再多就该挤了。
  const inner = Math.min(6, t)
  const outer = t - inner
  const innerR = n * 0.3
  const step = (Math.PI * 2) / Math.max(1, inner)
  if (k < inner) {
    const ang = k * step - Math.PI / 2
    return { x: c + Math.cos(ang) * innerR, y: c + Math.sin(ang) * innerR }
  }
  const i = k - inner
  const ang = (i / Math.max(1, outer)) * Math.PI * 2 - Math.PI / 2 + step / 2
  const rad = n * 0.42
  return { x: c + Math.cos(ang) * rad, y: c + Math.sin(ang) * rad }
}

/** 从后端 runtime 的 `actors` 对象里安全地读一个人的位置（形状不受控，一律兜住） */
export function readRuntimePos(
  actors: unknown,
  name: string,
): { gx: number; gy: number; place: string } | null {
  if (!actors || typeof actors !== 'object' || !name) return null
  const node = (actors as Record<string, unknown>)[name]
  if (!node || typeof node !== 'object') return null
  const o = node as Record<string, unknown>
  const x = Number(o.x)
  const y = Number(o.y)
  if (!Number.isFinite(x) || !Number.isFinite(y)) return null
  const place = String(o.facility || o.place || o.label || '').trim()
  return { gx: x, gy: y, place }
}

/**
 * 「日程：现在 / 接下来」压成一行字。
 *
 * 拿不到就返回空串（面板显示空态），绝不编内容 —— 编出来的日程比空着更糟。
 */
export function scheduleLine(
  role: { now?: { content?: string; name?: string; time?: string; place?: { name?: string; label?: string } | null } | null; next?: { content?: string; name?: string; time?: string } | null } | null | undefined,
): { nowText: string; nextText: string; place: string } {
  const r = role || null
  const now = r?.now || null
  const next = r?.next || null
  const nowText = now ? String(now.content || now.name || '').trim() : ''
  const nextText = next ? `${String(next.time || '').trim()} ${String(next.content || next.name || '').trim()}`.trim() : ''
  const place = String(now?.place?.name || now?.place?.label || '').trim()
  return { nowText, nextText, place }
}

/* ══════════════════════════════════════════════════════════════════
 * 四、玩家头像（本地上传，独立存储键，不碰 LingChat 原有任何键）
 * ══════════════════════════════════════════════════════════════════ */

/** localStorage 键：`{ kind:'data'|'file', value:string }` */
export const K_ME_AVATAR = 'wsm:v1:me-avatar'

export interface MeAvatarStored {
  /** data = 内联 data URL（浏览器 / 小图）；file = 设备上的绝对路径（Tauri，配 convertFileSrc） */
  kind: 'data' | 'file' | ''
  value: string
}

/** 读玩家头像（坏数据一律当成没设过） */
export function parseMeAvatar(raw: string | null): MeAvatarStored {
  if (!raw) return { kind: '', value: '' }
  try {
    const o = JSON.parse(raw) as MeAvatarStored
    const kind = o?.kind === 'data' || o?.kind === 'file' ? o.kind : ''
    const value = typeof o?.value === 'string' ? o.value : ''
    if (!kind || !value) return { kind: '', value: '' }
    return { kind, value }
  } catch {
    return { kind: '', value: '' }
  }
}

/**
 * 上传的图片最大字符数。
 *
 * 为什么卡这个数：`localStorage` 在 Android WebView 上通常只有 5~10MB，
 * 一张没压过的手机原图 base64 就能到 8MB —— 会把整个站点存储写爆，
 * 连带把 `wsm:v1:state`（引导状态）一起写失败。宁可提示用户换张小图。
 */
export const ME_AVATAR_MAX_CHARS = 1_400_000

/** 数据 URL 能不能收（太长 / 不是图片都拒，并把原因说清楚） */
export function checkMeAvatarData(dataUrl: string): { ok: boolean; reason: 'empty' | 'notimage' | 'toolarge' | '' } {
  const s = String(dataUrl || '')
  if (!s) return { ok: false, reason: 'empty' }
  if (!/^data:image\//i.test(s)) return { ok: false, reason: 'notimage' }
  if (s.length > ME_AVATAR_MAX_CHARS) return { ok: false, reason: 'toolarge' }
  return { ok: true, reason: '' }
}

/**
 * 把图片等比压到最长边 `max` 的 data URL（canvas 缩放）。
 *
 * 为什么必须压：手机相册原图动辄 4000×3000，直接进 localStorage 必爆；
 * 头像显示尺寸只有几十像素，压到 256 完全够用（还更快）。
 * 在 node 自检里没有 canvas → 抛错由调用方兜住，不影响其它逻辑。
 */
export async function shrinkImageToDataUrl(file: Blob, max = 256, quality = 0.82): Promise<string> {
  const url = URL.createObjectURL(file)
  try {
    const img = await new Promise<HTMLImageElement>((resolve, reject) => {
      const el = new Image()
      el.onload = () => resolve(el)
      el.onerror = () => reject(new Error('图片读不出来（格式不支持？）'))
      el.src = url
    })
    const w0 = img.naturalWidth || img.width || 0
    const h0 = img.naturalHeight || img.height || 0
    if (!w0 || !h0) throw new Error('图片尺寸是 0')
    const k = Math.min(1, max / Math.max(w0, h0))
    const w = Math.max(1, Math.round(w0 * k))
    const h = Math.max(1, Math.round(h0 * k))
    const cv = document.createElement('canvas')
    cv.width = w
    cv.height = h
    const ctx = cv.getContext('2d')
    if (!ctx) throw new Error('画布不可用')
    ctx.drawImage(img, 0, 0, w, h)
    return cv.toDataURL('image/jpeg', quality)
  } finally {
    URL.revokeObjectURL(url)
  }
}
