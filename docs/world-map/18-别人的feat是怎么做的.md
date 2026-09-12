# 18 · 别人的 feat PR 是怎么做的（结构化调研）

> 本文只回答一个问题：**上游把"新功能"合并进来的 PR，在 commit、目录、文档、拆分上是长什么样的**，
> 目标是给我们自己的「世界模拟 / 地图」feat 一份**可照抄的模板**。
>
> - 方法：**只读** `api.github.com`（未 fork、未建分支、未评论、未改任何东西）。
> - 数据：复用已有采集（`~/lingchat-pr-research/` 的 `pulls_closed.json` / `merged30_detail.json` /
>   `merged30_commits.json` / `reviews.json` / `tree_dev.json`），本轮新增探针脚本
>   `feat_probe.py` / `feat_comments.py` / `feat_files.py`，新数据落在
>   `feat_pr_detail.json` / `feat_comments.json` / `feat_commits.json` / `feat_files.json` / `feat_issues*.json`。
> - 与 `15-向LingChat提PR操作手册.md` 的关系：15 号覆盖"已合并 PR 的整体规模 / review 生态 / 维护者口味"，
>   本文**不重复**那些统计，只补三块新东西：**feat 的代码与提交结构**、**大功能拆分的实证依据**、**可执行的切法**。
> - 每条结论后面带 `【证据】`；拿不到的数据写「拿不到」。

---

## 0. 结论速览

| # | 结论 | 一句话证据 |
|---|---|---|
| 1 | **前端后端在同一个 PR 里，不拆** | 9 个已合并 feat，**9/9 同时改 `src-tauri/src/**` 与 `src/**`**，无一例外【证据 A1】 |
| 2 | **"收进自己的子目录"是硬要求，维护者会逐行点名** | 维护者原话：「这个文件不要直接放在 providers 目录下，模块化到 codex 的地方」【证据 B1】 |
| 3 | **既有文件只改"接线"几行，其余全在新目录** | #711 的 14 个 Rust 文件全在 `ai_service/asr/`，既有文件改动是 `mod.rs +1`、`api/mod.rs +1`【证据 B2】 |
| 4 | **commit 数量不是评判标准（1~79 个都有），"一件事"才是** | #671/#715 各 1 个 commit 合并；#711 79 个 commit、#750 27 个 commit 也合并【证据 A2】 |
| 5 | **拆分按"可独立交付的功能"切，不按前后端/抽象层切** | 维护者原话：「124 文件 +13334 行，多组独立功能捆绑 → 拆分为"引擎能力"与"DLC 管理"两个 PR」【证据 C1】 |
| 6 | **"引擎能力大 PR" 这种切法会失败** | #659 自拆后重提的 #677 反而涨到 **129 文件 / +12977**，作者明确拒绝再拆【证据 C3】 |
| 7 | **快 = 先有 issue（带模块结构 + 行数估算）** | 最快 5 个 feat 中 4 个的上游 issue 早于 PR 存在；#650 里连目录树都画好了【证据 D1】 |
| 8 | **快 = 单功能，不夹带** | #753 独立提交 1 个 feat commit 并当天合并【证据 D3】 |
| 9 | **PR body 里必须有「验证」小节** | 已合并 30 个里 `## 验证` 出现 13 次，排第一（15 号手册已统计） |
| 10 | **demo（截图/录屏）放**评论**里，维护者会主动要** | 所有者原话：「请提供一下基础的功能展示 demo（图片 or 视频）」【证据 B4】 |
| 11 | **项目级"不要自带测试脚手架"是政策，不是偏好** | #651 `chore: 清理项目内测试代码与测试文档` **-3744 行 / 61 文件，已合并**（15 号手册已统计） |
| 12 | **别碰 `.github/workflows/`** | #754 是"iOS 适配 + 打包工作流"**52 文件**，被拆成 #775 才合（该拆论据较弱，见 C2 备注） |

**一句话给我们的**：一万行不是一个 PR 的事，也不是"前后端拆开"的事；
要切成**若干个「能单独跑起来、有 demo、有 issue 打底」的功能垂直切片**，
第一刀要小到 **≤1 个功能、≤30 个文件、≤1500 行**，让维护者 10 分钟能看完并当场点掉。

---

## 1. feat 类 PR 的"结构"（逐个体检）

### 1.1 样本定义

从最近 30 个已合并 PR 里按标题筛出 feat 类（含 `feat` / 新增功能语义），**排除 fix / chore / refactor / perf**：

| PR | 标题 | 文件 | +行 | commit | 创建→合并 |
|---|---|---|---|---|---|
| #641 | feat：添加了 ds websearch | 8 | +253 | 3 | 3.36 天 |
| #643 | feat: 新增 Codex 风格工具访问模式、Glob/Grep 与 ReadMediaFile | 20 | +2166 | 16 | 6.23 天 |
| #665 | feat: 添加可选 Live2D 角色支持 | 46 | +3358 | 15 | 5.43 天 |
| #671 | feat: 注册 LLM 错误分类并在前端 i18n 提示可能原因 | 11 | +246 | 1 | 4.91 天 |
| #711 | feat：语音识别系统（VAD + 双 provider + 流式） | 53 | +6017 | 79 | **2.19 天** |
| #714 | feat: OpenAI Codex LLM 提供商（OAuth/代理/额度/推理档位） | 29 | +1983 | 7 | 0.75 天 |
| #715 | feat: 恢复网页搜索「模型 API 内置联网」选项 | 11 | +261 | 1 | **0.52 天** |
| #750 | feat(tts): 云端语音克隆 TTS(CosyVoice) | 32 | +2075 | 27 | 1.18 天 |
| #753 | feat: GPU 性能检测显示实际调用的 GPU 并分级 | 9 | +206 | 9 | 0.65 天 |

**同时体检的"反面样本"（未合并 / 已关闭）**：
#659（124 文件 / +6950 / 22 commit，作者自关）、#695（42 文件 / +3182 / 14 commit，作者自拆）、
#776-778（拆分产物，至今 open）、#677（129 文件 / +12977 / 47 commit，open）、#780（87 文件 / +4039 / 1 commit，draft）。

### 1.2 commit 数量与粒度：**没有统一标准，但有一条隐线**

- **1 个 commit 也能合**：#671、#715 都是单 commit【证据 A2】。
- **79 个 commit 也能合**：#711（79 commit，8 天开发，2.19 天合并）【证据 A2】。
- **27 个 commit 在同一天内打完，次日合并**：#750（27 commit 全部集中在 08-29，08-30 合并）。
- 所以**"要不要 squash"不是问题**；维护者看的是 PR 的**功能边界**。

