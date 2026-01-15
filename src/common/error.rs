//! 错误类型定义

use thiserror::Error;

/// sharSelf 库的错误类型
#[derive(Error, Debug, Clone)]
pub enum Error {
    /// mDNS 相关错误
    #[error("mDNS error: {0}")]
    Mdns(String),

    /// 网络错误
    #[error("network error: {0}")]
    Network(String),

    /// DNS 解析错误
    #[error("DNS error: {0}")]
    Dns(String),

    /// 服务注册失败
    #[error("service registration failed: {0}")]
    RegistrationFailed(String),

    /// 服务发现失败
    #[error("service discovery failed: {0}")]
    DiscoveryFailed(String),

    /// 配置错误
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    /// 超时
    #[error("operation timed out")]
    Timeout,

    /// 已取消
    #[error("operation cancelled")]
    Cancelled,

    /// 序列化错误
    #[error("serialization error: {0}")]
    Serialization(String),

    /// 其他错误
    #[error("unknown error: {0}")]
    Other(String),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Network(err.to_string())
    }
}

/// Result 类型别名
pub type Result<T> = std::result::Result<T, Error>;
