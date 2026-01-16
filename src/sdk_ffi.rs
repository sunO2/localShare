//! SDK FFI (Foreign Function Interface) 层
//!
//! 提供基于新 SDK 的 C 兼容接口供 Flutter/Dart 调用

use std::ffi::{CString, c_char, c_int, c_uint};
use std::ptr;
use std::sync::Mutex;
use crate::sdk_main::{ShareSelfSDK, SDKHandle};
use crate::sdk::*;

// ========== 全局 SDK 实例 ==========

static mut SDK_INSTANCE: Option<*mut ShareSelfSDK> = None;
static mut SDK_LOCK: Option<Mutex<()>> = None;
static mut LOGGER_INIT: bool = false;

/// 初始化全局锁
fn init_global_lock() {
    unsafe {
        if SDK_LOCK.is_none() {
            SDK_LOCK = Some(Mutex::new(()));
        }
    }
}

/// 初始化 Android 日志
#[cfg(target_os = "android")]
fn init_logging() {
    unsafe {
        if !LOGGER_INIT {
            android_logger::init_once(
                android_logger::Config::default()
                    .with_max_level(log::LevelFilter::Debug)
                    .with_tag("ShareSelf"),
            );
            LOGGER_INIT = true;
        }
    }
}

/// 初始化日志 (非 Android)
#[cfg(not(target_os = "android"))]
fn init_logging() {
    // 非平台不需要特殊初始化
    let _ = tracing_subscriber::fmt();
}

// ========== 错误码定义 ==========

/// FFI 错误码
#[repr(C)]
pub enum FfiErrorCode {
    Ok = 0,
    Error = -1,
    NotInitialized = -2,
    AlreadyRunning = -3,
    InvalidArgument = -4,
    NotFound = -5,
    Io = -6,
    Network = -7,
    Discovery = -8,
    Transfer = -9,
    Server = -10,
}

impl From<SDKError> for FfiErrorCode {
    fn from(err: SDKError) -> Self {
        match err {
            SDKError::NotInitialized => FfiErrorCode::NotInitialized,
            SDKError::AlreadyRunning => FfiErrorCode::AlreadyRunning,
            SDKError::InvalidArgument(_) => FfiErrorCode::InvalidArgument,
            SDKError::NotFound(_) => FfiErrorCode::NotFound,
            SDKError::Io(_) => FfiErrorCode::Io,
            SDKError::Network(_) => FfiErrorCode::Network,
            SDKError::Discovery(_) => FfiErrorCode::Discovery,
            SDKError::Transfer(_) => FfiErrorCode::Transfer,
            SDKError::Server(_) => FfiErrorCode::Server,
            _ => FfiErrorCode::Error,
        }
    }
}

/// FFI 结果包装器
#[repr(C)]
pub struct FfiResult {
    pub error_code: c_int,
    pub error_message: *mut c_char,
}

impl FfiResult {
    /// 创建成功结果
    fn success() -> Self {
        Self {
            error_code: FfiErrorCode::Ok as c_int,
            error_message: ptr::null_mut(),
        }
    }

    /// 创建错误结果
    fn error(code: FfiErrorCode, message: &str) -> Self {
        Self {
            error_code: code as c_int,
            error_message: CString::new(message)
                .map(|s| s.into_raw())
                .unwrap_or_else(|_| ptr::null_mut()),
        }
    }

    fn from_sdk_error(err: SDKError) -> Self {
        let code = FfiErrorCode::from(err.clone());
        match err {
            SDKError::InvalidArgument(msg) |
            SDKError::NotFound(msg) |
            SDKError::Network(msg) |
            SDKError::Discovery(msg) |
            SDKError::Transfer(msg) |
            SDKError::Server(msg) => Self::error(code, &msg),
            _ => Self::error(code, &format!("{:?}", err)),
        }
    }
}

// ========== 数据结构 FFI 表示 ==========

/// 设备信息的 FFI 表示
#[repr(C)]
pub struct FfiDeviceInfo {
    pub id: *mut c_char,
    pub name: *mut c_char,
    pub hostname: *mut c_char,
    pub addresses: *mut *mut c_char,  // 字符串数组
    pub addresses_count: c_int,
    pub port: c_uint,
    pub service_type: *mut c_char,
    pub source: c_int,  // DiscoverySource 枚举值
}

