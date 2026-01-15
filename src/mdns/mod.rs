//! mDNS 协议实现模块
//!
//! 提供 mDNS 的底层协议支持，包括数据包编解码和 socket 通信

pub mod packet;
pub mod socket;
pub mod query;
pub mod response;

pub use socket::{MdnsSocket, MdnsSocketConfig};
pub use packet::{MdnsPacket, MdnsRecord, RecordType, RecordClass};
pub use query::{MdnsQuery, QueryType};
pub use response::{MdnsResponse, ResponseBuilder};

/// mDNS 组播地址
pub const MDNS_IPV4: &str = "224.0.0.251";
pub const MDNS_IPV6: &str = "ff02::fb";

/// mDNS 端口
pub const MDNS_PORT: u16 = 5353;

/// 默认 TTL（秒）
pub const DEFAULT_TTL: u32 = 120;
