//! 设备发现模块
//!
//! 基于 mDNS + DNS-SD 的设备发现服务

pub mod types;
pub mod service;
pub mod browser;
pub mod registrar;
pub mod resolver;

pub use types::{DeviceInfo, ServiceIdentifier, TxtRecord, SharedFile};
pub use service::{DiscoveryEvent, DiscoveryHandle, discovery_service};
pub use registrar::{register_service, ServiceHandle};
