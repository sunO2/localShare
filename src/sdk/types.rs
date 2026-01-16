//! SDK 类型定义
//!
//! 定义 SDK 使用的数据结构和枚举类型

use serde::{Deserialize, Serialize};

/// 传输方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// 传输状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferStatus {
    Pending,
    Preparing,
    Transferring,
    Paused,
    Completed,
    Failed(String),
    Cancelled,
}

impl TransferStatus {
    /// 是否为终态
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed(_) | Self::Cancelled)
    }

    /// 是否可以暂停
    pub fn can_pause(&self) -> bool {
        matches!(self, Self::Transferring | Self::Preparing)
    }
}

/// 发现来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoverySource {
    MDNS,
    Broadcast,
    HTTPScan,
}

/// 设备信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub addresses: Vec<String>,
    pub port: u16,
    pub service_type: String,
    pub source: DiscoverySource,
    pub last_seen: std::time::Instant,
}

/// 文件信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub id: String,
    pub name: String,
    pub path: std::path::PathBuf,
    pub size: u64,
    pub mime_type: String,
    pub hash: String,
}

/// 传输信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferInfo {
    pub id: String,
    pub direction: TransferDirection,
    pub file_name: String,
    pub file_size: u64,
    pub transferred: u64,
    pub status: TransferStatus,
    pub remote_device: String,
    pub local_path: std::path::PathBuf,
    pub error_message: Option<String>,
}

impl TransferInfo {
    /// 计算进度百分比
    pub fn percentage(&self) -> u8 {
        if self.file_size == 0 {
            return 0;
        }
        ((self.transferred as f64 / self.file_size as f64) * 100.0) as u8
    }

    /// 是否完成
    pub fn is_completed(&self) -> bool {
        matches!(self.status, TransferStatus::Completed)
    }
}

/// 传输进度
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub transferred: u64,
    pub total: u64,
    pub percentage: u8,
    pub speed: u64,        // bytes/s
    pub remaining: u64,    // 秒
}

/// SDK 事件
#[derive(Debug, Clone)]
pub enum SDKEvent {
    // 设备事件
    DeviceFound(DeviceInfo),
    DeviceLost(String),

    // 发送事件
    SendStarted(String),                   // transfer_id
    SendProgress(String, u64, u64),       // id, transferred, total
    SendCompleted(String),
    SendFailed(String, String),            // id, error

    // 下载事件
    DownloadStarted(String),
    DownloadProgress(String, u64, u64),
    DownloadCompleted(String, String),    // id, save_path
    DownloadFailed(String, String),

    // 服务器事件
    ServerStarted(u16),
    ServerStopped,

    // 错误
    Error(String),
}

/// SDK 错误
#[derive(Debug, thiserror::Error)]
pub enum SDKError {
    #[error("SDK not initialized")]
    NotInitialized,

    #[error("Already running")]
    AlreadyRunning,

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Discovery error: {0}")]
    Discovery(String),

    #[error("Transfer error: {0}")]
    Transfer(String),

    #[error("Server error: {0}")]
    Server(String),
}

pub type SDKResult<T> = Result<T, SDKError>;