/// 文件信息的 FFI 表示
#[repr(C)]
pub struct FfiFileInfo {
    pub id: *mut c_char,
    pub name: *mut c_char,
    pub path: *mut c_char,
    pub size: u64,
    pub mime_type: *mut c_char,
    pub hash: *mut c_char,
}

/// 传输方向的 FFI 枚举
#[repr(C)]
pub enum FfiTransferDirection {
    Upload = 0,
    Download = 1,
}

/// 传输状态的 FFI 枚举
#[repr(C)]
pub enum FfiTransferStatus {
    Pending = 0,
    Preparing = 1,
    Transferring = 2,
    Paused = 3,
    Completed = 4,
    Failed = 5,
    Cancelled = 6,
}

impl From<TransferStatus> for FfiTransferStatus {
    fn from(status: TransferStatus) -> Self {
        match status {
            TransferStatus::Pending => FfiTransferStatus::Pending,
            TransferStatus::Preparing => FfiTransferStatus::Preparing,
            TransferStatus::Transferring => FfiTransferStatus::Transferring,
            TransferStatus::Paused => FfiTransferStatus::Paused,
            TransferStatus::Completed => FfiTransferStatus::Completed,
            TransferStatus::Failed(_) => FfiTransferStatus::Failed,
            TransferStatus::Cancelled => FfiTransferStatus::Cancelled,
        }
    }
}

impl From<DiscoverySource> for c_int {
    fn from(source: DiscoverySource) -> Self {
        match source {
            DiscoverySource::MDNS => 0,
            DiscoverySource::Broadcast => 1,
            DiscoverySource::HTTPScan => 2,
        }
    }
}

/// 传输信息的 FFI 表示
#[repr(C)]
pub struct FfiTransferInfo {
    pub id: *mut c_char,
    pub direction: c_int,
    pub file_name: *mut c_char,
    pub file_size: u64,
    pub transferred: u64,
    pub status: c_int,
    pub remote_device: *mut c_char,
    pub local_path: *mut c_char,
    pub error_message: *mut c_char,
}

/// 事件类型的 FFI 枚举
#[repr(C)]
pub enum FfiEventType {
    None = 0,
    DeviceFound = 1,
    DeviceLost = 2,
    SendStarted = 3,
    SendProgress = 4,
    SendCompleted = 5,
    SendFailed = 6,
    DownloadStarted = 7,
    DownloadProgress = 8,
    DownloadCompleted = 9,
    DownloadFailed = 10,
    ServerStarted = 11,
    ServerStopped = 12,
    Error = 13,
}

/// SDK 事件的 FFI 表示
#[repr(C)]
pub struct FfiEvent {
    pub event_type: c_int,
    pub data: *mut c_char,  // JSON 字符串
}

// ========== 辅助函数 ==========

/// 将 Rust 字符串转换为 C 字符串
fn string_to_c_char(s: String) -> *mut c_char {
    CString::new(s)
        .map(|s| s.into_raw())
        .unwrap_or_else(|_| ptr::null_mut())
}

/// 将 DeviceInfo 转换为 FfiDeviceInfo
fn device_info_to_ffi(device: &DeviceInfo) -> FfiDeviceInfo {
    let addresses: Vec<*mut c_char> = device.addresses.iter()
        .map(|a| string_to_c_char(a.clone()))
        .collect();

    FfiDeviceInfo {
        id: string_to_c_char(device.id.clone()),
        name: string_to_c_char(device.name.clone()),
        hostname: string_to_c_char(device.hostname.clone()),
        addresses: if addresses.is_empty() {
            ptr::null_mut()
        } else {
            Box::into_raw(addresses.into_boxed_slice()) as *mut *mut c_char
        },
        addresses_count: addresses.len() as c_int,
        port: device.port,
        service_type: string_to_c_char(device.service_type.clone()),
        source: device.source.into(),
    }
}

/// 将 FileInfo 转换为 FfiFileInfo
fn file_info_to_ffi(file: &FileInfo) -> FfiFileInfo {
    FfiFileInfo {
        id: string_to_c_char(file.id.clone()),
        name: string_to_c_char(file.name.clone()),
        path: string_to_c_char(file.path.to_string_lossy().to_string()),
        size: file.size,
        mime_type: string_to_c_char(file.mime_type.clone()),
        hash: string_to_c_char(file.hash.clone()),
    }
}

