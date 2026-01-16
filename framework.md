# ShareSelf SDK 设计文档

> **版本**: v1.0
> **更新时间**: 2025-01-16
> **状态**: 设计完成，开始实施

---

## 1. 概述

ShareSelf SDK 是一个跨平台的本地网络文件共享库，通过 FFI 提供给 Flutter/Dart/移动端调用。

### 1.1 核心功能

- **设备发现**: mDNS + UDP 广播 + HTTP 扫描
- **文件服务器**: 提供文件上传/下载服务
- **文件发送**: 向远程设备发送文件
- **文件接收/下载**: 从远程设备下载文件

### 1.2 设计原则

| 原则 | 说明 | 实现方式 |
|------|------|----------|
| **同步 FFI** | 所有 FFI 调用立即返回，不阻塞 | 返回 handle/id，异步执行 |
| **状态可查** | 任何操作都能查询当前状态 | `get_status()` 系列方法 |
| **事件推送** | Rust → Dart 异步事件通知 | 使用 Port 机制或轮询 |
| **可取消** | 所有长时间操作可取消 | `cancel_*()` 方法 |

---

## 2. 架构设计

### 2.1 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                     Dart/Flutter Client                       │
│                                                              │
│  ┌─────────┐    ┌──────────┐    ┌─────────────────────┐    │
│  │ UI 操作  │───→│ FFI Call │───→│   Rust SDK          │    │
│  └─────────┘    └──────────┘    │                      │    │
│       ↑              │              │ - 异步任务执行       │    │
│       │              │              │ - 状态变更           │    │
│       │              │              │ - 事件产生           │    │
│       │              │              │                      │    │
│  ┌─────────┐    ┌──────────┐    │        ┌───────────┐ │    │
│  │事件流    │←───│ Port/FFI │←───│ Event Loop │ │    │
│  │Stream   │    └──────────┘    └─────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

### 2.2 模块划分

```
┌─────────────────────────────────────────────────────┐
│                   ShareSelf SDK                     │
│  ┌──────────────┬──────────────┬───────────────┐   │
│  │  Discovery   │   Transfer    │    Server     │   │
│  │  发现设备     │   文件传输     │   文件服务器    │   │
│  │              │  ┌────────┐   │               │   │
│  │              │  │  Send  │   │               │   │
│  │              │  │  Recv  │   │               │   │
│  │              │  └────────┘   │               │   │
│  └──────────────┴──────────────┴───────────────┘   │
└─────────────────────────────────────────────────────┘
```

---

## 3. FFI 接口定义

### 3.1 初始化与清理

```c
// 初始化 SDK
shareself_init() → Result

// 清理资源
shareself_cleanup() → Result

// 获取版本
shareself_get_version() → char*

// 获取/设置设备名称
shareself_get_device_name() → char*
shareself_set_device_name(name) → Result
shareself_get_device_id() → char*
```

### 3.2 设备发现

```c
// 启动发现
shareself_start_discovery(port) → Result

// 停止发现
shareself_stop_discovery() → Result

// 查询状态
shareself_discovery_is_running() → bool

// 获取设备列表
shareself_get_devices(count) → DeviceInfo[]
```

### 3.3 文件服务器

```c
// 启动服务器
shareself_start_server(port) → Result

// 停止服务器
shareself_stop_server() → Result

// 查询状态
shareself_server_is_running() → bool
shareself_get_server_port() → int32

// 共享文件管理
shareself_add_shared_file(path) → Result
shareself_remove_shared_file(path) → Result
shareself_get_shared_files(count) → FileInfo[]
shareself_clear_shared_files() → Result
```

### 3.4 文件发送

```c
// 发起发送
shareself_send_file(device_id, file_path) → Result {
    char* transfer_id;
}

// 查询状态
shareself_get_send_status(transfer_id) → TransferStatus
shareself_get_send_progress(transfer_id) → Progress

// 控制
shareself_pause_send(transfer_id) → Result
shareself_resume_send(transfer_id) → Result
shareself_cancel_send(transfer_id) → Result
```

### 3.5 文件接收/下载

```c
// 发起下载
shareself_download_file(device_id, file_id, save_path) → Result {
    char* transfer_id;
}

// 查询状态
shareself_get_download_status(transfer_id) → TransferStatus
shareself_get_download_progress(transfer_id) → Progress

// 控制
shareself_pause_download(transfer_id) → Result
shareself_resume_download(transfer_id) → Result
shareself_cancel_download(transfer_id) → Result
```

### 3.6 传输管理

```c
// 获取所有传输
shareself_list_transfers(count) → TransferInfo[]

// 按类型获取
shareself_list_uploads(count) → TransferInfo[]
shareself_list_downloads(count) → TransferInfo[]

// 获取单个传输详情
shareself_get_transfer(transfer_id) → TransferInfo
```

### 3.7 事件系统

```c
// 轮询事件（非阻塞）
shareself_poll_event() → SDKEvent*

// 释放事件
shareself_free_event(event)
```

