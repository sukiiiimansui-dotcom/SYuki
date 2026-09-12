# 14 · LingChat 架构地图（给后续开发用的施工图）

> 目的：把 LingChat 现有架构摸清到「能照着插东西」的粒度。
> 纪律：本文档只描述**已有事实**，每条结论都带 `路径:行号`；拿不准的一律写「**未确认**」，不猜。
> 最后更新：见 git 提交记录。

---

## 0. 怎么读这份文档

### 0.1 行号基准（**极其重要，别混**）

| 标记 | 仓库 | 说明 |
|---|---|---|
| **无前缀** | `~/lingchat-official/` | 「官方线」。当前 checkout = 分支 `feat/world-map-official` @ `615e64c0` = 官方 0.5.1 基线（`348ef00c`）+ 地图前端接线 + 环境适配剔除 |
| **【我】** | `~/lingchat-main/` | 我们的仓库，分支 `feat/world-map`，版本号 0.1.3；地图后端 `src-tauri/src/world_map/` 在这里 |

两个目录**不是两个 clone**：`~/lingchat-official/.git` 是一行文本
`gitdir: /data/data/com.termux/files/home/lingchat-main/.git/worktrees/lingchat-official`
→ 它是 `lingchat-main` 仓库的一个 **git worktree**，共享对象库。
（依据：`git worktree list` 列出的 4 个 worktree；`~/lingchat-official/.git` 文件内容）

### 0.2 三个必须先知道的前提（会推翻旧假设）

1. **两条线是不同代际，不能互相套行号。**
   官方 = `0.5.1`（`src-tauri/Cargo.toml:3`、`package.json:4`、`src-tauri/tauri.conf.json:4`）；
   我们 = `0.1.3`（【我】`src-tauri/Cargo.toml:3`、`package.json:4`）。
   官方有 `src/cast/`（投屏，9 条命令，`src-tauri/src/cast/mod.rs:1-21`），**【我】完全没有这个目录**；
   官方有 Live2D（`src/components/game/standard/Live2DRolePresentation.vue`），【我】没有；
   官方 `src-tauri/src/lib.rs` 926 行，【我】805 行。
   → **给官方线提 PR 时，我们 fork 的行号一律作废，必须重新定位。**

2. **「官方 0.5.1」这个身份只是 commit message 说的，未独立验证。**
   `348ef00c` 的提交信息是「照搬官方 LingChat 0.5.1 源码作为官方线基线」；
   本机 `github.com` git 通道不通（只有 `api.github.com`），无法逐字节比对上游。
   【我】`_upstream_base.txt:1` 记录的真实上游基准是 `ef8f7914`（≠0.5.1）。→ **未确认**。

3. **世界地图在【我】里是「旁路模块」，不是插件。**
   全后端 `grep -i "world_map" src-tauri/src/ai_service/` **零命中**；
   `InnerAppState` 没有地图字段；地图没加任何 Tauri 事件、没加 capability 条目、
   没进 `tauri.conf.json` 的 `resources`。**它目前是一个「挂得上、拆得下、默认不打扰」的独立块。**
   这也是第 4 节所有插入点的立论基础。

4. **官方线只有「地图前端」，没有「地图后端」。**
   `~/lingchat-official/src-tauri/src/` 里 `grep -rn world_map` **命中 0**；
   官方线的 `src/api/services/worldMap.ts:10` 是 `USE_RUST = false`、`:16` 默认指向 `http://127.0.0.1:8791`（HTTP 侧车），
   `world_map_*` 命令名只是**预留字符串**。
   → **官方线现在跑得起「地图页面」，但数据来自外部 HTTP 服务；把 Rust 地图后端接过去是独立的一件事。**

---

# 一、前端（Vue3 + Vite）

## 1.1 `src/` 目录职责

| 目录 | 职责 | 代表位置 |
|---|---|---|
| `src/main.ts` | 应用入口：`createApp` → `app.use(i18n)`(`:39`) → 挂载 | `src/main.ts:9`(i18n import)、`:39` |
| `src/App.vue` | **所有窗口（main / log / cast / pet / settings）的根组件**；全局弹窗与全局叠加层挂在这里 | `src/App.vue:1-18` |
| `src/api/` | 前端数据层。`services/` = 一域一文件的 Tauri 命令封装；`types/` = 类型；`websocket/` = 旧 WS 遗留（`handlers/script-handler.ts:134`） | `src/api/services/character.ts:167`、`src/api/tauri-events.ts:58` |
| `src/assets/` | 图片与全局样式（`images/alona.png` 主菜单立绘、`styles/base.css`） | `src/components/views/MainMenu.vue:35` |
| `src/components/` | 见 §1.2 分层表 | — |
| `src/composables/` | 组合式函数（无 UI 的逻辑单元）。`ui/useTypeWriter.ts` 打字机 | `src/composables/useAsrInput.ts:593` |
| `src/config/` | 前端静态配置 | — |
| `src/constants/` | 常量表 | — |
| `src/controllers/` | **名字骗人：不是控制器，是静态配置常量表**（无 class、无状态机、无事件订阅）。`controllers/emotion/config.ts:16` `EMOTION_CONFIG_EMO`（情绪→文件名）、`:39` `EMOTION_CONFIG`（情绪→动画/气泡/音效）；`controllers/core/config.ts:5` `API_CONFIG`、`:15` `APP_CONFIG`。消费者只有 `GameRoleAvatar.vue:39` 与 `Live2DStage.vue:16` | `src/controllers/emotion/config.ts:16,39` |
| `src/core/events/` | **前端事件总线**：`EventQueue` 类（`event-queue.ts:8`）+ 模块级单例 `eventQueue`（`:185`）；`EventProcessorManager` 单分派（`event-processor.ts:16`，找第一个 `canHandle` 的 processor）；`index.ts:7-28` 用 `import.meta.glob("./processors/*.ts",{eager:true})` **自动注册 19 个处理器**（dialogue / narration / player / free-dialogue / music / sound / ambient / background / background-effect / present-pic / chapter-change / script-choice / script-end / modify-character / input / thinking / error / status-reset）；在 `src/main.ts:27` 调用。**这就是我们加地图事件时要挂的地方** | `src/core/events/index.ts:7-28` |
| `src/core/events/dialogue-merge.ts` | 跨组件共享的打字/音频瞬时状态（被 `event-queue.ts:41-44`、`GameDialog.vue:733-734`、`MainChat.vue:169,203-215` 读写） | 同上 |
| `src/data/` | 内置静态数据 | — |
| `src/locales/` | i18n，见 §1.8 | `src/locales/index.ts:1-143` |
| `src/router/` | 路由表，见 §1.3 | `src/router/index.ts:21-72` |
| `src/stores/` | Pinia，见 §1.4 | `src/stores/modules/` |
| `src/types/` | 全局 TS 类型 | — |
| `src/utils/` | 工具函数。`utils/tts/`、`utils/typewriter/` 是子系统级工具 | `src/utils/platform.ts`（`isAndroid()`，`MainChat.vue:28-30` 用） |

## 1.2 组件分层与 z-index（**决定我们往哪里插 UI**）

### 1.2.1 `src/components/` 一级目录

| 目录 | 职责 |
|---|---|
| `base/` | 原子组件，如 `Button`（`MainChat.vue:18-26` 的导航按钮） |
| `effects/` | 全屏特效，`CursorEffects.vue` |
| `game/standard/` | **游戏主界面那一套**，见 §1.6 |
| `game/live2d/` | Live2D 舞台（官方线独有） |
| `pet/`、`pomodoro/`、`schedule/`、`script-editor/` | 各功能域组件 |
| `settings/`、`settings_pet/` | 设置面板与桌宠设置 |
| `tools/` | 工具调用相关 UI（`FreeModeTools.vue`、`ToolActivityStatus.vue`，`MainChat.vue:4/17`） |
| `ui/` | 通用 UI 件（`Notification`、`AppDialog`、`ImageAcrossFade`…） |
| `views/` | **页面级组件**（路由目标），见 §1.3 |

### 1.2.2 App.vue 全局挂载清单与层叠层级

`src/App.vue` 的 template 只有 17 行，是**所有窗口共用的根**：

| 顺序 | 组件 | 挂载行 | 挂载条件 | 根元素定位 / z-index |
|---|---|---|---|---|
| 1 | `<router-view />` | `src/App.vue:2` | 总是 | 页面自身决定 |
| 2 | `<Teleport to="body"><CursorEffects/></Teleport>` | `src/App.vue:4-6` | 总是（**teleport 到 body**，注释在 `:3`：避开 `#app` 的 `transform: scale` 坐标偏移） | `position: fixed; z-index: 9999`（`src/components/effects/CursorEffects.vue:768,773`） |
| 3 | `<Notification>` | `src/App.vue:11` | `isMainWindow && route.path !== '/pet'` | `@apply fixed left-0 z-[10000]`（`src/components/ui/Notification.vue:43`） |
| 4 | `<AchievementToast>` | `src/App.vue:12` | `isMainWindow` | `@apply fixed right-8 z-[9999]`（`src/components/ui/AchievementToast.vue:107`） |
| 5 | `<AdventureUnlockNotify>` | `src/App.vue:13` | `isMainWindow` | `fixed right-8 bottom-[...] z-9999`（`src/components/ui/AdventureUnlockNotify.vue:5`） |
| 6 | `<AppDialog>` | `src/App.vue:14` | `isMainWindow` | `position: fixed; z-index: 10000`（`src/components/ui/AppDialog.vue:145,150`） |
| 7 | `<WorldMapLayer>` | `src/App.vue:17` | `isMainWindow` | overlay 模式 `position: fixed; inset:0; z-index: 1; pointer-events:none`（`src/components/views/WorldMapLayer.vue:182-185`）；corner 小窗 `position: fixed; right:16px; bottom:84px; width:232px; z-index: 60`（`:218-222`）。组件根是 `<template v-if="mode !== 'off'">`（`:3`） |

**补充：主窗口内各层的完整层叠表**（从下到上）：

| 层 | z-index / position | 位置 |
|---|---|---|
| 背景图 `.game-background` | `position:absolute; z-index:-2` | `src/components/game/standard/GameBackground.vue:328` |
| 粒子特效 | 容器 `style="isolation: isolate"`（`:17`）自建层叠上下文，内部 inline `z-index:114514`（常量 `BACKGROUND_ZINDEX` `:174`，注入 `:25,32,38,44,50`）→ **被 isolate 关在容器内，不外泄** | `GameBackground.vue:17,25-50,174` |
| 角色立绘层 | `zIndex:"1"`（inline）；气泡/特效层 `zIndex:"2"` | `GameRoleAvatar.vue:152-153` |
| **地图 overlay 层** | `z-index:1`（**与立绘同值**） | `WorldMapLayer.vue:184` |
| Live2D 舞台 | `class="z-2"` | `GameRolesStage.vue:5` |
| 场景光照叠加 | `absolute inset-0 z-10` | `GameRolesStage.vue:27` |
| 对话框 | 根 `class="game-dialog relative z-2 …"`；隐藏态追加 `z-[-1]!` | `GameDialog.vue:3,9-13` |
| 额外 UI 层 | `fixed top-0 left-0 z-999` | `GameExtraUI.vue:3` |
| 右上菜单 `#menu-panel` | `position:fixed; z-index:1000` | `MainChat.vue:310-316` |
| 设置面板 | `z-index:999` / `1000` | `src/components/settings/SettingsPanel.vue:212,223` |
| 日程面板 | `fixed inset-0 z-[1100]` | `src/components/schedule/SchedulePanel.vue:26` |
| 开屏过渡 | `z-index:100` 与 `z-index:9999` | `src/components/views/LoadingTransition.vue:748,809` |
| 通知/成就/冒险 | `9999~10000` | 见上表 |
| 光标特效 | `9999` | `CursorEffects.vue:773` |
| 对话框确认 | `10000` / `10001` | `AppDialog.vue:145-150` / `AppDialog.vue:14` |

**结论（插 UI 的层位选择，已按上表修正）**：

- ⚠️ **`WorldMapLayer` 的 overlay 模式 `z-index:1` 与立绘层 `z-index:1` 同值，而它在 DOM 里更靠后 → 它画在立绘之上**（不是"在页面内容之下"）。
  这是「半透明地图盖在聊天画面上」的刻意设计（`App.vue:16` 注释「半透明背景层 / 聊天时可缩成角落小窗」），
  但它**会同时盖住立绘和背景**，只被对话框（`z-2`）和菜单（`1000`）压住。
  → **我们若要做"真正的背景层"（在地图之上还能看到角色），z-index 必须小于 1（例如 `-1`）或改用 `z-index:0` 并放在 `router-view` 之前。**
- 想浮在**聊天界面之上但不挡弹窗**：`z-index` 落到 `60`（WorldMapLayer corner 那一档）或 `999~1100`（额外 UI / 菜单 / 日程那一档）。
- 弹窗/通知一档是 `9999~10001`，**永远别超过它**。
- `AppDialog`（`z-index:10000` 遮罩 + `10001` 卡片，且自身 `<Teleport to="body">`，`AppDialog.vue:2,14,145-150`）
  是确认框的统一出口（`useDialogStore`，`src/stores/modules/ui/dialog.ts:13-19`），`MainMenuOptions.vue:45-51` 的退出确认走它。

### 1.2.3 聊天页自己的层叠（`MainChat.vue`）

`src/components/views/MainChat.vue:2-48` 的顺序即叠放顺序：

`FreeModeTools`(:4) → `FullAccessWarning`(:5) → `GameBackground`(:6) → `GameRolesStage`(:8-12) → `GameDialog`(:13) → `#menu-panel`(:16-40，右上导航按钮) → `GameExtraUI`(:41) → `ImageSourcePicker`(:44) → `LoadingTransition`(:47，加载动画盖在最上)

游戏内部层级（`GameRolesStage.vue`）：
`Live2DStage` 是 `class="z-2"`（`src/components/game/standard/GameRolesStage.vue:5`）→ 立绘 `zIndex: "1"`、特效层 `zIndex: "2"`（`GameRoleAvatar.vue:152-153`）→ 场景光照叠加 `class="... z-10"`（`GameRolesStage.vue:27`）。

> ⚠️ Tailwind v4 的 `z-2` / `z-3` 是任意值语法，**别当成 `z-index:2` 之外的语义**；`MainMenu.vue:31` 也有 `z-3`。

## 1.3 路由表全貌（`src/router/index.ts`，83 行）

懒加载声明：`Credits`:6、`ComapionMode`:7、`MainMenu`:8、`PetMode`:9、`Second`:10、`LogWindow`:11、`CastWindow`:12、`ScriptEditor`:15、`WorkshopPage`:17、`WorldMap`:18
（注释 `:13-14` 说明：项目**没配 `manualChunks`**，非懒加载的 view 会整块进主 chunk）

| path | name | component | 职责 |
|---|---|---|---|
| `/` | `MainMenu` | `MainMenu.vue` | 主菜单（`index.ts:22-26`） |
| `/world` | `WorldMap` | `WorldMap.vue` | **世界地图页（我们加的）**（`index.ts:27-31`） |
| `/chat` | `LingChat` | `CompanionMode.vue` | 主聊天页（`index.ts:32-36`） |
| `/credit` | `Credits` | `Credits.vue` | 致谢（`:37-41`） |
| `/pet` | `PetMode` | `PetMode.vue` | 桌宠窗口（`:42-46`） |
| `/second` | `Second` | `Second.vue` | 第二窗口（`:47-51`） |
| `/log-window` | `LogWindow` | `LogWindow.vue` | 独立日志窗口（`:52-56`） |
| `/cast` | `CastWindow` | `CastWindow.vue` | 投屏窗口（`:57-61`） |
| `/script-editor` | `ScriptEditor` | `ScriptEditor.vue` | 剧本编辑器（`:62-66`） |
| `/workshop` | `WorkshopPage` | `WorkshopPage.vue` | 云端创意工坊（`:67-71`） |

路由实例：`createWebHistory()`（HTML5 模式，非 hash），`src/router/index.ts:75-80`。
**路由表里没有任何 `meta`**（没有 `windowLabel`、`transparent` 之类）—— 窗口差异**全部靠 `getCurrentWindow().label` 运行时判断**（见 §1.5）。

**10 条路由 → 窗口的映射**（依据 `src/main.ts:31-50` 的 `?window=` 改写 + §1.5 的 label 判定）：

| 跑在哪个窗口 | 路由 |
|---|---|
| `main`（主窗口） | `/`、`/world`、`/chat`、`/credit`、`/pet`、`/script-editor`、`/workshop` |
| `log` | `/log-window`（`main.ts:43-45` 经 `?window=log` 改写） |
| `cast` | `/cast`（`main.ts:48-50` 经 `?window=cast` 改写） |
| `settings` | `/second`（由 `PetMode.vue:273` 的 `url:"/second"` 指定，**不是** `?window=` 机制） |

`/chat` 页面极小：`src/components/views/CompanionMode.vue:1-4` 只有 `<MainChat />` + `<Settings />`。

## 1.4 状态管理（Pinia）

`src/stores/` 结构：

```
src/stores/
├── adventure.ts
├── llm-providers.ts
├── index.ts                     ← createPinia()
└── modules/
    ├── agent/       { actions, getters, index, state }.ts
    ├── config/      config.ts
    ├── game/        { actions, getters, index, state }.ts
    ├── script-editor/ { actions, getters, index, state }.ts
    ├── settings/    index.ts(422 行) + asr.ts(91 行)
    ├── ui/          ui.ts(657 行) + achievement.ts(137) + dialog.ts(68)
    │                + archive-import.ts(35) + plugin-archive.ts(20) + role-archive.ts(71)
    └── user/
```

### 1.4.1 `ui` store（`src/stores/modules/ui/ui.ts`）—— **UI 总状态**

`UIState` 接口定义在 `:25-90`，store 定义在 `:97`。关键字段（都在 `:98-160` 的 `state: (): UIState => ({...})` 里）：

| 字段 | 类型 | 默认 | 用途 |
|---|---|---|---|
| `showCharacterTitle` / `showCharacterSubtitle` | string | `"Lovely You"` / `"Bilibili"` | 标题栏（`App.vue:122-123` 投屏镜像用） |
| `showCharacterLine` | string | `""` | **当前台词**（`App.vue:121` 监听它触发投屏镜像） |
| `showCharacterEmotion` | string | `""` | 对话框情绪标签 |
| `showCharacterMotionText` | string | `""` | 动作文本 |
| `showSettings` / `currentSettingsTab` / `advanceTab` | bool/string | `false`/`"text"`/`"menu"` | 设置面板开关 |
| `currentBackground` 相关 | — | — | **getter（转发 settings）**，见下方说明 |
| `currentBackgroundEffect` | — | — | **getter（转发 settings）** |
| `currentBackgroundMusic` / `bgMusicMode` / `bgMusicPaused` / `bgMusicStoped` / `bgMusicPlaybackRate` | — | `"None"`/`"loop-single"`/…/`1` | BGM |
| `ambientTracks` | `Array<{id,src,name?,volume,loop,paused?,fade?}>`（最多 8 轨） | `[]` | 环境音 |
| `autoMode` | bool | `false` | 自动播放模式 |
| `viewportWidth` / `viewportHeight` | number | `window.innerWidth/innerHeight` | **全局唯一 resize 监听**，组件直接读（`GameRoleAvatar.vue:72-95` 靠它做窄屏适配） |
| `safeAreaInsetTop/Bottom/Left/Right` | number | 0 | 刘海屏安全区 |
| `scheduleView` | string | `"schedule_groups"` | 日程视图 |
| `notification` | `NotificationState{isVisible,type,title,message,avatarUrl,duration}` | — | 通知（`Notification.vue` 直读） |
| `tipsMap` / `tipsAvailable` | — | — | 角色 tips |

初始化：`initUIStore()`（`App.vue:308` 调用，注释说「加载角色 tips」；定义在 `ui.ts:603-657`，含 `initialized` 单次守卫、安全区同步、**全局唯一 resize 监听** `:621-625`）。

> ⚠️ **`ui` store 的 getters 全是 `settings` store 的转发层**（`ui.ts:155-201`）：
> `currentBackground`(`:156`)、`typeWriterSpeed`(`:160`)、`enableChatEffectSound`(`:163`)、`currentBackgroundEffect`(`:166`)、
> `characterVolume`(`:169`)、`backgroundVolume`(`:172`)、`bubbleVolume`(`:175`)、`achievementVolume`(`:178`)、
> `ambientVolume`(`:182`)、`currentCharacterFolder`(`:186`)；
> 本地计算的只有 `aspectRatio`(`:190`)、`isNarrowScreen`(`:194`)、`isSmallScreen`(`:198`)。
> **改背景/音量要动 `settings` store，不是 `ui` store。** `App.vue:127-128` 监听的两个字段实际上是 getter。
> `ui` store **没有 `persist`**（不写 localStorage）。

### 1.4.2 `game` store（`src/stores/modules/game/`）

四文件拆分：`state.ts`(112 行) / `getters.ts`(33 行) / `actions.ts`(288 行) / `index.ts`(10 行)。
关键字段（**定义行号已核实**）：

| 字段 | 类型 | state 定义 | 初始值 | 用途 / 使用点 |
|---|---|---|---|---|
| `presentRoleIds` | `number[]` | `state.ts:67` | `[]`（`:94`） | **在场角色 id 列表**（决定立绘位置分配）——`GameRoleAvatar.vue:99`、`App.vue:130` |
| `presentRolesList` | getter → `GameRole[]` | `getters.ts:15-17` | — | 在场角色对象列表（`presentRoleIds.map(id => gameRoles[id]).filter(!!)`）——`GameRolesStage.vue:6,16` |
| `currentInteractRoleId` | `number \| null` | `state.ts:69` | `-1`（`:96`） | 当前交互角色 —— `GameRolesStage.vue:8`、`App.vue:131` |
| `currentInteractRole` | getter → `GameRole \| undefined` | `getters.ts:19-22` | — | 当前交互角色对象（`.emotion` / `.originalEmotion`）——`App.vue:136-137` |
| `currentStatus` | `"input" \| "thinking" \| "responding" \| "presenting"` | `state.ts:75` | `"input"`（`:102`） | 状态 —— `App.vue:126` |
| `thinkingLength` | `number` | `state.ts:77` | `0`（`:103`） | 思考链长度（流式）——`tauri-events.ts:75` |
| `currentScene` | `SceneInfo \| null` | `state.ts:79` | `null`（`:105`） | 当前场景（`.id` / `.lighting`）——`App.vue:129`、`GameRolesStage.vue:65`、`GameRoleAvatar.vue:107` |
| `command` | `string \| null` | `state.ts:80` | `null`（`:106`） | 交互指令（`'touch'` 时挂触摸层）——`GameRoleAvatar.vue:23` |
| `runningScript` | `ScriptInfo \| null` | `state.ts:64` | `null`（`:91`） | 运行中的剧本 —— `MainChat.vue:17` |
| `exitStoryMode()` | action | — | — | 退出剧情模式 —— `GameModeOptions.vue:37` |

