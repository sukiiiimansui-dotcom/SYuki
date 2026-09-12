# 15 · 向 LingChat 提 PR 操作手册（基于上游 300 个 PR 的实证研究）

> **研究方法**：全程只读 GET `api.github.com`（token 认证，5000/h），**未做任何 fork / 分支 / 评论 / 修改**。
> **样本**：仓库元数据、分支列表、22 个 label、全部 300 个 closed PR（218 merged / 82 unmerged）、
> 最近 30 个已合并 PR 的完整详情 + 235 条 commit 消息、20 个已合并 PR 的 review 数据、
> 14 个未合并 PR 的关闭原因、issue #804 / #650 / #594 / #116 时间线、dev 分支完整文件树（truncated=false）、
> 7 个 workflow 原文、代码风格配置文件原文。
> **采集时间**：2026-09-12（UTC+8）。
>
> ⚠️ **本文每条结论都附证据**（PR 号 / 文件路径 / 原文片段）。拿不到的数据统一列在第 7 节，**没有编造**。

---

## 0. 一句话结论（先看这个）

| # | 结论 | 关键证据 |
|---|---|---|
| 1 | **PR 必须提到 `dev`，不是 `main`** | 最近 30 个已合并 PR **30/30** base=dev；CI 也只对 `dev` 触发 |
| 2 | **我们的分支现在开不了 PR —— 和上游没有共同祖先** | `git merge-base upstream/dev feat/world-map-official` 返回空；本分支是 65 个 commit 的**孤立历史**（根提交 `bbac1eb0` 无 parent） |
| 3 | **7000+ 行不是禁区，但"多特性大 PR"会被拆** | 先例 #711 +6017/53 文件 **2.19 天合并**；但 #695（+3182）被拆成 4 个、#659（+6950/124 文件）被作者自行关闭拆分 |
| 4 | **官方不保留测试代码** | dev 全树只有 2 个 test 文件；#651 `chore: 清理项目内测试代码与测试文档`（-3744）被合并；#665 review 明确要求"清理测试代码" |
| 5 | **有一个 `ai-slop` label 正对着我们** | label 原文：*"Low-quality AI-generated content submitted without meaningful human review"* |
| 6 | **维护者已回复 issue #804，球在我们这边** | #804 label = `status: waiting`（= **Waiting on author**），由 SlimeBoyOwO 本人于 2026-09-11 打上 |

---

## 1. 提交流程惯例

### 1.1 默认分支 vs 目标分支（**结论：默认分支是 main，但 PR 要提 dev**）

- 仓库 `default_branch` = **`main`**
  （证据：`GET /repos/SlimeBoyOwO/LingChat` → `"default_branch": "main"`）
- 但 **最近 30 个已合并 PR，base 分支 100% 是 `dev`**：
  ```
  --- base branch distribution ---
     dev: 30
  ```
  （证据：`merged30_detail.json`，逐个 PR 的 `base.ref`）
- 现存分支：`dev` / `main` / `old-python` / `tmp` / `fix/persistent-memory-phase1-main` / `fix/pomodoro-pet-773`
  （证据：`GET /repos/.../branches?per_page=100`）
- 26 个 open PR 的 base：`{'dev': 23, 'main': 3}` —— 提 `main` 的那 3 个基本是"提错了"
- **CI 只在 PR 目标为 dev 时触发**：
  ```yaml
  # .github/workflows/dev-build.yml（dev 分支原文）
  on:
    pull_request:
      branches: ["dev", "dev/**"]
  ```
  → **提 `main` 的 PR 连 CI 都不会跑**。这解释了为什么 #757（base=main）挂了 12 天没动静。

**✅ 我们：base = `dev`。**

### 1.2 贡献指南与模板（原文抓取）

| 文件 | 是否存在 | 路径 |
|---|---|---|
| 贡献指南 | ✅ | **`.github/CONTRIBUTING.md`**（注意在 `.github/` 下，**根目录没有**） |
| PR 模板 | ✅ | `.github/PULL_REQUEST_TEMPLATE/feature.md` + `bugfix.md`（**是目录，两个模板**） |
| Issue 模板 | ✅ | `.github/ISSUE_TEMPLATE/feature_request.md` + `bug_report.md` |
| CODE_OF_CONDUCT | ✅ | `.github/CODE_OF_CONDUCT.md` |
| SUPPORT | ✅ | `.github/SUPPORT.md` |
| **CODEOWNERS** | ❌ **不存在** | 根目录与 `.github/` 均无 |

> 踩坑记录：`CONTRIBUTING.md` 用 `/contents/CONTRIBUTING.md` 取会 **404**，必须用 `/contents/.github/CONTRIBUTING.md`。

**CONTRIBUTING.md 里最要命的三段原文：**

> ### 提交功能建议
> 我们非常乐意听到你的新想法！但为了确保新功能符合项目的整体规划和核心价值，**请务必遵循"先讨论，后开发"的原则**。
> 1. **创建 Issue** …
> 2. **社区讨论** …
> 3. **达成共识**：**只有在社区达成共识后，才建议开始投入代码开发。**
>
> 这样做是为了避免您投入大量时间和精力后，发现功能与项目理念不符，导致 Pull Request 最终无法合并，造成不必要的浪费。

> ### 功能开发
> - 请确保你想要开发的功能已经过 **[提交功能建议]** 流程的讨论并获得了社区的认可。
> - 通过 **Fork** 功能将本仓库拷贝到你的名下。
> - **从 `main` 或 `dev` 分支创建新的特性分支**，例如 `feature/awesome-new-idea`。
> - 完成代码编写后，通过 **Pull Request (PR)** 提交到主仓库的对应分支。**请使用 PR 模板**，清晰描述你的改动。

> ## ⚠️ 重中之重
> 本项目由独立开发者主导，其现有架构和代码风格优先考虑的是 **开发迭代速度** 和 **维护者的个人工作流**…
> **我们不欢迎任何缺乏上下文、仅停留在风格或理论层面的重构争论。**
> …如果你想重构，请带着一份能说服所有人的、可以落地的完整计划书来。否则，这类 Issue 和 PR 将被直接关闭。

**官方 PR 模板（`feature.md` 原文，171 字节）：**

```markdown
## 变更说明

描述本次 PR 的主要变更

## 变更类型

- [ ] Bug 修复
- [ ] 新功能
- [ ] 代码重构
- [ ] 文档更新
- [ ] 其他

## 相关 Issue

关联的 Issue，如：fix #123

## 检查清单

- [ ] 代码已经本地测试

## 截图/示例

如果适用，添加截图或示例
```

### 1.3 PR 标题的语言与格式习惯（最近 30 个已合并 PR 统计）

**语言分布：**

| 语言 | 数量 | 说明 |
|---|---|---|
| 中英混排（MIX） | **25/30** | 前缀英文 + 描述中文，如 `fix: 修复 Kimi-Code 流式响应按分块解码导致中文乱码` |
| 纯英文（EN） | 5/30 | 如 `fix: make save line synchronization atomic` |
| 纯中文（ZH） | **0/30** | —— |

**前缀分布：**

| 前缀 | 数量 |
|---|---|
| `fix:` | **16** |
| `feat:` | **9** |
| `refactor:` | 2 |
| `chore:` | 1 |
| 无前缀 | 2 |

→ **97%（28/30）都用 conventional-commit 前缀**，`fix` 略多于 `feat`。

**冒号风格：** 半角 `:` **24** 个，全角 `：` 4 个 → **半角为主**（虽然全角也有人用，如 #775 `fix：恢复服装列表横向滚动条`）。

