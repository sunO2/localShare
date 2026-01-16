//! 下载器 (Leecher)
//!
//! 从 peers 下载文件

use crate::torrent::metainfo::TorrentMetaInfo;
use crate::torrent::piece::{PieceManager, PieceState};
use crate::torrent::protocol::Message;
use crate::torrent::peer::PeerConnection;
use crate::common::error::{Error, Result};
use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration, sleep};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 下载事件
#[derive(Debug, Clone)]
pub enum DownloadEvent {
    /// 连接到 peer
    Connected { peer: String },

    /// Piece 下载完成
    PieceCompleted { index: usize, downloaded_bytes: u64 },

    /// 下载完成
    DownloadComplete,

    /// 下载失败
    DownloadFailed { reason: String },

    /// 进度更新 (百分比 0.0-100.0)
    Progress { percent: f64 },

    /// 字节进度更新
    BytesProgress { downloaded_bytes: u64, total_bytes: u64 },

    /// Peer 断开连接
    PeerDisconnected { peer: String },
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

    /// Info hash
    info_hash: [u8; 20],

    /// 存储路径
    storage_path: PathBuf,
}

impl Downloader {
    /// 创建新的下载器
    pub fn new(
        metainfo: TorrentMetaInfo,
        piece_manager: Arc<PieceManager>,
        event_tx: mpsc::Sender<DownloadEvent>,
        storage_path: PathBuf,
    ) -> Result<Self> {
        let peer_id = Self::generate_peer_id();
        let info_hash = metainfo.info_hash()?;

        Ok(Downloader {
            metainfo,
            piece_manager,
            event_tx,
            peer_id,
            info_hash,
            storage_path,
        })
    }

    /// 从 peer 获取 torrent 元数据
    pub async fn fetch_metadata(peer_addr: std::net::SocketAddr, info_hash_hex: &str) -> Result<Vec<u8>> {
        tracing::info!("=== 开始获取元数据 ===");
        tracing::info!("从 {} 获取元数据", peer_addr);
        tracing::info!("Info Hash (hex): {}", info_hash_hex);

        // 连接到元数据端口 (8080)
        let metadata_port = 8080;
        let metadata_addr = std::net::SocketAddr::new(peer_addr.ip(), metadata_port);

        tracing::info!("连接到元数据服务器: {}", metadata_addr);

        match timeout(Duration::from_secs(5), TcpStream::connect(metadata_addr)).await {
            Ok(Ok(mut stream)) => {
                tracing::info!("已连接到元数据服务器");

                // 发送请求: 格式为 "GET /<info_hash>\n"
                let request = format!("GET /{}\n", info_hash_hex);
                tracing::debug!("发送请求: {}", request.trim());

                stream.write_all(request.as_bytes()).await
                    .map_err(|e| {
                        tracing::error!("发送元数据请求失败: {}", e);
                        Error::Network(format!("Failed to send metadata request: {}", e))
                    })?;

                tracing::info!("等待元数据响应...");

                // 读取响应长度 (4 字节)
                let mut len_bytes = [0u8; 4];
                stream.read_exact(&mut len_bytes).await
                    .map_err(|e| {
                        tracing::error!("读取元数据长度失败: {}", e);
                        Error::Network(format!("Failed to read metadata length: {}", e))
                    })?;
                let len = u32::from_be_bytes(len_bytes) as usize;

                tracing::info!("元数据长度: {} 字节", len);

                if len == 0 {
                    let msg = format!(
                        "元数据服务器返回空响应。可能原因:\n\
                         1. 对方设备还没有共享此文件\n\
                         2. 文件共享失败\n\
                         3. Info Hash 不匹配\n\
                         \n\
                         请确保对方设备已成功共享文件，且两台设备在同一局域网内。"
                    );
                    tracing::warn!("{}", msg);
                    return Err(Error::Other(msg));
                }

                if len > 10 * 1024 * 1024 { // 最大 10MB
                    return Err(Error::Other(format!("元数据太大: {} bytes", len)));
                }

                // 读取元数据
                let mut metadata = vec![0u8; len];
                stream.read_exact(&mut metadata).await
                    .map_err(|e| {
                        tracing::error!("读取元数据失败: {}", e);
                        Error::Network(format!("Failed to read metadata: {}", e))
                    })?;

                tracing::info!("✓ 成功获取 {} 字节的元数据", metadata.len());
                Ok(metadata)
            }
            Ok(Err(e)) => {
                tracing::error!("连接元数据服务器失败: {}", e);
                Err(Error::Network(format!("Failed to connect to metadata server: {}", e)))
            }
            Err(_) => {
                tracing::error!("连接元数据服务器超时");
                Err(Error::Timeout)
            }
        }
    }