### 1.4.3 其他 store

| store | 文件 | 备注 |
|---|---|---|
| `settings` | `src/stores/modules/settings/index.ts`（422 行） | `useSettingsStore().text.fontFamily`（`App.vue:65`）、`.text.vueDevToolsEnabled`（`App.vue:90`）、`setUiLocale()`（`locales/index.ts:126`）。**用 persist 插件写 localStorage 键 `lingchat-settings`**（`src/locales/index.ts:41`） |
| `llm-providers` | `src/stores/modules/llm-providers.ts` | `load()`（`App.vue:320`），避免主界面误判未选模型 |
| `ui/achievement` | `src/stores/modules/ui/achievement.ts` | `notifyBackendUnlock` / `addAchievement` / `listenForUnlocks()`（`App.vue:324-328`） |
| `ui/dialog` | `src/stores/modules/ui/dialog.ts` | `confirm(msg,title)`（`MainMenuOptions.vue:47`、`App.vue:390`） |
| `ui/asr` | `stores/modules/settings/asr.ts` | `setVadLoaded()`（`tauri-events.ts:291`） |

## 1.5 页面与「窗口」的关系

**没有路由级的窗口声明**，窗口身份靠两套机制决定：

**机制一：启动时按 `?window=` 查询参数改写初始路由**（`src/main.ts:31-50`）——
`?window=cast` → `router.replace("/cast")`（`:48-50`）；`?window=log` → `router.replace("/log-window")`（`:43-45`）。
`main.ts:21-23` 还只在主窗口清 `lingchat_loading_shown` 标记。
`main.ts:29-36` 按窗口类型分别调用 `initializeTauriEventListeners()`（主窗口）/ `initializeCastWindowListeners()`（投屏窗口）。

**机制二：`getCurrentWindow().label` 运行时判断**：

| 位置 | 代码 | 说明 |
|---|---|---|
| `src/App.vue:263` | `const isMainWindow = getCurrentWindow().label === "main"` | **全应用最重要的窗口判定**；决定 Notification / AchievementToast / AdventureUnlockNotify / AppDialog / WorldMapLayer 是否挂载 |
| `src/api/tauri-events.ts:59-60` | `const mainWindow = currentWindow.label === "main" ? currentWindow : null;` | 后续用 `mainWindow?.listen(...)` —— **子窗口不注册驱动型监听**（注释 `:446-452` 解释为何投屏不能自己消费 `ai:reply`） |
| `src/App.vue:268-270` | `if (isMainWindow) useAsrInput()` | ASR 全局初始化只在主窗口 |
| `src/App.vue:311-316` | `getCurrentWindow().label === "main" && localStorage... === "1"` → `invoke("open_log_window")` | 自动开日志窗口 |
| `src/App.vue:379-381` | `onCloseRequested(...)` 内 `if (label !== "main") return;` | 只有主窗口拦截关闭做确认 |
| `src/App.vue:282-284` | `getCurrentWindow().setFullscreen()` | F11 全屏（`App.vue:272-289`，`/pet` 下直接 return） |
| `src/components/pet/GameRoleAvatar.vue:111,160` | `getCurrentWindow().startDragging()` | 桌宠拖拽 |
| `src/settings_pet/pages/SettingsPage.vue:155-216` | `isMaximized / minimize / toggleMaximize / close` + `emit(PET_*_EVENT)` + `listen("dialog-history-changed")` | 桌宠设置窗的窗口控制 |

> `getAllWindows` **全仓零使用**。

**各窗口 label 与创建方**：

| label | 谁创建 | 加载的路由 | 依据 |
|---|---|---|---|
| `main` | `tauri.conf.json` 静态声明（唯一一个；未写 label → Tauri 默认 `main`） | `/`（默认） | `src-tauri/tauri.conf.json:14-22` |
| `log` | Rust `utils/log_bridge.rs:49-53`（960×640，title「日志」，`index.html?window=log`） | `/log-window` | 命令 `open_log_window`（`App.vue:315` 调用） |
| `cast` | Rust `cast/mod.rs:31`（常量）+ `:133-146`（800×450、`decorations(false)`、`shadow(false)`） | `/cast` | 打开入口 `cast/mod.rs:121`，调用点 `:183`、`:312` |
| `screenshot-overlay` | Rust `api/screenshot.rs:45,49-62`（`screenshot-overlay.html`，独立静态页，**不含 Vue**） | — | — |
| `settings` | **前端** `new WebviewWindow("settings", {url:"/second", …})`，`src/components/views/PetMode.vue:271-280` | `/second` → `settings_pet/pages/SettingsPage.vue` | 这就是 `capabilities` 里要 `core:webview:allow-create-webview-window` 的原因（`src-tauri/capabilities/default.json:8`） |
| （无独立 label）`/pet` | **不是新窗口**：`MainChat.vue:109` `router.push("/pet")`，由 Rust 重配主窗口（`api/pet.rs:107-160`：`set_skip_taskbar(true)` `:133`、`set_always_on_top(true)` `:134`、尺寸 `240*scale × 485*scale` `:127-133`、`get_webview_window("main")` `:118`） | `/pet` | Android 不可用（`MainChat.vue:27-29`） |

> ⚠️ `capabilities/default.json:5` 的 `windows` 白名单是 `["main","settings","log","screenshot-overlay","cast"]` —— **窗口 label 变了要同步改这里**。

**哪些组件只在主窗口挂载**：`Notification`、`AchievementToast`、`AdventureUnlockNotify`、`AppDialog`、`WorldMapLayer`（全部 `v-if="isMainWindow"`，`src/App.vue:11-17`）。

## 1.6 游戏模式那一套（`src/components/game/standard/`）

### 1.6.1 文件清单与职责

| 文件 | 行数 | 职责 |
|---|---|---|
| `GameBackground.vue` | 330 | 背景图层：静态图 / 视频 / 特效（`convertFileSrc` 在 `:119`、`:258`） |
| `GameRolesStage.vue` | 139 | **角色舞台**：遍历 `presentRolesList` 渲染每个 `RoleAvatar`；统一主语音播放器；场景光照叠加层 |
| `GameRoleAvatar.vue` | 324 | **单个角色**：选 Live2D 还是静态立绘、算布局位置、切情绪、气泡、触摸层 |
| `GameDialog.vue` | 1164 | 对话框（打字机、输入、历史、ASR 麦克风、选择支） |
| `GameExtraUI.vue` | 31 | 额外 UI 插槽容器 |
| `StaticRolePresentation.vue` | 65 | **静态立绘的实际渲染**（`ImageAcrossFade` 交叉淡入） |
| `Live2DRolePresentation.vue` | 48 | Live2D 渲染（官方线独有） |
| `TouchAreas.vue` | 374 | 身体部位触摸区 |
| `avatar-animation.css` | 219 | 立绘切换动画（keyframes） |
| `index.ts` | 4 | 只导出 4 个：`GameBackground` / `GameDialog` / `GameRoleAvatar` / `GameRolesStage`（`index.ts:1-4`） |
| `animations/`、`particles/`、`extra/` | — | 背景动画、粒子、剧本额外 UI（章节名/选项/音乐播放器/立绘图…） |

### 1.6.2 props / emits 一览（复用时的接口契约）

| 组件 | props | emits | 谁用它 |
|---|---|---|---|
| `GameBackground` | 无 | 无 | `MainChat.vue:6`、`CastWindow.vue:11` |
| `GameRolesStage` | `castScale?: number`（`:53`，默认 1 `:58`）、`castOffsetY?: number`（`:55`，默认 0） | `audio-ended`、`audio-started`（`:47`） | `MainChat.vue:8-12`、`CastWindow.vue:20` |
| `GameRoleAvatar` | `role: GameRole`（`:47`）、`castScale?`（`:49`）、`castOffsetY?`（`:52`） | 无 | 仅 `GameRolesStage.vue:15-21`（v-for） |
| **`StaticRolePresentation`** | **`src`、`visible?`、`layerStyle`、`animationClasses`、`objectFit`（`:29-38`）——零 store 依赖** | `animation-end`（`:40-42`）；`defineExpose({waitForLoad})`（`:50`） | `GameRoleAvatar.vue:12-20`；`Live2DRolePresentation.vue:2-17`（Live2D 未就绪时的兜底） |
| `Live2DRolePresentation` | `roleId`、`src`、`layerStyle`、`animationClasses`、`objectFit`（`:26-32`） | `animation-end`（`:34-36`） | `GameRoleAvatar.vue:2-11`（`v-if="role.live2d"`） |
| `TouchAreas` | `bodyParts?`（`:55-61`） | `player-continued`、`dialog-proceed`（`:64`） | `GameRoleAvatar.vue:23`（**只传 body-parts，两个 emit 没接** —— 疑似遗留） |
| **`GameDialog`** | **无 props**（全读 store） | `player-continued`、`dialog-proceed`（`:568`） | `MainChat.vue:13`、`CastWindow.vue:25`（ref 调 `continueDialog`） |
| `GameExtraUI` | 无 | 无 | `MainChat.vue:41`（**投屏窗口不挂它**） |

> ⚠️ `src/components/game/standard/index.ts:1-4` **只导出 4 个**（`GameBackground`/`GameDialog`/`GameRoleAvatar`/`GameRolesStage`）
> —— `StaticRolePresentation` / `Live2DRolePresentation` / `TouchAreas` **不在 barrel 里**，要按相对路径 import。

### 1.6.3 怎么协作（数据流）

```
MainChat.vue / CastWindow.vue / PetMode.vue
  └─ <GameRolesStage>            GameRolesStage.vue:8-12
       ├─ gameStore.presentRolesList  ────────┐   （谁在场）
       ├─ gameStore.currentInteractRoleId ──┐ │   （谁是当前说话人 → Live2D active-speaker）
       └─ <RoleAvatar v-for="role in presentRolesList">  GameRolesStage.vue:15-21
            └─ GameRoleAvatar.vue
                 ├─ role.live2d ? <Live2DRolePresentation> : <StaticRolePresentation>  :2-20
                 ├─ <TouchAreas v-if="gameStore.command === 'touch'">                     :23
                 └─ 气泡层（zIndex 2）                                                     :24-30
```

- **谁持有角色列表**：`gameStore.presentRolesList`（服务端 `game_status.present_role_ids` 的镜像，靠 `character:switch` 事件更新，`src/api/tauri-events.ts:408-426`）。
- **谁决定当前说话人**：`gameStore.currentInteractRoleId`（`GameRolesStage.vue:8` 传给 Live2DStage 的 `active-speaker-id`），由 `character:switch` 事件设置。
- **立绘怎么切情绪**：`GameRoleAvatar.vue:197-206` 监听 `[roleId, emotion, clothesName, character_folder]` → `resolveAvatar()`；
  `:209-230` 再监听 `role.emotion` → 等路径解析 → 等 DOM → 等图片加载 → 按 `EMOTION_CONFIG[emotion]` 播动画。
  **情绪来源**：`ai:reply` 事件的 `emotion` 字段（后端 `generator.rs:830-834` 决定用 predicted 还是 original_tag）→ `eventQueue` → store → `role.emotion`。

### 1.6.3 立绘怎么渲染（尺寸 / 定位 / 动画）

**DOM 结构**（`StaticRolePresentation.vue:2-21`）：
```
<Transition name="character-fade">            :2
  <div class="role-container-transition pointer-events-none absolute h-full w-full
              origin-[center_0%]" :style="layerStyle">   :3-7
    <ImageAcrossFade class="absolute h-[102%] w-full"
                     :class="animationClasses" position="center bottom"
                     :object-fit="objectFit" />             :9-18
  </div>
</Transition>
```
- **容器撑满视口**，`origin` 在 `center 0%`（顶部中心），缩放靠 `transform`。
- **图片高度 102%**（`h-[102%]`），`position="center bottom"` → **底边锚定**。
- `object-fit` 是**动态**的：宽高比 ≥1.0 时 `"contain"`，否则 `auto NN%`（`GameRoleAvatar.vue:75-80`，窄屏把高度压到 80%~100%）。

**布局定位**（`GameRoleAvatar.vue:118-150` 的 `roleLayerStyle`）：
- 水平：按在场顺序均分 —— `left = ((myIndex+1)/(totalCount+1))*100%`（`:98-104`），再用 `role.offsetX` 微调、`translateX(-50%)` 居中（`:138-140`）。
- 垂直：`top = role.offsetY - 窄屏补偿 - 宽屏补偿`（`:125-126`；补偿公式在 `:83-95`）。
- 缩放：`scaleTotal = role.scale * castScale`（`:124`）。
- 透明度：`role.show ? 1 : 0`（`:141`）。
- 过渡：`left 0.5s cubic-bezier(.25,.8,.5,1), top .3s ease, opacity .3s ease-in-out`（`:142-143`）。
- 光照滤镜：`filter: brightness/contrast/saturate/drop-shadow/sepia`（`:106-116`）。
- **两张层**：静态立绘 `zIndex:"1"`、气泡/特效层 `zIndex:"2"`（`:152-153`）—— 共用同一套 `roleLayerStyle`。

**动画**：`import "./avatar-animation.css"`（`GameRoleAvatar.vue:44`），类名通过 `containerClasses` 挂到图片上（`:155-157`），
动画名 = `activeAnimationClass`（`:64`），由 `EMOTION_CONFIG[emotion]` 决定，`@animationend` 回调 `handleAnimationEnd`（`:2-20` 的 `@animation-end`）。

`avatar-animation.css`（219 行）一览：

| keyframes | 行 | 对应类 | 类定义行 |
|---|---|---|---|
| `angryJump` | `:2` | `.angry-jump` | `:136` |
| `suprisedJump` | `:22` | `.suprised-jump` | `:171` |
| `heartBeat` | `:36` | `.heart-beat` | `:178` |
| `seriousThink` | `:56` | `.serious-think` | `:150` |
| `embarrassedEmo` | `:67` | `.embarrassed-emo` | `:157` |
| `happyBounce` | `:82` | `.happy-bounce` | `:143` |
| `naughtyBounce` | `:95` | `.naughty-bounce` | `:164` |
| `breathing` | `:106` | `.character-animation`(基类 `:117`) / `.normal`(`:121`) | — |
| （可见性） | — | `.avatar-visible`(`:126`) / `.avatar-hidden`(`:130`) | — |
| （气泡） | — | `.bubble`(`:186`) / `.bubble.show`(`:208`) / `.bubble.angry`(`:213`) / `.bubble.happy`(`:217`) | — |

**情绪映射表**：`src/controllers/emotion/config.ts`
- `EMOTION_CONFIG_EMO`（`:16-37`）——**19 个情绪 → 素材文件名**的归一化表，例：`哭泣 → 伤心`、`难为情 → 羞耻`。`GameRoleAvatar.vue:178` 用它把 `role.emotion` 转成文件名。
- `EMOTION_CONFIG`（`:39` 起）——每个情绪一个配置对象，字段 `{animation, bubbleImage, bubbleClass, audio}`（例 `:40-45` 的 `厌恶`）。`GameRoleAvatar.vue:228` 读它决定播哪个动画。

### 1.6.4 立绘 vs 头像

| | 头像 | 立绘 |
|---|---|---|
| 磁盘路径 | `game_data/characters/<角色>/avatar/头像.webp` 或 `avatar/<情绪>.webp` | 同目录 `正常.webp` 等（**同一套文件**，区别在尺寸与用法） |
| 后端命令 | `get_avatar_file`（`src-tauri/src/api/character.rs:458`） | 同上（同一个命令） |
| 前端组件 | 列表/面板里的小圆图（`getAvatarFile()`，`src/api/services/character.ts:172-177`） | `GameRoleAvatar` → `StaticRolePresentation`（大图，撑满视口） |
| 真实素材 | `data/game_data/characters/诺一钦灵/avatar/{正常,高兴,生气,…}.webp` + 服装子目录 `avatar/泳装/` | 同左 |

## 1.7 角色素材怎么加载

**唯一入口是 Tauri 命令 `get_avatar_file`**（`src-tauri/src/api/character.rs:458-…`），参数 `{characterFolder, emotion, clothesName}`：

1. 允许的扩展名：`png/jpg/jpeg/webp/bmp/gif`（`character.rs:463`）。
2. `clothesName` 为空或 `"default"` → 不追加子目录；否则拼 `avatar/<clothesName>/`（`:465-469`）。
3. **候选目录按优先级**（`:471-488`）：
   - ① 主角色：`characters/<folder>/avatar`（插件角色解析到 `plugins/<id>/characters/<folder>`，走 `resolve_character_dir`）
   - ② NPC/剧本角色：`<剧本目录>/characters/<folder>/avatar`
4. 命中返回**绝对路径**（`:501-506`）；「平静」找不到回退「正常」（`:508` 起）。

**前端转 URL**：`convertFileSrc(path)`（`@tauri-apps/api/core`）→ `asset://` URL。
零散调用点：`GameRoleAvatar.vue:182-188`、`src/components/pet/GameRoleAvatar.vue:218-224`、`GameBackground.vue:119,258`、`Live2DStage.vue:142-147`、`src/api/services/font.ts:91`、`script-editor` 系列。

**`public/` 静态资源 vs `data/` 的分工**：
- `public/` 里的东西**直接进前端包**（如 `public/world_map/*.js`，运行时由 `useWorldModules.ts` 用 `<script>` 注入）。
- `data/` 里的东西是**用户可改的数据**，走 `convertFileSrc` 或专用命令读。
- `src-tauri/tauri.conf.json:25-28` 的 `assetProtocol.scope = ["**"]` 意味着 **asset:// 可以读全盘**（安全上偏宽，但省事）。

## 1.8 i18n

- 初始化：`src/locales/index.ts:1-143`。`createI18n({legacy:false, locale: detectLocale(), fallbackLocale:"zh-CN", messages:{...}})`（`:70-83`）；`app.use(i18n)` 在 `src/main.ts:39`。
- 语言目录：`src/locales/{zh-CN, zh-HK, ja, en}/`，每语言 13 个文件（`index.ts` + `advance/api/common/game/misc/nav/pet/scriptEditor/settings/stores/ui/views`）；聚合在 `zh-CN/index.ts:1-27`。
- **基准 schema 是 `zh-CN`**（`index.ts:56` `type MessageSchema = typeof zhCN`），其他语言用 `as MessageSchema` 强转，缺键运行时回落中文（`:73-78` 注释）。
- **额外的文件层**：启动时 `get_locale_messages` 从 `<data_dir>/locales/<locale>.json` 读词条并与内置**深合并**（`index.ts:95-113`），用户编辑优先、缺键用内置兜底；带 `__locale_version` 版本戳做重新播种（版本 = 内置词条轻量 hash，`index.ts:28-39`；后端 `src-tauri/src/api/locale.rs:25` 命令、`:34` 目录、`:39` 播种、`:48-59` 版本比对重播）。
- 简→繁：`opencc-js`（`index.ts:8`），`hkify()` 只作用于**对话内容**显示层（`:136-143`，消费点 `dialogue-processor.ts:31`）；`isJaLocale()` 同理（`:132`）。**界面词条要手写繁体。**
- `schema-i18n.ts`（303 行）：**Rust schema 驱动**的字段文案映射（`EVENT_KEYS:26-44`、`CATEGORY_KEYS:56-60`、`FIELD_KEYS`、`STORY_FIELD_KEYS`）——剧本编辑器的事件类型/字段名走这里，不补就显示 Rust `schema.rs` 的中文原文。

**新增一条文案要动哪些文件**（以「世界模拟」菜单项为例）：

| 文件 | 改什么 |
|---|---|
| `src/locales/zh-CN/views.ts` | 在 `menu: {...}` 里加键（`menu` 段起于 `src/locales/zh-CN/views.ts:48`，现有 `startGame`/`continueGame`/`gameConfig`/`freeDialogue`/`storyMode` 等，见 `:49-55`） |
| `src/locales/zh-HK/views.ts` | 同步加（**繁体要手写**；`scripts/generate-zh-hk.mjs:19-20` 只是生成辅助） |
| `src/locales/ja/views.ts`、`src/locales/en/views.ts` | 同步加（不加则回落中文） |
| `src/locales/schema-i18n.ts` | **仅当**该文案属于剧本编辑器 schema 驱动的字段时才需要登记 |
| 使用处 | 模板 `$t("views.menu.xxx")`（如 `MainMenuOptions.vue:4,8,13,20,23,26`），或 TS 里 `useI18n()`（`MainMenu.vue:109`）/ `i18n.global.t(...)`（`ui.ts:376-394`、`stores/modules/ui/dialog.ts:30`） |
| （可选）运行时覆盖 | 直接编辑数据目录的 `data/locales/<locale>.json`，改语言或重启生效；注意 `__locale_version` 会在内置词条变更时**重新播种覆盖** |

> 注：官方线当前的地图入口走的是**硬编码中文字面量**「世界」（`src/components/views/menu/page/MainMenuOptions.vue:17`），**没有走 i18n** —— 这是我们自己的遗留，正式提 PR 时要补 i18n。

---

# 二、Rust 后端（Tauri 2）

## 2.1 `src-tauri/src/` 顶层模块清单

