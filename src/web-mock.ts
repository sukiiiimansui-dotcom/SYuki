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
  // —— CPU 检测：浏览器预览强制「最省性能」，让自动配置关闭高开销特效 ——
  if (c.includes('get_cpu_info') || c.includes('redetect_cpu')) return {
    brand: 'Snapdragon (mock / web preview)',
    tier: 'Internet',
    is_unknown: false,
    unknown_message: null,
  }
  // —— 记忆可视化面板 ——
  if (c.includes('get_role_memory_bank')) return {
    role_id: 1,
    role_name: '示例角色',
    memory_enabled: true,
    schema_version: 1,
    updated_at: '2026-08-28 13:00:00',
    short_term: [
      '刚刚在聊「天气」和「周末计划」，用户提到明天想去看海，心情不错。',
      '话题还没结束，可以接着问他要不要带相机。',
      '用户说最近晚上总失眠，想早点睡。',
      '上一条对话里用户分享了看到的一只流浪猫，想收养。',
      '今晚用户想吃火锅，问我能不能陪。',
    ].join('\n'),
    long_term: [
      '用户上周分享了第一次去咖啡馆做手冲的经历，说想以后自己学。',
      '用户养过一只叫「牛奶」的猫，三年前走丢了，一直记得。',
      '用户曾在海边城市读书，喜欢看日落。',
      '用户提过想换一份能远程的工作，好到处旅行。',
      '第一次见面时用户穿蓝色外套，聊了很多星座话题。',
    ].join('\n'),
    user_info: [
      '用户昵称：示例。喜欢猫、咖啡、深夜听歌。',
      '性格比较慢热，但熟悉后话很多。',
      '怕吵、怕辣、对海鲜过敏。',
      '最近在学手冲咖啡，也偶尔玩摄影。',
      '熬夜党，作息不大规律，想改。',
    ].join('\n'),
    promises: [
      '本周内一起尝试做一次手冲咖啡。',
      '约好下个月一起去海边看日落。',
      '答应帮他留意工作机会。',
      '约定了互相监督早睡。',
      '说好下次带猫粮去看那只流浪猫。',
    ].join('\n'),
  }
  // —— 主动/心跳「想念」状态与历史 ——
  if (c.includes('get_proactive_status')) return {
    enabled: true,
    running: true,
    can_deliver: true,
    last_interaction_ago_secs: 132,
    away_delivered_count: 1,
    away_max_times: 3,
    away_timeout_secs: 600,
    interest: 32.5,
    interest_cap: 50,
    proactive_times: 1,
    max_proactive_count: 3,
    state: 'IDLE',
    description: '用户似乎离开了，最近没有交互。',
    pending_intents: [
      { kind: 'todo', waited_secs: 45 },
    ],
    history: [
      { ts_ms: Date.now() - 132000, kind: 'miss', preview: '（示例）已经有一阵子没说话啦，有点想你…你在忙什么呀？' },
      { ts_ms: Date.now() - 900000, kind: 'alarm', preview: '（示例）提醒：我们约好的手冲时间快到啦。' },
    ],
  }
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
