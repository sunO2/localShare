#!/bin/bash

# 编译 Rust 动态库的脚本 (iOS/macOS)

set -e

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RUST_PROJECT_ROOT="$(cd "$PROJECT_ROOT/.." && pwd)"

# 检测目标平台
ARCH="$1"
PLATFORM="$2"

if [ -z "$PLATFORM" ]; then
  echo "Usage: $0 <arch> <platform>"
  echo "Example: $0 aarch64 ios"
  exit 1
fi

# 设置目标三元组
case "$ARCH-$PLATFORM" in
  x86_64-ios)
    TARGET="x86_64-apple-ios"
    ;;
  arm64-ios|aarch64-ios)
    TARGET="aarch64-apple-ios"
    ;;
  x86_64-macos)
    TARGET="x86_64-apple-darwin"
    ;;
  arm64-macos|aarch64-macos)
    TARGET="aarch64-apple-darwin"
    ;;
  universal-macos)
    # 同时编译两个架构
    "$0" x86_64 macos
    "$0" aarch64 macos
    exit 0
    ;;
  *)
    echo "Unsupported target: $ARCH-$PLATFORM"
    exit 1
    ;;
esac

echo "Building shareself for $TARGET..."

cd "$RUST_PROJECT_ROOT"

# 编译动态库
cargo build --release --target "$TARGET"

# 输出目录
OUTPUT_DIR="$PROJECT_ROOT/ios/Frameworks"
mkdir -p "$OUTPUT_DIR"

# 复制动态库
if [ "$PLATFORM" = "ios" ]; then
  # iOS: 创建 framework
  FRAMEWORK_DIR="$OUTPUT_DIR/sharSelf.framework"
  mkdir -p "$FRAMEWORK_DIR/Versions/A"

  cp "target/$TARGET/release/libsharSelf.dylib" \
     "$FRAMEWORK_DIR/Versions/A/shareself"

  # 创建符号链接
  cd "$FRAMEWORK_DIR"
  ln -sf Versions/A/shareself shareself
  ln -sf Versions/A Current
  cd -

else
  # macOS: 直接复制 dylib
  cp "target/$TARGET/release/libsharSelf.dylib" "$OUTPUT_DIR/"
fi

echo "Build complete: $TARGET"