| 条目 | 职责 | 代表位置 |
|---|---|---|
| `main.rs` | 6 行；Windows release 子系统 + 调 `ling_chat_lib::run()`。**它调 lib.rs** | `src/main.rs:5` |
| `lib.rs` | **库入口**（926 行）：日志 → `generate_context!()`(:261) → `Builder` 装 9 个插件(:279-291) → `.setup`(:293) → `manage` 状态(:303-315) → `init::initialize`(:319) → `fill`(:495-525) → `.invoke_handler`(:675) → `.run`(:918) | `src/lib.rs:236`(fn run) |
| `achievements/` | 成就：内置表、解锁落盘 + 广播 | `achievements/mod.rs:17`、`manager.rs:9,89` |
| `adventures/` | 冒险/羁绊解锁、完成、重置 | `adventures/manager.rs:9`、`trigger.rs:25` |
| `ai_service/` | **全部 AI 能力**，134 个 `.rs` —— 见 §2.4 | `ai_service/mod.rs:1-15` |
| `api/` | **Tauri 命令层**，34 文件 / 173 命令 | `api/mod.rs:1-25`、`:134` |
| `cast/` | 投屏：独立窗口截图 → MJPEG 串流（**官方线独有**） | `cast/mod.rs:1-21,35` |
| `config/` | settings.json store、`AppConfig`、配置树 | `config/mod.rs:34`、`app_config.rs:74` |
| `db/` | SQLite + sea-orm：entities / managers / 旧 Python 库兼容 | `db/mod.rs:13`(init_db)、`compat.rs:36` |
| `init/` | 启动装配：播种数据目录、角色入库、DB/LLM/记忆、ASR | `init/mod.rs:33`(initialize)、`:179`(init_asr)、`static_copy.rs:7` |
| `lan_sync/` | 局域网全量同步（mDNS + HTTP + manifest diff） | `lan_sync/mod.rs:42` |
| `manifest/` | 数据清单类型 + SHA256（被 resource_sync / lan_sync / init 共用） | `manifest/mod.rs:20,36,60` |
| `migration/` | sea-orm 迁移：1 建表 + 6 增量 | `migration/mod.rs:6-17` |
| `plugins/` | manifest.toml + RustPython 插件；工具进 registry、资源进游戏数据 | `plugins/mod.rs:1-13`、`manager.rs:24` |
| `resource_sync/` | 安装包内置资源 → `data/` 本地同步 | `resource_sync/mod.rs:74` |
| `utils/` | 日志桥 / 性能探测 / prompt / 路径 / YAML / 解压 / 下载 / 代理 / TLS | `utils/mod.rs:1-16`、`log_bridge.rs:11` |

## 2.2 `AppState` 的完整结构

### 2.2.1 外层：防竞态壳

```rust
pub struct AppState { inner: std::sync::OnceLock<InnerAppState> }   // src/lib.rs:148-150
```
- `empty()`(`:154`) / `fill()`(`:161`，重复写 panic) / `data()`(`:172`，IDE 补全用)。
- `Deref`：桌面直接 `expect`（`:181-189`）；**Android 自旋等待**（`:195-206`）——
  因为 Tauri 在 Android 上**在 setup 闭包执行前就创建 webview**，前端 JS 一加载就 invoke，
  此时若 panic 会把 IPC worker 线程拖死。
- **`app.manage(AppState::empty())` 在 `src/lib.rs:315`（setup 最开头）**，真实值在 `:495-525` 一次性 `fill`。

### 2.2.2 `InnerAppState`：22 个字段

定义 `src/lib.rs:83` 起，字段与初始化行（= `fill` 实参行）：

| 字段 | 类型 | init | 用途 / 被谁用 |
|---|---|---|---|
| `db` | `sea_orm::DatabaseConnection`（**无外层锁**） | 498 | 命令内 59 处/44 条：`api/save.rs:64,131,202,278,338,382`、`api/chat.rs:75,133`、`api/game.rs:329`… |
| `ai_service` | `SharedAIService = Arc<tokio::sync::Mutex<AIService>>`（别名 `ai_service/service.rs:268`） | 499 | 命令内 54 处/40 条：`api/chat.rs:48`、`api/save.rs:133,204,280`… **锁顺序约定 `ai_service.lock()` → `game_status.lock()`**（`ai_service/tools/mod.rs:50-56`） |
| `chat` | `ChatComponents`（`lib.rs:63-71`）= `llm: LlmSlot` + `processor: Arc<MessageProcessor>` + `translator: Arc<Translator>` | 500 | `api/chat.rs:38,96`、`api/game.rs:882` |
| `script_channels` | `SharedScriptChannels = Arc<Mutex<ScriptChannels>>`（`script_engine/events/mod.rs:68`） | 501 | `api/script.rs:85,146,177` |
| `generation_lock` | `Arc<tokio::sync::Mutex<()>>` | 502 | `api/chat.rs:161,263,359`、`api/game.rs:393` |
| `tool_registry` | `Arc<ToolRegistry>`（内部 3 个 **std** RwLock，`tools/registry.rs:26-30`） | 503 | `api/chat.rs:98`、`api/tool_settings.rs:65,113` |
| `tool_settings` | `SharedToolSettings(Arc<RwLock<ToolSettings>>)`（std，`tools/settings.rs:364`） | 504 | `api/tool_settings.rs:18,110,126` |
| `plugin_manager` | `Arc<PluginManager>` | 505 | `api/ambient.rs:73`、`api/background.rs:73`、`api/music.rs:85`、`api/plugins.rs:256` |
| `proactive_system` | `Option<Arc<tokio::sync::Mutex<ProactiveSystem>>>` | 506 | `api/chat.rs:107`、`api/schedule.rs:63,74`、`api/mod.rs:129` |
| `achievement_manager` | `Arc<tokio::sync::Mutex<AchievementManager>>` | 507 | `api/achievement.rs:12,23`、`api/chat.rs:116,135` |
| `screen_analyzer` | `Arc<tokio::sync::Mutex<ScreenAnalyzer>>` | 508 | `api/chat.rs:68,563` |
| `screenshot_capture` | `Arc<tokio::sync::Mutex<ScreenshotCaptureState>>`（struct `lib.rs:74-80`） | 509 | `api/screenshot.rs:43,79,122` |
| `auto_save_manager` | `Arc<tokio::sync::Mutex<AutoSaveManager>>` | 510 | **命令 0 处**；只在 setup 后台：`:570-574`（关闭）+ `:577-582`（每 5 分钟） |
| `asr_state` | `Arc<AsrState>`（`asr/mod.rs:23-28`，内部一个 tokio Mutex） | 511-513 | `api/asr.rs:103,113,147,187,251,264,307,316,324,362,378,444,466` |
| `god_agent` | `Option<Arc<GodAgentCore>>`（`god_agent/core.rs:15`） | 514（构建 `:482-487`） | `api/chat.rs:100`、`api/settings.rs:280`、`api/game.rs:906` |
| `skill_agent` | `Arc<SkillAgentState>`（`skill_agent/mod.rs:33-41`） | 515 | `api/script_editor/agent.rs:308,327,339,364` |
| `chat_command_approvals` | `ApprovalMap = Arc<Mutex<HashMap<String, ApprovalRequest>>>`（std，`command_executor.rs:198`） | 516 | `api/tool_settings.rs:177` |
| `chat_file_change_approvals` | 同上 | 517 | `api/tool_settings.rs:207` |
| `chat_file_delete_approvals` | 同上 | 518 | `api/tool_settings.rs:237` |
| `background_commands` | `Arc<BackgroundCommandManager>`（`tools/background_command.rs:34-37`） | 519-521 | 命令 0 处 |
| `preview_task` | `Arc<tokio::sync::Mutex<Option<JoinHandle<()>>>>` | 522 | `script_editor/commands.rs:1459,1497,1936` |
| `pending_preview_restore` | `Arc<tokio::sync::Mutex<Option<PreviewSession>>>` | 523 | `script_editor/commands.rs:1457,1651` |

**锁类型速记**：字段级互斥**全是 tokio 异步 Mutex**（除 `ToolRegistry` / `SharedToolSettings` 内部是 std RwLock）；
`LlmSlot = Arc<tokio::sync::RwLock<Option<Arc<LlmClient>>>>`（`ai_service/llm/mod.rs:43`）；`OnceLock` 只在最外层壳。

**AppState 之外 `manage` 的 10 个状态**：
`pet::HitTestState`:303、`resource_sync::ResourceSyncState`:304、`lan_sync::LanSyncState`:305、`cast::CastManager`:306、`utils::cpu_perf::CpuDetectionCache`:307、`utils::gpu_perf::GpuDetectionCache`:308、`api::role_archive::RoleArchiveState`:309、`AppState` 空壳:315，以及本地 TTS 的 `LocalTtsSwitch`（`ai_service/tts/local/setup.rs:58`）与 `LocalTtsState`（`setup.rs:73`）。

**全局静态**（节选）：`DATA_DIR: OnceLock<PathBuf>`（`init/static_copy.rs:4`，读 `get_data_dir():13` ← `api/mod.rs:54`）；
`APP_HANDLE`（`utils/log_bridge.rs:11`，`lib.rs:296` 设置）；`LOG_BUFFER: Lazy<Mutex<VecDeque>>`（`log_bridge.rs:19`）；
**剧本事件注册表** `REGISTRY: LazyLock<RwLock<HashMap<&str, EventFactory>>>`（`script_engine/events/mod.rs:128`，初始化于 `script_engine/mod.rs:26` ← `service.rs:64`）。

## 2.3 Tauri 命令是怎么注册的

- **`generate_handler!` 宏**：`src/lib.rs:675`（`[`）→ `:917`（`])`），链尾 `:918 .run(context)`；context = `:261 tauri::generate_context!()`。
- **共 234 条命令路径**（241 行 − 6 行注释 − 1 行 `#[cfg]` 属性）。
- 反向核对：`src/` 下 `#[tauri::command]` 237 处 → 名字去重 234 个，差额来自 3 个 **cfg 双实现**（同一名字两平台各一份）：
  `api/font.rs:51`(win) / `:123`(非 win)、`api/save.rs:400`(win) / `:430`(其他)、`lan_sync/mod.rs:464`(desktop) / `:471`。
  → **注册 234 = 定义 234，无漏注册**。
- **唯一的条件注册**：`src/lib.rs:697-698` `#[cfg(target_os = "windows")] api::settings::set_hdr_mode`。

**坑（照抄时最容易炸的几条）**：

1. **宏里只写一次名字，两平台各自实现** —— 看到同一个命令名有两处 `#[tauri::command]` 不要以为是重复。
2. **`generate_handler!` 按「命令宏定义处的路径」找 `__cmd__xxx`**，所以模块里的命令**必须带模块前缀**。
   【我】的地图命令就是活例子：`world_map::bridge::world_map_district_stream`、【我】`src-tauri/src/lib.rs:576`，
   注释在【我】`lib.rs:574-575` 与 【我】`src-tauri/src/world_map/mod.rs:34-45`；写短路径会 **E0433**。
3. **新增命令必须手工加进这个宏** —— 没有自动注册，没有 `build.rs` 扫描。
4. **自定义命令不需要在 `capabilities/*.json` 里声明**。
   依据：Tauri 2 官方文档 "By default, all commands that you registered in your app (using `tauri::Builder::invoke_handler`) are allowed to be used by all the windows and webviews of the app."
   （https://v2.tauri.app/security/capabilities/ ）；本地佐证：【我】注册了 21 条 `world_map_*`（【我】`lib.rs:555-581`），
   而 `capabilities/default.json:6-38` 一个都没列，且【我】`src-tauri/gen/schemas/acl-manifests.json` 里 `world_map*` 零命中。
   → 只有 `core:*` 与 `plugin:*` 走 ACL。
5. **Builder 链**：`lib.rs:278` `Builder::default()` → `:279-285` 7 个无条件插件（opener/store/dialog/screenshots/fs/notification/android_fs）
   → `:288-291` 桌面额外 2 个（updater/process，用 **变量遮蔽**写法：`#[cfg(desktop)] let builder = builder.plugin(...)`）
   → `:293 .setup` → `:675 .invoke_handler` → `:918 .run`。
6. **setup 顺序不能乱**：`:300` `init_data_dir` 必须最早（`init/mod.rs:37-41` 注释：重复调用 `OnceLock` 会 panic）
   → `:315` manage 空壳 AppState → `:319-320` `rt.block_on(init::initialize)` → `:495-525` fill → `:528-534` init_asr。

## 2.4 `ai_service/` 分层

`src-tauri/src/ai_service/`（134 个 `.rs`）顶层 6 文件 + 11 个子目录：

| 条目 | 职责 | 关键位置 |
|---|---|---|
| `mod.rs` | 模块声明（15 行） | `ai_service/mod.rs:1-15` |
| `service.rs` | **不是状态机、不是事件循环**，而是「AI 服务可变状态容器」：`AIService:28-49` = `db:29` / `data_dir:30` / **`game_status: Arc<Mutex<GameStatus>>:31`（真正的世界状态）** / `config:32` / 快照字段 `:35-45` / `script_manager:48`；方法只有状态管理（`new:52`、`import_settings:103`、`init_game_status:149`、`load_lines:198`、`persist/restore_memory_banks:221/231`、`clear/reset_lines:241/262`）；单例 = `SharedAIService:268` | `service.rs:28-49,268` |
| `types.rs` | 全部共享类型：`ToolCall:13-30`、`ToolDefinition`、`parse_tool_args:68-91`、`LlmMessage:93-101`、`LineBase:152-167`、`GameLine:203-208`、`GameMemoryBank:233-282`、`GameRole.memory:569` | `types.rs` |
| `config.rs` | `AIServiceConfig`（8 行有效） | `config.rs:8` |
| `translator.rs` | 翻译 LLM 封装 | `translator.rs:39` |
| `screen_analyzer.rs` | 截图理解（视觉模型） | `screen_analyzer.rs:103` |
| `asr/` (9) | 语音识别：`provider.rs:155`(trait) / `:208`(Qwen) / `:420`(Llama)、`provider_stream.rs:182`(DashScope 实时 WS)、`provider_stream_llama.rs:89`、`session.rs:54`、`vad.rs:62`(Silero ONNX)、`vad_segmenter.rs:48`(纯状态机)、`settings.rs:67,109` | — |
| `emotion/` (2) | **情绪分类器**：`classifier.rs:48` 加载 ONNX BERT，`predict:157-194`，`run_inference:196-270` | `emotion/classifier.rs` |
| `game_system/` (7 + script_engine 26) | `game_status.rs:17 GameStatus`、`role_manager.rs:22 GameRoleManager`、`persistent_memory_system.rs:127`、`memory_builder.rs:5`、`auto_save.rs:26`、`scene_store.rs:237`；`script_engine/`：`script_manager.rs:43`、`events/mod.rs:47,75,112,128`、`events/` 下 **18 文件 = 16 个事件处理器 + mod** | — |
| `god_agent/` (4) | 见 §2.6 | `god_agent/core.rs:15` |
| `llm/` | `mod.rs:117 LlmClient / :95 LlmChunk / :43 LlmSlot`；`provider.rs:43 trait`；`provider_config.rs` 多供应商解析；`providers/genai_provider.rs:25`、`providers/kimi_code.rs:71`；`codex/` | — |
| `message_system/` (6) | **对话管线核心**：`processor.rs:91`（`append_user_message:259`、情绪分段 `:115`）、`generator.rs:86/48/75`（`process_message:101`、`process_notification:157`）、`producer.rs:29`、`responses.rs:41-189`、`events.rs:15,21,48` | — |
| `proactive_system/` (9) | 主动搭话：`mod.rs:37`、`start:103`（30s 循环 `:118-150`）、`delivery_evaluator.rs:7,12`（唯一投放闸门）、`activity_monitor.rs:43`、`schedule_manager.rs:4`、`types.rs:6,86,124` | — |
| `skill_agent/` (9) | 剧本编辑器 AI 助手：`mod.rs:33 SkillAgentState`、`core.rs:200 run_chat`（多轮工具循环）、`command_executor.rs:198`、`file_tools.rs:34`、`skills.rs:45`、`db.rs:16-187` | — |
| `tools/` (16) | **通用工具子系统**：`executor.rs:61 Tool trait / :79 ToolExecutor`（默认 **2 秒超时** `:89`）、`registry.rs:26`、`mod.rs:94 built_in_registry`（注册 30 个工具）、`permissions.rs:57`（场景组×角色组矩阵）、`tool_loop.rs:72 stream_with_tool_loop`；具体工具 `clock:10`、`character:16,58`、`scene:14,56`、`schedule:121,…`、`memory:25,…`、`status:12,58`、`skill_files:…`、`read_media_file:65`、`web_search:29` | `tools/mod.rs` |
| `tts/` (29) | `provider.rs:24,49`、`voice_maker.rs:47`、`adapters/` 11 个、`cloud/`（7 命令 + 音色注册）、`local/`（SBV2 进程内实现 + 14 命令） | — |

## 2.5 **对话管线：从用户发消息到角色回复**（最重要的一节）

### 2.5.1 入口命令

| 命令 | 位置 | 用途 |
|---|---|---|
| `send_chat_message(app, text, screenshot_base64)` | `src-tauri/src/api/chat.rs:20-25` | **主入口**（普通/流式同一入口） |
| `rollback_conversation(app, message_seq)` | `api/chat.rs:254-258` | 回溯对话 |
| `generate_line_voice(app, line_seq)` | `api/chat.rs:353-354` | 历史台词补语音 |
| `feed_image(app, path)` | `api/chat.rs:546-547` | 图片投喂 |
| `feed_text(app, text)` | `api/chat.rs:589-590` | 文本投喂 |
| `trigger_ai_response(app)` | `api/chat.rs:505`（**非命令**，pub 函数） | 无用户输入拉起回复 |
| `notify_player_entry(app)` | `api/game.rs:819-820` | 入场问候（内部 `process_message(None)`） |

**流式不是独立命令**：`send_chat_message` spawn 后台任务，由 publisher 逐句 `emit("ai:reply")`（`message_system/generator.rs:474`）。

**前端调用点**：`src/components/game/standard/GameDialog.vue:914`、`src/components/pet/ChatInput.vue:215`、
`src/components/game/standard/TouchAreas.vue:250`、`src/components/pomodoro/PomodoroPanel.vue:386`。

### 2.5.2 完整链路（箭头图）

```
[前端] GameDialog.send()                        src/components/game/standard/GameDialog.vue:914
  → invoke("send_chat_message")
    → send_chat_message                         src-tauri/src/api/chat.rs:21
      ├─ 空校验 :27 ／ "/" 调试指令 → handle_debug_command :32 → :175
      ├─ llm 快照 = slot_snapshot(&state.chat.llm)          chat.rs:38
      ├─ concurrency = AppConfig::load().consumers          chat.rs:42
      ├─ game_status 句柄 :47 ／ user_name :52
      ├─ events::emit_thinking(true)                        chat.rs:57 → message_system/events.rs:21
      ├─ [可选] 截图分析                                     chat.rs:60-88
      │    → ScreenAnalyzer::analyze_image :69
      │    → gs.add_line(Narrator 旁白) :74 → game_status.rs:103
      ├─ 构造 GeneratorDeps{source: UserChat, …}            chat.rs:90-104
      ├─ 旁路 spawn：proactive.on_user_message_received     chat.rs:107-113
      ├─ 旁路 spawn：achievements handle_user_message       chat.rs:119-130
      ├─ 旁路 spawn：adventures check_all_adventures        chat.rs:137-158
      └─ tokio::spawn + generation_lock                     chat.rs:163-169
           → MessageGenerator::new(deps).process_message(Some(text))   generator.rs:101
                │
                ├─ Step1 handle_user_message                 generator.rs:173
                │    → MessageProcessor::append_user_message :183 → processor.rs:259
                │        （抽 {…}→旁白、[!Temp!]→$…$、时间感知 :294-304、拼 {系统提醒:…} :320-323）
                │    → gs.add_line(USER 行, sender_role_id=Some(0))       :194 → game_status.rs:103
                │         └→ refresh_memories                game_status.rs:108 → :112
                │              └→ GameRoleManager::sync_memories          role_manager.rs:260
                │                   ├─ PersistentMemorySystem::sync_to_role        role_manager.rs:317
                │                   ├─ check_and_trigger_auto_update                role_manager.rs:318
                │                   ├─ get_slice_start_index（取窗口）              role_manager.rs:319
                │                   ├─ ★MemoryBuilder::build  ← prompt 在这里成型   role_manager.rs:347
                │                   │     → memory_builder.rs:72-273
                │                   └─ merge_memory_bank_into_context              role_manager.rs:353 → :610
                │
                ├─ Step1.5 detect_scene_change               generator.rs:106 → :214
                │
                ├─ Step2 god_agent_pre_select                generator.rs:110 → :306   【上帝 Agent ①】
                │
                └─ Step3 loop（God Agent 可多轮）             generator.rs:118-149
                     ├─ get_current_context（取 role.memory）  generator.rs:120 → :246
                     │    → gs.get_role(db, rid)              :252 → game_status.rs:94
                     │    → role.memory.clone()   ← 这就是发给 LLM 的 messages
                     ├─ execute_pipeline                      generator.rs:131 → :257
                     │    └─ run_pipeline                     generator.rs:267 → :418
                     │         ├─ stream_with_tool_loop       generator.rs:434 → tools/tool_loop.rs:72
                     │         │    ├─ registry.allowed_tools(source, role_name)  tool_loop.rs:86
                     │         │    ├─ registry.definitions_for_allowed           tool_loop.rs:87
                     │         │    ├─ [provider 不支持 tools] → llm.complete_stream   :93
                     │         │    └─ [支持] 插 TOOL_USE_POLICY_PROMPT           :114
                     │         │         → llm.complete_stream_with_tools         :146
                     │         │         → ToolExecutor::execute  :228 → tools/executor.rs:94
                     │         │         → emit ai:tool_activity / _progress / ai:tool_call
                     │         │                                   tool_loop.rs:368 / :379 / :438
                     │         ├─ mpsc channel(concurrency*2)                    generator.rs:454-457
                     │         ├─ publisher：按 index 顺序 emit "ai:reply"        generator.rs:465-483
                     │         ├─ producer：LLM 流 → 句子                         generator.rs:540 → producer.rs:57
                     │         │    ★ 切句：找 '【' → 找 '】' → 吃到下一个 '【'    producer.rs:93-170
                     │         │    ★ fix_ai_generated_text 归一化  producer.rs:214 → processor.rs:348
                     │         │    ★ emit ai:thinking_progress   producer.rs:186 → events.rs:31
                     │         └─ consumer 池 ×concurrency                        generator.rs:489-529
                     │              → consume_sentence            generator.rs:506 → :650
                     │                 ├─ parse_segments          :664 → :698
                     │                 │    → processor.parse_and_classify_emotional_segments
                     │                 │                              processor.rs:115
                     │                 │       → EmotionClassifier::predict  ★情绪
                     │                 │            processor.rs:168 → emotion/classifier.rs:157
                     │                 ├─ enrich_segments         :670 → :754
                     │                 │    ├─ Translator::translate_segments_to  :779
                     │                 │    └─ VoiceMaker::generate_voice_files   ★TTS
                     │                 │         :792 → tts/voice_maker.rs:516
                     │                 ├─ build_reply_response    :673 → :799 → ReplyResponse
                     │                 └─ add_assistant_line      :692 → :880
                     │                      → gs.add_line(ASSISTANT 行)   :910 → game_status.rs:103
                     ├─ cleanup_temp_message                  generator.rs:138 → :288
                     └─ god_agent_post_select                 generator.rs:145 → :353   【上帝 Agent ②】
                          → emit_character_switch → emit "character:switch"     :407-416
```

