// 「世界模拟」词条（namespace: worldsim）
//
// 为什么新开一个命名空间、而不是往 views.ts 里塞：
//   ① 这一整套功能（P1~P5）会有几十条文案，混进 views.ts 会让那个文件越来越难维护；
//   ② 官方规矩是**只写 zh-CN 就合规**（i18n 的 fallbackLocale 是 zh-CN，
//      en/ja/zh-HK 缺键时运行时自动回落中文），所以这里先只落中文。
//
// 注意：应用启动时会把内置词条**播种**到数据目录（见 locales/index.ts 的 BUNDLE_VERSION），
// 所以这里改了文案要重新播种才生效 —— 词条版本号是由全部内置词条算出来的哈希，
// 新增命名空间会让版本变化，下次启动会自动重新播种，不用手动清缓存。
export default {
  /** 主菜单入口：主菜单 →「开始游戏」那一排的第四个 */
  entry: '世界模拟',
  title: '世界模拟',
  /** 主按钮 */
  enter: '进入',
  generate: '生成小区地图',
  reselect: '换个区县',
  reLocate: '重新定位',
  manual: '手动选城市',
  ipEstimate: '用 IP 估测',
  restart: '重新引导',
  back: '返回',
  retry: '重试',
  refresh: '刷新',
  loading: '读取中…',
  /** 阶段标签 */
  stage: {
    boot: '初始化',
    locating: '定位中',
    locateFailed: '待选位置',
    manual: '手动选择',
    confirm: '确认位置',
    neighborhood: '小区',
  },

  /* ══ P2：地图上的人 ══════════════════════════════════════════════════ */
  /** 玩家自己在地图上的称呼（没有设置用户名时用） */
  actor: {
    me: '我',
  },
  /** 「即将上线」这类占位动作的统一后缀（点了必须有反应，绝不静默） */
  soon: '即将上线',
  emotionNormal: '正常',

  /** 位置来源：如实告诉用户这个坐标是怎么来的 */
  pos: {
    runtime: '世界运行中（后端实时）',
    schedule: '按日程推算',
    scatter: '本地散开（暂无位置数据）',
    me: '你的位置',
  },

  /** 角色面板（P2-3） */
  panel: {
    title: '角色',
    close: '收起面板',
    onStage: '在场',
    portrait: '立绘',
    portraitExpand: '点开查看立绘',
    portraitCollapse: '收起立绘',
    portraitLoading: '正在加载立绘…',
    portraitNone: '这个角色还没有立绘素材',
    portraitFailed: '立绘加载失败（文件可能在角色目录里被改过名）',
    clothes: '服装',
    clothesDefault: '默认',
    clothesMissing: '这套服装没有立绘，已切回默认',
    schedule: '日程',
    scheduleNow: '现在',
    scheduleNext: '接下来',
    location: '位置',
    area: '行政区',
    place: '地点',
    posSource: '位置来源',
    memory: '记忆',
    memoryOff: '这个角色没有开启永久记忆，下面是当前能读到的内容',
    memoryNoId: '没有拿到这个角色的角色库 ID，读不到记忆（多半是角色只在日程里出现过）',
    memShort: '近期',
    memLong: '长期经历',
    memUser: '关于你',
    memPromise: '约定与待办',
    expand: '展开全文',
    collapse: '收起',
    relation: '关系',
    goto: '去找他聊聊',
    gotoNow: '你们正在聊着，点了直接回对话',
    gotoWarn: '注意：这个角色不是当前对话对象，聊天里还要再选一次 ta',
    actions: '快捷动作',
  },

  /** 关系（后端没有好感度接口，这里的数值是从记忆文本里**如实推断**的） */
  rel: {
    deep: '很亲近',
    good: '关系不错',
    normal: '普通',
    cool: '有点疏远',
    tense: '有些紧张',
    fromMemory: '根据记忆内容估算（仅供参考）',
  },

  /** 快捷动作（P2-3 第 7 项：先做成按钮，后端能力后面接） */
  action: {
    hi: '打招呼',
    gift: '送礼物',
    outing: '约他出门',
    hint: '这三个动作的后端能力还没接，点了会提示「即将上线」，不会静默无反应。',
  },

  /** 对话（P2-3 第 6 项） */
  chat: {
    noRole: '这个角色没有角色库 ID，暂时去不了对话（ta 可能只在日程里出现过）',
    switchWarn: '「{name}」不是当前对话角色。\n继续会跳到聊天页，你需要在那边再选一次 ta。\n（这一步**不会**动你现在的对话记录）\n\n现在就过去吗？',
  },

  /** 自己的面板（P2-4） */
  me: {
    title: '我的面板',
    avatar: '我的头像',
    avatarUpload: '上传头像',
    avatarChange: '更换头像',
    avatarReset: '恢复默认',
    avatarResetDone: '已恢复默认头像',
    avatarSaved: '头像已保存',
    avatarTooLarge: '图片太大了（压到 256px 还是超过 1.4MB），换一张小点的试试',
    avatarBad: '这张图读不出来，换一张试试',
    avatarHint: '图片会先压到 256px 再存到本机（localStorage），只在这台设备的「世界模拟」里使用。',
    location: '我的位置',
    timeWeather: '时间与天气',
    time: '当前时间',
    weather: '天气',
    minimap: '众人小地图',
    minimapHint: '点谁就选中谁（小地图不参与地图的拖动缩放）',
    todo: '我的日程 / 待办',
  },

  /** P4-2/P4-3：行程（角色在地图上移动）由**页面**弹出来的那几条提示。
   * 行程卡与车辆标记**内部**的文案还在组件常量里（`WsTripCard` 的 ZH / `wsVehicles.ts`），
   * 按同名前缀陆续搬进来即可。 */
  trip: {
    arrivedToast: '{name} 已到达{place}',
    fastOn: '⚡ 已开到 {n}× 加速',
    fastOff: '已回到常速',
    cancelled: '已取消行程',
    nothingToCancel: '没有可取消的行程{reason}',
  },

  /** 空态（拿不到数据时如实说，绝不编内容） */
  empty: {
    schedule: '还没有今天的日程。日程可以从游戏里的「日程」设置添加，加完这里就能看到。',
    location: '还没定位到行政区',
    place: '地点未知',
    memory: '还没有记忆内容。多聊几句，长期记忆开启后这里会慢慢长出来。',
    relation: '暂无（要先有记忆内容才能推断关系）',
    weather: '暂无天气数据',
    actors: '地图上还没有人',
    todo: '今天没有待办',
    todoSub: '空着也是一种好日子 —— 想加点什么的话，去游戏里的日程设置看看。',
  },

  /** ══ P5-2：现实事件（事件流面板 + 三通道开关 + 地图气泡 + 与角色的记忆交接）══
   * 词条口径：面板里的**可见文字全部**在这里；模板中一个中文都不留（同 P2 的纪律）。 */
  events: {
    title: '世界事件',
    empty: '这个世界还很平静',
    emptySub: '安静也是一种好日子 —— 有事发生时，这里会一条条记下来。',
    refresh: '刷新',
    /** 三通道（默认全开）与每路的说明（鼠标悬停看） */
    channel: {
      bubble: '地图气泡',
      popup: '应用内提示',
      speech: '角色口述',
      bubbleHint: '事件发生时，在对应角色的头像上冒一个气泡（3~5 秒后消失）',
      popupHint: '在屏幕底部弹一条提示（好事绿、坏事黄、中性灰）',
      speechHint: '让角色在对话里自然地提一句',
    },
    /** 关于「角色口述」这一路，如实说清前端做了什么（后端注入不受这个开关影响） */
    channelHint: '角色口述由后端注入实现：事件会进对话的「最近：…」，角色下一轮自然知道。这个开关只控制本面板要不要展示口述提示。',
    speechPreview: '角色会这样说',
    /** 待写记忆（drain 取到的行） */
    memoryTitle: '待写记忆',
    memoryEmpty: '这一次没有取到待写记忆（队列是空的）',
    memoryAfter: '队列里还剩 {n} 行（别人的）',
    memoryHint: '取走即清空。事件文本已经通过「最近：…」进了角色的对话上下文，所以角色本来就「知道」——这里只是把取到的行如实显示出来。',
    /** 相对时间（纯函数只给描述符，文案在这一层拼） */
    rel: {
      justNow: '刚刚',
      minAgo: '{n} 分钟前',
      hourAgo: '{n} 小时前',
      dayAgo: '{n} 天前',
    },
    /** 引擎状态行：为什么还没出事 / 下次最早什么时候 */
    state: {
      idle: '还没开始（进到小区图后自动开始）',
      unsupported: '这个环境没有事件引擎（浏览器预览）',
      fired: '刚刚发生了一件事',
      no_candidate: '引擎醒着，这会儿没有合适的事件',
      throttled: '节流中：下次最早 {n} 秒后',
      no_scene: '世界模拟还没就位（后端没收到场景，这时不会产生事件）',
      unknown: '引擎回了一个不认识的状态',
      error: '读事件出错：{msg}',
    },
    /** 十大类别（与后端 Category 一一对应） */
    category: {
      weather: '天气',
      traffic: '交通',
      social: '社交',
      work: '工作学习',
      health: '健康',
      money: '消费',
      accident: '意外',
      festival: '节日',
      mood: '情绪',
      luck: '小确幸',
      unknown: '事件',
    },
  },
}