---

## 4. 数据结构定义

### 4.1 设备信息

```c
typedef struct {
    char* id;
    char* name;
    char* hostname;
    char* address;
    uint32_t port;
    char* service_type;
    char* discovery_source;
} DeviceInfo;
```

### 4.2 文件信息

```c
typedef struct {
    char* id;
    char* name;
    char* path;
    uint64_t size;
    char* mime_type;
    char* hash;
} FileInfo;
```

### 4.3 传输状态

```c
typedef enum {
    STATUS_PENDING = 0,
    STATUS_PREPARING = 1,
    STATUS_TRANSFERRING = 2,
    STATUS_PAUSED = 3,
    STATUS_COMPLETED = 4,
    STATUS_FAILED = 5,
    STATUS_CANCELLED = 6,
} TransferStatus;

typedef enum {
    DIR_UPLOAD = 0,
    DIR_DOWNLOAD = 1,
} TransferDirection;
```

### 4.4 传输信息

```c
typedef struct {
    char* transfer_id;
    int direction;
    char* file_name;
    uint64_t file_size;
    uint64_t transferred;
    int status;
    char* remote_device;
    char* local_path;
    char* error_message;
} TransferInfo;
```

### 4.5 进度信息

```c
typedef struct {
    uint64_t transferred;
    uint64_t total;
    uint8_t percentage;
    uint64_t speed;
    uint64_t remaining;
} Progress;
```

### 4.6 事件类型

```c
typedef enum {
    EVENT_NONE = 0,
    EVENT_DEVICE_FOUND,
    EVENT_DEVICE_LOST,
    EVENT_SEND_STARTED,
    EVENT_SEND_PROGRESS,
    EVENT_SEND_COMPLETED,
    EVENT_SEND_FAILED,
    EVENT_DOWNLOAD_STARTED,
    EVENT_DOWNLOAD_PROGRESS,
    EVENT_DOWNLOAD_COMPLETED,
    EVENT_DOWNLOAD_FAILED,
    EVENT_SERVER_STARTED,
    EVENT_SERVER_STOPPED,
    EVENT_ERROR,
} SDKEventType;

typedef struct {
    int type;
    char* data;
    int data_len;
} SDKEvent;
```

### 4.7 错误码

```c
typedef enum {
    SHARESELF_OK = 0,
    SHARESELF_ERROR = -1,
    SHARESELF_ERROR_INIT = -2,
    SHARESELF_ERROR_NOT_INIT = -3,
    SHARESELF_ERROR_ALREADY_RUNNING = -4,
    SHARESELF_ERROR_INVALID_ARG = -5,
    SHARESELF_ERROR_NOT_FOUND = -6,
    SHARESELF_ERROR_IO = -7,
    SHARESELF_ERROR_NETWORK = -8,
} ShareSelfErrorCode;
```

---

## 5. Rust SDK 内部架构

```rust
pub struct ShareSelfSDK {
    // Runtime
    runtime: tokio::runtime::Runtime,

    // 模块
    discovery: DiscoveryModule,
    server: FileServerModule,
    transfer: TransferModule,

    // 状态管理
    state: Arc<RwLock<SDKState>>,

    // 事件系统
    event_tx: mpsc::UnboundedSender<SDKEvent>,
}

pub struct SDKState {
    pub discovery_running: bool,
    pub server_running: bool,
    pub devices: Vec<DeviceInfo>,
    pub shared_files: Vec<FileInfo>,
    pub transfers: HashMap<String, TransferState>,
}
```

---

## 6. 实施进度

### 已完成 ✅

- [x] 设计文档编写
- [x] SDK 核心类型定义 (sdk/types.rs)
- [x] 状态管理模块 (sdk/state.rs)
- [x] 事件系统 (sdk/events.rs)
- [x] 设备发现模块封装 (sdk/discovery.rs)
- [x] 文件服务器模块 (sdk/server.rs)
- [x] 传输管理模块 (sdk/transfer.rs)
- [x] SDK 主模块 (sdk_main.rs)
- [x] SDK 模块导出 (sdk/mod.rs)
- [x] SDK FFI 接口层 (sdk_ffi.rs)

### 进行中 🔄

- [ ] 正在编译测试 SDK...

### 待实施 ⏳

- [ ] Dart 绑定更新
- [ ] 测试与验证

---

## 7. 文件结构

```
src/
├── sdk.rs              # SDK 主模块
├── sdk/
│   ├── mod.rs          # 模块入口
│   ├── types.rs        # 类型定义
│   ├── state.rs        # 状态管理
│   ├── events.rs       # 事件系统
│   ├── discovery.rs    # 设备发现封装
│   ├── server.rs       # 文件服务器封装
│   └── transfer.rs     # 传输管理封装
├── ffi.rs              # FFI 接口层
└── discovery/          # 原有发现模块
```

---

*最后更新: 实施前准备*
