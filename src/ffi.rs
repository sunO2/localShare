//! FFI (Foreign Function Interface) 层
//!
//! 提供 C 兼容的接口供 Flutter/Dart 调用

use std::ffi::{CString, c_char, c_int, c_uint};
use std::ptr;
use std::sync::{Arc, Mutex};

use crate::discovery::manager::{ManagerConfig, ManagedEvent, ManagedDevice, DiscoverySource};

impl Drop for FfiDiscoveryHandle {
    fn drop(&mut self) {
        if !self._runtime_handle.is_null() {
            unsafe {
                let _ = Box::from_raw(self._runtime_handle);
            }
        }
    }
}

/// FFI 错误码
#[repr(C)]
pub enum FfiErrorCode {
    Success = 0,
    UnknownError = -1,
    InvalidArgument = -2,
    NullPointer = -3,
    Utf8Error = -4,
    DiscoveryError = -5,
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
            error_code: FfiErrorCode::Success as c_int,
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
}

/// 设备信息的 FFI 表示
#[repr(C)]
pub struct FfiDeviceInfo {
    pub id: *mut c_char,
    pub name: *mut c_char,
    pub hostname: *mut c_char,
    pub address: *mut c_char,
    pub port: c_uint,
    pub service_type: *mut c_char,
    pub discovery_source: *mut c_char, // 新增：发现来源
}

/// FFI 发现句柄
#[repr(C)]
pub struct FfiDiscoveryHandle {
    _runtime_handle: *mut tokio::runtime::Runtime,
}

/// 全局发现实例
static mut DISCOVERY_INSTANCE: Option<FfiDiscoveryHandle> = None;
static mut DISCOVERY_LOCK: Option<std::sync::Mutex<()>> = None;
static mut DEVICES: Option<Arc<Mutex<Vec<ManagedDevice>>>> = None;
static mut LOGGER_INIT: bool = false;

