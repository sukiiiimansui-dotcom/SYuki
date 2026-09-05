# L-SYuki 上游同步指南（让官方更新快速移植）

> 目的：官方 LingChat 每次更新后，如何**最低成本**地把新功能并入我们 L-SYuki，且不丢我们已有功能。

## 0. 现状（为什么之前移植慢）
- 我们的 `main` 是**很老的 LingChat 旧结构** + 我们的全部功能（记忆/主动/网易云/B站等）。
- 官方 `upstream/main` 已进化到**新结构**（client 模块化等），当前官方版本 **v0.5.1**，`upstream/main = ef8f7914`。
- 我们 `main` 相对官方**落后约 5480 个 commit** → 结构差距巨大，所以每次"移植"都要逐功能手工适配，慢且易冲突。

## 1. 现状下的"快速移植"操作（不换结构时）
用 `bash _upstream_sync.sh`（已提供）：
1. `git fetch upstream` 拉最新。
2. 脚本按 `_upstream_base.txt` 记录的上次基准，打印**自上次以来官方新增 commit**，一眼看清新功能。
3. 对每个新功能：`git show <upstream_commit>` 提取 → 按 `HANDOFF` 的"适配模板"搬进我们旧结构 → `npx vue-tsc --noEmit --skipLibCheck` + `npx vite build`（必要时 `cargo check --lib`) 验证 → 提交。
4. 完成后 `echo "<新upstream commit>" > _upstream_base.txt` 更新基准。

> 优点：不重构、风险小。缺点：结构不同，每个功能都要手工适配。

## 2. 推荐的快速方案（向官方对齐，长期省力）⭐
**让"移植"变成"合入上游"**：把我们仓库改成**基于官方结构（migration-official 那样）+ 我们的功能作为薄层提交**。这样：
- 每次官方更新：`git fetch upstream && git rebase upstream/main`（或 merge）。
- 结构相同 → **绝大多数自动合并**，只在"我们的功能文件"上有冲突（因为我们功能是少量文件/薄层）。
- 从"手工搬运 5480 个 commit 的功能" → 变成"merge 一个 upstream 更新"。

### 具体做法（一次性迁移）
1. 以 `migration-official`（= 官方 ef8f7914 + 我们功能，已构建绿）为**新主基线**。
2. 把「我们独有的功能」整理成**少量、清晰的 commit 层**（记忆系统/主动心跳/网易云/B站/治卡/ASR/设置页/台词融合等），提交到一个分支（如 `syuki-features`）。
3. `main` 改为 = `upstream/main` + `syuki-features`（rebase 在上）。
4. 此后官方更新：`git fetch upstream && git rebase upstream/main`（main 上我们功能层顺延重放）。

### 理由
- 结构一致后，官方的新功能会**自动进入**，我们只需要处理"我们改动过的文件"潜在的少量冲突。
- 避免现在这种"落后 5480 个 commit 再一个个搬"的困境。

### 代价
- 一次性把我们的功能重新落到官方新结构上（工作量一次），但换来未来**每次官方更新近乎零成本**。

## 3. 维护清单
- `_upstream_sync.sh`：拉取+显示差异+可选 rebase。
- `_upstream_base.txt`：上次同步的 upstream commit 基准。
- `_upstream_base.txt` 更新后提交（`chore(sync): bump upstream base`）。
- 每功能移植参照 `HANDOFF-20260905-迁移与待办.md` 的适配模板（不改结构、不删我们功能、验证构建、不 push main 等约束）。

## 4. 对比
| 维度 | Mode A：旧结构手移 | Mode B：向官方对齐 |
|---|---|---|
| 同步速度 | 慢（每功能手工适配） | 快（merge/rebase 自动并入） |
| 冲突 | 结构不同到处冲突 | 仅我们功能文件可能冲突 |
| 前期成本 | 无 | 一次性结构迁移 |
| 长期可维护性 | 低（差距会越拉越大） | 高 |

## 5. 多频道模型（本项目采用，一劳永逸）

GitHub 分支即"频道"，各频道互不干扰、保留 main：

| 频道 | 内容 | 角色 |
|---|---|---|
| `main` | 当前稳定版（旧结构 + 我们功能） | 稳定/可发布 |
| `channel/upstream` | **官方上游(LingChat 0.5.1) + 我们功能**（migration-official 基线） | 前卫频道：全面领先官方 |

### 为什么这样最好
- **不用 force-push main**（main 历史保留、可随时回滚）。
- `channel/upstream` 按官方结构组织：
  - 官方 0.5.1 全部功能（治卡/ASR/设置页/台词融合/web投影/0.1.2增强等）**自动内置**（upstream 自带）——无需手工搬运。
  - 我们独有功能（记忆/主动/网易云/B站/情绪雷达等）作为**薄层叠加上去**。
- **未来官方更新**：在 `channel/upstream` 上 `git fetch upstream && git rebase upstream/main`（结构相同→自动合并），极少冲突。同步后 `main` 保持不动。

### 日常操作
- 看前卫功能/体验官方新特性 → `channel/upstream`。
- 看稳定版 → `main`。
- 官方更新同步 → 在 `channel/upstream` 上 `bash _upstream_sync.sh rebase`。
- 确认稳定后，把 `channel/upstream` 的关键成果 cherry-pick / merge 回 `main`（人工审查）。

### 约束
- channel/upstream 不覆盖/不删我们的独有功能；不 force-push main。
