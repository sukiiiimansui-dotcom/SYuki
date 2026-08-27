import { defineStore } from 'pinia'
import type { NetMusicSong } from '@/api/services/netmusic'

/**
 * 网易云播放全局状态（L-SYuki）。
 * 独立于游戏 BGM（currentBackgroundMusic / AudioAcrossFade），
 * 拥有自己的队列与音量，实现「后台播放、不打扰游戏运行时」。
 */
interface NetMusicState {
  /** 播放队列 */
  queue: NetMusicSong[]
  /** 当前播放的歌曲 */
  current: NetMusicSong | null
  /** 是否正在播放 */
  playing: boolean
  /** 独立音量 0-100 */
  volume: number
  /** 当前播放进度/时长（秒），用于前端进度条 */
  progress: number
  duration: number
}

export const useNetmusicStore = defineStore('netmusic', {
  state: (): NetMusicState => ({
    queue: [],
    current: null,
    playing: false,
    volume: 80,
    progress: 0,
    duration: 0,
  }),
  actions: {
    /** 用一组歌曲替换队列并播放第一首 */
    setQueue(songs: NetMusicSong[], playNow = true) {
      this.queue = songs
      if (songs.length && playNow) {
        this.current = songs[0]
        this.playing = true
        this.progress = 0
        this.duration = songs[0].duration || 0
      }
    },
    /** 直接指定播放某首歌（追加到队列头） */
    playSong(song: NetMusicSong) {
      if (!this.queue.some((s) => s.url === song.url)) {
        this.queue.unshift(song)
      }
      this.current = song
      this.playing = true
      this.progress = 0
      this.duration = song.duration || 0
    },
    toggle() {
      this.playing = !this.playing
    },
    next() {
      if (!this.queue.length) return
      const idx = this.current
        ? this.queue.findIndex((s) => s.url === this.current!.url)
        : -1
      const nextIdx = idx < 0 ? 0 : (idx + 1) % this.queue.length
      this.current = this.queue[nextIdx]
      this.playing = true
      this.progress = 0
      this.duration = this.queue[nextIdx].duration || 0
    },
    setVolume(v: number) {
      this.volume = Math.max(0, Math.min(100, v))
    },
    clear() {
      this.queue = []
      this.current = null
      this.playing = false
      this.progress = 0
      this.duration = 0
    },
  },
})
