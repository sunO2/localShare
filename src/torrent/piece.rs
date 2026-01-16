//! Piece 分片管理
//!
//! 管理 torrent 的 piece 状态，包括下载、校验和存储

use crate::torrent::metainfo::TorrentMetaInfo;
use sha1::Digest;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Piece 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceState {
    /// 未开始
    NotStarted,
    /// 下载中
    Downloading,
    /// 已完成（已校验）
    Completed,
    /// 校验失败
    Failed,
}

/// Piece 信息
#[derive(Debug, Clone)]
pub struct Piece {
    /// Piece 索引
    pub index: usize,

    /// Piece 大小（最后一个 piece 可能较小）
    pub size: usize,

    /// Piece 状态
    pub state: PieceState,

    /// 下载的数据
    pub data: Vec<u8>,

    /// 已下载的 block 数量
    pub blocks_downloaded: usize,

    /// 总 block 数量
    pub blocks_total: usize,
}

impl Piece {
    /// 创建新的 piece
    pub fn new(index: usize, size: usize, block_size: usize) -> Self {
        let blocks_total = (size + block_size - 1) / block_size;
        Piece {
            index,
            size,
            state: PieceState::NotStarted,
            data: Vec::with_capacity(size),
            blocks_downloaded: 0,
            blocks_total,
        }
    }

    /// 添加 block 数据
    pub fn add_block(&mut self, offset: usize, data: &[u8], block_size: usize) -> Result<(), String> {
        if offset + data.len() > self.size {
            return Err("Block offset out of bounds".to_string());
        }

        // 确保 data 足够大
        if self.data.len() < self.size {
            self.data.resize(self.size, 0);
        }

        // 复制 block 数据
        self.data[offset..offset + data.len()].copy_from_slice(data);

        // 更新计数
        let block_index = offset / block_size;
        self.blocks_downloaded = (self.blocks_total).max(block_index + 1);

        Ok(())
    }

    /// 检查 piece 是否完整
    pub fn is_complete(&self) -> bool {
        self.blocks_downloaded >= self.blocks_total && self.data.len() == self.size
    }

    /// 校验 piece
    pub fn verify(&self, expected_hash: &[u8; 20]) -> bool {
        if !self.is_complete() {
            return false;
        }

        let hash = sha1::Sha1::digest(&self.data);
        &hash[..] == expected_hash
    }
}

/// Piece 管理器
pub struct PieceManager {
    /// Torrent 元信息
    metainfo: TorrentMetaInfo,

    /// 存储路径
    storage_path: PathBuf,

    /// Pieces（索引 -> Piece）
    pieces: Arc<RwLock<Vec<Piece>>>,

    /// Piece 大小
    piece_length: u32,

    /// Block 大小（通常 16KB）
    block_size: usize,
}

impl PieceManager {
    /// 创建新的 piece 管理器
    pub fn new(metainfo: TorrentMetaInfo, storage_path: PathBuf) -> Self {
        let piece_length = metainfo.info.piece_length;
        let total_size = metainfo.total_size();
        let piece_count = metainfo.piece_count();

        let mut pieces = Vec::with_capacity(piece_count);
        for i in 0..piece_count {
            let start = (i as u64) * (piece_length as u64);
            let end = std::cmp::min(start + piece_length as u64, total_size);
            let size = (end - start) as usize;

            pieces.push(Piece::new(i, size, 16 * 1024)); // 16KB blocks
        }

        PieceManager {
            metainfo,
            storage_path,
            pieces: Arc::new(RwLock::new(pieces)),
            piece_length,
            block_size: 16 * 1024,
        }
    }

    /// 获取 piece 数量
    pub async fn piece_count(&self) -> usize {
        self.pieces.read().await.len()
    }

    /// 获取已完成的 piece 数量
    pub async fn completed_count(&self) -> usize {
        self.pieces
            .read()
            .await
            .iter()
            .filter(|p| p.state == PieceState::Completed)
            .count()
    }

    /// 获取下载进度
    pub async fn progress(&self) -> f64 {
        let total = self.piece_count().await;
        if total == 0 {
            return 0.0;
        }
        let completed = self.completed_count().await;
        (completed as f64) / (total as f64)
    }

