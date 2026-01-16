//! HTTP 子网扫描模块
//!
//! 扫描本地子网内的设备，尝试 HTTP 连接发现服务

use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, broadcast};
use crate::Result;
use tracing::{debug, info, warn};

/// ShareSelf API 端点
const API_PATH: &str = "/api/info";

/// HTTP 扫描配置
#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// 扫描超时 (秒)
    pub timeout_secs: u64,
    /// 最大并发扫描数
    pub max_concurrent: usize,
    /// 自定义 IP 范围 (可选)
    pub custom_ranges: Vec<String>,
    /// 扫描端口列表
    pub scan_ports: Vec<u16>,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 2,
            max_concurrent: 50,
            custom_ranges: Vec::new(),
            scan_ports: vec![8080, 53317, 3000, 8000, 9000],
        }
    }
}

/// ShareSelf 设备信息响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfoResponse {
    /// 设备名称
    pub name: String,
    /// 设备类型
    #[serde(rename = "type")]
    pub device_type: String,
    /// 版本
    pub version: String,
    /// 设备 ID
    pub id: String,
}

/// HTTP 扫描事件
#[derive(Debug, Clone)]
pub enum ScanEvent {
    /// 发现设备
    DeviceFound {
        name: String,
        address: String,
        port: u16,
        info: DeviceInfoResponse,
    },
    /// 扫描进度
    Progress {
        current: usize,
        total: usize,
    },
    /// 扫描完成
    Complete {
        found_count: usize,
    },
    /// 错误
    Error(String),
}

/// 获取本地网络接口的所有 IP 地址
fn get_local_networks() -> Result<Vec<String>> {
    let mut networks = Vec::new();

    // 尝试获取本地 IP 地址
    match local_ip_address::local_ip() {
        Ok(local_ip) => {
            let ip_str = local_ip.to_string();
            if let Some(dot_pos) = ip_str.rfind('.') {
                let base = &ip_str[..dot_pos + 1];
                // 生成 /24 网段范围
                for i in 1..254 {
                    networks.push(format!("{}{}", base, i));
                }
            }
        }
        Err(e) => {
            warn!("Failed to get local IP: {}", e);
            // 回退到常见的私有网络段
            networks.extend_from_slice(&[
                "192.168.1.1".to_string(),
                "192.168.0.1".to_string(),
                "10.0.0.1".to_string(),
                "172.16.0.1".to_string(),
            ]);
        }
    }

    Ok(networks)
}

/// 生成网络段的 IP 列表
fn generate_network_range(network: &str) -> Vec<String> {
    // 解析 CIDR 或 IP 范围
    if network.contains('/') {
        // CIDR 格式 (例如 192.168.1.0/24)
        if let Some(slash_pos) = network.find('/') {
            let base = &network[..slash_pos];
            if let Some(dot_pos) = base.rfind('.') {
                let prefix = &base[..dot_pos + 1];
                let mut ips = Vec::new();
                for i in 1..254 {
                    ips.push(format!("{}{}", prefix, i));
                }
                return ips;
            }
        }
    }

    // 单个 IP
    vec![network.to_string()]
}

/// 检查单个 IP:端口 是否运行 ShareSelf 服务
fn check_host(address: String, port: u16, timeout_secs: u64) -> Option<DeviceInfoResponse> {
    let url = format!("http://{}:{}{}", address, port, API_PATH);

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout_secs))
        .build();

    match agent.get(&url).call() {
        Ok(response) => {
            if response.status() == 200 {
                if let Ok(text) = response.into_string() {
                    if let Ok(info) = serde_json::from_str::<DeviceInfoResponse>(&text) {
                        return Some(info);
                    }
                }
            }
        }
        Err(e) => {
            // 忽略连接失败，这是预期的
            debug!("No service at {}: {} - {}", url, e.kind(), e);
        }
    }

    None
}

/// HTTP 扫描句柄
pub struct ScanHandle {
    _shutdown_tx: broadcast::Sender<()>,
}

