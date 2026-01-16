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

    /// 从单个 peer 下载（支持并行下载优化）
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

        // === 并行下载优化 ===
        // 配置参数
        let max_parallel_pieces = 4;       // 同时下载的 piece 数量
        let max_pending_requests = 16;     // 同时未完成的 block 请求数
        let block_size = 16 * 1024;        // 16KB block 大小

        // 下载状态
        let mut completed_pieces = std::collections::HashSet::new();
        let mut total_downloaded_bytes = 0u64;
        let mut last_progress_time = std::time::Instant::now();

        // 正在下载的 pieces: piece_index -> (data, requested_blocks, received_blocks)
        let mut downloading_pieces: std::collections::HashMap<
            usize,
            (Vec<u8>, std::collections::HashSet<usize>, usize)
        > = std::collections::HashMap::new();

        // 待请求的 blocks: (piece_index, offset, length)
        let mut pending_requests: std::vec::Vec<(usize, usize, usize)> = std::vec::Vec::new();

        // 已发送但未收到响应的请求
        let mut outstanding_requests: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();

        // 初始化：找出所有需要下载的 pieces
        let mut pieces_to_download: Vec<usize> = Vec::new();
        for piece_index in 0..piece_count {
            if let Some(state) = piece_manager.piece_state(piece_index).await {
                if state != PieceState::Completed && peer_pieces.contains(&piece_index) {
                    pieces_to_download.push(piece_index);
                }
            } else if peer_pieces.contains(&piece_index) {
                pieces_to_download.push(piece_index);
            }
        }

        if pieces_to_download.is_empty() {
            tracing::info!("没有需要下载的 pieces");
            return Ok(());
        }

        tracing::info!("需要下载 {} 个 pieces，使用并行下载 ({} pieces, {} pending requests)",
            pieces_to_download.len(), max_parallel_pieces, max_pending_requests);

        // 主下载循环
        while completed_pieces.len() < pieces_to_download.len() {
            // 1. 启动新的 piece 下载（如果有空间）
            while downloading_pieces.len() < max_parallel_pieces && !pieces_to_download.is_empty() {
                let piece_index = pieces_to_download.remove(0);

                // 计算 piece 大小
                let piece_size = if piece_index + 1 < piece_count {
                    piece_length as usize
                } else {
                    let offset = (piece_index as u64) * (piece_length as u64);
                    (total_size - offset) as usize
                };

                let num_blocks = (piece_size + block_size - 1) / block_size;

                downloading_pieces.insert(
                    piece_index,
                    (vec![0u8; piece_size], std::collections::HashSet::new(), 0)
                );

                // 添加所有 block 到待请求队列
                for offset in (0..piece_size).step_by(block_size) {
                    let request_size = std::cmp::min(block_size, piece_size - offset);
                    pending_requests.push((piece_index, offset, request_size));
                }

                tracing::debug!("启动 piece {} 下载 ({} bytes, {} blocks)",
                    piece_index, piece_size, num_blocks);
                piece_manager.mark_piece_downloading(piece_index).await;
            }

            // 2. 发送请求（保持足够数量的未完成请求）
            while outstanding_requests.len() < max_pending_requests && !pending_requests.is_empty() {
                let (piece_index, offset, size) = pending_requests.remove(0);
                outstanding_requests.insert((piece_index, offset));

                peer.request_piece(
                    piece_index as u32,
                    offset as u32,
                    size as u32,
                ).await?;
            }

            // 3. 等待响应（带超时）
            match timeout(Duration::from_secs(30), peer.recv()).await {
                Ok(Ok(Message::Piece { index, begin, block })) => {
                    let piece_index = index as usize;
                    let offset = begin as usize;

                    // 移除未完成请求标记
                    outstanding_requests.remove(&(piece_index, offset));

                    // 处理收到的 block
                    if let Some((piece_data, received_blocks, downloaded)) = downloading_pieces.get_mut(&piece_index) {
                        if offset + block.len() <= piece_data.len() {
                            piece_data[offset..offset + block.len()].copy_from_slice(&block);
                            received_blocks.insert(offset);
                            *downloaded += block.len();

                            // 检查 piece 是否完成
                            let piece_size = piece_data.len();
                            let num_blocks = (piece_size + block_size - 1) / block_size;

                            if received_blocks.len() == num_blocks {
                                // Piece 完成，校验并存储
                                tracing::info!("Piece {} 下载完成，正在校验...", piece_index);

                                match piece_manager.store_piece(piece_index, piece_data).await {
                                    Ok(_) => {
                                        tracing::info!("Piece {} 校验通过并已存储", piece_index);
                                        completed_pieces.insert(piece_index);
                                        total_downloaded_bytes += piece_size as u64;

                                        // 发送事件
                                        let _ = event_tx.send(DownloadEvent::PieceCompleted {
                                            index: piece_index,
                                            downloaded_bytes: total_downloaded_bytes,
                                        }).await;

                                        let _ = event_tx.send(DownloadEvent::BytesProgress {
                                            downloaded_bytes: total_downloaded_bytes,
                                            total_bytes: total_size,
                                        }).await;

                                        peer.send_have(piece_index).await?;
                                    }
                                    Err(e) => {
                                        tracing::error!("Piece {} 校验失败: {}", piece_index, e);
                                        return Err(Error::Other(format!("Piece {} 校验失败: {}", piece_index, e)));
                                    }
                                }

                                // 移除已完成 piece
                                downloading_pieces.remove(&piece_index);
                            }
                        } else {
                            tracing::warn!("Piece {} block 数据越界", piece_index);
                        }
                    } else {
                        tracing::warn!("收到未知 piece {} 的 block", piece_index);
                    }

                    // 发送进度更新
                    if last_progress_time.elapsed() >= Duration::from_millis(200) {
                        let percent = ((completed_pieces.len() as f64) / (piece_count as f64)) * 100.0;
                        let _ = event_tx.send(DownloadEvent::Progress { percent }).await;
                        last_progress_time = std::time::Instant::now();
                    }
                }
                Ok(Ok(msg)) => {
                    tracing::debug!("收到其他消息: {:?}", std::mem::discriminant(&msg));
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    tracing::warn!("接收消息超时，重试未完成的请求");
                    // 超时：重发未完成的请求
                    for (piece_index, offset) in outstanding_requests.clone() {
                        if let Some((piece_data, _, _)) = downloading_pieces.get(&piece_index) {
                            let remaining = piece_data.len() - offset;
                            let request_size = std::cmp::min(block_size, remaining);
                            peer.request_piece(
                                piece_index as u32,
                                offset as u32,
                                request_size as u32,
                            ).await?;
                        }
                    }
                }
            }

            // 检查是否所有 pieces 都完成
            if completed_pieces.len() == piece_count {
                tracing::info!("所有 {} 个 pieces 下载完成!", completed_pieces.len());
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
