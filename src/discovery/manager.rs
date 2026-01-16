//! 统一发现管理器
//!
//! 协调 mDNS、UDP 广播和 HTTP 扫描三种发现方式

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, broadcast};
use crate::Result;
use tracing::{info, debug, warn};

use super::broadcast::{BroadcastConfig, BroadcastEvent, start_broadcast_discovery};
use super::scan::{ScanConfig, ScanEvent, start_subnet_scan};

/// 设备唯一标识符
type DeviceId = String;

/// 发现管理器配置
#[derive(Debug, Clone)]
pub struct ManagerConfig {
    /// 启用 mDNS 发现
    pub enable_mdns: bool,
    /// 启用 UDP 广播发现
    pub enable_broadcast: bool,
    /// 启用 HTTP 扫描
    pub enable_scan: bool,

    /// mDNS 配置
    pub mdns_service_type: String,
    pub mdns_domain: String,

    /// UDP 广播配置
    pub broadcast_config: Option<BroadcastConfig>,

    /// HTTP 扫描配置
    pub scan_config: Option<ScanConfig>,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            enable_mdns: true,
            enable_broadcast: true,
            enable_scan: false, // 默认关闭扫描，比较耗时

            mdns_service_type: "_shareself._tcp.local".to_string(),
            mdns_domain: "local".to_string(),

            broadcast_config: Some(BroadcastConfig::default()),
            scan_config: None,
        }
    }
}

/// 设备信息
#[derive(Debug, Clone)]
pub struct ManagedDevice {
    /// 设备 ID (名称或唯一标识)
    pub id: String,
    /// 设备名称
    pub name: String,
    /// 主机名
    pub hostname: String,
    /// IP 地址列表
    pub addresses: Vec<String>,
    /// 端口
    pub port: u16,
    /// 服务类型
    pub service_type: String,
    /// 发现来源
    pub source: DiscoverySource,
    /// 最后发现时间
    pub last_seen: std::time::Instant,
}

/// 发现来源
#[derive(Debug, Clone, PartialEq)]
pub enum DiscoverySource {
    MDNS,
    Broadcast,
    HTTPScan,
}

/// 统一发现事件
#[derive(Debug, Clone)]
pub enum ManagedEvent {
    /// 发现新设备
    DeviceFound(ManagedDevice),
    /// 设备丢失
    DeviceLost(String),
    /// 设备更新
    DeviceUpdated(ManagedDevice),
    /// 扫描进度
    ScanProgress { current: usize, total: usize },
    /// 错误
    Error(String),
}

/// 发现管理器句柄
pub struct DiscoveryManager {
    _shutdown_tx: broadcast::Sender<()>,
}

impl DiscoveryManager {
    /// 停止所有发现服务
    pub async fn shutdown(self) -> Result<()> {
        drop(self._shutdown_tx);
        Ok(())
    }
}