**典型 PR 的 commit 列表原文（照抄用）**

**① #750（27 commit，全是同一天，粒度=一个最小可编译单元）**——最值得模仿的节奏：

```
442820ce 2026-08-29T02:48 | feat(tts): cosyvoice 配置层——API Key/模型列表/音色映射存 settings.json
4881f7d1 2026-08-29T02:51 | feat(tts): cosyvoice 云端 HTTP 客户端——音色注册/查询/上传
8a7d9dce 2026-08-29T02:57 | feat(tts): cosyvoice 音色注册状态机——上传/轮询/列表/删除
88799e5b 2026-08-29T02:59 | feat(tts): cosyvoice 适配器接入——provider 路由 + 角色级音色配置
5a8cfa13 2026-08-29T03:05 | feat(tts): cosyvoice 命令集——配置/音色注册/列表/删除/试听
e67bfcd3 2026-08-29T03:07 | feat(tts): cosyvoice 前端 API service
4087330a 2026-08-29T03:08 | feat(tts): 设置页新增语音克隆TTS卡片——Key/模型/音色注册/试听
f6749e32 2026-08-29T03:10 | feat(tts): 角色语音设置新增语音克隆TTS选项与音色选择
42064a26 2026-08-29T03:11 | feat(i18n): 语音克隆TTS 4语种文案
f878878a 2026-08-29T03:14 | fix(tts): getPolicy 改用 POST(与官方示例一致)
...（中间 15 条 fix/chore，都是实测反馈的回修）
276d2f30 2026-08-29T08:21 | feat(tts): 语音克隆设置完善——TTS 标题改 TTS 设置/导入音色独立小节(必填项标注)/删除模型管理/提示文案 i18n 化/高级设置卡片等高
6f48bfc5 2026-08-29T08:42 | fix(tts): 语音克隆收尾——toAudioBuffer 视图区间截取/导入小节分割线统一/必选标注/新文件 rustfmt+注释更新
```
【证据 A3】原文见 `feat_commits.json["750"]`。

**② #711（79 commit，按"后端→前端→i18n→真机验证→回修"推进）**——前 10 条：

```
c587ea6b 08-19T08:07 | chore(deps): 加入 silero-vad 模型到 data/third_party/asr_vad/
f1e7dc63 08-19T08:18 | feat(asr): 后端 AsrProvider trait + 4 个云 provider 实现
a9441a77 08-19T08:24 | feat(asr): ASR 配置持久化复用 tauri_plugin_store
c8967e33 08-19T08:27 | feat(asr): 后端 Silero VAD 端点检测（ort Session bundled）
0b363739 08-19T09:28 | feat(asr): 后端 AsrSession 会话编排 + Tauri commands
944f838a 08-19T09:33 | feat(asr): 前端 ASR service + store + 事件总线
2f854677 08-19T09:43 | feat(asr): 前端 useAsrInput composable 统一三种触发源
b2a51cf0 08-19T09:46 | feat(asr): 前端 useGlobalHotkey + 快捷键设置
313519a6 08-19T10:10 | feat(asr): SettingsAsr.vue 设置页 + i18n 4 语言
46b8f776 08-19T13:00 | refactor(asr): 删除原 Web Speech API 实现 + i18n key 替换
```
【证据 A3】注意第 1 条是 `chore(deps)`、第 10 条是 `refactor`——**功能 PR 里也允许顺手的清理 commit**。

**③ #695（14 commit，反面教材）**——多组互不相关的东西混在一起：

```
97c21f13 08-23T18:22 | feat: 场景/音乐/背景分类、收藏置顶、场景排序、清空空场景   ← 一次塞了 8 个子功能
de18e02b 08-24T10:35 | Merge branch 'SlimeBoyOwO:dev' into dev              ← 拉了上游
b785f421 08-24T19:44 | feat: 开机自启动功能（角色桌宠 + TTS 联动 + 启动设置）    ← 与上一条零关系
74abf03e 08-26T15:54 | Merge remote-tracking branch 'upstream/dev' into live2d-merge-test  ← 借鉴了别人的分支名
87f0940f 08-30T05:45 | merge: 同步 upstream/dev 并修复开机自启动桌宠窗口默认置顶
9baa264a 08-31T06:31 | merge: sync upstream dev into PR #695             ← 14 条里 5 条是 merge
```
【证据 A4】原文见 `feat_commits.json["695"]`。**14 条里 5 条 merge commit、耗时 8 天、跨 3 套功能**——
这就是后来被拆成 #775~#778 的病根。

**commit message 风格结论**：
- 中文与英文都合法。`feat:` / `fix:` / `chore:` / `docs(asr):` 这类**前缀是主流**（#711、#750、#665、#643、#799 都是）；
  中文前缀（`功能：` / `优化：`）只在 #776-778 出现，是同一作者的少数派风格【证据 A5】。
- 标题写"做了什么"而不是"改了哪个文件"；正文里写**为什么**（#711 几乎每条都有为什么）。
- **不要用 `Merge branch ...` 反复同步上游**：#643/#665/#695 都有，但 #695 因为 merge 太多，conflict 反复出现，
  最后一条 commit 就叫 `merge: sync upstream dev into PR #695`。

### 1.3 文件组织：**"必须收进自己的子目录"有实证**

**证据 B1（最硬的一条）**——维护者 `SlimeBoyOwO` 在 #714 的行内 review（08-26T09:35，`CHANGES_REQUESTED`）：

```
[src-tauri/src/ai_service/llm/codex/auth.rs] 这个文件不要直接放在providers目录下，模块化到codex的地方。
[src/api/services/codex/index.ts]            codex.ts 这个也不要直接放在 services 目录下
[src-tauri/src/api/codex/mod.rs]             也不要直接放在 api 下
```
作者 `sdfsfsk` 当天回复：
```
已按建议模块化收拢（df637e9f）：后端 `llm/codex/{auth,provider}`、`api/codex/mod.rs`，
前端 `services/codex/index.ts`，模块路径与导入不变。
```
【证据 B1】→ 连"路径与导入不变"都特意说明，说明维护者在意的是**归属**而不是形式。
**注意：这是在已经 APPROVED 的同一条 review 里说的**（"大体没问题，就是文件放的位置不优雅，改改就行"）——
位置不对不会毙掉 PR，但一定会被要求改，且会拖一天。

**证据 B2（现状代码的实证）**——#711 的 53 个文件分布：

