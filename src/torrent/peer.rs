//! P2P 对等节点连接管理
//!
//! 管理与单个 peer 的连接

use crate::torrent::protocol::{Handshake, Message, PeerState};
use crate::common::error::{Error, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{timeout, Duration};

/// Peer 连接
pub struct PeerConnection {
    /// Peer 地址
    pub addr: SocketAddr,

    /// Peer ID
    pub peer_id: [u8; 20],

    /// Peer 状态
    pub state: Arc<RwLock<PeerState>>,

    /// 拥有的 pieces（位图）
    pub have_pieces: Arc<RwLock<Vec<usize>>>,

    /// Socket 发送器
    tx: mpsc::Sender<Message>,

    /// 消息接收器
    rx: mpsc::Receiver<Message>,
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
        writer.flush().await
            .map_err(|e| Error::Network(format!("Failed to flush handshake: {}", e)))?;

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

        // 创建消息通道（双向）
        let (tx, mut send_rx) = mpsc::channel::<Message>(100);
        let (message_tx, rx) = mpsc::channel::<Message>(100);

        let state = Arc::new(RwLock::new(PeerState::default()));
        let state_clone = Arc::clone(&state);

        let have_pieces = Arc::new(RwLock::new(Vec::new()));
        let have_pieces_clone = Arc::clone(&have_pieces);

        let addr_clone = addr;

        // 启动写入任务
        tokio::spawn(async move {
            while let Some(msg) = send_rx.recv().await {
                if let Err(e) = Self::send_message(&mut writer, msg).await {
                    tracing::warn!("Failed to send message to {}: {}", addr_clone, e);
                    break;
                }
            }
        });

        // 启动读取任务
        tokio::spawn(async move {
            while let Ok(msg) = Self::receive_message(&mut reader).await {
                // 更新 peer 状态
                match &msg {
                    Message::Choke | Message::Unchoke | Message::Interested | Message::NotInterested => {
                        let mut s = state_clone.write().await;
                        s.handle_message(&msg);
                    }
                    Message::Bitfield { bitmap } => {
                        let mut pieces = have_pieces_clone.write().await;
                        *pieces = Self::parse_bitmap(bitmap);
                        tracing::debug!("Peer {} sent bitfield with {} pieces", addr_clone, pieces.len());
                    }
                    Message::Have { index } => {
                        let mut pieces = have_pieces_clone.write().await;
                        if !pieces.contains(index) {
                            pieces.push(*index);
                            pieces.sort();
                        }
                    }
                    _ => {}
                }

                // 发送到通道
                if message_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });

        Ok(PeerConnection {
            addr,
            peer_id,
            state,
            have_pieces,
            tx,
            rx,
        })
    }

    /// 解析位图
    fn parse_bitmap(bitmap: &[u8]) -> Vec<usize> {
        let mut pieces = Vec::new();
        for (byte_i, &byte) in bitmap.iter().enumerate() {
            for bit in 0..8 {
                if byte & (1 << (7 - bit)) != 0 {
                    pieces.push(byte_i * 8 + bit);
                }
            }
        }
        pieces
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

    /// 接收消息
    async fn receive_message(reader: &mut tokio::net::tcp::OwnedReadHalf) -> Result<Message> {
        // 读取消息长度（4 字节）
        let mut len_bytes = [0u8; 4];
        reader.read_exact(&mut len_bytes).await
            .map_err(|e| Error::Network(format!("Failed to read message length: {}", e)))?;

        let msg_len = u32::from_be_bytes(len_bytes) as usize;

        // Keep-alive 消息
        if msg_len == 0 {
            // 返回一个占位消息，实际应该单独处理
            return Ok(Message::Unchoke);
        }

        // 读取消息内容
        let mut msg_bytes = vec![0u8; msg_len];
        reader.read_exact(&mut msg_bytes).await
            .map_err(|e| Error::Network(format!("Failed to read message payload: {}", e)))?;

        // 解析消息
        Message::from_bytes(&len_bytes, &msg_bytes)
    }

    /// 发送消息
    pub async fn send(&self, message: Message) -> Result<()> {
        self.tx.send(message)
            .await
            .map_err(|_| Error::Other("Peer connection closed".to_string()))?;
        Ok(())
    }

    /// 接收消息（阻塞）
    pub async fn recv(&mut self) -> Result<Message> {
        self.rx.recv().await
            .ok_or_else(|| Error::Other("Peer connection closed".to_string()))
        }

    /// 尝试接收消息（非阻塞）
    pub fn try_recv(&mut self) -> Result<Option<Message>> {
        match self.rx.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(_) => Err(Error::Other("Peer connection closed".to_string())),
        }
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
        {
            let mut s = self.state.write().await;
            s.interested = true;
        }
        self.send(Message::Interested).await
    }

    /// 发送 not interested
    pub async fn send_not_interested(&mut self) -> Result<()> {
        {
            let mut s = self.state.write().await;
            s.interested = false;
        }
        self.send(Message::NotInterested).await
    }

    /// 发送 unchoke（允许对方下载）
    pub async fn send_unchoke(&mut self) -> Result<()> {
        {
            let mut s = self.state.write().await;
            s.peer_choked = false;
        }
        self.send(Message::Unchoke).await
    }

    /// 发送 choke（阻止对方下载）
    pub async fn send_choke(&mut self) -> Result<()> {
        {
            let mut s = self.state.write().await;
            s.peer_choked = true;
        }
        self.send(Message::Choke).await
    }

    /// 检查是否拥有某个 piece
    pub async fn has_piece(&self, index: usize) -> bool {
        self.have_pieces.read().await.contains(&index)
    }

    /// 获取拥有的所有 pieces
    pub async fn get_have_pieces(&self) -> Vec<usize> {
        self.have_pieces.read().await.clone()
    }

    /// 检查是否被 choke
    pub async fn is_choked(&self) -> bool {
        self.state.read().await.choked
    }
}
