// 「世界模拟」加载态的**纯逻辑**（没有 Vue、没有 DOM、没有计时器）—— 自检脚本直接 import 它跑。
//
// 为什么这几行要从组件里搬出来：
//   · 「等多久才该显示、显示成什么样」是**产品规则**，不是渲染细节。散在模板的 v-if 里
//     没人能测，也没人敢改（改错了只是「闪一下」或「数字乱跳」，测试全绿）。
//   · 「预估剩余时间」是最容易变成骗人的地方：样本只有 2 条时外推出来的数字会乱跳，
//     卡住不动时还在倒计时更糟。规则（样本下限 / 停滞判定 / 取整粒度）集中在这里，
//     自检可以逐条盯边界值。
//
// 分档规则（业界通行，见报告里引的资料）：
//   < 100ms      → 什么都不显示（一闪而过的指示器比不显示更糟，还会闪一下）
//   100ms ~ 1s   → 轻提示（淡入的占位/插画），不摆大转圈
//   1s ~ 10s     → 明确的加载态 + 能算就给确定性进度
//   > 10s        → 详细进度 + 预估时间 + 取消/后台选项
// 本文件把「<100ms 不显示」再收紧成 delayMs=180ms（行政区划列表常常 100ms 内就回来）。

/** 时长分档阈值（毫秒）。改这里 = 改产品行为，自检会盯住边界值。 */
export const LOADING_TIERS = {
  /** 小于它 → **什么都不显示**。180 而不是 100：列表/缓存命中常常 100~150ms 就回来了，
   *  那些场景闪一个转圈比不显示更糟。 */
  delayMs: 180,
  /** delayMs ~ briefMs：轻提示（淡入的占位/插画），不摆大转圈、不给进度条 */
  briefMs: 1000,
  /** briefMs ~ longMs：明确的加载态（能算就给确定性进度条） */
  longMs: 10000,
} as const

export type LoadingTier = 'idle' | 'brief' | 'normal' | 'long'

/**
 * 已等待 ms → 档位。
 * @param delayMs 这个实例的显示延迟（0 = 立刻显示；骨架屏默认 0，普通指示器默认 LOADING_TIERS.delayMs）
 */
export function tierOf(elapsedMs: number, delayMs: number = LOADING_TIERS.delayMs): LoadingTier {
  // 写成 `!(a >= b)` 而不是 `a < b`：NaN 也走「还没到点」这一支，不会漏出一个转圈
  if (!(elapsedMs >= delayMs)) return 'idle'
  if (elapsedMs < LOADING_TIERS.briefMs) return 'brief'
  if (elapsedMs < LOADING_TIERS.longMs) return 'normal'
  return 'long'
}

/* ── 预估剩余时间 ─────────────────────────────────────────────────────────
 * 只在**样本足够**时给数字，而且给的是取整到 5 秒的粗数字：
 * 宁可「约 20 秒」稳稳地跳两次，也不要「19.3 → 24.1 → 17.8」这种乱跳的假精确。 */
export const ETA_RULES = {
  /** 至少收到这么多条要素才敢外推（少于它，抖动会被放大成乱跳的数字） */
  minItems: 3,
  /** 至少跑了这么久才敢外推 */
  minElapsedMs: 1200,
  /** 超过这么久没有新要素 = 卡住了，这时候还给倒计时就是撒谎 */
  stallMs: 4000,
  /** 取整粒度 */
  stepMs: 5000,
  /** 低于它就不必显示（几秒的事，说了反而啰嗦） */
  minEtaMs: 5000,
  /** 高于它就别报了（报了也没意义，反而像卡死） */
  maxEtaMs: 240000,
} as const

export type EtaReason =
  /** 有可信预估 */
  | 'ok'
  /** 样本不足（刚开始 / 要素太少） */
  | 'sample'
  /** 停滞：超过 stallMs 没有新要素 */
  | 'stall'
  /** 已达目标量（该切到收尾阶段，不再报剩余时间） */
  | 'reached'
  /** 没有时间基准（elapsedMs 为 0） */
  | 'notime'

export interface EtaInput {
  /** 目标口径的已完成量（小区 AI 精绘 = 已出**建筑**数） */
  done: number
  /** 目标总量（= 计划建筑数，见 planBuildingsOf） */
  target: number
  /** 已收到的**全部**要素数（建筑之外还有路/树/水：用它换算「每栋楼要等几条消息」） */
  received: number
  /** 已耗时（毫秒） */
  elapsedMs: number
  /** 距最近一条要素过去多久（毫秒）；不传按 0 算 */
  sinceLastItemMs?: number
}

export interface EtaResult {
  /** 能不能给出可信预估（false 时 UI 只显示不确定态 + 已等 N 秒，绝不显示假数字） */
  confident: boolean
  /** 进度 0..1（done/target）；target 非法时为 null（UI 退回不确定态） */
  ratio: number | null
  /** 预估剩余毫秒；null = 不给 */
  etaMs: number | null
  /** 已收到的速率（项/秒）—— 它永远可算，且不会乱跳 */
  ratePerSec: number | null
  reason: EtaReason
}

function clamp(n: number, lo: number, hi: number): number {
  return n < lo ? lo : n > hi ? hi : n
}

