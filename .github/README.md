# SYuki · LingChat 改造版（L-SYuki）

> 本仓库是 **L-SYuki 改造版**，基于开源项目 [LingChat](https://github.com/SlimeBoyOwO/LingChat)（Tauri 2 + Vue 3 + Rust）改造。
> 完整的功能清单 / 构建说明 / 文档索引在仓库根 **[`README.md`](README.md)**；原版上游说明见 **[`upstream/README.md`](upstream/README.md)**。
>
> ⚠️ 本文件是**仓库首页**（GitHub 的 README 优先级是 `.github/README.md` > 根 `README.md`）。

---

## 🌏 世界模拟（World Sim）· 开发中

> **给 LingChat 加一层「真实世界」**：角色不再只活在对话框里 —— 他们有自己所在的城市、
> 会按日程出门、会在地图上被你看见、会遇上堵车和下雨，而这些都会**流回对话**
> （角色知道自己在哪、附近有什么、你离他多远）。

<p align="center">
  <img src="docs/assets/worldsim/01-national.png" width="180" alt="全国首屏">
  <img src="docs/assets/worldsim/02-neighborhood.png" width="180" alt="小区地图（AI 生成街区）">
  <img src="docs/assets/worldsim/04-dark.png" width="180" alt="深色主题">
</p>

| | |
|---|---|
| 🗺️ **五层下钻** | 国 → 省 → 市 → 区县 → 小区；首屏只画 34 个省级轮廓（秒开），行政区划走本地缓存 |
| 🎭 **AI 生成街区** | 真实地理（真的）+ 虚构街区布局（AI 编的），界面上如实标注 |
| 🚶 **角色在地图上生活** | 头像跟随情绪上地图、AI 决定移动、9 种出行方式、行程卡、10 种交通工具扁平立绘 |
| 🎲 **现实事件** | 10 大类 41 条事件，加权随机触发（深夜没人约饭、雨天堵车翻倍） |
| 💬 **注入对话上下文** | 位置 / 附近设施 / 天气 / 你离他多远 / 最近发生了什么 —— 只在变化时更新 |
| 🎨 **小清新 UI** | 薄荷奶油 + 毛玻璃两套皮肤、深色模式、手机等比缩放 |

<p align="center"><img src="docs/assets/worldsim/05-vehicles.png" width="560" alt="十种交通工具立绘"></p>

> ⚠️ **状态：开发中**，**真机运行验证尚未完成**，APK 构建暂时搁置。
> 上面的图是**后端真实渲染的地图**套上合成外壳（手机上跑不了浏览器截图）—— 地图内容是真的。
> 代码目前只在 **`feat/world-map`** 分支，**不在 `main`**。

📖 详细说明、技术取舍、进度表 → [根 README 的「🌏 世界模拟」一节](README.md#-世界模拟world-sim)
📂 设计文档（19 篇）→ [`docs/world-map/`](docs/world-map/)

---

## 快速导航

- 📖 [项目主页（根 README.md）](README.md) —— 完整功能清单 / 构建 / 文档索引
- 🌏 [世界模拟设计文档](docs/world-map/) —— 分层结构 / 统一坐标 / 渲染方案 / 接入实施 / PR 方案 / **进度总表**
- 🌐 [原版上游说明](upstream/README.md) · [上游 LingChat 原始项目](https://github.com/SlimeBoyOwO/LingChat)
- 📥 [下载 / Release](https://github.com/sukiiiimansui-dotcom/SYuki/releases)

---

## L-SYuki 是什么

把 **SYuki 的差异化能力** 搬进 LingChat 原生体系，并新增**世界模拟**：

- 🌏 **世界模拟**（本仓库新增能力，见上）—— 真实地图 + 角色在地图上的生活
- 🧠 分层记忆系统（L1-L4，按角色隔离，多记忆不混）+ 记忆可视化图谱
- 🎵 网易云音乐（搜索 / 心情推荐 / App 级全局后台播放 / AI 发歌）
- 📺 B站学习（为 AI 提供 B站网络文化，AI 自主搜索 / 调用工具）
- 💗 主动 + 心跳系统（用户离开 → AI 主动想念搭话）
- 🌐 AI 工具系统（设置界面一键开关）

## LingChat 最新 0.5.1 版已搬运功能

> **（由于技术原因，短期不全，全部功能请以官方 [LingChat 仓库](https://github.com/SlimeBoyOwO/LingChat) 为准，并由本仓库持续将官方功能搬入主分支）**

- ⚡ **演出流畅度**（消息合并 / 事件队列，同角色连续短句自动合并续打，减少切句闪断）
- 🎙️ **语音输入 ASR**（手动麦克风 + 自动监听，统一采集/识别/流式，设置含总开关）
- 📝 **台词融合 + 动作优化**（连续台词/动作按段续打，`charReveal` 逐字符渲染）
- 🎛️ **设置页重排**（部分）· **web 投影入口**（`SettingsCast`，后端 cast 服务暂缓）

> 官方 **LingChat 仓库**（含上述与更多，版本见上游）为功能全量来源，本仓库主分支持续搬运官方功能；我们对齐的官方最新版本以 [LingChat](https://github.com/SlimeBoyOwO/LingChat) 为准。

## 社区与源码

- 改动 / 迭代：本仓库 `main` 分支（L-SYuki）；世界模拟在 `feat/world-map` 分支
- 原版功能 / 社区支持 / 下载：请前往上游 [LingChat](https://github.com/SlimeBoyOwO/LingChat)
- 许可：**AGPL-3.0**，见 [`LICENSE`](LICENSE) 与 [`NOTICE`](NOTICE)
