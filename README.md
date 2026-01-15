# sharSelf

一个用 Rust 实现的局域网文件分享库，支持多平台集成。

## 概述

sharSelf 是一个灵活的局域网文件分享解决方案，设计为可编译成动态库供多平台应用调用。

## 核心功能

- **设备发现**: 基于 mDNS + DNS-SD 的跨平台设备发现
- **文件传输**: 高效的点对点文件传输
- **多平台支持**: 支持 Linux、macOS、Windows、Android

---

## 设备发现服务设计

### 技术方案

采用 **mDNS (Multicast DNS)** + **DNS-SD (DNS-Based Service Discovery)** 标准协议：

| 组件 | 协议 | 端口 | 说明 |
|------|------|------|------|
| 服务广播 | mDNS | 5353 | UDP 组播 224.0.0.251 (IPv4) / ff02::fb (IPv6) |
| 服务类型 | DNS-SD | - | `_http._tcp.local` 或自定义服务类型 |

### 模块架构

```
src/
├── lib.rs                      # 库入口，导出公共 API
│
├── discovery/                  # 设备发现模块
│   ├── mod.rs
│   ├── service.rs             # 服务注册与发现核心
│   ├── browser.rs             # 服务浏览（被动发现设备）
│   ├── registrar.rs           # 服务注册（主动广播自己）
│   ├── resolver.rs            # 服务解析（获取设备详细信息）
│   └── types.rs               # 共享数据结构
│
├── mdns/                      # mDNS 协议实现（底层）
│   ├── mod.rs
│   ├── packet.rs              # DNS 数据包编解码
│   ├── socket.rs              # mDNS socket 封装（跨平台）
│   ├── query.rs               # mDNS 查询处理
│   └── response.rs            # mDNS 响应处理
│
├── transport/                 # 传输层模块（未来）
│   └── mod.rs
│
└── common/                    # 公共模块
    ├── mod.rs
    ├── error.rs               # 错误类型定义
    └── config.rs              # 配置管理
```

### 核心数据结构

```rust
// discovery/types.rs

/// 服务标识符
pub struct ServiceIdentifier {
    pub service_type: String,  // 如 "_shareself._tcp"
    pub domain: String,        // 默认 "local"
}

/// 设备信息
pub struct DeviceInfo {
    pub name: String,           // 设备名称
    pub hostname: String,       // 主机名
    pub addresses: Vec<SocketAddr>,  // IP 地址列表
    pub port: u16,              // 服务端口
    pub txt_records: HashMap<String, String>,  // 额外信息
}

/// 服务发现事件
pub enum DiscoveryEvent {
    DeviceFound(DeviceInfo),
    DeviceLost(String),         // 设备名称
    DeviceUpdated(DeviceInfo),
}
```

### 公共 API 设计

```rust
// lib.rs - 主 API

/// 创建设备发现服务
pub fn discovery_service(config: DiscoveryConfig) -> Result<DiscoveryHandle>;

/// 发现服务控制器
pub struct DiscoveryHandle {
    /// 订阅发现事件
    pub fn subscribe(&self) -> mpsc::Receiver<DiscoveryEvent>;

    /// 主动搜索设备
    pub fn browse(&self) -> Result<()>;

    /// 停止发现服务
    pub fn shutdown(self) -> Result<()>;
}

/// 服务注册控制器
pub struct ServiceHandle {
    /// 更新 TXT 记录
    pub fn update_txt(&self, records: HashMap<String, String>) -> Result<()>;

    /// 注销服务
    pub fn unregister(self) -> Result<()>;
}

/// 注册当前设备到局域网
pub fn register_service(config: ServiceConfig) -> Result<ServiceHandle>;
```

### 依赖规划

```toml
[dependencies]
# 异步运行时
tokio = { version = "1", features = ["full"] }

# DNS 协议
trust-dns-client = "0.23"      # DNS 解析
trust-dns-proto = "0.23"       # DNS 协议

# 网络与序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 错误处理
thiserror = "1"
anyhow = "1"

# 日志
tracing = "0.1"
tracing-subscriber = "0.3"

# 跨平台 socket
socket2 = "0.5"

[build-dependencies]
# 可能需要编译时生成一些绑定
cbindgen = "0.24"  # 用于生成 C 头文件

[target.'cfg(target_os = "android")'.dependencies]
# Android 特定依赖
jni = "0.21"       # JNI 支持
```

