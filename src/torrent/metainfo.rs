//! Torrent 元信息 (.torrent 文件格式)
//!
//! 负责创建和解析 .torrent 文件

use crate::common::error::{Error, Result};
use crate::torrent::{DEFAULT_PIECE_LENGTH, bencode::BencodeValue};
use sha1::Digest;
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

/// Torrent 元信息结构
#[derive(Debug, Clone)]
pub struct TorrentMetaInfo {
    /// Tracker URL（局域网环境可以为空，使用 PEX/DHT）
    pub announce: Option<String>,

    /// 创建者
    pub created_by: Option<String>,

    /// 创建时间
    pub creation_date: Option<i64>,

    /// 编码
    pub encoding: Option<String>,

    /// 文件信息
    pub info: TorrentInfo,
}

/// Torrent 文件信息
#[derive(Debug, Clone)]
pub struct TorrentInfo {
    /// 文件/目录名
    pub name: String,

    /// 每个 piece 的大小（字节）
    pub piece_length: u32,

    /// 所有 piece 的 SHA1 hash 拼接（每 20 字节一个 hash）
    pub pieces: Vec<u8>,

    /// 单文件模式下文件大小
    pub length: Option<u64>,

    /// 多文件模式下的文件列表
    pub files: Option<Vec<FileInfo>>,

    /// 是否私有（不使用 DHT/PEX）
    pub private: Option<u8>,

    /// MD5 sum（可选）
    pub md5sum: Option<String>,
}

/// 文件信息（多文件模式）
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// 文件路径（相对于顶层目录）
    pub path: Vec<String>,

    /// 文件大小
    pub length: u64,

    /// MD5 sum（可选）
    pub md5sum: Option<String>,
}

impl TorrentMetaInfo {
    /// 创建新的 torrent 元信息（单文件）
    pub fn new_single_file(
        file_path: &Path,
        piece_length: Option<u32>,
    ) -> Result<Self> {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::Other("Invalid file name".to_string()))?;

        let metadata = std::fs::metadata(file_path)?;
        let file_length = metadata.len();

        let piece_length = piece_length.unwrap_or(DEFAULT_PIECE_LENGTH);

        // 计算 pieces hash
        let pieces = Self::compute_pieces(file_path, piece_length, file_length)?;

        let info = TorrentInfo {
            name: file_name.to_string(),
            piece_length,
            pieces,
            length: Some(file_length),
            files: None,
            private: Some(1), // 私有 torrent，仅通过 mDNS 发现
            md5sum: None,
        };

