# sharSelf 开发进度文档

## 项目概述

sharSelf 是一个用 Rust 实现的局域网文件分享库，支持多平台集成。使用 mDNS + DNS-SD 协议实现设备发现，使用 BitTorrent 协议实现点对点文件传输。

---

## 已完成的功能

### Phase 1: 设备发现服务 ✅

#### 1.1 mDNS 协议实现 (src/mdns/)

| 模块 | 文件 | 状态 | 说明 |
|------|------|------|------|
| 数据包编解码 | packet.rs | ✅ | 完整实现 RFC 1035 DNS 协议 |
| Socket 封装 | socket.rs | ✅ | 跨平台 mDNS socket，支持 IPv4/IPv6 |
| 查询处理 | query.rs | ✅ | PTR、SRV、A、AAAA、TXT 查询 |
| 响应处理 | response.rs | ✅ | 响应构建器和相关工具 |

**核心功能：**
- DNS 数据包编码/解码（A、AAAA、PTR、TXT、SRV 记录）
- DNS 名称压缩指针
- 跨平台 socket 创建（Linux/Android/macOS/Windows）
- 组播地址绑定和发送
- 异步查询发送和接收

#### 1.2 设备发现服务 (src/discovery/)

| 模块 | 文件 | 状态 | 说明 |
|------|------|------|------|
| 服务主逻辑 | service.rs | ✅ | 设备发现服务核心 |
| 服务注册 | registrar.rs | ✅ | mDNS 服务广播和注册，支持动态 TXT 更新 |
| 服务浏览 | browser.rs | ✅ | 服务浏览和解析 |
| 服务解析 | resolver.rs | ✅ | 服务地址解析 |
| 类型定义 | types.rs | ✅ | 共享数据结构，包括 SharedFile |

**核心功能：**
- 设备发现服务：监听 mDNS 响应，生成设备事件
- 服务注册服务：广播自己的设备信息，响应查询
- 动态 TXT 记录更新：用于广播共享文件信息
- 事件系统：DeviceFound、DeviceLost、DeviceUpdated、Error
- 自动清理过期设备
- 定期重广播机制

#### 1.3 公共模块 (src/common/)

| 模块 | 文件 | 状态 | 说明 |
|------|------|------|------|
| 错误处理 | error.rs | ✅ | 统一错误类型定义 |
| 配置管理 | config.rs | ✅ | DiscoveryConfig、ServiceConfig |

---

### Phase 2: 文件传输服务 ✅

#### 2.1 BitTorrent 协议实现 (src/torrent/)

| 模块 | 文件 | 状态 | 说明 |
|------|------|------|------|
| 元信息 | metainfo.rs | ✅ | .torrent 文件格式，Bencode 编解码 |
| Piece 管理 | piece.rs | ✅ | Piece 管理、磁盘 I/O、SHA1 验证 |
| 协议实现 | protocol.rs | ✅ | BitWire 协议（握手、消息交换） |
| Peer 连接 | peer.rs | ✅ | Peer 连接管理 |
| 上传服务 | seeder.rs | ✅ | 种子上传服务 |
| 下载客户端 | downloader.rs | ✅ | Leech 下载客户端 |

**核心功能：**
- Torrent 文件创建（支持文件和目录）
- Bencode 编码/解码
- Piece 管理（分块、验证、存储）
- SHA1 完整性校验
- BitWire 协议实现：
  - Handshake 握手
  - 消息交换（choke, unchoke, interested, uninterested, have, bitfield, request, piece）
- Seeder（上传服务）：响应 peer 请求，发送 piece 数据
- Downloader（下载客户端）：连接 seeder，下载文件
- 并行下载和进度跟踪

#### 2.2 文件共享集成

- ✅ 与设备发现服务集成
- ✅ 通过 mDNS TXT 记录广播共享文件
- ✅ 动态更新 TXT 记录以反映当前共享状态

---

### Phase 3: TUI 界面 ✅

#### 3.1 交互式终端界面 (src/ui/)

| 模块 | 文件 | 状态 | 说明 |
|------|------|------|------|
| 主应用 | app.rs | ✅ | TUI 主应用程序 |
| 文件浏览器 | file_browser.rs | ✅ | 文件系统浏览组件 |

**核心功能：**
- 设备列表：显示局域网内的设备
- 文件浏览器：浏览本地文件系统
- 传输列表：管理上传和下载任务
- 共享文件列表：查看设备共享的文件
- 实时进度显示：上传/下载进度更新
- 事件驱动架构：后台任务与主线程通信

