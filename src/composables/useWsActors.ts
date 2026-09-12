// 「世界模拟」P2：地图上的「人」——数据层（谁在地图上、站哪儿、头像是什么、点开看什么）
//
// 分工（与 P1 的三件套保持一致）：
//   · 纯逻辑（坐标换算/重叠错开/存储校验）→ wsActors.ts（可用 node 自检）
//   · 本文件：把 LingChat 的**真实数据**（游戏角色、角色目录、日程、runtime、
//     Tauri 命令）汇成一张「地图上的人」的清单，并把 UI 状态（选中谁、面板开不开）管起来
//   · 组件只负责画
//
// ⚠️ 性能红线（机主明确要求，改之前先读）：
//   角色立绘是 3511×5242 的 webp（**解码后 ≈73MB 一张**）。所以：
//     ① 地图上的头像**绝不**用立绘 —— 走 `get_avatar_file`，并且优先要
//        `avatar/头像.webp`（LingChat 素材里每套立绘都自带的小方图，≈45KB）；
//     ② 只有「立绘侧边栏」打开时才解析立绘，关掉就把 src 置空（见 WsPortraitDrawer）；
//     ③ 任何一次都不允许「预加载所有人的立绘」。
//
// 复用而不是造轮子：
//   · 头像路径解析 = `invoke('get_avatar_file')`（与 GameRoleAvatar.vue 同一条命令、同一种拼法）
//   · 情绪 → 文件名 = `EMOTION_CONFIG_EMO`（controllers/emotion/config.ts，官方那张表）
//   · 角色名单兜底 = `useWorldMapBindings.loadWorldCharacters()`（已经处理了双通路）
//   · 日程 / runtime / 时间 / 天气 = `worldMapApi`（已经处理了 Tauri↔HTTP 双通路）

import { computed, ref, type Ref } from 'vue'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import worldMapApi, { isTauriRuntime } from '@/api/services/worldMap'
import { getRoleMemoryBank, type RoleMemoryView } from '@/api/services/memory'
import { loadWorldCharacters } from '@/composables/useWorldMapBindings'
import { useGameStore } from '@/stores/modules/game'
import { EMOTION_CONFIG_EMO } from '@/controllers/emotion/config'
import {
  K_ME_AVATAR,
  checkMeAvatarData,
  parseMeAvatar,
  readRuntimePos,
  scatterGrid,
  scheduleLine,
  spreadCrowd,
  type ActorPosSource,
  type MapActor,
  type MeAvatarStored,
  type PlacedActor,
} from '@/components/views/worldsim/wsActors'

/* ── 小工具 ────────────────────────────────────────────────────────────── */

function readLS(key: string): string {
  try {
    return localStorage.getItem(key) || ''
  } catch {
    return ''
  }
}
function writeLS(key: string, val: string) {
  try {
    localStorage.setItem(key, val)
  } catch {
    /* 写不进去就只在本次会话生效，不该让页面报错 */
  }
}

/**
 * 情绪 → 立绘文件名（不含扩展名）。
 *
 * 与 GameRoleAvatar.vue 完全同一条规则：先过 `EMOTION_CONFIG_EMO` 做一次官方映射
 * （`哭泣`→`伤心`、`难为情`→`羞耻` 这种），映射不到就用「正常」。
 * 空情绪也算「正常」——`gameRoles[..].emotion` 在没进过游戏时就是空的。
 */
export function emotionFile(emotion: string): string {
  const e = String(emotion || '').trim()
  if (!e) return '正常'
  return EMOTION_CONFIG_EMO[e] || '正常'
}

/** 服装：`默认`/空 → `default`（与 GameRoleAvatar.vue 的 clothesName 处理逐字一致） */
export function clothesKey(clothesName: string): string {
  const c = String(clothesName || '').trim()
  return !c || c === '默认' ? 'default' : c
}

/* ── 头像解析（带缓存 + 三档降级）────────────────────────────────────────── */

/**
 * 头像 URL 缓存。
 *
 * 为什么 key 里带 `small`：小方图与情绪立绘是**两张不同的文件**，
 * 共用 key 会让「先要小的、再要情绪的」互相顶掉（画面上表现为切情绪不换脸）。
 */
const avatarCache = new Map<string, string>()

