//! UDP 广播发现模块
//!
//! 模仿 LocalSend 的 UDP 广播机制，适用于移动热点场景

use std::net::{SocketAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use socket2::{Socket, Domain, Type, Protocol};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, broadcast};
use crate::Result;
use tracing::{debug, info, warn};

// Add rand import at module level
use rand::Rng;

/// LocalSend 风格的广播包端口
const BROADCAST_PORT: u16 = 53317;

/// 广播发现配置
#[derive(Debug, Clone)]
pub struct BroadcastConfig {
    /// 绑定端口
    pub bind_port: u16,
    /// 广播端口
    pub broadcast_port: u16,
    /// 发现间隔 (秒)
    pub interval_secs: u64,
    /// 设备名称
    pub device_name: String,
    /// 服务端口
    pub service_port: u16,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        Self {
            bind_port: 0, // 随机端口
            broadcast_port: BROADCAST_PORT,
            interval_secs: 5,
            device_name: gethostname::gethostname()
                .to_string_lossy()
                .to_string(),
            service_port: 8080,
        }
    }
}

/// LocalSend 风格的广播包格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastPacket {
    /// 设备名称
    pub name: String,
    /// 设备类型
    #[serde(rename = "type")]
    pub device_type: String,
    /// HTTP 端口
    pub port: u16,
    /// 加密密钥 (可选)
    pub key: Option<String>,
    /// 版本
    pub version: String,
    /// 标识
    pub id: String,
}

impl BroadcastPacket {
    /// 创建新的广播包
    pub fn new(device_name: String, port: u16) -> Self {
        Self {
            name: device_name,
            device_type: "sharSelf".to_string(),
            port,
            key: None,
            version: env!("CARGO_PKG_VERSION").to_string(),
            id: generate_device_id(),
        }
    }

    /// 序列化为 JSON 字节
    pub fn to_json(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self)
            .map_err(|e| crate::Error::InvalidConfig(format!("Failed to serialize: {}", e)))
    }

    /// 从 JSON 字节反序列化
    pub fn from_json(data: &[u8]) -> Result<Self> {
        serde_json::from_slice(data)
            .map_err(|e| crate::Error::InvalidConfig(format!("Failed to deserialize: {}", e)))
    }
}

/// 广播发现事件
#[derive(Debug, Clone)]
pub enum BroadcastEvent {
    /// 发现新设备
    DeviceFound {
        name: String,
        address: String,
        port: u16,
        packet: BroadcastPacket,
    },
    /// 收到响应
    ResponseReceived {
        from: String,
        packet: BroadcastPacket,
    },
    /// 错误
    Error(String),
}

/// 生成设备 ID
fn generate_device_id() -> String {
    let mut rng = rand::thread_rng();
    let id: u64 = rng.gen();
    hex::encode(id.to_be_bytes())
}

/// 创建 UDP 广播 socket
fn create_broadcast_socket() -> Result<Socket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|e| crate::Error::DiscoveryFailed(format!("Failed to create socket: {}", e)))?;

    // 设置广播选项
    socket.set_broadcast(true)
        .map_err(|e| crate::Error::DiscoveryFailed(format!("Failed to set broadcast: {}", e)))?;

    // 设置重用地址
    socket.set_reuse_address(true)
        .map_err(|e| crate::Error::DiscoveryFailed(format!("Failed to set reuse address: {}", e)))?;

    Ok(socket)
}

/// 广播发现句柄
pub struct BroadcastHandle {
    _shutdown_tx: broadcast::Sender<()>,
}

impl BroadcastHandle {
    /// 停止广播发现
    pub async fn shutdown(self) -> Result<()> {
        // 当 Sender 被释放时，所有 Receiver 会收到错误
        drop(self._shutdown_tx);
        Ok(())
    }
}

