# sharSelf 开发进度文档

## 项目概述

sharSelf 是一个用 Rust 实现的局域网文件分享库，支持多平台集成。使用 mDNS + DNS-SD 协议实现设备发现功能。

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
| 服务注册 | registrar.rs | ✅ | mDNS 服务广播和注册 |
| 服务浏览 | browser.rs | 🚧 | 基础框架已实现 |
| 服务解析 | resolver.rs | 🚧 | 基础框架已实现 |
| 类型定义 | types.rs | ✅ | 共享数据结构 |

**核心功能：**
- 设备发现服务：监听 mDNS 响应，生成设备事件
- 服务注册服务：广播自己的设备信息，响应查询
- 事件系统：DeviceFound、DeviceLost、DeviceUpdated、Error
- 自动清理过期设备
- 定期重广播机制

#### 1.3 公共模块 (src/common/)

| 模块 | 文件 | 状态 | 说明 |
|------|------|------|------|
| 错误处理 | error.rs | ✅ | 统一错误类型定义 |
| 配置管理 | config.rs | ✅ | DiscoveryConfig、ServiceConfig |

#### 1.4 演示程序 (src/main.rs)

- ✅ 交互式菜单系统
- ✅ 4种运行模式：仅注册、仅浏览、同时运行、快速测试
- ✅ 设备信息显示
- ✅ 优雅的关闭处理

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
- 已实现优雅的错误处理，程序不会崩溃
- 权限错误转换为警告

**长期解决方案：**
- [ ] 编译为真正的 Android APK，添加必要的权限到 `AndroidManifest.xml`
- [ ] 使用 Android NDK 的 multicast API
- [ ] 在桌面环境（Linux/macOS/Windows）中测试完整功能

#### 2. Ctrl+C 退出需要多次

**状态：** 🟡 已修复但待验证

**问题描述：**
之前需要按多次 Ctrl+C 才能退出程序，因为后台的 `spawn_blocking` 线程没有正确终止。

**已实施的修复：**
- ✅ 添加 `task_handle` 字段跟踪后台任务
- ✅ 在 shutdown 时调用 `handle.abort()`
- ✅ 添加 100ms 等待时间让任务清理

**需要验证：**
- [ ] 用户测试确认 Ctrl+C 现在可以正常退出

### 🟡 中优先级问题

#### 3. get_local_ip_addresses 实现简化

**问题描述：**
由于 `get_if_addrs` 0.1.x 版本使用了不同的 IpAddr 类型，当前实现简化为只返回回环地址。

**影响：**
- 服务广播的 IP 地址是 127.0.0.1 而不是真实的局域网 IP
- 其他设备无法连接到本机服务

**临时方案：**
```rust
// 当前简化实现
ips.push(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
```

**需要改进：**
- [ ] 实现 get_if_addrs 类型转换
- [ ] 或者使用其他方法获取本机 IP
- [ ] 或者在 ServiceConfig 中允许手动指定 IP 地址

#### 4. IPv6 地址解析崩溃 (已修复)

**问题描述：**
`send_to_v6` 方法中 IPv6 地址格式错误导致 panic。

**修复：**
```rust
// 修复前
let addr: SocketAddr = format!("{}:{}", MDNS_IPV6, MDNS_PORT).parse().unwrap();

// 修复后
let addr: SocketAddr = format!("[{}]:{}", MDNS_IPV6, MDNS_PORT).parse().unwrap();
```

### 🟢 低优先级问题

#### 5. 编译警告

运行 `cargo build` 会产生一些警告，主要是：
- 未使用的导入
- 未使用的变量
- 未使用的代码

这些不影响功能，但应该清理以提高代码质量。

---

## 待开发功能

### Phase 2: 传输层 (下一步)

#### 2.1 HTTP 服务器
- [ ] 创建 HTTP 服务器用于文件传输
- [ ] 实现 GET 请求处理（文件下载）
- [ ] 实现 POST 请求处理（文件上传）
- [ ] 支持 CORS（跨域请求）
- [ ] 请求认证和安全验证

