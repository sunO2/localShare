//! 设备发现模块
//!
//! 支持多种发现方式：
//! - mDNS/DNS-SD 标准发现
//! - UDP 广播发现 (LocalSend 风格)
//! - HTTP 子网扫描

pub mod types;
pub mod service;
pub mod browser;
pub mod registrar;
pub mod resolver;
pub mod broadcast;
pub mod scan;
pub mod manager;

pub use types::{DeviceInfo, ServiceIdentifier, TxtRecord, SharedFile};
pub use service::{DiscoveryEvent, DiscoveryHandle, discovery_service};
pub use registrar::{register_service, ServiceHandle};

pub use broadcast::{BroadcastConfig, BroadcastEvent, BroadcastPacket, start_broadcast_discovery};
pub use scan::{ScanConfig, ScanEvent, start_subnet_scan};
pub use manager::{ManagerConfig, ManagedEvent, ManagedDevice, DiscoverySource, start_discovery_manager};
