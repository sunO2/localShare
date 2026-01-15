//! mDNS 数据包编解码
//!
//! 参考 RFC 1035: https://tools.ietf.org/html/rfc1035

use crate::common::error::{Error, Result};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::collections::HashMap;

/// DNS 压缩指针掩码 (11xxxxxx xxxxxxxx)
const COMPRESS_POINTER_MASK: u16 = 0xC000;

/// mDNS 数据包
#[derive(Debug, Clone, Default)]
pub struct MdnsPacket {
    /// 事务 ID (mDNS 中通常为 0)
    pub id: u16,

    /// 标志位
    pub flags: u16,

    /// 查询记录数量
    pub questions: Vec<MdnsQuestion>,

    /// 回答记录
    pub answers: Vec<MdnsRecord>,

    /// 权威记录
    pub authorities: Vec<MdnsRecord>,

    /// 附加记录
    pub additionals: Vec<MdnsRecord>,
}

impl MdnsPacket {
    /// 创建新的数据包
    pub fn new() -> Self {
        Self::default()
    }

    /// 创建新的查询数据包
    pub fn query() -> Self {
        Self {
            id: 0,  // mDNS 查询 ID 总是 0
            flags: 0,  // QR=0 表示查询
            questions: Vec::new(),
            answers: Vec::new(),
            authorities: Vec::new(),
            additionals: Vec::new(),
        }
    }

    /// 创建新的响应数据包
    pub fn response() -> Self {
        Self {
            id: 0,
            flags: 0x8400,  // QR=1, AA=1 (响应且是权威回答)
            questions: Vec::new(),
            answers: Vec::new(),
            authorities: Vec::new(),
            additionals: Vec::new(),
        }
    }

    /// 是否为响应包
    pub fn is_response(&self) -> bool {
        self.flags & 0x8000 != 0
    }

    /// 设置为查询包
    pub fn set_query(&mut self) {
        self.flags &= 0x7FFF;
    }

    /// 设置为响应包
    pub fn set_response(&mut self) {
        self.flags |= 0x8000;
    }

    /// 编码为字节
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        let mut compressor = NameCompressor::new();

        // 编码头部 (12 字节)
        buffer.extend_from_slice(&self.id.to_be_bytes());
        buffer.extend_from_slice(&self.flags.to_be_bytes());
        buffer.extend_from_slice(&(self.questions.len() as u16).to_be_bytes());
        buffer.extend_from_slice(&(self.answers.len() as u16).to_be_bytes());
        buffer.extend_from_slice(&(self.authorities.len() as u16).to_be_bytes());
        buffer.extend_from_slice(&(self.additionals.len() as u16).to_be_bytes());

        // 编码问题
        for question in &self.questions {
            encode_name(&question.name, &mut buffer, &mut compressor)?;
            buffer.extend_from_slice(&question.qtype.to_u16().to_be_bytes());
            buffer.extend_from_slice(&question.qclass.to_u16().to_be_bytes());
        }

        // 编码资源记录
        for record in self.answers.iter().chain(self.authorities.iter()).chain(self.additionals.iter()) {
            encode_name(&record.name, &mut buffer, &mut compressor)?;
            buffer.extend_from_slice(&record.rtype.to_u16().to_be_bytes());
            buffer.extend_from_slice(&record.rclass.to_u16().to_be_bytes());
            buffer.extend_from_slice(&record.ttl.to_be_bytes());

            // 预留数据长度位置 (2 字节)
            let rdlength_pos = buffer.len();
            buffer.extend_from_slice(&[0u8, 0u8]);

            // 编码记录数据
            let data_start = buffer.len();
            match &record.data {
                RecordData::A(addr) => {
                    buffer.extend_from_slice(&addr.octets());
                }
                RecordData::Aaaa(addr) => {
                    buffer.extend_from_slice(&addr.octets());
                }
                RecordData::Ptr(name) | RecordData::RawDomain(name) => {
                    encode_name(name, &mut buffer, &mut compressor)?;
                }
                RecordData::Txt(strings) => {
                    for s in strings {
                        if s.len() > 255 {
                            return Err(Error::Mdns("TXT record too long".to_string()));
                        }
                        buffer.push(s.len() as u8);
                        buffer.extend_from_slice(s.as_bytes());
                    }
                }
                RecordData::Srv { priority, weight, port, target } => {
                    buffer.extend_from_slice(&priority.to_be_bytes());
                    buffer.extend_from_slice(&weight.to_be_bytes());
                    buffer.extend_from_slice(&port.to_be_bytes());
                    encode_name(target, &mut buffer, &mut compressor)?;
                }
                RecordData::Raw(data) => {
                    buffer.extend_from_slice(data);
                }
            }

            // 回填数据长度
            let data_len = buffer.len() - data_start;
            let rdlength = (data_len as u16).to_be_bytes();
            buffer[rdlength_pos] = rdlength[0];
            buffer[rdlength_pos + 1] = rdlength[1];
        }