### 2.5.3 消息怎么组装

- **历史来源**：**内存** `GameStatus.line_list: Vec<GameLine>`（`game_status.rs:21`）。**DB 只在存档时批量写**。
- **窗口/截断常量**：

| 值 | 默认 | 位置 |
|---|---|---|
| 记忆窗口 `recent_window` | **30** | `config/app_config.rs:40-42`（上限常量 `MAX_MEMORY_RECENT_WINDOW = 10_000`，`:64`） |
| 压缩触发阈值 `update_interval` | **250** | `app_config.rs:37-39` |
| 记忆段上限 short/long/user/promises | **500 / 2000 / 800 / 800** | `app_config.rs:43-54` → `persistent_memory_system.rs:96-105` |
| 工具结果裁剪 | 单条 12 000 字符 / 单轮累计 32 000 | `tools/tool_loop.rs:20-22` |
| 投喂文本截断 | 2000 | `api/chat.rs:609` |
| God Agent 决策窗口 | 20（最小 5） | `god_agent/config.rs:32,58` |

- **窗口的真正算法**：`PersistentMemorySystem::get_slice_start_index`（`persistent_memory_system.rs:270-295`）——
  从 `last_processed_global_idx` 往回数 `recent_window` 条**该角色可见且非 system** 的台词。
- **消息结构体**：
  - `LlmMessage { role, content, tool_calls, tool_call_id }` — `ai_service/types.rs:93-101`（构造器 `:104-145`）
  - `GameLine { base: LineBase, perceived_role_ids: Vec<i32> }` — `types.rs:203-208`
  - `LineBase` 字段 — `types.rs:152-167`：`id, content, original_emotion, predicted_emotion, tts_content, action_content, audio_file, thinking, tool_call, attribute, sender_role_id, display_name`
  - `LineAttribute` = `user/system/assistant/tool` — `db/entities/line.rs:6-15`；DB 列 `line.rs:19-40`（另有 `save_id`、`parent_line_id` 链表）

### 2.5.4 prompt 怎么拼

**两段式**：① 人设 system 提示**一次性生成**并固化成一条 `LineAttribute::System` 台词；② 每条台词后由 `MemoryBuilder` 把台词流还原成 messages。

**① 人设提示构建**：`sys_prompt_builder`（`utils/prompt.rs:180-241`），拼接顺序（中文模式 `:210-218`）：

1. `ai_prompt`（角色 `settings.yml` 的 `system_prompt`）
2. `build_framing_prefix_cn`（`prompt.rs:160-177`）—— 解释 `{旁白: ...}` / `{系统: ...}` 语义
3. `DIALOG_FORMAT_PROMPT_CN`（`prompt.rs:31-54`）
4. `DEFAULT_EXAMPLE_CN`（`prompt.rs:108-125`）
5. `EXAMPLE_CUSTOM` + `settings.system_prompt_example`（`prompt.rs:196-199`，常量 `:156-158`）
6. `DIALOG_FORMAT_PROMPT_2_EMOTION_LIMIT_HEAD`（**19 情绪白名单**，`prompt.rs:86-89`）
7. `DIALOG_FORMAT_PROMPT_2_BODY`（`prompt.rs:91-106`，含「输出边界铁律」）

日文模式把 3/4 换成 `DIALOG_FORMAT_PROMPT_JP`（`:56-84`）/ `DEFAULT_EXAMPLE_JP`（`:127-154`）。
**分隔符**：各常量自带前导换行，纯 `push_str` 顺序拼接，**没有显式 separator**。
变量替换只有 `%player%`（`replace_placeholder`，`prompt.rs:15-17`）。

**写进台词表的时机**：`service.rs:158-165`（`init_game_status`）、`api/game.rs:661-672`（`add_role_to_scene`）。

**③ 每轮 messages 的成型**：`MemoryBuilder::build`（`game_system/memory_builder.rs:72-273`）：

1. 建 `buffer` / `buffer_kind`（`:73-75`）+ `flush` 闭包（`:77-152`）
2. 遍历 `lines`（`:156`）：
   - **System 行**（`:158-174`）：仅当 `sender_role_id == target` 且尚无 system 时注入；重复只 warn（`:162-167`）
   - **带 tool_call 的 Assistant**（`:177-224`）→ 还原成 OpenAI `tool_calls` 格式
   - **Tool 结果行**（`:227-248`）→ `role:"tool"` 消息
   - **可见性过滤** `is_target`（`:19-24`）：`sender_role_id == target` **或** `perceived_role_ids` 含 target；否则跳过
3. 分类入 buffer（`:254-268`）：自己的 Assistant → `TargetAssistant`；其他 → `OtherBlock`；切换时 flush
4. 输出：`TargetAssistant` 拼成**一条** assistant 消息（`format_content_with_extras`，`:27-53`，格式 `【情绪】内容\n(动作)\n<日语>\n\n`）；
   `OtherBlock` 用 `format_context_line`（`:56-70`）**包进 `{...}`**（`:120`），并把末尾连续 user 行单独切出（`:98-107`）

**为什么这套设计对我们有利**：**所有 prompt 都从 `line_list` 派生**，所以任何「系统信息」只要变成一条 `LineAttribute::System` 或用 `PromptRole` 包一下（`utils/prompt.rs:269-293`），就会自动进上下文。

### 2.5.5 记忆怎么注入

1. **短期**：`get_slice_start_index`（`persistent_memory_system.rs:270`）→ 在 `role_manager.rs:330-334` 切片。
2. **长期**：**LLM 摘要压缩，不是向量检索**（这点很重要，别按 RAG 想）。
   `check_and_trigger_auto_update`（`persistent_memory_system.rs:354-428`）在可见台词 ≥250 时
   `spawn_background_update`（`:432-581`），`tokio::join!` 并发跑 **4 段**（`:487-524`）：
   `short_term` / `long_term` / `user_info` / `promises`，提示词硬编码在 `init_prompts()`（`:19-76`）。
   **4 段全成功才写回**并推进 `last_processed_global_idx`（`:526-542`、`commit_update_if_current` `:186-214`）。
   **没有打分/关键词/向量召回**。
3. **合并进 messages**：`merge_memory_bank_into_context`（`role_manager.rs:610-662`）：
   `system_addendum` 追加到**第一条 system** 末尾（`:617-630`，用 `content.contains` 去重 `:621`）；
   `short_term_prefix` 前插到**第一条非 system** 消息（`:632-648`）；连续 system 合并（`:650-661`）。
4. **数据结构**：`GameMemoryBank { schema_version, meta:{last_processed_global_idx, updated_at}, data:{short_term, long_term, user_info, promises} }`（`types.rs:233-282`）。

### 2.5.6 情绪怎么判定 / 影响什么

- **方式**：**本地 ONNX 分类器**（BERT 字符级 + 线性头，`MAX_SEQ_LEN=128`，置信阈值 `0.08`）——
  `emotion/classifier.rs:15-16`，`predict:157-194`，`run_inference:196-270`
  （含 `"撒娇"→"调皮"` 特例 `:169-177`、低置信返回 `"不确定"` `:243-255`）。
- **输入**：模型输出文本里的 `【情绪tag】` 原文；**输出**：`EmotionPrediction { label, confidence, top3, disabled, warning }`（`:19-28`）。
- **调用点**：`MessageProcessor::parse_and_classify_emotional_segments`（`processor.rs:166-172`）；
  分类器为 `None` 时回退 `(emotion_tag, 1.0)`。
- **装配**：`init/mod.rs:150` `load_emotion_classifier(enable_emotion_classifier, &data_dir)` → `init/mod.rs:151-157`；
  开关 `features.enable_emotion_classifier`（`config/keys.rs:38`，默认 true `app_config.rs:34-36`）；
  模型目录 `<data_dir>/third_party/emotion_model_19emo/`（`classifier.rs:78-92`）。
- **对 prompt 的影响 = 无**：进 prompt 的是**原始 tag**（`MemoryBuilder::format_content_with_extras` 用 `line.original_emotion`，`memory_builder.rs:29-33`）。
- **对前端的影响 = 立绘**：`ReplyResponse.emotion = predicted 非空 ? predicted : original_tag`（`generator.rs:830-834`），
  `original_tag` 单列（`:835`）→ `emit("ai:reply")`（`:474`）→ `src/api/tauri-events.ts:62` → `eventQueue.addEvent({type:"reply"})`（`:67`）
  → `gameStore` → `role.emotion` → `GameRoleAvatar.vue:209` 的 watch。

### 2.5.7 回复怎么后处理

1. **切句**：`StreamProducer::run`（`producer.rs:57-243`）：遇 `【` 起句 → 遇 `】` 闭合 → 吃到下一个 `【` 前（`:93-170`）；
   含 `【数字】` 兼容分支（`:134-168`）；末尾残余作为 `is_final=true`（`:206-240`）。
2. **去重**：`is_duplicate`（`producer.rs:287-293`，<8 字符豁免）。
3. **归一化**：`fix_ai_generated_text`（`processor.rs:348-413`）。
4. **情绪分段解析**：`parse_and_classify_emotional_segments`（`processor.rs:115-195`）→ `EmotionSegment`（字段 `processor.rs:24-38`，正则表 `:48-71`）。
5. **TTS**：`enrich_segments` → `VoiceMaker::generate_voice_files`（`generator.rs:792` → `tts/voice_maker.rs:516`）；
   选文 `segment_text_for_lang`（`voice_maker.rs:114-129`）；翻译目标语言决策 `tts_translation_language`（`generator.rs:709-730`）；
   产物写入 `<data_dir>/voice/`（`role_manager.rs:804`）。
6. **打字机是前端的**：`GameDialog.vue:929` `continueDialog`；后端只按句 emit，用 `ReplyResponse.duration` 控节奏（默认 `-1.0`，`generator.rs:874`）。

### 2.5.8 怎么落库

- **内存优先**：全程只改 `GameStatus.line_list`（`game_status.rs:103-110`）。
- **唯一 DB 写路径**：`SaveRepo::sync_lines(db, save_id, &line_list)`（`db/managers/save_repo.rs:280-415`）——
  按 `id` 强匹配 → 弱匹配（content+attribute+sender+action+tool_call，`:325-333`）→ 找分叉点 `diverge`
  → 删陈旧行及 `line_perception`（`:341-356`）→ 插新行并按 `parent_line_id` 串链（`:368-399`）
  → 更新 `save.last_message_id`（`:405-412`）。
- **调用点（无显式事务）**：`api/save.rs:149`（create_save）、`api/save.rs:296`（update_save）、
  `api/chat.rs:296`（rollback）、`api/chat.rs:491`（补语音）、`auto_save.rs:131`（自动存档）。
- **MemoryBank 落库**：`persist_memory_banks_to_db`（`role_manager.rs:575-602`）→ `MemoryRepo::upsert_memory`（`memory_repo.rs:55-88`）。
- **自动存档**：`AutoSaveManager::run_periodic`（`auto_save.rs:50-59`，间隔 300s `:15`）；`perform_save`（`:106-190`）：
  hash 变化检测（`:203-221`）→ `find_or_create_slot`（`:225-271`，标题前缀 `"自动存档"` `:14`）→ sync_lines → 快照 → MemoryBank → 剧本状态。
  退出钩子 `setup_close_handler`（`:65-101`）→ `emit("app:close-ready")`（`:97`）→ 前端 `App.vue:373`。

### 2.5.9 ⭐ 「地图上下文」最合适的注入点（3 个候选）

| # | 位置 | 做法 | 优点 | 风险 |
|---|---|---|---|---|
| **① 推荐** | `GameRoleManager::sync_memories`（`role_manager.rs:310-327` 取 addendum，`:350-361` 写 `role.memory`）+ `merge_memory_bank_into_context`（`:610-662`） | 加第 4 参 `map_addendum: &str`，复用现有「追加首条 system + `contains` 去重」逻辑（`:617-630`） | **唯一「每角色 × 每次 `add_line`」都重算 prompt 的咽喉**；群聊/多轮 God Agent 自动生效；调用点只有一处（`role_manager.rs:353`） | **调频极高**（每条台词一次，含每条 assistant 句）；**在 `game_status` 锁内被调用**（`game_status.rs:112` 持 `&mut self`）→ **绝不能在这里 await 网络**，必须提前缓存快照 |
| **②** | `MessageGenerator::get_current_context`（`generator.rs:246-254`） | `Ok(role.memory.clone())` 之后追加/前插一条 `LlmMessage`，或改写首条 system | 最贴近「这一轮到底发什么」；每轮 loop 调一次（`:120`），改动面最小；**不在 `add_line` 高频路径** | 绕过 `MemoryBuilder`/memory_bank 既有构造逻辑；God Agent 多轮时可能重复注入（需自去重） |
| **③** | 新增工具 `tools/world_map.rs`，注册进 `tools/mod.rs:94` `built_in_registry` | 抄 `tools/scene.rs:14-56`（`SceneList`，无参只读工具最佳模板） | **零 prompt 侵入**；权限自动放行（见 §2.6.5） | 模型可能不调用；多一次 LLM 往返；**prompt 里没有常驻位置感，模型想不到要查** |

**建议 ①+③ 组合**（常驻一行精炼摘要 + 详查工具）。

**不推荐**改 `sys_prompt_builder`（`utils/prompt.rs:180`）：它在 `import_settings`/`add_role_to_scene` 时**一次性固化**成 System 台词
（`service.rs:158-165`、`game.rs:661-672`），地图是动态的，写进去会过期，还要重写台词表。

## 2.6 上帝 Agent（`src-tauri/src/ai_service/god_agent/`）

### 2.6.1 文件与职责

| 文件 | 行数 | 职责 |
|---|---|---|
| `mod.rs` | 12 | 模块声明 + re-export `GodAgentCore`（`:8-12`） |
| `config.rs` | 112 | 配置结构/加载（`:18-66`）、LLM provider **四级 fallback**（`:73-112`） |
| `core.rs` | 217 | 激活判断（`:35-37`）、决策 prompt 构建（`:47-137`）、`decide_next_speaker`（`:139-216`） |
| `tools.rs` | 69 | **唯一工具** `select_next_speaker`（`:12-33`）+ 解析（`:44-55`）+ 容错 `parse_role_id`（`:58-69`） |

### 2.6.2 它到底做什么

**一句话**：多人自由对话的「AI 导演」—— 在自由对话且**在场 NPC > 1** 时，用一次独立的 LLM function calling
决定「下一个该谁说话」，从而把单角色对话循环扩成多 NPC 自动接力。

它不是工具执行器，而是**发言权调度器**：`decide_next_speaker` 把（在场 NPC 列表 + 最近 N 条台词 + 当前发言者提示）
塞进一条 system prompt（`core.rs:125-136`），让 LLM 调 `select_next_speaker(role_id, reason)`；
`role_id=0` 表示交还玩家。MessageGenerator 据此改写 `gs.current_role_id` 让 loop 再跑一轮（`generator.rs:118-149`）。

### 2.6.3 `should_activate` 的完整触发条件

```rust
pub fn should_activate(&self, gs: &GameStatus) -> bool {          // god_agent/core.rs:35-37
    gs.script_status.is_none() && gs.present_role_ids.len() > 1   // :36
}
```

| # | 条件 | 位置 | 说明 |
|---|---|---|---|
| 1 | `gs.script_status.is_none()` | `core.rs:36` | **不在剧本模式**（剧本下完全关闭） |
| 2 | `gs.present_role_ids.len() > 1` | `core.rs:36` | ⚠️ **`present_role_ids` 只装 NPC，玩家不在其中**（`game_status.rs:138-143` 由 `onstage_role` 填真实 role_id；`decide_next_speaker` 也 `.filter(\|&&id\| id != 0)`，`core.rs:144-149`）→ **实际语义是 NPC ≥ 2**。`core.rs:34` 的注释「含玩家」是**误述** |

调用点：`generator.rs:313`（pre-select，仅当 `user_message.is_some()`，`generator.rs:109`）、
`generator.rs:370`（post-select）；提前退出 `consecutive_npc_rounds >= god.config.max_consecutive_npc`（`generator.rs:359`，默认 3）。
`god_agent` 为 `None` 时全部短路（`generator.rs:307-309`、`:354-356`）。

### 2.6.4 工具（function calling）怎么定义与解析

- **定义**：`god_agent/tools.rs:12-33` 的 `select_next_speaker_tool()`。参数：
  `role_id: integer`（必填，`:21-24`）、`reason: string`（必填，`:25-28`），`required: ["role_id","reason"]`（`:30`）。
- **下发**：`let tools = vec![tools::select_next_speaker_tool()];`（`core.rs:169`）
  → `llm.complete_with_tools(&messages, &tools, Some("auto"))`（`core.rs:174-176`）。
- **解析**：`parse_speaker_selection`（`tools.rs:44-55`）→ `parse_tool_args`（`types.rs:68-91`）做容错归一化：
  双编码 JSON 字符串（`:71-75`）、单键包裹 `{"arguments":{…}}` / `{"params":{…}}`（`:77-85`）、非法 JSON → `{}`（`:87-90`）。
  `role_id` 类型容忍：整数 / 浮点 `6.0` / 字符串 `"6"` `" 6 "` `"6.0"`（`parse_role_id`，`tools.rs:58-69`）。
- **执行**：**没有通用 executor**，God Agent 自己内联消费（`core.rs:180-202`）：
  取 `tool_calls.first()` → 解析 → 校验 `result.0 == 0 || gs.present_role_ids.contains(&result.0)`（`:183`）；
  不在场则 warn + Err（`:186-191`）。失败路径：无 `tool_calls`（`:204-215`）、空数组（`:201`）、解析失败（`:194-198`）。
- **调用格式**：原生 OpenAI 风格 `tool_calls[].function.arguments`（JSON 字符串），
  由各 provider 归一化成 `ToolCall { id, type_, function: FunctionCall { name, arguments } }`（`types.rs:13-30`）。
  **不支持**文本内 `<tool_call>` 之类自定义格式。

> ⚠️ **God Agent 的工具 ≠ 主对话的工具**，两套系统完全独立（见下一节）。

### 2.6.5 在管线的哪一步被调用 + 配置来源

| 阶段 | 位置 |
|---|---|
| 用户消息预处理之后 | `handle_user_message`（`generator.rs:103`） |
| 场景检测之后 | `detect_scene_change`（`generator.rs:106`） |
| **pre-select** | `god_agent_pre_select()`（`generator.rs:110`）→ `should_activate`（`:313`）→ `decide_next_speaker`（`:322`） |
| **post-select**（每轮 loop 末尾） | `god_agent_post_select(...)`（`generator.rs:145`）→ `:359/370/379` |
| 角色切换通知 | `emit_character_switch`（`generator.rs:407-416`，调用点 `:344`、`:402`） |

**配置**：`GodAgentConfig { provider_id: Option<String>, max_consecutive_npc: usize, recent_window: usize }`（`config.rs:18-25`），
默认 `None / 3 / 20`（`config.rs:27-35`）；`load(app)`（`config.rs:39-65`）从 **Tauri store**（`settings.json`）读：
`llm.god_agent_provider_id`（`config/keys.rs:15` ← `config.rs:44-46`）、
`god_agent.max_consecutive_npc`（`keys.rs:101` ← `config.rs:48-52`，有 `.max(1)`）、
`god_agent.recent_window`（`keys.rs:102` ← `config.rs:54-58`，有 `.max(5)`）—— **值以字符串存储**（`.as_str().and_then(parse)`）。

LLM 四级 fallback（`resolve_god_agent_provider`，`config.rs:73-112`）：
① `config.provider_id` → ② `assignment.god_agent_provider_id` → ③ `assignment.chat_provider_id` → ④ 第一个可用 provider。
装配在 `lib.rs:482-487` → `InnerAppState.god_agent`（`lib.rs:114`）→ `GeneratorDeps.god_agent`（`api/chat.rs:100`、`api/game.rs:906`）；热切换 `api/settings.rs:278-280`。

### 2.6.6 ⭐ 加一个工具的两条路（**别搞混**）

**路 A：给上帝 Agent 加**（3 处，无注册表/宏/清单）：

1. `god_agent/tools.rs` 新增 `pub fn xxx_tool() -> ToolDefinition`（照 `:12-33`）+ 解析器（照 `:44-55`）。
2. `god_agent/core.rs:169` 的 `vec![tools::select_next_speaker_tool()]` 改成 vec 里加新工具 —— **这是唯一的 tools 装配点**。
3. `god_agent/core.rs:180-202` 的分派逻辑只认 `tool_calls.first()` 且只处理 `select_next_speaker`；
   要支持多工具必须改这段（**纯手写，没有注册表**）。