/// 初始化全局锁
fn init_global_lock() {
    unsafe {
        if DISCOVERY_LOCK.is_none() {
            DISCOVERY_LOCK = Some(std::sync::Mutex::new(()));
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

/// 释放字符串内存
///
/// # Safety
/// 必须传入由 FFI 函数返回的有效指针
#[no_mangle]
pub unsafe extern "C" fn shareself_free_string(s: *mut c_char) {
    if !s.is_null() {
        let _ = CString::from_raw(s);
    }
}

/// 释放 FfiResult 内存
///
/// # Safety
/// 必须传入由 FFI 函数返回的有效指针
#[no_mangle]
pub unsafe extern "C" fn shareself_free_result(result: *mut FfiResult) {
    if !result.is_null() {
        let r = Box::from_raw(result);
        if !r.error_message.is_null() {
            let _ = CString::from_raw(r.error_message);
        }
    }
}

/// 释放设备信息数组内存
///
/// # Safety
/// 必须传入由 FFI 函数返回的有效指针
#[no_mangle]
pub unsafe extern "C" fn shareself_free_device_list(
    devices: *mut FfiDeviceInfo,
    count: c_int,
) {
    if !devices.is_null() && count > 0 {
        let devices_ptr = devices;
        for i in 0..count as isize {
            let device = &*devices_ptr.offset(i);
            shareself_free_string(device.id);
            shareself_free_string(device.name);
            shareself_free_string(device.hostname);
            shareself_free_string(device.address);
            shareself_free_string(device.service_type);
            shareself_free_string(device.discovery_source);
        }
        // 释放数组本身
        let _ = Box::from_raw(devices_ptr);
    }
}

/// 启动设备发现
///
/// # Safety
/// `port` 参数必须为有效端口范围 (1-65535)
/// 返回的指针必须使用 `shareself_free_result` 释放
#[no_mangle]
pub unsafe extern "C" fn shareself_start_discovery(
    port: c_uint,
    result_out: *mut *mut FfiResult,
) -> *mut FfiDiscoveryHandle {
    init_global_lock();
    init_logging();

    let result: std::result::Result<Box<tokio::runtime::Runtime>, String> = std::panic::catch_unwind(|| {
        if port > 65535 {
            return Err("Invalid port".to_string());
        }

        // 检查是否已有运行中的实例
        let _lock = DISCOVERY_LOCK.as_ref().unwrap().lock().unwrap();
        if DISCOVERY_INSTANCE.is_some() {
            if let Some(devices) = &DEVICES {
                // 检查是否还在运行
                let devs = devices.lock().unwrap();
                if !devs.is_empty() {
                    return Err("Discovery already running".to_string());
                }
            }
        }

        // 创建 tokio runtime
        let rt = Box::new(tokio::runtime::Runtime::new()
            .map_err(|e| format!("Failed to create runtime: {}", e))?);

        // 创建设备存储
        let devices = Arc::new(Mutex::new(Vec::new()));
        DEVICES = Some(devices.clone());

        // 创建混合发现配置
        let broadcast_config = Some(crate::discovery::broadcast::BroadcastConfig {
            bind_port: 0,
            broadcast_port: 53317,
            interval_secs: 5,
            device_name: gethostname::gethostname()
                .to_string_lossy()
                .to_string(),
            service_port: port as u16,
        });

        let config = ManagerConfig {
            enable_mdns: true,           // 路由器 WiFi 场景
            enable_broadcast: true,      // 移动热点场景
            enable_scan: false,          // HTTP 扫描默认关闭（比较耗时）
            mdns_service_type: "_shareself._tcp.local".to_string(),
            mdns_domain: "local".to_string(),
            broadcast_config,
            scan_config: None,
        };

        // 在 runtime 中启动发现管理器
        let rt_clone = rt.as_ref();
        let devices_clone = devices.clone();
        rt_clone.block_on(async move {
            // 启动统一发现管理器
            let (mut event_rx, _manager) = match crate::discovery::manager::start_discovery_manager(config) {
                Ok((rx, m)) => (rx, m),
                Err(e) => {
                    tracing::error!("Failed to start manager: {}", e);
                    return;
                }
            };

            // 启动事件处理任务
            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    handle_managed_event(event, &devices_clone);
                }
            });
        });

        Ok(rt)
    }).unwrap_or(Err("Panic during discovery start".to_string()));

    let ffi_result = match result {
        Ok(rt) => {
            let handle = FfiDiscoveryHandle {
                _runtime_handle: Box::into_raw(rt),
            };
            let _lock = DISCOVERY_LOCK.as_ref().unwrap().lock().unwrap();
            DISCOVERY_INSTANCE = Some(handle);
            FfiResult::success()
        }
        Err(e) => {
            let msg = format!("Failed to start discovery: {}", e);
            FfiResult::error(FfiErrorCode::DiscoveryError, &msg)
        }
    };

    if !result_out.is_null() {
        *result_out = Box::into_raw(Box::new(ffi_result));
    }

    DISCOVERY_INSTANCE.as_ref().map(|h| h as *const _ as *mut _).unwrap_or(ptr::null_mut())
}

/// 处理统一发现管理器事件
fn handle_managed_event(event: ManagedEvent, devices: &Arc<Mutex<Vec<ManagedDevice>>>) {
    match event {
        ManagedEvent::DeviceFound(device) => {
            tracing::info!("Device found: {} via {:?}", device.name, device.source);
            let mut devs = devices.lock().unwrap();
            // 检查是否已存在
            if !devs.iter().any(|d| d.id == device.id) {
                devs.push(device);
            }
        }
        ManagedEvent::DeviceLost(id) => {
            tracing::info!("Device lost: {}", id);
            let mut devs = devices.lock().unwrap();
            devs.retain(|d| d.id != id);
        }
        ManagedEvent::DeviceUpdated(device) => {
            tracing::debug!("Device updated: {}", device.name);
            let mut devs = devices.lock().unwrap();
            if let Some(d) = devs.iter_mut().find(|d| d.id == device.id) {
                *d = device;
            }
        }
        ManagedEvent::ScanProgress { current, total } => {
            tracing::debug!("Scan progress: {}/{}", current, total);
        }
        ManagedEvent::Error(e) => {
            tracing::error!("Discovery error: {}", e);
        }
    }
}