async function avatarUrlOf(folder: string, emotion: string, clothes: string): Promise<string> {
  const key = `${folder}|${emotion}|${clothes}|small`
  const hit = avatarCache.get(key)
  if (hit !== undefined) return hit
  // 纯 web 预览（含 web-mock）：Tauri 命令一律不存在 —— 直接画占位，
  // 别去 invoke 一个注定失败的 mock（与 worldMap.ts 里 isTauriRuntime() 的用法同款纪律）
  if (!folder || !isTauriRuntime()) return ''
  let url = ''
  try {
    // ① 首选角色自带的**头像小方图**（avatar/头像.webp，≈45KB）
    const p = await invoke<string>('get_avatar_file', {
      characterFolder: folder,
      emotion: '头像',
      clothesName: clothes,
    })
    url = convertFileSrc(p)
  } catch {
    // ② 没有小方图 → 退到情绪立绘（与聊天里同一张，只是显示得小）
    try {
      const p = await invoke<string>('get_avatar_file', {
        characterFolder: folder,
        emotion: emotionFile(emotion),
        clothesName: clothes,
      })
      url = convertFileSrc(p)
    } catch {
      url = '' // ③ 都没有 → 画占位（名字首字），绝不空着也不报错
    }
  }
  avatarCache.set(key, url)
  return url
}

/* ── composable ────────────────────────────────────────────────────────── */

export interface UseWsActorsOptions {
  /** 小区图的网格边长（与草图的 size 一致，默认 28） */
  grid?: Ref<number>
  /** 玩家所在的行政区文案（面板上「我的位置」用） */
  areaLabel?: Ref<string>
}

