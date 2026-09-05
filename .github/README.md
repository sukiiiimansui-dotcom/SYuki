# SYuki · LingChat 改造版（L-SYuki）

> 本仓库是 **L-SYuki 改造版**，基于开源项目 [LingChat](https://github.com/SlimeBoyOwO/LingChat) 改造。
> 项目主页 / 功能 / 构建请看仓库根 **`README.md`**；原版上游说明见 **`upstream/README.md`**。

## 快速导航

- 📖 [项目主页（根 README.md）](../README.md)
- 🌐 [原版上游说明（upstream/README.md）](../upstream/README.md)
- 📦 [上游 LingChat 原始项目](https://github.com/SlimeBoyOwO/LingChat)

## L-SYuki 是什么

把 **SYuki 的差异化能力** 搬进 LingChat 原生体系：

- 🧠 分层记忆系统（L1-L4，按角色隔离，多记忆不混）
- 🎵 网易云音乐（搜索 / 心情推荐 / App 级全局后台播放 / AI 发歌）
- 📺 B站学习（为 AI 提供 B站网络文化，AI 自主搜索 / 调用工具）
- 💗 主动 + 心跳系统（用户离开 → AI 主动想念搭话）
- 🌐 AI 工具系统（设置界面一键开关）

## LingChat 最新 0.5.1 版已搬运功能

> **（由于技术原因，短期不全，全部功能请以官方 [LingChat 仓库](https://github.com/SlimeBoyOwO/LingChat) 为准，并由本仓库持续将官方功能搬入主分支）**

本仓库同时把 **官方 LingChat 最新 0.5.1** 的功能搬进 L-SYuki，已搬运：

- ⚡ **演出流畅度**（消息合并 / 事件队列，同角色连续短句自动合并续打，减少切句闪断）
- 🎙️ **语音输入 ASR**（手动麦克风 + 自动监听，统一采集/识别/流式，设置含总开关）
- 📝 **台词融合 + 动作优化**（连续台词/动作按段续打，`charReveal` 逐字符渲染）
- 🎛️ **设置页重排**（部分）· **web 投影入口**（`SettingsCast`，后端 cast 服务暂缓）

> 官方 **LingChat 仓库**（含上述与更多，版本见上游）为功能全量来源，本仓库主分支持续搬运官方功能；我们对齐的官方最新版本以 [LingChat](https://github.com/SlimeBoyOwO/LingChat) 为准。

## 社区与源码

- 改动 / 迭代：本仓库 `main` 分支（L-SYuki）
- 原版功能 / 社区支持 / 下载：请前往上游 [LingChat](https://github.com/SlimeBoyOwO/LingChat)