/// 将 TransferInfo 转换为 FfiTransferInfo
fn transfer_info_to_ffi(transfer: &TransferInfo) -> FfiTransferInfo {
    FfiTransferInfo {
        id: string_to_c_char(transfer.id.clone()),
        direction: match transfer.direction {
            TransferDirection::Upload => 0,
            TransferDirection::Download => 1,
        },
        file_name: string_to_c_char(transfer.file_name.clone()),
        file_size: transfer.file_size,
        transferred: transfer.transferred,
        status: match transfer.status {
            TransferStatus::Pending => 0,
            TransferStatus::Preparing => 1,
            TransferStatus::Transferring => 2,
            TransferStatus::Paused => 3,
            TransferStatus::Completed => 4,
            TransferStatus::Failed(_) => 5,
            TransferStatus::Cancelled => 6,
        },
        remote_device: string_to_c_char(transfer.remote_device.clone()),
        local_path: string_to_c_char(transfer.local_path.to_string_lossy().to_string()),
        error_message: transfer.error_message.as_ref()
            .map(|s| string_to_c_char(s.clone()))
            .unwrap_or_else(|| ptr::null_mut()),
    }
}

/// 将 SDKEvent 转换为 FfiEvent
fn event_to_ffi(event: SDKEvent) -> FfiEvent {
    let (event_type, data) = match event {
        SDKEvent::DeviceFound(device) => {
            (FfiEventType::DeviceFound as c_int,
             serde_json::to_string(&device).ok())
        }
        SDKEvent::DeviceLost(id) => {
            (FfiEventType::DeviceLost as c_int,
             serde_json::to_string(&id).ok())
        }
        SDKEvent::SendStarted(id) => {
            (FfiEventType::SendStarted as c_int,
             serde_json::to_string(&id).ok())
        }
        SDKEvent::SendProgress(id, transferred, total) => {
            (FfiEventType::SendProgress as c_int,
             serde_json::to_string(&(id, transferred, total)).ok())
        }
        SDKEvent::SendCompleted(id) => {
            (FfiEventType::SendCompleted as c_int,
             serde_json::to_string(&id).ok())
        }
        SDKEvent::SendFailed(id, error) => {
            (FfiEventType::SendFailed as c_int,
             serde_json::to_string(&(id, error)).ok())
        }
        SDKEvent::DownloadStarted(id) => {
            (FfiEventType::DownloadStarted as c_int,
             serde_json::to_string(&id).ok())
        }
        SDKEvent::DownloadProgress(id, transferred, total) => {
            (FfiEventType::DownloadProgress as c_int,
             serde_json::to_string(&(id, transferred, total)).ok())
        }
        SDKEvent::DownloadCompleted(id, path) => {
            (FfiEventType::DownloadCompleted as c_int,
             serde_json::to_string(&(id, path)).ok())
        }
        SDKEvent::DownloadFailed(id, error) => {
            (FfiEventType::DownloadFailed as c_int,
             serde_json::to_string(&(id, error)).ok())
        }
        SDKEvent::ServerStarted(port) => {
            (FfiEventType::ServerStarted as c_int,
             serde_json::to_string(&port).ok())
        }
        SDKEvent::ServerStopped => {
            (FfiEventType::ServerStopped as c_int,
             Some("{}".to_string()))
        }
        SDKEvent::Error(msg) => {
            (FfiEventType::Error as c_int,
             serde_json::to_string(&msg).ok())
        }
    };

    FfiEvent {
        event_type,
        data: data.map(|s| string_to_c_char(s)).unwrap_or_else(|| ptr::null_mut()),
    }
}

/// 获取 SDK 实例
unsafe fn get_sdk() -> Option<&'static mut ShareSelfSDK> {
    SDK_INSTANCE.as_mut().map(|p| &mut **p)
}

// ========== FFI API 实现 ==========

