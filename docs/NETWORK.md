# 网络与推送说明（GitHub push 偶发超时）

> **结论：网络一直是好的（连通正常），只是波动很频繁。** 因此 git push / curl 到 GitHub 会**偶发超时**，属正常现象，不是网络坏了或凭据问题。

## 现象
- `git push origin ...` 有时在 60s 后 `timed out` / `killed by SIGTERM`。
- 但**重试往往很快成功**（同一命令隔几秒再跑就通）。

## 原因
- 网络连通性正常（`curl github.com` 有时 000 是波动瞬间）。
- **波动频繁**：连接有时断一下、或 GitHub 响应慢，导致单个请求超时。

## 应对方式（反复重试）
- **不要一次判断失败**，用脚本循环重试（间隔 60s），网络恢复后自动成功。
- 项目内置脚本：
  - `_upstream_sync.sh`：拉取官方最新 + 显示差异（含网络重试思路）。
  - 推送用背景重试循环（如 `push_all_retry.sh`：`git push origin main channel/upstream` 循环重试，成功后退出并记日志）。
- 手动重试：`for i in 1 2 3; do git push origin main && break; sleep 5; done`

## 经验
- 重试 1~5 次内通常能成功（波动期）。
- 实在持续失败（>10 次），才是真断网，才需检查网络/代理。
- GitHub 推送无需代理，直接 https 即可（我们已配 `store` 凭据 + GITHUB_TOKEN）。

## 记录
- 日志：`$HOME/push_all_retry.log`、`$HOME/push_main_retry*.log`（最近一次推送结果、时间、成功与否）。
