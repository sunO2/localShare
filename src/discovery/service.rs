//! 设备发现服务主逻辑

use super::types::DeviceInfo;
use crate::common::{config::DiscoveryConfig, error::{Error, Result}};
use tokio::sync::{mpsc, oneshot};
use std::collections::HashMap;

/// 设备发现事件
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    /// 发现新设备
    DeviceFound(DeviceInfo),

    /// 设备离线
    DeviceLost(String),  // 设备名称

    /// 设备信息更新
    DeviceUpdated(DeviceInfo),

    /// 错误事件
    Error(Error),
}

/// 设备发现句柄
///
/// 用于控制设备发现服务和接收事件
#[derive(Debug)]
pub struct DiscoveryHandle {
    /// 事件接收器
    event_rx: mpsc::Receiver<DiscoveryEvent>,

    /// 关闭信号发送器
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl DiscoveryHandle {
    /// 获取事件接收器
    pub fn receiver(&mut self) -> &mut mpsc::Receiver<DiscoveryEvent> {
        &mut self.event_rx
    }

    /// 分离事件接收器，独立管理
    pub fn split(self) -> (mpsc::Receiver<DiscoveryEvent>, ShutdownHandle) {
        let shutdown = ShutdownHandle {
            shutdown_tx: self.shutdown_tx,
        };
        (self.event_rx, shutdown)
    }

    /// 关闭发现服务
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            tx.send(()).map_err(|_| Error::Other("Failed to send shutdown signal".to_string()))?;
        }
        Ok(())
    }
}

/// 关闭句柄（可与事件接收器分离使用）
#[derive(Debug)]
pub struct ShutdownHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl ShutdownHandle {
    /// 关闭发现服务
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            tx.send(()).map_err(|_| Error::Other("Failed to send shutdown signal".to_string()))?;
        }
        Ok(())
    }
}

/// 发现服务内部状态
struct DiscoveryState {
    /// 已发现的设备
    devices: HashMap<String, DeviceInfo>,
}

impl DiscoveryState {
    fn new() -> Self {
        Self {
            devices: HashMap::new(),
        }
    }

    /// 更新或添加设备
    fn update_device(&mut self, device: DeviceInfo) -> Option<DiscoveryEvent> {
        let name = device.name.clone();
        let is_new = !self.devices.contains_key(&name);

        self.devices.insert(name.clone(), device.clone());

        if is_new {
            Some(DiscoveryEvent::DeviceFound(device))
        } else {
            Some(DiscoveryEvent::DeviceUpdated(device))
        }
    }

    /// 移除设备
    fn remove_device(&mut self, name: &str) -> Option<DiscoveryEvent> {
        self.devices.remove(name).map(|_| DiscoveryEvent::DeviceLost(name.to_string()))
    }

    /// 获取设备
    fn get_device(&self, name: &str) -> Option<&DeviceInfo> {
        self.devices.get(name)
    }

    /// 获取所有设备
    fn get_all_devices(&self) -> Vec<DeviceInfo> {
        self.devices.values().cloned().collect()
    }

    /// 清理过期设备
    fn cleanup_expired(&mut self, ttl_secs: u64) -> Vec<DiscoveryEvent> {
        let mut events = Vec::new();
        let expired: Vec<String> = self.devices
            .iter()
            .filter(|(_, device)| device.is_expired(ttl_secs))
            .map(|(name, _)| name.clone())
            .collect();

        for name in expired {
            if let Some(event) = self.remove_device(&name) {
                events.push(event);
            }
        }

        events
    }
}

/// 创建设备发现服务
///
/// # Arguments
///
/// * `config` - 发现服务配置
///
/// # Returns
///
/// 返回发现服务句柄，通过它可以接收发现事件
pub fn discovery_service(config: DiscoveryConfig) -> Result<DiscoveryHandle> {
    let (event_tx, event_rx) = mpsc::channel(100);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    // TODO: 启动 mDNS 监听任务
    // tokio::spawn(async move {
    //     run_discovery(config, event_tx, shutdown_rx).await;
    // });

    Ok(DiscoveryHandle {
        event_rx,
        shutdown_tx: Some(shutdown_tx),
    })
}

/// 运行发现服务的内部逻辑
async fn run_discovery(
    config: DiscoveryConfig,
    event_tx: mpsc::Sender<DiscoveryEvent>,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    let mut state = DiscoveryState::new();
    let mut cleanup_interval = tokio::time::interval(std::time::Duration::from_secs(30));

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                tracing::info!("Discovery service shutting down");
                break;
            }
            _ = cleanup_interval.tick() => {
                // 清理过期设备
                let events = state.cleanup_expired(config.timeout_secs.unwrap_or(120));
                for event in events {
                    let _ = event_tx.send(event).await;
                }
            }
        }
    }
}