/// 初始化 SDK
#[no_mangle]
pub unsafe extern "C" fn shareself_init() -> *mut FfiResult {
    init_global_lock();
    init_logging();

    let result = std::panic::catch_unwind(|| {
        let _lock = SDK_LOCK.as_ref().unwrap().lock().unwrap();

        if SDK_INSTANCE.is_some() {
            return Err(FfiResult::error(FfiErrorCode::AlreadyRunning, "SDK already initialized"));
        }

        let sdk = ShareSelfSDK::new();
        sdk.initialize()
            .map_err(|e| FfiResult::from_sdk_error(e))?;

        SDK_INSTANCE = Some(Box::into_raw(Box::new(sdk)));
        Ok(FfiResult::success())
    }).unwrap_or_else(|_| Err(FfiResult::error(FfiErrorCode::Error, "Panic during init")));

    Box::into_raw(Box::new(match result {
        Ok(r) => r,
        Err(e) => e,
    }))
}

/// 清理 SDK 资源
#[no_mangle]
pub unsafe extern "C" fn shareself_cleanup() -> *mut FfiResult {
    let result = std::panic::catch_unwind(|| {
        let _lock = SDK_LOCK.as_ref().unwrap().lock().unwrap();

        if let Some(sdk_ptr) = SDK_INSTANCE.take() {
            // 注意：这里需要异步 cleanup，但在 FFI 中很难处理
            // 实际应用中应该使用 runtime block_on
            let _ = Box::from_raw(sdk_ptr);
        }

        Ok(FfiResult::success())
    }).unwrap_or_else(|_| Err(FfiResult::error(FfiErrorCode::Error, "Panic during cleanup")));

    Box::into_raw(Box::new(match result {
        Ok(r) => r,
        Err(e) => e,
    }))
}

/// 获取版本信息
#[no_mangle]
pub extern "C" fn shareself_get_version() -> *mut c_char {
    string_to_c_char(crate::VERSION.to_string())
}

// ========== 设备发现 ==========

/// 启动设备发现
#[no_mangle]
pub unsafe extern "C" fn shareself_start_discovery(port: c_uint) -> *mut FfiResult {
    let result = std::panic::catch_unwind(|| {
        let sdk = get_sdk().ok_or(FfiResult::error(FfiErrorCode::NotInitialized, "SDK not initialized"))?;

        // 在 runtime 中执行异步操作
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| FfiResult::error(FfiErrorCode::Error, &format!("Failed to create runtime: {}", e)))?;

        rt.block_on(async {
            sdk.start_discovery(port as u16).await
        }).map_err(|e| FfiResult::from_sdk_error(e))?;

        Ok(FfiResult::success())
    }).unwrap_or_else(|_| Err(FfiResult::error(FfiErrorCode::Error, "Panic during start_discovery")));

    Box::into_raw(Box::new(match result {
        Ok(r) => r,
        Err(e) => e,
    }))
}

/// 停止设备发现
#[no_mangle]
pub unsafe extern "C" fn shareself_stop_discovery() -> *mut FfiResult {
    let result = std::panic::catch_unwind(|| {
        let sdk = get_sdk().ok_or(FfiResult::error(FfiErrorCode::NotInitialized, "SDK not initialized"))?;

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| FfiResult::error(FfiErrorCode::Error, &format!("Failed to create runtime: {}", e)))?;

        rt.block_on(async {
            sdk.stop_discovery().await
        }).map_err(|e| FfiResult::from_sdk_error(e))?;

        Ok(FfiResult::success())
    }).unwrap_or_else(|_| Err(FfiResult::error(FfiErrorCode::Error, "Panic during stop_discovery")));

    Box::into_raw(Box::new(match result {
        Ok(r) => r,
        Err(e) => e,
    }))
}

/// 获取设备列表
#[no_mangle]
pub unsafe extern "C" fn shareself_get_devices(count_out: *mut c_int) -> *mut FfiDeviceInfo {
    if count_out.is_null() {
        return ptr::null_mut();
    }

    let sdk = match get_sdk() {
        Some(s) => s,
        None => {
            *count_out = 0;
            return ptr::null_mut();
        }
    };

    let devices = sdk.get_devices();
    *count_out = devices.len() as c_int;

    if devices.is_empty() {
        return ptr::null_mut();
    }

    let boxed_array: Box<[FfiDeviceInfo]> = devices.iter()
        .map(device_info_to_ffi)
        .collect::<Vec<_>>()
        .into_boxed_slice();

    Box::into_raw(boxed_array) as *mut FfiDeviceInfo
}

// ========== 文件服务器 ==========

