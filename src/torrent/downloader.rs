//! 下载器 (Leecher)
//!
//! 从 peers 下载文件

use crate::torrent::metainfo::TorrentMetaInfo;
use crate::torrent::piece::PieceManager;
use crate::torrent::protocol::Message;
use crate::torrent::peer::PeerConnection;
use crate::common::error::Result;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

/// 下载事件
#[derive(Debug, Clone)]
pub enum DownloadEvent {
    /// Piece 下载完成
    PieceCompleted { index: usize },

    /// 下载完成
    DownloadComplete,

    /// 下载失败
    DownloadFailed { reason: String },

    /// 进度更新
    Progress { completed: usize, total: usize },
}

/// 下载器
pub struct Downloader {
    /// Torrent 元信息
    metainfo: TorrentMetaInfo,

    /// Piece 管理器
    piece_manager: Arc<PieceManager>,

    /// 事件发送器
    event_tx: mpsc::Sender<DownloadEvent>,

    /// Peer ID
    peer_id: [u8; 20],
}

impl Downloader {
    /// 创建新的下载器
    pub fn new(
        metainfo: TorrentMetaInfo,
        piece_manager: Arc<PieceManager>,
        event_tx: mpsc::Sender<DownloadEvent>,
    ) -> Self {
        let peer_id = Self::generate_peer_id();

        Downloader {
            metainfo,
            piece_manager,
            event_tx,
            peer_id,
        }
    }

    /// 添加 peer 并开始下载
    pub async fn add_peer(&self, addr: std::net::SocketAddr) -> Result<()> {
        let info_hash = self.metainfo.info_hash()?;
        let peer_id = self.peer_id;
        let piece_manager = Arc::clone(&self.piece_manager);
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            match PeerConnection::connect(addr, info_hash, peer_id).await {
                Ok(mut peer) => {
                    if let Err(e) = Self::download_from_peer(
                        &mut peer,
                        &piece_manager,
                        &event_tx,
                    ).await {
                        tracing::warn!("Download from {} failed: {}", addr, e);
                        let _ = event_tx.send(DownloadEvent::DownloadFailed {
                            reason: format!("Peer {} error: {}", addr, e),
                        }).await;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to {}: {}", addr, e);
                }
            }
        });

        Ok(())
    }

    /// 从单个 peer 下载
    async fn download_from_peer(
        peer: &mut PeerConnection,
        piece_manager: &Arc<PieceManager>,
        event_tx: &mpsc::Sender<DownloadEvent>,
    ) -> Result<()> {
        // 发送 interested
        peer.send_interested().await?;

        // 获取需要的 pieces
        let piece_count = piece_manager.piece_count().await;
        let info_hash = /* 需要传入 */ [0u8; 20];

        // 简化的下载逻辑：按顺序下载
        for i in 0..piece_count {
            // 检查是否已完成
            if let Some(state) = piece_manager.piece_state(i).await {
                if state == crate::torrent::piece::PieceState::Completed {
                    continue;
                }
            }

            // 请求 piece（分块请求）
            let piece_size = /* 获取 piece 大小 */ 256 * 1024;
            let block_size = 16 * 1024;
            let mut offset = 0;

            while offset < piece_size {
                let request_size = std::cmp::min(block_size, piece_size - offset);

                // 发送请求
                peer.request_piece(i as u32, offset as u32, request_size as u32).await?;

                // 等待响应（简化，实际应该有超时和重试）
                match timeout(Duration::from_secs(30), Self::receive_piece(peer)).await {
                    Ok(Ok(Some((index, begin, data)))) => {
                        if index == i as u32 && begin == offset as u32 {
                            // 存储到 piece manager
                            // piece_manager.add_block(...);

                            offset += data.len();
                        }
                    }
                    _ => {
                        return Err(crate::common::error::Error::Timeout);
                    }
                }
            }

            // 校验并存储 piece
            // let piece_data = piece_manager.get_piece_data(i).await;
            // piece_manager.store_piece(i, &piece_data).await?;

            // 发送完成事件
            let _ = event_tx.send(DownloadEvent::PieceCompleted { index: i }).await;

            // 发送进度
            let completed = piece_manager.completed_count().await;
            let total = piece_count;
            let _ = event_tx.send(DownloadEvent::Progress { completed, total }).await;

            if completed == total {
                let _ = event_tx.send(DownloadEvent::DownloadComplete).await;
                break;
            }
        }

        Ok(())
    }

    /// 接收 piece 数据
    async fn receive_piece(peer: &mut PeerConnection) -> Result<Option<(u32, u32, Vec<u8>)>> {
        // 这里应该从 peer 的读取通道获取消息
        // 简化实现
        Ok(None)
    }

    /// 生成 peer ID
    fn generate_peer_id() -> [u8; 20] {
        let prefix = crate::torrent::PEER_ID_PREFIX;
        let mut peer_id = [0u8; 20];
        peer_id[..prefix.len()].copy_from_slice(prefix.as_bytes());

        for i in prefix.len()..20 {
            peer_id[i] = rand::random::<u8>();
        }

        peer_id
    }

    /// 获取下载进度
    pub async fn progress(&self) -> f64 {
        self.piece_manager.progress().await
    }
}