/// 启动 UDP 广播发现
pub fn start_broadcast_discovery(
    config: BroadcastConfig,
) -> Result<(mpsc::Receiver<BroadcastEvent>, BroadcastHandle)> {
    let (event_tx, event_rx) = mpsc::channel(100);
    let (shutdown_tx, _) = broadcast::channel(1);

    // 创建 socket
    let socket = create_broadcast_socket()?;

    // 绑定到配置的端口
    let bind_addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, config.bind_port));
    socket.bind(&bind_addr.into())
        .map_err(|e| crate::Error::DiscoveryFailed(format!("Failed to bind: {}", e)))?;

    // 获取实际绑定的端口
    let local_port = socket.local_addr()?
        .as_socket_ipv4()
        .map(|addr| addr.port())
        .unwrap_or(config.bind_port);

    info!("Broadcast discovery listening on port {}", local_port);

    // 转换为 tokio UdpSocket
    let std_socket: std::net::UdpSocket = socket.into();
    std_socket.set_nonblocking(true)
        .map_err(|e| crate::Error::DiscoveryFailed(format!("Failed to set nonblocking: {}", e)))?;
    let udp_socket = Arc::new(UdpSocket::from_std(std_socket)
        .map_err(|e| crate::Error::DiscoveryFailed(format!("Failed to create tokio socket: {}", e)))?);

    let device_name = config.device_name.clone();
    let service_port = config.service_port;
    let broadcast_port = config.broadcast_port;
    let interval_secs = config.interval_secs;

    // 启动发送任务
    let event_tx_clone = event_tx.clone();
    let udp_socket_clone = udp_socket.clone();
    let mut shutdown_rx_send = shutdown_tx.subscribe();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // 发送广播包
                    let packet = BroadcastPacket::new(device_name.clone(), service_port);

                    match packet.to_json() {
                        Ok(data) => {
                            // 广播到常见的广播地址
                            let broadcast_addrs = vec![
                                (Ipv4Addr::new(255, 255, 255, 255), broadcast_port),
                                (Ipv4Addr::new(224, 0, 0, 1), broadcast_port), // mDNS 多播
                            ];

                            for (addr, port) in broadcast_addrs {
                                let dest = SocketAddr::from((addr, port));
                                if let Err(e) = udp_socket_clone.send_to(&data, dest).await {
                                    debug!("Failed to send broadcast to {}: {}", dest, e);
                                } else {
                                    debug!("Sent broadcast to {}", dest);
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to create broadcast packet: {}", e);
                        }
                    }
                }
                _ = shutdown_rx_send.recv() => {
                    info!("Broadcast send task shutting down");
                    break;
                }
            }
        }
    });

    // 启动接收任务
    let mut shutdown_rx_recv = shutdown_tx.subscribe();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65535];

        loop {
            tokio::select! {
                result = udp_socket.recv_buf_from(&mut buf) => {
                    match result {
                        Ok((size, src_addr)) => {
                            let data = &buf[..size];

                            // 尝试解析广播包
                            if let Ok(packet) = BroadcastPacket::from_json(data) {
                                // 忽略自己的包
                                if packet.id != generate_device_id() {
                                    info!("Received broadcast from {}: {}", src_addr, packet.name);

                                    let event = BroadcastEvent::DeviceFound {
                                        name: packet.name.clone(),
                                        address: src_addr.ip().to_string(),
                                        port: packet.port,
                                        packet,
                                    };

                                    if event_tx_clone.send(event).await.is_err() {
                                        break;
                                    }
                                }
                            } else {
                                debug!("Received invalid broadcast packet from {}", src_addr);
                            }
                        }
                        Err(e) => {
                            warn!("Broadcast receive error: {}", e);
                        }
                    }
                }
                _ = shutdown_rx_recv.recv() => {
                    info!("Broadcast receive task shutting down");
                    break;
                }
            }
        }
    });

    let handle = BroadcastHandle {
        _shutdown_tx: shutdown_tx,
    };

    Ok((event_rx, handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broadcast_packet_serialization() {
        let packet = BroadcastPacket::new("Test Device".to_string(), 8080);
        let json = packet.to_json().unwrap();
        let deserialized = BroadcastPacket::from_json(&json).unwrap();

        assert_eq!(packet.name, deserialized.name);
        assert_eq!(packet.port, deserialized.port);
        assert_eq!(packet.device_type, "sharSelf");
    }

    #[test]
    fn test_device_id_generation() {
        let id1 = generate_device_id();
        let id2 = generate_device_id();
        assert_ne!(id1, id2);
        assert_eq!(id1.len(), 16); // 8 bytes = 16 hex chars
    }
}
