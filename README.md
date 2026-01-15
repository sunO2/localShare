# sharSelf

一个用 Rust 实现的局域网文件分享库，支持多平台集成。

## 概述

sharSelf 是一个灵活的局域网文件分享解决方案，设计为可编译成动态库供多平台应用调用。它使用 mDNS + DNS-SD 协议进行设备发现，使用 BitTorrent 协议进行点对点文件传输。

## 核心功能

- **设备发现**: 基于 mDNS + DNS-SD 的跨平台设备发现
- **文件传输**: 基于 BitTorrent 协议的高效点对点文件传输
  - 支持文件分块和并行下载
  - 自动进度跟踪和恢复
  - Piece 完整性验证（SHA1）
- **TUI 界面**: 交互式终端界面，支持文件浏览和传输管理
- **多平台支持**: 支持 Linux、macOS、Windows、Android

---

## 设备发现服务设计

### 技术方案

采用 **mDNS (Multicast DNS)** + **DNS-SD (DNS-Based Service Discovery)** 标准协议：

| 组件 | 协议 | 端口 | 说明 |
|------|------|------|------|
| 服务广播 | mDNS | 5353 | UDP 组播 224.0.0.251 (IPv4) / ff02::fb (IPv6) |
| 服务类型 | DNS-SD | - | `_shareself._tcp.local` |

### 模块架构

```
src/
├── lib.rs                      # 库入口，导出公共 API
│
├── discovery/                  # 设备发现模块
│   ├── mod.rs
│   ├── service.rs             # 服务注册与发现核心 ✅
│   ├── browser.rs             # 服务浏览 ✅
│   ├── registrar.rs           # 服务注册 ✅
│   ├── resolver.rs            # 服务解析 ✅
│   └── types.rs               # 共享数据结构 ✅
│
├── mdns/                      # mDNS 协议实现（底层）
│   ├── mod.rs
│   ├── packet.rs              # DNS 数据包编解码 ✅
│   ├── socket.rs              # mDNS socket 封装 ✅
│   ├── query.rs               # mDNS 查询处理 ✅
│   └── response.rs            # mDNS 响应处理 ✅
│
├── torrent/                   # BitTorrent 文件传输模块
│   ├── mod.rs
│   ├── metainfo.rs            # Torrent 文件格式 ✅
│   ├── piece.rs               # Piece 管理和验证 ✅
│   ├── protocol.rs            # BitWire 协议 ✅
│   ├── peer.rs                # Peer 连接管理 ✅
│   ├── seeder.rs              # 上传服务 ✅
│   └── downloader.rs          # 下载客户端 ✅
│
├── ui/                        # TUI 界面模块
│   ├── mod.rs
│   ├── app.rs                 # 主应用程序 ✅
│   └── file_browser.rs        # 文件浏览器 ✅
│
└── common/                    # 公共模块
    ├── mod.rs
    ├── error.rs               # 错误类型定义 ✅
    └── config.rs              # 配置管理 ✅
```

### 核心数据结构

```rust
// discovery/types.rs

/// 服务标识符
pub struct ServiceIdentifier {
    pub service_type: String,  // "_shareself._tcp"
    pub domain: String,        // "local"
}

/// 设备信息
pub struct DeviceInfo {
    pub name: String,                    // 设备名称
    pub hostname: String,                  // 主机名
    pub addresses: Vec<SocketAddr>,        // IP 地址列表
    pub port: u16,                         // 服务端口
    pub txt_records: TxtRecord,            // TXT 记录
    pub service_type: String,              // 服务类型
    pub last_seen: Instant,                // 最后发现时间
}

/// 共享文件信息
pub struct SharedFile {
    pub name: String,       // 文件名
    pub info_hash: String,  // Info Hash (40位十六进制)
    pub size: Option<u64>,  // 文件大小
}
```

### 公共 API 设计