        Ok(TorrentMetaInfo {
            announce: None, // 局域网环境不需要 tracker
            created_by: Some(format!("sharSelf/{}", env!("CARGO_PKG_VERSION"))),
            creation_date: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0)
            ),
            encoding: Some("UTF-8".to_string()),
            info,
        })
    }

    /// 创建新的 torrent 元信息（多文件/目录）
    pub fn new_directory(
        dir_path: &Path,
        piece_length: Option<u32>,
    ) -> Result<Self> {
        let dir_name = dir_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::Other("Invalid directory name".to_string()))?;

        let piece_length = piece_length.unwrap_or(DEFAULT_PIECE_LENGTH);

        // 收集所有文件
        let mut files = Vec::new();
        let mut total_length = 0u64;

        for entry in walkdir::WalkDir::new(dir_path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let path = entry.path();
                let metadata = std::fs::metadata(path)?;
                let length = metadata.len();

                // 计算相对路径
                let relative_path = path
                    .strip_prefix(dir_path)
                    .map_err(|_| Error::Other("Failed to compute relative path".to_string()))?;

                let path_vec: Vec<String> = relative_path
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .collect();

                files.push(FileInfo {
                    path: path_vec,
                    length,
                    md5sum: None,
                });

                total_length += length;
            }
        }

        if files.is_empty() {
            return Err(Error::Other("No files found in directory".to_string()));
        }

        // 计算 pieces hash
        let pieces = Self::compute_pieces_for_directory(dir_path, piece_length, &files)?;

        let info = TorrentInfo {
            name: dir_name.to_string(),
            piece_length,
            pieces,
            length: None,
            files: Some(files),
            private: Some(1),
            md5sum: None,
        };

        Ok(TorrentMetaInfo {
            announce: None,
            created_by: Some(format!("sharSelf/{}", env!("CARGO_PKG_VERSION"))),
            creation_date: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0)
            ),
            encoding: Some("UTF-8".to_string()),
            info,
        })
    }

    /// 计算 pieces hash
    fn compute_pieces(
        file_path: &Path,
        piece_length: u32,
        file_length: u64,
    ) -> Result<Vec<u8>> {
        use sha1::Sha1;
        use std::io::Read;

        let mut file = File::open(file_path)?;
        let mut pieces = Vec::new();
        let mut buffer = vec![0u8; piece_length as usize];

        loop {
            let mut total_read = 0;
            while total_read < piece_length as usize {
                let n = file.read(&mut buffer[total_read..])?;
                if n == 0 {
                    break;
                }
                total_read += n;
            }

            if total_read == 0 {
                break;
            }

            let hash = sha1::Sha1::digest(&buffer[..total_read]);
            pieces.extend_from_slice(&hash);

            if total_read < piece_length as usize {
                break;
            }
        }

        Ok(pieces)
    }

    /// 计算多文件模式的 pieces hash
    fn compute_pieces_for_directory(
        dir_path: &Path,
        piece_length: u32,
        files: &[FileInfo],
    ) -> Result<Vec<u8>> {
        use sha1::Sha1;
        use std::io::Read;

        let mut pieces = Vec::new();
        let mut buffer = vec![0u8; piece_length as usize];
        let mut current_piece = Vec::new();
        let mut total_read = 0u64;

        for file_info in files {
            // 构建完整文件路径
            let mut file_path = dir_path.to_path_buf();
            for component in &file_info.path {
                file_path.push(component);
            }

            let mut file = File::open(&file_path)?;
            let mut file_remaining = file_info.length;

            while file_remaining > 0 {
                let piece_remaining = piece_length as u64 - (total_read % piece_length as u64);
                let to_read = std::cmp::min(piece_remaining, file_remaining) as usize;

                let n = file.read(&mut buffer[..to_read])?;
                if n == 0 {
                    break;
                }

                current_piece.extend_from_slice(&buffer[..n]);
                total_read += n as u64;
                file_remaining -= n as u64;

                // 如果 piece 已满，计算 hash
                if total_read % piece_length as u64 == 0 {
                    let hash = sha1::Sha1::digest(&current_piece);
                    pieces.extend_from_slice(&hash);
                    current_piece.clear();
                }
            }
        }

        // 处理最后一个不完整的 piece
        if !current_piece.is_empty() {
            let hash = sha1::Sha1::digest(&current_piece);
            pieces.extend_from_slice(&hash);
        }

        Ok(pieces)
    }

    /// 编码为 bencode 格式
    pub fn to_bencode(&self) -> Result<Vec<u8>> {
        let mut dict = BTreeMap::new();

        if let Some(announce) = &self.announce {
            dict.insert(b"announce".to_vec(), BencodeValue::Bytes(announce.as_bytes().to_vec()));
        }

        if let Some(created_by) = &self.created_by {
            dict.insert(b"created by".to_vec(), BencodeValue::Bytes(created_by.as_bytes().to_vec()));
        }

        if let Some(creation_date) = &self.creation_date {
            dict.insert(b"creation date".to_vec(), BencodeValue::Int(*creation_date));
        }

        if let Some(encoding) = &self.encoding {
            dict.insert(b"encoding".to_vec(), BencodeValue::Bytes(encoding.as_bytes().to_vec()));
        }

        // 编码 info 字典
        let mut info_dict = BTreeMap::new();
        info_dict.insert(b"name".to_vec(), BencodeValue::Bytes(self.info.name.as_bytes().to_vec()));
        info_dict.insert(b"piece length".to_vec(), BencodeValue::Int(self.info.piece_length as i64));
        info_dict.insert(b"pieces".to_vec(), BencodeValue::Bytes(self.info.pieces.clone()));

        if let Some(length) = &self.info.length {
            info_dict.insert(b"length".to_vec(), BencodeValue::Int(*length as i64));
        }

        if let Some(files) = &self.info.files {
            let mut files_list = Vec::new();
            for file in files {
                let mut file_dict = BTreeMap::new();
                file_dict.insert(b"length".to_vec(), BencodeValue::Int(file.length as i64));

                let path_list: Vec<BencodeValue> = file.path.iter()
                    .map(|s| BencodeValue::Bytes(s.as_bytes().to_vec()))
                    .collect();
                file_dict.insert(b"path".to_vec(), BencodeValue::List(path_list));

                files_list.push(BencodeValue::Dict(file_dict));
            }
            info_dict.insert(b"files".to_vec(), BencodeValue::List(files_list));
        }

        if let Some(private) = &self.info.private {
            info_dict.insert(b"private".to_vec(), BencodeValue::Int(*private as i64));
        }

        dict.insert(b"info".to_vec(), BencodeValue::Dict(info_dict));

        // 编码
        Ok(BencodeValue::Dict(dict).encode())
    }

    /// 从 bencode 数据解析（简化版本，仅支持基本功能）
    pub fn from_bencode(data: &[u8]) -> Result<Self> {
        // 简化的解析 - 实际应用中需要完整的 bencode 解析器
        // 这里返回一个基本的实现，主要用于创建 torrent
        Err(Error::Other("Bencode decoding not implemented yet".to_string()))
    }

    /// 获取 piece 数量
    pub fn piece_count(&self) -> usize {
        self.info.pieces.len() / 20
    }

    /// 获取指定索引的 piece hash
    pub fn piece_hash(&self, index: usize) -> Option<[u8; 20]> {
        let start = index * 20;
        if start + 20 > self.info.pieces.len() {
            return None;
        }

        let mut hash = [0u8; 20];
        hash.copy_from_slice(&self.info.pieces[start..start + 20]);
        Some(hash)
    }

    /// 获取总大小
    pub fn total_size(&self) -> u64 {
        if let Some(length) = self.info.length {
            length
        } else if let Some(files) = &self.info.files {
            files.iter().map(|f| f.length).sum()
        } else {
            0
        }
    }

    /// 是否为单文件 torrent
    pub fn is_single_file(&self) -> bool {
        self.info.length.is_some()
    }

    /// 生成 info hash（用于唯一标识 torrent）
    pub fn info_hash(&self) -> Result<[u8; 20]> {
        use sha1::{Sha1, Digest};

        // 编码 info 字典
        let mut info_dict = BTreeMap::new();
        info_dict.insert(b"name".to_vec(), BencodeValue::Bytes(self.info.name.as_bytes().to_vec()));
        info_dict.insert(b"piece length".to_vec(), BencodeValue::Int(self.info.piece_length as i64));
        info_dict.insert(b"pieces".to_vec(), BencodeValue::Bytes(self.info.pieces.clone()));

        if let Some(length) = &self.info.length {
            info_dict.insert(b"length".to_vec(), BencodeValue::Int(*length as i64));
        }

        if let Some(files) = &self.info.files {
            let mut files_list = Vec::new();
            for file in files {
                let mut file_dict = BTreeMap::new();
                file_dict.insert(b"length".to_vec(), BencodeValue::Int(file.length as i64));

                let path_list: Vec<BencodeValue> = file.path.iter()
                    .map(|s| BencodeValue::Bytes(s.as_bytes().to_vec()))
                    .collect();
                file_dict.insert(b"path".to_vec(), BencodeValue::List(path_list));

                files_list.push(BencodeValue::Dict(file_dict));
            }
            info_dict.insert(b"files".to_vec(), BencodeValue::List(files_list));
        }

        if let Some(private) = &self.info.private {
            info_dict.insert(b"private".to_vec(), BencodeValue::Int(*private as i64));
        }

        let info_bytes = BencodeValue::Dict(info_dict).encode();

        let mut hash = [0u8; 20];
        hash.copy_from_slice(&Sha1::digest(&info_bytes));
        Ok(hash)
    }

    /// 保存到文件
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let data = self.to_bencode()?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// 从文件加载
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let data = std::fs::read(path)?;
        Self::from_bencode(&data)
    }
}