        Ok(buffer)
    }

    /// 从字节解码
    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < 12 {
            return Err(Error::Mdns("Packet too short".to_string()));
        }

        let mut cursor = 0;

        // 解码头部
        let id = u16::from_be_bytes([data[cursor], data[cursor + 1]]);
        cursor += 2;
        let flags = u16::from_be_bytes([data[cursor], data[cursor + 1]]);
        cursor += 2;
        let qdcount = u16::from_be_bytes([data[cursor], data[cursor + 1]]) as usize;
        cursor += 2;
        let ancount = u16::from_be_bytes([data[cursor], data[cursor + 1]]) as usize;
        cursor += 2;
        let nscount = u16::from_be_bytes([data[cursor], data[cursor + 1]]) as usize;
        cursor += 2;
        let arcount = u16::from_be_bytes([data[cursor], data[cursor + 1]]) as usize;
        cursor += 2;

        let mut packet = Self {
            id,
            flags,
            questions: Vec::new(),
            answers: Vec::new(),
            authorities: Vec::new(),
            additionals: Vec::new(),
        };

        // 解码问题
        for _ in 0..qdcount {
            let name = decode_name(data, &mut cursor)?;
            let qtype = RecordType::from_u16(u16::from_be_bytes([data[cursor], data[cursor + 1]]))
                .ok_or_else(|| Error::Mdns("Invalid question type".to_string()))?;
            cursor += 2;
            let qclass = RecordClass::from_u16(u16::from_be_bytes([data[cursor], data[cursor + 1]]))
                .ok_or_else(|| Error::Mdns("Invalid question class".to_string()))?;
            cursor += 2;

            packet.questions.push(MdnsQuestion {
                name,
                qtype,
                qclass,
                unicast_response: false,
            });
        }

        // 解码资源记录
        let decode_records = |count: usize, cursor: &mut usize| -> Result<Vec<MdnsRecord>> {
            let mut records = Vec::new();
            for _ in 0..count {
                let name = decode_name(data, cursor)?;
                let rtype = RecordType::from_u16(u16::from_be_bytes([data[*cursor], data[*cursor + 1]]))
                    .ok_or_else(|| Error::Mdns("Invalid record type".to_string()))?;
                *cursor += 2;
                let rclass = RecordClass::from_u16(u16::from_be_bytes([data[*cursor], data[*cursor + 1]]))
                    .ok_or_else(|| Error::Mdns("Invalid record class".to_string()))?;
                *cursor += 2;
                let ttl = u32::from_be_bytes([
                    data[*cursor], data[*cursor + 1], data[*cursor + 2], data[*cursor + 3],
                ]);
                *cursor += 4;
                let rdlength = u16::from_be_bytes([data[*cursor], data[*cursor + 1]]) as usize;
                *cursor += 2;

                let data_start = *cursor;
                let record_data = match rtype {
                    RecordType::A => {
                        if rdlength != 4 {
                            return Err(Error::Mdns("Invalid A record length".to_string()));
                        }
                        let addr = Ipv4Addr::new(data[*cursor], data[*cursor + 1], data[*cursor + 2], data[*cursor + 3]);
                        *cursor += 4;
                        RecordData::A(addr)
                    }
                    RecordType::AAAA => {
                        if rdlength != 16 {
                            return Err(Error::Mdns("Invalid AAAA record length".to_string()));
                        }
                        let octets: [u8; 16] = data[*cursor..*cursor + 16].try_into()
                            .map_err(|_| Error::Mdns("Invalid AAAA record".to_string()))?;
                        *cursor += 16;
                        RecordData::Aaaa(Ipv6Addr::from(octets))
                    }
                    RecordType::PTR => {
                        let name = decode_name(data, cursor)?;
                        RecordData::Ptr(name)
                    }
                    RecordType::TXT => {
                        let mut strings = Vec::new();
                        while *cursor < data_start + rdlength {
                            let len = data[*cursor] as usize;
                            *cursor += 1;
                            if *cursor + len > data.len() {
                                return Err(Error::Mdns("Invalid TXT record".to_string()));
                            }
                            let s = String::from_utf8(data[*cursor..*cursor + len].to_vec())
                                .map_err(|_| Error::Mdns("Invalid TXT string".to_string()))?;
                            strings.push(s);
                            *cursor += len;
                        }
                        RecordData::Txt(strings)
                    }
                    RecordType::SRV => {
                        let priority = u16::from_be_bytes([data[*cursor], data[*cursor + 1]]);
                        *cursor += 2;
                        let weight = u16::from_be_bytes([data[*cursor], data[*cursor + 1]]);
                        *cursor += 2;
                        let port = u16::from_be_bytes([data[*cursor], data[*cursor + 1]]);
                        *cursor += 2;
                        let target = decode_name(data, cursor)?;
                        RecordData::Srv { priority, weight, port, target }
                    }
                    _ => {
                        // 跳过未知类型
                        *cursor += rdlength;
                        RecordData::Raw(data[data_start..data_start + rdlength].to_vec())
                    }
                };

                records.push(MdnsRecord {
                    name,
                    rtype,
                    rclass,
                    ttl,
                    data: record_data,
                });
            }
            Ok(records)
        };

        packet.answers = decode_records(ancount, &mut cursor)?;
        packet.authorities = decode_records(nscount, &mut cursor)?;
        packet.additionals = decode_records(arcount, &mut cursor)?;

        Ok(packet)
    }

    /// 获取数据包大小
    pub fn size(&self) -> usize {
        self.encode().map(|b| b.len()).unwrap_or(0)
    }
}

