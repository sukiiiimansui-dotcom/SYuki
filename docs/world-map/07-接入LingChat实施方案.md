# 07 · 全面接入 LingChat 实施方案

> 目标：把 `world_map/` 那套 Python 原型（已验证可行的全部能力）**完整接入 LingChat 0.1.3**，
> 后端走 **Rust（Tauri commands）**、前端走 **Vue 3 组件**，产出 Web 版 + Android APK。
>
> 制定于 2026-09-11。原型侧代码约 6800 行 Python + 15 个页面。

---

## 一、总体原则

1. **原型 = 设计验证，Rust/Vue = 最终实现**
   Python 侧继续用来快速试错（改一行刷新即见），**设计定稿后才移植**，避免反复搬。
2. **分层不要串**
   - Rust：几何计算、数据获取与缓存、布局生成、SVG 拼装、持久化
   - Vue：页面、交互、动画、图层控制
   - 纯前端渲染模块（`transport.js` / `npc.js` / `world_time.js` / `world_weather.js` / `phone.js`）
     **不移植**，直接复制进 `public/world_map/` 由 Vue 复用
3. **不新增 Python 依赖**
   LingChat 是 Tauri 应用，装不了 Python 运行时；移植完成即彻底摆脱 8790 侧车。
4. **每个阶段都要能跑**
   不允许"移植一半跑不起来"——每阶段结束时 Web 版必须可用。

---

## 二、现状盘点

### 已完成（阶段 0）

| 位置 | 内容 | 行数 |
|---|---|---|
| `src-tauri/src/world_map/coord.rs` | 四层坐标系统 + 距离/方位/射线法（11 个单元测试） | 8.0KB |
| `src-tauri/src/world_map/geo.rs` | geojson 缓存优先 + 远程兜底、远处区块计算 | 13.9KB |
| `src-tauri/src/world_map/render.rs` | SVG 渲染（先于原型定型，已输出矢量） | 11.1KB |
| `src-tauri/src/world_map/mod.rs` | 7 个 `world_map_*` 命令已注册 | 9.0KB |
| `src/api/services/worldMap.ts` | 前端数据层，`USE_RUST` 一处切换 | 6.3KB |
| `WorldMap.vue` / `WorldMapLayer.vue` | 世界页 + 半透明叠加层（背景层/角落小窗） | 22.6KB |
| `useWorldMap*.ts` | 模块加载 / 数据绑定 / 图层状态 / 世界心跳 | 12.5KB |
| `public/world_map/*.js` | 5 个前端渲染模块已就位 | 283KB |

### 待移植（原型已验证）

| Python 模块 | 行数 | 能力 | 目标 Rust 模块 | 优先级 |
|---|---|---|---|---|
| `svg_render.py` | 491 | 小区 SVG：全细节 + z1/z2/z3 分层 + 2D/3D + 数据卡片 | `render.rs` 扩展 | **P0** |
| `svg_geo.py` | 285 | 地理 SVG：全国/省/市/区 + 可点击区划 + 抽稀 | `render_geo.rs` | **P0** |
| `details.py` | 273 | 街道细节：人行道/斑马线/行道树/停车位/路灯/公交站/红绿灯 | `details.rs` | **P0** |
| `sketch.py` | 167 | 本地规则草图（毫秒级，两段式第一段） | `sketch.rs` | **P0** |
| `stream_gen.py` | 300 | 流式生成 + 增量 JSON 解析 | `stream.rs` | **P0** |
| `stats.py` | 177 | 容积率/绿化率/人口估算/设施配比 | `stats.rs` | P1 |
| `facilities.py` | 700+ | 7 类生活设施 + 7 类交通设施 + 绘制 | `facilities.rs` | P1 |
| `transport.py` | 850 | 9 种交通工具 + 路径规划 + 行程推进 | `transport.rs` | P1 |
| `schedule.py` | 300 | LingChat 日程解析 + 日程→地图位置 | `schedule.rs` | P1 |
| `maplib.py` | 300 | 地图库（索引/分组/布局缓存/LRU 容量控制） | `maplib.rs` | P1 |
| `osm_fetch.py` | 100 | Overpass 真实地物（按 200m 网格缓存） | `osm.rs` | P2 |
| `hier_api.py` | 800+ | HTTP 服务（30+ 接口） | 拆成 Tauri commands | **P0** |

---

## 三、架构设计

### 后端（Rust）

