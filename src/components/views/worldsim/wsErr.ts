// 把「技术错误」翻成「人话」——纯函数，可被 node 直接 import 做自检。
//
// ── 为什么必须单独一层 ────────────────────────────────────────────────────
// 浏览器/网络层的报错是英文技术串（`Failed to fetch`、`Load failed`、
// `NetworkError when attempting to fetch resource.`），直接 `e.message` 就漏给用户了。
// 真机上截图确认过两处：定位失败卡的副标题、以及三级选择器的「列表加载失败」。
// 用户看到「Failed to fetch」既不知道发生了什么、也不知道该做什么。
//
// ── 设计 ──────────────────────────────────────────────────────────────────
// 只分**类**（kind），文案交给 i18n（`worldsim.err.*`）—— 这样纯函数不依赖 vue-i18n，
// 能在 node 里直接跑断言；组件侧再 `t('worldsim.err.' + kind)`。
// 原始 message 仍然保留在 `detail` 里：**不丢证据**，排障时能拿到（放在可折叠的详情里）。

/** 错误大类：文案键就是 `worldsim.err.<kind>` */
export type WsErrKind = 'offline' | 'timeout' | 'toomany' | 'server' | 'unknown'

export interface WsErrInfo {
  kind: WsErrKind
  /** 原始技术信息（可能为空）—— 不显示给普通用户，但排障要有 */
  detail: string
}

/** 网络层错误的特征串（浏览器实现不一，各家说法都收着） */
const NET_PATTERNS = [
  'failed to fetch',
  'load failed',
  'networkerror',
  'network request failed',
  'net::err_',
  'err_connection',
  'err_internet',
  'err_name_not_resolved',
  'fetch failed',
  'econnrefused',
  'econnreset',
  'socket hang up',
]

const TIMEOUT_PATTERNS = ['timeout', 'timed out', 'etimedout', 'aborted']

/**
 * 把任意异常/字符串归类成「人话」。
 *
 * @param e 抛出来的东西（Error / string / 其它）
 * @returns `{ kind, detail }`；`detail` 是原样的技术信息，只用于排障
 */
export function classifyError(e: unknown): WsErrInfo {
  const detail = e instanceof Error ? e.message : typeof e === 'string' ? e : e ? String(e) : ''
  const low = detail.toLowerCase()

  if (TIMEOUT_PATTERNS.some((p) => low.includes(p))) return { kind: 'timeout', detail }
  if (NET_PATTERNS.some((p) => low.includes(p))) return { kind: 'offline', detail }

  // 后端自己的错误消息（Rust 侧用的是中文），带「失败 / 错误 / 不存在」这类词
  if (/失败|错误|不存在|超时|没有/.test(detail)) return { kind: 'server', detail }

  return { kind: 'unknown', detail }
}

/** 只有「网络不通」这一种值得把原始信息藏起来；其余原样透出更有用 */
export function errTextKey(e: unknown): { key: string; detail: string; showDetail: boolean } {
  const { kind, detail } = classifyError(e)
  return {
    key: `worldsim.err.${kind}`,
    detail,
    showDetail: kind === 'offline' || kind === 'timeout' || kind === 'unknown',
  }
}
