//! 种子上传器 (Seeder)
//!
//! 作为种子节点，向其他 peer 提供文件数据

use crate::torrent::metainfo::TorrentMetaInfo;
use crate::torrent::piece::PieceManager;
use crate::torrent::protocol::Message;
use crate::torrent::peer::PeerConnection;
use crate::common::error::Result;
use std::net::{SocketAddr, IpAddr};
use std::sync::Arc;
use std::collections::{HashMap, HashSet};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

/// 种子上传器
pub struct Seeder {
    /// Torrent 元信息
    metainfo: TorrentMetaInfo,

    /// Piece 管理器
    piece_manager: Arc<PieceManager>,

    /// 监听地址
    listen_addr: SocketAddr,

    /// 连接的 peers
    peers: Arc<RwLock<HashMap<SocketAddr, PeerConnection>>>,

    /// 是否运行
    running: Arc<RwLock<bool>>,

    /// 允许连接的设备白名单 (IP 地址集合)
    /// None 表示允许所有设备连接
    allowed_devices: Option<HashSet<String>>,
}

impl Seeder {
    /// 创建新的种子
    pub fn new(
        metainfo: TorrentMetaInfo,
        piece_manager: Arc<PieceManager>,
        listen_addr: SocketAddr,
        allowed_devices: Option<HashSet<String>>,
    ) -> Self {
        Seeder {
            metainfo,
            piece_manager,
            listen_addr,
            peers: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(RwLock::new(false)),
            allowed_devices,
        }
    }

    /// 启动种子服务
    pub async fn start(&self) -> Result<()> {
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        let listener = TcpListener::bind(&self.listen_addr)
            .await
            .map_err(|e| crate::common::error::Error::Network(format!("Failed to bind {}: {}", self.listen_addr, e)))?;

        tracing::info!("Seeder listening on {}", self.listen_addr);

        // 生成 peer ID
        let peer_id = Self::generate_peer_id();

        let info_hash = self.metainfo.info_hash()?;
        let piece_manager = Arc::clone(&self.piece_manager);
        let peers = Arc::clone(&self.peers);
        let running = Arc::clone(&self.running);
        let allowed_devices = self.allowed_devices.clone();

        loop {
            // 检查是否应该停止
            {
                let is_running = *running.read().await;
                if !is_running {
                    break;
                }
            }

            // 接受新连接
            match listener.accept().await {
                Ok((socket, addr)) => {
                    let info_hash = info_hash;
                    let peer_id = peer_id;
                    let piece_manager = Arc::clone(&piece_manager);
                    let peers = Arc::clone(&peers);
                    let allowed_devices = allowed_devices.clone();

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_peer(
                            socket,
                            addr,
                            info_hash,
                            peer_id,
                            piece_manager,
                            allowed_devices,
                        ).await {
                            tracing::warn!("Peer {} error: {}", addr, e);
                        }

                        // 移除断开的 peer
                        peers.write().await.remove(&addr);
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to accept connection: {}", e);
                }
            }
        }

        Ok(())
    }

    /// 处理 peer 连接
    async fn handle_peer(
        socket: tokio::net::TcpStream,
        addr: SocketAddr,
        info_hash: [u8; 20],
        peer_id: [u8; 20],
        piece_manager: Arc<PieceManager>,
        allowed_devices: Option<HashSet<String>>,
    ) -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // 验证白名单
        if let Some(allowed) = &allowed_devices {
            let peer_ip = addr.ip().to_string();
            if !allowed.contains(&peer_ip) {
                tracing::warn!("拒绝连接：{} 不在白名单中", addr);
                return Err(crate::common::error::Error::Other(
                    format!("Connection from {} not in whitelist", addr)
                ));
            }
            tracing::info!("接受连接：{} (在白名单中)", addr);
        } else {
            tracing::debug!("Seeder: 接受来自 {} 的连接", addr);
        }

        let (mut reader, mut writer) = socket.into_split();

        // 接收握手
        let mut handshake_bytes = vec![0u8; 68];
        tracing::debug!("Seeder: 等待握手数据...");
        reader.read_exact(&mut handshake_bytes).await
            .map_err(|e| crate::common::error::Error::Network(format!("Failed to read handshake: {}", e)))?;

