//! 服务注册（主动广播自己）
//!
//! 负责将当前设备注册到局域网，使其他设备能够发现

use crate::common::{config::ServiceConfig, error::Result};
use crate::mdns::{socket::MdnsSocket, socket::MdnsSocketConfig, packet::{MdnsPacket, MdnsRecord, RecordData, RecordType, RecordClass}};
use tokio::sync::{oneshot, mpsc, broadcast};
use tokio::time::{interval, Duration};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 服务注册句柄
///
/// 用于控制已注册的服务
pub struct ServiceHandle {
    /// 服务名称
    service_name: String,

    /// 关闭信号发送器
    shutdown_tx: Option<oneshot::Sender<()>>,

    /// 后台任务句柄
    task_handle: Option<tokio::task::JoinHandle<()>>,

    /// TXT 记录更新发送器
    txt_update_tx: mpsc::Sender<HashMap<String, String>>,
}

impl ServiceHandle {
    /// 获取服务名称
    pub fn name(&self) -> &str {
        &self.service_name
    }

    /// 更新 TXT 记录
    pub async fn update_txt(&mut self, records: HashMap<String, String>) -> Result<()> {
        self.txt_update_tx.send(records).await
            .map_err(|e| crate::common::error::Error::Other(format!("Failed to update TXT: {}", e)))?;
        tracing::info!("TXT records update requested for service: {}", self.service_name);
        Ok(())
    }

    /// 注销服务
    pub async fn unregister(mut self) -> Result<()> {
        tracing::info!("Unregistering service: {}", self.service_name);

        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        // 终止后台任务
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
        }

        // 等待任务清理
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok(())
    }
}

/// 注册当前设备到局域网
///
/// # Arguments
///
/// * `config` - 服务配置
///
/// # Returns
///
/// 返回服务句柄，用于控制已注册的服务
pub fn register_service(config: ServiceConfig) -> Result<ServiceHandle> {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let (txt_update_tx, mut txt_update_rx) = mpsc::channel::<HashMap<String, String>>(10);

    let service_name = config.service_name.clone();
    let port = config.port;

    tracing::info!("Registering service: {} on port {}", service_name, port);

    // 将初始 TXT 记录包装在 Arc 中以便共享
    let txt_records = Arc::new(RwLock::new(config.txt_records.clone()));

    // 启动 mDNS 广播任务
    let task_handle = tokio::spawn(async move {
        run_registration_with_updates(config, shutdown_rx, txt_update_rx, txt_records).await;
    });

    Ok(ServiceHandle {
        service_name,
        shutdown_tx: Some(shutdown_tx),
        task_handle: Some(task_handle),
        txt_update_tx,
    })
}

