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

        // 尝试绑定到 mDNS 端口 (5353)
        // 在 Android/Termux 上可能需要特殊权限，如果失败则绑定到随机端口
        let bind_result = socket.bind(&SockAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MDNS_PORT)));

        #[cfg(target_os = "android")]
        if bind_result.is_err() {
            tracing::warn!("Failed to bind to mDNS port 5353, falling back to random port. This may limit discovery functionality.");
            // 绑定到端口 0 让系统选择可用端口
            socket.bind(&SockAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)))?;
        } else {
            bind_result?;
        }

        #[cfg(not(target_os = "android"))]
        bind_result?;

        // 尝试加入组播组（可能在某些平台上失败）
        let mdns_addr: Ipv4Addr = MDNS_IPV4.parse().unwrap();
        if let Err(e) = socket.join_multicast_v4(&mdns_addr, &Ipv4Addr::UNSPECIFIED) {
            tracing::warn!("Failed to join multicast group: {}. Discovery may be limited.", e);
        }

        // 设置组播 TTL
        if let Err(e) = socket.set_multicast_ttl_v4(config.multicast_ttl as u32) {
            tracing::warn!("Failed to set multicast TTL: {}", e);
        }

        // 设置循环回
        if let Err(e) = socket.set_multicast_loop_v4(config.loop_enable) {
            tracing::warn!("Failed to set multicast loop: {}", e);
        }

        Ok(socket)
    }

    /// 创建 IPv6 mDNS socket
    fn create_ipv6_socket(config: &MdnsSocketConfig) -> Result<Socket> {
        let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;

        // 设置 SO_REUSEADDR
        socket.set_reuse_address(true)?;

        // 尝试绑定到 mDNS 端口 (5353)
        let addr = std::net::SocketAddrV6::new(std::net::Ipv6Addr::UNSPECIFIED, MDNS_PORT, 0, 0);
        let bind_result = socket.bind(&SockAddr::from(addr));

        #[cfg(target_os = "android")]
        if bind_result.is_err() {
            tracing::warn!("Failed to bind IPv6 socket to mDNS port 5353, falling back to random port.");
            socket.bind(&SockAddr::from(std::net::SocketAddrV6::new(std::net::Ipv6Addr::UNSPECIFIED, 0, 0, 0)))?;
        } else {
            bind_result?;
        }

        #[cfg(not(target_os = "android"))]
        bind_result?;

        // 尝试加入组播组
        let mdns_addr: std::net::Ipv6Addr = MDNS_IPV6.parse().unwrap();
        if let Err(e) = socket.join_multicast_v6(&mdns_addr, config.interface_index.unwrap_or(0)) {
            tracing::warn!("Failed to join IPv6 multicast group: {}", e);
        }

        // 设置组播 TTL
        if let Err(e) = socket.set_multicast_ttl_v4(config.multicast_ttl as u32) {
            tracing::warn!("Failed to set IPv6 multicast TTL: {}", e);
        }

        // 设置循环回
        if let Err(e) = socket.set_multicast_loop_v6(config.loop_enable) {
            tracing::warn!("Failed to set IPv6 multicast loop: {}", e);
        }

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
        match self.send_to(data, &addr) {
            Ok(n) => Ok(n),
            Err(e) => {
                // 权限错误转换为警告，不影响程序运行
                let error_msg = e.to_string();
                if error_msg.contains("Operation not permitted") || error_msg.contains("Permission denied") {
                    tracing::warn!("Permission denied when sending mDNS packet. This is expected on unrooted Android/Termux.");
                    Ok(data.len()) // 假装发送成功，避免阻塞程序
                } else {
                    Err(e)
                }
            }
        }
    }

    /// 发送到 IPv6 组播地址
    pub fn send_to_v6(&self, data: &[u8]) -> Result<usize> {
        let addr: SocketAddr = format!("[{}]:{}", MDNS_IPV6, MDNS_PORT).parse().unwrap();
        match self.send_to(data, &addr) {
            Ok(n) => Ok(n),
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("Operation not permitted") || error_msg.contains("Permission denied") {
                    tracing::warn!("Permission denied when sending IPv6 mDNS packet.");
                    Ok(data.len())
                } else {
                    Err(e)
                }
            }
        }
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
