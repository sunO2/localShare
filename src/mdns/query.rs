//! mDNS 查询处理

use crate::common::error::{Error, Result};
use super::packet::{MdnsPacket, MdnsQuestion, RecordType, RecordClass};
use super::socket::MdnsSocket;
use tokio::time::Duration;

/// mDNS 查询类型
#[derive(Debug, Clone, Copy)]
pub enum QueryType {
    /// 单播查询（期望单播响应）
    Unicast,

    /// 组播查询（期望组播响应）
    Multicast,
}

/// mDNS 查询
///
/// 用于构建和发送 mDNS 查询
pub struct MdnsQuery {
    /// 查询类型
    query_type: QueryType,

    /// 问题列表
    questions: Vec<MdnsQuestion>,
}

impl MdnsQuery {
    /// 创建新的查询
    pub fn new(query_type: QueryType) -> Self {
        Self {
            query_type,
            questions: Vec::new(),
        }
    }

    /// 添加问题
    pub fn add_question(&mut self, name: String, rtype: RecordType) -> &mut Self {
        let question = MdnsQuestion {
            name,
            qtype: rtype,
            qclass: RecordClass::IN,
            unicast_response: matches!(self.query_type, QueryType::Unicast),
        };
        self.questions.push(question);
        self
    }

    /// PTR 查询（服务发现）
    pub fn query_ptr(service_type: String) -> Self {
        let mut query = Self::new(QueryType::Multicast);
        query.add_question(service_type, RecordType::PTR);
        query
    }

    /// SRV 查询（服务位置）
    pub fn query_srv(service_name: String) -> Self {
        let mut query = Self::new(QueryType::Multicast);
        query.add_question(service_name, RecordType::SRV);
        query
    }

    /// A 查询（IPv4 地址）
    pub fn query_a(hostname: String) -> Self {
        let mut query = Self::new(QueryType::Multicast);
        query.add_question(hostname, RecordType::A);
        query
    }

    /// AAAA 查询（IPv6 地址）
    pub fn query_aaaa(hostname: String) -> Self {
        let mut query = Self::new(QueryType::Multicast);
        query.add_question(hostname, RecordType::AAAA);
        query
    }

    /// TXT 查询（文本信息）
    pub fn query_txt(service_name: String) -> Self {
        let mut query = Self::new(QueryType::Multicast);
        query.add_question(service_name, RecordType::TXT);
        query
    }

    /// 构建数据包
    pub fn build_packet(&self) -> MdnsPacket {
        let mut packet = MdnsPacket::new();
        packet.set_query();
        packet.questions = self.questions.clone();
        packet
    }

    /// 发送查询
    pub fn send(&self, socket: &MdnsSocket) -> Result<()> {
        let packet = self.build_packet();
        let data = packet.encode()?;

        tracing::debug!("Sending mDNS query: {} questions", self.questions.len());

        // 发送到组播地址
        socket.send_to_v4(&data)?;

        Ok(())
    }

    /// 异步发送查询并等待响应
    pub async fn send_and_recv(&self, socket: &MdnsSocket, timeout_ms: u64) -> Result<MdnsPacket> {
        // 发送查询
        self.send(socket)?;

        // 等待响应 - 使用简单的轮询方式
        let start = std::time::Instant::now();
        let mut buffer = vec![0u8; 4096];

        loop {
            let elapsed = start.elapsed();
            if elapsed >= Duration::from_millis(timeout_ms) {
                return Err(Error::Timeout);
            }

            // 尝试接收（非阻塞超时）
            let recv_result = tokio::task::spawn_blocking({
                // 不能直接移动 socket，所以创建新 socket 用于接收
                let socket_config = super::socket::MdnsSocketConfig {
                    enable_ipv6: true,
                    ..Default::default()
                };
                move || {
                    let recv_socket = MdnsSocket::new(socket_config)?;
                    let mut buf = vec![0u8; 4096];
                    recv_socket.recv_from(&mut buf).map(|(size, addr)| (buf, size, addr))
                }
            }).await;

            match recv_result {
                Ok(Ok((recv_buffer, size, addr))) => {
                    // 解析响应
                    match MdnsPacket::decode(&recv_buffer[..size]) {
                        Ok(packet) if packet.is_response() => {
                            tracing::debug!("Received mDNS response from {}", addr);
                            return Ok(packet);
                        }
                        Ok(_) => {
                            // 忽略查询包，继续等待
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse mDNS packet: {}", e);
                        }
                    }
                }
                Ok(Err(e)) => {
                    tracing::trace!("Socket recv error: {}", e);
                }
                Err(e) => {
                    tracing::trace!("Spawn blocking error: {}", e);
                }
            }

            // 短暂休眠后继续尝试
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// 查询构建器
///
/// 提供流式 API 构建复杂查询
pub struct QueryBuilder {
    query: MdnsQuery,
}

impl QueryBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self {
            query: MdnsQuery::new(QueryType::Multicast),
        }
    }

    /// 设置查询类型
    pub fn query_type(mut self, query_type: QueryType) -> Self {
        self.query.query_type = query_type;
        self
    }

    /// 添加 PTR 问题
    pub fn ptr(mut self, service_type: String) -> Self {
        self.query.add_question(service_type, RecordType::PTR);
        self
    }

    /// 添加 SRV 问题
    pub fn srv(mut self, service_name: String) -> Self {
        self.query.add_question(service_name, RecordType::SRV);
        self
    }

    /// 添加 A 问题
    pub fn a(mut self, hostname: String) -> Self {
        self.query.add_question(hostname, RecordType::A);
        self
    }

    /// 添加 AAAA 问题
    pub fn aaaa(mut self, hostname: String) -> Self {
        self.query.add_question(hostname, RecordType::AAAA);
        self
    }

    /// 添加 TXT 问题
    pub fn txt(mut self, service_name: String) -> Self {
        self.query.add_question(service_name, RecordType::TXT);
        self
    }

    /// 构建查询
    pub fn build(self) -> MdnsQuery {
        self.query
    }
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}