/// 启动文件服务器
#[no_mangle]
pub unsafe extern "C" fn shareself_start_server(port: c_uint) -> *mut FfiResult {
    let result = std::panic::catch_unwind(|| {
        let sdk = get_sdk().ok_or(FfiResult::error(FfiErrorCode::NotInitialized, "SDK not initialized"))?;

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| FfiResult::error(FfiErrorCode::Error, &format!("Failed to create runtime: {}", e)))?;

        rt.block_on(async {
            sdk.start_server(port as u16).await
        }).map_err(|e| FfiResult::from_sdk_error(e))?;

        Ok(FfiResult::success())
    }).unwrap_or_else(|_| Err(FfiResult::error(FfiErrorCode::Error, "Panic during start_server")));

    Box::into_raw(Box::new(match result {
        Ok(r) => r,
        Err(e) => e,
    }))
}

/// 停止文件服务器
#[no_mangle]
pub unsafe extern "C" fn shareself_stop_server() -> *mut FfiResult {
    let result = std::panic::catch_unwind(|| {
        let sdk = get_sdk().ok_or(FfiResult::error(FfiErrorCode::NotInitialized, "SDK not initialized"))?;

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| FfiResult::error(FfiErrorCode::Error, &format!("Failed to create runtime: {}", e)))?;

        rt.block_on(async {
            sdk.stop_server().await
        }).map_err(|e| FfiResult::from_sdk_error(e))?;

        Ok(FfiResult::success())
    }).unwrap_or_else(|_| Err(FfiResult::error(FfiErrorCode::Error, "Panic during stop_server")));

    Box::into_raw(Box::new(match result {
        Ok(r) => r,
        Err(e) => e,
    }))
}

/// 添加共享文件
#[no_mangle]
pub unsafe extern "C" fn shareself_add_shared_file(path: *const c_char) -> *mut FfiResult {
    let result = std::panic::catch_unwind(|| {
        let sdk = get_sdk().ok_or(FfiResult::error(FfiErrorCode::NotInitialized, "SDK not initialized"))?;

        if path.is_null() {
            return Err(FfiResult::error(FfiErrorCode::InvalidArgument, "Null path"));
        }

        let path_str = std::ffi::CStr::from_ptr(path)
            .to_str()
            .map_err(|_| FfiResult::error(FfiErrorCode::InvalidArgument, "Invalid UTF-8"))?;

        sdk.add_shared_file(path_str)
            .map_err(|e| FfiResult::from_sdk_error(e))?;

        Ok(FfiResult::success())
    }).unwrap_or_else(|_| Err(FfiResult::error(FfiErrorCode::Error, "Panic during add_shared_file")));

    Box::into_raw(Box::new(match result {
        Ok(r) => r,
        Err(e) => e,
    }))
}

/// 获取共享文件列表
#[no_mangle]
pub unsafe extern "C" fn shareself_get_shared_files(count_out: *mut c_int) -> *mut FfiFileInfo {
    if count_out.is_null() {
        return ptr::null_mut();
    }

    let sdk = match get_sdk() {
        Some(s) => s,
        None => {
            *count_out = 0;
            return ptr::null_mut();
        }
    };

    let files = sdk.get_shared_files();
    *count_out = files.len() as c_int;

    if files.is_empty() {
        return ptr::null_mut();
    }

    let boxed_array: Box<[FfiFileInfo]> = files.iter()
        .map(file_info_to_ffi)
        .collect::<Vec<_>>()
        .into_boxed_slice();

    Box::into_raw(boxed_array) as *mut FfiFileInfo
}

/// 清空共享文件列表
#[no_mangle]
pub unsafe extern "C" fn shareself_clear_shared_files() -> *mut FfiResult {
    let result = std::panic::catch_unwind(|| {
        let sdk = get_sdk().ok_or(FfiResult::error(FfiErrorCode::NotInitialized, "SDK not initialized"))?;
        sdk.clear_shared_files();
        Ok(FfiResult::success())
    }).unwrap_or_else(|_| Err(FfiResult::error(FfiErrorCode::Error, "Panic during clear_shared_files")));

    Box::into_raw(Box::new(match result {
        Ok(r) => r,
        Err(e) => e,
    }))
}

// ========== 传输管理 ==========