```
src-tauri/src/world_map/
├── mod.rs          模块入口 + 全部 #[tauri::command]
├── coord.rs        ✅ 坐标/几何/距离/方位（已完成）
├── geo.rs          ✅ geojson 获取与缓存、远处区块（已完成）
├── render.rs       ⏳ SVG 渲染基座（已完成）→ 扩展：细节/分层/2D3D/图表
├── render_geo.rs   ⏳ 行政区划级 SVG（全国/省/市/区）
├── details.rs      ⏳ 街道细节生成（纯几何，最容易移植）
├── sketch.rs       ⏳ 本地规则草图
├── stream.rs       ⏳ 流式生成（复用 LingChat 的 LlmClient）
├── stats.rs        ⏳ 统计指标
├── facilities.rs   ⏳ 设施
├── transport.rs    ⏳ 交通
├── schedule.rs     ⏳ 日程（读 game_data/schedules.json）
├── maplib.rs       ⏳ 地图库
└── osm.rs          ⏳ Overpass
```

**关键复用点**（不用自己写）：

```rust
// 拿 LLM 客户端（LingChat 已封装好，含流式）
let llm = crate::ai_service::llm::slot_snapshot(&state.chat.llm)
    .await
    .ok_or("未配置可用模型")?;
let mut stream = llm.complete_stream(&messages).await?;   // ChunkStream

// 数据目录（Android 上自动指向应用私有目录）
let dir = crate::api::game_data_dir();                    // game_data/
```

### 前端（Vue 3）

```
src/components/views/world_map/
├── WorldMapHome.vue      总入口（对应 index.html）
├── WorldMapDrill.vue     点击下钻（对应 world_svg.html）
├── DistrictLive.vue      AI 绘制过程可视化（对应 district_live.html）
├── DistrictViz.vue       可视化实验室：图层/2D3D/图表（对应 viz_demo.html）
├── DistrictView.vue      小区地图（对应 district.html）
├── BlocksView.vue        远处区块（对应 blocks.html）
├── MapLibrary.vue        地图库（对应 maplib.html）
└── PhoneOverlay.vue      悬浮手机（对应 phone_demo.html）
```

**技术要点**：
- **样式隔离**：原型页把 SVG 内联时用 `#stage` 前缀作用域化 CSS；Vue 里改用
  `<iframe srcdoc>` 或 `<div v-html>` + `<style scoped>`，或直接把 SVG 拆成组件
- **图层开关**：SVG 元素已带 `layer-*` / `wm-bld` `data-type` 类名，Vue 里用计算属性
  切换 `display` 即可，无需重渲染
- **流式绘制**：Tauri 命令无法直接 SSE，改用 **Tauri event**（`app.emit("world_map://chunk", …)`）
  或 `Channel<T>`（Tauri 2 的流式 API）——推荐 `Channel`，类型安全
- **i18n**：所有界面文案走 `$t()`，中文放 `locales/zh-CN`

### 数据目录

```
<data_dir>/game_data/
├── world_map/
│   ├── geo/            geojson 缓存（{adcode}.json）
│   ├── maplib/         地图库索引 index.json + 布局 layouts/*.json
│   ├── osm/            Overpass 缓存（按 200m 网格）
│   └── events.json     世界事件（供主动系统读取）
└── schedules.json      已有（LingChat 日程，地图读它）
```

---

## 四、接口映射（HTTP → Tauri command）

| 原 HTTP 接口 | 新 Tauri 命令 | 说明 |
|---|---|---|
| `GET /api/geo_svg` | `world_map_geo_svg` | 行政区划级 SVG |
| `GET /api/district_svg` | `world_map_district_svg` | 小区 SVG（含 `mode`/`charts`/`layers`） |
| `GET /api/district_stream`（SSE） | `world_map_district_stream` + **Channel** | 流式绘制 |
| `GET /api/district_stats` | `world_map_district_stats` | 统计指标 |
| `GET /api/blocks` | `world_map_blocks` ✅ 已做 | 主块+远处块 |
| `GET /api/district`（PNG） | — | **废弃**，SVG 全面替代 |
| `GET /api/map` / `bigmap`（PNG） | — | **废弃** |
| `GET /api/maplib/*` | `world_map_maplib_*` | 地图库增删查 |
| `GET /api/facilities*` | `world_map_facilities*` | 设施 |
| `GET /api/transport_plan` | `world_map_transport_plan` | 路线规划 |
| `GET /api/schedule` | `world_map_schedule` | 日程→地图位置 |
| `GET /api/time` / `weather` | `world_map_time` / `weather` | 时间天气（可复用 world_time.js/weather.js 走前端） |
| `GET /api/location` | `world_map_location` | 设备定位（Android 权限由 Tauri 管） |
| `GET /api/osm_*` | `world_map_osm_*` | Overpass |

---

## 五、分阶段计划

### 阶段 1 · 核心渲染链（P0）
**目标：LingChat 里能看到「AI 一栋栋画小区」和「全国点击下钻」**