/// 运行服务注册的内部逻辑（支持动态更新）
async fn run_registration_with_updates(
    config: ServiceConfig,
    mut shutdown_rx: oneshot::Receiver<()>,
    mut txt_update_rx: mpsc::Receiver<HashMap<String, String>>,
    txt_records: Arc<RwLock<HashMap<String, String>>>,
) {
    tracing::info!("Starting service registration for {}", config.service_name);

    // 创建 mDNS socket 配置
    let socket_config = MdnsSocketConfig {
        enable_ipv6: config.enable_ipv6,
        ..Default::default()
    };

    // 创建独立的 socket 用于发送和接收
    let send_socket = match MdnsSocket::new(socket_config.clone()) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create mDNS send socket for registration: {}", e);
            return;
        }
    };

    let recv_socket = match MdnsSocket::new(socket_config) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create mDNS recv socket for registration: {}", e);
            return;
        }
    };

    // 构建服务实例全名
    let service_instance = format!("{}.{}.local",
        config.service_name,
        config.service_type
    );

    // 获取本地 IP 地址
    let local_ips = get_local_ip_addresses(&config);

    if local_ips.is_empty() {
        tracing::warn!("No local IP addresses found, service registration may not work properly");
    }

    // 首次通告
    if let Err(e) = send_announcement_with_txt(&send_socket, &config, &service_instance, &local_ips, &txt_records).await {
        tracing::warn!("Failed to send initial announcement: {}", e);
    }

    // 设置定期重广播间隔
    let announce_interval = Duration::from_secs(config.ttl as u64 / 2);
    let mut ticker = interval(announce_interval);

    // 创建通道用于接收查询包
    let (query_tx, mut query_rx) = mpsc::channel::<(Vec<u8>, std::net::SocketAddr)>(100);

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
                        tracing::debug!("Blocking thread received shutdown signal");
                        break;
                    }
                    Err(broadcast::error::TryRecvError::Empty) => {}
                }

                // 使用超时 recv，避免永久阻塞
                match recv_socket.recv_from(&mut buffer) {
                    Ok((size, addr)) => {
                        let data = buffer[..size].to_vec();
                        if query_tx.blocking_send((data, addr)).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        // 接收失败，短暂休眠后继续检查关闭信号
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        }
    });

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                tracing::info!("Service registration shutting down");

                // 发送注销消息
                if let Err(e) = send_goodbye(&send_socket, &config, &service_instance).await {
                    tracing::warn!("Failed to send goodbye message: {}", e);
                }

                // 通知阻塞线程退出
                let _ = thread_shutdown_tx.send(());

                // 等待一小段时间让线程清理
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                break;
            }

            Some(new_txt) = txt_update_rx.recv() => {
                // 更新 TXT 记录
                *txt_records.write().await = new_txt;

                // 立即发送新的通告
                if let Err(e) = send_announcement_with_txt(&send_socket, &config, &service_instance, &local_ips, &txt_records).await {
                    tracing::warn!("Failed to send updated announcement: {}", e);
                }

                tracing::info!("TXT records updated and announcement sent for {}", config.service_name);
            }

            _ = ticker.tick() => {
                // 定期重新发送通告
                if let Err(e) = send_announcement_with_txt(&send_socket, &config, &service_instance, &local_ips, &txt_records).await {
                    tracing::warn!("Failed to send periodic announcement: {}", e);
                }
            }

            Some((data, addr)) = query_rx.recv() => {
                // 处理接收到的查询包
                if let Err(e) = handle_query_packet(&data, addr, &send_socket, &config, &service_instance, &local_ips, &txt_records).await {
                    tracing::trace!("Failed to handle query packet: {}", e);
                }
            }
        }
    }

    tracing::info!("Service registration stopped");
}

/// 发送服务通告（使用动态 TXT 记录）
async fn send_announcement_with_txt(
    socket: &MdnsSocket,
    config: &ServiceConfig,
    service_instance: &str,
    local_ips: &[IpAddr],
    txt_records: &Arc<RwLock<HashMap<String, String>>>,
) -> Result<()> {
    let txt = txt_records.read().await;
    let packet = build_announcement_packet_with_txt(config, service_instance, local_ips, &txt)?;
    drop(txt);
    let data = packet.encode()?;

    tracing::debug!("Sending service announcement for {}", service_instance);

    // 发送到 IPv4 组播地址
    socket.send_to_v4(&data)?;

    // 如果启用 IPv6，也发送到 IPv6 组播地址
    if config.enable_ipv6 {
        socket.send_to_v6(&data)?;
    }

    Ok(())
}

/// 发送注销消息
async fn send_goodbye(
    socket: &MdnsSocket,
    config: &ServiceConfig,
    service_instance: &str,
) -> Result<()> {
    let packet = build_goodbye_packet(config, service_instance)?;
    let data = packet.encode()?;

    tracing::debug!("Sending goodbye message for {}", service_instance);

    // 发送到 IPv4 组播地址
    socket.send_to_v4(&data)?;

    // 如果启用 IPv6，也发送到 IPv6 组播地址
    if config.enable_ipv6 {
        socket.send_to_v6(&data)?;
    }

    Ok(())
}

