# SYuki 源项目 · 具体功能说明

> 本文档记录 **原 SYuki 项目**（我们改造 L-SYuki 之前的独立项目）的全部功能。
> SYuki 原是一个基于 RikkaHub + 自建 Python 后端的 AI 陪伴项目，其"魂"（人设 / 记忆 / 主动 / 情感）被本仓库的 L-SYuki 以 LingChat 原生方式吸收。
>
> 原项目架构：**RikkaHub（Kotlin Compose App）+ companion_new（Python FastAPI, 8766）+ Live2D 独立层**

---

## 一、App 端功能（RikkaHub 原生 Kotlin Compose）

### 聊天主页面
- 透明 Scaffold + 背景层（图片/渐变/Live2D）
- Box 三层结构：底色 → 背景图 → Live2D 独立层 → 对话透明
- 消息气泡（AI 白 / 用户蓝渐变）
- 输入区 + 发送 + 语音输入按钮 + TTS 自动朗读

### 主题系统
- PresetTheme 体系：8 个预设（Sakura / Ocean / Spring / Autumn / Black / Minimal / Claude / **SYuki 蓝白**）
- 自定义主题 + AMOLED 深色模式 + 运行时动态切换

### SYuki 功能区（14 个原生页）
| 页 | 功能 |
|---|---|
| 🎨 UI 预览 | N.E.K.O 风格界面设计稿 |
| 📝 待办清单 | 列表 + 添加 + 完成/删除 |
| 🍅 番茄钟 | 25 分钟专注 + 5 分钟休息 + 成就上报 |
| ❤️ 心情雷达 | 五维情绪进度条（喜悦/亲近/害羞/烦躁/活力 0-100）|
| 🧠 记忆列表 | 最近 50 条记忆 + 3D 星系入口 |
| 🎵 网易云 | 搜索 + 播放 + 底部播放条 + **二维码登录** + 歌单 |
| 🎨 SVG 工作台 | LLM 生成 SVG + 预览/代码/复制 |
| 🏸 羽毛球 | Canvas 接球小游戏 |
| 🖊️ 五子棋 | 15×15 + **人机 AI**（评分算法）+ 双人模式 |
| 🏆 成就系统 | 成就列表 + 解锁状态（首次对话/七日之约等）|
| 😺 表情库 | 73 个 OpenMoji 表情网格 |
| 💡 破冰话题 | 随机话题 + 换一个 |
| 📊 记忆报告 | 记忆整理统计 |
| 📡 屏幕面板 | 屏幕监控状态 + 主动消息轮询 |

### 语音 / Live2D
- 🎤 语音输入（Web Audio → vosk 本地识别）+ 📖 语音朗读 + 🎛️ 音色定制（6 MiniMax + 6 edge + Yuki）
- Live2D 看板娘：3 模型（猫娘 Mao / 悠小喵 / 雪熊少女），独立层渲染

---

## 二、Web 端功能（chat-lite.html）

### 聊天核心
- 流式回复（SSE）+ 取消 + 60s 超时兜底 + Focus Mode 思考气泡
- `<mood>` 情绪标签（驱动 TTS 情绪 + Live2D 表情）+ 会话持久化 + 空闲气泡淡出

### 快捷按钮
🌤️ 天气生活卡 · 🎵 心情推歌 · 🎮 galgame 选项 · 🎤 语音输入

### 侧边栏抽屉
记忆列表 / 报告 / 待办 / 成就 / 心情雷达 / 番茄钟 / 表情库 / SVG / 3D 工作台 / 屏幕监控 / 网易云 / 语音朗读 / 破冰器 / Live2D 切换

### 屏幕监控
定时截屏 + OCR + 视觉分析 → 内容变化 → LLM 判断 → 主动搭话

### 视觉风格
N.E.K.O 蓝白换肤（syuki-ui.css）/ 大厂极简风（ui-premium.html）

---

## 三、后端功能（8766 FastAPI · 123 个接口）

- **对话/LLM**：`/api/chat`（OpenAI 兼容流式）· `/v1/chat/completions` · `/api/ai-config` · `/api/config` · `/api/status` · `/api/game/llm` · `/api/chat/immersive` · `/api/chat/export`
- **语音**：`/api/tts`（+voices/voice/settings 定制）· `/api/asr`（WAV→文字）
- **记忆系统**：`/api/memories`（列表/搜索/stats/report/graph 3D星系/add/restore/archived）· `/api/training/*` · `/api/token_usage`
- **情绪/人格/角色**：`/api/emotion/state` `/api/emotion/chart` · `/api/personality` · `/api/persona` · `/api/affection`（好感度）· `/api/characters`（CRUD）
- **生活/工具**：`/api/weather` · `/api/icebreaker` · `/api/mood/music` · `/api/galgame/options` · `/api/todos` · `/api/pomodoro/complete` · `/api/achievements` · `/api/meme/library` · `/api/jukebox/*`
- **屏幕/主动**：`/api/screen/*` · `/api/notifications/pop` · `/api/idle/monologue` · `/api/inner-monologue/toggle` · `/api/heartbeat/*` · `/api/sleep` · `/api/checkin`
- **宠物/其它**：`/api/pet/*` · `/api/budget` · `/api/backup` · `/api/pngtuber/*` · `/api/live2d/*` · `/api/bili/*` · `/api/bing/search` · `/api/translate` · `/api/vision` · `/api/3d/*` · `/api/debug_log`

---

## 四、三大系统

- **语音系统**：MiniMax TTS（speech-2.8-hd，6 音色，情绪联动，音色定制）+ vosk ASR（本地离线）+ edge-tts 回退
- **记忆系统**：L1-L4 分层（会话摘要 / 长期记忆 / 用户画像）+ 睡眠整合 + 3D 记忆星系（LLM 语义关联）
- **主动系统**：屏幕监控（OCR+视觉）→ 内容变化 → LLM 判断 → 主动搭话（60s 间隔）+ 空闲独白 + 心跳

---

## 五、特色模式

### Galgame 物语
- 6 场景 + 诺一钦灵 8 种立绘情绪表情 + 16 个情绪动画 + 粒子特效
- 打字机台词 + 3 选项分支 + 自由输入 + 历史
- 后端：`/api/galgame/scene` `/api/galgame/line`

### 皮套舞台（Neuro 模式）
- Live2D 5 模型 + **单/双皮套模式**（SYuki ↔ 里人格「黑雪」同台对话）
- 表情 / 动作 / 自由移动 / 口型随 TTS 音量
- 后端：`/api/stage/models` `/drive` `/duo` `/background`

### 自主 AI 引擎（autopilot）
- 用户离开 **10 分钟** → 自主模式激活；每 **3-5 分钟** LLM 自主决策
- 工具系统：search_web / memory_read / get_weather / generate_image / rest
- 用户回来（心跳）自动退出

### 其它
- 文件喂猫（拖拽文件 → LLM 读取回应）
- 七日引导（第 1-7 天开场演出）

---

## 六、决策：为什么迁到 LingChat

SYuki 原项目是双端（RikkaHub App + Python 后端）。为了统一载体、减少维护、复用成熟引擎，我们把 SYuki 的差异化"魂"迁移进了 **LingChat**（Tauri 2 + Vue3 + Rust，天然支持角色卡 / 剧本引擎 / 主动系统 / 工具 / 语音 / 记忆库 / 同步），形成了本仓库的 **L-SYuki**。

> 迁移后的对照与改动见 [`L-SYuki-changes.md`](L-SYuki-changes.md)。
