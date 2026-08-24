// 纯 web 预览 mock：伪造 Tauri __TAURI_INTERNALS__，让 LingChat 在无 Rust 后端也能启动渲染 UI。
// getCurrentWindow 读 metadata.currentWindow.label；invoke 按命令返回模拟数据（B站学习/网易云用示例，其余返回空）。
declare global {
  interface Window { __TAURI_INTERNALS__?: any }
}

let cbId = 0

function mockResult(cmd: string): any {
  const c = cmd.toLowerCase()
  // —— B站学习 ——
  if (c.includes('bili_hot')) return [
    { bvid: 'BV1xx411c7mD' }, { bvid: 'BV1Q5411a7Jv' },
  ]
  if (c.includes('bili_knowledge')) return [
    {
      bvid: 'BV1xx411c7mD', title: '示例视频：五分钟了解人工智能', up: 'UP主·小猫', tname: '知识',
      vdesc: '这是一个示例视频简介，用来预览学习卡片的展开详情布局。',
      repeat_danmaku: '“前排” “2333” “学到了”', top_comments: '[120赞] 讲得真好，已三连',
      culture: '示例弹幕文化：前排/致敬', learned_at: String(Math.floor(Date.now() / 1000)),
    },
  ]
  if (c.includes('bili_search')) return [
    { bvid: 'BV111', title: '示例 5 分钟带你上手', up: 'UP·示例', play: 1200, like: 88, desc: 'description' },
  ]
  if (c.includes('bili_learn')) return { ok: true, bvid: 'BV1xx411c7mD', title: '已学习示例视频', up: 'UP主·小猫', danmaku: 100, repeat: 10, comments: 5, culture: '' }
  // —— 网易云 ——
  if (c.includes('netmusic_search') || c.includes('netmusic_recommend')) return [
    {
      source: 'netease', title: '示例歌曲：晴天', artist: '示例歌手', album: '示例专辑',
      url: 'https://music.163.com/#/song?id=1', cover: '', duration: 200,
    },
    {
      source: 'netease', title: '示例歌曲：起风了', artist: '示例歌手', album: '示例专辑',
      url: 'https://music.163.com/#/song?id=2', cover: '', duration: 180,
    },
  ]
  if (c.includes('netmusic_url')) return 'https://music.163.com/song/media/outer/url?id=1.mp3'
  // —— 其它初始化为安全空值 ——
  if (c.includes('font') || c.includes('llm') || c.includes('provider') || c.includes('settings')
    || c.includes('character') || c.includes('background') || c.includes('scene')
    || c.includes('schedule') || c.includes('save') || c.includes('script') || c.includes('music')) {
    return []
  }
  return undefined
}

export function installWebMock() {
  if (window.__TAURI_INTERNALS__) return
  window.__TAURI_INTERNALS__ = {
    invoke: (cmd: string, args?: any) => Promise.resolve(mockResult(cmd)),
    transformCallback: (cb: any) => { cbId += 1; return cbId },
    unregisterCallback: () => {},
    convertFileSrc: (p: string) => p,
    metadata: { currentWindow: { label: 'main' } },
    // Channel / event 相关兜底
    channels: { __TAURI_CHANNEL_MARKER__: true, onmessage: () => {} },
  }
}
installWebMock()