/// Torrent 文件（封装元信息和额外信息）
#[derive(Debug, Clone)]
pub struct TorrentFile {
    /// 元信息
    pub metainfo: TorrentMetaInfo,

    /// 本地文件路径（下载/上传时使用）
    pub local_path: PathBuf,

    /// Torrent 数据（用于通过 mDNS 分发）
    pub torrent_data: Vec<u8>,
}

impl TorrentFile {
    /// 创建新的 torrent 文件
    pub fn create(path: &Path, piece_length: Option<u32>) -> Result<Self> {
        let metadata = if path.is_dir() {
            TorrentMetaInfo::new_directory(path, piece_length)?
        } else {
            TorrentMetaInfo::new_single_file(path, piece_length)?
        };

        let torrent_data = metadata.to_bencode()?;

        Ok(TorrentFile {
            metainfo: metadata,
            local_path: path.to_path_buf(),
            torrent_data,
        })
    }

    /// 从 bencode 数据创建
    pub fn from_bencode(data: Vec<u8>, download_path: PathBuf) -> Result<Self> {
        let metainfo = TorrentMetaInfo::from_bencode(&data)?;
        Ok(TorrentFile {
            metainfo,
            local_path: download_path,
            torrent_data: data,
        })
    }

    /// 获取 info hash
    pub fn info_hash(&self) -> Result<[u8; 20]> {
        self.metainfo.info_hash()
    }

    /// 获取 piece 数量
    pub fn piece_count(&self) -> usize {
        self.metainfo.piece_count()
    }
}