/// mDNS 查询问题
#[derive(Debug, Clone)]
pub struct MdnsQuestion {
    /// 查询名称
    pub name: String,

    /// 记录类型
    pub qtype: RecordType,

    /// 记录类
    pub qclass: RecordClass,

    /// 单播响应标志
    pub unicast_response: bool,
}

/// DNS 记录类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordType {
    /// A 记录 (IPv4 地址)
    A = 1,

    /// NS 记录 (名称服务器)
    NS = 2,

    /// CNAME 记录 (别名)
    CNAME = 5,

    /// SOA 记录 (区域授权)
    SOA = 6,

    /// PTR 记录 (指针)
    PTR = 12,

    /// TXT 记录 (文本)
    TXT = 16,

    /// AAAA 记录 (IPv6 地址)
    AAAA = 28,

    /// SRV 记录 (服务)
    SRV = 33,

    /// ANY 记录
    ANY = 255,
}

impl RecordType {
    /// 从数值解析
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::A),
            2 => Some(Self::NS),
            5 => Some(Self::CNAME),
            6 => Some(Self::SOA),
            12 => Some(Self::PTR),
            16 => Some(Self::TXT),
            28 => Some(Self::AAAA),
            33 => Some(Self::SRV),
            255 => Some(Self::ANY),
            _ => None,
        }
    }

    /// 转换为数值
    pub fn to_u16(self) -> u16 {
        self as u16
    }
}

/// DNS 记录类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordClass {
    /// IN (Internet)
    IN = 1,

    /// ANY (任何类)
    ANY = 255,
}

impl RecordClass {
    /// 从数值解析
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::IN),
            255 => Some(Self::ANY),
            _ => None,
        }
    }

    /// 转换为数值
    pub fn to_u16(self) -> u16 {
        self as u16
    }
}

/// mDNS 资源记录
#[derive(Debug, Clone)]
pub struct MdnsRecord {
    /// 记录名称
    pub name: String,

    /// 记录类型
    pub rtype: RecordType,

    /// 记录类
    pub rclass: RecordClass,

    /// TTL (秒)
    pub ttl: u32,

    /// 记录数据
    pub data: RecordData,
}

/// DNS 记录数据
#[derive(Debug, Clone)]
pub enum RecordData {
    /// A 记录数据
    A(Ipv4Addr),

