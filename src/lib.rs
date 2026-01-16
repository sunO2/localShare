//! sharSelf - 局域网文件分享库
//!
//! 本库提供跨平台的局域网文件分享功能，支持设备发现和文件传输。

// 核心模块
pub mod common;
pub mod discovery;
pub mod mdns;

// SDK 模块
pub mod sdk;
pub mod sdk_main;

// FFI 模块
pub mod ffi;
pub mod sdk_ffi;  // 新 SDK FFI 层

// BitTorrent 传输模块
pub mod torrent;

// 重新导出常用类型
pub use common::error::{Error, Result};
pub use common::config::{DiscoveryConfig, ServiceConfig};

pub use discovery::{
    registrar::register_service,
    service::{DiscoveryEvent, DiscoveryHandle},
    types::{DeviceInfo, ServiceIdentifier},
};

// 预留传输模块
pub mod transport {
    //! 文件传输模块 (待实现)

    /// 传输会话句柄
    pub struct TransferHandle;

    /// 文件传输进度
    pub struct TransferProgress {
        pub transferred: u64,
        pub total: u64,
        pub percentage: f32,
    }
}

/// 库版本信息
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 默认服务类型
pub const DEFAULT_SERVICE_TYPE: &str = "_shareself._tcp.local";

/// 默认 mDNS 端口
pub const MDNS_PORT: u16 = 5353;