        tracing::debug!("Seeder: 收到握手数据，解析中...");

        let handshake = crate::torrent::protocol::Handshake::from_bytes(&handshake_bytes)?;

        // 验证 info hash
        if handshake.info_hash != info_hash {
            tracing::warn!("Seeder: Info hash 不匹配");
            return Err(crate::common::error::Error::Other("Info hash mismatch".to_string()));
        }

        tracing::debug!("Seeder: Info hash 匹配，发送响应握手");

        // 发送握手
        let response_handshake = crate::torrent::protocol::Handshake::new(info_hash, peer_id);
        let handshake_data = response_handshake.to_bytes();
        writer.write_all(&handshake_data).await?;
        writer.flush().await?;

        tracing::debug!("Seeder: 握手完成，发送 bitfield");

        // 发送 bitfield
        let bitmap = piece_manager.bitmap().await;
        let bitfield_msg = Message::Bitfield { bitmap };
        let msg_bytes = bitfield_msg.to_bytes();
        writer.write_all(&msg_bytes).await?;
        writer.flush().await?;

        // 消息处理循环
        loop {
            // 读取消息长度
            let mut length_bytes = [0u8; 4];
            match reader.read_exact(&mut length_bytes).await {
                Ok(_) => {}
                Err(_) => break, // 连接关闭
            }

            let length = u32::from_be_bytes(length_bytes) as usize;

            // Keep-alive 消息
            if length == 0 {
                continue;
            }

            // 读取消息
            let mut message_bytes = vec![0u8; length];
            match reader.read_exact(&mut message_bytes).await {
                Ok(_) => {}
                Err(_) => break,
            }

            // 解析消息
            match Message::from_bytes(&length_bytes, &message_bytes) {
                Ok(message) => {
                    // 处理消息并发送响应
                    if let Err(e) = Self::handle_peer_message(
                        &mut reader,
                        &mut writer,
                        message,
                        &piece_manager,
                    ).await {
                        tracing::warn!("Failed to handle message from {}: {}", addr, e);
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to parse message from {}: {}", addr, e);
                }
            }
        }

        Ok(())
    }

    /// 处理 peer 发送的消息
    async fn handle_peer_message(
        reader: &mut tokio::net::tcp::OwnedReadHalf,
        writer: &mut tokio::net::tcp::OwnedWriteHalf,
        message: Message,
        piece_manager: &Arc<PieceManager>,
    ) -> Result<()> {
        match message {
            Message::Request { index, begin, length } => {
                // 读取 piece 并发送
                let piece_index = index as usize;

                // 读取整个 piece
                match piece_manager.read_piece(piece_index).await {
                    Ok(piece_data) => {
                        let start = begin as usize;
                        let end = std::cmp::min(start + length as usize, piece_data.len());
                        let block = piece_data[start..end].to_vec();

                        let piece_msg = Message::Piece {
                            index,
                            begin,
                            block,
                        };

                        let msg_bytes = piece_msg.to_bytes();
                        writer.write_all(&msg_bytes).await?;
                        writer.flush().await?;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to read piece {}: {}", index, e);
                    }
                }
            }
            Message::Interested => {
                // Leecher 感兴趣，发送 unchoke 允许下载
                tracing::debug!("收到 interested 消息，发送 unchoke");
                let unchoke_msg = Message::Unchoke;
                let msg_bytes = unchoke_msg.to_bytes();
                writer.write_all(&msg_bytes).await?;
                writer.flush().await?;
            }
            Message::Choke | Message::Unchoke | Message::NotInterested => {
                // 忽略这些消息
            }
            _ => {
                tracing::trace!("Unhandled message from peer: {:?}", message);
            }
        }

        Ok(())
    }

    /// 停止种子服务
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }

    /// 生成 peer ID
    fn generate_peer_id() -> [u8; 20] {
        let prefix = crate::torrent::PEER_ID_PREFIX;
        let mut peer_id = [0u8; 20];
        peer_id[..prefix.len()].copy_from_slice(prefix.as_bytes());

        // 随机部分
        for i in prefix.len()..20 {
            peer_id[i] = rand::random::<u8>();
        }

        peer_id
    }

    /// 获取连接的 peer 数量
    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }
}