**路 B（更常用）：给主对话加**（3 处 + 0 处权限）：

1. 新建 `src/ai_service/tools/world_map.rs`，实现 `Tool` trait，抄 `tools/scene.rs:14-56`：
   `fn definition()`（`scene.rs:18-31`）+ `async fn execute()`（`scene.rs:33-55`）；
   取共享状态用 `super::game_status_handle(&app)`（`tools/mod.rs:52-56`）或 `context.require_app()`（`executor.rs:40-44`）。
2. `tools/mod.rs:1-15` 加 `pub mod world_map;`，并在 `use`（照 `:36` 的 `use scene::{SceneList, SceneSwitch};`）。
3. `tools/mod.rs:94` 的 `built_in_registry` 加一行 `registry.register(Arc::new(XxxTool))?;`（照 `:111-113`）。
4. **权限不用手改**：`ToolPermissionConfig::with_default_tools` 用全量工具名自动建默认组（`permissions.rs:351-410`）；
   `scene_mapping` 里 `UserChat → "scene_admin"` 且 `all_tools: true`（`permissions.rs:356`、`:367-374`）→ **新工具对普通聊天自动放行**。
   角色组 `default` 需用户在前端开（`permissions.rs:162`）。
   ⚠️ 若 `data/tool_permissions.toml` 已存在，`load_or_create` 走已存在分支（`permissions.rs:151-154`），**不会自动补新工具名进 `available_tools`**
   （该字段仅展示用，不参与运行时计算）。
5. 调用链自动生效：`stream_with_tool_loop`（`tools/tool_loop.rs:86-87`）。
6. ⚠️ **工具执行默认 2 秒超时**（`tools/executor.rs:89`）—— 地图查网络要自己控时或走缓存。

## 2.7 记忆系统

### 2.7.1 存哪

| 类型 | 位置 |
|---|---|
| **短期（运行时）** | 内存 `GameRole.memory: Vec<LlmMessage>`（`ai_service/types.rs:569`）；每次 `add_line` 由 `MemoryBuilder` 重算 |
| **长期 / 用户画像 / 约定** | DB 表 **`memory_bank`**，列 `id, info(Text, JSON), save_id, role_id`（`db/entities/memory_bank.rs:6-13`；DDL `migration/m20240101_000001_create_tables.rs:209-235`）；`info` 里是 `GameMemoryBank` 的 JSON（`role_manager.rs:597`） |
| **手动笔记** | 文件 `<data_dir>/game_data/notes/<角色名>.json`（`tools/memory.rs:33-41`；`Note { id, content, tags, created_at }` `:24-31`） |
| 对话历史 | 内存 `GameStatus.line_list`（`game_status.rs:21`）+ 存档时落 `line` / `line_perception` |

### 2.7.2 读写函数

`MemoryRepo`：`get_memories`（`memory_repo.rs:29-39`）、`get_latest_memory`（`:41-53`）、`upsert_memory`（`:55-88`）、
`add_memory`（`:14-27`）、`update_memory`（`:91-113`）、`delete_*`（`:116-172`）。
批量：`SaveRepo::upsert_memory_bank`（`save_repo.rs:422-447`）、`get_memory_banks`（`:450-459`）、`delete_memory_banks_by_save`（`:461-468`）。
高层：`persist_memory_banks_to_db`（`role_manager.rs:575-602`）、`load_memory_banks_from_db`（`role_manager.rs:418-487`）。
门面：`AIService::persist_memory_banks`（`service.rs:221-228`）、`restore_memory_banks`（`service.rs:231-238`）。

### 2.7.3 `memory_builder` 做什么

见 §2.5.4 第 ③ 步（`memory_builder.rs:72-273`）。一句话：**把 `line_list` 按「目标角色视角」还原成 `Vec<LlmMessage>`** ——
自己的话合成一条 assistant、别人的话包进 `{...}` 作为上下文、system 只取第一条、tool 消息原样还原。

### 2.7.4 类别 / 标签体系

**只有两套，都没有 category 枚举**：

- **MemoryBank 四段（硬编码）**（`types.rs:242-247`、`persistent_memory_system.rs:19-76`）：

| 段 | 含义 | 注入位置 | 默认上限 |
|---|---|---|---|
| `short_term` | 短期上下文摘要 | user 消息前缀 `【近期回顾】` | 500 |
| `long_term` | 角色经历编年史 | system 尾部 `【长期经历】` | 2000 |
| `user_info` | 用户画像 | system 尾部 `【taの信息】` | 800 |
| `promises` | 待办与约定 | system 尾部 `【重要约定】` | 800 |

- **手动笔记 `tags: Vec<String>`** 自由文本（`tools/memory.rs:29`、`parse_tags` `:127-142`）。

### 2.7.5 ⭐ 地图信息挂到记忆上的扩展点

**推荐走 `system_addendum`，不要新增 MemoryBank 段** —— 四段是硬编码 4 元组，全链路 `[String; 4]`
（`persistent_memory_system.rs:191,203,527-529,554-555`）+ `tokio::join!` 四路（`:487-524`），
加第 5 段要动 6 处且有数组长度硬约束。

具体落点（按优先级）：
1. **`merge_memory_bank_into_context`（`role_manager.rs:610-662`）加第 4 个参数 `map_addendum`**，在 `:617-630` 之后追加到第一条 system（复用 `content.contains` 去重 `:621`）。调用点只有一处：`role_manager.rs:353`。
2. **要随存档持久化** → 写进 `GameStatusSnapshot`（`game_status.rs:267-285`，`#[serde(default)]` 保证向后兼容）。
3. **要模型主动查** → 加 `tools/world_map.rs`（§2.6.6 路 B）。
4. **不要**挂到手动笔记 `notes/*.json`（那是给角色写手记的，语义不符，且按角色名分文件）。

## 2.8 存档系统

### 2.8.1 存档在哪（**重要：不是 JSON 文件，是 SQLite**）

| 内容 | 位置 |
|---|---|
| 存档主体 | DB 表 **`save`**（`db/entities/save.rs:6-17`；DDL `migration/m20240101_000001_create_tables.rs:61-85`） |
| 台词 | DB 表 `line`（`db/entities/line.rs:19-40`）+ `line_perception`（可见性） |
| 记忆 | DB 表 `memory_bank` |
| 截图 | `<data_dir>/screenshots/<save_id>.png`（`api/save.rs:46-53`，`screenshots_dir` 是硬编码字面量 `:47`） |
| 数据库文件 | `<data_dir>/game_database.db`（`db/mod.rs:16-17`，`sqlite:…?mode=rwc`） |

`data_dir` 解析规则（`init/static_copy.rs:26-72`）：
① Android → 应用专属外部存储 `/storage/emulated/0/Android/data/<package>/files`（`:30-40`）
② iOS → 沙盒 `Documents`（`:42-54`）③ 桌面 debug → 项目根 `data/`（`:58-63`）④ 桌面 release → `exe 同级/data`（`:64-71`）。

### 2.8.2 存档结构

**`save` 表**（`db/entities/save.rs:6-17`）：

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | int PK auto | save_id |
| `title` | string(255) | 标题（自动存档为 `"自动存档 YYYY-MM-DD HH:MM:SS"`，`auto_save.rs:14,176`） |
| `status` | text default `"{}"` | **`GameStatusSnapshot` 的 JSON** |
| `create_date` / `update_date` | datetime | — |
| `running_script_id` | int? | → `running_script.id` |
| `last_message_id` | int? | → `line.id`（**链表头**，历史靠它反推） |
| `main_role_id` | int? FK→`role.id` | 主角 |

**`GameStatusSnapshot` 字段清单**（`game_status.rs:267-285`，`rename_all="snake_case"`）：
`present_role_ids: Vec<i32>`、`current_role_id: Option<i32>`、`background: String`、
`background_music: String`（默认 `"none"`）、`background_effect: String`（默认 `"none"`）、
`current_scene_id: Option<String>`、`global_variables: HashMap<String,Value>`、
`completed_scripts: Vec<String>`、`last_dialog_time: Option<String>`（RFC3339）、`scene_awareness_enabled: bool`（默认 true）。

**历史重建**：`SaveRepo::get_line_list`（`save_repo.rs:195-223`）从 `last_message_id` 沿 `parent_line_id` 反向走链再 reverse；
`get_gameline_list`（`:226-271`）再批量取 `line_perception`。

### 2.8.3 存档 / 读档入口命令

| 命令 | 位置 | 内部函数链 |
|---|---|---|
| `list_saves` | `api/save.rs:57-62` | `count_saves`/`list_saves`（`:68/72`）+ 截图存在性检查（`:100-105`） |
| `create_save` | `api/save.rs:124-129` | `create_save`（`:137`）→ `sync_lines`（`:149`）→ `update_save_main_role`（`:156`）→ `to_snapshot`+`update_save_status`（`:162-167`）→ `persist_memory_banks`（`:174`）→ `upsert_running_script`（`:181`） |
| `load_save` | `api/save.rs:199-200` | `get_save_by_id`（`:207`）→ `get_gameline_list`（`:213`）→ `get_role_settings_by_id`（`:224`）→ `import_settings`（`:242`）→ **`restore_memory_banks`（`:248`，必须早于 load_lines）** → `load_lines`（`:254`）→ `apply_snapshot`（`:260`）→ `build_web_init_data`（`:268`） |
| `update_save` | `api/save.rs:271-276` | 同 create 的 2-6 步（`:296-330`） |
| `delete_save` | `api/save.rs:335-336` | 删 memory_bank（`:343`）→ 删 running_script（`:350`）→ 删截图（`:359`）→ 删 save（`:363`，级联 `save_repo.rs:156-187`） |
| `update_save_title` | `api/save.rs:379-380` | `SaveRepo::update_save_title`（`:383`） |
| `save_screenshot` | `api/save.rs:389-390` | `save_screenshot_file`（`:46`） |
| `capture_main_window_screenshot` | `api/save.rs:399-400`(Windows) / `:429-430`(其他平台返回 Err) | GDI HWND 截图 |

### 2.8.4 版本号 / 迁移

- **SQL 迁移**：`migration/mod.rs:6-17` 注册 7 个：`m20240101_000001_create_tables`（建 6 表）、
  `m20260727_000002_add_line_tool_call`、`m20260729_000002_add_line_thinking`、
  `m20260803_000001_create_skill_agent_tables`、`m20260807_000001_add_skill_agent_reasoning`、
  `m20260814_000001_add_skill_agent_token_usage`、`m20260815_000001_add_skill_agent_cached_tokens`。
- **MemoryBank JSON 版本**：`GameMemoryBank.schema_version: u32`，默认 1（`types.rs:262,270-272`）。
  ⚠️ **未找到任何按 `schema_version` 分支的迁移逻辑** —— 疑似仅占位（**未确认**）。
- **`save.status` 快照**：**无版本字段**，靠 `#[serde(default)]` 字段级向后兼容（`game_status.rs:270-284`）；
  反序列化失败整体 `unwrap_or_default()` 静默降级（`api/save.rs:259`）。
- 资源侧有版本：`DataManifest.data_version`（`manifest/mod.rs:38`）。

### 2.8.5 ⭐「地图存档」加在哪

**首选：`GameStatusSnapshot` 加字段**（`game_status.rs:267-285`）：
加 `#[serde(default)] pub world_map: Option<WorldMapSnapshot>,`，
同步改 `to_snapshot`（`:229-242`）与 `apply_snapshot`（`:245-261`）—— **这是唯一的读写对**。
好处：`save.status` 是 TEXT 列存 JSON，**不需要新迁移**；旧存档读入得到 `None`，天然兼容。

**若数据量大（>几十 KB）或需独立查询**：新增迁移文件（照 `migration/m20260727_000002_add_line_tool_call.rs` 35 行模板）
+ 在 `migration/mod.rs:8-16` 的 vec 末尾注册。
⚠️ **新表必须同步登记进 `lan_sync/db_sync.rs:16,30,39` 的导出表清单与导出顺序（外键序），
以及 `lan_sync/messages.rs:130-142` 的 `DbRecords`**，否则局域网同步会静默丢这份数据
（现有 7 张表都在里面：`roles/saves/runningScripts/adventureUnlocks/lines/memoryBanks/linePerceptions`）。

**第三条路（最省事，推荐与首选组合）**：**落文件**到 `data_dir/world_map/`（与 `game_data/` 平级）。
【我】已有先例：【我】`src-tauri/src/world_map/mod.rs:401` 写 `data_dir/world_map/events.json`、`:54-56` 的 `cache_dir()` = `data_dir/world_map/geo`。
好处：**零迁移、容量无上限、和地图缓存同目录**，且**不用改 `data_manifest.json` 与 `resource_sync`**
（manifest 只登记 `game_data/**` 默认资源，见 §2.10.3）。
代价：**与存档生命周期不绑定** —— 读档要自己切文件、删档要自己清文件，
所以要在 `delete_save`（`api/save.rs:335-377`）和 `load_save`（`api/save.rs:199-269`）各补一处；
另外**这类文件不会被 LAN 同步搬走**（同步按 manifest 走）。
→ **建议组合**：快照存「当前位置 + 当前区块指针」（轻量、随存档走），文件存「已探索集合 / 模拟快照」（重数据）。

**若需地图自身历史/事件流**：不要塞进 save，走【我】已有的独立事件存储
（`world_map_push_events`，【我】`src-tauri/src/world_map/mod.rs:397`），存档只存「当前指针」。

## 2.9 事件机制

### 2.9.1 后端 emit 调用点（按事件名分组）

**AI 对话核心**

| 事件名 | emit 位置 | payload |
|---|---|---|
| `ai:reply` | `message_system/generator.rs:474` | `ReplyResponse`（`message_system/responses.rs:41-69`，camelCase：`type, duration, isFinal, character, roleId, emotion, originalTag, message, ttsText, motionText, audioFile, originalMessage, displayName, displaySubtitle, userMessageSeq?, thinking, previewGen?`） |
| `ai:thinking` | `message_system/events.rs:23` | `ThinkingResponse{type,isThinking,duration}`（`responses.rs:101-106`） |
| `ai:thinking_progress` | `message_system/events.rs:31`（调用点 `producer.rs:186`） | `ThinkingProgressResponse{type,thinkingLength}`（`responses.rs:120-125`） |
| `ai:error` | `message_system/events.rs:56` | `ErrorResponse{type,error_code,detail}`（`responses.rs:162-168`） |
| `status:reset` | `message_system/events.rs:58` | `StatusResetResponse{type,status}`（`responses.rs:182-186`） |
| `ai:tool_activity` | `tools/tool_loop.rs:368` | `{call_id, tool, phase, ok, arguments(≤1000字)}`（`:361-367`） |
| `ai:tool_call_progress` | `tools/tool_loop.rs:379` | `{tool, chars}`（`:375-378`） |
| `ai:tool_call_progress_end` | `tools/tool_loop.rs:386` | `()` |
| `ai:tool_call` | `tools/tool_loop.rs:438` | `{call_id, tool, ok, summary, error, arguments(≤1000), result(≤1000)}`（`:429-437`） |
| `character:switch` | 3 处：`generator.rs:413`、`tools/character.rs:176`、`api/game.rs:780` | `{type:"character_switch", roleId, characterName}` |
| `scene:switch` | `tools/scene.rs:139` | `{type:"scene_switch", scene:{…}}`（`:128-138`） |

**TTS**：`tts:cleanup`←`events.rs:42`；`tts://status-changed`←`api/chat.rs:384`；
`tts://install-complete`←`tts/local/mod.rs:377,564`；`tts://download-complete`←`tts/local/mod.rs:436`；
`tts://engine-ready`←`tts/local/setup.rs:117`；`tts://download-progress`←`tts/local/download.rs:45`（payload **未确认**）。

**ASR**：`asr://stream_partial`←`api/asr.rs:204`、`asr/session.rs:190`；`asr://vad_ready`←`init/mod.rs:226`；
**VAD 四事件**（`asr://speech_started` / `silence_started` / `turn_candidate` / `turn_sealed`）由 `asr/vad.rs:234` 一处 emit，
事件名映射表在 `asr/vad.rs:228-233`，payload = `VadEvent`（`asr/vad.rs:31`）。

**剧本**：事件名常量集中在 `script_engine/responses.rs:14-27`（14 个），emit 点**全部**走共享 helper
`crate::ai_service::message_system::events::emit`（`events.rs:15-18`，一行 `app.emit` + `anyhow` 包装）：

| 事件名 | 常量行 | emit 点 |
|---|---|---|
| `script:narration` | `responses.rs:14` | `script_engine/events/narration_event.rs:57` |
| `script:player` | `:15` | `events/player_event.rs:51` |
| `script:chapter-change` | `:16` | `script_engine/chapter.rs:55` |
| `script:background` | `:17` | `events/background_event.rs:67` |
| `script:background-effect` | `:18` | `events/background_effect_event.rs:77` |
| `script:music` | `:19` | `events/music_event.rs:64` |
| `script:sound` | `:20` | `events/sound_event.rs:59` |
| `script:ambient` | `:21` | `events/ambient_event.rs:61` 和 `:90` |
| `script:present-pic` | `:22` | `events/present_pic_event.rs:64` |
| `script:modify-character` | `:23` | `events/modify_character_event.rs:151` |
| `script:input` | `:24` | `events/input_event.rs:51`、`events/free_dialogue_event.rs:179` |
| `script:choice` | `:25` | `events/choice_event.rs:101` |
| `script:end` | `:26` | `script_engine/script_manager.rs:522` |
| `script:free-dialogue` | `:27` | `events/free_dialogue_event.rs:111`（start）和 `:240`（end） |

前端 listen：`src/api/tauri-events.ts:346-404`（13 个，全部 `eventQueue.addEvent(asEvent(...))`）。

**存档**：`save:auto-saved`←`auto_save.rs:179`（`AutoSaveEventPayload{save_id,title,timestamp}`，`:19-24`）；`app:close-ready`←`auto_save.rs:97`。

**成就 / 冒险**：`achievement:unlocked`←`achievements/mod.rs:24`、`api/chat.rs:126`、`api/adventure.rs:381`、`api/script.rs`（**行号未确认**）、`script_engine/events/achievement_event.rs:95,122`；
`adventure:unlocked`←`api/chat.rs:156`、`api/adventure.rs:291,409`；`adventure:completed`←`api/adventure.rs:387`。

**角色 / 资源**：`role:list-updated`←`api/character.rs:744`、`api/plugins.rs:240`、`role_archive/mod.rs:130,285,400`；
`role:import-started`←`role_archive/mod.rs:98,213`；`role:import-progress`←`role_archive/import_pipeline.rs:75`；
`role:export-progress`←`role_archive/export_pipeline.rs:70`；`plugin:import-started`←`api/plugins.rs:118`；`plugin:import-progress`←`plugins/importer.rs:97`。

**截图**：`screenshot:request-source`←`api/screenshot.rs:20`；`screenshot:captured`←`:94`；`screenshot:cancelled`←`:109`。

**投屏 / 桌宠**：`cast-window:state`←`cast/mod.rs:155,158`；`cast:mirror`←`cast/mod.rs:408`；`cast:mic:recognized`←`cast/server.rs:333`；
`cast:config`（后端 emit 未确认）；`pet:cursor`←`lib.rs:633`；`pet-*-changed` / `request-dialog-history`（后端 emit 未确认）。

**局域网**：`lan-sync-peers-updated`←`lan_sync/mod.rs:289`；`lan-sync-plan`←`:307,330`；`lan-sync-complete`←`:370,429`；`lan-sync-progress`←`lan_sync/sync_engine.rs:478`。

**日志 / 审批**：`log:entry`←`utils/log_bridge.rs:176`（payload `LogEntry` `:86`）；`log-window:state`←`utils/log_bridge.rs:60,63`（payload `bool`）；
`chat:command_approval` / `chat:command_delete_approval` / `chat:file_change_approval` / `chat:file_delete_approval`
—— 实际 emit 点 `tools/skill_files.rs:789 / 773 / 291 / 445`，**统一出口**是 `tools/skill_files.rs:99` 的 `app.emit_to("main", event, payload)`（事件名由 `event` 参数传入，`:88-104`）。

> ⚠️ `tools/skill_files.rs:99` 是**全仓库唯一的 `emit_to`**（定向主窗口），其余全是全局 `emit`。
> **`emit_filter` 全库 0 处。** 后端 emit 总计 **68 处**。
> **事件名常量只有一处集中表**：`message_system/responses.rs:12-33` 的 `event_names`（10 个 `ai:*`/`tts:*`/`status:*`）；
> **其余 50+ 事件名都是裸字符串字面量**，散在各 emit 点。前端**没有任何事件名常量表**。

### 2.9.2 前端 listen 调用点

两个集中注册入口：
- `initializeTauriEventListeners()` —— **主窗口**，`src/api/tauri-events.ts:58`
- `initializeCastWindowListeners()` —— **投屏窗口**，`src/api/tauri-events.ts:452`

