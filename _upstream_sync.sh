#!/data/data/com.termux.files/usr/bin/bash
# L-SYuki 上游同步助手：拉取官方 LingChat 最新，输出相对上次同步的差异，供快速移植。
# 用法: bash _upstream_sync.sh            # fetch upstream 并显示差异
#       bash _upstream_sync.sh rebase     # 把当前分支 rebase 到最新 upstream/main
# 说明: 我们的"官方基准"文件 _upstream_base.txt 记录上次同步到的 upstream commit。
set -e
BASE_FILE="$HOME/lingchat-main/_upstream_base.txt"
REPO="$HOME/lingchat-main"
cd "$REPO"

echo "▶ fetch upstream ..."
git fetch upstream 2>&1 | tail -3

UPSTREAM_HEAD=$(git rev-parse --short upstream/main)
echo "▶ upstream/main = $UPSTREAM_HEAD"

if [ -f "$BASE_FILE" ]; then
  BASE=$(cat "$BASE_FILE" | head -1)
  echo "▶ 上次同步基准 = $BASE"
  if [ "$BASE" = "$UPSTREAM_HEAD" ]; then
    echo "✅ 无新上游更新（已同步到 $UPSTREAM_HEAD）"
    exit 0
  fi
  echo ""; echo "=== 自上次同步以来的新 commit ($(git rev-list --count "$BASE"..upstream/main) 个) ==="
  git log --oneline "$BASE"..upstream/main | head -40
else
  echo "▶ 无基准记录。全量上游 commit: $(git rev-list --count upstream/main)"
fi

if [ "${1:-}" = "rebase" ]; then
  echo ""; echo "▶ rebase 当前分支到 upstream/main ..."
  git rebase upstream/main 2>&1 | tail -8 || echo "⚠️ rebase 有冲突需手动解决"
  echo "▶ 更新基准为 $UPSTREAM_HEAD"
  echo "$UPSTREAM_HEAD" > "$BASE_FILE"
fi

echo ""; echo "▶ 若要按结构迁移（把我们的功能作为薄层叠在官方上，未来同步=合并 upstream），参考 docs/SYNC_GUIDE.md"
echo "▶ 更新基准: echo \"$UPSTREAM_HEAD\" > _upstream_base.txt"