#### 2.2 HTTP 客户端
- [ ] 实现文件上传客户端
- [ ] 实现文件下载客户端
- [ ] 支持断点续传
- [ ] 进度回调

#### 2.3 文件管理
- [ ] 文件列表 API
- [ ] 目录浏览
- [ ] 文件元数据获取
- [ ] 文件删除/重命名

### Phase 3: 平台集成

#### 3.1 FFI 层实现
- [ ] C ABI 头文件定义
- [ ] 使用 cbindgen 生成头文件
- [ ] 实现 C 兼容的 API

#### 3.2 Android 集成
- [ ] JNI 绑定实现
- [ ] Gradle 构建脚本
- [ ] Android 权限配置
- [ ] Android 服务生命周期管理

#### 3.3 其他平台
- [ ] iOS/CocoaPods 集成
- [ ] Node.js Neon/NAPI-RS 绑定
- [ ] Python 扩展（PyO3）

### Phase 4: 高级功能

#### 4.1 安全性
- [ ] TLS/HTTPS 支持
- [ ] 设备认证机制
- [ ] 传输加密

#### 4.2 性能优化
- [ ] 文件分块传输
- [ ] 并发传输支持
- [ ] 连接池管理

#### 4.3 用户体验
- [ ] 设备图标和元数据
- [ ] 传输历史记录
- [ ] 设备别名设置

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

---

## 开发环境信息

### 当前环境
- **平台**: Android (Termux)
- **Rust 版本**: 1.x
- **目标架构**: aarch64-linux-android

### 编译信息
```bash
# Debug 构建
cargo build

# Release 构建
cargo build --release

# 运行演示程序
cargo run --release
```

### 项目结构
```
src/
├── lib.rs              # 库入口
├── main.rs             # 演示程序
├── discovery/          # 设备发现模块
│   ├── service.rs     # 服务发现核心 ✅
│   ├── registrar.rs   # 服务注册 ✅
│   ├── browser.rs     # 服务浏览 🚧
│   ├── resolver.rs    # 服务解析 🚧
│   └── types.rs       # 类型定义 ✅
├── mdns/              # mDNS 协议实现
│   ├── packet.rs      # 数据包编解码 ✅
│   ├── socket.rs      # Socket 封装 ✅
│   ├── query.rs       # 查询处理 ✅
│   └── response.rs    # 响应处理 ✅
├── common/            # 公共模块
│   ├── error.rs       # 错误类型 ✅
│   └── config.rs      # 配置管理 ✅
└── transport/         # 传输层（未来）
    └── mod.rs
```

---

## 测试指南

### 在桌面环境测试完整功能

由于 Android/Termux 的权限限制，建议在以下环境测试完整的 mDNS 功能：

1. **Linux**
   ```bash
   cargo run --release
   ```

2. **macOS**
   ```bash
   cargo run --release
   ```

3. **Windows**
   ```powershell
   cargo run --release
   ```

### 测试场景

1. **单机测试**: 两台设备在同一局域网运行程序
2. **多设备测试**: 多台设备同时运行，验证发现功能
3. **长时间运行**: 验证设备过期清理和重广播机制
4. **关闭测试**: 验证服务注销和退出是否正常

---

## 下一步开发计划

### 短期目标 (1-2周)
1. 在桌面环境验证 mDNS 功能
2. 实现 HTTP 服务器基础框架
3. 添加基础文件传输功能

### 中期目标 (1个月)
1. 完成文件传输功能
2. 实现 FFI 层
3. 修复 get_local_ip_addresses 问题

### 长期目标 (3个月)
1. 完成平台集成（Android/iOS/Node.js）
2. 添加安全性功能
3. 性能优化和测试覆盖

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

---

*最后更新时间: 2026-01-15*