    /// 获取 piece 位图（用于 have 消息）
    pub async fn bitmap(&self) -> Vec<u8> {
        let pieces = self.pieces.read().await;
        let piece_count = pieces.len();
        let mut bitmap = vec![0u8; (piece_count + 7) / 8];

        for (i, piece) in pieces.iter().enumerate() {
            if piece.state == PieceState::Completed {
                bitmap[i / 8] |= 1 << (7 - (i % 8));
            }
        }

        bitmap
    }

    /// 从位图解析
    pub async fn parse_bitmap(&self, bitmap: &[u8]) -> Vec<usize> {
        let mut have_pieces = Vec::new();
        let piece_count = self.piece_count().await;

        for (byte_i, &byte) in bitmap.iter().enumerate() {
            for bit in 0..8 {
                let piece_index = byte_i * 8 + bit;
                if piece_index >= piece_count {
                    break;
                }
                if byte & (1 << (7 - bit)) != 0 {
                    have_pieces.push(piece_index);
                }
            }
        }

        have_pieces
    }

    /// 获取需要下载的 pieces
    pub async fn needed_pieces(&self, peer_have: &[usize]) -> Vec<usize> {
        let pieces = self.pieces.read().await;
        peer_have
            .iter()
            .filter(|&&i| {
                pieces
                    .get(i)
                    .map(|p| p.state != PieceState::Completed)
                    .unwrap_or(false)
            })
            .copied()
            .collect()
    }

    /// 存储完成的 piece 到磁盘
    pub async fn store_piece(&self, index: usize, data: &[u8]) -> Result<(), String> {
        // 校验 hash
        let expected_hash = self
            .metainfo
            .piece_hash(index)
            .ok_or("Invalid piece index")?;

        let hash = sha1::Sha1::digest(data);
        if &hash[..] != expected_hash {
            return Err("Piece hash verification failed".to_string());
        }

        // 存储到文件
        if self.metainfo.is_single_file() {
            self.store_piece_single_file(index, data).await
        } else {
            self.store_piece_multi_file(index, data).await
        }
    }

    /// 存储单文件模式下的 piece
    async fn store_piece_single_file(&self, index: usize, data: &[u8]) -> Result<(), String> {
        let file_path = self.storage_path.join(&self.metainfo.info.name);

        // 使用 tokio 的文件操作
        use tokio::io::{AsyncWriteExt, AsyncSeekExt};

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&file_path)
            .await
            .map_err(|e| format!("Failed to open file: {}", e))?;

