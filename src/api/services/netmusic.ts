import { invoke } from '@tauri-apps/api/core'

export interface NetMusicSong {
  source: string
  title: string
  artist: string
  album: string
  url: string
  cover: string
  duration: number
}

/** 网易云搜索歌曲 */
export const netmusicSearch = (keyword: string, limit = 8): Promise<NetMusicSong[]> =>
  invoke<NetMusicSong[]>('netmusic_search', { keyword, limit })

/** 心情推荐歌曲 */
export const netmusicRecommend = (mood = '', limit = 10): Promise<NetMusicSong[]> =>
  invoke<NetMusicSong[]>('netmusic_recommend', { mood, limit })

/** 外链播放地址 */
export const netmusicUrl = (songId: number): Promise<string> =>
  invoke<string>('netmusic_url', { songId })