```
后端  src-tauri/src/ai_service/asr/  ← 新子系统，14 个文件全部在这里
        mod.rs / error.rs / provider.rs / provider_stream.rs / provider_stream_llama.rs
        session.rs / settings.rs / vad.rs / vad_segmenter.rs
      src-tauri/src/api/asr.rs        ← 命令注册，一个新文件
      src-tauri/src/ai_service/mod.rs ← +1 行
      src-tauri/src/api/mod.rs        ← +1 行
前端  src/api/services/asr.ts / src/stores/modules/settings/asr.ts / src/composables/useAsrInput.ts
      src/components/settings/pages/SettingsAsr.vue / src/utils/asrAudio.ts / src/utils/asrError.ts
      src/locales/{zh-CN,zh-HK,en,ja}/{settings.ts,advance.ts,game.ts} ← 12 个 i18n 文件
接入  src/App.vue +8 / GameDialog.vue +114-93 / ChatInput.vue +53 / MainChat.vue +2 ...
```
【证据 B2】原文见 `feat_files.json["711"]`。
**模式 = 「新东西全在自己的目录」+「既有文件只改登记/接线的那几行」**。

**#714 收敛前的样子 → 收敛后**（`feat_files.json["714"]` 未单独采集，但 review 与回复给出了路径）：
`ai_service/llm/codex/{auth.rs,provider.rs}`、`api/codex/mod.rs`、`api/services/codex/index.ts`。
dev 分支上现存的实际形态也印证了这一点（`tree_dev.json`）：
`src-tauri/src/api/codex/`、`src-tauri/src/api/role_archive/`、`src-tauri/src/api/script_editor/`、
`src-tauri/src/utils/archive/`、`src/components/script-editor/{agent,fields,flow,modals,panels,preview,tabs}/`、
`src/api/services/tts/`、`src/stores/modules/script-editor/` —— **子目录是既定惯例，不是可选项**。

**对照两个 PR 的目录结构（题面要求）**：

| | #711（ASR，新子系统） | #750（CosyVoice，接进已有子系统） |
|---|---|---|
| 新代码落点 | 全新目录 `src-tauri/src/ai_service/asr/`（14 文件） | 挂进已有树 `ai_service/tts/cloud/`（5 文件）+ `ai_service/tts/adapters/cosyvoice.rs` |
| 对既有 Rust 的侵入 | `ai_service/mod.rs +1`、`api/mod.rs +1` | 改 12 个既有文件，但除 `config/tts.rs +57` 外都 ≤9 行 |
| 前端新文件 | 6 个 | 1 个（`api/services/tts/tts-cosyvoice.ts`），主要改既有设置页 |
| 结论 | 新能力 → **开新目录** | 扩展能力 → **进已有目录**，但**仍然要有自己的子目录**（`tts/cloud/`） |

**→ 对我们的直接含义**：地图是"新能力"，所以 `src-tauri/src/world_map/`（我们现在就是这样）**符合惯例**；
前端应当是 `src/components/views/worldsim/`（现在也是）+ `src/api/services/worldMap.ts`（现在也是）+
`src/composables/useWorldSim.ts`（现在也是）。**唯一需要调整的是"侵入既有文件的量"**：
`MainMenu.vue`、`MainMenuOptions.vue`、`router/index.ts` 这种每个只 +2~10 行 ✅，
但任何既有文件被改到 +50 以上，就该考虑抽到自己的目录里再接线。

### 1.4 前后端一起改的 feat 怎么切 commit

**结论：不按前后端切 PR，按"能力链路"切 commit，顺序是 后端 → 前端 service/store → 组件 → i18n → 回修。**

三条实证：
- **#711**：`08:18~09:28` 五条后端 commit（trait → 持久化 → VAD → session/commands），
  `09:33~10:10` 五条前端（service/store → composable → 快捷键 → 设置页 + i18n）【证据 A3】。
- **#750**：`02:48~03:05` 后端五条（配置层 → HTTP 客户端 → 状态机 → 适配器 → 命令集），
  `03:07~03:11` 前端四条（service → 设置卡片 → 角色设置 → i18n），之后全是回修【证据 A3】。
- **#695（反面）**：`feat: 场景/音乐/背景分类…` 一条 commit 同时改后端 `music.rs`/`scene.rs` 和前端多个设置页，
  维护者随后必须逐文件写长 review（见 C2）【证据 A4】。

**唯一允许"跨前后端强行分开"的情形**：既有文件被大范围重构（#643 最后一条
`refactor: 抽取 FullAccessWarning 独立组件，减轻 MainChat 职责` 就是单独一条）。**但它在同一个 PR 里。**

### 1.5 文档 / 截图 / 设计说明：**有，而且比想象的重**

| 项目 | 实证 |
|---|---|
| **仓库内文档** | #665 带了 `docs/live2d/{README.md +18, authoring.md +199, development.md +200}` + `docs/utils/live2d.md +73`，**共 +490 行 markdown**【证据 B3】 |
| 文档目录惯例 | dev 分支已有 `docs/live2d/`（含 `diagrams/*.html`）、`docs/script-editor/`（含 6 个 `diagrams/*.html`）、`docs/function_call/`【证据 B5】 |
| **PR body 结构** | #665 完整套用了官方模板：`## 变更说明 / ## 变更类型（checkbox）/ ## 相关 Issue / ## 检查清单 / ## 截图/示例`，检查清单里逐条列了 7 项验证命令【证据 B6】 |
| **截图/录屏** | 放**评论**里：#714 作者在 19:03~19:07 连发 3 张 1502×832 截图；#665 的 review 结尾要求「提供简单的视频 demo 演示也是必要的」【证据 B4】 |
| **维护者主动索要 demo** | 所有者 `SlimeBoyOwO` 在 #776（08-31T12:15）：「请提供一下基础的功能展示 demo（图片 or 视频）」；作者 13:16 补了 5 张图 + 2 段视频【证据 B4】 |
| **设计文档链接** | #677 body 末尾「## 相关资料」给了 **配套 DLC 仓库链接**（`sdfsfsk/LingChat-DLC-seventh-test-script`）【证据 C3】 |

**⚠️ 但"测试脚手架"是明确不要的**：#651 `chore: 清理项目内测试代码与测试文档`（**+38 / -3744 / 61 文件**）
把所有 `#[cfg(test)]`/`#[test]`/测试目录/测试命令清成 0，PR body 原文：
> 「本 PR 不修改产品运行逻辑，但会移除现有自动化回归覆盖；**后续功能变更将主要依赖编译、构建和人工验证**。」
【证据 B7】