/// 获取已发现的设备列表
///
/// # Safety
/// `count_out` 必须指向有效的指针
/// 返回的数组必须使用 `shareself_free_device_list` 释放
#[no_mangle]
pub unsafe extern "C" fn shareself_get_devices(
    count_out: *mut c_int,
) -> *mut FfiDeviceInfo {
    if count_out.is_null() {
        return ptr::null_mut();
    }

    let devices_ref = match &DEVICES {
        Some(d) => d,
        None => {
            *count_out = 0;
            return ptr::null_mut();
        }
    };

    let devices = devices_ref.lock().unwrap();

    if devices.is_empty() {
        *count_out = 0;
        return ptr::null_mut();
    }

    let count = devices.len() as c_int;
    *count_out = count;

    // 使用 Box 分配设备数组
    let boxed_array: Box<[FfiDeviceInfo]> = devices.iter().map(|device| {
        let source_str = match device.source {
            DiscoverySource::MDNS => "mdns",
            DiscoverySource::Broadcast => "broadcast",
            DiscoverySource::HTTPScan => "scan",
        };

        FfiDeviceInfo {
            id: CString::new(device.id.as_str()).map(|s| s.into_raw()).unwrap_or(ptr::null_mut()),
            name: CString::new(device.name.as_str()).map(|s| s.into_raw()).unwrap_or(ptr::null_mut()),
            hostname: CString::new(device.hostname.as_str()).map(|s| s.into_raw()).unwrap_or(ptr::null_mut()),
            address: CString::new(
                device.addresses.first()
                    .map(|a| a.as_str())
                    .unwrap_or("unknown")
            ).map(|s| s.into_raw()).unwrap_or(ptr::null_mut()),
            port: device.port as c_uint,
            service_type: CString::new(device.service_type.as_str()).map(|s| s.into_raw()).unwrap_or(ptr::null_mut()),
            discovery_source: CString::new(source_str).map(|s| s.into_raw()).unwrap_or(ptr::null_mut()),
        }
    }).collect::<Vec<_>>().into_boxed_slice();

    Box::into_raw(boxed_array) as *mut FfiDeviceInfo
}

/// 获取版本信息
///
/// # Safety
/// 返回的字符串必须使用 `shareself_free_string` 释放
#[no_mangle]
pub extern "C" fn shareself_get_version() -> *mut c_char {
    CString::new(crate::VERSION)
        .map(|s| s.into_raw())
        .unwrap_or_else(|_| ptr::null_mut())
}

/// 停止设备发现
///
/// # Safety
/// `handle` 必须是由 `shareself_start_discovery` 返回的有效句柄
#[no_mangle]
pub unsafe extern "C" fn shareself_stop_discovery(
    _handle: *mut FfiDiscoveryHandle,
) -> *mut FfiResult {
    init_global_lock();

    if _handle.is_null() {
        let error = Box::new(FfiResult::error(
            FfiErrorCode::NullPointer,
            "Null discovery handle",
        ));
        return Box::into_raw(error);
    }

    let _lock = DISCOVERY_LOCK.as_ref().unwrap().lock().unwrap();

    // 清空设备列表
    if let Some(devices) = &DEVICES {
        let mut devs = devices.lock().unwrap();
        devs.clear();
    }

    // 取出并销毁实例，这会自动释放 tokio runtime
    let _ = DISCOVERY_INSTANCE.take();

    Box::into_raw(Box::new(FfiResult::success()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_version() {
        let version_ptr = unsafe { shareself_get_version() };
        assert!(!version_ptr.is_null());

        let version = unsafe { CStr::from_ptr(version_ptr) }.to_str().unwrap();
        assert!(!version.is_empty());

        unsafe { shareself_free_string(version_ptr) };
    }
}