/**
 * 外推剩余时间。
 *
 * 算法：`速率 = 已收要素数 / 已耗时`（用**全部**要素，因为它才是「消息在流动」的证据）；
 * `每栋楼伴随的消息数 = 已收要素 / 已出建筑`；`剩余消息 = (目标建筑 − 已出建筑) × 每栋消息数`；
 * `剩余时间 = 剩余消息 / 速率`，取整到 5 秒。
 *
 * 为什么不用「已耗时 / 已出建筑 × 剩余建筑」：那会把路/树/水的时间摊到建筑头上，
 * 建筑阶段的预估会偏大（实测同一轮里能差 30%+）。
 */
export function etaOf(input: EtaInput): EtaResult {
  const done = Math.max(0, Number(input.done) || 0)
  const received = Math.max(done, Number(input.received) || 0)
  const target = Number(input.target) || 0
  const elapsed = Math.max(0, Number(input.elapsedMs) || 0)
  const since = Math.max(0, Number(input.sinceLastItemMs) || 0)

  const ratio = target > 0 ? clamp(done / target, 0, 1) : null
  const ratePerSec = elapsed > 0 && received > 0 ? received / (elapsed / 1000) : null
  const base: EtaResult = { confident: false, ratio, etaMs: null, ratePerSec, reason: 'sample' }

  if (elapsed <= 0) return { ...base, reason: 'notime' }
  if (target <= 0) return { ...base, reason: 'sample' }
  if (elapsed < ETA_RULES.minElapsedMs || received < ETA_RULES.minItems) return base
  if (done >= target) return { ...base, confident: ratio !== null, etaMs: 0, reason: 'reached' }
  if (since > ETA_RULES.stallMs) return { ...base, reason: 'stall' }
  if (!ratePerSec) return { ...base, reason: 'notime' }

  const msgsPerItem = done > 0 ? received / done : 0
  const remainingMsgs = (target - done) * msgsPerItem
  const rawMs = (remainingMsgs / ratePerSec) * 1000
  const rounded = Math.ceil(rawMs / ETA_RULES.stepMs) * ETA_RULES.stepMs
  return {
    confident: true,
    ratio,
    etaMs: clamp(rounded, ETA_RULES.minEtaMs, ETA_RULES.maxEtaMs),
    ratePerSec,
    reason: 'ok',
  }
}

/* ── 小区 AI 精绘：阶段推断 ───────────────────────────────────────────────
 * 后端是**流式**推要素的（一栋楼一条消息），所以「正在推什么类型的要素」就是
 * 最可靠的阶段信号 —— 不需要后端多告诉我们任何东西。 */
export type DrawStage = 'think' | 'buildings' | 'roads' | 'finish'

/** 阶段顺序（UI 的步骤条按它排，自检也按它验证「只前进不后退」） */
export const DRAW_STAGES: DrawStage[] = ['think', 'buildings', 'roads', 'finish']

export interface DrawCounts {
  buildings: number
  roads: number
  parks: number
  water: number
}

/**
 * 当前阶段。
 *   · 一条要素都没有 → 构思布局（这一步最慢：模型在思考，没有任何东西可以显示）
 *   · 建筑还没到位   → 落建筑
 *   · 建筑到位、还有别的要素在来 → 铺路绿化
 *   · 收到 done       → 收尾（对齐/补齐）
 */
export function drawStageOf(counts: DrawCounts, opts: { done?: boolean; planBuildings?: number } = {}): DrawStage {
  if (opts.done) return 'finish'
  const b = Math.max(0, Number(counts?.buildings) || 0)
  const others =
    Math.max(0, Number(counts?.roads) || 0) + Math.max(0, Number(counts?.parks) || 0) + Math.max(0, Number(counts?.water) || 0)
  if (b <= 0 && others <= 0) return 'think'
  const plan = Math.max(1, Number(opts.planBuildings) || 0)
  // 90% 就算「建筑到齐」：模型最后几栋经常和道路交错着吐，卡在 100% 会让阶段条显得死住
  if (b < plan * 0.9) return 'buildings'
  return 'roads'
}

/**
 * 计划建筑数 = 后端提示词自己写的规则（`build_prompt` 第 7 条：「建筑数量约 size*0.7 栋」）。
 *
 * 为什么必须换掉原来那个常量 420：28×28 档告诉模型画 ~20 栋，分母 420 意味着
 * 进度条永远只走到 5% 就停在那儿 —— 那不是「进度」，那是装饰。
 * 这里的分母来自**后端自己的计划**，所以它是有依据的，不是拍脑袋。
 */
export function planBuildingsOf(size: number): number {
  const n = Number(size) || 0
  if (n <= 0) return 0
  return Math.max(6, Math.round(n * 0.7))
}

/* ── 骨架屏的形状 ────────────────────────────────────────────────────────
 * 骨架屏只有「形状贴近真内容」才有意义（否则内容回来时会跳版）。
 * 宽度必须是**确定性**的：用随机数会让每次渲染都不一样，看起来像内容在抖。 */
const SK_WIDTHS = [0.94, 0.72, 0.86, 0.62, 0.9, 0.68, 0.8, 0.58, 0.88, 0.74, 0.66, 0.84]

/** 第 index 行骨架条的宽度（0.5~1 的比例，确定性循环） */
export function skeletonWidth(index: number): number {
  const i = Math.abs(Math.trunc(Number(index) || 0)) % SK_WIDTHS.length
  return SK_WIDTHS[i]
}