后续 feat 的应对方式（这是**最新的可行姿势**）：
- #750（#651 之后 5 天创建）仍然带了 `src-tauri/src/ai_service/tts/cloud/enrollment_test.rs`（+58）与
  `src-tauri/src/config/tts_test.rs`（+46），**是独立文件 + 裸 `#[test]`，不含 `#[cfg(test)]`**，并成功合并【证据 B8】。
- 反过来 #643 有一条 commit `chore: remove Codex catalog unit tests from PR`、#665 被点名
  「存在许多未清理的测试代码（包括硬编码），解决一下」并删掉了 4 个 `.test.ts` + Vitest【证据 B7】。

**→ 对我们的直接含义**：`*_selftest.py`、`frontend_selftest.js`、`page_smoke.js` **绝对不进 PR**；
Rust 侧如果确实想留测试，用 `xxx_test.rs` 独立文件 + 裸 `#[test]`（#750 姿势），**不要 `#[cfg(test)] mod tests`**。

---

## 2. 大功能怎么拆（本文最重要的一节）

### 2.1 #695：维护者**没有直接说"拆"**，但逐文件 review 逼出了拆分

**时间线（原文，`feat_comments.json["695"]`）**：

| 时间 | 谁 | 内容（原文节选） |
|---|---|---|
| 08-23 21:04 | CF-612 | 开 PR，3182 行 / 42 文件 / 14 commit，标题把"Bug 修改 + 场景音乐优化 + 开机自启"混在一起 |
| 08-24 20:38 | CF-612 | 评论贴功能清单 + 2 张截图，「目前只编写和测试了 Windows 版本，没有 Mac 设备无法测试」 |
| **08-30 14:45** | **T-Auto（COLLABORATOR）** | **逐文件技术 review（约 500 字）**，指名 3 个安全问题：`delete_music` 用 `file_name()` 拼根目录会误删、分类操作缺 `validate_path_in_base` 白名单、移动失败后 `remove_dir_all` 会连带删掉没搬走的文件 |
| 08-30 15:46 | CF-612 | 「8.30 安全性问题已调整」 |
| 08-31 07:06 | CF-612 | 同步 dev、吸收 #759 的设计 |
| **08-31 09:20** | **T-Auto** | 第二轮：`set_pet_mode(true)` 失败后窗口卡死、虚拟分类「插件」前端没禁用、**场景只记文件名不记分类路径导致同名背景串味** |
| **08-31 11:26** | CF-612 | **「该大型 PR 已按功能范围拆分为四个独立 PR：#775 服装滚动条、#776 背景与音乐分类、#777 开机自启与桌宠启动、#778 角色收藏。后续请分别审阅与合并。」** |

**拆的依据 = 按功能范围（原文 "按功能范围"），不是按前后端、不是按依赖顺序。**
触发点不是一句"请拆分"，而是 **7 小时一轮、连续两轮、指名到函数的 review 压力**【证据 C2】。

**拆出来的四个 PR 各是什么**（`feat_pr_detail.json`）：

| PR | 标题 | 文件 | +行 | commit | 状态（截至采集） | 维护者评价 |
|---|---|---|---|---|---|---|
| #775 | fix：恢复服装列表横向滚动条 | 1 | +26 | 1 | **已合并**（08-31 开，当天合） | 「**拆的比之前好很多了，+26 / -1**」，然后提了 3 条样式复用意见；作者当轮改完 |
| #776 | feat：完善背景与音乐分类管理 | 17 | +2246 | 2 | **open**，labels `status: needs more info` + `status: waiting`，11 天无新动作 | 「**这个算大型 PR**」+ 5 大段问题 + **所有者追加索要 demo** |
| #777 | feat：增加 Windows 开机自启并完善桌宠启动 | 23 | +1090 | 1 | **open**，`status: waiting` | 「优雅么…算了这个问钦灵，我在隔壁对启动项管理及其严格，这个开机启动很潦草，神秘神秘硬编码，神秘状态管理」 |
| #778 | feat：支持角色收藏跨页置顶 | 9 | +139 | 2 | **open**，`status: waiting`，**CHANGES_REQUESTED** | review 4 条 + **所有者的架构级否决**：作者原本存 localStorage，所有者回「建议记录在 data 的某个文件里…**不要存在 localStorage 里**」 |

**关键观察（题面问"拆分后是否更好合并"）**：
1. **只有最小的那个（#775，1 文件 +26 行）当天合并**。
2. **拆完之后"大"的那条依然被叫"大型 PR"**——#776 只有 17 个文件，仍被 T-Auto 定性为「这个算大型 PR」。
   → 说明**阈值跟功能复杂度强相关，不只是行数**。
3. #778 的 review 暴露了一个更贵的风险：**架构选择错误（localStorage vs 数据文件）会让"小 PR"也卡住**，
   而且作者 12:59 已经改完（迁到 `data/game_data/characters/favorites.json` 纳入 LAN 同步），
   维护者 12:29 已经 `CHANGES_REQUESTED`——**说明"小"不等于"低风险"，得先问清楚落库位置**。
4. #775 里 T-Auto 的第一句话才是真正的奖品：**「拆的比之前好很多了，+26 / -1」**——
   维护者**明确表扬了拆分**，而且是用行数表扬的。

> ⚠️ 这一点要修正 15 号手册的表述：15 号写的是"#695 被拆成 4 个 PR"，
> 严格讲是**作者自己拆的**，维护者从未直接下达"请拆分"的指令，但两轮逐文件 review 事实上逼出了它。

### 2.2 #659：作者**自己**关闭并拆分，理由是"内容 vs 引擎"

**原文（08-21 18:12，`feat_comments.json["659"]`）**：
> 「关闭本 PR：按讨论将拆分为「**剧本引擎通用能力 + DLC 识别接口**」的新 PR 重新提交
> （特效/小游戏事件、剧本退出清理、台词隔离、DLC 导入管理等）；
> **恐怖剧本内容本身不再随仓库发布，改以 DLC 包形式单独分发**。」

**动机里有两条完全不同的东西，别只看"拆分"**：
1. **技术拆分**：引擎通用能力 / 内容包；
2. **内容与素材不入库**：#659 的 body 自己写了版权说明——`Musics/` 与部分音效来自《Doki Doki Literature Club》，
   靠 "DDLC IP Guidelines 允许非商业社区二创" 兜底。**这是一个自伤风险点，作者主动撤掉了。**
3. 还有一条**真实的技术阻塞**在 body「已知问题」里：
   「本 PR 基于最新 `dev` 时，恐怖特效层在运行时不生效…定位修复前保持草稿」——**基底漂移导致功能失效**。

**前史**：#659 之前，作者先提了 **#656**（引擎能力部分）与 **#657**（内置剧本部分），两个都被关闭，
#659 是"合并成一个大 PR 重提"，然后再一次关掉重拆【证据 C3】。

