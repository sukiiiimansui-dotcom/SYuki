// 「世界模拟」P4-3：交通工具的**唯一真源** —— 方式 → 立绘 / 速度 / 中文名
//
// 为什么单独一个纯 TS 文件（而不是塞进组件里）：
//   · 行程卡（要画小图标 + 写「步行」）、地图标记（要画车）、以及将来「出行方式选择器」
//     三处都要同一张表 —— 各写一份必然会漂移（改了一个忘了另一个）；
//   · 纯数据 + 纯函数，没有 Vue 依赖，将来要 `node xxx.mjs` 自检也拉得动。
//
// 与后端的对齐关系（`src-tauri/src/world_map/move.rs` 的 `MoveKind`）：
//   · key（walk/bike/…）   —— 逐字照抄后端的 `MoveKind::key()`，**不翻译**
//   · speed_mps            —— 逐字照抄后端的 `MoveKind::speed_mps()`（步行 1.4 / 高铁 70 / 飞机 220）
//   · zh（中文名）          —— 里子是**后端权威值优先**：`Trip.kind_zh` 有就用它，
//                             这里这份只是「后端没给 kind_zh 时」的兜底（见 kindLabelOf）
//   · ferry（轮渡）         —— 后端 `MoveKind` **没有**这一档（9 种），但美术已经把
//                             `ferry.svg` 做出来了。这里先收录，等后端加了就能直接用；
//                             speed 是前端估的（8 m/s ≈ 29 km/h，客轮巡航档），已在下面标注。
//
// ⚠️ 素材目录 `public/world_map/vehicles/` 由美术维护，文件名 == kind（`<kind>.svg`）。
//    10 个都是自包含扁平 SVG（viewBox 0 0 64 64，< 1KB），直接当 `<img src>` 用。

/** 出行方式（与后端 `MoveKind::key()` 逐字一致；`ferry` 目前后端还没有） */
export type WsVehicleKind =
  | 'walk'
  | 'bike'
  | 'ebike'
  | 'bus'
  | 'subway'
  | 'taxi'
  | 'car'
  | 'train'
  | 'plane'
  | 'ferry'

export interface WsVehicle {
  kind: WsVehicleKind
  /** 中文名（后端 `kind_zh` 缺失时的兜底；有 kind_zh 时以它为准，见 kindLabelOf） */
  zh: string
  /** 立绘 URL（`public/world_map/vehicles/<kind>.svg`，已按 BASE_URL 拼好） */
  svg: string
  /** 现实速度（米/秒）—— 与后端 `MoveKind::speed_mps()` 一致 */
  speed_mps: number
  /**
   * 这一类**不画车**：人自己走（P4-3 明确「walk 可以不画车」）。
   * 地图上步行时只画人 —— 脚底下再压一辆「步行车」既不存在也难看。
   */
  bare: boolean
}

/* ── 立绘根路径 ─────────────────────────────────────────────────────────────
 * `public/` 是 Vite 的静态根，本仓库既有代码也是写绝对路径的
 * （`WorldMap.vue` 加载 `/world_map/world_weather.js`）。
 * 这里仍然过一遍 `import.meta.env.BASE_URL`：将来若把应用挂到子路径下
 * （或给 Tauri 设 `base: './'`），只有这一个地方要改，不会散落一堆写死的 `/world_map/`。 */
const BASE: string = (() => {
  try {
    const b = String(import.meta.env?.BASE_URL || '/')
    return b.endsWith('/') ? b : `${b}/`
  } catch {
    return '/'
  }
})()

const SVG_DIR = 'world_map/vehicles/'

/** kind → 立绘 URL（导出的，供 `WsVehicleMark` 直接用） */
export function vehicleSvgUrl(kind: string): string {
  return `${BASE}${SVG_DIR}${vehicleKindOf(kind)}.svg`
}

/* ── 那张表 ────────────────────────────────────────────────────────────────
 * 顺序与后端 `MoveKind::ALL` 一致（步行 → 自行车 → … → 飞机），ferry 垫在最后。 */
