# SYuki · LingChat 改造版

<div align="center">

> 一个搭载了 SYuki「魂」的灵动 AI 陪伴助手
>
> 基于开源 [LingChat](https://github.com/SlimeBoyOwO/LingChat)（Tauri 2 + Vue 3 + Rust）深度改造，
> 把 SYuki 的差异化能力搬进 LingChat 原生体系。

[![OS](https://img.shields.io/badge/OS-Android%20%7C%20Windows%20%7C%20Linux%20%7C%20macOS-blue?style=flat-square)](https://github.com/SlimeBoyOwO/LingChat)
[![Rust](https://img.shields.io/badge/Backend-Rust-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Vue](https://img.shields.io/badge/Frontend-Vue3+-green?style=flat-square&logo=vuedotjs)](https://vuejs.org/)

**L-SYuki** — 搭 LingChat 的车，注入 SYuki 的魂。

</div>

---

## 📌 这是什么

`SYuki` 是基于开源项目 **LingChat**（Tauri 2 + Vue 3 + Rust，聊天 + 剧本引擎 + 角色卡）改造的 AI 陪伴 App。
目标是「搭 LingChat 的车，注入 SYuki 的魂」——把 SYuki 特色的 B站学习 / 网易云音乐 / 分层记忆 / 主动心跳等能力，以 LingChat 原生方式迁移进来。

主要技术栈：

| 层 | 技术 |
|---|---|
| 界面 | Vue 3 + TypeScript + Tailwind |
| 后端 | Rust（Tauri 2）+ onnxruntime（本地 TTS / 情绪模型） |
| 内核 | 角色卡 / 剧本引擎 / 技能代理 / 主动系统 / 记忆库 / 插件沙箱 |

### 代码规模

| 模块 | 行数 |
|---|---|
| Rust 后端（`src-tauri`） | ~52.8k |
| TS + Vue 前端（`src`） | ~65.2k |

<img src="docs/assets/tech_stack.png" alt="技术栈构成" width="70%">

---

## ✨ 核心功能

LingChat 原生已内置：角色卡换装 / 立绘、剧本引擎（多事件 + script_editor）、技能 Agent、上帝 Agent、主动系统、工具 function-calling、多 TTS 适配器、记忆库、局域网同步、插件沙箱、番茄钟 / 日程 / 成就。

**SYuki 差异化能力（本次移植重点）：**

1. **🧠 分层记忆系统（L1-L4）**：每角色独立记忆库，自动压缩 短期回顾 / 长期经历 / 用户画像 / 约定，多记忆不混。
2. **🎵 网易云音乐**：搜索 / 心情推荐 / **App 级全局后台播放**（切页/玩游戏不停，独立音量），AI 可发歌给前端自动播。
3. **📺 B站学习**：为 AI 提供 B站网络文化（热榜 / 搜索 / 弹幕梗 / 高赞评论学习库），AI 可按聊天趋向自主搜索并灵活调用工具。
4. **💗 主动 + 心跳系统**：用户离开一段时间 → AI 主动想念搭话；主动投放可走上帝 Agent 多角色自主接话。
5. **🌐 AI 工具系统**：搜索 / 天气 / 音乐 / 屏幕 / 休息等，AI function-calling 灵活调用，全部开关可在设置界面一键启用。

> 所有移植功能的开关都已注册进设置界面（`config/tree.rs` → 设置面板「高级设置」），像 LingChat 原生功能一样可开可关。

---

## 🎛️ 功能模块构成

| 后端模块 | 行数 | 前端模块 | 行数 |
|---|---|---|---|
| 游戏/角色/记忆系统 | 6624 | 设置界面 | 12134 |
| AI 工具系统 | 5227 | 游戏渲染 | 6880 |
| TTS 语音 | 4028 | 状态管理 | 4112 |
| 技能代理 | 3058 | UI 组件 | 2612 |
| LLM 接入 | 2191 | 组合式函数 | 1774 |
| 对话消息系统 | 1927 | | |
| 主动/心跳系统 | 1463 | | |

<img src="docs/assets/backend_modules.png" alt="后端功能模块分布" width="70%">

<br/>

<img src="docs/assets/frontend_modules.png" alt="前端功能模块分布" width="70%">

---

## 🚀 快速开始

### 环境
- Rust（stable）+ Android NDK（构建 APK 用）
- Node.js + pnpm（前端）
- Android SDK

### 开发运行

```bash
pnpm install
pnpm tauri dev        # 桌面端开发
```

### 构建 Android APK

```bash
bash build_setup.sh                 # 一次性准备环境（rustup + ndk + target）
node scripts/prepare-bundled-resources.mjs 9   # 打包 data.7z
pnpm tauri android build --target aarch64     # 构建 arm64 APK
```

> 本机构建若遇 `ort-sys` 报 `could not determine cache directory`，设置 `ORT_CACHE_DIR` 环境变量绕过：
> `ORT_CACHE_DIR=$HOME/.cache/ort cargo build --lib`

---

## 🧩 移植对照（SYuki → L-SYuki）

| SYuki 资产 | L-SYuki 落地 |
|---|---|
| `setting_prompt.txt` 人设 | 角色卡 `settings.yml` 的 `system_prompt` |
| `rikka_memory.db`（L1-L4） | `memory_bank` 表 + `PersistentMemorySystem`（按角色隔离） |
| `jukebox_engine` / `music_login` | `netmusic_service` + `tools/netmusic` + 全局播放器 |
| `bili_learn` | `bilibili_service` + `tools/bilibili` + 知识注入对话 |
| `emotion_engine` | `emotion/classifier`（ONNX） |
| `autopilot`（主动） | `proactive_system` + `god_agent` |

---

## 📦 仓库结构

```
src/              Vue3 前端（界面 / 视图 / 组件 / store）
src-tauri/        Rust 后端（Tauri 2，RustPython 插件沙箱）
src-tauri/src/ai_service/   核心：记忆 / 主动 / 工具 / TTS / 剧本引擎
data/game_data/  角色卡 / 剧本 / 资源
scripts/         构建脚本（prepare-bundled-resources 等）
docs/            文档与资产
```

---

## 📄 上游项目

本仓库基于 [LingChat](https://github.com/SlimeBoyOwO/LingChat) 改造，遵循其开源许可。原项目功能与社区支持请见上游仓库。

## 📝 License

本项目基于上游 LingChat，遵循其 LICENSE（见 `LICENSE`）。

---

<div align="center">

**L-SYuki · 一个会记着你、会想你、会陪你学习的 AI 陪伴。**

</div>