    /// AAAA 记录数据
    Aaaa(Ipv6Addr),

    /// PTR 记录数据
    Ptr(String),

    /// 原始域名数据
    RawDomain(String),

    /// TXT 记录数据
    Txt(Vec<String>),

    /// SRV 记录数据
    Srv {
        priority: u16,
        weight: u16,
        port: u16,
        target: String,
    },

    /// 原始数据
    Raw(Vec<u8>),
}

impl RecordData {
    /// 获取字节数据
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            RecordData::Raw(data) => Some(data),
            _ => None,
        }
    }

    /// 获取域名
    pub fn as_domain(&self) -> Option<&str> {
        match self {
            RecordData::Ptr(name) | RecordData::RawDomain(name) => Some(name),
            RecordData::Srv { target, .. } => Some(target),
            _ => None,
        }
    }
}

/// 编码 DNS 名称 (带压缩)
fn encode_name(name: &str, buffer: &mut Vec<u8>, compressor: &mut NameCompressor) -> Result<()> {
    compressor.encode_name(name, buffer)
}

/// 解码 DNS 名称 (处理压缩指针)
fn decode_name(data: &[u8], cursor: &mut usize) -> Result<String> {
    let mut labels = Vec::new();
    let mut original_cursor = *cursor;
    let mut jumped = false;

    loop {
        if *cursor >= data.len() {
            return Err(Error::Mdns("Unexpected end of name".to_string()));
        }

        let byte = data[*cursor];

        // 检查是否为压缩指针
        if byte & 0xC0 == 0xC0 {
            if *cursor + 1 >= data.len() {
                return Err(Error::Mdns("Incomplete compression pointer".to_string()));
            }

            let pointer = u16::from_be_bytes([byte, data[*cursor + 1]]) & 0x3FFF;
            *cursor += 2;

            if !jumped {
                jumped = true;
                original_cursor = *cursor;
            }

            *cursor = pointer as usize;
            continue;
        }

        // 长度为 0 表示名称结束
        if byte == 0 {
            *cursor += 1;
            break;
        }

        // 读取标签
        let len = byte as usize;
        *cursor += 1;

        if *cursor + len > data.len() {
            return Err(Error::Mdns("Incomplete label".to_string()));
        }

        let label = String::from_utf8(data[*cursor..*cursor + len].to_vec())
            .map_err(|_| Error::Mdns("Invalid label encoding".to_string()))?;
        labels.push(label);
        *cursor += len;
    }

    if jumped {
        *cursor = original_cursor;
    }

    Ok(labels.join("."))
}

/// 名称压缩辅助结构
///
/// mDNS/DNS 使用名称压缩来减少数据包大小
pub struct NameCompressor {
    /// 名称位置映射
    positions: HashMap<String, usize>,
}

impl NameCompressor {
    /// 创建新的压缩器
    pub fn new() -> Self {
        Self {
            positions: HashMap::new(),
        }
    }

    /// 编码名称（可能使用压缩指针）
    pub fn encode_name(&mut self, name: &str, buffer: &mut Vec<u8>) -> Result<()> {
        // 检查是否可以使用压缩指针
        if let Some(&pos) = self.positions.get(name) {
            let pointer = ((pos >> 8) as u8 | 0xC0);
            buffer.push(pointer);
            buffer.push((pos & 0xFF) as u8);
            return Ok(());
        }

        // 记录当前位置
        let start_pos = buffer.len();

        // 编码每个标签
        for label in name.split('.') {
            if label.is_empty() {
                continue;
            }

            if label.len() > 63 {
                return Err(Error::Mdns("Label too long".to_string()));
            }

            buffer.push(label.len() as u8);
            buffer.extend_from_slice(label.as_bytes());
        }

        // 结束标记
        buffer.push(0);

        // 记录压缩位置（只记录完整名称）
        if name.ends_with('.') {
            self.positions.insert(name.to_string(), start_pos);
        } else {
            self.positions.insert(format!("{}.", name), start_pos);
        }

        Ok(())
    }

    /// 解码名称（处理压缩指针）
    #[allow(dead_code)]
    pub fn decode_name(&self, buffer: &[u8], mut offset: usize) -> Result<(String, usize)> {
        let name = decode_name(buffer, &mut offset)?;
        Ok((name, offset))
    }
}