/// 发送文件
#[no_mangle]
pub unsafe extern "C" fn shareself_send_file(
    device_id: *const c_char,
    file_path: *const c_char,
    transfer_id_out: *mut *mut c_char,
) -> *mut FfiResult {
    let result = std::panic::catch_unwind(|| {
        let sdk = get_sdk().ok_or(FfiResult::error(FfiErrorCode::NotInitialized, "SDK not initialized"))?;

        if device_id.is_null() || file_path.is_null() {
            return Err(FfiResult::error(FfiErrorCode::InvalidArgument, "Null argument"));
        }

        let device_id_str = std::ffi::CStr::from_ptr(device_id)
            .to_str()
            .map_err(|_| FfiResult::error(FfiErrorCode::InvalidArgument, "Invalid UTF-8"))?;

        let file_path_str = std::ffi::CStr::from_ptr(file_path)
            .to_str()
            .map_err(|_| FfiResult::error(FfiErrorCode::InvalidArgument, "Invalid UTF-8"))?;

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| FfiResult::error(FfiErrorCode::Error, &format!("Failed to create runtime: {}", e)))?;

        let transfer_id = rt.block_on(async {
            sdk.send_file(device_id_str, file_path_str).await
        }).map_err(|e| FfiResult::from_sdk_error(e))?;

        if !transfer_id_out.is_null() {
            *transfer_id_out = string_to_c_char(transfer_id);
        }

        Ok(FfiResult::success())
    }).unwrap_or_else(|_| Err(FfiResult::error(FfiErrorCode::Error, "Panic during send_file")));

    Box::into_raw(Box::new(match result {
        Ok(r) => r,
        Err(e) => e,
    }))
}

/// 下载文件
#[no_mangle]
pub unsafe extern "C" fn shareself_download_file(
    device_id: *const c_char,
    file_id: *const c_char,
    save_path: *const c_char,
    transfer_id_out: *mut *mut c_char,
) -> *mut FfiResult {
    let result = std::panic::catch_unwind(|| {
        let sdk = get_sdk().ok_or(FfiResult::error(FfiErrorCode::NotInitialized, "SDK not initialized"))?;

        if device_id.is_null() || file_id.is_null() || save_path.is_null() {
            return Err(FfiResult::error(FfiErrorCode::InvalidArgument, "Null argument"));
        }

        let device_id_str = std::ffi::CStr::from_ptr(device_id)
            .to_str()
            .map_err(|_| FfiResult::error(FfiErrorCode::InvalidArgument, "Invalid UTF-8"))?;

        let file_id_str = std::ffi::CStr::from_ptr(file_id)
            .to_str()
            .map_err(|_| FfiResult::error(FfiErrorCode::InvalidArgument, "Invalid UTF-8"))?;

        let save_path_str = std::ffi::CStr::from_ptr(save_path)
            .to_str()
            .map_err(|_| FfiResult::error(FfiErrorCode::InvalidArgument, "Invalid UTF-8"))?;

        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| FfiResult::error(FfiErrorCode::Error, &format!("Failed to create runtime: {}", e)))?;

        let transfer_id = rt.block_on(async {
            sdk.download_file(device_id_str, file_id_str, save_path_str).await
        }).map_err(|e| FfiResult::from_sdk_error(e))?;

        if !transfer_id_out.is_null() {
            *transfer_id_out = string_to_c_char(transfer_id);
        }

        Ok(FfiResult::success())
    }).unwrap_or_else(|_| Err(FfiResult::error(FfiErrorCode::Error, "Panic during download_file")));

    Box::into_raw(Box::new(match result {
        Ok(r) => r,
        Err(e) => e,
    }))
}

/// 取消传输
#[no_mangle]
pub unsafe extern "C" fn shareself_cancel_transfer(transfer_id: *const c_char) -> *mut FfiResult {
    let result = std::panic::catch_unwind(|| {
        let sdk = get_sdk().ok_or(FfiResult::error(FfiErrorCode::NotInitialized, "SDK not initialized"))?;

        if transfer_id.is_null() {
            return Err(FfiResult::error(FfiErrorCode::InvalidArgument, "Null transfer_id"));
        }

        let transfer_id_str = std::ffi::CStr::from_ptr(transfer_id)
            .to_str()
            .map_err(|_| FfiResult::error(FfiErrorCode::InvalidArgument, "Invalid UTF-8"))?;

        sdk.cancel_transfer(transfer_id_str)
            .map_err(|e| FfiResult::from_sdk_error(e))?;

        Ok(FfiResult::success())
    }).unwrap_or_else(|_| Err(FfiResult::error(FfiErrorCode::Error, "Panic during cancel_transfer")));

    Box::into_raw(Box::new(match result {
        Ok(r) => r,
        Err(e) => e,
    }))
}

