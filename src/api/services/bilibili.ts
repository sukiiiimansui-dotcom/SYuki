import { invoke } from '@tauri-apps/api/core'

export interface BiliVideo {
  bvid: string
  title: string
  up: string
  tname: string
  vdesc: string
  repeat_danmaku: string
  top_comments: string
  culture: string
  learned_at: string
}

export interface BiliSearchItem {
  bvid: string
  title: string
  up: string
  play: number
  like: number
  desc: string
}

export interface BiliLearnResult {
  ok: boolean
  bvid: string
  title: string
  up: string
  danmaku: number
  repeat: number
  comments: number
  culture: string
}

/** B站热榜 bvid 列表 */
export const biliHot = (limit = 10): Promise<string[]> =>
  invoke<string[]>('bili_hot', { limit })

/** B站视频搜索 */
export const biliSearch = (query: string, limit = 8): Promise<BiliSearchItem[]> =>
  invoke<BiliSearchItem[]>('bili_search', { query, limit })

/** 学习一个视频 */
export const biliLearn = (bvid: string): Promise<BiliLearnResult> =>
  invoke<BiliLearnResult>('bili_learn', { bvid })

/** 查询学习库 */
export const biliKnowledge = (q = '', limit = 20): Promise<BiliVideo[]> =>
  invoke<BiliVideo[]>('bili_knowledge', { q, limit })