### 2.3 #775~#778 之后：拆分并没有带来"好合"，因为切法错了

**这才是最有价值的一条。**

作者把 #659 的引擎部分重提为 **#677**（08-21 18:21，**就在关闭 #659 的 9 分钟之后**）。
结果是：**#677 = 129 文件 / +12977 / -404 / 47 commit —— 比原来的 #659（124 文件 / +6950）更大。**

T-Auto 在 08-30 的 review 里给出了**目前最明确的拆分指令**（原文，`feat_comments.json["677"]`）：

```
| 🟡 | 整个 PR | 124 文件 +13334 行，多组独立功能捆绑 | 拆分为"引擎能力"与"DLC 管理"两个 PR |
```
（注：该表格里维护者用的是 124/+13334，与 API 报的 129/+12977 有出入，
可能是维护者看的 diff 口径不同；**两个数字都照录，不替维护者解释**。）

**作者的回应（08-30 16:40，原文第 8 条）——拒绝拆分，并给了理由**：
> 「8. 关于拆分：**现有事件生命周期、schema/validator 和 DLC 事务共用同一状态机，未回写重排已有 PR 历史**；
> 本轮同步与审查修复已分别隔离为 merge commit 和 `e587f3ee`，**后续独立能力会继续拆 PR**。」

**从这条可以提炼出拆分的三条硬约束**：
1. **按"共享状态机"分界**：如果两块功能共用同一状态机/schema/事务，硬拆会制造跨 PR 依赖，**拆不动**；
   → 所以拆分应该**沿着状态机的边界**切，而不是沿着功能名字切。
2. **不要回写已有 PR 历史**：已经积累 47 个 commit、被 review 过的分支，重排历史成本极高（作者直接放弃）。
   → **要拆就趁早**（#695 是 8 天后才拆，只能在旧 PR 里留一句"已拆分"）。
3. **"引擎能力"不是一个可用单元**：它是个抽象层，不是一个能独立交付的功能，所以越拆越大。

**拆分失败的完整代价（#677 当前状态）**：open、`status: waiting`、129 文件被 15 条 review 意见围着、
最后一条评论是 09-05「已修改合并冲突，等待全量测试」。**8 天前建，至今未合**【证据 C3】。

### 2.4 拆分依据的三种候选，按实证排序

| 切法 | 实证支持 | 结论 |
|---|---|---|
| **按功能（每个可独立使用的功能一个 PR）** | #695 原文「按功能范围拆分为四个独立 PR」；#775 被表扬；T-Auto 对 #677 的指令是按"引擎能力 vs DLC 管理"两个**功能域** | ✅ **主切法** |
| **按风险/复杂度（最独立、最小的先提）** | #695 的四个里，**只有 +26 行的 #775 当天合**；#776（+2246）仍被叫"大型 PR"；#777/#778 被要 demo / 被 CHANGES_REQUESTED | ✅ **决定顺序** |
| **按前后端分** | **零实证**：9/9 已合并 feat 都是前后端同一个 PR【证据 A1】 | ❌ 不要 |
| **按抽象层（引擎/框架/内容）** | #659 试过 → #677 更大更卡 | ❌ 不要作为**唯一**切法（可作为"内容资产"的剥离依据，见 §2.2） |

### 2.5 结论：一万行、前后端都动的 feat，怎么切最可能被合

**三种切法对比**（用我们的世界模拟为例）：

**切法 A：按"功能垂直切片"切（推荐）**
每个 PR = 一个能单独演示、能单独发版的功能 + 它的后端命令 + 它的最小 UI 接线。
第一个 PR 不做定位、不做 AI，只做"能画出并走进一张街区图"。

- 优点：每个 PR 都能附 demo，符合"维护者会主动索要 demo"的现实；依赖是线性的；失败可以停在任何一个切片。
- 缺点：要忍受前几个 PR "功能不完整"（但项目里 #753、#671 都是这种小切片，完全被接受）。
- 实证：与 #695 的实际切法同构，且 #775 当天合并。

**切法 B：按"后端先行 / 前端跟进"切**
PR1 = 全部 Rust（数据模型 + 命令 + 测试），PR2 = 全部 Vue。

- 优点：PR1 的内部耦合最低。
- 缺点：**1/9 的已合并 feat 是这么做的**（0 个）。PR1 没有 demo 可给，维护者会问"这东西在哪能看到"；
  而且 #714 已经证明维护者会在意前端目录归属，前端延后交付等于把 review 拖成两轮。
- 结论：**不可取**。

**切法 C：按"依赖顺序/里程碑"切（P1 定位 → P2 地图 → P3 生成 → P4 集成）**
每个 PR 是一个技术里程碑。

- 优点：符合我们自己的开发顺序。
- 缺点：**"定位"这一刀是权限/隐私敏感面**（Android 权限、系统弹窗、后台定位），
  把它放第一个 PR，等于让维护者在最没看到价值的时候先评估最敏感的东西；
  而且 #778 的教训是——**落库位置/权限模型这类架构决定一旦选错，小 PR 也会被 CHANGES_REQUESTED**。
- 结论：可用，但**必须把"定位"往后放**，先交付零权限、纯本地、能看的东西。

**→ 推荐：A 为主切法，C 的顺序（零权限优先），B 只在"后端基础设施"内部作为一个 commit 段而不是一个 PR。**

---

## 3. 被合得最快的 5 个 feat 有什么共同点

**排序（创建→合并，`feat_pr_detail.json`）**：

| 排名 | PR | 耗时 | 文件 | +行 | commit | 功能数 |
|---|---|---|---|---|---|---|
| 1 | #715 恢复「模型 API 内置联网」选项 | **0.52 天** | 11 | +261 | 1 | 1（纯 revert） |
| 2 | #753 GPU 性能检测显示实际调用的 GPU | **0.65 天** | 9 | +206 | 9 | 1 |
| 3 | #714 OpenAI Codex 提供商 | **0.75 天** | 29 | +1983 | 7 | 1（大但单一） |
| 4 | #750 云端语音克隆 TTS | **1.18 天** | 32 | +2075 | 27 | 1 |
| 5 | #711 语音识别系统 | **2.19 天** | 53 | +6017 | 79 | 1（大但单一） |
| （对照）| #641 | 3.36 天 | 8 | +253 | 3 | 1 |
| （对照）| #671 | 4.91 天 | 11 | +246 | 1 | 1 |
| （对照）| #665 | 5.43 天 | 46 | +3358 | 15 | 1 |
| （对照）| #643 | 6.23 天 | 20 | +2166 | 16 | **多**（权限模式 + glob/grep + 媒体识别）|

