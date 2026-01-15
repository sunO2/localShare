//! P2P 对等节点连接管理
//!
//! 管理与单个 peer 的连接

use crate::torrent::protocol::{Handshake, Message, PeerState};
use crate::common::error::{Error, Result};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

/// Peer 连接
pub struct PeerConnection {
    /// Peer 地址
    pub addr: SocketAddr,

    /// Peer ID
    pub peer_id: [u8; 20],

    /// Peer 状态
    pub state: PeerState,

    /// 拥有的 pieces（位图）
    pub have_pieces: Vec<usize>,

    /// Socket 发送器
    tx: mpsc::Sender<Message>,
}

impl PeerConnection {
    /// 创建新的 peer 连接
    pub async fn connect(
        addr: SocketAddr,
        info_hash: [u8; 20],
        peer_id: [u8; 20],
    ) -> Result<Self> {
        // 连接到 peer
        let socket = TcpStream::connect(addr)
            .await
            .map_err(|e| Error::Network(format!("Failed to connect to {}: {}", addr, e)))?;

        // 发送握手
        let handshake = Handshake::new(info_hash, peer_id);
        let handshake_bytes = handshake.to_bytes();

        let (mut reader, mut writer) = socket.into_split();

        writer.write_all(&handshake_bytes).await
            .map_err(|e| Error::Network(format!("Failed to send handshake: {}", e)))?;

        // 接收握手
        let mut recv_handshake = vec![0u8; 68];
        timeout(Duration::from_secs(10), reader.read_exact(&mut recv_handshake))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|e| Error::Network(format!("Failed to receive handshake: {}", e)))?;

        let recv_handshake = Handshake::from_bytes(&recv_handshake)?;

        // 验证 info hash
        if recv_handshake.info_hash != info_hash {
            return Err(Error::Other("Info hash mismatch".to_string()));
        }

        let peer_id = recv_handshake.peer_id;

        // 创建消息通道
        let (tx, mut rx) = mpsc::channel::<Message>(100);

        // 启动写入任务
        tokio::spawn(async move {
            // 发送任务
            while let Some(msg) = rx.recv().await {
                if let Err(e) = Self::send_message(&mut writer, msg).await {
                    tracing::warn!("Failed to send message to {}: {}", addr, e);
                    break;
                }
            }
        });

        Ok(PeerConnection {
            addr,
            peer_id,
            state: PeerState::default(),
            have_pieces: Vec::new(),
            tx,
        })
    }

    /// 发送消息
    async fn send_message(writer: &mut tokio::net::tcp::OwnedWriteHalf, message: Message) -> Result<()> {
        let bytes = message.to_bytes();
        writer.write_all(&bytes).await
            .map_err(|e| Error::Network(format!("Send failed: {}", e)))?;
        writer.flush().await
            .map_err(|e| Error::Network(format!("Flush failed: {}", e)))?;
        Ok(())
    }

    /// 发送消息
    pub async fn send(&self, message: Message) -> Result<()> {
        self.tx.send(message)
            .await
            .map_err(|_| Error::Other("Peer connection closed".to_string()))?;
        Ok(())
    }

    /// 发送 bitfield
    pub async fn send_bitfield(&mut self, bitmap: Vec<u8>) -> Result<()> {
        self.send(Message::Bitfield { bitmap }).await?;
        Ok(())
    }

    /// 发送 have 消息
    pub async fn send_have(&mut self, index: usize) -> Result<()> {
        self.send(Message::Have { index }).await?;
        Ok(())
    }

    /// 请求 piece
    pub async fn request_piece(&mut self, index: u32, begin: u32, length: u32) -> Result<()> {
        self.send(Message::Request {
            index,
            begin,
            length,
        }).await?;
        Ok(())
    }

    /// 取消请求
    pub async fn cancel_request(&mut self, index: u32, begin: u32, length: u32) -> Result<()> {
        self.send(Message::Cancel {
            index,
            begin,
            length,
        }).await?;
        Ok(())
    }

    /// 发送 interested
    pub async fn send_interested(&mut self) -> Result<()> {
        self.state.interested = true;
        self.send(Message::Interested).await
    }

    /// 发送 not interested
    pub async fn send_not_interested(&mut self) -> Result<()> {
        self.state.interested = false;
        self.send(Message::NotInterested).await
    }

    /// 发送 unchoke（允许对方下载）
    pub async fn send_unchoke(&mut self) -> Result<()> {
        self.state.peer_choked = false;
        self.send(Message::Unchoke).await
    }

    /// 发送 choke（阻止对方下载）
    pub async fn send_choke(&mut self) -> Result<()> {
        self.state.peer_choked = true;
        self.send(Message::Choke).await
    }

    /// 检查是否拥有某个 piece
    pub fn has_piece(&self, index: usize) -> bool {
        self.have_pieces.contains(&index)
    }
}
