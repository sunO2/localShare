//! 公共模块 - 错误类型、配置等共享组件

pub mod error;
pub mod config;

pub use error::{Error, Result};
pub use config::{DiscoveryConfig, ServiceConfig};