### 共同点（按证据强度排序）

**① 单一功能，绝不夹带（9/9 全部满足）**
最快的五个各自只有一件事；最慢的 #643 是唯一一个 body 里列了 4 组能力的（"三档权限模式 + glob/grep +
ReadMediaFile + 管理员重启"），耗时 6.23 天，是 #715 的 12 倍【证据 D2】。

**② 先有 issue，而且 issue 本身就是设计规格（4/5 可查）**

| PR | 对应 issue | issue 创建 | PR 创建 | issue 里有什么 |
|---|---|---|---|---|
| #711 | **#650** | 08-19 | 08-25 | **完整 spec**：问题定位到 `GameDialog.vue:130-142`、方案表、**"零新增 Rust/npm 依赖"约束**、**体积账**、**目录树**（后端 6 文件 + `api/asr.rs` + 前端 5 文件）、**事件总线命名**、**"不在 v1 范围"清单**、**"~1800 行"估算** |
| #750 | **#667** | 08-21 | 08-29 | 背景 + 试点结论 + 后端/前端清单 + **验收标准** + **风险与成本**（¥0.8/万字符） |
| #714 | 未查到关联 issue（拿不到） | — | — | — |
| #715 / #753 | 未查到关联 issue（#715 是 revert，天然不需要规格） | — | — | — |

【证据 D1】issue #650 原文见 `feat_issues_detail.json["650"]`。
**#650 与 #711 的最终文件清单高度对应**（#650 画的 `asr/{mod,error,provider,vad,session,settings}.rs`
→ #711 实际交付 `asr/{mod,error,provider,provider_stream,provider_stream_llama,session,settings,vad,vad_segmenter}.rs`）。
但**不是逐项照做**：#650 里写「AsrProvider trait + **4 个实现**：OpenAI Whisper / Qwen ASR / Gemini / LAN Whisper」，
而 dev 分支的 `provider.rs` 里只有 **2 个实现**（`QwenAsrProvider`、`LlamaAsrProvider`）。
→ **issue 是规格，不是合同**：v1 砍掉一半 provider 照样 2.19 天合并；
真正起作用的是"issue 里写清楚了目录树和边界"。**"issue 里写清楚的目录树 = 无痛 review"**。

**③ 作者对 review 的响应是"当天"（3/5 可查）**
- #714：08-26 09:35 所有者提 3 条文件位置意见 → **作者 10:01（26 分钟后）**逐条回复"已按建议模块化收拢（df637e9f）"，
  并在 12:22 合并【证据 B1】。
- #665：08-24 09:21 `CHANGES_REQUESTED`（含"提供简单视频 demo 也是必要的"）→ 08-25 04:19~04:35 五条回复 →
  08-26 09:18 所有者 APPROVED「**很好的改动！现在代码逻辑很清晰了 鉴定为高质量 PR，接下来我会顺着继续维护这些代码**」【证据 D3】。
- #715：08-25 21:10 提 → 08-26 09:36 APPROVED「没问题捏」→ 09:37 合并。

**④ 作者是"重复贡献者"（强相关，但不是必要条件）**

| 作者 | 该仓库累计 merged PR | 在本样本中的表现 |
|---|---|---|
| sdfsfsk | **38** | #714(0.75d) / #715(0.52d) / #643(6.23d) / #659(自关) |
| lxdzh13 | **26** | #711(2.19d) / #750(1.18d) |
| Heiyahand | **8** | #753(0.65d) |
| Immaomao | 7 | #671(4.91d) |
| cafeawa | 3 | #641(3.36d) |
| **Slapq** | **1（就是 #665）** | 5.43 天合并 —— **新人也能合大 feat** |
| **CF-612** | **1（只有 #775）** | #695 拆四份，11 天只合了最小的那个 |

【证据 D4】→ **新人不是障碍（Slapq 首 PR 就合了 46 文件 / +3358 的 Live2D），
但没有画像积累时，"先给一个能一眼看完的小 PR" 是唯一可靠的开局**。

**⑤ 维护者 review 的"一句话门槛"很低，但要先通过"体量门槛"**
- 快的：`#711` 两条 `REVIEW APPROVED`「ok，很不错」（**0 条行内评论**）、`#671`「非常好的优化内，suki」、
  `#715`「没问题捏」、`#714`「大体没问题，就是文件放的位置不优雅，改改就行」。
  注意 #711 是**建 PR 后 21 小时就 APPROVED，但到第 2.19 天才真正 merge**——
  合并动作本身还有额外等待（可能是维护者的发版节奏），"被审"快 ≠ "被合"同样快。
- 慢/卡住的：`#665`（10 条行内 + 4 条总评）、`#677`（15 行表格 + 6 条 line comment）、`#695`（两轮共 3 大段）。
→ **维护者的 review 成本是自适应的：PR 越小，他给的反馈越短、越快。**

**⑥ 不约而同都避开了"顺手改既有行为"**
最快的 5 个里没有一个改动了既有功能的默认行为（#715 是恢复，不算）；而 #665 被点名的第一条正是
「本次更改存在一些其他逻辑更改，比如 input 状态本来设计的是显示玩家名字，但是我注意到代码更改改成了显示角色名称？」
【证据 D3】→ **顺手改既有行为 = 必被点名**。

---

## 4. 我们该怎么组织（结论性、可执行）

### 4.1 现状盘点（我们自己这边）

- **后端**：`src-tauri/src/world_map/`，**19 个 `.rs`，13,604 行**（其中 `facilities.rs` 2387、
  `transport.rs` 2191、`schedule.rs` 814、`mod.rs` 782、`render.rs` 754、`summary.rs` 736、`live.rs` 701 …）。
- **前端**：`src/components/views/worldsim/WorldSim.vue`、`src/components/views/worldmap/MapLibrary.vue`、
  `src/composables/{useWorldSim,useWorldSimGeo,useWorldMapLayer,useWorldMapBindings,useWorldModules,useWorldTick}.ts`、
  `src/api/services/worldMap.ts`、`src/assets/styles/worldsim.css`、`src/locales/zh-CN/worldsim.ts`。
- **提交历史显示单次提交已经在 1000~4700 行区间**：
  `bc79d51d` 17 文件 / +3899、`c5be77fa` 13 文件 / +4736、`4549ce16` 9 文件 / +1141、`482d3d0c` 8 文件 / +1039。
  → **照现在这样整段推上去，第一刀就是 4700 行，超过 #711 之后所有已合并 feat 的单 PR 体量中位数的 20 倍。**
