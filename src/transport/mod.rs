//! 文件传输模块
//!
//! 提供高效的点对点文件传输功能（待实现）

/// 传输会话句柄
pub struct TransferHandle {
    // TODO: 实现传输会话管理
}

/// 文件传输进度
#[derive(Debug, Clone)]
pub struct TransferProgress {
    /// 已传输字节数
    pub transferred: u64,

    /// 总字节数
    pub total: u64,

    /// 百分比 (0.0 - 1.0)
    pub percentage: f32,

    /// 当前传输速率 (bytes/s)
    pub rate: f64,
}

impl TransferProgress {
    /// 创建新的进度
    pub fn new(total: u64) -> Self {
        Self {
            transferred: 0,
            total,
            percentage: 0.0,
            rate: 0.0,
        }
    }

    /// 更新进度
    pub fn update(&mut self, transferred: u64, rate: f64) {
        self.transferred = transferred;
        self.percentage = if self.total > 0 {
            (transferred as f32 / self.total as f32).min(1.0)
        } else {
            0.0
        };
        self.rate = rate;
    }

    /// 是否完成
    pub fn is_complete(&self) -> bool {
        self.transferred >= self.total && self.total > 0
    }
}

/// 传输方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    /// 上传（发送）
    Upload,

    /// 下载（接收）
    Download,
}

/// 传输状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferState {
    /// 等待中
    Pending,

    /// 传输中
    Transferring,

    /// 已暂停
    Paused,

    /// 已完成
    Completed,

    /// 已失败
    Failed,

    /// 已取消
    Cancelled,
}