| 事件 | 监听位置 | 处理逻辑 |
|---|---|---|
| `ai:reply` | `tauri-events.ts:62` | 过期试玩丢弃（`isStalePreviewReply` `:51-56`）→ `eventQueue.addEvent({type:"reply",duration:-1})`（`:67`） |
| `ai:thinking` | `:70` | 入队 `type:"thinking"` |
| `ai:thinking_progress` | `:75` | `gameStore.thinkingLength = payload.thinkingLength` |
| `ai:error` | `:84` | `interruptToolActivities()` + 入队 error |
| `ai:tool_activity` / `_progress` / `_progress_end` / `ai:tool_call` | `:97 / :103 / :108 / :113` | 顶栏工具状态 / 记录 + toast |
| `chat:*_approval`（4 个） | `:149 / :174 / :202 / :227` | 弹确认框 → `invoke("resolve_*_approval")` |
| `status:reset` | `:247` | 入队 `status_reset` |
| `tts:cleanup` | `:252` | 存 localStorage `lingchat:last_tts_cleanup` |
| `asr://speech_started` / `turn_candidate` / `turn_sealed` | `:281 / :284 / :287` | 转发 `useAsrStore()` |
| `asr://vad_ready` | `:291` | `setVadLoaded(true)` |
| `adventure:unlocked` / `completed` | `:297 / :306` | 通知 / 标记完成 |
| `save:auto-saved` | `:317` | 截屏 → `invoke("save_screenshot")` + 通知 |
| `script:*`（13 个） | `:346-404` | 全部 `eventQueue.addEvent(asEvent(...))` |
| `character:switch` | `:408`（主）/ `:453`（投屏） | 加载角色 → `currentInteractRoleId` → 改 `presentRoleIds` → 更新标题 |
| `scene:switch` | `:428` / `:466` | `setCurrentScene` + `setCurrentBackground` |
| `cast:mic:recognized` | `:477` | `dispatchEvent('asr-send')` |
| `app:close-ready` | `src/App.vue:373` | 关窗收尾（`saveCompleted=true` → `tryExit()`） |
| 其它 | `tts-local.ts:20`、`Live2DStage.vue:613`、`GameDialog.vue:815/822`、`useScreenshot.ts:16/24`、`useImageSourcePicker.ts:40`、`LogConsole.vue:281/297`、`SettingsCast.vue:509`、`SettingsTts.vue:1303/1306/1310`、`CastWindow.vue:322/345`、`PetMode.vue:126/134/144/152,283/287`、`useAsrInput.ts:677`、`useArchiveImport.ts:91/103/112`、`useLanSync.ts:44/52/57/63`、`achievement.ts:108` | — |

**清理**：`App.vue:403-427` 的 `onUnmounted` 会调 `unlistenCloseReady()` / `unlistenCloseRequested()`（`:425-426`）；
其余 `listen` 的 unlisten 清理**未逐个核对（未确认）**。

### 2.9.3 ⭐ 加「地图事件」要动哪些地方

**最小改动 3 步，零注册表**：

1. **后端 emit**：在【我】`src-tauri/src/world_map/` 里选一处（如 `world_map_push_events`，【我】`world_map/mod.rs:397`）或新增，
   调 `app.emit("world:map-changed", &payload)`。模板：`tools/scene.rs:139`（一行 emit + warn）。
   要定向主窗口用 `app.emit_to("main", ...)`，模板 `tools/skill_files.rs:99`。
2. **前端 listen**：在 `src/api/tauri-events.ts:58` 的 `initializeTauriEventListeners()` 内加 `listen("world:map-changed", (event) => {...})`。
   - 驱动气泡/演出动画 → 走 `eventQueue.addEvent(asEvent(payload, {type:"...", defaultDuration:...}))`（照 `:346-404` 的剧本事件），
     但要同时扩展 **`ScriptEventType` 联合类型 —— 它在 `src/types/script.ts:141`**（同文件 `:1` 起是 `ScriptEvent` 与 20 个子类型）。
   - 只是 UI 状态 → 直接调 store setter（照 `scene:switch` `:428-435`）。
   - ⚠️ **`initializeTauriEventListeners()` 的 40+ 个监听全部不保存 unlisten（终生常驻）**。
     若地图事件需要可清理（例如只在地图页面期间有效），**应该走组件内注册**：
     在 `WorldMapLayer.vue` 的 `onMounted` 里自己 `listen` 并存下 unlisten，`onBeforeUnmount` 调它，
     参考 `Live2DStage.vue:613-621` 的 `pet:cursor` 与 `useScreenshot.ts:31-43` 的写法。
3. **（可选）事件名常量**：前端没有集中事件名常量文件；后端可加进 `message_system/responses.rs:12` 的 `event_names`（该 mod 目前只放 AI 系，**非强制**）。
   命名惯例是 `域:动作` / `域-子域:动作` 小写短横线（`save:auto-saved`、`cast-window:state`、`asr://stream_partial` 三种风格并存）。

> ⚠️ **官方 0.5.1 里世界地图没有任何先例，地图事件全部要在【我】新加。**
> 另一条路（【我】现在用的）：**完全不用事件**，流式用 `tauri::ipc::Channel` 当作 `invoke` 的实参
> （【我】`src-tauri/src/world_map/bridge.rs:296` `on_event: Channel<Value>`；前端 `src/api/services/worldMap.ts:8,707`）。
> 好处：**不占全局事件命名空间，不可能和既有 `listen` 互扰**。

## 2.10 数据目录

### 2.10.1 `game_data_dir()`

```rust
pub(crate) fn data_dir()      -> PathBuf { crate::init::static_copy::get_data_dir().clone() }  // src/api/mod.rs:54-56
pub(crate) fn game_data_dir() -> PathBuf { data_dir().join("game_data") }                       // src/api/mod.rs:58-60
```
- `get_data_dir()` / `init_data_dir()` — `init/static_copy.rs:13-17` / `:7-10`（`OnceLock`，**只能初始化一次**）。
- 返回值规则 — `init/static_copy.rs:26-72`（见 §2.8.1）。
- 其他路径辅助：`characters_dir()` `api/mod.rs:62-64`、`backgrounds_dir()` `:99-101`、`music_dir()` `:103-105`、
  `ambient_dir()` `:107-109`、`voice_dir()` `:111-113`、`fonts_dir()` `:115-117`。
- 角色路径解析：`resolve_character_path` — `utils/path.rs:8-26`（相对路径 → `<data_dir>/game_data/characters/<path>`；
  `plugin:<id>/<folder>` → `<data_dir>/plugins/<id>/characters/<folder>`）。

### 2.10.2 `game_data_dir()` 下面有什么

| 子项 | 内容 | 命名规则 | 真实例子 |
|---|---|---|---|
| `characters/` | 角色资源包 | 一角色一目录 | `data/game_data/characters/诺一钦灵/settings.yml`、`.../ai模式钦灵.txt`、`.../avatar/{正常,高兴,…}.webp`、`.../avatar/泳装/`。另有 `DeepSeek/`、`风雪/` |
| `scripts/` | 剧本 | `scripts/character/<角色名>/<剧本名>/` | `data/game_data/scripts/character/诺一钦灵/小狼的爱好/` |
| `backgrounds/` | 背景图 | — | 目录存在 |
| `musics/` | BGM | `<名>.mp3` | `夜晚音效.mp3` |
| `ambients/` | 环境音 | 代码里定义（`api/mod.rs:107-109`）；**仓库快照里不存在** | — |
| `skills/` | 技能（Skill Agent） | 一技能一目录，含 `SKILL.md` | `file-operations/`、`lingchat-script-editor/`、`my-first-skill/` |
| `scenes.json` | 场景定义（**单文件**） | — | `scene_store.rs:244` |
| `notes/` | 手动笔记（**运行时创建**） | `<角色名>.json` | `tools/memory.rs:33-41`；本机**尚不存在** |

**`data_dir` 根下还有**：`plugins/<id>/characters/<folder>/`、`third_party/emotion_model_19emo/`（情绪 ONNX 模型，
`emotion/classifier.rs:78-92`；**本机该目录为空 → 分类器在本机跑不起来**）、`third_party/asr_vad/`、
`voice/`（TTS 产物，`role_manager.rs:804`）、`fonts/`、`screenshots/<save_id>.png`、
`data_manifest.json`、`.seeded`（首次播种标记，`init/static_copy.rs:80`）、`.official/`（安装包自带默认资源，播种后删，`:114-126`）、
`tool_permissions.toml`（`tools/permissions.rs:27`、`tools/mod.rs:152`）、`game_database.db`（`db/mod.rs:16-17`）、
以及【我】特有的 `world_map/geo/`（【我】`world_map/mod.rs:47-49`）。

**移动端播种**：`seed_data_dir`（`init/static_copy.rs:78-101`）→ 移动端从 APK 解 `data.7z`
（`seed_via_fs_plugin` `:147-180`，路径 `{resource_dir}/data/data.7z` `:166`）；桌面端从 `data/.official/` 复制（`seed_desktop` `:108-131`）。

### 2.10.3 manifest / 资源同步

- 类型：`DataManifest { data_version: u64, files: HashMap<String, FileEntry> }`、`FileEntry { sha256, size, modified_at }`（`manifest/mod.rs:19-41`）。
- **真实文件长什么样**（`data_manifest.json`，本机唯一一份真货在【我】`data/data_manifest.json`，18144 B）：
  顶层 `{"data_version": 1, "files": {...}}`，**104 个条目**；每条目 `{"sha256":"<hex64>","size":<int>}`（旧版可能带 `modified_at`，`manifest/mod.rs:25-28`）；
  key = 相对 `data/` 的 `/` 分隔路径，前缀分布：`game_data/characters` 87 条、`game_data/skills` 13 条、`game_data/backgrounds` 3 条、`game_data/musics` 1 条
  → **manifest 只登记 `game_data/**` 的默认资源**，不登记运行时产物。
- 生成脚本：`scripts/generate-data-manifest.js:24`（输出）、`:114`（`data_version`）、`:122`（`files[subPath]`）。
- 差异比较：`DataManifest::diff`（`manifest/mod.rs:60-87`）。
- 读写：`load`（`:90-93`）/ `save`（原子 `.tmp`→rename，`:96-108`）；`compute_sha256`（`:115-120`）。
- 命令：`check_resource_sync()`（`resource_sync/mod.rs:73-74`，比对 `data/.official/data_manifest.json` vs `data/data_manifest.json`，`:76-77`）、
  `apply_resource_sync(...)`（`:182`）、`get_data_version()`（`:216`）；状态锁 `ResourceSyncState`（`:28-31`）。
- `lan_sync`（局域网全量同步）也复用 `DataManifest`（`manifest/mod.rs:6` 注释）。

### 2.10.4 ⭐ 新建一个 `world_map/` 数据子目录

**最自然的落点**：【我】已经这么做了 —— `world_map/geo/` 走 `cache_dir()`
（【我】`src-tauri/src/world_map/mod.rs:47`），根就是 `data_dir`，**不需要改 `manifest` 或资源同步逻辑**
（manifest 只管 `game_data` + `third_party`，由 `scripts/prepare-desktop-resources.mjs` 用 `git ls-files data/` 生成）。

要放进打包资源（进 APK/安装包）才需要动：`scripts/prepare-desktop-resources.mjs` + `src-tauri/tauri.conf.json:52-56` 的 `resources`。

---

# 三、构建与工程

## 3.1 `package.json` scripts 全解

脚本段：官方 `package.json:8-24`；【我】`package.json:7-14`。

| script | 官方行 | 【我】行 | 命令 / 作用 / 坑 |
|---|---|---|---|
| `init` | 官:8 | 我:7 | `tauri icon … && node scripts/prepare-desktop-resources.mjs && node scripts/download_emotion_model.mjs`。首次/换图标时跑。坑：① 需联网从 modelscope 下 7 个模型文件（`scripts/download_emotion_model.mjs:11-22`），落 `data/third_party/emotion_model_19emo`（`:8`）；② 已判 `process.stdin.isTTY`（`:139,148-160`），CI 非 TTY 不会挂死；③ 会重写 `src-tauri/icons/*`；④ **CI 里也跑它**（`.github/workflows/build-android.yml:90-93`） |
| `dev` | 官:9 | 我:8 | `vite`。纯前端热更（无 Tauri 壳）。坑：端口写死 **1420 + `strictPort`**（`vite.config.ts:25-26`），被占用直接失败 |
| `build` | 官:10 | 我:9 | `vue-tsc --noEmit --skipLibCheck && vite build` → `dist/`。**这是手机上唯一允许跑的构建（约 3 分钟）**。坑：`--skipLibCheck` 掩盖 .d.ts 冲突 |
| `preview` | 官:11 | 我:10 | `vite preview`。坑：`vite.config.ts` **没有 `preview` 段**，端口不受 1420 约束 → 别当稳定服务地址 |
| `tauri` | 官:12 | 我:11 | 官方是 `node scripts/tauri-cli.mjs` **包装器**：`dev` 子命令会**先跑 `pnpm format`**（`scripts/tauri-cli.mjs:17-26`），其它透传（`:28-36`）；【我】是裸 `tauri` |
| `android:prepare` | 官:13 | 我:12 | `tauri android init && node scripts/generate-android-icons.mjs`。坑：`init` 会**重建 `gen/android`，可能覆盖手改**；【我】`build-debug.yml:58-61` 就是先 `rm -rf src-tauri/gen/android` 再 prepare |
| `android:dev` | 官:14 | 我:13 | `tauri android dev`。需 adb+SDK+NDK，**手机不可用** |
| `android:build` | 官:15 | 我:14 | `node scripts/prepare-bundled-resources.mjs 9 && tauri android build --target aarch64 --apk`。坑：① 等级 9 = LZMA2 最慢压缩（`scripts/prepare-bundled-resources.mjs:26-33`）；② **`data/` 被压成 `data.7z` 塞进安卓 assets**（`:38-39,93-113`），**移动端不走 `tauri.conf.json` 的 `bundle.resources`** |
| `android:check` | 官:16 | 缺 | `cargo ndk check --target aarch64-linux-android`，只做类型检查 |
| `android:devbuild` | 官:17 | 缺 | `prepare-bundled-resources.mjs 5 && tauri android build -d -t aarch64 --apk` |
| `ios:init` / `ios:build` | 官:18-19 | 缺 | macOS 专用 |
| `format` / `format:rs` / `format:frontend` / `format:check` | 官:20-23 | 缺 | `prettier --write . && cargo fmt` 等。坑：**全仓重写**；`tauri dev` 会自动触发 |
| `check:rs` | 官:24 | 缺 | `cargo check --manifest-path src-tauri/Cargo.toml` |

其它差异：官方 `package.json:5` 有 `packageManager: pnpm@11.21.0`，【我】没有 → 【我】pnpm 版本不锁（CI 用 `pnpm/action-setup@v4 version: latest`，【我】`.github/workflows/build-android.yml:37-39`）。

**与世界地图 / HTTP / 渲染 / i18n / 状态管理相关的依赖**：

| 包 | 版本 | 用途 |
|---|---|---|
| `pinia` | ^3.0.4（官:41 / 我:31） | 状态管理（`src/stores/index.ts` `createPinia()`） |
| `vue-i18n` | ^10.0.8（官:47 / 我:35） | i18n |
| `opencc-js` | ^1.4.1（官:40 / 我:30） | 简→繁（`scripts/generate-zh-hk.mjs:19-20`） |
| `@tauri-apps/api` | ^2（官:29 / 我:19） | `invoke` / `Channel`（地图走它） |
| `axios` | ^1.16.0（官:36 / 我:26） | HTTP。**世界地图前端不用它**（走 `invoke`/`fetch`） |
| `pixi.js` 8.15.0 + `untitled-pixi-live2d-engine` 1.3.5 | 官:42,45 | Live2D（【我】无） |
| `7zip-min` ^3.0.1（官:27 / 我:17） | — | 打 `data.7z` |
| **地图渲染库** | — | **两仓库都没有 leaflet/maplibre/d3/three/geojson** —— 地图是自绘 Canvas/SVG（【我】`src/components/views/worldmap/districtDraw.ts`）+ Rust 几何 |

**`vite.config.ts`**（官方 46 行 /【我】48 行，差异仅引号与换行）：
`plugins:[vue(), VueDevTools(), tailwindcss()]`(`:11`)、`resolve.alias @ → ./src`(`:15`)、`clearScreen:false`(`:22`)、
`server:{port:1420, strictPort:true, host:TAURI_DEV_HOST, hmr.ws 1421, watch.ignored:[src-tauri/.venv/target/data]}`(`:24-39`)、
`optimizeDeps:{exclude:['src-tauri/*'], entries:['src/*']}`(`:42-45`)。
**没有 `server.proxy`、没有 `build` 段**（输出目录全默认 `dist/`，与 `tauri.conf.json:10` 对齐）。

## 3.2 Tauri 配置

### 3.2.1 `src-tauri/tauri.conf.json`（官方 58 行 /【我】66 行）

| 字段 | 官方行 | 【我】行 | 说明 / 差异 |
|---|---|---|---|
| `productName` | 3 | 3 | `LingChat` vs **`SYuki`** |
| `version` | 4 | 4 | `0.5.1` vs `0.1.3` |
| `identifier` | 5 | 5 | `com.noiq.ling-chat` vs **`com.syuki.lingchat`** |
| `build.beforeDevCommand` | 7 | 7 | `pnpm dev` |
| `build.devUrl` | 8 | 8 | `http://localhost:1420` |
| **`build.beforeBuildCommand`** | 9 | 9 | `node scripts/prepare-desktop-resources.mjs && pnpm build`。⚠️ **cargo 不会执行它**；CI 里单独跑（`.github/workflows/world-map-check.yml:98-99`） |
| `build.frontendDist` | 10 | 10 | `../dist` |
| `app.macOSPrivateApi` | 13 | **无** | 【我】缺它 → macOS 透明窗口静默失效（`Cargo.toml:27-30` 注释） |
| `app.windows[0]` | 14-22 | 13-21 | **只声明 1 个窗口**：`title:"LingChat"`(:16)、1500×800(:17-18)、`transparent:true`(:19)、`shadow:false`(:20)。其余窗口运行时创建（见 §1.5） |
| `app.security.csp` | 24 | 23 | `null`（**无 CSP 限制**） |
| `app.security.assetProtocol` | 25-28 | 24-29 | `enable:true` + `scope:["**"]`（**全盘可读**，偏宽） |
| `plugins.updater` | 32-38 | 33-41 | endpoints 指向**官方仓库** `SlimeBoyOwO/LingChat/releases/...`（官:33 / 我:34-35）—— 【我】的更新器仍指向官方渠道 |
| `bundle.active` / `targets` / `icon` / `createUpdaterArtifacts` | 41/42/43-50/51 | 44/45-50/51-58/59 | targets = nsis/dmg/deb/appimage（**无 android/ios**） |
| **`bundle.resources`** | 52-56 | 60-64 | 见下 |

**`resources` 三组映射**（官:52-56 / 我:60-64）：
`".bundled_resources/game_data/" → "data/.official/game_data/"`、`".bundled_resources/third_party/" → "data/third_party/"`、
`".bundled_resources/data_manifest.json" → "data/.official/data_manifest.json"`。

**和 `data/` 的关系**：源目录 `src-tauri/.bundled_resources/` **不是手写的**，由 `scripts/prepare-desktop-resources.mjs`
用 `git ls-files data/` 生成（`:4-5,56`；third_party 例外，直接拷 `:79-89,136-148`；manifest 生成 `:192-199`）。
staging 目录本身被忽略（`.gitignore:80` /【我】`.gitignore:69`）。**移动端不走这条路**（走 `data.7z`）。

### 3.2.2 `tauri.android.conf.json`（Android 覆盖层）

| 文件 | 内容 |
|---|---|
| 官方 `src-tauri/tauri.android.conf.json:1-3` | 只覆盖 `identifier: com.noiq.lingchat` |
| 【我】`…:1-4` | `identifier: com.syuki.lingchat` + **`version: 0.1.3`**（多覆盖了 version → 【我】Android 版本号被这个文件钉死） |

### 3.2.3 `Cargo.toml` 与 `build.rs`

- **`[features]` 只有一个**：`custom-protocol = ["tauri/custom-protocol"]`（官:18-21 /【我】:18-21），移动端必须启用。
- `tauri` features：官方 `["macos-private-api","protocol-asset"]`（:31），【我】`["protocol-asset"]`（:27）。
- `reqwest`：官方 features **含 `form`**（`:60-68`），【我】**没有**（:47）—— 这正是【我】踩过的坑（`.form()` 需要 feature）。
- `rustpython-*`：官方**所有平台（含 Android）都编**（:109-117）；【我】放在 `cfg(not(android/ios))`（我:92-98）→ **【我】的 Android 包没有插件系统**。
- `md5`：**只有【我】有**（我:81），用于世界地图模块的稳定随机种子。
- `[patch.crates-io]`：esaxx-rs + jpreprocess（官:177-179 / 我:121-123）。
- **profile 不在 `Cargo.toml`**（两边都没有 `[profile.*]`），而在 `src-tauri/.cargo/config.toml`：
  `[profile.dev] debug=1, strip="debuginfo", lto=false, opt-level=0`（`:1-6`）；
  `[profile.release] lto="thin", codegen-units=16, panic="abort", strip=true, opt-level="z"`（`:8-13`）；
  `[target.aarch64-linux-android] rustflags = -Wl,--allow-shlib-undefined, --unresolved-symbols=ignore-all`（`:15-21`）。
  ⚠️ **【我】的 `feat/world-map` 分支没有这个文件**（`git ls-tree feat/world-map -- src-tauri/.cargo/` 为空，
  且 `**/.cargo/**` 被 `.gitignore:89` 忽略）→ 后果：release 无 lto/panic=abort/opt-level=z、Android 无那两个 rustflags，
  优化只能靠 CI 环境变量（【我】`.github/workflows/build-android.yml:28-30`、`world-map-check.yml:69-70,89-90,135-136`）。
- `build.rs`（官方 27 行 /【我】3 行）：官方额外做两件事 ——
  Android 下把 `gen/android/app/src/main/jniLibs/arm64-v8a/libffi.a` 用 `cargo:rustc-link-arg` 链进去（`:9-11`，rustpython ctypes）；
  Windows 下声明 comctl32 v6 manifest（`:22-26`）。用 `CARGO_CFG_TARGET_OS` 而非 `#[cfg]`（`:18-21` 有解释）。【我】只有 `tauri_build::build()`（`:1-3`）。

## 3.3 权限 / 能力（capabilities）

### 3.3.1 `capabilities/default.json`

| 项 | 官方行 | 【我】行 | 含义 |
|---|---|---|---|
| `windows` | 5 | 5 | 官方 `["main","settings","log","screenshot-overlay","cast"]`；【我】少 **cast** |
| `permissions` | 6-34 | 6-38 | `core:default`(:7)、`core:webview:allow-create-webview-window`(:8)、一串 `core:window:allow-*`(:9-23)、`opener:default`(:24)、`dialog:allow-open`(:25)、`screenshots:default`(:26)、`fs:default`(:27)、`dialog:default`(:28)、`notification:default`(:29)、`android-fs:allow-get-name`(:30) |
| `fs:scope` | 31-34 | 31-38 | allow `$RESOURCE/**`、`$APPCACHE/**`、`$APPDATA/**` |
| `platforms` | 无 | 无 | default 对**所有平台**生效 |

