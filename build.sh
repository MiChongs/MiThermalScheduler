#!/usr/bin/env bash
# 构建 thermalctl 与 WebUI，并打包为可刷入的模块 zip（输出到 dist/）
set -euo pipefail
cd "$(dirname "$0")"

API=29
NDK="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
if [ -z "$NDK" ]; then
    NDK=$(ls -d "$HOME"/Android/Sdk/ndk/* 2>/dev/null | sort -V | tail -n 1 || true)
fi
[ -d "$NDK" ] || { echo "未找到 Android NDK，请设置 ANDROID_NDK_HOME" >&2; exit 1; }
TOOLCHAIN=$(ls -d "$NDK"/toolchains/llvm/prebuilt/* | head -n 1)

VERSION=$(sed -n 's/^version=//p' module/module.prop)

echo "==> 单元测试"
(cd thermalctl && cargo test --release --quiet)

echo "==> 编译 thermalctl (aarch64-linux-android)"
rustup target add aarch64-linux-android > /dev/null
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLCHAIN/bin/aarch64-linux-android${API}-clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_AR="$TOOLCHAIN/bin/llvm-ar"
# rusqlite 内置的 SQLite 需要用 NDK 编译 C 代码
export CC_aarch64_linux_android="$TOOLCHAIN/bin/aarch64-linux-android${API}-clang"
export AR_aarch64_linux_android="$TOOLCHAIN/bin/llvm-ar"
(cd thermalctl && cargo build --release --target aarch64-linux-android)
mkdir -p module/bin
cp thermalctl/target/aarch64-linux-android/release/thermalctl module/bin/thermalctl

echo "==> 构建 WebUI"
(cd webui && { [ -d node_modules ] || npm ci; } && npm run build)

echo "==> 打包"
mkdir -p dist
OUT="dist/MiThermalScheduler-${VERSION}.zip"
rm -f "$OUT"
(cd module && zip -qr "../$OUT" .)
echo "完成：$OUT"