```rust
// lib.rs - 主 API

/// 创建设备发现服务
pub fn discovery_service(config: DiscoveryConfig) -> Result<DiscoveryHandle>;

/// 注册当前设备到局域网
pub fn register_service(config: ServiceConfig) -> Result<ServiceHandle>;
```

---

## 文件传输服务设计

### 技术方案

采用 **BitTorrent** 协议实现点对点文件传输：

| 组件 | 协议 | 端口 | 说明 |
|------|------|------|------|
| 文件传输 | BitTorrent | 6881 (默认) | 点对点文件传输 |
| 服务类型 | DNS-SD | - | `_shareself._tcp.local` |

### BitTorrent 模块架构

```
torrent/
├── metainfo.rs      # .torrent 文件格式
├── piece.rs         # Piece 管理和磁盘 I/O
├── protocol.rs      # BitWire 协议实现
├── peer.rs          # Peer 连接管理
├── seeder.rs        # 上传服务（Seed）
└── downloader.rs    # 下载客户端（Leech）
```

**核心功能：**
- ✅ Torrent 文件创建（.torrent）
- ✅ Piece 管理（分块、验证、存储）
- ✅ BitWire 协议实现（握手、消息交换）
- ✅ Seeder（上传服务）
- ✅ Downloader（下载客户端）
- ✅ SHA1 完整性校验
- ✅ 动态 TXT 记录更新（广播共享文件）

---

## TUI 界面

### 功能特性

交互式终端界面，提供完整的文件共享体验：

**主界面：**
- 📱 设备列表 - 显示局域网内的设备
- 📁 文件浏览器 - 浏览本地文件系统
- 📤 传输列表 - 管理上传和下载任务

**快捷键：**
```
全局：
  q / Esc    退出程序
  Tab       切换焦点

设备列表：
  ↑/k/j/↓   选择设备
  Enter     查看设备共享的文件
  a         全选/取消全选

文件浏览器：
  ↑/k/j/↓   选择文件/目录
  Enter     进入目录
  Backspace 返回上级目录
  s         共享选中的文件

传输列表：
  ↑/k/j/↓   选择任务
  d         删除任务
  t         切换到此界面

共享文件列表：
  ↑/k/j/↓   选择文件
  Enter/d   下载文件
  Esc/h     返回设备列表
```

---

## 快速开始

### 编译运行演示程序

```bash
# 编译项目
cargo build --release

# 运行 TUI 界面
cargo run --release --bin shareself

# 或运行交互式菜单
cargo run --release
```

### TUI 使用流程

1. **启动程序**：选择 `6` 进入 TUI 界面

2. **共享文件**：
   ```
   按 Tab 切换到文件浏览器
   选择文件
   按 s 键共享
   按 t 切换到传输列表查看进度
   ```

3. **浏览设备**：
   ```
   设备列表自动显示局域网内的设备
   按 Enter 查看设备共享的文件
   ```

4. **下载文件**：
   ```
   在共享文件列表中选择文件
   按 Enter 或 d 开始下载
   按 t 查看传输进度
   ```

---

## 当前状态

> ✅ **核心功能已完成**
>
> - ✅ mDNS 协议完整实现
> - ✅ 设备发现服务（注册、浏览、解析）
> - ✅ BitTorrent 文件传输协议
> - ✅ TUI 交互式界面
> - ✅ 文件共享和下载功能
> - ✅ 动态 mDNS TXT 记录更新
> - ✅ 实时传输进度显示

### 已实现功能

#### Phase 1: 设备发现服务 ✅
- [x] mDNS 数据包编解码
- [x] 跨平台 mDNS socket
- [x] 服务注册和广播
- [x] 服务浏览和解析
- [x] 设备发现事件系统
- [x] 自动设备清理和重广播

#### Phase 2: 文件传输 ✅
- [x] BitTorrent 协议实现
- [x] Torrent 文件创建
- [x] Piece 管理和验证
- [x] Seeder（上传服务）
- [x] Downloader（下载客户端）
- [x] SHA1 完整性校验
- [x] 动态 TXT 记录更新

