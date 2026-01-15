//! 设备发现服务主逻辑

use super::types::DeviceInfo;
use crate::common::{config::DiscoveryConfig, error::{Error, Result}};
use crate::mdns::{socket::MdnsSocket, socket::MdnsSocketConfig, query::MdnsQuery, packet::{RecordType, RecordData}};
use tokio::sync::{mpsc, oneshot, broadcast};
use tokio::time::Duration;
use std::collections::HashMap;
use std::net::SocketAddr;

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

    /// 后台任务句柄
    task_handle: Option<tokio::task::JoinHandle<()>>,
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
            task_handle: self.task_handle,
        };
        (self.event_rx, shutdown)
    }

    /// 关闭发现服务
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        // 终止后台任务
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
        // 等待一小段时间让任务清理
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        Ok(())
    }
}

/// 关闭句柄（可与事件接收器分离使用）
#[derive(Debug)]
pub struct ShutdownHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl ShutdownHandle {
    /// 关闭发现服务
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
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

    // 启动 mDNS 监听任务
    let task_handle = tokio::spawn(async move {
        run_discovery(config, event_tx, shutdown_rx).await;
    });

    Ok(DiscoveryHandle {
        event_rx,
        shutdown_tx: Some(shutdown_tx),
        task_handle: Some(task_handle),
    })
}

/// 运行发现服务的内部逻辑
async fn run_discovery(
    config: DiscoveryConfig,
    event_tx: mpsc::Sender<DiscoveryEvent>,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    tracing::info!("Starting device discovery for {}", config.service_type);

    // 创建 mDNS socket (用于发送)
    let socket_config = MdnsSocketConfig {
        enable_ipv6: config.enable_ipv6,
        ..Default::default()
    };

    let socket = match MdnsSocket::new(socket_config.clone()) {
        Ok(s) => s,
        Err(e) => {
            let _ = event_tx.send(DiscoveryEvent::Error(e)).await;
            return;
        }
    };

    // 创建独立的 socket 用于接收
    let recv_socket = match MdnsSocket::new(socket_config) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to create recv socket: {}, discovery may not work properly", e);
            let _ = event_tx.send(DiscoveryEvent::Error(e)).await;
            return;
        }
    };

    let mut state = DiscoveryState::new();
    let mut cleanup_interval = tokio::time::interval(Duration::from_secs(30));
    let mut query_interval = tokio::time::interval(Duration::from_secs(60));

    // 创建通道用于接收数据包
    let (packet_tx, mut packet_rx) = mpsc::channel::<(Vec<u8>, SocketAddr)>(100);

    // 创建 broadcast channel 用于通知阻塞线程退出
    let (thread_shutdown_tx, _) = broadcast::channel::<()>(1);

    // 在独立线程中运行 socket 接收
    tokio::task::spawn_blocking({
        let mut thread_shutdown_rx = thread_shutdown_tx.subscribe();
        move || {
            let mut buffer = vec![0u8; 4096];
            loop {
                // 检查是否收到关闭信号
                match thread_shutdown_rx.try_recv() {
                    Ok(_) | Err(broadcast::error::TryRecvError::Closed) | Err(broadcast::error::TryRecvError::Lagged(_)) => {
                        tracing::debug!("Discovery blocking thread received shutdown signal");
                        break;
                    }
                    Err(broadcast::error::TryRecvError::Empty) => {}
                }

                match recv_socket.recv_from(&mut buffer) {
                    Ok((size, addr)) => {
                        let data = buffer[..size].to_vec();
                        if let Err(e) = packet_tx.blocking_send((data, addr)) {
                            tracing::debug!("Failed to send packet to channel: {}", e);
                            break;
                        }
                    }
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        }
    });

    // 首次查询
    if config.active_browse {
        if let Err(e) = send_ptr_query(&socket, &config.service_type).await {
            tracing::warn!("Failed to send initial PTR query: {}", e);
        }
    }

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                tracing::info!("Discovery service shutting down");

                // 通知阻塞线程退出
                let _ = thread_shutdown_tx.send(());

                // 等待一小段时间让线程清理
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                break;
            }

            _ = cleanup_interval.tick() => {
                // 清理过期设备
                let events = state.cleanup_expired(config.timeout_secs.unwrap_or(120));
                for event in events {
                    let _ = event_tx.send(event).await;
                }
            }

            _ = query_interval.tick() => {
                // 定期重新发送 PTR 查询
                if config.active_browse {
                    if let Err(e) = send_ptr_query(&socket, &config.service_type).await {
                        tracing::warn!("Failed to send periodic PTR query: {}", e);
                    }
                }
            }

            Some((data, addr)) = packet_rx.recv() => {
                // 处理接收到的数据包
                if let Err(e) = handle_mdns_packet(&data, addr, &mut state, &event_tx).await {
                    tracing::warn!("Failed to handle mDNS packet: {}", e);
                }
            }
        }
    }

    tracing::info!("Discovery service stopped");
}

