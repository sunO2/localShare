//! mDNS Socket 封装（跨平台）
//!
//! 提供跨平台的 mDNS socket 支持

use crate::common::error::{Error, Result};
use crate::mdns::{MDNS_IPV4, MDNS_IPV6, MDNS_PORT};
use socket2::{Socket, Domain, Protocol, Type, SockAddr};
use std::net::{SocketAddr, SocketAddrV4, Ipv4Addr};

/// mDNS Socket 配置
#[derive(Debug, Clone)]
pub struct MdnsSocketConfig {
    /// 是否启用 IPv6
    pub enable_ipv6: bool,

    /// 绑定地址（None 表示使用默认组播地址）
    pub bind_addr: Option<SocketAddr>,

    /// 接口索引（用于多网卡场景）
    pub interface_index: Option<u32>,

    /// TTL (组播跳数)
    pub multicast_ttl: u32,

    /// 是否启用循环回（接收自己发送的数据包）
    pub loop_enable: bool,
}

impl Default for MdnsSocketConfig {
    fn default() -> Self {
        Self {
            enable_ipv6: true,
            bind_addr: None,
            interface_index: None,
            multicast_ttl: 255,  // mDNS 标准值
            loop_enable: true,
        }
    }
}

/// mDNS Socket
///
/// 封装用于 mDNS 通信的 UDP socket
pub struct MdnsSocket {
    /// IPv4 socket
    socket_v4: Option<Socket>,

    /// IPv6 socket
    socket_v6: Option<Socket>,
}

impl MdnsSocket {
    /// 创建新的 mDNS socket
    pub fn new(config: MdnsSocketConfig) -> Result<Self> {
        let socket_v4 = Self::create_ipv4_socket(&config)?;
        let socket_v6 = if config.enable_ipv6 {
            Some(Self::create_ipv6_socket(&config)?)
        } else {
            None
        };

        Ok(Self {
            socket_v4: Some(socket_v4),
            socket_v6,
        })
    }

    /// 创建 IPv4 mDNS socket
    fn create_ipv4_socket(config: &MdnsSocketConfig) -> Result<Socket> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;

        // 设置 SO_REUSEADDR
        socket.set_reuse_address(true)?;

        // SO_REUSEPORT 在某些平台可能不可用
        #[cfg(all(unix, not(target_os = "android")))]
        {
            let _ = socket.set_reuse_port(true);
        }

        // 绑定到 mDNS 端口
        let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MDNS_PORT);
        socket.bind(&SockAddr::from(addr))?;

        // 加入组播组
        let mdns_addr: Ipv4Addr = MDNS_IPV4.parse().unwrap();
        socket.join_multicast_v4(&mdns_addr, &Ipv4Addr::UNSPECIFIED)?;

        // 设置组播 TTL
        socket.set_multicast_ttl_v4(config.multicast_ttl as u32)?;

        // 设置循环回
        socket.set_multicast_loop_v4(config.loop_enable)?;

        Ok(socket)
    }

    /// 创建 IPv6 mDNS socket
    fn create_ipv6_socket(config: &MdnsSocketConfig) -> Result<Socket> {
        let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;

        // 设置 SO_REUSEADDR
        socket.set_reuse_address(true)?;

        // SO_REUSEPORT 在某些平台可能不可用
        #[cfg(all(unix, not(target_os = "android")))]
        {
            let _ = socket.set_reuse_port(true);
        }

        // 绑定到 mDNS 端口
        let addr = std::net::SocketAddrV6::new(std::net::Ipv6Addr::UNSPECIFIED, MDNS_PORT, 0, 0);
        socket.bind(&SockAddr::from(addr))?;

        // 加入组播组
        let mdns_addr: std::net::Ipv6Addr = MDNS_IPV6.parse().unwrap();
        socket.join_multicast_v6(&mdns_addr, config.interface_index.unwrap_or(0))?;

        // 设置组播 TTL (IPv6 使用相同的值)
        socket.set_multicast_ttl_v4(config.multicast_ttl as u32)?;

        // 设置循环回
        socket.set_multicast_loop_v6(config.loop_enable)?;

        Ok(socket)
    }

    /// 发送数据到 mDNS 组播地址
    pub fn send_to(&self, data: &[u8], target: &SocketAddr) -> Result<usize> {
        let sock_addr = SockAddr::from(*target);

        match target {
            SocketAddr::V4(_) => {
                if let Some(socket) = &self.socket_v4 {
                    socket.send_to(data, &sock_addr).map_err(Error::from)
                } else {
                    Err(Error::Other("IPv4 socket not available".to_string()))
                }
            }
            SocketAddr::V6(_) => {
                if let Some(socket) = &self.socket_v6 {
                    socket.send_to(data, &sock_addr).map_err(Error::from)
                } else {
                    Err(Error::Other("IPv6 socket not available".to_string()))
                }
            }
        }
    }

    /// 发送到 IPv4 组播地址
    pub fn send_to_v4(&self, data: &[u8]) -> Result<usize> {
        let addr: SocketAddr = format!("{}:{}", MDNS_IPV4, MDNS_PORT).parse().unwrap();
        self.send_to(data, &addr)
    }

    /// 发送到 IPv6 组播地址
    pub fn send_to_v6(&self, data: &[u8]) -> Result<usize> {
        let addr: SocketAddr = format!("{}:{}", MDNS_IPV6, MDNS_PORT).parse().unwrap();
        self.send_to(data, &addr)
    }

    /// 接收数据
    pub fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        use std::mem::MaybeUninit;

        let mut uninit_buf: Vec<MaybeUninit<u8>> = vec![MaybeUninit::uninit(); buf.len()];

        // 优先使用 IPv4 socket
        if let Some(socket) = &self.socket_v4 {
            match socket.recv_from(&mut uninit_buf) {
                Ok((size, addr)) => {
                    // 转换 SockAddr 到 SocketAddr
                    if let Some(socket_addr) = addr.as_socket() {
                        // 复制数据到输出缓冲区
                        for (i, byte) in uninit_buf.iter().take(size).enumerate() {
                            unsafe {
                                buf[i] = byte.assume_init();
                            }
                        }
                        return Ok((size, socket_addr));
                    }
                }
                Err(e) => return Err(Error::Network(e.to_string())),
            }
        }

        if let Some(socket) = &self.socket_v6 {
            match socket.recv_from(&mut uninit_buf) {
                Ok((size, addr)) => {
                    if let Some(socket_addr) = addr.as_socket() {
                        for (i, byte) in uninit_buf.iter().take(size).enumerate() {
                            unsafe {
                                buf[i] = byte.assume_init();
                            }
                        }
                        return Ok((size, socket_addr));
                    }
                }
                Err(e) => return Err(Error::Network(e.to_string())),
            }
        }

        Err(Error::Other("No socket available".to_string()))
    }
}

/// 获取默认 mDNS 组播地址列表
pub fn get_multicast_addresses() -> Vec<SocketAddr> {
    let mut addrs = Vec::new();

    if let Ok(addr) = format!("{}:{}", MDNS_IPV4, MDNS_PORT).parse() {
        addrs.push(addr);
    }

    if let Ok(addr) = format!("{}:{}", MDNS_IPV6, MDNS_PORT).parse() {
        addrs.push(addr);
    }

    addrs
}