**键盘快捷键：**
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

#### 3.2 事件通道架构

**关键设计决策：**
- 主线程 → 后台任务：使用 `transfer_tx` (mpsc sender)
- 后台任务 → 主线程：使用 `event_back_tx` (独立的 mpsc channel)

这种双向通信架构确保：
1. 主线程可以向后台任务发送命令（开始下载、取消任务等）
2. 后台任务可以向主线程报告进度（下载进度、完成状态等）
3. 避免通道所有权冲突

#### 3.3 日志系统

- ✅ TUI 模式下日志输出到 `/tmp/shareself.log`
- ✅ 其他模式日志输出到控制台
- ✅ 使用 tracing/tracing-subscriber 实现结构化日志

---

## 当前存在的问题

### 🔴 高优先级问题

#### 1. Android/Termux 权限限制

**问题描述：**
在未 root 的 Android/Termux 环境中，程序无法：
- 绑定到 mDNS 端口 5353
- 发送组播数据包
- 加入组播组

**错误信息：**
```
WARN Permission denied when sending mDNS packet. This is expected on unrooted Android/Termux.
```

**临时解决方案：**
- ✅ 已实现优雅的错误处理，程序不会崩溃
- ✅ 权限错误转换为警告
- ✅ TUI 界面完全可用
- ✅ 文件共享功能正常（通过 seeder 广播）

**已知限制：**
- mDNS 设备发现功能在 Android/Termux 上受限
- 可以使用本机 IP 直接连接进行测试

**长期解决方案：**
- [ ] 编译为真正的 Android APK，添加必要的权限到 `AndroidManifest.xml`
- [ ] 使用 Android NDK 的 multicast API
- [ ] 在桌面环境（Linux/macOS/Windows）中测试完整功能

### 🟡 中优先级问题

#### 2. get_local_ip_addresses 实现简化

**问题描述：**
由于 `get_if_addrs` 0.1.x 版本使用了不同的 IpAddr 类型，当前实现简化为只返回回环地址。

**影响：**
- 服务广播的 IP 地址是 127.0.0.1 而不是真实的局域网 IP
- 其他设备无法通过 mDNS 发现连接到本机服务

**临时方案：**
在演示程序中使用 UdpSocket 连接外部地址来获取本机 IP：
```rust
fn get_local_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let local_addr = socket.local_addr().ok()?;
    Some(local_addr.ip().to_string())
}
```

**需要改进：**
- [ ] 实现 get_if_addrs 类型转换
- [ ] 在 ServiceConfig 中支持手动指定 IP 地址

#### 3. 编译警告

运行 `cargo build` 会产生一些警告，主要是：
- 未使用的导入
- 未使用的变量
- 未使用的代码

这些不影响功能，但应该清理以提高代码质量。

### 🟢 低优先级问题

#### 4. IPv6 地址解析 (已修复)

**问题描述：**
`send_to_v6` 方法中 IPv6 地址格式错误导致 panic。

**修复：**
```rust
// 修复前
let addr: SocketAddr = format!("{}:{}", MDNS_IPV6, MDNS_PORT).parse().unwrap();

// 修复后
let addr: SocketAddr = format!("[{}]:{}", MDNS_IPV6, MDNS_PORT).parse().unwrap();
```

---

## 待开发功能

### Phase 4: 平台集成 (规划中)

#### 4.1 FFI 层实现
- [ ] C ABI 头文件定义
- [ ] 使用 cbindgen 生成头文件
- [ ] 实现 C 兼容的 API

#### 4.2 Android 集成
- [ ] JNI 绑定实现
- [ ] Gradle 构建脚本
- [ ] Android 权限配置
- [ ] Android 服务生命周期管理

#### 4.3 其他平台
- [ ] iOS/CocoaPods 集成
- [ ] Node.js Neon/NAPI-RS 绑定
- [ ] Python 扩展（PyO3）

### Phase 5: 高级功能 (规划中)

#### 5.1 安全性
- [ ] TLS 加密传输
- [ ] 设备认证机制
- [ ] 传输加密

#### 5.2 性能优化
- [ ] 多文件并行传输
- [ ] 连接池管理
- [ ] 传输速率限制

#### 5.3 用户体验
- [ ] 设备图标和元数据
- [ ] 传输历史记录
- [ ] 设备别名设置
- [ ] 断点续传

---

## 技术债务

### 需要重构的部分