    /// 启动下载（连接到单个 peer）
    pub async fn start_download(&self, addr: std::net::SocketAddr) -> Result<()> {
        tracing::info!("=== 开始下载 ===");
        tracing::info!("连接到: {}", addr);
        tracing::info!("Info Hash: {}", hex::encode(self.info_hash));

        let info_hash = self.info_hash;
        let peer_id = self.peer_id;
        let piece_manager = Arc::clone(&self.piece_manager);
        let event_tx = self.event_tx.clone();
        let metainfo = self.metainfo.clone();
        let storage_path = self.storage_path.clone();

        tokio::spawn(async move {
            match Self::download_from_peer(
                addr,
                info_hash,
                peer_id,
                piece_manager,
                event_tx.clone(),
                metainfo,
                storage_path,
            ).await {
                Ok(_) => {
                    tracing::info!("下载完成");
                    let _ = event_tx.send(DownloadEvent::DownloadComplete).await;
                }
                Err(e) => {
                    tracing::error!("下载失败: {}", e);
                    let _ = event_tx.send(DownloadEvent::DownloadFailed {
                        reason: e.to_string(),
                    }).await;
                }
            }
        });

        Ok(())
    }

    /// 从单个 peer 下载
    async fn download_from_peer(
        addr: std::net::SocketAddr,
        info_hash: [u8; 20],
        peer_id: [u8; 20],
        piece_manager: Arc<PieceManager>,
        event_tx: mpsc::Sender<DownloadEvent>,
        metainfo: TorrentMetaInfo,
        storage_path: PathBuf,
    ) -> Result<()> {
        // 连接到 peer
        let mut peer = PeerConnection::connect(addr, info_hash, peer_id).await
            .map_err(|e| {
                tracing::error!("连接失败: {}", e);
                e
            })?;

        tracing::info!("已连接到 peer: {}", addr);
        let _ = event_tx.send(DownloadEvent::Connected {
            peer: addr.to_string(),
        }).await;

        // 获取 piece 数量
        let piece_count = piece_manager.piece_count().await;
        let total_size = metainfo.total_size();
        let piece_length = metainfo.info.piece_length;

        tracing::info!("文件大小: {} bytes, piece 数量: {}, piece 大小: {}",
            total_size, piece_count, piece_length);

        // 等待 peer 发送 bitfield
        sleep(Duration::from_millis(100)).await;

        // 获取 peer 拥有的 pieces
        let peer_pieces = peer.get_have_pieces().await;
        tracing::info!("Peer 拥有 {} 个 pieces", peer_pieces.len());

        // 发送 interested
        peer.send_interested().await?;

        // 等待 unchoke
        let mut unchoked = false;
        for _ in 0..10 {
            match timeout(Duration::from_secs(2), peer.recv()).await {
                Ok(Ok(Message::Unchoke)) => {
                    unchoked = true;
                    tracing::info!("收到 unchoke");
                    break;
                }
                Ok(Ok(msg)) => {
                    tracing::debug!("收到其他消息: {:?}", std::mem::discriminant(&msg));
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    tracing::warn!("等待 unchoke 超时，重试...");
                }
            }
        }

        if !unchoked {
            return Err(Error::Other("Peer 未发送 unchoke".to_string()));
        }

        // 开始下载 pieces
        let block_size = 16 * 1024; // 16KB
        let mut completed_count = 0;
        let mut last_progress_time = std::time::Instant::now();
        let mut total_downloaded_bytes = 0u64; // 总下载字节数

        // 策略：按顺序下载每个需要的 piece
        for piece_index in 0..piece_count {
            // 检查是否已完成
            if let Some(state) = piece_manager.piece_state(piece_index).await {
                if state == PieceState::Completed {
                    completed_count += 1;
                    continue;
                }
            }

            // 检查 peer 是否有这个 piece
            if !peer_pieces.contains(&piece_index) {
                tracing::warn!("Peer 没有 piece {}", piece_index);
                continue;
            }

            // 计算 piece 大小
            let piece_size = if piece_index + 1 < piece_count {
                piece_length as usize
            } else {
                // 最后一个 piece
                let offset = (piece_index as u64) * (piece_length as u64);
                (total_size - offset) as usize
            };

            tracing::info!("开始下载 piece {} (大小: {} bytes)", piece_index, piece_size);
            piece_manager.mark_piece_downloading(piece_index).await;

            // 分块下载
            let mut piece_data = vec![0u8; piece_size];
            let mut downloaded = 0;

            while downloaded < piece_size {
                let request_size = std::cmp::min(block_size, piece_size - downloaded);

                // 发送请求
                peer.request_piece(
                    piece_index as u32,
                    downloaded as u32,
                    request_size as u32,
                ).await?;

                // 等待响应
                match timeout(Duration::from_secs(30), peer.recv()).await {
                    Ok(Ok(Message::Piece { index, begin, block })) => {
                        if index as usize == piece_index && begin as usize == downloaded {
                            // 复制数据
                            let start = downloaded;
                            let end = downloaded + block.len();
                            if end <= piece_size {
                                piece_data[start..end].copy_from_slice(&block);
                                downloaded += block.len();

                                // 发送进度更新
                                let percent = ((completed_count as f64) / (piece_count as f64)) * 100.0;
                                if last_progress_time.elapsed() >= Duration::from_millis(500) {
                                    let _ = event_tx.send(DownloadEvent::Progress {
                                        percent: ((completed_count as f64 + (downloaded as f64) / (piece_size as f64)) / (piece_count as f64)) * 100.0,
                                    }).await;
                                    last_progress_time = std::time::Instant::now();
                                }
                            } else {
                                tracing::warn!("Piece 数据越界");
                            }
                        } else {
                            tracing::warn!("收到的 piece 数据不匹配");
                        }
                    }
                    Ok(Ok(msg)) => {
                        tracing::debug!("收到其他消息: {:?}", std::mem::discriminant(&msg));
                    }
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        tracing::warn!("请求超时，重试...");
                        // 重试
                        continue;
                    }
                }
            }

            // 校验并存储 piece
            tracing::info!("Piece {} 下载完成，正在校验...", piece_index);

            match piece_manager.store_piece(piece_index, &piece_data).await {
                Ok(_) => {
                    tracing::info!("Piece {} 校验通过并已存储", piece_index);
                    completed_count += 1;

                    // 更新总下载字节数
                    total_downloaded_bytes += piece_size as u64;

                    // 发送 piece 完成事件（包含已下载字节数）
                    let _ = event_tx.send(DownloadEvent::PieceCompleted {
                        index: piece_index,
                        downloaded_bytes: total_downloaded_bytes,
                    }).await;

                    // 发送字节进度事件
                    let _ = event_tx.send(DownloadEvent::BytesProgress {
                        downloaded_bytes: total_downloaded_bytes,
                        total_bytes: total_size,
                    }).await;

                    // 发送进度更新
                    let percent = ((completed_count as f64) / (piece_count as f64)) * 100.0;
                    let _ = event_tx.send(DownloadEvent::Progress { percent }).await;
                }
                Err(e) => {
                    tracing::error!("Piece {} 校验失败: {}", piece_index, e);
                    return Err(Error::Other(format!("Piece {} 校验失败: {}", piece_index, e)));
                }
            }

            // 发送 have 消息
            peer.send_have(piece_index).await?;

            // 检查是否全部完成
            if completed_count == piece_count {
                tracing::info!("所有 pieces 下载完成!");
                break;
            }
        }

        Ok(())
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
