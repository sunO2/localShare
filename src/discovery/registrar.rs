//! 服务注册（主动广播自己）
//!
//! 负责将当前设备注册到局域网，使其他设备能够发现

use crate::common::{config::ServiceConfig, error::{Error, Result}};
use tokio::sync::oneshot;

/// 服务注册句柄
///
/// 用于控制已注册的服务
pub struct ServiceHandle {
    /// 服务名称
    service_name: String,

    /// 关闭信号发送器
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl ServiceHandle {
    /// 获取服务名称
    pub fn name(&self) -> &str {
        &self.service_name
    }

    /// 更新 TXT 记录
    pub async fn update_txt(&mut self, records: std::collections::HashMap<String, String>) -> Result<()> {
        // TODO: 发送 mDNS 更新，包含新的 TXT 记录
        tracing::info!("Updating TXT records for service: {}", self.service_name);
        Ok(())
    }

    /// 注销服务
    pub async fn unregister(mut self) -> Result<()> {
        // TODO: 发送 mDNS 注销消息
        tracing::info!("Unregistering service: {}", self.service_name);

        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        Ok(())
    }
}

/// 注册当前设备到局域网
///
/// # Arguments
///
/// * `config` - 服务配置
///
/// # Returns
///
/// 返回服务句柄，用于控制已注册的服务
pub fn register_service(config: ServiceConfig) -> Result<ServiceHandle> {
    let (shutdown_tx, shutdown_rx) = oneshot::channel();

    // TODO: 启动 mDNS 广播任务
    // tokio::spawn(async move {
    //     run_registration(config, shutdown_rx).await;
    // });

    tracing::info!("Registering service: {} on port {}", config.service_name, config.port);

    Ok(ServiceHandle {
        service_name: config.service_name,
        shutdown_tx: Some(shutdown_tx),
    })
}

/// 运行服务注册的内部逻辑
async fn run_registration(
    config: ServiceConfig,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    // TODO: 实现 mDNS 广播循环
    // 1. 定期发送 mDNS 响应（通告）
    // 2. 响应 mDNS 查询
    // 3: 处理关闭信号

    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                tracing::info!("Service registration shutting down");
                // 发送注销消息
                break;
            }
        }
    }
}

/// 构建 mDNS 响应包
fn build_announcement(config: &ServiceConfig) -> Vec<u8> {
    // TODO: 构建 mDNS 响应数据包
    // 包含 PTR, SRV, TXT, A/AAAA 记录
    Vec::new()
}

/// 构建 mDNS 注销包
fn build_goodbye(config: &ServiceConfig) -> Vec<u8> {
    // TODO: 构建 mDNS 注销数据包（TTL=0）
    Vec::new()
}