/// 启动统一发现管理器
pub fn start_discovery_manager(
    config: ManagerConfig,
) -> Result<(mpsc::Receiver<ManagedEvent>, DiscoveryManager)> {
    let (event_tx, event_rx) = mpsc::channel(100);
    let (shutdown_tx, _) = broadcast::channel(1);

    // 设备存储
    let devices: Arc<Mutex<HashMap<DeviceId, ManagedDevice>>> = Arc::new(Mutex::new(HashMap::new()));

    // 1. UDP 广播发现
    if config.enable_broadcast {
        if let Some(broadcast_config) = config.broadcast_config.clone() {
            info!("Starting UDP broadcast discovery");
            match start_broadcast_discovery(broadcast_config) {
                Ok((mut broadcast_rx, broadcast_handle)) => {
                    let event_tx = event_tx.clone();
                    let devices = devices.clone();
                    let mut shutdown_rx = shutdown_tx.subscribe();

                    tokio::spawn(async move {
                        loop {
                            tokio::select! {
                                event = broadcast_rx.recv() => {
                                    match event {
                                        Some(BroadcastEvent::DeviceFound { name, address, port, packet }) => {
                                            let device = ManagedDevice {
                                                id: packet.id.clone(),
                                                name: name.clone(),
                                                hostname: address.clone(),
                                                addresses: vec![address.clone()],
                                                port,
                                                service_type: "broadcast".to_string(),
                                                source: DiscoverySource::Broadcast,
                                                last_seen: std::time::Instant::now(),
                                            };

                                            handle_device_found(device, &devices, &event_tx).await;
                                        }
                                        Some(BroadcastEvent::ResponseReceived { from, packet }) => {
                                            debug!("Broadcast response from {}: {}", from, packet.name);
                                        }
                                        Some(BroadcastEvent::Error(e)) => {
                                            warn!("Broadcast error: {}", e);
                                            let _ = event_tx.send(ManagedEvent::Error(e)).await;
                                        }
                                        None => {
                                            info!("Broadcast receiver closed");
                                            break;
                                        }
                                    }
                                }
                                _ = shutdown_rx.recv() => {
                                    info!("Broadcast task shutting down");
                                    let _ = broadcast_handle.shutdown().await;
                                    break;
                                }
                            }
                        }
                    });
                }
                Err(e) => {
                    warn!("Failed to start broadcast discovery: {}", e);
                }
            }
        }
    }

    // 2. HTTP 扫描
    if config.enable_scan {
        if let Some(scan_config) = config.scan_config.clone() {
            info!("Starting HTTP subnet scan");
            match start_subnet_scan(scan_config) {
                Ok((mut scan_rx, scan_handle)) => {
                    let event_tx = event_tx.clone();
                    let devices = devices.clone();
                    let mut shutdown_rx = shutdown_tx.subscribe();

                    tokio::spawn(async move {
                        loop {
                            tokio::select! {
                                event = scan_rx.recv() => {
                                    match event {
                                        Some(ScanEvent::DeviceFound { name, address, port, info }) => {
                                            let device = ManagedDevice {
                                                id: info.id.clone(),
                                                name: name.clone(),
                                                hostname: address.clone(),
                                                addresses: vec![address.clone()],
                                                port,
                                                service_type: "http".to_string(),
                                                source: DiscoverySource::HTTPScan,
                                                last_seen: std::time::Instant::now(),
                                            };

                                            handle_device_found(device, &devices, &event_tx).await;
                                        }
                                        Some(ScanEvent::Progress { current, total }) => {
                                            let _ = event_tx.send(ManagedEvent::ScanProgress { current, total }).await;
                                        }
                                        Some(ScanEvent::Complete { found_count }) => {
                                            info!("HTTP scan complete, found {} devices", found_count);
                                        }
                                        Some(ScanEvent::Error(e)) => {
                                            warn!("Scan error: {}", e);
                                            let _ = event_tx.send(ManagedEvent::Error(e)).await;
                                        }
                                        None => {
                                            info!("Scan receiver closed");
                                            break;
                                        }
                                    }
                                }
                                _ = shutdown_rx.recv() => {
                                    info!("Scan task shutting down");
                                    let _ = scan_handle.shutdown().await;
                                    break;
                                }
                            }
                        }
                    });
                }
                Err(e) => {
                    warn!("Failed to start subnet scan: {}", e);
                }
            }
        }
    }

    // 启动设备超时检查任务
    let devices_cleanup = devices.clone();
    let event_tx_cleanup = event_tx.clone();
    let mut shutdown_rx_cleanup = shutdown_tx.subscribe();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let mut devices = devices_cleanup.lock().await;
                    let timeout = tokio::time::Duration::from_secs(120);
                    let now = std::time::Instant::now();

                    devices.retain(|id, device| {
                        if now.duration_since(device.last_seen) > timeout {
                            debug!("Device timeout: {}", id);
                            let _ = event_tx_cleanup.try_send(ManagedEvent::DeviceLost(id.clone()));
                            false
                        } else {
                            true
                        }
                    });
                }
                _ = shutdown_rx_cleanup.recv() => {
                    info!("Cleanup task shutting down");
                    break;
                }
            }
        }
    });

    let manager = DiscoveryManager {
        _shutdown_tx: shutdown_tx,
    };

    Ok((event_rx, manager))
}

/// 处理设备发现
async fn handle_device_found(
    device: ManagedDevice,
    devices: &Arc<Mutex<HashMap<DeviceId, ManagedDevice>>>,
    event_tx: &mpsc::Sender<ManagedEvent>,
) {
    let mut devices = devices.lock().await;

    if let Some(existing) = devices.get_mut(&device.id) {
        // 设备已存在，更新
        let updated = existing.last_seen < device.last_seen;
        if updated {
            existing.addresses = device.addresses;
            existing.port = device.port;
            existing.last_seen = device.last_seen;

            debug!("Device updated: {}", device.name);
            let _ = event_tx.send(ManagedEvent::DeviceUpdated(existing.clone())).await;
        }
    } else {
        // 新设备
        info!("New device found: {} ({:?})", device.name, device.source);
        devices.insert(device.id.clone(), device.clone());
        let _ = event_tx.send(ManagedEvent::DeviceFound(device)).await;
    }
}

/// 获取所有已发现的设备
pub async fn get_all_devices(
    devices: &Arc<Mutex<HashMap<DeviceId, ManagedDevice>>>,
) -> Vec<ManagedDevice> {
    devices.lock().await.values().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_manager_start() {
        let config = ManagerConfig {
            enable_mdns: false,
            enable_broadcast: false,
            enable_scan: false,
            ..Default::default()
        };

        let result = start_discovery_manager(config);
        assert!(result.is_ok());

        let (_event_rx, manager) = result.unwrap();
        let _ = manager.shutdown().await;
    }
}