/// 处理查询数据包
async fn handle_query_packet(
    data: &[u8],
    _source: std::net::SocketAddr,
    socket: &MdnsSocket,
    config: &ServiceConfig,
    service_instance: &str,
    local_ips: &[IpAddr],
    txt_records: &Arc<RwLock<HashMap<String, String>>>,
) -> Result<()> {
    use crate::mdns::packet::MdnsPacket;

    // 解析数据包
    let packet = MdnsPacket::decode(data)?;

    // 只处理查询包
    if packet.is_response() {
        return Ok(());
    }

    // 检查是否有相关的查询
    let should_respond = packet.questions.iter().any(|q| {
        match q.qtype {
            RecordType::PTR => {
                // 检查是否查询我们的服务类型
                q.name == config.service_type || q.name.ends_with(&format!("{}.local", config.service_type))
            }
            RecordType::SRV | RecordType::TXT => {
                // 检查是否查询我们的服务实例
                q.name == service_instance || q.name == service_instance.trim_end_matches('.')
            }
            RecordType::A | RecordType::AAAA => {
                // 检查是否查询我们的主机
                if let Some(hostname) = &config.hostname {
                    q.name == *hostname || q.name == format!("{}.local", hostname)
                } else {
                    false
                }
            }
            _ => false,
        }
    });

    if should_respond {
        tracing::debug!("Responding to mDNS query from {}", _source);

        // 发送响应
        let response = build_response_packet_with_txt(config, service_instance, local_ips, txt_records).await?;
        let response_data = response.encode()?;

        socket.send_to_v4(&response_data)?;
    }

    Ok(())
}

/// 构建通告数据包（使用动态 TXT）
fn build_announcement_packet_with_txt<'a>(
    config: &ServiceConfig,
    service_instance: &str,
    local_ips: &[IpAddr],
    txt_records: &'a HashMap<String, String>,
) -> Result<MdnsPacket> {
    let mut packet = MdnsPacket::response();

    // 1. PTR 记录 (服务类型 -> 服务实例)
    let ptr_name = format!("{}.local", config.service_type);
    packet.answers.push(MdnsRecord {
        name: ptr_name,
        rtype: RecordType::PTR,
        rclass: RecordClass::IN,
        ttl: config.ttl,
        data: RecordData::Ptr(service_instance.to_string()),
    });

    // 2. SRV 记录 (服务实例 -> 主机名 + 端口)
    let hostname = config.hostname.as_ref()
        .map(|h| format!("{}.local", h))
        .unwrap_or_else(|| service_instance.split('.').next().unwrap_or("localhost").to_string() + ".local");

    packet.answers.push(MdnsRecord {
        name: service_instance.to_string(),
        rtype: RecordType::SRV,
        rclass: RecordClass::IN,
        ttl: config.ttl,
        data: RecordData::Srv {
            priority: 0,
            weight: 0,
            port: config.port,
            target: hostname.clone(),
        },
    });

    // 3. TXT 记录 (服务实例 -> 文本信息)
    let txt_strings: Vec<String> = txt_records.iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();

    if !txt_strings.is_empty() {
        packet.answers.push(MdnsRecord {
            name: service_instance.to_string(),
            rtype: RecordType::TXT,
            rclass: RecordClass::IN,
            ttl: config.ttl,
            data: RecordData::Txt(txt_strings),
        });
    }

    // 4. A/AAAA 记录 (主机名 -> IP 地址)
    for ip in local_ips {
        match ip {
            IpAddr::V4(addr) => {
                packet.additionals.push(MdnsRecord {
                    name: hostname.clone(),
                    rtype: RecordType::A,
                    rclass: RecordClass::IN,
                    ttl: config.ttl,
                    data: RecordData::A(*addr),
                });
            }
            IpAddr::V6(addr) => {
                packet.additionals.push(MdnsRecord {
                    name: hostname.clone(),
                    rtype: RecordType::AAAA,
                    rclass: RecordClass::IN,
                    ttl: config.ttl,
                    data: RecordData::Aaaa(*addr),
                });
            }
        }
    }

    Ok(packet)
}

