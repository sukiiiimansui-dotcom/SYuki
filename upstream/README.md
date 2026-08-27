# Upstream: LingChat (original)

> 本目录存放 **原版上游项目 LingChat** 的相关**说明与参考**，
> 仅用于标识来源与许可，**不是本仓库改造的代码本体**。
>
> 本仓库（`sukiiiimansui-dotcom/SYuki`）是 **L-SYuki 改造版**，代码主体在根目录的
> `src/`、`src-tauri/`、`data/`、`public/`、`scripts/` 下。

## 上游项目信息

- **项目名**：LingChat
- **仓库**：<https://github.com/SlimeBoyOwO/LingChat>
- **技术栈**：Tauri 2 + Vue 3 + TypeScript + Rust
- **定位**：一个灵动的 AI 聊天陪伴助手（聊天 / 剧本引擎 / 角色卡 / 技能代理 / 主动系统 / 记忆库 / 插件沙箱）

## 本仓库与上游的关系

| | 位置 | 说明 |
|---|---|---|
| 原版 LingChat | `upstream/`（本目录） | 上游说明 / 来源标识 / 许可参考 |
| L-SYuki 改造版 | 根目录 `src/`、`src-tauri/` 等 | 在 LingChat 基础上改造，注入 SYuki 差异化能力 |

## 改造亮点（L-SYuki 新增/增强）

- 🧠 分层记忆系统（L1-L4，按角色隔离，多记忆不混）
- 🎵 网易云音乐（搜索 / 心情推荐 / App 级全局后台播放 / AI 发歌）
- 📺 B站学习（为 AI 提供 B站网络文化，AI 自主搜索 / 调用工具）
- 💗 主动 + 心跳系统（用户离开 → AI 主动想念搭话，上帝 Agent 多角色自主接话）
- 🌐 AI 工具系统（搜索 / 天气 / 音乐等，设置界面一键开关）

## 许可

本仓库基于上游 [LingChat](https://github.com/SlimeBoyOwO/LingChat) 改造，遵循上游 LICENSE（见仓库根 `LICENSE`）。
