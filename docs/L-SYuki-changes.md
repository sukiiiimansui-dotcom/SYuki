# L-SYuki · 我们改了什么

> 本仓库是 **L-SYuki**：在开源 [LingChat](https://github.com/SlimeBoyOwO/LingChat)（Tauri 2 + Vue 3 + Rust）基础上改造，
> 把原 [SYuki](SYuki-original-features.md) 的差异化能力以 **LingChat 原生方式**迁移进来。
>
> 一句话：**搭 LingChat 的车，注入 SYuki 的魂。**

---

## 一、核心决策

| 决策 | 结论 |
|---|---|
| 载体 | 只用 **LingChat**（弃 RikkaHub App + Python 后端双栈） |
| SYuki 人设 | 迁入**角色卡** `settings.yml` 的 `system_prompt`（保留人设/性格/说话风格） |
| 记忆 | 用 LingChat 的 `memory_bank` + `PersistentMemorySystem`，**按角色隔离**，多记忆不混 |
| TTS | 用 LingChat 多适配器（本地 SBV2 等）替代 MiniMax |
| 主动/自主 | 用 LingChat 的 `proactive_system` + `god_agent` |
| 立绘/换装 | 用 LingChat 原生立绘 + 换装（弃 Live2D 独立层） |

---

## 二、SYuki → L-SYuki 迁移对照

| SYuki 资产 | L-SYuki 落地 | 说明 |
|---|---|---|
| `setting_prompt.txt` 人设 | 角色卡 `system_prompt` | 保留人设，去重/拟人化 |
| `rikka_memory.db`（L1-L4） | `memory_bank` + `PersistentMemorySystem` | 按角色隔离，自动压缩 短期/长期/画像/约定 |
| `jukebox_engine` / `music_login` | `netmusic_service` + `tools/netmusic` + **全局播放器** | 搜索/心情推荐/**后台播放不打扰游戏**/AI 发歌 |
| `bili_learn` | `bilibili_service` + `tools/bilibili` + 知识注入 | 热榜/搜索/弹幕梗/高赞评论学习库，AI 自主调用 |
| `emotion_engine` | `emotion/classifier`（ONNX） | 自动标注 emotion |
| `autopilot`（主动）| `proactive_system` + `god_agent` | 用户离开 → 主动想念/多角色自主接话 |
| 语音（MiniMax） | LingChat 多 TTS 适配器（本地 SBV2 等） | 复用成熟悉音色/离线 |
| 主题（SYuki 蓝白） | LingChat 主题 | 复用现有主题 |

---

## 三、本次改造重点（已落地）

### 1. 🧠 记忆系统融合（多记忆，AI 不记混）
- 用 LingChat 的 `memory_bank`（按 `save_id`+`role_id` 维度）承载 L1-L4 分层记忆
- 每角色一个 `PersistentMemorySystem`，4 段（short_term / long_term / user_info / promises）后台 LLM 并发压缩
- 修复：`short_term` 原本被 `merge_memory_bank_into_context` 忽略，现已正确注入
- 补齐单测 `memory_per_role_is_isolated`，验证多角色记忆隔离、不混

### 2. 🎵 网易云音乐（后台播放，不打扰游戏）
- 新增 **App 级全局迷你播放条** `NetMusicPlayer.vue`（切页/玩游戏不停，独立音量）
- 独立 `netmusic` store（与游戏 BGM 分离）
- `NetMusic.vue` 由外链跳转改为**应用内播放**
- 后端 `tools/netmusic` 加 `netmusic:play` 事件 → **AI 发歌自动播**

### 3. 📺 B站学习（为 AI 提供 B站网络文化）
- `bilibili_service` + `tools/bilibili`（搜索/学习/查知识库），AI 可自主调用
- 知识库最近的弹幕文化/高赞评论 **注入对话 system 提示**（`generator.rs`），AI 基于所学文化回应

### 4. 💗 主动 + 心跳系统（用户离开 → AI 想念）
- `ProactiveConfig` 加离开触发配置（`enable_away_trigger` / `away_timeout_secs` / `away_max_times`）
- 新增 `Miss` 意图 + 离开检测：用户离开超时 → AI 主动想念搭话
- `deliver` **注入 `god_agent`**，离开时主动对话可多角色自主接话
- 前端 `useHeartbeat.ts` 心跳上报 + 后端 `proactive_mark_active`

### 5. 🌐 AI 工具系统（设置界面一键开关）
- 网易云 / B站 / 离开想念等开关全部注册进 `config/tree.rs` → 设置面板「高级设置」显示
- 像 LingChat 原生功能一样，可在设置界面打开/关闭

---

## 四、平台 / 构建

- Android 包名：`com.syuki.lingchat`
- 构建：`bash build_setup.sh` + `node scripts/prepare-bundled-resources.mjs 9` + `pnpm tauri android build --target aarch64`
- 本机构建遇 `ort-sys` 报 `could not determine cache directory` → 设 `ORT_CACHE_DIR` 绕过

---

## 五、未移植 / 保留原版

- **Live2D 皮套**：L-SYuki 用 LingChat 原生**立绘 + 换装**（弃 Live2D 独立层），如需动态表情可再评估增量
- **MiniMax 云端 TTS**：改用 LingChat 本地多 TTS 适配器（离线可用）
- **屏幕监控**（主动窥屏）：LingChat 的 visual_monitor 仅 Windows 生效，Android 上为**暂缓项**（保留框架）

---

> 想看 SYuki 源项目具体功能，见 [`SYuki-original-features.md`](SYuki-original-features.md)。
> 想看源码结构 & 原版区分，见仓库根 [`README.md`](../README.md) 和 [`upstream/`](../upstream/)。