export function useWsActors(opts: UseWsActorsOptions = {}) {
  const grid = opts.grid || ref(28)
  const gameStore = useGameStore()

  /** 地图上的人（未错开，gx/gy 是原始坐标） */
  const actors = ref<MapActor[]>([])
  /** 载入中（第一次进小区图时用） */
  const loading = ref(false)
  const loadError = ref('')
  /** 后端的 runtime 快照（facilities / me / scene 也在这里面） */
  const runtime = ref<Record<string, unknown> | null>(null)
  /** 最近一次日程（面板里按角色名查） */
  const schedule = ref<Awaited<ReturnType<typeof worldMapApi.schedule>> | null>(null)
  /** 上次取日程时**有没有**拿到设施表 —— 决定 kind-only 的结果能不能继续用（见 ② 的注释） */
  let scheduleHadFacilities = false
  /** 当前时间 + 天气（P2-4 的第 3 项） */
  const nowText = ref('')
  const weatherText = ref('')

  const meAvatar = ref<MeAvatarStored>(parseMeAvatar(readLS(K_ME_AVATAR)))

  /* ── 玩家头像 ─────────────────────────────────────────────────────────── */
  const meAvatarUrl = computed(() => {
    const a = meAvatar.value
    if (!a.value) return ''
    // 存的是设备绝对路径时必须过 convertFileSrc（与 GameRoleAvatar 的用法一致）；
    // 浏览器预览里存的是 data URL，直接用。
    return a.kind === 'file' ? convertFileSrc(a.value) : a.value
  })

  /** 设置玩家头像：data = 内联图（浏览器/小图）；file = 设备路径（Tauri） */
  function setMeAvatar(next: MeAvatarStored) {
    meAvatar.value = next
    writeLS(K_ME_AVATAR, JSON.stringify(next))
  }

  /**
   * 保存一张「已经变成 data URL」的玩家头像（走本地上传那条路）。
   * 校验不通过时**返回原因**（由调用方 toast 出来），静默失败是最糟的体验。
   */
  function setMeAvatarData(dataUrl: string): { ok: boolean; reason: 'empty' | 'notimage' | 'toolarge' | '' } {
    const r = checkMeAvatarData(dataUrl)
    if (!r.ok) return r
    setMeAvatar({ kind: 'data', value: dataUrl })
    return r
  }

  function clearMeAvatar() {
    meAvatar.value = { kind: '', value: '' }
    writeLS(K_ME_AVATAR, '')
  }

  /* ── 名单装配 ─────────────────────────────────────────────────────────── */

  /**
   * 角色名 → roleId 的映射。
   *
   * 为什么要它：`get_role_memory_bank` / 「去找他聊聊」都要**角色主键**，
   * 而地图上的名字来自不同来源。三档来源按可信度排：
   *   ① gameStore.gameRoles（正在玩的时候最准，带 folder 与实时情绪）
   *   ② world_map_runtime 的 current_role（只有名字，但说明后端认得这个人）
   *   ③ LingChat 角色列表（character_id 就是主键）
   */
  function roleIdOfName(name: string): number {
    for (const r of Object.values(gameStore.gameRoles || {})) {
      if (r && r.roleName === name) return r.roleId
    }
    return 0
  }

  /** 实时情绪（gameRoles 里有就用，没有就算空 → 头像落到「正常」） */
  function emotionOfName(name: string, folder: string): string {
    for (const r of Object.values(gameStore.gameRoles || {})) {
      if (r && (r.roleName === name || (folder && r.character_folder === folder))) return r.emotion || ''
    }
    return ''
  }

  /** 服装（同上；只用于取头像，不做换装） */
  function clothesOfName(name: string, folder: string): string {
    for (const r of Object.values(gameStore.gameRoles || {})) {
      if (r && (r.roleName === name || (folder && r.character_folder === folder))) return clothesKey(r.clothesName)
    }
    return 'default'
  }

  /**
   * 装配「地图上的人」。
   *
   * 三条数据通路**各自独立兜底**（任何一条挂了，其余仍然出人）：
   *   · 角色名单：gameRoles → loadWorldCharacters（内部还有 LingChat 接口 → schedule.characters 两级）
   *   · 位置：runtime.actors → 日程里该角色当前活动的设施点 → 本地圆环散点
   *   · 头像：get_avatar_file（小方图 → 情绪立绘 → 占位）
   * 最后统一交给 `spreadCrowd` 错开重叠。
   */
  async function load(): Promise<MapActor[]> {
    loading.value = true
    loadError.value = ''
    try {
      /* ① runtime（拿后端已知的坐标/设施/场景）——失败不影响出人 */
      let actorsRaw: unknown = null
      try {
        // 只在真壳里 invoke：web 预览的 mock 对所有 world_map_* 一律返回 undefined
        const rt = isTauriRuntime() ? await invoke<Record<string, unknown>>('world_map_runtime', {}) : null
        runtime.value = rt || null
        actorsRaw = (rt as Record<string, unknown>)?.actors ?? null
      } catch {
        runtime.value = null
      }

      /* ② 日程（拿「现在在做什么 / 在哪个设施」）——失败不影响出人
       *
       * ⚠️ 必须把 runtime 里那份**设施表**带回去：后端只有拿到它，才能把日程里的
       * "商业区/住宅区"这种**类型**落到**具体设施点**上（给 `place.name`）。
       * 不带的话 `place` 恒为空 → 推给 Rust 的 `facility` 也是空 →
       * 事件引擎判室内外恒为「户外」→ 停电/失眠那批仅室内事件永不触发（静默）。
       * 缓存的判据也因此加上"当时有没有设施表"：首轮设施表还没装好时算出来的
       * kind-only 结果不能一直用下去。 */
      const facs = (runtime.value?.facilities as unknown[] | undefined) || []
      const facsReady = facs.length > 0
      if (!schedule.value || (facsReady && !scheduleHadFacilities)) {
        try {
          schedule.value = await worldMapApi.schedule(undefined, undefined, facs)
          scheduleHadFacilities = facsReady
        } catch {
          schedule.value = null
        }
      }
      const sch = schedule.value
      const schByName = new Map<string, NonNullable<typeof sch>['roles'][number]>()
      for (const r of sch?.roles || []) {
        if (r?.name) schByName.set(r.name, r)
      }

      /* ③ 名单：优先 gameRoles，再补 LingChat 角色列表 */
      const list: { name: string; folder: string; roleId: number; subtitle: string }[] = []
      const seen = new Set<string>()
      const push = (name: string, folder: string, roleId: number, subtitle = '') => {
        const k = name || folder
        if (!k || seen.has(k)) return
        seen.add(k)
        list.push({ name: name || folder, folder, roleId, subtitle })
      }
      const liveRoles = Object.values(gameStore.gameRoles || {}).filter(Boolean)
      for (const r of liveRoles) {
        push(r.roleName || r.character_folder || String(r.roleId), r.character_folder || '', r.roleId, r.roleSubTitle || '')
      }
      try {
        for (const c of await loadWorldCharacters()) {
          const id = Number(c.id)
          push(c.name || c.folder, c.folder, Number.isFinite(id) ? id : 0, '')
        }
      } catch {
        /* 角色列表也挂了：至少 gameRoles 里的人还在 */
      }

      /* ④ 逐个人装配：位置 → 情绪/头像 */
      const total = list.length
      const out: MapActor[] = []
      // 头像解析**并发**发起（每人最多 2 次 invoke），但每批最多 4 个人：
      //   串行 = 十几个角色要等十几轮 IPC（手机上肉眼可见地卡）；
      //   全并发 = 一次性把十几个 webp 塞进图片解码队列，低端机容易顶到内存峰值。
      const AVATAR_BATCH = 4
      const avatarUrls: string[] = new Array(total).fill('')
      for (let i = 0; i < total; i += AVATAR_BATCH) {
        const slice = list.slice(i, i + AVATAR_BATCH)
        const got = await Promise.all(
          slice.map((it) => avatarUrlOf(it.folder, emotionOfName(it.name, it.folder), clothesOfName(it.name, it.folder))),
        )
        got.forEach((u, k) => {
          avatarUrls[i + k] = u
        })
      }
      for (let i = 0; i < total; i++) {
        const it = list[i]
        const rtPos = readRuntimePos(actorsRaw, it.name) || readRuntimePos(actorsRaw, it.folder)
        const line = scheduleLine(schByName.get(it.name))
        let gx: number
        let gy: number
        let posSource: ActorPosSource
        let place = ''
        if (rtPos) {
          gx = rtPos.gx
          gy = rtPos.gy
          place = rtPos.place || line.place
          posSource = 'runtime'
        } else {
          const s = scatterGrid(i, total, grid.value)
          gx = s.x
          gy = s.y
          place = line.place
          posSource = 'schedule'
        }
        const emotion = emotionOfName(it.name, it.folder)
        out.push({
          id: `r${it.roleId || it.folder || i}`,
          roleId: it.roleId || roleIdOfName(it.name),
          name: it.name,
          subtitle: it.subtitle,
          folder: it.folder,
          emotion,
          avatarUrl: avatarUrls[i] || '',
          isMe: false,
          gx,
          gy,
          posSource,
          place,
          nowText: line.nowText,
        })
      }

      /* ⑤ 玩家自己（永远有 —— 没有玩家头像时画占位） */
      const meRaw = (runtime.value?.me || null) as Record<string, unknown> | null
      const mx = Number(meRaw?.gx ?? meRaw?.x)
      const my = Number(meRaw?.gy ?? meRaw?.y)
      const meHas = Number.isFinite(mx) && Number.isFinite(my)
      out.unshift({
        id: 'me',
        roleId: 0,
        name: '',
        subtitle: '',
        folder: '',
        emotion: '',
        avatarUrl: meAvatarUrl.value,
        isMe: true,
        gx: meHas ? mx : grid.value / 2,
        gy: meHas ? my : grid.value / 2,
        posSource: 'me',
        place: String(meRaw?.place || meRaw?.area || ''),
        nowText: '',
      })

      actors.value = out
      return out
    } catch (e) {
      // 「绝不能白屏」：装配整体失败时说清楚，并把上一次的清单留着
      loadError.value = e instanceof Error ? e.message : String(e)
      return actors.value
    } finally {
      loading.value = false
    }
  }

  /**
   * 带重叠错开的最终清单（**视图直接用这个**）。
   *
   * 顺序固定为「角色在前、玩家在后」：玩家永远压在最上面（不然被角色盖住就点不到了）。
   * 错开只作用在**角色**之间，玩家保持自己的真实坐标。
   */
  const placed = computed<PlacedActor[]>(() => {
    const list = actors.value || []
    const roles = list.filter((a) => !a.isMe)
    const me = list.find((a) => a.isMe) || null
    const spread = spreadCrowd(
      roles.map((a) => ({ x: a.gx, y: a.gy })),
      grid.value,
      // 命中半径：格子边长的 5.5%（28 格 → 1.54 格）。太小等于没散，太大会把人挪到别的小区。
      Math.max(0.6, grid.value * 0.055),
    )
    const out: PlacedActor[] = roles.map((a, i) => ({
      ...a,
      px: spread[i]?.x ?? a.gx,
      py: spread[i]?.y ?? a.gy,
      crowd: spread[i]?.crowd ?? 1,
    }))
    if (me) out.push({ ...me, px: me.gx, py: me.gy, crowd: 1 })
    return out
  })

  /* ── 时间 / 天气（P2-4 第 3 项）────────────────────────────────────────── */
  async function loadTimeWeather() {
    // 时间：后端有就以后端为准（它带设备时区），拿不到就用本地时钟（永远有，绝不空着）
    try {
      const t = await worldMapApi.time()
      const s = String((t as Record<string, unknown>)?.time || (t as Record<string, unknown>)?.now || '').trim()
      nowText.value = s || localClock()
    } catch {
      nowText.value = localClock()
    }
    try {
      const w = await worldMapApi.weather()
      weatherText.value = describeWeather(w)
    } catch {
      weatherText.value = '' // 拿不到天气就空着（面板显示「暂无」），不编
    }
  }

  function localClock(): string {
    const d = new Date()
    const p = (n: number) => String(n).padStart(2, '0')
    return `${p(d.getHours())}:${p(d.getMinutes())}`
  }

  /* ── 角色资料（面板用，按需拉取并缓存）────────────────────────────────── */
  const memoryCache = new Map<number, RoleMemoryView | null>()

  /** 记忆摘要：拿不到返回 null（面板显示空态），不抛 */
  async function loadMemory(roleId: number): Promise<RoleMemoryView | null> {
    if (!roleId) return null
    if (memoryCache.has(roleId)) return memoryCache.get(roleId) ?? null
    let v: RoleMemoryView | null = null
    try {
      v = await getRoleMemoryBank(roleId)
    } catch {
      v = null
    }
    memoryCache.set(roleId, v)
    return v
  }

  /** 该角色的日程条目（今天的一整天 + 现在/接下来） */
  function scheduleOf(name: string) {
    if (!schedule.value) return null
    return (schedule.value.roles || []).find((r) => r.name === name) || null
  }

  /**
   * 服装变体清单：拿「角色设置里的 clothes[]」当候选，
   * 再逐个用 `get_avatar_file` 验证**默认这套**能不能取到图（取不到就不列，免得点了没反应）。
   *
   * 为什么只验默认服装：切到某个变体是侧边栏里按需再取的；
   * 一次性把每个变体的 20 张情绪图都探一遍在手机上太慢，也没必要。
   */
  async function clothesVariants(folder: string): Promise<string[]> {
    const out: string[] = ['default']
    if (!folder) return out
    const roleId = (Object.values(gameStore.gameRoles || {}).find((r) => r?.character_folder === folder)?.roleId) || 0
    if (!roleId) return out
    try {
      const { getRoleSettings } = await import('@/api/services/character')
      const s = (await getRoleSettings(roleId)) as { clothes?: { name?: string }[] }
      const list = Array.isArray(s?.clothes) ? s.clothes : []
      for (const c of list) {
        const n = String(c?.name || '').trim()
        if (n && n !== '默认' && !out.includes(n)) out.push(n)
      }
    } catch {
      /* 拿不到角色设置就只留「默认」这套，不报错 */
    }
    return out
  }

  /* ── 天气描述（形状不受控 → 一律兜住）────────────────────────────────── */
  function describeWeather(w: unknown): string {
    if (!w || typeof w !== 'object') return ''
    const root = w as Record<string, unknown>
    const cur = (root.current || root.weather || root) as Record<string, unknown>
    const desc = String(cur?.desc ?? cur?.weather_desc ?? cur?.text ?? '').trim()
    const temp = cur?.temp_c ?? cur?.tempC ?? cur?.temp
    const t = Number(temp)
    const parts: string[] = []
    if (desc) parts.push(desc)
    if (Number.isFinite(t)) parts.push(`${Math.round(t)}°C`)
    return parts.join(' ')
  }

  /** 行政区 + 地点名（面板「位置」那一项） */
  const myAreaLabel = computed(() => opts.areaLabel?.value || '')

  return {
    // 状态
    actors,
    placed,
    loading,
    loadError,
    runtime,
    schedule,
    nowText,
    weatherText,
    meAvatar,
    meAvatarUrl,
    myAreaLabel,
    // 动作
    load,
    loadTimeWeather,
    loadMemory,
    scheduleOf,
    clothesVariants,
    setMeAvatar,
    setMeAvatarData,
    clearMeAvatar,
    refreshAvatars: () => avatarCache.clear(),
  }
}

export type WsActors = ReturnType<typeof useWsActors>
