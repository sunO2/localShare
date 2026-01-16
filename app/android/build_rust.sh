#!/bin/bash

# 编译 Rust 动态库的脚本 (Android)

set -e

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUST_PROJECT_ROOT="$(cd "$PROJECT_ROOT/.." && pwd)"
JNI_LIBS_DIR="$PROJECT_ROOT/android/app/src/main/jniLibs"

# Android ABI 列表
ABIS=(
  "arm64-v8a:aarch64-linux-android"
  "armeabi-v7a:armv7-linux-androideabi"
  "x86_64:x86_64-linux-android"
)

mkdir -p "$JNI_LIBS_DIR"

for abi_target in "${ABIS[@]}"; do
  IFS=':' read -ra PARTS <<< "$abi_target"
  ABI="${PARTS[0]}"
  TARGET="${PARTS[1]}"

  echo "Building shareself for $ABI ($TARGET)..."

  cd "$RUST_PROJECT_ROOT"
  cargo build --release --target "$TARGET"

  # 复制到 jniLibs
  mkdir -p "$JNI_LIBS_DIR/$ABI"
  cp "target/$TARGET/release/libsharSelf.so" "$JNI_LIBS_DIR/$ABI/"

  echo "Built: $JNI_LIBS_DIR/$ABI/libsharSelf.so"
done

echo "All Android builds complete!"