/// 构建响应数据包（使用动态 TXT）
async fn build_response_packet_with_txt(
    config: &ServiceConfig,
    service_instance: &str,
    local_ips: &[IpAddr],
    txt_records: &Arc<RwLock<HashMap<String, String>>>,
) -> Result<MdnsPacket> {
    let txt = txt_records.read().await;
    let mut packet = MdnsPacket::response();

    // PTR 记录
    let ptr_name = format!("{}.local", config.service_type);
    packet.answers.push(MdnsRecord {
        name: ptr_name,
        rtype: RecordType::PTR,
        rclass: RecordClass::IN,
        ttl: config.ttl,
        data: RecordData::Ptr(service_instance.to_string()),
    });

    // SRV 记录
    let hostname = config.hostname.as_ref()
        .map(|h| format!("{}.local", h))
        .unwrap_or_else(|| service_instance.split('.').next().unwrap_or("localhost").to_string() + ".local");

    packet.additionals.push(MdnsRecord {
        name: service_instance.to_string(),
        rtype: RecordType::SRV,
        rclass: RecordClass::IN,
        ttl: config.ttl,
        data: RecordData::Srv {
            priority: 0,
            weight: 0,
            port: config.port,
            target: hostname.clone(),
        },
    });

    // TXT 记录
    let txt_strings: Vec<String> = txt.iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();
    drop(txt);

    if !txt_strings.is_empty() {
        packet.additionals.push(MdnsRecord {
            name: service_instance.to_string(),
            rtype: RecordType::TXT,
            rclass: RecordClass::IN,
            ttl: config.ttl,
            data: RecordData::Txt(txt_strings),
        });
    }

    // A/AAAA 记录
    for ip in local_ips {
        match ip {
            IpAddr::V4(addr) => {
                packet.additionals.push(MdnsRecord {
                    name: hostname.clone(),
                    rtype: RecordType::A,
                    rclass: RecordClass::IN,
                    ttl: config.ttl,
                    data: RecordData::A(*addr),
                });
            }
            IpAddr::V6(addr) => {
                packet.additionals.push(MdnsRecord {
                    name: hostname.clone(),
                    rtype: RecordType::AAAA,
                    rclass: RecordClass::IN,
                    ttl: config.ttl,
                    data: RecordData::Aaaa(*addr),
                });
            }
        }
    }

    Ok(packet)
}

/// 构建注销数据包（TTL=0）
fn build_goodbye_packet(
    config: &ServiceConfig,
    service_instance: &str,
) -> Result<MdnsPacket> {
    let mut packet = MdnsPacket::response();

    // PTR 记录 (TTL=0 表示注销)
    let ptr_name = format!("{}.local", config.service_type);
    packet.answers.push(MdnsRecord {
        name: ptr_name,
        rtype: RecordType::PTR,
        rclass: RecordClass::IN,
        ttl: 0,  // TTL=0 表示注销
        data: RecordData::Ptr(service_instance.to_string()),
    });

    // SRV 记录 (TTL=0)
    let hostname = config.hostname.as_ref()
        .map(|h| format!("{}.local", h))
        .unwrap_or_else(|| service_instance.split('.').next().unwrap_or("localhost").to_string() + ".local");

    packet.answers.push(MdnsRecord {
        name: service_instance.to_string(),
        rtype: RecordType::SRV,
        rclass: RecordClass::IN,
        ttl: 0,
        data: RecordData::Srv {
            priority: 0,
            weight: 0,
            port: config.port,
            target: hostname,
        },
    });

    Ok(packet)
}

/// 获取本地 IP 地址
fn get_local_ip_addresses(config: &ServiceConfig) -> Vec<IpAddr> {
    let mut ips = Vec::new();

    // 简单实现：使用回环地址作为后备
    // 在实际环境中，可以添加更多网络接口检测逻辑

    // 尝试使用 get_if_addrs 获取接口地址
    // 注意: 0.1.x 版本的 get_if_addrs 返回的 interface.addr 是 ip crate 的 IpAddr
    // 由于类型转换复杂，暂时使用简单的回环地址
    tracing::debug!("Using fallback loopback addresses for service registration");

    ips.push(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    if config.enable_ipv6 {
        ips.push(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)));
    }

    ips
}