1. **类型转换工具函数**
   - get_if_addrs 的 IpAddr 类型转换应该提取为独立模块

2. **配置系统增强**
   - 支持从文件加载配置
   - 环境变量支持

3. **错误处理改进**
   - 更细粒度的错误类型
   - 错误恢复策略

4. **测试覆盖**
   - [ ] 单元测试
   - [ ] 集成测试
   - [ ] 端到端测试

5. **代码清理**
   - [ ] 移除未使用的导入和变量
   - [ ] 清理编译警告
   - [ ] 添加文档注释

---

## 开发环境信息

### 当前环境
- **平台**: Android (Termux) / Linux
- **Rust 版本**: 1.x
- **目标架构**: aarch64-linux-android / x86_64-unknown-linux-gnu

### 编译信息
```bash
# Debug 构建
cargo build

# Release 构建
cargo build --release

# 运行演示程序
cargo run --release

# 运行 TUI 界面
cargo run --release --bin shareself
# 然后选择 6
```

### 项目结构
```
src/
├── lib.rs              # 库入口
├── main.rs             # 演示程序
├── discovery/          # 设备发现模块 ✅
│   ├── service.rs     # 服务发现核心
│   ├── registrar.rs   # 服务注册（支持动态 TXT 更新）
│   ├── browser.rs     # 服务浏览
│   ├── resolver.rs    # 服务解析
│   └── types.rs       # 类型定义（包含 SharedFile）
├── mdns/              # mDNS 协议实现 ✅
│   ├── packet.rs      # 数据包编解码
│   ├── socket.rs      # Socket 封装
│   ├── query.rs       # 查询处理
│   └── response.rs    # 响应处理
├── torrent/           # BitTorrent 文件传输模块 ✅
│   ├── metainfo.rs    # Torrent 文件格式（Bencode）
│   ├── piece.rs       # Piece 管理和验证
│   ├── protocol.rs    # BitWire 协议
│   ├── peer.rs        # Peer 连接管理
│   ├── seeder.rs      # 上传服务
│   └── downloader.rs  # 下载客户端
├── ui/                # TUI 界面模块 ✅
│   ├── app.rs         # 主应用程序
│   └── file_browser.rs # 文件浏览器
└── common/            # 公共模块 ✅
    ├── error.rs       # 错误类型
    └── config.rs      # 配置管理
```

---

## 测试指南

### 在桌面环境测试完整功能

由于 Android/Termux 的权限限制，建议在以下环境测试完整的 mDNS 功能：

1. **Linux**
   ```bash
   cargo run --release --bin shareself
   # 选择 6 进入 TUI 界面
   ```

2. **macOS**
   ```bash
   cargo run --release --bin shareself
   ```

3. **Windows**
   ```powershell
   cargo run --release --bin shareself
   ```

### 测试场景

1. **设备发现测试**: 两台设备在同一局域网运行程序，验证互相发现
2. **文件共享测试**:
   - 在文件浏览器中选择文件
   - 按 `s` 键共享
   - 在另一台设备上查看共享文件
3. **文件下载测试**:
   - 在共享文件列表中选择文件
   - 按 `Enter` 或 `d` 开始下载
   - 在传输列表中查看进度
4. **长时间运行**: 验证设备过期清理和重广播机制
5. **关闭测试**: 验证服务注销和退出是否正常

### 日志调试

```bash
# 在另一个终端查看实时日志
tail -f /tmp/shareself.log
```

---

## 下一步开发计划

### 短期目标 (1-2周)
1. [ ] 在桌面环境验证完整的 mDNS + 文件传输功能
2. [ ] 清理编译警告
3. [ ] 添加单元测试

### 中期目标 (1个月)
1. [ ] 实现 FFI 层
2. [ ] 修复 get_local_ip_addresses 问题
3. [ ] 添加断点续传功能

### 长期目标 (3个月)
1. [ ] 完成平台集成（Android/iOS/Node.js）
2. [ ] 添加安全性功能（TLS、认证）
3. [ ] 性能优化和测试覆盖

---

## 贡献指南

### 代码风格
- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 检查代码质量
- 遵循 Rust 命名规范

### 提交规范
- feat: 新功能
- fix: 修复 bug
- docs: 文档更新
- refactor: 重构
- test: 测试相关
- chore: 构建/工具相关

### 开发流程
1. Fork 项目
2. 创建功能分支
3. 提交更改
4. 推送到分支
5. 创建 Pull Request

---

*最后更新时间: 2026-01-16*