- **风险项**：`.github/workflows/world-map-check.yml`、`build-android.yml` 的改动（**上游明确没有 CI 门禁**，
  15 号手册已核实 7 个 workflow 无 fmt/test/lint；带 workflow 改动的 #754 也经历了 52 文件的来回）。

### 4.2 切法：**6 刀，按"零权限 → 有权限"、"单体 → 集成"排**

命名沿用仓库惯例（`worldsim` 已是我们前端目录名，`world_map` 是后端目录名）。

| # | 标题建议（守 `feat:` + 中文说明 + scope） | 边界（文件） | 估算 | 依赖 | 里面对应的 issue |
|---|---|---|---|---|---|
| **P1** | `feat(worldsim): 街区网格地图原型——坐标/街区/设施分层与手机端渲染` | 后端 `world_map/{coord,maplib,blocks*,render}.rs` 的**必要子集**；前端 `views/worldsim/{WorldSim,DistrictMap}.vue`、`composables/useWorldSim.ts`、`api/services/worldMap.ts`、`locales/*/worldsim.ts`、`MainMenuOptions.vue`(+2)、`router/index.ts`(+10)；`docs/worldsim/README.md` | **~1300 行 / ≤28 文件** | — | **#804**（已有） |
| **P2** | `feat(worldsim): AI 生成街区——LLM 提示词/流式返回/草图兜底` | `world_map/{district_gen*,sketch,details}.rs` + `ai_service/tools/world_map.rs`；前端 `composables/useWorldSimGeo.ts` 的 AI 分支；`SettingsWorldSim.vue` | ~1500 行 | P1 | 需新开 issue（引用 #804 子项） |
| **P3** | `feat(worldsim): 设施路由与交通——从 A 点到 B 点的可达性` | `world_map/{facilities,transport}.rs`（**2387+2191 行，必须再砍**：先只做"设施接入 + 单向路径"，把 2191 行的多模式交通放到 P6） | ~1200 行 | P1 | 需新开 issue |
| **P4** | `feat(worldsim): 日程世界模拟——时间推进与 NPC 日程` | `world_map/{schedule,live,stream}.rs`；接 `api/schedule.rs` 既有通路（**优先复用，不新造**） | ~1200 行 | P1 | 需新开 issue |
| **P5** | `feat(worldsim): 地图摘要接入角色上下文——把"人在哪"喂给 LLM` | `world_map/{summary,state}.rs` + `ai_service/tools/` 接线；**这是全项目"为什么需要地图"的答案，建议提前**（见 4.4） | ~600 行 | P1 | 需新开 issue |
| **P6** | `feat(worldsim): 真实定位与地理下钻（可选开关）` | `world_map/{geo,osm,render_geo,loc_android,bridge}.rs` + Android 权限 + 隐私开关 | ~1500 行 | P1~P3 | 需新开 issue，**并且要先问所有者** |

**顺序**：`P1 → P5 → P2 → P3 → P4 → P6`。
理由：P1 立骨架；**P5 最便宜（600 行）且是"世界模拟对聊天有用"的唯一证据**，先合它能把后面的 PR 变成"扩展已验证能力"；
P2/P3/P4 是三个互不依赖的功能域，谁先合都不影响；**P6 最后，因为它触发权限与隐私 review**。

### 4.3 每个 PR 的"边界纪律"（照抄这一节）

1. **后端**：新文件一律进 `src-tauri/src/world_map/`；`api/` 下若新增命令**收敛成一个 `api/world_map.rs`
   或 `api/world_map/mod.rs`**（参照 `api/codex/mod.rs`、`api/asr.rs`）。
2. **既有文件只改接线**：`ai_service/mod.rs`、`api/mod.rs`、`lib.rs` 的注册行**各 ≤ 4 行**；
   任何一个既有文件改动超过 **+50 行**，就把那段逻辑抽到 `world_map/` 里再调用。
3. **前端**：新组件进 `src/components/views/worldsim/`；service 进 `src/api/services/worldMap.ts`；
   store 进 `src/stores/modules/`；i18n **四语种各一个文件**（`locales/{zh-CN,zh-HK,en,ja}/worldsim.ts`）。
4. **不塞 `*_selftest.py` / `frontend_selftest.js` / `page_smoke.js`**。要做 Rust 测试就学 #750：
   `world_map/xxx_test.rs` 独立文件 + 裸 `#[test]`（**不加 `#[cfg(test)] mod tests`**）。
5. **不动 `.github/workflows/`**。上游没有 CI 门禁，我们的 `world-map-check.yml` 对它没有价值，
   反而提供"改 CI"的吐槽面。**保留在 fork 里自用，不进 PR。**
6. **不顺手改既有行为**。任何"我顺便把 X 也改了"都单独开 PR 或干脆不做。
7. **文档**：`docs/worldsim/README.md` 起步（对应 `docs/live2d/README.md +18`），
   后续按需加 `architecture.md` + 可选 `diagrams/*.html`（对应 `docs/live2d/development.md +200`）。
   **文档可以单独一个 PR 先合**（上游 `dev-build.yml` 的 `paths-ignore` 含 `docs/**`，纯文档不触发 CI）。
8. **body 必须有 `## 验证`**（已合并 PR 里出现 13 次，排第一），写成可复制的命令 + 真实结果：
   ```
   ## 验证
   - npx vue-tsc --noEmit --skipLibCheck —— 通过
   - cargo check --manifest-path src-tauri/Cargo.toml —— 通过（仅仓库原有 warning）
   - 安卓真机（红米）装机：进入主菜单 → 世界模拟 → 街区图正常渲染，返回无残留
   ```
   ⚠️ 写**上游有的命令**（`pnpm build` / `cargo check` / `npx vue-tsc`），
   不要写我们自己的 `*_selftest.py`——那些文件不在 PR 里。
9. **demo 放第一条评论**，body 里不要塞图（15 号手册统计：30 个已合并 PR 的 body 里只有 2 个含图片）。
   P1 至少要有一张主菜单入口图 + 一段进入街区图的录屏（≤30s，手机竖屏）。
10. **每个 PR 先有 issue**。P2~P6 各开一个小 issue（用官方 `feature_request` 模板），
    内容照 #650 的结构写：问题/动机 → 方案表 → **目录树** → **行数估算** → **不在本次范围**。
    P1 直接用已有的 **#804**。

### 4.4 第一个 PR 应该小到什么程度

**目标：≤ 1 个功能、≤ 28 个文件、≤ 1500 行，维护者 10 分钟能读完、当天能点掉。**