/// 发送 PTR 查询
async fn send_ptr_query(socket: &MdnsSocket, service_type: &str) -> Result<()> {
    let query = MdnsQuery::query_ptr(service_type.to_string());
    query.send(socket)?;
    tracing::debug!("Sent PTR query for {}", service_type);
    Ok(())
}

/// 处理 mDNS 数据包
async fn handle_mdns_packet(
    data: &[u8],
    source: SocketAddr,
    state: &mut DiscoveryState,
    event_tx: &mpsc::Sender<DiscoveryEvent>,
) -> Result<()> {
    use crate::mdns::packet::MdnsPacket;

    // 解析数据包
    let packet = MdnsPacket::decode(data)?;

    // 只处理响应包
    if !packet.is_response() {
        return Ok(());
    }

    tracing::trace!("Received mDNS response from {} with {} records",
        source, packet.answers.len() + packet.additionals.len());

    // 处理 PTR 记录（服务发现）
    for record in &packet.answers {
        if record.rtype == RecordType::PTR {
            if let RecordData::Ptr(service_name) = &record.data {
                // 从服务名称提取设备名称
                // 格式: "MyDevice._shareself._tcp.local"
                if let Some(device_name) = extract_device_name(service_name) {
                    // 查找完整的设备信息
                    if let Some(device_info) = find_device_info(&packet, &device_name, service_name) {
                        if let Some(event) = state.update_device(device_info) {
                            let _ = event_tx.send(event).await;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// 从服务全名中提取设备名称
fn extract_device_name(service_full_name: &str) -> Option<String> {
    // 格式: "MyDevice._shareself._tcp.local"
    let parts: Vec<&str> = service_full_name.split('.').collect();
    if parts.len() >= 4 {
        Some(parts[0].to_string())
    } else {
        None
    }
}

/// 从数据包中查找完整的设备信息
fn find_device_info(
    packet: &crate::mdns::packet::MdnsPacket,
    device_name: &str,
    service_full_name: &str,
) -> Option<DeviceInfo> {
    // 查找 SRV 记录
    let srv_record = packet.answers.iter()
        .chain(packet.additionals.iter())
        .find(|r| r.rtype == RecordType::SRV && r.name == service_full_name)?;

    let (_priority, weight, port, target) = if let RecordData::Srv { priority, weight, port, target } = &srv_record.data {
        (*priority, *weight, *port, target.clone())
    } else {
        return None;
    };

    // 查找 TXT 记录
    let txt_records = packet.answers.iter()
        .chain(packet.additionals.iter())
        .filter(|r| r.rtype == RecordType::TXT && r.name == service_full_name)
        .filter_map(|r| {
            if let RecordData::Txt(strings) = &r.data {
                Some(parse_txt_records(strings))
            } else {
                None
            }
        })
        .next()
        .unwrap_or_default();

    // 查找 A/AAAA 记录
    let mut addresses = Vec::new();
    for record in packet.answers.iter().chain(packet.additionals.iter()) {
        if record.name == target {
            match &record.data {
                RecordData::A(addr) => {
                    addresses.push(SocketAddr::from((*addr, port)));
                }
                RecordData::Aaaa(addr) => {
                    addresses.push(SocketAddr::from((*addr, port)));
                }
                _ => {}
            }
        }
    }

    // 如果没有找到地址记录，尝试从源地址构建
    if addresses.is_empty() {
        // 这里需要从 source 参数获取地址，暂时跳过
    }

    Some(DeviceInfo {
        name: device_name.to_string(),
        hostname: target.trim_end_matches('.').to_string(),
        addresses,
        port,
        txt_records,
        service_type: service_full_name
            .split('.')
            .skip(1)
            .take(2)
            .collect::<Vec<_>>()
            .join("."),
        last_seen: std::time::Instant::now(),
    })
}

/// 解析 TXT 记录
fn parse_txt_records(strings: &[String]) -> HashMap<String, String> {
    let mut records = HashMap::new();
    for s in strings {
        if let Some((key, value)) = s.split_once('=') {
            records.insert(key.to_string(), value.to_string());
        }
    }
    records
}
