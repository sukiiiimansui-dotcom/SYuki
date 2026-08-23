#!/data/data/com.termux/files/usr/bin/bash
# LingChat (Tauri Android) 构建环境一键准备脚本
# 用法：bash build_setup.sh
# 说明：在 Termux 或 proot debian 里均可跑；Android SDK/NDK 已存在于 ~/android-sdk。
set -e

PROJECT="${PROJECT:-$HOME/lingchat-main}"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
export ANDROID_SDK_ROOT="$ANDROID_HOME"

echo "==> 项目目录: $PROJECT"
echo "==> ANDROID_HOME: $ANDROID_HOME"

# ---------- 1. rustup + android target ----------
if ! command -v rustup >/dev/null 2>&1; then
  echo "==> 未检测到 rustup，安装..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  . "$HOME/.cargo/env"
fi
rustup toolchain list
echo "==> 添加 aarch64-linux-android target"
rustup target add aarch64-linux-android

# ---------- 2. 确认 NDK ----------
if [ ! -d "$ANDROID_HOME/ndk" ]; then
  echo "!! 未找到 NDK，请确认 ~/android-sdk/ndk/<版本> 存在（本机有 28.2.13676358）"
else
  echo "    NDK: $(ls "$ANDROID_HOME/ndk")"
fi

# ---------- 3. cpp / 链接器（cargo-ndk 需要 clang/llvm）----------
# Tauri Android 用 NDK 自带的 clang 链接；如缺系统 clang，按需安装：
#   pkg install clang llvm (Termux)  或  apt install clang llvm (proot debian)

# ---------- 4. pnpm install（前端 + tauri CLI）----------
cd "$PROJECT"
if [ ! -d node_modules ]; then
  echo "==> pnpm install ..."
  pnpm install
else
  echo "==> node_modules 已存在，跳过 install"
fi
echo "==> tauri CLI: $(ls node_modules/.bin/tauri 2>/dev/null || echo '(未装，请 pnpm install)')"

# ---------- 5. Android 构建 ----------
cd "$PROJECT"
echo "==> 开始 Android build (aarch64)..."
echo "==> 若首次报错 git 依赖(sbv2_core)/ort 下载失败，请确认能访问 github / gh-proxy.org"
pnpm android:build

echo "==> 完成。APK 输出: $PROJECT/src-tauri/gen/android/app/build/outputs/apk/*/app-arm64-v8a-*.apk"