1. `details.rs` — 街道细节（纯几何，无依赖，最好移植）→ 带单元测试
2. `sketch.rs` — 本地草图（确定性随机，用 `rand` + `md5`）
3. `render.rs` 扩展 — 细节绘制 / z1z2z3 分层 / `layer-*` 类名 / 2D-3D / 数据卡片
4. `render_geo.rs` — 行政区划 SVG（含抽稀：像素级 0.6px 容差）
5. `stream.rs` — 流式生成：`LlmClient::complete_stream` + 增量 JSON 解析 + `Channel` 推送
6. `DistrictLive.vue` / `WorldMapDrill.vue` — 两个页面接上
7. 命令注册 + CI 通过

**验收**：LingChat 里点「世界 → 生成小区」能看到建筑一栋栋长出来；点全国地图能下钻到区县。

### 阶段 2 · 玩法层（P1）
1. `stats.rs` + 数据卡片/面板
2. `maplib.rs` — 地图库（索引 + 布局缓存 + LRU 容量控制，索引丢失可用 scan 自愈）
3. `facilities.rs` / `transport.rs` — 设施与交通（T2 系列能力）
4. `schedule.rs` — 日程 → 角色在地图上的位置
5. `DistrictViz.vue` / `MapLibrary.vue` / `BlocksView.vue`

### 阶段 3 · 打磨（P2）
1. `osm.rs` — Overpass 真实地物（注意：生成路径只读缓存，抓取走显式命令）
2. 性能：全国级 SVG 690KB 偏大 → 按缩放级别分级抽稀 / 异步补细
3. 移动端适配：触摸缩放、小屏布局
4. 与 LingChat 深度联动：世界事件写入 `events.json` 供主动系统读取

### 阶段 4 · 发布
1. Web 版：`pnpm build` + CI（`world-map-check.yml`）
2. Android APK：`build-android.yml`（手动触发，需要签名 secrets）
3. 从 `feat/world-map` 合并回 `main`

---

## 六、风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| **移植工作量大**（6800 行 Python → Rust） | 周期长、易中途卡住 | 分阶段，每阶段可独立交付；P0 之外不阻塞发布 |
| **Rust 编译慢**（Tauri 全量 40 分钟起） | 迭代慢 | 一律交给 CI；本地只跑 `cargo check`（有增量缓存时几分钟） |
| **国区网络**：`github.com` git 通道不通 | 无法 push | 已用 Git Data API 脚本 `push_via_api.py` 绕过 |
| **本机无 Android SDK/NDK** | 打不了 APK | 走 GitHub Actions（仓库已有 `build-android.yml`，历史构建成功） |
| **中文字体**（PIL 渲染中文变方块） | 原型踩过的坑 | SVG 方案天然规避：文字交给 WebView 渲染 |
| **流式实现差异** | 原型用 curl 子进程（Termux Python SSL 有 bug），Rust 侧不同 | Rust 侧直接用 `LlmClient::complete_stream`，不碰 Python SSL |
| **并发写索引丢数据** | 地图库索引条目变少 | Rust 侧用 `Mutex`/`RwLock` 保护"读-改-写"整个事务；索引可 scan 自愈 |
| **AI 输出不稳定** | 生成失败 | 保留兜底：流式失败交付部分结果 + 整体解析重试 + 草图先顶上 |
| **Tauri 2 Channel 类型** | 流式推送写法不熟 | 先写最小验证（一个命令推 10 条消息），跑通再改正式 |

---

## 七、验收标准

**阶段 1 完成时**：
- [ ] `cargo test --lib world_map` 全绿（含新增的 details/sketch/render 测试）
- [ ] CI `world-map-check.yml` 双 job 通过
- [ ] LingChat 里「世界」入口 → 能看到全国地图，点击下钻到区县
- [ ] 能触发小区生成，建筑一栋栋出现（流式）
- [ ] 图层开关 / 2D-3D / 数据卡片在 Vue 里可用

**最终交付**：
- [ ] Web 版构建通过，页面与原型视觉一致
- [ ] APK 产出且能安装运行
- [ ] 原型侧的 15 个页面能力全部在 LingChat 内可用
- [ ] Python 侧车可完全停用（不再需要 8788/8790）

---

## 八、立即可以开工的第一步

`details.rs` 是最佳起点：**纯几何、零依赖、原型有 273 行可逐函数对照**，
而且它决定了地图"像不像真实街道"。移植顺序建议：

```
details.rs  →  sketch.rs  →  render.rs 扩展  →  stream.rs  →  Vue 页面
   (几何)       (布局)         (渲染)          (生成)        (交互)
```

---

> 本文档随实现进展更新；全部文档索引见 `docs/world-map/README.md`。
