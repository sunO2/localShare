//! 配置管理

use std::collections::HashMap;
use std::net::SocketAddr;

/// 设备发现配置
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// 服务类型，如 "_shareself._tcp.local"
    pub service_type: String,

    /// 搜索域名，默认 "local"
    pub domain: String,

    /// 是否主动广播（发送 PTR 查询）
    pub active_browse: bool,

    /// 发现超时（秒），None 表示持续监听
    pub timeout_secs: Option<u64>,

    /// 是否监听 IPv6
    pub enable_ipv6: bool,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            service_type: crate::DEFAULT_SERVICE_TYPE.to_string(),
            domain: "local".to_string(),
            active_browse: true,
            timeout_secs: None,
            enable_ipv6: true,
        }
    }
}

/// 服务注册配置
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// 服务名称（设备名称）
    pub service_name: String,

    /// 服务类型
    pub service_type: String,

    /// 监听端口
    pub port: u16,

    /// TXT 记录（额外信息）
    pub txt_records: HashMap<String, String>,

    /// 域名，默认 "local"
    pub domain: String,

    /// 服务主机名，默认使用系统主机名
    pub hostname: Option<String>,

    /// TTL（生存时间，秒）
    pub ttl: u32,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            service_name: "SharSelf Device".to_string(),
            service_type: crate::DEFAULT_SERVICE_TYPE.to_string(),
            port: 8080,
            txt_records: HashMap::new(),
            domain: "local".to_string(),
            hostname: None,
            ttl: 120,
        }
    }
}

impl ServiceConfig {
    /// 创建新的服务配置
    pub fn new(service_name: String, port: u16) -> Self {
        Self {
            service_name,
            port,
            ..Default::default()
        }
    }

    /// 添加 TXT 记录
    pub fn with_txt_record(mut self, key: String, value: String) -> Self {
        self.txt_records.insert(key, value);
        self
    }

    /// 设置主机名
    pub fn with_hostname(mut self, hostname: String) -> Self {
        self.hostname = Some(hostname);
        self
    }

    /// 设置 TTL
    pub fn with_ttl(mut self, ttl: u32) -> Self {
        self.ttl = ttl;
        self
    }
}