        let offset = (index as u64) * (self.piece_length as u64);

        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|e| format!("Failed to seek: {}", e))?;

        file.write_all(data)
            .await
            .map_err(|e| format!("Failed to write: {}", e))?;

        Ok(())
    }

    /// 存储多文件模式下的 piece
    async fn store_piece_multi_file(&self, index: usize, data: &[u8]) -> Result<(), String> {
        let files = self
            .metainfo
            .info
            .files
            .as_ref()
            .ok_or("No files in torrent")?;

        // 计算这个 piece 跨越的文件
        let piece_offset = (index as u64) * (self.piece_length as u64);
        let mut current_offset = piece_offset;
        let mut data_offset = 0;

        for file_info in files {
            if current_offset < file_info.length {
                // 这个文件包含 piece 的一部分数据
                let _file_offset_in_piece = (current_offset - piece_offset) as usize;
                let file_start = (current_offset % self.piece_length as u64) as usize;

                // 构建文件路径
                let mut file_path = self.storage_path.join(&self.metainfo.info.name);
                for component in &file_info.path {
                    file_path.push(component);
                }

                // 确保目录存在
                if let Some(parent) = file_path.parent() {
                    tokio::fs::create_dir_all(parent)
                        .await
                        .map_err(|e| format!("Failed to create directory: {}", e))?;
                }

                // 写入文件
                use tokio::io::{AsyncWriteExt, AsyncSeekExt};

                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(&file_path)
                    .await
                    .map_err(|e| format!("Failed to open file: {}", e))?;

                file.seek(std::io::SeekFrom::Start(current_offset))
                    .await
                    .map_err(|e| format!("Failed to seek: {}", e))?;

                let bytes_in_file = std::cmp::min(
                    file_info.length - current_offset,
                    (data.len() - data_offset) as u64,
                ) as usize;

                let end_index = file_start + bytes_in_file;
                if end_index <= data.len() {
                    file.write_all(&data[file_start..end_index])
                        .await
                        .map_err(|e| format!("Failed to write: {}", e))?;
                }

                data_offset += bytes_in_file;
                current_offset += bytes_in_file as u64;

                if data_offset >= data.len() {
                    break;
                }
            } else {
                current_offset -= file_info.length;
            }
        }

        Ok(())
    }

    /// 读取 piece 从磁盘
    pub async fn read_piece(&self, index: usize) -> Result<Vec<u8>, String> {
        let piece_size = if index + 1 < self.metainfo.piece_count() {
            self.piece_length as usize
        } else {
            // 最后一个 piece
            let total_size = self.metainfo.total_size();
            let offset = (index as u64) * (self.piece_length as u64);
            (total_size - offset) as usize
        };

        let mut data = vec![0u8; piece_size];
        self.read_piece_into(index, &mut data).await?;
        Ok(data)
    }

    /// 读取 piece 到指定缓冲区
    pub async fn read_piece_into(&self, index: usize, buffer: &mut [u8]) -> Result<(), String> {
        if self.metainfo.is_single_file() {
            self.read_piece_single_file(index, buffer).await
        } else {
            self.read_piece_multi_file(index, buffer).await
        }
    }

    /// 读取单文件模式下的 piece
    async fn read_piece_single_file(
        &self,
        index: usize,
        buffer: &mut [u8],
    ) -> Result<(), String> {
        let file_path = self.storage_path.join(&self.metainfo.info.name);

        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        let mut file = tokio::fs::File::open(&file_path)
            .await
            .map_err(|e| format!("Failed to open file: {}", e))?;

        let offset = (index as u64) * (self.piece_length as u64);

        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(|e| format!("Failed to seek: {}", e))?;

        file.read_exact(buffer)
            .await
            .map_err(|e| format!("Failed to read: {}", e))?;

        Ok(())
    }

    /// 读取多文件模式下的 piece
    async fn read_piece_multi_file(
        &self,
        index: usize,
        buffer: &mut [u8],
    ) -> Result<(), String> {
        let files = self
            .metainfo
            .info
            .files
            .as_ref()
            .ok_or("No files in torrent")?;

        let piece_offset = (index as u64) * (self.piece_length as u64);
        let mut current_offset = piece_offset;
        let mut buffer_offset = 0;

        for file_info in files {
            if current_offset < file_info.length {
                let _file_start = (current_offset % self.piece_length as u64) as usize;

                // 构建文件路径
                let mut file_path = self.storage_path.join(&self.metainfo.info.name);
                for component in &file_info.path {
                    file_path.push(component);
                }

                use tokio::io::{AsyncReadExt, AsyncSeekExt};

                let mut file = tokio::fs::File::open(&file_path)
                    .await
                    .map_err(|e| format!("Failed to open file: {}", e))?;

                file.seek(std::io::SeekFrom::Start(current_offset))
                    .await
                    .map_err(|e| format!("Failed to seek: {}", e))?;

                let bytes_in_file = std::cmp::min(
                    file_info.length - current_offset,
                    (buffer.len() - buffer_offset) as u64,
                ) as usize;

                file.read_exact(&mut buffer[buffer_offset..buffer_offset + bytes_in_file])
                    .await
                    .map_err(|e| format!("Failed to read: {}", e))?;

                buffer_offset += bytes_in_file;
                current_offset += bytes_in_file as u64;

                if buffer_offset >= buffer.len() {
                    break;
                }
            } else {
                current_offset -= file_info.length;
            }
        }

        Ok(())
    }

    /// 标记 piece 为已完成
    pub async fn mark_piece_completed(&self, index: usize) {
        let mut pieces = self.pieces.write().await;
        if let Some(piece) = pieces.get_mut(index) {
            piece.state = PieceState::Completed;
        }
    }

    /// 标记 piece 为下载中
    pub async fn mark_piece_downloading(&self, index: usize) {
        let mut pieces = self.pieces.write().await;
        if let Some(piece) = pieces.get_mut(index) {
            piece.state = PieceState::Downloading;
        }
    }

    /// 获取 piece 状态
    pub async fn piece_state(&self, index: usize) -> Option<PieceState> {
        let pieces = self.pieces.read().await;
        pieces.get(index).map(|p| p.state)
    }

    /// 标记所有 pieces 为已完成（用于种子场景）
    pub async fn mark_all_completed(&self) {
        let mut pieces = self.pieces.write().await;
        for piece in pieces.iter_mut() {
            piece.state = PieceState::Completed;
        }
        tracing::info!("已标记所有 {} 个 pieces 为已完成（种子模式）", pieces.len());
    }
}