export const WS_VEHICLES: Record<WsVehicleKind, WsVehicle> = {
  walk: { kind: 'walk', zh: '步行', svg: '', speed_mps: 1.4, bare: true },
  bike: { kind: 'bike', zh: '自行车', svg: '', speed_mps: 4.2, bare: false },
  ebike: { kind: 'ebike', zh: '电动车', svg: '', speed_mps: 6.0, bare: false },
  bus: { kind: 'bus', zh: '公交', svg: '', speed_mps: 8.0, bare: false },
  subway: { kind: 'subway', zh: '地铁', svg: '', speed_mps: 11.0, bare: false },
  taxi: { kind: 'taxi', zh: '出租车', svg: '', speed_mps: 10.0, bare: false },
  car: { kind: 'car', zh: '私家车', svg: '', speed_mps: 12.0, bare: false },
  train: { kind: 'train', zh: '高铁', svg: '', speed_mps: 70.0, bare: false },
  plane: { kind: 'plane', zh: '飞机', svg: '', speed_mps: 220.0, bare: false },
  // 后端 MoveKind 里还没有轮渡；速度是前端估的（见文件头说明）
  ferry: { kind: 'ferry', zh: '轮渡', svg: '', speed_mps: 8.0, bare: false },
}

/** 全部 kind（按「慢 → 快」排，UI 上要做选择器时直接用） */
export const WS_VEHICLE_KINDS: WsVehicleKind[] = Object.keys(WS_VEHICLES) as WsVehicleKind[]

// 把立绘 URL 补进表里（写成字面量会重复 10 遍 `world_map/vehicles/`，容易打错一个就 404）
for (const k of WS_VEHICLE_KINDS) {
  WS_VEHICLES[k].svg = `${BASE}${SVG_DIR}${k}.svg`
}

/** 认不出/为空时的兜底：**步行**（最保守的一档，绝不瞎猜成飞机） */
export const WS_VEHICLE_FALLBACK: WsVehicleKind = 'walk'

/**
 * 归一化一个 kind 字符串。
 *
 * 只认后端那套 key（大小写不敏感、去空白）。**不认中文别名**：
 * 中文 → key 的映射归后端 `MoveKind::parse()` 管（它才是权威，且带别名表）；
 * 前端再抄一份别名表，两边一改就是不一致。认不出 → `walk`（兜底，见上）。
 */
export function vehicleKindOf(raw: unknown): WsVehicleKind {
  const s = String(raw ?? '').trim().toLowerCase()
  return (WS_VEHICLE_KINDS as string[]).includes(s) ? (s as WsVehicleKind) : WS_VEHICLE_FALLBACK
}

/** 这个 kind 是不是后端认得的（`ferry` 目前 false —— 后端只有 9 种） */
export function isBackendVehicleKind(raw: unknown): boolean {
  const s = String(raw ?? '').trim().toLowerCase()
  return s !== 'ferry' && (WS_VEHICLE_KINDS as string[]).includes(s)
}

/** kind → 整条记录（永远有值，认不出给步行） */
export function vehicleOf(raw: unknown): WsVehicle {
  return WS_VEHICLES[vehicleKindOf(raw)]
}

/** kind → 立绘 URL（永远有值） */
export function vehicleIconOf(raw: unknown): string {
  return vehicleOf(raw).svg
}

/** 现实速度（米/秒）；后端给了 `speed_mps` 时**以行程里的为准**，这里只是兜底 */
export function vehicleSpeedOf(raw: unknown): number {
  return vehicleOf(raw).speed_mps
}

/** 这一类要不要画车（walk = 不画） */
export function isBareVehicle(raw: unknown): boolean {
  return vehicleOf(raw).bare
}

/**
 * 出行方式的中文名。
 *
 * **后端权威值优先**：`Trip.kind_zh` 是 Rust 侧 `MoveKind::zh()` 给的
 * （步行/骑车/电动车/公交/地铁/打车/开车/高铁/飞机），与 AI 提示词、行程事件
 * 里的用词是同一份，前端不该自作主张换一套说法。
 * 只有后端没给（老数据 / ferry 这种前端先收录的）才落到本地这张表。
 *
 * @param raw      后端的 `kind`（walk/bike/…）
 * @param backendZh 后端的 `kind_zh`（有就用它）
 * @param fallback 两者都没有时的兜底文案（默认「赶路」）
 */
export function kindLabelOf(raw: unknown, backendZh?: unknown, fallback = '赶路'): string {
  const zh = String(backendZh ?? '').trim()
  if (zh) return zh
  const s = String(raw ?? '').trim()
  if (!s) return fallback
  const hit = (WS_VEHICLE_KINDS as string[]).includes(s.toLowerCase())
  return hit ? WS_VEHICLES[s.toLowerCase() as WsVehicleKind].zh : fallback
}