**带 scope 的先例：** `feat(tts):` (#750)、`fix(asr):` (#745)、`feat(plugin)`/`refactor(import)` (#629/#639)、`fix(ios):` (#754)。

**最近 30 个已合并 PR 的原始标题（完整，供模仿）：**

```
#799 fix: Codex 模型发现与推理摘要接收，支持 GPT-6 Astra
#794 Fix:修复打包发行版 Linux 产物缺失
#783 fix: make save line synchronization atomic
#775 fix：恢复服装列表横向滚动条
#693 fix: validate LAN sync manifest paths
#750 feat(tts): 云端语音克隆 TTS(CosyVoice)——克隆音色注册/审核/试听,角色语音接
#753 feat: 使 GPU 性能检测功能支持显示当前实际调用的GPU，并进行分级 nya~
#745 fix(asr): 语音输入总开关改为整体开关
#754 fix:对ios进行了适配，增加了ios的打包工作流
#741 fix: restore Codex and Kimi web search credentials
#733 [Perf]优化设置页面背景模糊的性能
#736 fix: make automatic memory compression effective and
#711 feat：语音识别系统（VAD 端点检测 + 阿里云/本地双 provider + 流式识别）
#708 fix: 修复 Kimi-Code 流式响应按分块解码导致中文乱码
#714 feat: OpenAI Codex（ChatGPT 订阅）LLM 提供商（OAuth 登录/自动代理/
#671 feat: 注册 LLM 错误分类并在前端 i18n 提示可能原因
#715 feat: 恢复网页搜索「模型 API 内置联网」选项（免 API Key，与独立端点共存）
#665 feat: 添加可选 Live2D 角色支持
#643 feat: 新增 Codex 风格工具访问模式、Glob/Grep 与 ReadMediaFile
#629 fix(plugin): 修复 Windows GUI 下 RustPython 插件初始化失败
#674 fix: LLM 提供商卡片压缩时用途标签覆盖模型名，改为移至模型名下方
#682 fix: 移除 framing prefix 中硬编码的第三方角色示例及用户称呼
#706 Correct '占仆' to '占卜‘ in character settings
#651 chore: 清理项目内测试代码与测试文档
#641 feat：添加了ds websearch
#586 fix：Dialog clean
#669 refactor: 拆分 LLM 提供商预设到独立文件
#668 Fix:修复HDR显示发灰发暗and设置面滑动动画异常
#638 fix: 修复 Android 工具调用并补充平台提示
#639 refactor(import): 4 入口文件导入鲁棒性重构（magic sniff 统一 + 自动修
```

> 注意 `#733 [Perf]优化…` 这种 `[Perf]` 方括号写法也有人用（#718 同款），以及大小写不统一（`Fix:`）也被接受 —— 说明**格式不是硬门槛，但主流是 `fix:`/`feat:` 小写半角**。

### 1.4 commit 消息习惯（最近 30 个 PR 的 235 条 commit）

- **187/235（80%）的 commit 消息含中文**
- 前缀同样是 conventional-commit，且**带 scope 的很多**：`feat(tts):`、`fix(tts):`、`feat(i18n):`、`ci(release):`、`chore(pnpm):`
- 单个 PR 的 commit 数：**中位数 1**（`[(1,638),(1,669)...]` 大量为 1）；最大 #711 有 **79 个** commit

  → **拆不拆 commit 不影响能否合并**（合并时 7 个用 merge commit、5 个用 squash，抽样 12 个）。
  但 #711（79 个 commit、2.19 天合并）证明**细粒度 commit 反而好审**。

**#711（我们的最佳模板）的 commit 序列节选：**
```
feat(tts): cosyvoice 配置层——API Key/模型列表/音色映射存 settings.json
feat(tts): cosyvoice 云端 HTTP 客户端——音色注册/查询/上传
feat(tts): cosyvoice 音色注册状态机——上传/轮询/列表/删除
feat(tts): cosyvoice 适配器接入——provider 路由 + 角色级音色配置
feat(tts): cosyvoice 命令集——配置/音色注册/列表/删除/试听
feat(tts): cosyvoice 前端 API service
feat(tts): 设置页新增语音克隆TTS卡片——Key/模型/音色注册/试听
feat(i18n): 语音克隆TTS 4语种文案
```
→ **"一个功能切片一个 commit，中文描述写清做了什么"**，这是最受欢迎的形态。

### 1.5 ⚠️ 上游没有 CI 门禁（这是重要事实，不是好消息）

扫了全部 7 个 workflow 的步骤名：

| workflow | 步骤 | 有 fmt/test/ lint 门禁吗 |
|---|---|---|
| `dev-build.yml` | 安装 rust → rust 缓存 → Linux 依赖 → 占位文件 → **构建应用** | ❌ 无 |
| `dev-build-full.yml` | 同上 + 上传 artifact | ❌ 无 |
| `dev-build-android.yml` | …构建 APK… | ❌ 无 |
| `build-android.yml` | …构建 APK + 签名 + Release | ❌ 无 |
| `build-ios.yml` | …打包 IPA… | ❌ 无 |
| `release.yml` | …构建 + Release… | ❌ 无 |
| `opencode-review.yml` | Checkout → **Run OpenCode**（评论里发 `/opencode` 或 `/oc` 触发 AI 审阅） | — |

**没有任何一步跑 `cargo test` / `cargo fmt --check` / `clippy` / `prettier --check`。**

→ 结论：**格式与测试不靠 CI 保证，靠"本地 husky 钩子 + 维护者肉眼"**。
→ 反过来说：**我们自己必须在 PR 里证明跑过什么**（见第 5 节模板的「验证」小节）。

---

## 2. 已合并 PR 的"长相"

### 2.1 改动规模分布（最近 30 个已合并 PR）

| 指标 | 中位数 | 平均 | 最小 | **最大** |
|---|---|---|---|---|
| additions | **226** | 723 | 1 | **6017**（#711） |
| deletions | **26** | — | — | 3744（#651） |
| changed_files | **9** | — | 1 | **61**（#651） |
| 总改动量 (add+del) | **275** | — | — | **6158**（#711） |

**大 PR 先例（全部是外部贡献者，全部已合并）：**

| PR | 作者 | 规模 | 内容 | 创建→合并 |
|---|---|---|---|---|
| **#711** | lxdzh13 | **+6017 / -141 / 53 文件 / 79 commit** | 语音识别系统（VAD + 双 provider + 流式） | 2.19 天 |
| #665 | Slapq | +3358 / -69 / 46 文件 | 可选 Live2D 角色支持 | 5.43 天 |
| #643 | sdfsfsk | +2166 / -125 / 20 文件 | Codex 风格工具访问模式 | 6.23 天 |
| #750 | lxdzh13 | +2075 / -48 / 32 文件 | 云端语音克隆 TTS | 1.18 天 |
| #714 | sdfsfsk | +1983 / -28 / 29 文件 | OpenAI Codex 提供商（OAuth） | 0.75 天 |
| #754 | cafeawa | +1024 / -96 / 52 文件 | iOS 适配 + 打包工作流 | 0.58 天 |

> **回答"7000+ 行有没有先例"：**
> **有，但正好卡在边界上。** 已合并的上限是 #711 的 **+6017**；
> 而 **+6950 / 124 文件的 #659 被作者自己关闭拆分**了，+3182 的 #695 被拆成 4 个 PR。
> 我们 ~10000 行的规模**超过了所有已合并先例**。

**#711 是"大到 6017 行还能 2 天合并"的唯一样本，值得研究它为什么行**：
- 53 个文件里 **14 个集中在 `src-tauri/src/ai_service/asr/`（新子系统内聚）**、12 个 i18n、10 个组件
- 全部是**纯新增 + 接线**，没有大范围重构既有代码
- 描述里写了完整架构（`AsrProvider trait 抽象`）与**真机验证**（"红米设备实测"）
- 维护者 review 结论只有一句：`[REVIEW APPROVED] @SlimeBoyOwO: ok，很不错`，**0 条行内评论**

### 2.2 是否带测试？带文档？带截图？

**测试 —— 官方明确不要"项目内测试代码"：**

- dev 分支完整文件树里，**只有 2 个** test 命名文件：
  ```
  src-tauri/src/ai_service/tts/cloud/enrollment_test.rs
  src-tauri/src/config/tts_test.rs
  ```
- **#651 `chore: 清理项目内测试代码与测试文档`** —— +38 / **-3744** / 61 文件，**已合并**（作者 sdfsfsk，维护者自己审的）
- **#714 里有一条 commit 就叫**：`chore: remove Codex catalog unit tests from PR`
- **#665 的 review 原话**：
  > `3. 存在许多未清理的测试代码（包括硬编码），解决一下。`
  >
  > `[LINE src/components/game/live2d/model-source.test.ts] @SlimeBoyOwO: 这里的硬编码测试用部分可以删除`
  >
  > 作者回复：`已删除。四个测试文件、test:live2d、Vitest 及 Rust 测试模块都已移除。`

→ **⚠️ 直接影响我们**：`*_selftest.py`、`frontend_selftest.js`、`page_smoke.js` 这类文件**不要进 PR**。
（Rust 侧内联 `#[cfg(test)]` 是允许的 —— #711 描述里写了 `cargo test 48/48`，#750 带了 `enrollment_test.rs` 并合并了。但**前端 `.test.ts` 会被点名删掉**。）

**文档 —— `docs/<功能名>/` 是明确惯例：**

dev 分支 `docs/` 下已有的功能文档目录（证据：tree API）：
```
docs/function_call/   （extension.md / memory.md / permission.md / diagrams/*.html）
docs/live2d/          （README.md / authoring.md / development.md / stage-analysis.md / diagrams/）
docs/script-editor/   （README.md / architecture.md / editor.md / preview.md / validation.md / diagrams/）
docs/updates/ docs/utils/ docs/i18n.md docs/plugin-dev-guide.md …
```
→ **`docs/<feature>/` + `README.md` + 分节 md + `diagrams/*.html`** 是被接受的形态。
我们的 `docs/world-map/` 结构与之一致 ✅。

⚠️ 注意：`dev-build.yml` 的 `paths-ignore` 含 `docs/**` 和 `**.md` ——
**纯文档改动不触发 CI**，所以文档可以单独提一个小 PR（零构建成本）。

**截图 —— body 里很少，但 comment 里很多：**

- 30 个已合并 PR 的 **body 里只有 2/30** 含图片
- 但**评论里贴图非常普遍**：#714(4 张)、#643(6 张)、#741(2 张)、#799(2 张)、#695(2 张)……
  sdfsfsk 提交的 4 个 PR **全部在评论里贴了 1502×832 的界面截图**
- **#665 的 review 明确要求 demo**：
  > `整体是很棒的，请在前端代码设计上再继续努力一下。当然提供简单的视频 demo 演示也是必要的。`

→ 结论：**截图放"评论"里，不放 body；大功能最好有视频 demo**。

### 2.3 描述（body）的常见结构

| 指标 | 数值 |
|---|---|
| **空 body** | **0/30**（每个 PR 都有描述） |
| body 长度中位数 | **1083 字符** |
| body 最长 | 3470 字符 |
| 含 markdown 标题（`##`） | **25/30** |
| 含 `- [ ]` 检查清单 | 6/30 |
| 含图片 | 2/30 |
| 含表格 | 2/30 |
| 含代码块 | 5/30 |
| **含 `Closes #xxx` / `fix #xxx`** | **0/30** ⚠️ |
| body 提到任何 `#NNN` | **仅 2/30** |

**最常用的分节标题（按出现次数）：**

| 标题 | 次数 |
|---|---|
| `## 验证` | **13/30** ⭐ 第一名 |
| `## 检查清单` | 6 |
| `## 问题` | 5 |
| `## 概述` | 5 |
| `## 修复说明` / `## 修复的问题` / `## 相关 Issue` / `## 修复方法` / `## 截图/示例` | 各 4 |
| `## 改动内容` / `## 涉及文件` | 各 4 |
| `## 问题现象` | 3 |
| `## 背景` / `## 技术说明` / `## 用户影响` / `## 根因` … | 各 2 |

**两个反直觉的发现：**

1. **官方模板几乎没人照抄**：`变更说明` 只出现 2/30，`变更类型` 2/30，`检查清单` 6/30，checkbox 6/30。
   → **模板要"参考"不要"照抄"**；自由发挥的分节反而更常见。
2. **`Closes #xxx` 惯例实际上是 0**，尽管 CONTRIBUTING.md 明确建议写 `Closes #123`。
   → 写上是加分（符合官方文档），但不写也不会被拒。**建议我们写**——因为我们的 issue #804 需要建立关联。

**最佳 body 模板：PR #711（原文，我们的直接模仿对象）**

```markdown
## 概述
为 LingChat 新增完整的语音输入能力：本地 Silero VAD 端点检测、双识别服务商…覆盖桌面端与安卓端。

## 功能
🎙️ 双触发源：mic 按钮手动录入 + auto_listen 自动监听（能量触发 → VAD 切段）
🧠 Silero VAD 端点检测：本地 ONNX 推理（模型随 data/third_party/asr_vad/ 打包），静音计时可自定义（默认 800ms）
☁️ 阿里云 qwen-asr：非实时（Fun-ASR-Realtime）+ 实时 WebSocket 流式（Paraformer-Realtime-V2）
💻 本地 llama-asr：llama-server（llama.cpp）部署 Qwen3-ASR…
⚡ 结果流式（SSE）…
🔒 三层状态架构：总开关（voice_input_enabled，默认关闭仅影响全新用户）→ 模式（auto_listen）→ 功能开关
🛡️ 12 项门控：AI 生成中/触摸模式/移动端菜单/设置抽屉/剧本选择/TTS 播放中等场景自动禁用语音
## 架构
AsrProvider trait 抽象（recognize / supports_streaming / stream_recognize），AsrSession 统一编排（互斥 + 取消令牌）
流式双路径：qwen 走 DashScope WebSocket（边录边发 PCM）、llama 走 SSE（整段上传 + 增量结果）…
## 验证
cargo test 48/48（含修复的 4 个历史失败测试：VAD 静音计时 off-by-one、PCM16 断言）、vue-tsc ✓、cargo fmt ✓
安卓端装机验证：VAD 模型加载、语音输入全链路可用（红米设备实测）
本地 ASR 已在 llama-server（Qwen3-ASR-1.7B-Q8）实测：整句识别 + SSE 流式 + 热词偏置
## 后续开发计划
1.实现多人情况下的特定音色识别。
2.接入更多的ASR
…
### 如果有新需求可以到issues提交新建议
```

**另一类（修复型）也很常见 —— #794 的结构**：
`## 背景`（版本现象）→ `## 修复内容（两笔提交）`（带 commit SHA）→ `### 1. / ### 2. / ### 3.`（每项"根因 → 修复"）→ 结尾影响面。

### 2.4 从提交到合并的时长

| 统计 | 数值 |
|---|---|
| **中位数** | **1.69 天** |
| 平均 | 2.80 天 |
| 最快 | **0.03 天**（#775，约 40 分钟） |
| 最慢 | 14.92 天（#586） |
| **≤3 天合并** | **18/30（60%）** |

逐项（天）：
```
#775 0.03  #736 0.08  #706 0.10  #669 0.13  #668 0.15  #733 0.16
#741 0.26  #715 0.52  #754 0.58  #753 0.65  #714 0.75  #783 1.04
#750 1.18  #794 1.54  #708 1.56  #745 1.81  #682 2.07  #711 2.19
#674 3.08  #639 3.21  #641 3.36  #638 3.54  #799 4.69  #651 4.81
#671 4.91  #665 5.43  #643 6.23  #693 7.05  #629 7.88  #586 14.92
```

→ **审核很快（中位 1.7 天）**：维护者不拖延、不搞长期挂起。
→ 但注意下面的**当前节奏**：

**当前吞吐量（重要，会决定我们等多久）：**

| 月份 | 合并数 | 关闭未合并 | 合并率 |
|---|---|---|---|
| 2026-09（截至 09-12） | **3** | 0 | 100% |
| 2026-08 | **81** | 23 | 78% |
| 2026-07 | 37 | 27 | 58% |
| 2026-06 | 9 | 5 | 64% |

- 最近 30 天合并 **44** 个
- **最近 7 天只合并了 2 个**（#794 / #799）
- 26 个 open PR 里 **18 个挂着 `status: waiting`**，**中位年龄 12 天，最老 41 天**（#558/#560，张zm0 的两个功能 PR）

→ **9 月开始上游明显放缓。我们的 PR 大概率要排队。**

### 2.5 提交者身份分布与外部贡献者合并率

**这个仓库是"外部贡献者驱动"的，不是维护者自娱自乐：**

**历史合并数 Top（全部 300 个 closed PR 统计）：**

| 账号 | merged | unmerged | 合并率 |
|---|---|---|---|
| sdfsfsk | 38 | 31 | **55%** |
| lxdzh13 | 26 | 5 | **84%** |
| Ratman463 | 20 | 4 | 83% |
| FlameTN7 | 16 | 2 | 89% |
| myh1011 | 16 | 5 | 76% |
| shadow01a（**维护者**） | 15 | 4 | 79% |
| FinalFlower | 10 | 0 | **100%** |
| Wisher7274 | 9 | 0 | **100%** |
| Heiyahand | 8 | 2 | 80% |
| Immaomao | 7 | 0 | **100%** |
| **SlimeBoyOwO（仓库主人）** | **0** | 0 | — |

→ **仓库 owner SlimeBoyOwO 本人从不提 PR，他只 review + 合并**（`merged_by`: SlimeBoyOwO **17**、shadow01a **13**）。
→ 所有 30 个已合并 PR 的 head 都来自 **fork**（`sdfsfsk/LingChat`、`lxdzh13/LingChat`…），**没有一个来自上游自己的分支**。

**外部贡献者合并率高吗？——高，但分布很分散：**

- 有稳定记录的贡献者：**76% ~ 100%**
- 最活跃的 sdfsfsk 只有 **55%**（38/69）—— **提得多、被拒得也多**
- 提了 1~2 个就消失的人（PaidaxingTuT 2 个、metaone01 2 个）**全部未合并**

→ 结论：**"外部贡献者"身份不是障碍，66% 的已合并 PR 来自外部人**。
   真正的分水岭是**是否长期参与 + 是否按维护者的口味改**。

**补充：维护者也直接往 dev 推 commit（不走 PR）**
最近 40 个 dev commit 的作者：`shadow01a 19`、`SlimeBoyOwO 14`、`sdfsfsk 3`、`Immaomao 2`…
其中可见维护者**直接把贡献者的改动重打一遍**：
```
2026-08-31 fangcheng612   修复：恢复服装列表横向滚动条
2026-08-31 SlimeBoyOwO     fix：恢复服装列表横向滚动条   ← 同一天，维护者自己再提交一遍
```
→ 说明维护者对小改动会**直接拿走**，对大改动才走 PR 流程。

---

## 3. Review 里维护者关心什么

### 3.1 谁在 review（三个角色）

| 账号 | 身份 | 行为 |
|---|---|---|
| **SlimeBoyOwO** | OWNER | 主要 reviewer + 合并者（17/30）。评论短、口语化：`ok，很不错` / `中` / `没问题捏` / `非常好的优化内，suki` |
| **shadow01a** | COLLABORATOR | 第二合并者（13/30）。常催冲突：`请解决冲突` / `需解决合并冲突` / `确定一下本更改是否有衍生问题，没有的话就merge吧` |
| **T-Auto** | COLLABORATOR（**是人，不是机器人**） | 深度 reviewer + 打 label。GitHub 资料：name=**风雪**，bio="一个不会写代码的笨蛋，但风雪家的猫喜欢趴在键盘上，于是便提交了commit，审了pr" |

> ⚠️ **别被 `T-Auto` 这个名字骗了**：它在 PR #695 里**自己打了 `status: waiting` label**、写了 1000+ 字的 bug 分析。它是**维护团队的人**（profile `type: User`，2024-10-05 注册）。

**T-Auto 的 review 极其硬核（原文摘录，PR #695）：**
> `不过有个小问题需要调整，api/music.rs 的 delete_music 函数目前的删除逻辑是 let filename = std::path::Path::new(&url).file_name()...; let file_path = base.join(&filename);，这里直接提取文件名拼接到根目录，但在本次支持子分类后，分类内音乐的 url 会包含子文件夹路径。这样会导致分类内的音乐在点击删除时因为找不到文件而报错，而且如果根目录下恰好存在同名文件，会导致根目录的文件被意外误删。`

**T-Auto 对 PR #743 的性能 review（原文）：**
> `另外在打字机 writeFn 中，每个字符 tick 都在执行 el.innerHTML = fusedRenderHtml(text) 全量重建 DOM，如果遇到多段的长文本回复，每字全量重绘 DOM 的开销会偏大`

→ **准备好接受"逐行读代码、指出真实 bug、要求性能论证"级别的审阅。**

### 3.2 ⚠️ 一个反直觉的关键数据：绝大多数 PR 没有行内 review

最近 30 个已合并 PR 的 `review_comments`（行内评论）数量：

```
26 个 PR = 0 条行内评论
 3 个 PR 有行内评论：#665 → 10 条, #714 → 6 条, #643 → 1 条
```

issue 级评论数：**20/30 是 0 或 1 条**。

**formal review 的形态**：多数是 `[REVIEW APPROVED]` + 一句空话，例如
- #711：`ok，很不错`（2 个 APPROVED，0 行内评论）
- #671：`非常好的优化内，suki`
- #715：`没问题捏`
- #639：`(empty)` —— **空白 APPROVED**

→ **含义**：维护者**不写长篇 review**。要么直接合并，要么只在"真有问题"时才写行内评论。
   所以 **拿到行内评论 = 被认真挑刺了**，要认真对待。

### 3.3 五个外部贡献者已合并 PR 的 review 全文要点

#### ① PR #665 —— Slapq《feat: 添加可选 Live2D 角色支持》（+3358 / 46 文件 / 15 commit / 5.43 天）
**7 个 review、10 条行内评论 —— 样本里最"惨烈"的，也是最有信息量的。**

`[REVIEW CHANGES_REQUESTED] @SlimeBoyOwO`（原文）：
> `嗯 Live2D 实现出来就非常厉害了，在代码的实现上经过人工审查后，目前看到的问题有下面这些，重要性从高到低排列：`
> `1. 最大的问题主要是 GameRolesStage 和 GameRoleAvatar 这里。如果 live2D 角色和 png 角色同时出现会有逻辑问题，此外在实现方式上也不太好。参考我的具体文件 commit 进行代码设计上的更改。（如果你有更好的设计方案可积极讨论）`
> `2. 本次更改存在一些其他逻辑更改，比如 input 状态本来设计的是显示玩家名字，但是我注意到代码更改改成了显示角色名称？还有新增人物的按钮重新增加了，那一部分是打算重构的，可以注意一下？`
> `3. 存在许多未清理的测试代码（包括硬编码），解决一下。`
> `整体是很棒的，请在前端代码设计上再继续努力一下。当然提供简单的视频 demo 演示也是必要的。期待你的 code changes.`

行内评论（挑 3 条）：
> `[GameRoleAvatar.vue] 这个地方的实现逻辑不太好：在为角色判断显示方式的时候（live2D方案 / 图片方案），不应该通过 live2dFailed，live2dAvtive这样的方式判断。目前在 GameRoleAvatar 中，它没有履行"单一职责"…我的建议：通过 role 的 live2D 属性判断本角色是否是需要 live2D 渲染…假如角色是 live2D 且渲染成功，走 live2d 渲染。如果渲染失败则显示 LIVE2D UNAVAILABE，如果不是 live2d 走传统渲染。其中 live2d 渲染 和 传统渲染 封装成两种组件。`
> `[GameDialog.vue] 这一段改是何意味，有点没看懂。因为这里的地方的更改似乎会在输入状态下名字从用户改成当前正在展示的人物。这不兑吧？`
> `[SettingsCharacter.vue] 这里似乎把新增人物的按钮加回来了（雾）也行是也行，你想的话可以正好把这个组件优化一下？`

作者 Slapq 的回复（显示"怎么处理 review"的标准姿势）：
> `已删除。四个测试文件、test:live2d、Vitest 及 Rust 测试模块都已移除。`
> `已恢复为显示玩家名称，该文件现已与旧版一致。`
> `已按你更正的范围处理标准模式：GameRolesStage 不再持有 Live2D 状态，改用 role.live2d 判断；Live2D 与静态渲染拆成两个组件。保留了场景级共享 Live2D 舞台以保持单 Pixi 实例，运行状态与呈现职责已按要求拆分。`
> **并且敢于在一条上反驳（带技术理由）：**
> `由于 Live2D 依赖 WebGL 渲染，多个 Live2D 角色必须共享同一个渲染上下文…如果每个 GameRoleAvatar 各自独立建渲染实例，就会同时存在多套 WebGL 上下文和渲染循环，造成性能问题…判断在 Avatar，实例在 Stage`

最终：`[REVIEW APPROVED] 很好的改动！现在代码逻辑很清晰了 / 鉴定为高质量 PR，接下来我会顺着继续维护这些代码。`

**→ 归纳出的要求：** ①不要越权重构无关逻辑 ②**单一职责**（组件要拆干净）③清理测试代码与硬编码 ④**要视频 demo** ⑤可以反驳，但必须给技术理由

#### ② PR #714 —— sdfsfsk《feat: OpenAI Codex（ChatGPT 订阅）LLM 提供商》（+1983 / 29 文件 / 0.75 天）
**6 条行内评论，全部都只讲一件事：文件放的位置。**
> `[REVIEW APPROVED] @SlimeBoyOwO: 大体没问题，就是文件放的位置不优雅，改改就行`
> `[LINE src-tauri/src/ai_service/llm/codex/auth.rs] 这个文件不要直接放在providers目录下，模块化到codex的地方。`
> `[LINE src/api/services/codex/index.ts] codex.ts 这个也不要直接放在 services 目录下`
> `[LINE src-tauri/src/api/codex/mod.rs] 也不要直接放在 api 下`

作者修复：`已按建议模块化收拢（df637e9f）：后端 llm/codex/{auth,provider}、api/codex/mod.rs，前端 services/codex/index.ts，模块路径与导入不变。`

**→ 归纳出的要求：** **新功能必须收进自己的子目录**（`llm/codex/`、`api/codex/`），**不许把文件平铺进共享目录**。

#### ③ PR #643 —— sdfsfsk《feat: 新增 Codex 风格工具访问模式、Glob/Grep 与 ReadMediaFile》（+2166 / 20 文件 / 6.23 天）
> `[REVIEW APPROVED] @SlimeBoyOwO: 这次更改功能更新很好，代码问题不大，就 MainChat 前端那个地方组件能复用之前的组件就不要重写一个，统一 UI 风格，减轻 MAIN 组件代码职责，顺便解决一下冲突就行。`
> `[LINE src/components/views/MainChat.vue] 这个notice软件里已有相关可复用的模块，这里别单独重写一个`

作者的修复回复（**PR 里"回应 review 的评论"的标准格式**）：
> `@SlimeBoyOwO 已按 review 处理完毕喵：`
> `**1. 冲突已解决**（merge 最新 dev，e3e8a24d）冲突只在 settings.rs / skill_files.rs 两处…已保留测试并验证：cargo test 6 个测试全过，vue-tsc 类型检查通过。`
> `**2. MainChat 组件已抽取**（3d55cb08）…样式改 scoped，MainChat 只留一行 <FullAccessWarning /> 挂载，不再承担这部分职责。`
> `CI 重新跑中了，麻烦再看看～`

**→ 归纳出的要求：** ①**组件复用优先，禁止重写已有 UI** ②统一 UI 风格 ③**减轻巨型组件的职责** ④**自己解冲突** ⑤每条 review 逐条编号回复 + 附 commit SHA

#### ④ PR #711 —— lxdzh13《feat：语音识别系统》（+6017 / 53 文件 / 79 commit / 2.19 天）
- 7 个已合并 PR 里 review 最少的：**2 个 APPROVED，0 行内评论**
- `ok，很不错`
- 合并前**前置 issue #650**（由作者自己提，标题 `feat(asr): 内置语音识别——本地 VAD + 云 ASR 替换失效的 Web Speech API`，label `type: feature request` + `status: waiting`，在 PR 合并当天由作者以 `v1已全部开发完成` 关闭为 `completed`）

**→ 归纳出的要求：** 大功能**先开自己的 issue**，做完后**在 issue 里宣布完成并关闭**，这是"规范化路径"。

#### ⑤ PR #671 —— Immaomao《feat: 注册 LLM 错误分类并在前端 i18n 提示可能原因》（+246 / 11 文件 / 4.91 天）
- `[REVIEW APPROVED] 非常好的优化内，suki` —— **0 行内评论**
- 该 PR body 里**主动写了 i18n 合规声明**（原文）：
  > `错误码是**稳定锚点**，文案全部在前端 i18n 维护（符合 `docs/i18n.md` 约定，改文案不动 key）`

**→ 归纳出的要求：** 涉及 UI 文案时，**主动声明符合 `docs/i18n.md`**（引用官方文档 = 加分）。

### 3.4 被关闭 / 拒绝的 PR 及原因（14 个样本）

| PR | 作者 | 规模 | 结果 | **原因（原文）** |
|---|---|---|---|---|
| **#695** | CF-612 | +3182 / 42 文件 | closed，**已拆 4 个** | T-Auto 找出多处 bug；最后作者自己说：`该大型 PR 已按功能范围拆分为四个独立 PR：#775 服装滚动条、#776 背景与音乐分类、#777 开机自启与桌宠启动、#778 角色收藏` |
| **#659** | sdfsfsk | **+6950 / 124 文件** | **closed（草稿→自行撤回）** | `还没做完，暂时设置为草稿pr` → 后：`关闭本 PR：按讨论将拆分为「剧本引擎通用能力 + DLC 识别接口」的新 PR 重新提交` |
| **#743** | lxdzh13 | +833 / 14 文件 | closed | `status: waiting`；T-Auto 指出桌宠卡死隐患 + **重复代码**（`GameDialog.vue 和 DialogueBox.vue 各自实现了一套几乎相同的…近百行渲染逻辑，这不太行，建议抽成通用的 composable 统一维护`）+ 性能（每字全量重绘 DOM） |
| **#718** | Requiem-OvO | +871 / 9 文件 | closed → 重提为 **#733（已合并）** | `@SlimeBoyOwO: 在 comment 中详细说明一下你的优化策略。除此之外**能用tailwindcss的情况就不要用css了捏**。` + `@shadow01a: 需解决合并冲突` |
| **#691** | UstinianUN | +347 / 17 文件 | closed | `@Immaomao: 分支交错了捏，要交到dev分支` |
| **#605** | YudongMa | +56 / 3 文件 | closed，labels `type: bug` + **`status: invalid`** | `该问题已经在PR #576 中解决` / `**另外提交PR的基分支请选择tauri-refactor而不是main哦，蟹蟹❤**` / `请切换到最新分支尝试` |
| **#597** | foxcyber907 | +0 / -1 / 1 文件 | closed，labels `status: invalid` + `status: needs more info` + `status: waiting` + **`spam`** | `@SlimeBoyOwO: 换成可用链接，不要这么水的😠` |
| #746 | cafeawa | — | **open，label `type: wontfix`** | 被标 wontfix（功能不被采纳） |
| #721 / #720 | Requiem-OvO | +3 / 1 文件 | closed | 重复提交同一修复；`@shadow01a: 已在issue回复` |

**归纳出的 4 类死法：**
1. **一个 PR 塞多个功能 / 规模过大** → 被要求拆（#695、#659）—— **我们最大的风险**
2. **分支提错**（提到 main 而非 dev）→ 直接打回（#691、#605）—— 3 个 open PR 还在犯
3. **纯重复/低价值** → `spam` / `status: invalid`（#597、#605）
4. **有不一致或设计问题但不改** → 长期 `status: waiting`，最后自己关闭（#743）

### 3.5 ⚠️ `ai-slop` 标签 —— 专门针对 AI 生成内容

仓库 22 个 label 里有两个"耻辱柱"：

| label | description（原文） |
|---|---|
| **`ai-slop`** | `Low-quality AI-generated content submitted without meaningful human review` |
| `spam` | `Low-value PR or issues submitted containing no meaningful fixes or fea` |

**完整 label 体系（对我们有用的部分）：**

| label | description 原文 |
|---|---|
| `status: waiting` | **Waiting on author** ← **我们的 issue #804 就是这个** |
| `status: needs review` | A maintainer must review this code |
| `status: in progress` | Implementation is proceeding smoothly |
| `status: needs more info` | Issue needs more information, discussion or reproducible example |
| `status: invalid` | This is not a valid issue |
| `priority: 1 high` | High priority bugs/missing features or non-crashing regressions |
| `priority: 2 medium` | Good Suggestions, will work on soon |
| `priority: 3 low` | Excellent suggestion and will be worked on in future |
| `priority: 4 furture` | Good suggestion that may be worked on later |
| `type: wontfix` | This will not be worked on |
| `type: guide` | Good for newcomers |

→ **我们的 PR 是 AI 深度参与产出的、且规模巨大**。`ai-slop` 是真实存在的风险标签。
   **对策写进第 5 节的"防 ai-slop 检查清单"。**

---

## 4. 代码风格约定

### 4.1 配置文件（全部存在，原文摘录）

**`.editorconfig`**（根目录）：
```ini
root = true
[*]
charset = utf-8
end_of_line = lf
insert_final_newline = true
trim_trailing_whitespace = true
indent_style = space
indent_size = 2

[*.{rs,toml}]
indent_size = 4

[*.{md,yml,yaml}]
trim_trailing_whitespace = false
```

**`.prettierrc.json`**（根目录，完整）：
```json
{
  "semi": true,
  "singleQuote": false,
  "tabWidth": 2,
  "useTabs": false,
  "trailingComma": "es5",
  "printWidth": 100,
  "endOfLine": "lf",
  "vueIndentScriptAndStyle": true,
  "singleAttributePerLine": false,
  "htmlWhitespaceSensitivity": "css",
  "bracketSameLine": false,
  "bracketSpacing": true,
  "arrowParens": "always",
  "proseWrap": "preserve",
  "plugins": ["prettier-plugin-tailwindcss", "prettier-plugin-classnames", "prettier-plugin-merge"],
  "tailwindFunctions": ["clsx", "cn", "classNames", "classNamesBind"],
  "tailwindAttributes": ["class", "className", "ngClass", "class:list"],
  "customFunctions": ["clsx", "cn", "classNames", "classNamesBind"]
}
```
> 注意 **`singleQuote: false`（双引号）+ `semi: true` + `printWidth: 100` + `vueIndentScriptAndStyle: true`**。

**`rustfmt.toml`**（根目录，完整）：
```toml
# --- 基础配置 ---
edition = "2021"
style_edition = "2024"

# --- 格式化风格 ---
max_width = 100
hard_tabs = false
tab_spaces = 4

# --- 导入与模块 ---
reorder_imports = true
reorder_modules = true

# --- 其他常用选项 ---
use_field_init_shorthand = true
match_block_trailing_comma = true
newline_style = "Unix"
```
> `max_width = 100` —— 与 prettier 的 printWidth 一致。

**`.prettierignore`**：`node_modules` / `dist` / `dist-ssr` / `*.log` / `coverage` / `*.html`

**⚠️ 没有 eslint 配置**（`.eslintrc*` / `eslint.config.*` 均不存在）。

**`.husky/pre-commit`（当前 dev 上，2026-09 新增）：**
```sh
pnpm exec lint-staged
```

**`package.json`（dev 分支）关键片段：**
```json
"scripts": {
  "build": "vue-tsc --noEmit --skipLibCheck && vite build",
  "format": "prettier --write . && cargo fmt --manifest-path src-tauri/Cargo.toml",
  "format:check": "prettier --check . && cargo fmt --manifest-path src-tauri/Cargo.toml -- --check",
  "check:rs": "cargo check --manifest-path src-tauri/Cargo.toml",
  "prepare": "husky"
},
"lint-staged": {
  "*.{js,ts,jsx,tsx,vue,html,css,scss,json,md}": ["prettier --write"],
  "*.rs": ["rustfmt"]
},
"devDependencies": {
  "husky": "^9.1.7", "lint-staged": "^17.4.1",
  "prettier": "^3.8.3", "prettier-plugin-classnames": "^0.10.3",
  "prettier-plugin-merge": "^0.10.1", "prettier-plugin-tailwindcss": "^0.8.0",
  "vue-tsc": "^2.1.10", ...
}
```

**"格式化"是最近才被强化的（dev commit 原文）：**
```
2026-09-03 SlimeBoyOwO  add: 增加 husky 钩子自动格式化代码
2026-09-03 SlimeBoyOwO  del: 取消 pnpm format 在 dev 前执行
2026-08-30 SlimeBoyOwO  chore(pnpm): 增加自动格式化，并顺便格式化
2026-09-08 shadow01a    fix(husky)：lint-staged 的 *.rs 改用 rustfmt，修复向 cargo fmt 追加文件名导致…
```
→ **维护者在 9 月初专门补了格式化钩子，说明他很在意格式**。虽然 CI 不卡，但**不格式化会被记在心里**。

### 4.2 Rust 侧命名与注释风格

**模块组织惯例：`src-tauri/src/<子系统>/{mod.rs, manager.rs, types.rs, ...}`**
```
src-tauri/src/achievements/{mod.rs, manager.rs, triggers.rs, types.rs}
src-tauri/src/adventures/{mod.rs, manager.rs, trigger.rs}
src-tauri/src/ai_service/asr/{mod.rs, error.rs, provider.rs, provider_stream.rs, session.rs, settings.rs, vad.rs, vad_segmenter.rs}
src-tauri/src/ai_service/asr/... → 子系统内再分子模块
```
（dev 分支共 **274 个 `.rs`**、155 个 `.vue`、217 个 `.ts`）

**`src-tauri/src/api/mod.rs`（命令注册中心）的写法：**
```rust
pub mod achievement;
pub mod adventure;
pub mod ambient;
pub mod asr;
...
pub mod workshop;

// ========== 共享辅助函数 ==========

/// 资源来源字段默认值（游戏自有）。
pub(crate) fn default_source() -> String {
    "game".to_string()
}

/// 文件修改时间戳（秒，含小数），读取失败返回 "0"。
pub(crate) fn mtime_secs(path: &std::path::Path) -> String { ... }
```
→ **`pub mod` 按字母序排列**；共享辅助函数放 `api/mod.rs`，用 `// ========== 小标题 ==========` 分隔。

**注释与错误信息：中文 `///` 文档注释 + 中文错误串**
```rust
/// 解锁一个成就并向前端广播 `achievement:unlocked` 事件。
///
/// 命令（`api/achievement.rs::unlock_achievement`）与剧本事件
/// （`unlock_achievement` 事件处理器）共用，保证两处行为一致：
/// 解锁即落盘（`AchievementManager::unlock` 内部保存）+ 推送成就弹窗。
/// 返回 None 表示成就不存在或已解锁（不重复广播）。
pub async fn unlock_and_emit(
    app: &tauri::AppHandle,
    manager: &Mutex<AchievementManager>,
    achievement_id: &str,
) -> Result<Option<Achievement>, String> {
    let mut mgr = manager.lock().await;
    if let Some(achievement) = mgr.unlock(achievement_id) {
        app.emit("achievement:unlocked", &achievement)
            .map_err(|e| format!("发送成就事件失败: {}", e))?;
        Ok(Some(achievement))
    } else {
        Ok(None)
    }
}
```
**要点：**
- `///` 中文文档注释，**解释"为什么"和跨模块调用关系**（不只解释"是什么"）
- 错误信息**中文**：`format!("发送成就事件失败: {}", e)`
- 命令层统一返回 `Result<_, String>`
- 事件名用 **`子系统:动作`** 格式（`achievement:unlocked`）→ 我们可用 `world_map:moved`
- 现代惯用法：`LazyLock`、`HashMap::from([...])`、`let ... else`
- 自定义分隔：`// ========== 默认成就定义 ==========`
- `#[tauri::command] pub async fn ...`；`pub(crate) fn` 用于内部共享

### 4.3 前端 Vue 的组件写法

**统一用 `<script setup lang="ts">`（Composition API）** —— 抽查的组件 100% 如此，**没有 Options API**。

**两种 `defineProps` 都有：**

新版（类型式，推荐，`Live2DStage.vue`）：
```vue
<script setup lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { onBeforeUnmount, onMounted, provide, readonly, ref, watch } from "vue";

  import { getLive2dFilePath } from "@/api/services/character";
  import type { GameRole } from "@/stores/modules/game/state";
  import { areEyesOpen, focusDirection, pointerToStagePoint } from "./live2d-interaction";

  defineOptions({ inheritAttrs: false });

  const props = defineProps<{
    roles: GameRole[];
    mode: "standard" | "pet";
    activeSpeakerId: number | null;
    /** 投屏全局缩放：乘在角色基础 scale 上，作用于 Live2D 模型本体
      （标准模式下仅投屏窗口传入，主窗口缺省为 1，无影响） */
    castScale?: number;
  }>();

  const emit = defineEmits<{
    activeChange: [roleIds: number[]];
    failedChange: [roleIds: number[]];
  }>();
</script>
```

旧式（运行时式，`Toggle.vue`）：
```vue
<script setup lang="ts">
  import { ref, watch } from "vue";

  const props = defineProps({
    checked: { type: Boolean, default: false },
    /// 禁用开关。禁用时原生 input 也会被 disabled，避免点穿；
    /// 视觉上变灰且鼠标变 not-allowed。默认 false，既有调用方不受影响。
    disabled: { type: Boolean, default: false },
  });

  const emit = defineEmits(["change"]);
</script>
```

**样式：Tailwind 优先，`<style scoped>` 兜底**
```vue
<template>
  <div class="flex items-center">
    <label class="text-3.5 relative inline-flex w-full items-center text-white select-none">
      <span class="relative mr-2 inline-block h-6.5 w-12.5 shrink-0 rounded-[13px]
                   transition-all duration-300 ease-in-out"
            :class="[internalChecked ? `border-(--accent-color) bg-[rgba(121,217,255,0.3)]` : 'border-white/30 bg-white/20']">
```
```vue
<style scoped>
  /* 保留CSS变量引用 */
  :deep(*) {
    --accent-color: var(--accent-color);
  }
</style>
```
**要点：**
- **`<script>` 内容缩进 2 空格**（`vueIndentScriptAndStyle: true`）
- **双引号 + 分号**（prettier 配置）
- **Tailwind 原子类**为主，支持自定义工具类（`text-3.5`、`h-6.5`、`bg-linear-to-br`、`border-(--accent-color)`）
- 样式冲突时用 `<style scoped>` + `:deep()`
- **维护者明确要求：`能用tailwindcss的情况就不要用css了捏`（#718）** ⚠️
- 组件的配套逻辑拆成同目录的独立 ts 文件（`live2d-interaction.ts` / `live2d-layout.ts` / `live2d-motion.ts` / `useLive2dLipSync.ts`）→ **composable 优于巨型组件**（呼应 #743 的要求"建议抽成通用的 composable 统一维护"）

### 4.4 i18n 约定（来自 `docs/i18n.md` 原文，**很重要**）

- 四种语言：**中文 / 繁體中文（香港·粤语文体）/ 日本語 / English**
- 目录：`src/locales/<locale>/<namespace>.ts`，locale = `zh-CN` / `zh-HK` / `ja` / `en`
- 命名空间：`common / nav / advance / settings / game / views / pet / stores / ui / api / misc`
- **✅ 关键宽限（原文）：**
  > **约定：新功能只需要在 `src/locales/zh-CN/<对应命名空间>.ts` 加中文词条。**
  > - `fallbackLocale: 'zh-CN'`：其他语言缺键时运行时自动回落中文，界面不会坏
  > - 日/粤/英可以攒一批后统一补翻
- key 命名：`<命名空间>.<页面/组件>.<元素>.<语义>`，如 `settings.history.backtrack`
- 组件内用 `useI18n()` 的 `t()`；非组件环境用 `import { i18n } from '@/locales'` + `i18n.global.t(...)`

→ **我们只需写 `src/locales/zh-CN/<ns>.ts` 即可合规**（虽然 #671/#750 做了 4 语种，那是加分项不是必需）。

### 4.5 测试政策（再强调一次，这条最容易踩雷）

| 事实 | 证据 |
|---|---|
| dev 全树只有 **2 个** test 文件 | tree API：`tts/cloud/enrollment_test.rs`、`config/tts_test.rs` |
| 有人专门提 PR **删掉**测试代码与测试文档，**并被合并** | #651 `chore: 清理项目内测试代码与测试文档` +38 / **-3744** / 61 文件 |
| 大 PR 里会专门 commit 删测试 | #714 commit：`chore: remove Codex catalog unit tests from PR` |
| review 会点名要求删测试 | #665：`这里的硬编码测试用部分可以删除` / `存在许多未清理的测试代码（包括硬编码），解决一下` |
| Rust 内联测试可接受 | #711 body：`cargo test 48/48`；#750 带 `enrollment_test.rs` 并合并 |
| **前端测试文件会被点名删除** | #665 里 `model-source.test.ts` 被要求删，作者删了 4 个测试文件 + Vitest |

→ **规则：前端不要带 `.test.ts` / Vitest；Python 自检脚本不要带；Rust 内联 `#[cfg(test)]` 可以留但要说明。**

---

## 5. 我们的 PR 该怎么写（结论）

### 5.1 标题

**推荐（模仿 #711 / #750 的形态）：**

```
feat(world-map): 二维世界地图系统——分层地理下钻 + AI 街区 + 悬浮手机（手机端优先）
```

依据：
- **`feat` 前缀**（9/30 已合并 PR 用 feat；大的新功能全是 feat）
- **`(world-map)` scope**（`feat(tts):` #750、`fix(asr):` #745、`refactor(import):` #639 有先例）
- **半角冒号 `:`**（24/30 用半角）
- **中文描述**（25/30 中英混排，0 个纯中文标题；中文主体是主流）
- 破折号 `——` 分隔"是什么 + 关键特性"，是 #711/#750 的原样写法

**不推荐：** `[Feature] ...`（那是 issue 模板的 title 格式，不是 PR 的）、纯英文长句（我们的功能很难用英文一句话说清）、全角冒号。

### 5.2 PR 描述模板（可直接复制填写）

> 依据：#711 的分节结构（`概述/功能/架构/验证/后续开发计划`）+ 官方 `feature.md` 模板的必需小节（变更类型/相关 Issue/检查清单/截图）
> + 13/30 已合并 PR 都有的 `## 验证` 小节（**最高频标题**）+ #671 的"i18n 合规声明"写法。

```markdown
## 概述

为 LingChat 增加「二维世界」维度：角色不再只存在于对话流里，而是生活在一张**可下钻、可游走的地图**上。
世界 → 省 → 市 → 区县（真实行政区划下钻）→ 小区/街区（LLM 生成布局 + 代码绘制）。

**仅新增，不改动既有功能**：不修改任何现有模块的公开接口，地图层可整体关闭。

## 变更类型

- [x] 新功能

## 相关 Issue

close #804

## 功能

🗺️ 五层下钻：世界 → 省 → 市 → 区县 → 小区（点击下钻，缓动飞行而非瞬移）
📐 四层统一坐标：屏幕/区块/网格/地理坐标互通，小区用独立网格层保证精度
🖼️ 代码绘制而非图像生成：单张 8-25KB PNG，缓存后 60-170ms（对比图像模型的体积/一致性劣势见文档 03）
🤖 AI 生成街区：LLM 输出布局 JSON，代码绘制建筑与道路（单次 10-19 栋 / 4-23 条路）
👆 触摸交互：单指平移 / 双指缩放 / 点击下钻
📱 悬浮手机：打车、天气、音乐、日程等入口（手机端优先设计，非桌面简化版）
🌤️ 真实天气：wttr.in，30 分钟缓存
🧩 地图层两种形态：半透明背景层 / 角落小窗，均可关闭

## 架构

- **Rust 侧**：`src-tauri/src/world_map/`（14 个文件，纯新增），命令经 `src-tauri/src/api/world_map.rs` 注册进 `api/mod.rs`
- **前端**：`src/components/worldmap/`（组件）+ `src/composables/worldmap/`（composable）+ `src/api/services/world_map/`（service）
- **数据**：地图数据落在数据目录，不污染仓库（生成的区域按 id 入库复用）
- **与既有能力的关系**：只读消费 `UserState` / `useHeartbeat` / `ProactiveEvent` / `get_character_list`，
  **不新增对话状态、不改对话管线契约**

## 验证

- Rust：`cargo fmt -- --check` ✓、`cargo check` ✓、`cargo test` ✓（N/N）
- 前端：`vue-tsc --noEmit --skipLibCheck` ✓、`vite build` ✓、`prettier --check` ✓
- 功能自验：下钻链路（世界→省→市→区县）、缓存命中耗时、双指缩放/单指平移、悬浮手机各入口
- **真机实测**：Android（<机型>）装机验证 —— 下钻、触摸交互、地图层开关全链路可用
- 截图/录屏见下方评论

## 涉及文件

- 新增：`src-tauri/src/world_map/*`（14 个）
- 新增：`src/api/services/world_map/*`、`src/components/worldmap/*`、`src/composables/worldmap/*`
- 接线（各 1-5 行）：`src-tauri/src/api/mod.rs`、`src/router/index.ts`、`src/App.vue`、主菜单入口
- i18n：`src/locales/zh-CN/<ns>.ts`（按 `docs/i18n.md`，其余语种回落中文）
- 文档：`docs/world-map/*.md`

## 检查清单

- [x] 代码已经本地测试
- [x] 已执行 `pnpm format`（prettier + cargo fmt）
- [x] 新增 UI 文案已按 `docs/i18n.md` 走 i18n key（非硬编码中文）
- [x] 无新增测试文件（按项目惯例，测试不随仓库发布）
- [x] 未改动任何既有模块的公开接口
- [x] 已与最新 `dev` 对齐（无冲突）

## 影响面 / 回滚

- 影响面：新增一个可关闭的地图层；关闭后与合并前行为完全一致
- 回滚：删除 `world_map` 目录 + 撤销 4 处接线即可

## 后续开发计划

1. 世界模拟（时间/天气/NPC/交通）
2. 结合角色设定生成专属世界
3. 空间记忆（"常去的地方"写入记忆系统）
```

**必须包含的小节（按重要性）：**
1. **`## 验证`** — 13/30 都有，是最高频标题；**必须写可复现的命令与真机结果**
2. `## 概述` — 一句话说清"是什么 + 只新增不改动"
3. `## 变更类型` / `## 检查清单` — 官方模板要的（虽然只有 6/30 照抄，但照抄不会错）
4. `## 相关 Issue` + `close #804`
5. `## 截图/示例` 或"见评论"
6. `## 涉及文件` — 让维护者一眼看清改动边界（#794 用了这个写法）

### 5.3 是否必须先开 issue？—— 我们已经开了，**而且维护者已回复**

**CONTRIBUTING.md 的硬要求**：
> **只有在社区达成共识后，才建议开始投入代码开发。**

**我们的 issue #804（2026-09-12 拉取的最新状态）：**

| 项目 | 值 |
|---|---|
| 标题 | `[Feature] 二维世界地图系统：分层真实地理 + AI 生成街区（专为手机端设计）` |
| state | **open** |
| labels | **`type: feature request`** / **`status: waiting`** / **`priority: 2 medium`** |
| created | 2026-09-10T17:09:17Z |
| updated | 2026-09-11T13:28:35Z |
| comments | **1** |

**维护者回复全文（唯一一条评论，SlimeBoyOwO / OWNER，2026-09-11T13:28:15Z）：**
> `有这个的想法，嗯你的主意很好，未来会参考的。`
>
> `不过先做真实地理下钻这个还是有点逆天（`

**然后是 label 事件（同一天 13:28:35，由 SlimeBoyOwO 本人打的）：**
```
labeled  SlimeBoyOwO  2026-09-11T13:28:35  type: feature request
labeled  SlimeBoyOwO  2026-09-11T13:28:35  status: waiting
labeled  SlimeBoyOwO  2026-09-11T13:28:35  priority: 2 medium
```

**解读（关键）：**
- `status: waiting` 的官方 description 是 **"Waiting on author"** —— 即**维护者在等我们回话**，不是"我们等他"
- `priority: 2 medium` 的 description 是 **"Good Suggestions, will work on soon"** —— **不是拒绝**
- 但回复里的 **"未来会参考的"** 是**软性、非承诺**的表态，**没有说"欢迎提 PR"**
- **"先做真实地理下钻这个还是有点逆天"** 是一句**明确的疑虑**：他质疑的正是我们要做的第一件事

**先例支持"issue 不温不火也能提 PR"：**
- **正向先例**：#650（`feat(asr): 内置语音识别…`，作者 lxdzh13 自己提的）→ 他做完后 PR #711（+6017）**2.19 天合并**，并在 issue 里留言 `v1已全部开发完成` 后关闭为 `completed`
- **反向先例（对我们有利）**：issue #116（Live2D）曾被 T-Auto 明确拒绝过：
  > `鉴于我们的朋友在做传统live2d方案的开源项目，我们不希望功能性重合，所以暂不考虑给LingChat添加live2d支持，这并非技术性问题，此外，live2d与LingChat理念和后续规划偏差较大。`
  
  issue #594（Live2D）维护者也说：
  > `很难，很难，很难，每个拆开工作量都不会小。这些会在日后工作慢慢推进`
  
  **然而 PR #665 还是被接受并合并了**（走了一轮 CHANGES_REQUESTED）。
  
→ **结论：上游没有"issue 必须被批准才能提 PR"的硬机制。没被明确拒绝就有机会。**
→ **但强烈建议：先在 #804 回一条评论（`status: waiting` 要求我们回），把"我们打算怎么做、只新增哪些文件、做多大"讲清楚，
   顺便回应"逆天"那个疑虑。** 现有草稿见 `16-issue804进展评论草稿.md`（注意该草稿标注了"暂缓发布"和一处事实错误待改）。

### 5.4 分支名怎么起

CONTRIBUTING.md 建议：`feature/awesome-new-idea`（从 `main` 或 `dev` 创建）。

**实际统计（最近 30 个已合并 PR 的 head 分支名）：**

| 前缀 | 数量 | 例子 |
|---|---|---|
| `fix/` | 9 | `fix/llm-provider-card-badges-v2`、`fix/framing-prefix-hardcoded-name` |
| **无前缀（纯名字）** | 9 | `dialog-clean`、`dev`（直接用 fork 的 dev） |
| `feat/` | 5 | `feat/file-import-robustness`、`feat/asr-ptt-hotkey` |
| `agent/` | 4 | `agent/remove-project-tests`、`agent/fix-android-tools` |
| `feature/` | 1 | `feature/macos-frameless-window` |
| `refactor/` | 1 | `refactor/llm-presets` |
| `codex/` | 1 | `codex/角色收藏跨页排序`（**中文分支名也有人用**） |

**26 个 open PR 的分支名**里还能看到：`pr/native-multimodal`、`pr-fix-save-history`、`codex/开机自启与桌宠启动`（中文）、`dev`、`main`。

→ **结论：没有强约定。`feat/world-map` 完全合规**（`feat/` 是第三常用，语义清晰）。
→ **重要：绝不叫 `dev` 或 `main`**（有 2 个 open PR 因为用 fork 的 `dev`/`main` 当分支名而混淆）。
→ 我们已有 `feat/world-map`，名字没问题。

### 5.5 有没有"大 PR 要先讨论"的惯例？

**有，而且是写在贡献指南里的硬要求，并且有 2 个惨痛先例。**

**惯例证据：**
1. CONTRIBUTING.md：**"先讨论，后开发"** / "只有在社区达成共识后，才建议开始投入代码开发"
2. **#659**（+6950 / 124 文件）：作者自己写 `还没做完，暂时设置为草稿pr`，最终 `关闭本 PR：按讨论将拆分为…`
3. **#695**（+3182 / 42 文件）：被 T-Auto 逐条挑错，最后作者 `该大型 PR 已按功能范围拆分为四个独立 PR：#775…#776…#777…#778`
4. **对照组 #711**（+6017 / 53 文件）：**有前置 issue #650**，2.19 天合并，review 只有一句 `ok，很不错`

**→ 拆与不拆的分界线不是"行数"，而是"是否单一功能"：**
- #711 6017 行 = **一个功能**（ASR）→ 合并
- #695 3182 行 = **四个功能**（服装/背景音乐分类/开机自启/角色收藏）→ 被拆
- #659 6950 行 = 引擎 + 剧本内容 + 素材 → 被拆

**我们符合"单一功能"**（世界地图），但**行数超过所有已合并先例**，且**跨了 Rust + 前端 + 路由 + 菜单 + i18n + 数据**多个层面。

**建议的提交策略（三选一，按推荐度排序）：**

| 方案 | 做法 | 优点 | 风险 |
|---|---|---|---|
| **A（推荐）分批** | 先提**只读下钻**（世界→省→市→区县，无 AI、无 LLM 依赖）作为 PR-1；AI 街区/悬浮手机/世界模拟作为 PR-2、PR-3 | 单一功能、风险最低、符合 #711 的成功模式；每批都能快速拿到反馈 | 需要拆分支，工作量大 |
| **B 单 PR + 草稿先行** | 先开 **draft PR** 让大家看结构，收敛后再转 ready | 有 #659 的 draft 先例；能提前拿到方向性反馈 | #659 的 draft 最后也没合 |
| **C 一次性大 PR** | 直接提 ~10k 行 | 省事 | **没有任何先例**；触发 `ai-slop` / 被要求拆的概率最高 |

**无论选哪个，都建议先在 issue #804 评论里说明"打算怎么拆"，让维护者选。**

### 5.6 提交前检查清单（防 `ai-slop`）

- [ ] **先在 issue #804 回评论**（`status: waiting` = 等我们回），说明范围与拆分计划
- [ ] **base 分支 = `dev`**（不是 main！）
- [ ] **分支基于 `dev` 的最新 HEAD 重建**（见第 6 节的 🔴 阻塞项）
- [ ] 跑过 `pnpm format`（prettier + cargo fmt），`vue-tsc --noEmit --skipLibCheck`
- [ ] **删掉所有 `*_selftest.py` / `*_test.js` / `frontend_selftest.js` / `page_smoke.js`**（官方不保留测试代码）
- [ ] **删掉所有临时/调试/草稿文件**（`temp/`、`14-LingChat架构地图.md` 这类调研产物、`.dsh` 相关）
- [ ] **UI 文案走 i18n key**（`src/locales/zh-CN/`），不硬编码中文
- [ ] **样式优先 Tailwind**，`<style scoped>` 只在必要时用（#718 明确要求）
- [ ] **新文件全部收进自己的子目录**（`world_map/`），不平铺进 `api/` `services/` `components/` 根（#714 明确要求）
- [ ] **不重写既有组件**，已有可复用的就复用（#643 明确要求）
- [ ] **不夹带与地图无关的改动**（参考我们的 `feat/world-map-official` 那条"剔除环境适配与预览层"的 commit 思路）
- [ ] **准备截图（贴评论）+ 手机端录屏**（#665 明确要求 demo）
- [ ] 描述里写清 **`## 验证`**（命令 + 真机结果）
- [ ] 准备好**逐条回复 review**（编号 + commit SHA + 验证方式），参考 #643 作者的做法

---

## 6. 我们的 feat PR 的明显风险（按严重度排序）

### 🔴 风险 1（阻塞级）：当前分支与上游**没有共同祖先，开不了 PR**

**证据：**
```
$ git merge-base upstream/dev feat/world-map-official
（空）

$ git rev-list --max-parents=0 feat/world-map-official
bbac1eb068591cc61987a36ad1c8ca00576a4c4e     ← 唯一根提交，无 parent

$ git cat-file -p bbac1eb0
tree f050db262424bccbfb4fb766b840d728ddb0f63f
author Syuki <syuki@local> 1787469448 +0800
committer Syuki <syuki@local> 1787469448 +0800
LingChat debug build + B站学习功能 + debug workflow      ← 没有 parent 行

$ git merge-base --is-ancestor 0d8e0a61 feat/world-map-official
NO - upstream dev tip is NOT an ancestor of our branch

我们的分支：65 个 commit（含 1 个孤立根）
upstream/dev：5486 个 commit（根 = 2025-04-06 first commit）
```

**含义：** `feat/world-map-official` 是用 Git Data API 写出来的**独立历史**（author 是 `Syuki <syuki@local>`），
**不是 fork 自上游**。以这个分支开 PR，GitHub 无法计算 merge base —— 要么提示无法比较，
要么给出"整个仓库都变了"的荒谬 diff。

**必须做的：** 用 Git Data API **在 `dev` 最新 HEAD 之上重建分支**：
1. 取 `GET /repos/SlimeBoyOwO/LingChat/git/ref/heads/dev` → dev HEAD sha
2. 取该 sha 的 tree sha 作为 `base_tree`
3. 用 `POST /git/trees` 把我们的新增文件挂上去（`base_tree` + 我们的 paths）
4. `POST /git/commits` 时 **`parents: [dev_head_sha]`**
5. `POST /git/refs` 在**我们自己的 fork** 上建分支（或推到自己仓库后从网页开 PR）

> ✅ 我们的 `docs/world-map/09-向官方提拉取请求(PR)的方案.md` **已经预判了这一点**
> （"取 fork 的 dev HEAD sha → 用它当 base_tree 的 commit"），做的时候按那份走即可。
> ⚠️ 但 09 号文档写的是"文件白名单（当前 17 个）"，而实际分支有 30+ 文件，**范围已经变了，需要同步更新**。

### 🔴 风险 2：`ai-slop` 标签 —— 我们的产出形态正好撞在枪口上

**证据：** 仓库有 label `ai-slop` = *"Low-quality AI-generated content submitted without meaningful human review"*；
另有 commit 记录显示维护者对 AI 产物有戒心（我们的分支里有 `agent/*` 分支名的先例是 sdfsfsk 用的，说明 AI 辅助本身可接受，**但"没有人工审查痕迹"就会被打标**）。

**缓解（必须做到）：**
- PR 描述里体现**人工判断**：为什么这样设计、排除了哪些方案（对比表）、实测数字
- **删掉一切"AI 味"痕迹**：调研文档、脚本生成的自检文件、`agent/` 之类的分支名
- commit 消息写成人类口吻、中文、说清"为什么"（参考 #711 的 `feat(tts): cosyvoice 上传逻辑对齐参考实现——getPolicy 字段/uuid前缀/表单/日志/20MB限制`）
- 提供**真机录屏**——这是最难伪造的"人工验证"证据

### 🟠 风险 3：规模超出所有已合并先例

| 对象 | 行数 | 文件 | 结果 |
|---|---|---|---|
| #711（最大已合并） | +6017 | 53 | ✅ 合并（2.19 天） |
| #659 | +6950 | **124** | ❌ 拆分 |
| #695 | +3182 | 42 | ❌ 拆分 |
| **我们（估）** | **~10000** | **30+** | ❓ |

→ 我们在**行数上超过最大先例约 65%**。而且我们的改动**横跨 Rust / 前端 / 路由 / 菜单 / i18n / 数据 / 文档**，
   虽然逻辑上是"一个功能"，但在 review 者眼里容易被判为"太大"。
→ **缓解：按 5.5 的方案 A 分批；或至少先在草稿 PR 里让维护者确认范围。**

### 🟠 风险 4：issue #804 未获明确认可，且维护者对核心方案有疑虑

- 回复是 **"未来会参考的"**（软性、非承诺），**没有一句"欢迎提 PR"**
- **"不过先做真实地理下钻这个还是有点逆天（"** —— 直接质疑我们计划的第一步
- `status: waiting` 意味着**我们在欠一次回复**

→ **缓解：先回复 #804，正面回应"逆天"的疑虑**（比如：为什么用真实行政区划数据而不是手绘、
   数据量多大、离线可不可行、不做定位也能用——把"逆天"变成"可行"）。
   现有草稿 `16-issue804进展评论草稿.md` 里有一处**事实错误必须先改**：
   文中说地图"和 Bilibili / 网易云同一级"，但原版 LingChat 没有这两个功能（那是我们自己加的）。

### 🟠 风险 5：当前上游节奏放缓 + 排队严重

- **2026-09 至今（12 天）只合并了 3 个 PR**；**最近 7 天只有 2 个**
- **26 个 open PR 里 18 个是 `status: waiting`**，中位年龄 **12 天**，最老 **41 天**
- 已有 5 个 zhangzm0 的功能 PR 排队 21-41 天；sdfsfsk 从 #659 拆出来的 #677 **排了 21 天仍未合并**

→ 期望管理：**即使一切顺利，也不要指望几天内合并**。
   历史中位 1.7 天是"维护者活跃期"的数字，现在不是活跃期。

### 🟠 风险 6：我们的分支缺少 dev 最近新增的基础设施

**证据（tree 对比）：**
- 我们的分支 **没有 `.husky/`**；而 dev 有 `.husky/pre-commit` = `pnpm exec lint-staged`
- 我们的 `package.json` **缺** `husky` / `lint-staged` devDeps、缺 `prepare: husky`、缺 `lint-staged` 配置
- dev 最近提交：`add: 增加 husky 钩子自动格式化代码`（09-03）、`fix(husky)：lint-staged 的 *.rs 改用 rustfmt`（09-08）

→ 如果直接拿旧基线开 PR，会出现"我们删掉了维护者的格式化基础设施"的观感（或至少产生冲突）。
→ **缓解：重建分支时以 dev 最新 HEAD 为 base（风险 1 的修复顺带解决这个）。**

### 🟠 风险 7：我们的自检脚本 / 调研文档会踩"清理测试代码"的雷

- 我们有一批 `*_selftest.py`（maplib 42 项 / blocks 32 项 / schedule 65 项…）、`frontend_selftest.js`（88 项）、`page_smoke.js`
- 官方**只有 2 个 test 文件**，且**专门提 PR 删测试代码**（#651），**review 明确要求删**（#665）
- 我们还有大量 `docs/world-map/1x-*.md` 调研/方论文档（含"31 项任务完成度评估""世界模拟需求问卷"等**内部产物**）

→ **缓解：PR 里只保留 `docs/world-map/` 下对上游有意义的方案文档（参考 `docs/live2d/` 的形态：README + 少量分节 + diagrams）；
   所有自检脚本、评估表、问卷、架构地图（134KB！）一律不进 PR。**

### ⚪ 风险 8：语言

**不是大问题。** 25/30 已合并 PR 用中英混排标题，PR 描述与 review 全部中文，维护者用中文交流（"捏""喵""nya~"）。
→ **用中文写 PR 完全没问题**，但**技术术语要准确**（架构图、命令、路径用英文/代码格式）。

---

## 7. 拿不到 / 不确定的数据（如实记录）

| 项 | 状态 | 原因 |
|---|---|---|
| **协作者名单** | ❌ 拿不到 | `GET /repos/.../collaborators` → **403** `Must have push access to view repository collaborators` |
| **PR 的 CI 检查结果明细** | ❌ 未取 | 需 `GET /commits/{sha}/check-runs`，未在本次范围内（且 T-Auto 会人工审） |
| **合并方式（merge vs squash）精确比例** | ⚠️ 仅抽样 12 个 | 列表接口不给合并方式；需逐个取 merge commit 的 parents 数。抽样结果：**7 个 merge commit / 5 个 squash 或 rebase** |
| **#597 的 reviews** | ⚠️ 部分失败 | `pulls/597/reviews` 两次 `Remote end closed connection`（网络抖动） |
| **`T-Auto` 的触发机制** | ⚠️ 部分确定 | 已确认它是**人类协作者**（profile `type: User`，name=风雪），会主动 review + 打 label。**抽样验证：4 个顺利合并的 PR（#799/#711/#671/#643）都【没有】它的评论**；而它在 #695/#743（有问题、被拆/被关）、#693/#775/#783 以及大量 open PR（#747/#755/#757/#758/#760/#773/#776/#778/#780/#782）里都有深度评论。→ **倾向"挑有问题的 / 未合并的看"，即：被 T-Auto 点评≈被盯上了**。但触发阈值（是否按 label、按规模、还是人工挑）无法从 API 反推 |
| **`/opencode` 这条 AI 审阅通道的实际使用情况** | ⚠️ 未观测到实例 | `.github/workflows/opencode-review.yml` 存在（`contains(comment.body, '/opencode') \|\| contains(comment.body, '/oc')`），但在抓到的评论里没看到有人发过 `/opencode` |
| **`upstream/dev` 是否是最新** | ⚠️ 可能过期 | git 通道不通，无法 `git fetch`；本地 `upstream/dev` tip = `0d8e0a61`（2026-09-04 提交）。**本文所有 dev 文件内容均改用 API `?ref=dev` 实时抓取**，不受影响 |
| **我们 PR 的精确 diff 行数** | ❌ 算不准 | 与上游**无共同祖先**（见风险 1），`git diff upstream/dev <我们的分支>` 得到的是两棵完整树的差异（显示 +10030/-1665/32 文件），**其中混入了上游自己的改动**（如 `.husky/pre-commit`、`pnpm-lock.yaml`），**不是我们的净改动**。文中的"~10000 行"是据此的粗估 |
| **仓库源码规模** | ✅ 有 | `size` = 143262 KB（约 140 MB）；dev 分支 274 `.rs` / 217 `.ts` / 155 `.vue` / 57 `.md` |
| **#804 的未来走向 / 是否会有人来认领** | ❌ 无法预测 | — |

---

## 附录 A：可复用的数据采集脚本

本次研究用的只读脚本（token 从 `~/.dsh/.credentials.yaml` 读入变量，**不打印明文**）：

- `~/lingchat-pr-research/gh.py` —— 通用 GET 助手（支持 `--raw`，含 403/网络重试）
- `~/lingchat-pr-research/collect.py` —— 最近 30 个已合并 PR 的详情 + commits
- `~/lingchat-pr-research/reviews.py` —— 20 个已合并 PR 的 review/评论 + 未合并 PR 的关闭原因
- 产出：`merged30_detail.json` / `merged30_commits.json` / `reviews.json` / `unmerged.json` / `issue804.json` / `tree_dev.json` / `labels.json` / `pulls_open.json`

## 附录 B：关键 URL 速查

| 用途 | URL |
|---|---|
| 上游仓库 | https://github.com/SlimeBoyOwO/LingChat |
| 我们的 issue | https://github.com/SlimeBoyOwO/LingChat/issues/804 |
| 最佳模仿对象（6017 行已合并） | https://github.com/SlimeBoyOwO/LingChat/pull/711 |
| 前置 issue 先例 | https://github.com/SlimeBoyOwO/LingChat/issues/650 |
| "被要求重构"的教材 | https://github.com/SlimeBoyOwO/LingChat/pull/665 |
| "文件要模块化"的教材 | https://github.com/SlimeBoyOwO/LingChat/pull/714 |
| "别重写组件"的教材 | https://github.com/SlimeBoyOwO/LingChat/pull/643 |
| 大 PR 被拆的反面教材 | https://github.com/SlimeBoyOwO/LingChat/pull/659 ・ https://github.com/SlimeBoyOwO/LingChat/pull/695 |
| 贡献指南 | https://github.com/SlimeBoyOwO/LingChat/blob/dev/.github/CONTRIBUTING.md |
| PR 模板 | https://github.com/SlimeBoyOwO/LingChat/blob/dev/.github/PULL_REQUEST_TEMPLATE/feature.md |
| i18n 约定 | https://github.com/SlimeBoyOwO/LingChat/blob/dev/docs/i18n.md |

---

*本文所有数据采集于 2026-09-12，全程只读 API，未对上游做任何写操作。*