`capabilities/desktop.json`（两仓库逐字节相同）：`windows:["main","settings"]`(:5)、`platforms:["windows","linux","macOS"]`(:6)、
permissions `updater:default` / `updater:allow-check` / `updater:allow-download-and-install` / `process:allow-restart`(:8-11)。

**自定义命令要不要在这里声明？→ 不需要**（结论与依据见 §2.3 第 4 条）。

### 3.3.2 Android 权限声明

文件：`src-tauri/gen/android/app/src/main/AndroidManifest.xml`（官方 50 行 /【我】48 行）。

| 声明 | 官方行 | 【我】行 | 用途 |
|---|---|---|---|
| `android.permission.INTERNET` | 3 | 3 | 网络（API/更新/地图） |
| `android.permission.RECORD_AUDIO` | 4 | 5 | WebView 麦克风（ASR） |
| `android.permission.MODIFY_AUDIO_SETTINGS` | 9 | 10 | 与 RECORD_AUDIO 捆绑；不声明会被静默拒（官:5-8 注释） |
| `uses-feature leanback (required=false)` | 15 | 13 | AndroidTV |
| `<application android:usesCleartextTraffic="${usesCleartextTraffic}">` | 21 | 19 | 见下 |
| FileProvider + `${applicationId}.fileprovider` | 37-45 | 35-43 | 分享/截图 URI |
| 插件占位注释 `AUTO-GENERATED` | 47,49 | 45,47 | android-fs 注入 |

**全文只有 3 个 `uses-permission`**。⚠️ **没有 `ACCESS_*_LOCATION`、没有 `POST_NOTIFICATIONS`、没有 `READ_MEDIA_*`。**

**网络 / 存储的坑**：

- **cleartext 是编译期占位符**：release 默认 `false`（`gen/android/app/build.gradle.kts:20`），debug 覆写 `true`（`:29`）。
  → **release APK 禁止 `http://` 明文**，而世界地图的 HTTP 兜底是 `http://127.0.0.1:8791`（【我】`src/api/services/worldMap.ts:40`），
  旧 Python 路的 8790 写死在 5 个前端模块里（【我】`public/world_map/world_time.js:21`、`npc.js:144`、`phone.js:35`、`world_weather.js:29`）
  → **release 包里这些 http 调用会被系统拦，只能走 `invoke` 通路**（这正是「世界地图剩余 HTTP 依赖收口」这条待办的技术根因）。
- **没有 `network_security_config`**：两仓库都不存在该文件（只有 `res/xml/file_paths.xml`），manifest 里也无 `android:networkSecurityConfig`。
- **FileProvider 路径过宽**：`res/xml/file_paths.xml:3-4` 把 `external-path` 与 `cache-path` 的 path 都设成 `"."`。
- **Android 定位**：【我】的世界地图定位走 Rust 侧（`world_map/live.rs:164,413`）+ IP 兜底，
  `IP_API_URL` 是**明文 http**（`live.rs:59`），理由与真机验收项写在 `live.rs:51-58`。
  没有加 geolocation 插件、没有 `ACCESS_*_LOCATION` 权限 —— **能否在 release APK 里真通，未确认**。

### 3.3.3 改东西时精确该动哪些文件

| 要改什么 | 动哪些文件 |
|---|---|
| **加一个 Android 权限** | `gen/android/app/src/main/AndroidManifest.xml`（在 `:4` 或 `:9` 旁插 `<uses-permission>`）；需运行时授权还要改 `gen/android/app/src/main/java/<pkg>/lingchat/MainActivity.kt`（`:14-25`）。注意 `tauri android init`（`package.json:13`）会重建 `gen/android` |
| **改包名**（5+ 处） | `src-tauri/tauri.conf.json:5`、`src-tauri/tauri.android.conf.json:2`、`gen/android/app/build.gradle.kts:18`(namespace) 与 `:21`(applicationId)、`gen/android/app/src/main/java/<pkg>/lingchat/MainActivity.kt:1` **及目录名**；`buildSrc/src/main/java/com/<pkg>/kotlin/*.kt` 的**目录名**里也含包名（文件内无 `package` 声明） |
| **改版本号**（4 处） | `package.json:4`、`src-tauri/tauri.conf.json:4`、`src-tauri/Cargo.toml:3`、`src-tauri/tauri.android.conf.json:3`（**只有【我】有这行**）。`versionCode/versionName` 由生成物 `tauri.properties` 提供（`app/build.gradle.kts:24-25`，勿手改） |

### 3.3.4 `gen/android` 哪些能改

| 类别 | 文件 | 依据 |
|---|---|---|
| 纯生成 + 被忽略（**别提交**） | `app/tauri.build.gradle.kts`、`app/tauri.properties`、`app/proguard-tauri.pro`、`app/src/main/assets/tauri.conf.json`、`app/src/main/**/generated`、`app/src/main/jniLibs/**/*.so` | `gen/android/app/.gitignore:1-6` |
| 纯生成但**已提交** | `gradlew(.bat)`、`gradle-wrapper.*`、`buildSrc/*`、`build.gradle.kts`、`settings.gradle` | `git ls-files src-tauri/gen/android` = 28 项（官方）/ 26 项（【我】） |
| **生成后被手改（提交）** | `AndroidManifest.xml`、`MainActivity.kt`（安全区 CSS 注入 + WebView 透明，`:14-55`）、`app/build.gradle.kts`（包名/minSdk/targetSdk）、`res/values/strings.xml:2-3`、`res/values/themes.xml`、`buildSrc/.../RustPlugin.kt:20-46`（ABI/flavor）、`BuildTask.kt:19-21`（调 `pnpm tauri android build`） | 均进 git |

`app/build.gradle.kts`（两仓库除包名外**完全相同**，71 行）：`compileSdk=36`(:17)、`namespace`(:18)、`applicationId`(:21)、
`minSdk=24`(:22)、`targetSdk=36`(:23)、debug 分支(:28-38)、release 分支（`isMinifyEnabled=true` + proguard，:39-46）、
`jvmTarget=1.8`(:48-50)、`rust { rootDirRel="../../../" }`(:56-58)、`apply(from="tauri.build.gradle.kts")`(:71)。
**abiFilters 不在这个文件里**：由 `RustPlugin.kt:20-46` 按 Gradle 属性 `abiList/archList/targetList` 生成 flavor（默认 4 ABI）。
**没有 signingConfig**：签名靠 CI 写 `gen/android/keystore.properties`（被忽略）+ `apksigner` 手签
（官方 `.github/workflows/build-android.yml:108-115,134-148`）。

## 3.4 GitHub Actions

### 3.4.1 官方线 8 个 workflow

| 文件 | 触发 | 职责 → 产物 |
|---|---|---|
| `build-android.yml` | `workflow_dispatch` + 3 个 input（:3-20） | checkout `lfs:true`(:33-36) → node26(:38-41) → rust aarch64(:43-46) → Linux 依赖(:54-83) → java zulu21(:85-88) → `pnpm run init`(:90-93) → `android:prepare`(:95-96) → 写 keystore(:108-115) → `pnpm android:build`(:117-118) → 重命名 APK(:120-132) → apksigner 手签(:134-148) → `gh release create/upload`(:150-174)。**发行 APK → GitHub Release** |
| `dev-build-android.yml` | `workflow_dispatch`(:3,6) | 开发版 APK → artifact(:130) |
| `build-ios.yml` | `workflow_dispatch`(:3-9) | macos-latest，`pnpm ios:build`(:62-68) → 无签名 IPA → artifact(:70-76) |
| `release.yml` | push **被注释掉**(:4-5)；`workflow_dispatch`(:6-21) | `generate-data`(:30-64) + 四平台矩阵 `tauri-action@v1`(:121-135) + `softprops/action-gh-release@v2`(:159-183)。**桌面安装包 + data 包 → Release** |
| `dev-build.yml` | push/PR `dev`,`dev/**`(:4-15) + 手动(:16) | 四平台 `pnpm tauri build --no-bundle`(:100-113)。**只编译验证，无 artifact** |
| `dev-build-full.yml` | 同(:3-16) | 同 + 上传 artifact(:115-120) |
| `opencode-review.yml` | `issue_comment` / `pull_request_review_comment`(:3-7) | 注释含 `/opencode` 或 `/oc` 才跑(:11-13) → AI 审查 |
| `world-map-check.yml` | push `feat/world-map`,`feat/**` + paths 过滤(:8-17)；PR main/master(:18-19)；手动(:20) | **旧版**轻量检查：`frontend`(:27-64) + `rust`(:66-107)，单平台 |

### 3.4.2 【我】7 个 workflow 与官方线的差异

| 差异 | 【我】 | 官方 |
|---|---|---|
| `build-debug.yml` | **有**（`build-debug.yml:1-72`）：push main/master(:4-5) + 手动(:6)；`setup-android@v3`(:43) + `sdkmanager "ndk;28.2.13676358"`(:44-47)；**先 `rm -rf src-tauri/gen/android`**(:58-61)；`pnpm android:build`(:63-64)；APK artifact(:66-72) | 无 |
| `build-ios.yml` / `dev-build-full.yml` | 无 | 有 |
| `dev-build.yml` | 带去重 `if`(:21-25)、`pnpm/action-setup@v4`+node24(:51-58)、pnpm store 缓存(:60-64)、artifact(:122) | 精简版、`pnpm/setup@v1` node26、无 artifact |
| `build-android.yml` | `action-setup@v4`+node24(:37-44)、手动 `git lfs pull`(:46-47)、targets **aarch64 + armv7**(:52)、多一步上传未签名 APK(:145-150) | `pnpm/setup@v1` node26、`lfs:true`、仅 aarch64 |
| `release.yml` | push tags `v*` **生效**(:4-5)；`runs-on: ${{ matrix.platform }}`(**:74** —— 值域是 windows-amd64/macos/…，**不是合法 runner 标签 → 该 job 会直接失败**) | push 被注释(:4-5)；`runs-on: matrix.os`(:84，已修) |
| `world-map-check.yml` | **新版**（`:1-169`）：push `feat/**`,`main`(:21)；3 个 job —— `frontend`(:39-74)、`rust-check` 四平台矩阵(:77-128，`fail-fast:false` :81，`needs:frontend` :79)、`rust-test`(:131-169，`cargo test --lib world_map` :167-168) | 旧版单平台 |

**三者分工**（也是【我】文件头注释 `:3-15` 声明的边界）：

- **`world-map-check.yml`**：只**验证**（vue-tsc + vite build + 多平台 `cargo check --lib` + `cargo test --lib world_map`），**无产物**，几分钟出结果。
- **`build-android.yml`**：手动触发、带签名、建 Release —— 出**我们实际发布的 APK**。
- **`release.yml`**：桌面安装包 + `data_files.zip`。

> ⚠️ 纪律提醒：**手机上绝不跑 Tauri 全量编译**。本地只允许 `npx vue-tsc --noEmit --skipLibCheck && npx vite build`（约 3 分钟）。

---

# 四、我们的插入点（结论性一节）

> 前提：**只新增、不改坏原有**（K53 底线）。
> 本节同时给「官方线」与「我们线」的位置；给官方提 PR 用官方行号，自用改动落在【我】。

## 4.0 我们已经插过的位置（可复用的样板）

**官方线的插入 = 15 行挂载 + 11 个新文件、零删除**（`git diff 348ef00c 615e64c0`，17 文件 7251 插入 0 删除，其中只有 4 个既有文件被改）：

| # | 官方行号 | 插了什么 |
|---|---|---|
| 1 | `src/components/views/menu/page/MainMenuOptions.vue:17`（外层 `StartLine` 16-18） | `<StartItem @click="() => emit('open-world')">世界</StartItem>` |
| 2 | `MainMenuOptions.vue:41` | `(e: "open-world"): void;` |
| 3 | `src/components/views/MainMenu.vue:48` | `@open-world="() => router.push('/world')"` |
| 4 | `src/router/index.ts:18` | `const WorldMap = () => import("../components/views/WorldMap.vue");` |
| 5 | `src/router/index.ts:28-30` | 路由对象 `{path:"/world", name:"WorldMap", component:WorldMap}`（**插在 MainMenu 与 /chat 之间，未动既有路由**） |
| 6 | `src/App.vue:17` | `<WorldMapLayer v-if="isMainWindow" />` |
| 7 | `src/App.vue:31` | `import WorldMapLayer from "./components/views/WorldMapLayer.vue";` |

**【我】的插入更大**：`git diff 662bfb09^ HEAD`（排除 docs）= **33 新增 + 9 改动**，其中真正被动到的既有文件 **8 个**，合计 **85 行插入 / 2 行删除**，
其余 24164 行全在新增文件里（后端 16 个 `.rs` 共 11837 行）：

| 文件 | 增/删 | 性质 |
|---|---|---|
| 【我】`src-tauri/src/lib.rs` | 28 / 0 | 1 行 `mod world_map;`(:16) + 21 条命令注册(:555-581) |
| 【我】`src/router/index.ts` | 33 / 0 | 5 条路由，**纯追加** |
| 【我】`src-tauri/Cargo.toml` | 7 / 0 | 只加 `md5 = "0.8"`(:81) |
| 【我】`src/App.vue` | 4 / 0 | 挂载层(:21) + import(:44) |
| 【我】`src/components/views/MainMenu.vue` | 1 / 0 | 一行事件绑定(:52) |
| 【我】`src/components/views/menu/page/MainMenuOptions.vue` | 4 / 0 | 菜单项(:12-14) + 事件类型(:46) |
| 【我】`src/web-mock.ts` | 5 / 1 | 加 `__LINGCHAT_WEB_MOCK__` 标记（**唯一改了既有代码行**：扩 `interface Window`，`:4`） |
| 【我】`src-tauri/Cargo.lock` | 3 / 1 | 依赖解析结果 |

**刻意没用的方式**（都是「会更侵入」的选项）：不用 Tauri 事件（`emit`/`listen` 在【我】地图代码里零使用）、
不加 `AppState` 字段、不加 capability 条目、不改 `tauri.conf.json` 的 `resources`、
不改 `src/main.ts` / `vite.config.ts` / `package.json` / `src/locales/`、**不碰 `ai_service/`（对话管线 / 上帝 Agent / 存档）**。

→ **结论：地图目前是一个完全孤立的旁路模块。** 第四节的六件事，就是把它从「旁路」接进「主干」的六个接口。

## 4.1 ① 主菜单加 / 改入口

**现状**：官方线把「世界」放在**主菜单一级**（`MainMenuOptions.vue:16-18`）；
`13-世界模拟最终方案.md:13` 定的目标是「主菜单 →『开始游戏』→ 第四个选项」，即应该挪进 **`GameModeOptions.vue`**。

**目标位置**（官方线）：`src/components/views/menu/page/GameModeOptions.vue`
现在 4 个 `StartLine`：自由对话(:3-7) / 剧情模式 disabled(:9-11) / 小游戏 disabled(:13-15) / 返回(:17-19)。
→ **在 `:16` 之前插入**：

```vue
    <StartLine>
      <StartItem class="menu-subitem" @click="() => emit('open-world')">{{
        $t("views.menu.worldSim")
      }}</StartItem>
    </StartLine>
```
并加 `(e: "open-world"): void;`（`GameModeOptions.vue:28-31` 的 `defineEmits` 里），
再在 `src/components/views/MainMenu.vue:57-61` 的 `<GameModeOptions>` 上转发 `@open-world="..."`。

**「只新增不改坏」的做法**：

1. **一处只加一块，不重排**：`StartLine` 是列表项，**追加/插入一个完整块**即可，绝不动现有块。
2. **文案走 i18n**：在 `src/locales/{zh-CN,zh-HK,ja,en}/views.ts` 的 `menu` 段（zh-CN 在 `views.ts:48` 起）各加一个键，
   别学现在官方线的硬编码「世界」（`MainMenuOptions.vue:17`）—— 那是要修的。
3. **要不要从一级菜单删掉「世界」**：删 `MainMenuOptions.vue:16-18` **属于「改坏原有」的边界**。
   保守做法：**留着**，两边都能进（多一个入口不破坏任何功能）；提 PR 时再统一。
4. **`MainMenu.vue` 的 `menuState` 状态机不用动**：`open-world` 直接 `router.push('/world')`（照 `MainMenu.vue:48` 的写法），不进 `menuState` 分支。

## 4.2 ② App.vue 挂全局地图层

**位置**（官方线）：`src/App.vue:17` 已有 `<WorldMapLayer v-if="isMainWindow" />`，
import 在 `:31`。**但 `v-if` 只在主窗口生效，不解决「地图层本身开销」问题。**

**要改的其实是性能，不是位置。** 现在的隐患（真问题，不是风格问题）：

- `WorldMapLayer.vue` 的 `setup()` / `onMounted` **无条件执行**：
  `onMounted` 里 `await bindWorldData()`（`WorldMapLayer.vue:160`）、`loadRegion()`（`:164`）、`loadRoles()`（`:165`），
  并且**总是**注册一个 5 分钟 `setInterval`（`:167-172`）—— 只有 interval 内部才判 `mode !== 'off'`。
  → **即使用户从没开过地图，每次启动都会跑一次角色列表 + 日程拉取。**

**「只新增不改坏」的做法（推荐）**：

1. **把 `WorldMapLayer` 改成懒挂载**：在 `App.vue` 用一个「首次打开地图才变 true」的 ref，例如
   `<WorldMapLayer v-if="isMainWindow && worldLayerEnabled" />`。
   这个 ref 的来源**只新增一个模块级 composable**（照 `src/composables/useWorldMapLayer.ts` 的**模块级单例 + localStorage** 模式，
   那是【我】已经验证过的写法：`state` 是 `ref`，`watch(state, …, {deep:true})` 持久化到 `localStorage`），
   `setMode('overlay'|'corner')` 时置 true。**不动 App.vue 其它任何一行。**
2. **层位选择**：
   - 背景层 → 抄 `WorldMapLayer.vue:181-188`：`position:fixed; inset:0; z-index:1; pointer-events:none`。
   - 角落小窗 → 抄 `:217-231`：`z-index:60`。
   - **永远不要 ≥ 9999**（那是弹窗/通知的档位，`Notification.vue:43` / `AppDialog.vue:150`）。
3. **`isMainWindow` 判定照用**（`App.vue:263`）—— 子窗口（log/cast/pet）不该出现地图层。
4. **别用 `<Teleport to="body">`** 除非有 `#app` 缩放问题：`CursorEffects` 之所以 teleport 是因为 `App.vue:450-460` 给 `#app` 加了 `transform`（注释在 `:3`）。
   地图层用 `position:fixed` + `z-index` 就够。

## 4.3 ③ 对话管线注入地图上下文

**最推荐落点（候选 ①）**：`GameRoleManager::merge_memory_bank_into_context`（`role_manager.rs:610-662`）+ 其唯一调用点 `role_manager.rs:353`。

**改法（只新增、不改坏）**：

1. **不改函数签名也能做**：`merge_memory_bank_into_context(built, &system_addendum, &short_term_prefix)` 的
   `system_addendum` 是 `String`，**直接把地图摘要 concat 进去**即可（在 `role_manager.rs:310-327` 取 addendum 的地方拼）。
   零签名变更、零调用点变更 —— **这是最稳的「只新增」做法**。
   若想干净，则加第 4 个参数 `map_addendum: &str`，调用点只有一处（`role_manager.rs:353`），风险可控。
2. **去重已有**：`:621` 用 `content.contains(...)` 判重 —— 复用即可，不用自己写。
3. **⚠️ 死锁与性能红线（必须遵守）**：
   - `sync_memories` 在 `game_status` 的 `&mut self` 锁内被调用（`game_status.rs:112`）→ **这里绝不能 `await` 网络/文件**。
   - 正确做法：在管线**起点**（`send_chat_message` 或 `execute_pipeline` `generator.rs:257` 之前）**预取一份地图快照**，
     缓存进 `GameStatus` 的一个新字段（**加字段是新增，不改既有语义**），这里只读缓存。
   - `13-世界模拟最终方案.md:72` 已经定了策略：「**变化时更新，其余轮次复用**」—— 正好对应这个缓存。
4. **备选（改动更小，但更「打补丁」）**：`MessageGenerator::get_current_context`（`generator.rs:246-254`），
   在 `Ok(role.memory.clone())` 后插一条 `LlmMessage`。**不在 `add_line` 高频路径上**，但绕过 `MemoryBuilder`/memory_bank，
   God Agent 多轮时可能重复注入（需自去重）。
5. **系统提示「固定段落」写法**：`13` 号方案要求「系统提示的固定段落」。
   `PromptRole::System.build_prompt()`（`utils/prompt.rs:281-288`）会包成 `{旁白: （系统提示：…）}`，
   与 `detect_scene_change`（`generator.rs:214-245`）插系统旁白的写法**完全一致** —— **建议直接复用这条路**，因为它已经被验证不会污染 UI。

**不推荐**：改 `sys_prompt_builder`（`utils/prompt.rs:180`）—— 它一次性固化成 System 台词（`service.rs:158-165`、`game.rs:661-672`），动态数据会过期。

## 4.4 ④ 上帝 Agent 加一个工具

**先分清楚：加哪一边的工具。** `13-世界模拟最终方案.md:126-132` 说的是加在 `god_agent/tools.rs`（路 A）；
但**主对话工具系统（路 B）更通用、权限自动放行、执行器现成**。

### 路 A：加给上帝 Agent（让它「汇报必要地点」）

| 步骤 | 位置 | 改法 |
|---|---|---|
| 1 | `src-tauri/src/ai_service/god_agent/tools.rs` | 照 `select_next_speaker_tool()`（`:12-33`）加 `plan_map_locations_tool()`；照 `parse_speaker_selection`（`:44-55`）加 `parse_map_locations()`（`parse_tool_args` 现成，`types.rs:68-91`） |
| 2 | `god_agent/core.rs:169` | `let tools = vec![tools::select_next_speaker_tool()];` → vec 里加新工具（**唯一装配点**） |
| 3 | `god_agent/core.rs:180-202` | **必须改分派** —— 现在只取 `tool_calls.first()` 且只认 `select_next_speaker`。要支持多工具，得按 `tc.function.name` 匹配 |

