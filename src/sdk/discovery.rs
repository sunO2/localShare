//! 设备发现模块封装
//!
//! 封装底层 discovery 模块，提供统一的设备发现接口

use std::sync::Arc;
use crate::discovery::manager::{ManagerConfig, ManagedEvent, DiscoveryManager, start_discovery_manager};
use super::{types::DeviceInfo, state::StateHandle, events::EventSender, SDKResult, SDKError};

/// 设备发现管理器
pub struct DiscoveryModule {
    _manager: Option<DiscoveryManager>,
    state: StateHandle,
    event_tx: EventSender,
}

impl DiscoveryModule {
    pub fn new(state: StateHandle, event_tx: EventSender) -> Self {
        Self {
            _manager: None,
            state,
            event_tx,
        }
    }

    /// 启动设备发现
    pub async fn start(&mut self, port: u16) -> SDKResult<()> {
        if self.state.is_discovery_running() {
            return Err(SDKError::AlreadyRunning);
        }

        // 创建广播配置
        let broadcast_config = Some(crate::discovery::broadcast::BroadcastConfig {
            bind_port: 0,
            broadcast_port: 53317,
            interval_secs: 5,
            device_name: gethostname::gethostname()
                .to_string_lossy()
                .to_string(),
            service_port: port,
        });

        let config = ManagerConfig {
            enable_mdns: true,
            enable_broadcast: true,
            enable_scan: false,
            mdns_service_type: "_shareself._tcp.local".to_string(),
            mdns_domain: "local".to_string(),
            broadcast_config,
            scan_config: None,
        };

        // 启动发现管理器
        let (mut event_rx, manager) = start_discovery_manager(config)
            .map_err(|e| SDKError::Discovery(e.to_string()))?;

        // 启动事件处理任务
        let state = self.state.clone();
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                Self::handle_event(event, &state, &event_tx);
            }
        });

        self._manager = Some(manager);
        self.state.set_discovery_running(true);

        Ok(())
    }

    /// 停止设备发现
    pub async fn stop(&mut self) -> SDKResult<()> {
        if !self.state.is_discovery_running() {
            return Err(SDKError::NotFound("Discovery not running".to_string()));
        }

        if let Some(manager) = self._manager.take() {
            manager.shutdown().await
                .map_err(|e| SDKError::Discovery(e.to_string()))?;
        }

        self.state.set_discovery_running(false);
        Ok(())
    }

    /// 获取已发现的设备
    pub fn get_devices(&self) -> Vec<DeviceInfo> {
        self.state.get_devices()
    }

    /// 处理发现事件
    fn handle_event(
        event: ManagedEvent,
        state: &StateHandle,
        event_tx: &EventSender,
    ) {
        match event {
            ManagedEvent::DeviceFound(device) => {
                tracing::info!("Device found: {} via {:?}", device.name, device.source);
                let device_info = DeviceInfo {
                    id: device.id,
                    name: device.name,
                    hostname: device.hostname,
                    addresses: device.addresses,
                    port: device.port,
                    service_type: device.service_type,
                    source: device.source,
                    last_seen: device.last_seen,
                };
                state.add_device(device_info);
                event_tx.send(crate::sdk::types::SDKEvent::DeviceFound(device_info));
            }
            ManagedEvent::DeviceLost(id) => {
                tracing::info!("Device lost: {}", id);
                state.remove_device(&id);
                event_tx.send(crate::sdk::types::SDKEvent::DeviceLost(id));
            }
            ManagedEvent::DeviceUpdated(device) => {
                tracing::debug!("Device updated: {}", device.name);
                let device_info = DeviceInfo {
                    id: device.id,
                    name: device.name.clone(),
                    hostname: device.hostname,
                    addresses: device.addresses,
                    port: device.port,
                    service_type: device.service_type,
                    source: device.source,
                    last_seen: device.last_seen,
                };
                // 更新设备
                state.remove_device(&device.id);
                state.add_device(device_info);
            }
            ManagedEvent::ScanProgress { .. } => {
                // 扫描进度，暂不处理
            }
            ManagedEvent::Error(e) => {
                tracing::error!("Discovery error: {}", e);
                event_tx.send(crate::sdk::types::SDKEvent::Error(e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discovery_module() {
        let state = StateHandle::new();
        let (tx, _rx) = crate::sdk::events::event_channel();
        let event_tx = EventSender::new(tx);

        let mut discovery = DiscoveryModule::new(state, event_tx);

        // 测试启动（可能不会真正发现设备）
        let result = discovery.start(8080).await;
        // 在实际环境中可能成功或失败，取决于网络配置
        println!("Start result: {:?}", result);
    }
}
