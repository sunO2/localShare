//! 设备发现相关的共享数据结构

use std::collections::HashMap;
use std::net::SocketAddr;

/// 服务标识符
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServiceIdentifier {
    /// 服务类型，如 "_shareself._tcp"
    pub service_type: String,
    /// 域名，如 "local"
    pub domain: String,
}

impl ServiceIdentifier {
    /// 创建新的服务标识符
    pub fn new(service_type: String, domain: String) -> Self {
        Self { service_type, domain }
    }

    /// 获取完整的服务名称
    pub fn full_name(&self) -> String {
        format!("{}.{}.", self.service_type, self.domain)
    }
}

impl std::fmt::Display for ServiceIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.service_type, self.domain)
    }
}

/// TXT 记录类型
pub type TxtRecord = HashMap<String, String>;

/// 设备信息
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// 服务名称（实例名）
    pub name: String,

    /// 主机名
    pub hostname: String,

    /// IP 地址列表
    pub addresses: Vec<SocketAddr>,

    /// 服务端口
    pub port: u16,

    /// TXT 记录（元数据）
    pub txt_records: TxtRecord,

    /// 服务类型
    pub service_type: String,

    /// 最后更新时间
    pub last_seen: std::time::Instant,
}

/// 共享文件信息
#[derive(Debug, Clone)]
pub struct SharedFile {
    /// 文件名
    pub name: String,
    /// Info Hash (20字节的十六进制字符串)
    pub info_hash: String,
    /// 文件大小（可选）
    pub size: Option<u64>,
}

impl DeviceInfo {
    /// 创建新的设备信息
    pub fn new(
        name: String,
        hostname: String,
        addresses: Vec<SocketAddr>,
        port: u16,
        txt_records: TxtRecord,
        service_type: String,
    ) -> Self {
        Self {
            name,
            hostname,
            addresses,
            port,
            txt_records,
            service_type,
            last_seen: std::time::Instant::now(),
        }
    }

    /// 获取指定类型的地址
    pub fn get_address(&self, prefer_ipv6: bool) -> Option<&SocketAddr> {
        self.addresses.iter().find(|addr| {
            match addr {
                SocketAddr::V4(_) => !prefer_ipv6,
                SocketAddr::V6(_) => prefer_ipv6,
            }
        })
    }

    /// 从 TXT 记录获取值
    pub fn get_txt_value(&self, key: &str) -> Option<&String> {
        self.txt_records.get(key)
    }

    /// 检查是否过期
    pub fn is_expired(&self, ttl_secs: u64) -> bool {
        self.last_seen.elapsed().as_secs() > ttl_secs
    }

    /// 获取该设备共享的文件列表
    pub fn get_shared_files(&self) -> Vec<SharedFile> {
        let mut files = Vec::new();

        // 从 TXT 记录中查找以 "file_" 开头的记录
        for (key, value) in &self.txt_records {
            if let Some(file_name) = key.strip_prefix("file_") {
                files.push(SharedFile {
                    name: file_name.to_string(),
                    info_hash: value.clone(),
                    size: None, // 大小信息暂时不可用
                });
            }
        }

        files
    }

    /// 获取 BitTorrent 端口（如果有）
    pub fn get_bt_port(&self) -> Option<u16> {
        // 检查是否有 BT 端口信息
        self.txt_records.get("bt_port").and_then(|p| p.parse::<u16>().ok())
            .or(Some(6881)) // 默认端口
    }
}

/// 服务实例（用于内部状态管理）
#[derive(Debug, Clone)]
pub struct ServiceInstance {
    /// 实例名称
    pub instance_name: String,

    /// 服务类型
    pub service_type: String,

    /// 域名
    pub domain: String,

    /// TTL
    pub ttl: u32,
}

impl ServiceInstance {
    /// 获取完整的服务名称
    /// 格式: <instance>.<service>.<domain>
    pub fn full_name(&self) -> String {
        format!("{}.{}.{}", self.instance_name, self.service_type, self.domain)
    }
}