**风险提示**：`decide_next_speaker` 的失败语义很硬（无 tool_calls → `Err`，`core.rs:204-215`）。
**不要让地图工具污染发言者选择**：建议**单独发一次 `complete_with_tools`**（不要塞进同一个 vec），
或者在解析时「认不出名字就跳过、继续找 `select_next_speaker`」——后者要改 `:180-202`，属于**触碰核心逻辑**，谨慎。

### 路 B（推荐）：加给主对话工具系统

| 步骤 | 位置 | 改法（**全是新增**） |
|---|---|---|
| 1 | 新建 `src-tauri/src/ai_service/tools/world_map.rs` | 实现 `Tool` trait，模板 `tools/scene.rs:14-56`（`SceneList`：无参只读工具）。`definition()` 照 `scene.rs:18-31`，`execute()` 照 `scene.rs:33-55` |
| 2 | `tools/mod.rs:1-15` | 加 `pub mod world_map;`（一行）；`use` 照 `:36` |
| 3 | `tools/mod.rs:94 built_in_registry` | 加一行 `registry.register(Arc::new(WorldMapQuery))?;`（照 `:111-113`） |
| 4 | **权限** | **不用改**：`UserChat → scene_admin → all_tools:true`（`permissions.rs:356,367-374`）自动放行；角色组 `default` 由用户在前端开 |
| 5 | **不需要**改 `lib.rs` / capability / manifest | 工具注册表是运行时的，不进 `generate_handler!` |

**坑**：`ToolExecutor` 默认 **2 秒超时**（`tools/executor.rs:89`）—— 地图工具**必须读缓存**，不能现查网络。

## 4.5 ⑤ 存档系统加「地图」

**首选：`GameStatusSnapshot` 加字段**（`game_status.rs:267-285`）。

```rust
#[serde(default)]
pub world_map: Option<WorldMapSnapshot>,     // ← 只加这一行（类型新增在 world_map 模块里）
```

同步改**唯一的一对读写**：
- `to_snapshot`（`game_status.rs:229-242`）—— 写出去
- `apply_snapshot`（`game_status.rs:245-261`）—— 读回来

**为什么这是「只新增不改坏」**：
- `save.status` 是 TEXT 列存 JSON（`db/entities/save.rs:10-11`）→ **不需要新迁移、不需要动 `migration/`**。
- `#[serde(default)]` → **旧存档读进来得到 `None`，天然向后兼容**；写入端多一个字段，旧版本读也只忽略。
- 反序列化失败已有兜底：`api/save.rs:259` 整体 `unwrap_or_default()` 静默降级。

**配合 `13` 号方案的其他要求**：

| 需求（`13-世界模拟最终方案.md:27-32`） | 实现位置 |
|---|---|
| 「一个存档几张地图」「每个地图存档有独立地图库」 | 地图库根目录按 `save_id` 分：`<data_dir>/world_map/saves/<save_id>/maplib`（`13:188` 已定）。**`save_id` 在 `create_save`（`api/save.rs:137`）拿到，在 `load_save`（`:207`）读到** |
| 「选对话存档 → 选地图存档」 | 复用 `list_saves`（`api/save.rs:57-62`）；地图存档列表新增一个命令（**新增命令 = `lib.rs:675` 的宏里加一行**） |
| 「存档体积不限」「地图库上限不限制」 | 走【我】已有的 `world_map_maplib_*`（【我】`world_map/mod.rs:551,560,586`） |
| 「破坏性操作默认 dry_run」 | 【我】已有先例：`world_map_maplib_cleanup` 默认干跑（【我】`mod.rs:595` `dry_run.or(dry).unwrap_or(true)`）。**新增的地图存档删除接口必须照抄这个默认值** |

**若数据量大或需独立查询**：新增迁移（照 `migration/m20260727_000002_add_line_tool_call.rs` 35 行模板）
+ 在 `migration/mod.rs:8-16` 的 vec 末尾注册。**这是「新增」不是「改坏」**，但会动 schema，谨慎。

## 4.6 ⑥ 角色头像 / 立绘复用哪套组件

**结论：直接用 `GameRoleAvatar` + `StaticRolePresentation` 这一套，不要另写。**

| 要的东西 | 复用什么 | 位置 |
|---|---|---|
| 角色头像（随情绪变） | `get_avatar_file` 命令 + `convertFileSrc` | `src-tauri/src/api/character.rs:458`；前端 `GameRoleAvatar.vue:182-188` 是完整范例 |
| 立绘（大图） | `StaticRolePresentation.vue`（容器 `absolute h-full w-full origin-[center_0%]`，图片 `absolute h-[102%] w-full` + `position="center bottom"`，交叉淡入 `ImageAcrossFade`） | `src/components/game/standard/StaticRolePresentation.vue:2-21` |
| 立绘 + 情绪动画 | `GameRoleAvatar.vue`（`avatar-animation.css` + `EMOTION_CONFIG`） | `src/components/game/standard/GameRoleAvatar.vue:39,44,155-157,209-230` |
| 情绪映射表 | `EMOTION_CONFIG` / `EMOTION_CONFIG_EMO` | `src/controllers/emotion/config.ts`（导入点 `GameRoleAvatar.vue:39`） |
| 触摸交互（可选） | `TouchAreas.vue` | `GameRoleAvatar.vue:23` |

**「只新增不改坏」的做法**：

1. **别改 `GameRoleAvatar.vue`**。它 props 是 `{ role: GameRole; castScale?; castOffsetY? }`（`GameRoleAvatar.vue:46-53`），
   内部强依赖 `useGameStore()` / `useUIStore()`（`:55-56`）—— 地图页面没有那套 store 状态，硬用会拧。
2. **要的是「渲染逻辑」，不是「组件」**：直接复用 **`StaticRolePresentation.vue`**（它的 props 只有
   `{src, visible?, layerStyle, animationClasses, objectFit}`，`:29-38`，**零 store 依赖**）→ **地图页可以独立使用**。
3. **布局计算抄公式**，不要抄组件：`roleLayerStyle`（`GameRoleAvatar.vue:118-150`）里的
   `left = ((index+1)/(total+1))*100%`、`transform: translateX(-50%) scale(k)`、`top` 补偿 —— 抄成地图页自己的 computed 即可。
4. **`13` 号方案已确认**：「立绘直接用素材里的 `正常.webp` 等（3511×5242）；**同时只显示一张**（≈73MB 内存，与聊天里现状持平）」（`13-世界模拟最终方案.md:185`）。
   → **地图页不要同时渲染多个 `StaticRolePresentation`**，只渲染面板里那一个。
5. **侧边栏形态**（`13:40`）：桌面右侧滑出、手机全屏半透明遮罩 + 立绘 —— 用 `position:fixed` + 自己的 z-index（建议 60~100，**别超 9999**），
   与 `WorldMapLayer` corner 小窗（z-index 60，`WorldMapLayer.vue:222`）保持同一档位语义。

## 4.7 插入点模式总结 + 高风险区

### 4.7.1 LingChat 欢迎的 6 种插入方式

| 方式 | 真实例子（【我】行号） | 侵入度 |
|---|---|---|
| **① 纯新增文件**（首选） | `src-tauri/src/world_map/*.rs`（16 个 .rs / 11837 行）、`src/api/services/worldMap.ts`、`src/components/views/WorldMap.vue`、`public/world_map/*.js` | **0** |
| **② 单行挂载**（全局组件） | `src/App.vue:21` + import `:44` | 2 行 |
| **③ 单行事件转发 + 入口项** | `src/components/views/MainMenu.vue:52`、`MainMenuOptions.vue:12-14` + `:46` | 4 行 |
| **④ 路由追加**（数组末尾/中间插新对象） | `src/router/index.ts:18`、`:32-36`、`:82-103` | 33 行，**全是新对象** |
| **⑤ 命令注册块**（`generate_handler!` 内追加） | `src-tauri/src/lib.rs:555-581`（21 条）+ `mod` 声明 `:16` | 28 行，**零删除** |
| **⑥ 依赖 / 标记位追加** | `src-tauri/Cargo.toml:81` `md5 = "0.8"`；`src/web-mock.ts:116-119` | 7 行 / 5 行（**唯一改了 1 行既有代码**） |

### 4.7.2 改一行就动到原有逻辑的高风险区（按风险排序）

1. **`src/App.vue` 的全局挂载**（官方 `:17` /【我】`:21`）——
   `v-if="isMainWindow"` **只挡 DOM，不挡 `setup()`/`onMounted`**。
   `WorldMapLayer.vue` 的 `onMounted`（`:158-173`）无条件拉数据 + 注册定时器 → **每个用户每次启动都要付这个成本**。
   **这是本次插入唯一的「默认开启的既有路径成本」。**
2. **`src-tauri/src/lib.rs` 的 `generate_handler!` 块** ——
   21 条命令挤在列表**最前面**（【我】`:555-581`），紧贴既有命令。
   写错路径是**编译期就炸**（E0433，安全），但那 12 行「必须带模块前缀」的注释（【我】`lib.rs:574-575,579`、
   `world_map/mod.rs:34-45`）**是最容易被人误删的地方**。
3. **`src/router/index.ts` 的 `/world` 一级路径**（官方 `:28-30`）——
   官方底本目前没有 `/world`，但**官方将来若新增同名路径，合并会冲突**（提 PR 时要留意）。
4. **`src/web-mock.ts`**（【我】`:4` 被改）—— 唯一被**修改**的既有代码行（扩 `interface Window`）。
   影响面小，但它在**所有纯 web 预览的公共路径**上。
5. **`src/components/views/worldmap/DistrictViz.vue:69`** 用 `v-html` 注入后端产出的 SVG，
   而该 SVG **自带 `<style>`**（【我】`src-tauri/src/world_map/render.rs:584`、`render_geo.rs:345` 把 `{css}` 内联进 `<svg>`）。
   `v-html` 的内容**不受 `<style scoped>` 约束**，那段 CSS 第一条是裸元素选择器 `text{font-family:...}`（`render.rs:571-572`），
   还有全局 `@keyframes wmwave`（`render.rs:580`）→ **打开 `/world/district-viz` 期间会向全文档泄漏 `text` 元素的字体规则**。
   实际影响应该很小（LingChat 的 SVG 图标基本用 `<path>`），但这是**唯一一处真正会作用到组件外部 DOM** 的样式。
   → **新做地图页时，凡是 `v-html` 注入 SVG，都要把内联 CSS 里的选择器加前缀。**

### 4.7.3 一句话原则

> **新增文件 → 单行挂载 → 单行事件转发 → 一条路由 → 一行命令注册 → 一行依赖。**
> 能靠「多一个文件」解决的，绝不改既有函数体；必须改函数体的（如 §4.3 的 `system_addendum`），
> 优先选**只加参数/只 concat 字符串**而不是改控制流。
> 破坏性接口（删地图、清缓存）**一律默认 `dry_run`**（先例：【我】`world_map/mod.rs:595`）。

---

# 五、未确认 / 没搞懂（如实列，需要再查）

## 5.1 行号与基线

1. **`~/lingchat-official` 是否逐字节等于官方 0.5.1**：只依据 commit `348ef00c` 的 message；本机 `github.com` git 通道不通，无法独立比对。→ **未确认**
2. **「两条线的共同祖先」找不到**：`git merge-base main upstream/main` 无输出；`git rev-list --count main..upstream/main` = 5480。可能是浅克隆/历史被 graft，也可能确实无共同祖先。→ **未确认**（这影响「能不能把我们的改动 rebase 到官方线」的可行性判断）
3. **【我】的 `feat/world-map` 分支缺 `src-tauri/.cargo/config.toml`**：是「官方线基线没合过来」还是「刻意移除」，代码里没写。→ **未确认**（影响 release 优化与 Android 链接参数）

## 5.2 前端

4. ~~`avatar-animation.css` keyframes 未读~~ → **已补**（§1.6.3 表）。但 `EMOTION_CONFIG`（`src/controllers/emotion/config.ts:39` 起）里
   **每个情绪的 `animation` 具体取值未逐条抄**（只抽了 `厌恶` 一例），做地图表情时要逐个核对。→ **部分待补**
5. ~~`game/state.ts` 字段定义行号未读~~ → **已补**（§1.4.2 表，`state.ts` 112 行已核实）。
6. **`src/utils/typewriter/TypeWriter.ts`（`:5`）未读实现**；`event-queue.ts` 已读（见 §1.1 `core/events/`），但 `dialogue-merge.ts` 的合并时序只知其状态被谁读写，
   **「前端怎么逐字消费 `ai:reply`」的完整时序未走通**。→ **待补**（这决定地图气泡怎么插）
7. ~~`ScriptEventType` 位置未确认~~ → **已确认：`src/types/script.ts:141`**（同文件 `:1` 起是 `ScriptEvent` + 20 个子类型）。
   事件处理器注册在 `src/core/events/index.ts:7-28`（`import.meta.glob` 自动扫 `processors/*.ts`，**加一个 processor 文件即可，不需要改注册表**）。→ **已解决**
8. ~~前端 `listen` 的 unlisten 清理未核对~~ → **已核对**：
   **不清理的（常驻）**：`initializeTauriEventListeners()`（`tauri-events.ts:58`）注册的 40+ 个、`useAsrInput.ts:677`、`achievement.ts:108`、`App.vue:373`（有清理，见下）。
   **有清理的**：`Live2DStage.vue:616-621`（`pet:cursor`）、`useScreenshot.ts:31-43`（`destroy()`）、`GameDialog.vue:838-839`、
   `useImageSourcePicker.ts`（5 处 unlisten）、`tts-local.ts:23-26,43`、`SettingsTts.vue:1317-1319`、`LogConsole.vue:311-312`、
   `SettingsCast.vue:521`、`useLanSync.ts`、`useArchiveImport.ts`、`App.vue:296-297,425-426`（`app:close-ready`）。
   → **结论：主窗口的全局监听是刻意常驻的；地图事件要可清理就自己在组件内存 unlisten。**
9. **`GameDialog.vue`（1164 行）只读了 1-260 与关键 API 行号**：`dialogWrapperStyle`、`isHidden`、`removeDialog`、`inlineDisplayRef` 与合并追加路径的完整条件未确认。→ **未确认**
10. **`TouchAreas.vue:141-374` 未逐行读**：点击后的命令名、`player-continued`/`dialog-proceed` 的实际触发路径未确认；
    且 `GameRoleAvatar.vue:23` **只传 `body-parts`、没接这两个 emit**，怀疑是历史遗留。→ **未确认**
11. **`avatar-animation.css` 的 `.avatar-visible`(`:126`) / `.avatar-hidden`(`:130`) 全仓找不到使用点** → 疑似死代码。→ **未确认**
12. **`EMOTION_CONFIG` 里 `"../pictures/…"` / `"../audio_effects/…"` 相对路径的解析基准未实测**：
    只在 Vite `base` 为默认 `/` 时推导为 `/pictures/…`；Tauri 生产环境 URL 若形如 `http://tauri.localhost/xxx/index.html` 会有偏差。→ **未确认**
13. **`src/stores/types.ts`（15 行）疑似死代码**（全仓无 import 命中），但没做编译验证。→ **未确认**
14. **`script-editor` store 的完整 state 字段行号未逐个定位**（多行 `ref<T>(...)` 会漏）。
15. **`src/api/index.ts` 是 0 字节空文件** → 判断为空壳。→ **未确认**

## 5.3 后端

9. ~~`script:*` 系列 14 个事件的后端 emit 行号未定位~~ → **已补**（§2.9.1 的剧本表，14 个常量 + 15 个 emit 点全部落实）。
10. **`asr://speech_started` / `turn_candidate` / `turn_sealed` 的 emit 点**：`asr/vad.rs:234` 用**变量名** emit，未追到赋值处。→ **未确认**
11. ~~`log:entry` / `log-window:state` emit 点未定位~~ → **已定位**：`utils/log_bridge.rs:176`（`log:entry`，payload `LogEntry` `:86`）、`:60` 与 `:63`（`log-window:state`，payload `bool`）。
    **仍未确认的 emit 点**：`cast:config`、`pet-scale-changed`、`pet-effect-changed`、`pet-volume-changed`、`request-dialog-history`；
    `tts://download-progress` 的 payload 已确认是 `DownloadProgress`（`tts/local/download.rs:15`）；
    `pet:cursor` payload = `CursorPosition`（`api/pet.rs:23`）；`role:import-progress` / `plugin:import-progress` payload = `EntryEvent`（`utils/archive/mod.rs:152`）。
    `achievement:unlocked` 共 4 个 emit 点：`achievements/mod.rs:24`、`api/chat.rs:126`、`api/adventure.rs:381`、`script_engine/events/achievement_event.rs:95,122`
    —— **`api/script.rs` 里没有**（此前 grep 命中是误报）。
15. **本机拿不到真实存档数据**：`/storage/emulated/0/Android/data/com.noiq.ling-chat/` 不存在（app 未安装），
    `find ~ -maxdepth 4 -name 'game_database.db'` 无命中 → **只能给 schema，给不出真实字段取值样例**。→ **未确认**
16. **官方线的前端地图层其实走 HTTP**：`src/api/services/worldMap.ts:10` `USE_RUST = false`、`:16` 默认 `http://127.0.0.1:8791`，
    `world_map_*` 命令名只是**预留字符串**，官方线 Rust 侧完全没有实现（`grep -rn world_map src-tauri/src/` 命中 0）。
    → **官方线目前只有「地图前端」，没有「地图后端」**。
12. **`GameMemoryBank.schema_version` 没有消费方**：字段存在（`types.rs:262`）但找不到按版本分支的迁移代码，疑似仅占位。→ **未确认**
13. ~~`load_emotion_classifier` 降级分支未读~~ → **已确认**（`init/mod.rs:232-261`）：
    禁用 → 返回 `None`（`:236-239`）；模型目录不存在或缺 `model.onnx` → warn 后返回 `None`（`:255-258`）；
    `EmotionClassifier::load` 失败 → warn「回退为禁用状态」后返回 `None`（`:248-254`）。
    → **本机 `data/third_party/emotion_model_19emo/` 为空时，情绪链路会静默降级为「原始 tag 置信度 1.0」**（`processor.rs:166-172` 的 `None` 分支），
    **不会报错、也不会阻断对话** —— 本地验证情绪改动时要注意「跑的不是真分类器」。
14. **234 条命令这个总数由脚本加总，未人工逐条复核**；`invoke_handler` 只完整读了少数区段。→ **未确认**
15. **「世界书 / lorebook」在 Rust 侧完全不存在**（grep 零命中）。若官方有此概念，只可能在角色 `settings.yml` 的 `system_prompt` 里人工写。→ **未确认**
16. **`SkillAgentState.approvals`（`skill_agent/mod.rs:35`）与 `chat_command_approvals`（`lib.rs:118`）是否语义重复**：按命名推断，未逐行验证生产端 `tools/skill_files.rs`。→ **未确认**
17. **`api::script_editor::PreviewSession` 字段未读**（`lib.rs:135` 只有类型路径）。→ **未确认**

## 5.4 构建 / 平台

18. **APK 产物路径 vs `--target aarch64`**：CI 找 `apk/universal/release/app-universal-release-unsigned.apk`
    （`build-android.yml:123-124`），而构建命令只传 `--target aarch64`（`package.json:15`）。
    **手机上不能编译验证**，flavor 到底选哪个、路径是否真匹配 → **未确认**
19. **`abiFilters` 实际生效值**：`RustPlugin.kt:21` 依赖 Gradle 属性 `abiList`，仓库内无设置处 → 理论是 4 ABI 的 universal flavor。→ **未确认**
20. **`pnpm/setup@v1` 是否真实存在**（官方线 4 个 workflow 用它）—— 未联网核实。→ **未确认**
21. **Android 上世界地图定位**：manifest 里**没有任何定位权限**，Rust 侧走 IP 兜底（明文 http，`world_map/live.rs:59`）。
    代码注释自己也标了「真机 APK 内实测留作 CI 之后的验收项」（`live.rs:58`）。→ **未确认**
22. **【我】6 条「已注册、前端零调用」的地图命令**（`world_map_recent_events`、`world_map_coord_selftest`、
    `world_map_render_svg`、`world_map_stats`、`world_map_osm_summary`、`world_map_geo_status`）是有意留给调试，还是遗留。→ **未确认**
23. **`public/world_map/*.js`（5 个文件约 285KB）向 `window` 写入 `NPC_SYS`/`TRANSPORT`/`WORLD_TIME`/`WORLD_WEATHER`/`PHONE`
    是否与既有代码冲突**：只确认了注入幂等（`useWorldModules.ts:18`）且失败不阻塞（`:24`），未逐个审计。→ **未确认**
24. **`dist/` 是陈旧构建产物**（里面还写着 `http://127.0.0.1:8790` 旧端口），读它会被误导。→ **仅提示**

## 5.5 建议的下一步补查（按性价比排序）

1. 读 `src/controllers/emotion/config.ts:39` 起的 `EMOTION_CONFIG` 全表 → 补全情绪→动画/气泡/音效映射（§1.6.3）。
2. 读 `src/core/events/event-queue.ts` 全文 + `src/utils/typewriter/` → 搞清「前端怎么逐字消费 `ai:reply`」，这决定地图气泡/表演怎么插（§5.2 #6）。
3. 读 `GameDialog.vue` 全文（1164 行）→ 补全合并追加路径与对话框外观（§5.2 #9）。
4. 定位 `cast:config` / `pet-*-changed` / `request-dialog-history` 的后端 emit 点（§5.3 #11 剩余项）。
5. 真机验证项（需 CI 出包后）：release APK 里 IP 定位能否通、APK 产物路径是否匹配、地图页 `v-html` 的样式泄漏范围（§5.4 #18/#21）。

---

> **本文的一切改动建议都遵守同一条底线：只新增、不改坏原有。**
> 任何「改既有函数体」的提案，先问一句：能不能只加参数 / 只 concat 字符串 / 只追加一行？
