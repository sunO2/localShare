//! BitTorrent 文件传输模块
//!
//! 实现完整的 BitTorrent 协议用于局域网 P2P 文件传输

pub mod bencode;
pub mod metainfo;
pub mod piece;
pub mod protocol;
pub mod peer;
pub mod seeder;
pub mod downloader;
pub mod metadata;

pub use metainfo::{TorrentMetaInfo, TorrentFile, FileInfo};
pub use piece::{PieceManager, PieceState, Piece};
pub use seeder::Seeder;
pub use downloader::Downloader;
pub use metadata::MetadataServer;

/// 默认 Piece 大小 (256KB)
/// 局域网环境可以使用更大的 piece 以提高效率
pub const DEFAULT_PIECE_LENGTH: u32 = 256 * 1024;

/// BitTorrent 默认端口
pub const DEFAULT_BT_PORT: u16 = 6881;

/// Peer ID 前缀 (用于标识客户端)
pub const PEER_ID_PREFIX: &str = "-ST0001-"; // SharSelf Torrent v0.0.1
