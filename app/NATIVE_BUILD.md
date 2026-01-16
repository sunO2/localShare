# ShareSelf Flutter App - Native Build Guide

Flutter 客户端用于调用 Rust 编译的动态库。

## 项目结构

```
app/
├── lib/
│   ├── native/
│   │   ├── shareself_bindings.dart  # FFI 绑定定义
│   │   └── shareself_api.dart       # Dart API 包装器
│   └── main.dart
├── android/
│   └── build_rust.sh                # Android Rust 构建脚本
└── ios/
    └── Runner/Brigde.sh             # iOS/macOS Rust 构建脚本

src/
├── lib.rs                            # Rust 库入口
├── ffi.rs                            # FFI C 兼容层
└── ...
```

## 构建步骤

### 1. 安装 Rust 目标架构

```bash
# Android
rustup target add aarch64-linux-android
rustup target add armv7-linux-androideabi
rustup target add x86_64-linux-android

# iOS
rustup target add aarch64-apple-ios
rustup target add x86_64-apple-ios

# macOS
rustup target add x86_64-apple-darwin
rustup target add aarch64-apple-darwin
```

### 2. 构建 Rust 动态库

#### Android
```bash
cd app/android
chmod +x build_rust.sh
./build_rust.sh
```
这将生成 `.so` 文件到 `android/app/src/main/jniLibs/`。

#### iOS
```bash
cd app/ios/Runner
chmod +x Brigde.sh
# 编译模拟器版本 (x86_64)
./Brigde.sh x86_64 ios
# 编译真机版本 (aarch64)
./Brigde.sh aarch64 ios
```

#### macOS
```bash
cd app/ios/Runner
# 编译通用二进制文件
./Brigde.sh universal macos
```

### 3. 运行 Flutter 应用

```bash
cd app
flutter pub get
flutter run
```

## 开发注意事项

### 内存管理
FFI 返回的指针必须手动释放：
- 使用 `shareself_free_string()` 释放字符串
- 使用 `shareself_free_result()` 释放结果
- 使用 `shareself_free_device_list()` 释放设备列表

### 错误处理
所有 FFI 函数通过 `FfiResult` 返回错误码和消息：
- `error_code == 0` 表示成功
- `error_message` 包含错误详情 (成功时为 null)

### 调试
```dart
final api = ShareSelfApi();
print('Version: ${api.getVersion()}');
final (success, error) = await api.startDiscovery(8080);
if (!success) {
  print('Error: $error');
}
```

## 依赖

- Flutter SDK (>= 3.10)
- Rust toolchain (>= 1.70)
- NDK (Android)
- Xcode (iOS/macOS)