### 线程模型

```
┌─────────────────────────────────────────────────────────┐
│                   应用层 (Host Application)              │
└─────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                  sharSelf FFI Layer                      │
│  (C ABI / JNI / N-API 等，根据平台选择)                  │
└─────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                   Core Library                           │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  Discovery   │  │  Transfer    │  │   Common     │  │
│  │   Module     │  │   Module     │  │   Module     │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                   tokio Runtime                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │ mDNS Socket │  │ DNS Parser  │  │  Event Loop     │  │
│  │   (UDP)     │  │             │  │                 │  │
│  └─────────────┘  └─────────────┘  └─────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### 平台集成方式

| 平台 | 集成方式 | 生成文件 |
|------|----------|----------|
| Android | JNI / AAR | `.aar` + `.so` |
| iOS | CocoaPods | `.framework` / `.xcframework` |
| Desktop | C FFI | `.so` / `.dylib` / `.dll` + `.h` |
| Node.js | Neon / NAPI-RS | `.node` |

### 配置示例

```rust
// 使用示例
let config = DiscoveryConfig {
    service_name: "MyDevice".to_string(),
    service_type: "_shareself._tcp".to_string(),
    port: 8080,
    txt_records: [
        ("version".to_string(), "0.1.0".to_string()),
        ("platform".to_string(), "android".to_string()),
    ].into(),
};

// 注册自己
let service = register_service(config)?;

// 发现其他设备
let discovery = discovery_service(DiscoveryConfig::default())?;
let mut events = discovery.subscribe()?;

tokio::spawn(async move {
    while let Some(event) = events.recv().await {
        match event {
            DiscoveryEvent::DeviceFound(device) => {
                println!("Found device: {}", device.name);
            }
            _ => {}
        }
    }
});
```

---

## 快速开始

### 编译运行演示程序

```bash
# 编译项目
cargo build --release

# 运行演示程序
cargo run --release

# 或直接运行二进制文件
./target/release/shareself
```

### 演示程序功能

演示程序 `shareself` 提供了以下功能选项：

1. **仅注册自己** - 将当前设备注册到局域网，让其他设备可以发现
2. **仅浏览设备** - 扫描局域网，发现其他运行 sharSelf 的设备
3. **同时运行** - 既注册自己又浏览其他设备
4. **快速测试** - 注册并扫描 10 秒钟，用于快速验证功能

### 当前状态

> ⚠️ **注意**: 项目目前处于开发早期阶段
>
> - ✅ 完成的基础架构和模块设计
> - ✅ 核心数据结构和 API 定义
> - ✅ 跨平台 socket 封装
> - ⏳ mDNS 数据包编解码（TODO）
> - ⏳ 完整的设备发现逻辑（TODO）

目前运行演示程序不会发现真实设备，因为 mDNS 协议的核心编解码逻辑还需要实现。但框架已经搭建完成，可以作为后续开发的起点。

---

## 开发路线图

### Phase 1: 设备发现服务 (当前)
- [x] 模块架构设计
- [x] 核心数据结构定义
- [x] mDNS socket 封装
- [ ] mDNS packet 编解码
- [ ] 服务注册功能
- [ ] 服务浏览功能
- [ ] 基础事件系统

### Phase 2: 传输层
- [ ] HTTP 服务器
- [ ] HTTP 客户端
- [ ] 文件上传/下载
- [ ] 进度回调

### Phase 3: 平台集成
- [ ] C FFI 层
- [ ] Android JNI 绑定
- [ ] 构建脚本

### Phase 4: 高级功能
- [ ] TLS 加密传输
- [ ] 设备认证
- [ ] 断点续传
- [ ] 多文件传输

---

## 测试

```bash
# 运行所有测试
cargo test

# 运行特定模块测试
cargo test --package sharSelf --lib discovery

# 运行集成测试
cargo test --test '*'
```

---

## 构建动态库

```bash
# Android (需要 NDK)
cargo ndk --platform 21 --bindgen

# 通用动态库
cargo build --release --lib
```

---

## 许可证

MIT OR Apache-2.0