#### Phase 3: TUI 界面 ✅
- [x] 设备列表显示
- [x] 文件浏览器
- [x] 传输任务管理
- [x] 实时进度显示
- [x] 文件共享功能
- [x] 文件下载功能

### 已知问题

#### Android/Termux 权限限制

在未 root 的 Android/Termux 环境中：
- 程序可以正常编译和运行
- TUI 界面完全可用
- 文件共享功能正常（通过 seeder 广播）
- mDNS 功能受限（无法绑定 5353 端口或发送组播）

**建议：** 在桌面环境（Linux/macOS/Windows）测试完整的 mDNS 设备发现功能。

---

## 依赖项

```toml
[dependencies]
# 异步运行时
tokio = { version = "1.35", features = ["full"] }

# DNS 协议
trust-dns-client = "0.23"
trust-dns-proto = "0.23"

# 网络与 socket
socket2 = "0.5"

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 错误处理
thiserror = "1"
anyhow = "1"

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# 工具
once_cell = "1.19"
gethostname = "0.4"
get_if_addrs = "0.1"
walkdir = "2.4"
sha1 = "0.10"
rand = "0.8"
hex = "0.4"

# TUI 界面
ratatui = "0.26"
crossterm = "0.27"

[build-dependencies]
cbindgen = "0.26"

[target.'cfg(target_os = "android")'.dependencies]
jni = { version = "0.21", optional = true }
ndk-glue = { version = "0.7", optional = true }

[features]
default = []
android = ["jni", "ndk-glue"]
```

---

## 使用示例

### TUI 界面使用

```bash
# 运行 TUI 界面
cargo run --bin shareself

# 选择 6 进入 TUI
# 按提示操作即可
```

### 命令行模式

```bash
# 运行交互式菜单
cargo run --bin shareself

# 选项：
# 1. 仅注册自己
# 2. 仅浏览设备
# 3. 同时运行
# 4. 快速测试
# 5. 共享文件（命令行模式）
# 6. TUI 界面
```

---

## 开发路线图

### Phase 1: 设备发现服务 ✅
- [x] mDNS 协议实现
- [x] 设备发现核心功能
- [x] 服务注册和浏览
- [x] 事件系统

### Phase 2: 文件传输 ✅
- [x] BitTorrent 协议实现
- [x] Torrent 文件支持
- [x] Piece 管理和验证
- [x] Seeder 和 Downloader

### Phase 3: TUI 界面 ✅
- [x] 设备列表
- [x] 文件浏览器
- [x] 传输管理
- [x] 进度显示

### Phase 4: 平台集成 (规划中)
- [ ] C FFI 层
- [ ] Android JNI 绑定
- [ ] 构建脚本优化

### Phase 5: 高级功能 (规划中)
- [ ] TLS 加密传输
- [ ] 设备认证
- [ ] 断点续传
- [ ] 多文件并行传输

---

## 测试

```bash
# 运行所有测试
cargo test

# 运行特定模块测试
cargo test --package sharSelf --lib discovery
cargo test --package sharSelf --lib torrent

# 运行文档测试
cargo test --doc
```

---

## 构建动态库

```bash
# Android (需要 NDK)
cargo ndk --platform 21 --bindgen

# 通用动态库
cargo build --release --lib

# 生成 C 头文件
cargo build --release
cbindgen --crate sharSelf --output sharSelf.h
```

---

## 架构设计

```
┌─────────────────────────────────────────────────────────┐
│                   TUI / CLI Application                    │
└─────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                   Core Library                           │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  Discovery   │  │   Torrent    │  │     UI       │  │
│  │   Module     │  │   Module     │  │   Module     │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                   tokio Runtime                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │ mDNS Socket │  │ BitTorrent  │  │   Event Loop    │  │
│  │   (UDP)     │  │   (TCP)     │  │                 │  │
│  └─────────────┘  └─────────────┘  └─────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

---

## 许可证

MIT OR Apache-2.0
