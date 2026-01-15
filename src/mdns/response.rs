//! mDNS 响应处理

use crate::common::error::{Error, Result};
use super::packet::{MdnsPacket, MdnsRecord, RecordData, RecordType};

/// mDNS 响应
///
/// 表示接收到的 mDNS 响应
#[derive(Debug, Clone)]
pub struct MdnsResponse {
    /// 源地址
    pub source: std::net::SocketAddr,

    /// 响应记录
    pub records: Vec<MdnsRecord>,
}

impl MdnsResponse {
    /// 创建新的响应
    pub fn new(source: std::net::SocketAddr) -> Self {
        Self {
            source,
            records: Vec::new(),
        }
    }

    /// 从数据包解析
    pub fn from_packet(source: std::net::SocketAddr, packet: MdnsPacket) -> Self {
        let mut response = Self::new(source);
        response.records.extend(packet.answers);
        response.records.extend(packet.additionals);
        response
    }

    /// 获取指定类型的记录
    pub fn get_records(&self, rtype: RecordType) -> Vec<&MdnsRecord> {
        self.records
            .iter()
            .filter(|r| r.rtype == rtype)
            .collect()
    }

    /// 获取指定名称的记录
    pub fn get_records_by_name(&self, name: &str) -> Vec<&MdnsRecord> {
        self.records
            .iter()
            .filter(|r| r.name == name)
            .collect()
    }

    /// 获取 PTR 记录的服务名称列表
    pub fn get_service_names(&self) -> Vec<String> {
        self.get_records(RecordType::PTR)
            .iter()
            .filter_map(|r| {
                if let RecordData::Ptr(name) = &r.data {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// 获取 SRV 记录的目标和端口
    pub fn get_srv_target(&self) -> Vec<(String, u16)> {
        self.get_records(RecordType::SRV)
            .iter()
            .filter_map(|r| {
                if let RecordData::Srv { target, port, .. } = &r.data {
                    Some((target.clone(), *port))
                } else {
                    None
                }
            })
            .collect()
    }
}

/// 响应构建器
///
/// 用于构建 mDNS 响应数据包
pub struct ResponseBuilder {
    packet: MdnsPacket,
}

impl ResponseBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        let mut packet = MdnsPacket::new();
        packet.set_response();
        Self { packet }
    }

    /// 添加 PTR 记录
    pub fn add_ptr(
        mut self,
        name: String,
        ptr_domain: String,
        ttl: u32,
    ) -> Self {
        let record = MdnsRecord {
            name,
            rtype: RecordType::PTR,
            rclass: super::packet::RecordClass::IN,
            ttl,
            data: RecordData::Ptr(ptr_domain),
        };
        self.packet.answers.push(record);
        self
    }

    /// 添加 SRV 记录
    pub fn add_srv(
        mut self,
        name: String,
        priority: u16,
        weight: u16,
        port: u16,
        target: String,
        ttl: u32,
    ) -> Self {
        let record = MdnsRecord {
            name,
            rtype: RecordType::SRV,
            rclass: super::packet::RecordClass::IN,
            ttl,
            data: RecordData::Srv {
                priority,
                weight,
                port,
                target,
            },
        };
        self.packet.answers.push(record);
        self
    }

    /// 添加 A 记录
    pub fn add_a(
        mut self,
        name: String,
        addr: std::net::Ipv4Addr,
        ttl: u32,
    ) -> Self {
        let record = MdnsRecord {
            name,
            rtype: RecordType::A,
            rclass: super::packet::RecordClass::IN,
            ttl,
            data: RecordData::A(addr),
        };
        self.packet.answers.push(record);
        self
    }

    /// 添加 AAAA 记录
    pub fn add_aaaa(
        mut self,
        name: String,
        addr: std::net::Ipv6Addr,
        ttl: u32,
    ) -> Self {
        let record = MdnsRecord {
            name,
            rtype: RecordType::AAAA,
            rclass: super::packet::RecordClass::IN,
            ttl,
            data: RecordData::Aaaa(addr),
        };
        self.packet.answers.push(record);
        self
    }

    /// 添加 TXT 记录
    pub fn add_txt(
        mut self,
        name: String,
        txt_data: Vec<String>,
        ttl: u32,
    ) -> Self {
        let record = MdnsRecord {
            name,
            rtype: RecordType::TXT,
            rclass: super::packet::RecordClass::IN,
            ttl,
            data: RecordData::Txt(txt_data),
        };
        self.packet.answers.push(record);
        self
    }

    /// 构建数据包
    pub fn build(self) -> MdnsPacket {
        self.packet
    }

    /// 编码为字节
    pub fn encode(self) -> Result<Vec<u8>> {
        self.packet.encode()
    }
}

impl Default for ResponseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 服务宣告构建器
///
/// 专门用于构建 DNS-SD 服务宣告
pub struct AnnouncementBuilder {
    inner: ResponseBuilder,
}

impl AnnouncementBuilder {
    /// 创建新的服务宣告
    pub fn new() -> Self {
        Self {
            inner: ResponseBuilder::new(),
        }
    }

    /// 添加服务信息
    ///
    /// # Arguments
    ///
    /// * `instance_name` - 服务实例名称（如 "My Device"）
    /// * `service_type` - 服务类型（如 "_shareself._tcp"）
    /// * `domain` - 域名（如 "local"）
    /// * `hostname` - 主机名
    /// * `port` - 服务端口
    /// * `txt_records` - TXT 记录
    /// * `addresses` - IP 地址列表
    /// * `ttl` - TTL
    pub fn add_service(
        mut self,
        instance_name: String,
        service_type: String,
        domain: String,
        hostname: String,
        port: u16,
        txt_records: Vec<String>,
        addresses: Vec<(std::net::IpAddr, u32)>,
        ttl: u32,
    ) -> Result<Self> {
        // 构建各种名称
        let ptr_name = format!("{}.{}.", service_type, domain);
        let srv_name = format!("{}.{}.{}.", instance_name, service_type, domain);

        // 添加 PTR 记录
        self.inner = self.inner.add_ptr(ptr_name, srv_name.clone(), ttl);

        // 添加 SRV 记录
        self.inner = self.inner.add_srv(
            srv_name.clone(),
            0,  // priority
            0,  // weight
            port,
            format!("{}.", hostname),
            ttl,
        );

        // 添加 TXT 记录
        self.inner = self.inner.add_txt(srv_name.clone(), txt_records, ttl);

        // 添加 A/AAAA 记录
        for (addr, addr_ttl) in addresses {
            let hostname = format!("{}.", hostname);
            match addr {
                std::net::IpAddr::V4(v4) => {
                    self.inner = self.inner.add_a(hostname, v4, addr_ttl);
                }
                std::net::IpAddr::V6(v6) => {
                    self.inner = self.inner.add_aaaa(hostname, v6, addr_ttl);
                }
            }
        }

        Ok(self)
    }

    /// 构建并编码
    pub fn build(self) -> Result<Vec<u8>> {
        self.inner.encode()
    }
}

impl Default for AnnouncementBuilder {
    fn default() -> Self {
        Self::new()
    }
}