impl ScanHandle {
    /// 停止扫描
    pub async fn shutdown(self) -> Result<()> {
        drop(self._shutdown_tx);
        Ok(())
    }
}

/// 启动 HTTP 子网扫描
pub fn start_subnet_scan(
    config: ScanConfig,
) -> Result<(mpsc::Receiver<ScanEvent>, ScanHandle)> {
    let (event_tx, event_rx) = mpsc::channel(100);
    let (shutdown_tx, _) = broadcast::channel(1);

    // 获取扫描目标列表
    let mut targets = Vec::new();

    if config.custom_ranges.is_empty() {
        // 使用本地网络
        if let Ok(networks) = get_local_networks() {
            for ip in networks {
                for &port in &config.scan_ports {
                    targets.push((ip.clone(), port));
                }
            }
        }
    } else {
        // 使用自定义范围
        for range in &config.custom_ranges {
            for ip in generate_network_range(range) {
                for &port in &config.scan_ports {
                    targets.push((ip.clone(), port));
                }
            }
        }
    }

    let total_targets = targets.len();
    info!("Starting subnet scan: {} targets", total_targets);

    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_concurrent));
        let mut tasks = Vec::new();
        let mut shutdown_rx = shutdown_tx_clone.subscribe();
        let found_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let event_tx = event_tx.clone();

        for (address, port) in targets {
            let _event_tx = event_tx.clone();
            let semaphore_clone = semaphore.clone();
            let timeout = config.timeout_secs;
            let addr = address.clone();
            let shutdown_signal = shutdown_tx_clone.clone();

            let task = tokio::spawn(async move {
                // 限制并发
                let _permit = semaphore_clone.acquire().await.unwrap();

                // 检查是否需要停止
                let mut rx = shutdown_signal.subscribe();
                if rx.try_recv().is_ok() {
                    return None;
                }

                // 在线程池中执行同步 HTTP 请求
                let result = tokio::task::spawn_blocking(move || {
                    check_host(addr, port, timeout)
                }).await;

                match result {
                    Ok(Some(info)) => Some((address, port, info)),
                    _ => None,
                }
            });

            tasks.push(task);
        }

        // 收集结果
        for (index, task) in tasks.into_iter().enumerate() {
            // 发送进度
            let _ = event_tx.send(ScanEvent::Progress {
                current: index + 1,
                total: total_targets,
            }).await;

            // 检查是否需要停止
            if shutdown_rx.try_recv().is_ok() {
                break;
            }

            // 等待任务完成
            if let Ok(Some((address, port, info))) = task.await {
                found_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                info!("Found device: {} at {}:{}", info.name, address, port);

                let _ = event_tx.send(ScanEvent::DeviceFound {
                    name: info.name.clone(),
                    address,
                    port,
                    info,
                }).await;
            }
        }

        // 发送完成事件
        let _ = event_tx.send(ScanEvent::Complete {
            found_count: found_count.load(std::sync::atomic::Ordering::Relaxed),
        }).await;
    });

    let handle = ScanHandle {
        _shutdown_tx: shutdown_tx,
    };

    Ok((event_rx, handle))
}

/// 快速扫描单个主机
pub fn quick_scan_host(address: String, port: u16) -> Option<DeviceInfoResponse> {
    check_host(address, port, 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_range_generation() {
        let ips = generate_network_range("192.168.1.0/24");
        assert_eq!(ips.len(), 253);
        assert_eq!(ips[0], "192.168.1.1");
        assert_eq!(ips[252], "192.168.1.253");
    }

    #[test]
    fn test_single_ip() {
        let ips = generate_network_range("192.168.1.100");
        assert_eq!(ips.len(), 1);
        assert_eq!(ips[0], "192.168.1.100");
    }

    #[test]
    fn test_device_info_deserialization() {
        let json = r#"{"name":"Test Device","type":"sharSelf","version":"0.1.0","id":"abc123"}"#;
        let info: DeviceInfoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(info.name, "Test Device");
        assert_eq!(info.device_type, "sharSelf");
    }
}