/// 获取所有传输
#[no_mangle]
pub unsafe extern "C" fn shareself_list_transfers(count_out: *mut c_int) -> *mut FfiTransferInfo {
    if count_out.is_null() {
        return ptr::null_mut();
    }

    let sdk = match get_sdk() {
        Some(s) => s,
        None => {
            *count_out = 0;
            return ptr::null_mut();
        }
    };

    let transfers = sdk.get_all_transfers();
    *count_out = transfers.len() as c_int;

    if transfers.is_empty() {
        return ptr::null_mut();
    }

    let boxed_array: Box<[FfiTransferInfo]> = transfers.iter()
        .map(transfer_info_to_ffi)
        .collect::<Vec<_>>()
        .into_boxed_slice();

    Box::into_raw(boxed_array) as *mut FfiTransferInfo
}

// ========== 事件系统 ==========

/// 轮询事件（非阻塞）
#[no_mangle]
pub unsafe extern "C" fn shareself_poll_event() -> *mut FfiEvent {
    let sdk = match get_sdk() {
        Some(s) => s,
        None => return ptr::null_mut(),
    };

    match sdk.poll_event() {
        Some(event) => Box::into_raw(Box::new(event_to_ffi(event))),
        None => ptr::null_mut(),
    }
}

// ========== 内存释放函数 ==========

/// 释放字符串内存
#[no_mangle]
pub unsafe extern "C" fn shareself_free_string(s: *mut c_char) {
    if !s.is_null() {
        let _ = CString::from_raw(s);
    }
}

/// 释放 FfiResult 内存
#[no_mangle]
pub unsafe extern "C" fn shareself_free_result(result: *mut FfiResult) {
    if !result.is_null() {
        let r = Box::from_raw(result);
        if !r.error_message.is_null() {
            let _ = CString::from_raw(r.error_message);
        }
    }
}

/// 释放设备列表内存
#[no_mangle]
pub unsafe extern "C" fn shareself_free_device_list(devices: *mut FfiDeviceInfo, count: c_int) {
    if !devices.is_null() && count > 0 {
        for i in 0..count as isize {
            let device = &*devices.offset(i);
            shareself_free_string(device.id);
            shareself_free_string(device.name);
            shareself_free_string(device.hostname);
            // 释放地址数组
            if !device.addresses.is_null() && device.addresses_count > 0 {
                for j in 0..device.addresses_count as isize {
                    shareself_free_string(*device.addresses.offset(j));
                }
                let _ = Box::from_raw(device.addresses);
            }
            shareself_free_string(device.service_type);
        }
        let _ = Box::from_raw(devices);
    }
}

/// 释放文件列表内存
#[no_mangle]
pub unsafe extern "C" fn shareself_free_file_list(files: *mut FfiFileInfo, count: c_int) {
    if !files.is_null() && count > 0 {
        for i in 0..count as isize {
            let file = &*files.offset(i);
            shareself_free_string(file.id);
            shareself_free_string(file.name);
            shareself_free_string(file.path);
            shareself_free_string(file.mime_type);
            shareself_free_string(file.hash);
        }
        let _ = Box::from_raw(files);
    }
}

/// 释放传输列表内存
#[no_mangle]
pub unsafe extern "C" fn shareself_free_transfer_list(transfers: *mut FfiTransferInfo, count: c_int) {
    if !transfers.is_null() && count > 0 {
        for i in 0..count as isize {
            let transfer = &*transfers.offset(i);
            shareself_free_string(transfer.id);
            shareself_free_string(transfer.file_name);
            shareself_free_string(transfer.remote_device);
            shareself_free_string(transfer.local_path);
            if !transfer.error_message.is_null() {
                shareself_free_string(transfer.error_message);
            }
        }
        let _ = Box::from_raw(transfers);
    }
}

/// 释放事件内存
#[no_mangle]
pub unsafe extern "C" fn shareself_free_event(event: *mut FfiEvent) {
    if !event.is_null() {
        let e = Box::from_raw(event);
        if !e.data.is_null() {
            shareself_free_string(e.data);
        }
    }
}