实证锚点：
- 当天被表扬并合并的 #775 = **1 文件 / +26**；T-Auto 的原话是「拆的比之前好很多了，+26 / -1」。
- 已合并 feat 的体量分布（15 号手册）：additions 中位数 **226**、changed_files 中位数 **9**。
- 拆完之后仍被叫"大型 PR"的 #776 = **17 文件 / +2246**。
  → **1500 行是"不被叫大 PR"的安全线猜想；实际的安全线在 17 文件/+2246 以下，我们取 1500 行留余量。**
  （这是**推断**，不是维护者原话，标注为不确定项。）

**P1 必须包含**：
- 后端：`coord.rs`（坐标换算）+ `maplib.rs`（分层数据读写）+ **一个街区的最小渲染数据**（不接 AI）+ 1 个命令；
- 前端：主菜单入口（+2 行）+ 一个能看、能缩放、能点设施的页面 + 四语种文案；
- `docs/worldsim/README.md`（≤60 行）+ 一张截图 + 一段录屏。

**P1 必须不包含**：真实定位、Android 权限、AI 生成、日程/时间推进、交通路由、任何 workflow 改动。

**P1 的开场白建议**（直接抄 #715 的"给退路"结构 + #650 的"范围声明"）：

```markdown
## 概述
为 LingChat 增加一个可选的「世界模拟」地图层：把角色所在的位置画成一张可缩放的街区图，
让对话有"人在哪"这个维度。本 PR 只做**第一块砖**：网格坐标系统 + 街区图渲染 + 主菜单入口，
不涉及定位、不涉及 AI 生成、不新增任何依赖。

## 这是系列 PR 的第 1 个
后续计划（各自独立 PR，欢迎只合这一个）：AI 生成街区 / 设施路由 / 日程推进 / 位置注入上下文 / 真实定位。
关联 issue：#804

## 为什么值得合
（各 1 句：纯新增代码、零新增依赖、默认关闭、不动既有行为）

## 验证
（可复制命令 + 真机结果）

## 影响面 / 回滚
新增功能默认关闭；不改任何既有逻辑；删除新增文件即可完全回滚。
```

**P1 的红线（从 #804 的对话里来）**：
所有者在 #804（09-11）已经回过：
> 「有这个的想法，嗯你的主意很好，未来会参考的。**不过先做真实地理下钻这个还是有点逆天（**」

→ **P1 一定不要碰"真实地理下钻"**。这也正是把 P6（geo/osm/loc_android）排到最后的原因。

### 4.5 一句话执行清单

```
① 先提 docs/worldsim/README.md（纯文档，零 CI，最小成本"打个照面"）
② P1 = 网格街区图（≤1500 行 / ≤28 文件），body 带 ## 验证，首条评论带录屏
③ 维护者给意见 → 当天回复（#714 用了 26 分钟，#665 用了 19 小时）
④ P1 合并后再开 P5（位置注入上下文，600 行，最便宜的价值证明）
⑤ 之后 P2/P3/P4 任意顺序；P6（真实定位）最后，且先问所有者
⑥ 全程：不加测试脚手架、不动 workflows、不顺手改既有行为、新代码全进 worldsim/ 或 world_map/
```

---

## 5. 拿不到 / 不确定的数据

| 项 | 状态 |
|---|---|
| #714 是否有关联 issue | **拿不到**。GitHub 搜索 API 单独限速 30 次/分钟，本轮额度被 issue 列表查询耗尽；#714 body 中未出现 issue 编号。 |
| #715 / #753 是否有关联 issue | **拿不到**，同上。#715 是 `git revert`，天然无需规格；#753 的 issue 可能是 #387「基于 CPUID 的轻量性能分级检测」（同一作者 Heiyahand，主题相邻），**但没有直接链接证据，不作为结论**。 |
| **#650 与 #711 的对应关系** | 已核对：目录树对应，但 provider 数量从 issue 里的 4 个缩到实际 2 个（见 §3② 修正说明）。 |
| #695 是否由维护者明确要求拆分 | **未找到这样的原话**。维护者两轮 review 均为逐文件技术意见；"按功能范围拆分"出自作者 CF-612 自己的评论。**不能写成"维护者要求拆分"**。 |
| #677 维护者 review 与实际 diff 数字不一致 | 维护者表格写「124 文件 +13334 行」，API 报 `129 文件 / +12977 / -404`。**口径不同，本文两者都照录，不解释原因。** |
| #695 拆分后合并率 | 截至采集（数据止于 open PR 的 `updated_at` 最大值）：**4 个里只合了 #775**，#776/#777/#778 均 open 且 11 天无新动作。**"拆分后是否更好合并"目前只有"最小的那个合了"这一个结论，样本量 1，不足以断言因果。** |
| #659 拆分后的后续 PR 是否还有更多 | 只找到 #677 一个（由 body 与时间戳推断）。**不排除有其它未关联提及的后续 PR。** |
| 维护者名单与权限 | `collaborators.json` 返回 **4 字节（空 `[]`）**，拿不到。评审主体从评论的 `author_association` 读出：`SlimeBoyOwO`(OWNER)、`T-Auto`(COLLABORATOR)。 |
| "1500 行是安全线" | **推断**，非维护者原话。实证只有：+26 当天合、+2246 被称"大型 PR"、已合并 feat additions 中位数 226。 |
| 本仓库 issue 总数与 open PR 全量 | issue 侧只按 `type: feature request` 抽样（110 条）；PR 侧为 `pulls_closed.json`(300) + `pulls_open.json`(26)。**两处都可能不是全量**（分页上限）。 |

---

## 附：本轮新增的采集脚本（只读，可复跑）

| 文件 | 作用 |
|---|---|
| `~/lingchat-pr-research/feat_probe.py` | 按 PR 号拉 PR 详情（body/规模/commit 数/labels/author_association），写入 `feat_pr_detail.json`，**幂等，已有的跳过** |
| `~/lingchat-pr-research/feat_comments.py` | 拉 issue_comments + review_comments + reviews + commit 列表，写入 `feat_comments.json` / `feat_commits.json` |
| `~/lingchat-pr-research/feat_files.py` | 拉每个 PR 的 changed files（name/status/additions/deletions），写入 `feat_files.json` |
| `~/lingchat-pr-research/feat_issues.json`、`feat_issues_detail.json` | `type: feature request` 的 issue 列表与重点 issue 详情（#650/#667/#684/#804） |

全部为 GET；token 从 `~/.dsh/.credentials.yaml` 读取，**未打印明文**；
本轮结束后 API 限额剩余约 **4,950/5,000**（搜索 API 小额受限）。
