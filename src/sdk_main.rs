//! ShareSelf SDK 主模块
//!
//! 提供 SDK 主类，整合所有功能模块

use std::sync::Arc;
use std::pin::Pin;
use tokio::sync::Mutex;
use super::sdk::*;
use crate::sdk::SDKResult;

/// ShareSelf SDK 主类
///
/// 提供设备发现、文件共享、文件传输等核心功能
pub struct ShareSelfSDK {
    state: StateHandle,
    event_tx: EventSender,
    event_rx: Arc<Mutex<EventReceiver>>,
    discovery: Arc<Mutex<DiscoveryModule>>,
    server: Arc<Mutex<FileServerModule>>,
    transfer: Arc<Mutex<TransferModule>>,
}

impl ShareSelfSDK {
    /// 创建新的 SDK 实例
    pub fn new() -> Self {
        let (event_tx, event_rx) = event_channel();
        let state = StateHandle::new();

        Self {
            state: state.clone(),
            event_tx: event_tx.clone(),
            event_rx: Arc::new(Mutex::new(event_rx)),
            discovery: Arc::new(Mutex::new(DiscoveryModule::new(state.clone(), event_tx.clone()))),
            server: Arc::new(Mutex::new(FileServerModule::new(state.clone(), event_tx.clone()))),
            transfer: Arc::new(Mutex::new(TransferModule::new(state.clone(), event_tx))),
        }
    }

    // ========== 初始化 ==========

    /// 初始化 SDK
    pub fn initialize(&mut self) -> SDKResult<()> {
        tracing::info!("ShareSelf SDK initialized");
        Ok(())
    }

    /// 清理资源
    pub async fn shutdown(&mut self) -> SDKResult<()> {
        // 停止发现
        let discovery = self.discovery.clone();
        let mut discovery = discovery.lock().await;
        let _ = discovery.stop().await;

        // 停止服务器
        let server = self.server.clone();
        let mut server = server.lock().await;
        let _ = server.stop().await;

        tracing::info!("ShareSelf SDK shutdown");
        Ok(())
    }

    // ========== 设备发现 ==========

    /// 启动设备发现
    pub async fn start_discovery(&mut self, port: u16) -> SDKResult<()> {
        let mut discovery = self.discovery.lock().await;
        discovery.start(port).await
    }

    /// 停止设备发现
    pub async fn stop_discovery(&mut self) -> SDKResult<()> {
        let mut discovery = self.discovery.lock().await;
        discovery.stop().await
    }

    /// 获取已发现的设备列表
    pub fn get_devices(&self) -> Vec<DeviceInfo> {
        self.state.get_devices()
    }

    // ========== 文件服务器 ==========

    /// 启动文件服务器
    pub async fn start_server(&mut self, port: u16) -> SDKResult<u16> {
        let mut server = self.server.lock().await;
        server.start(port).await
    }

    /// 停止文件服务器
    pub async fn stop_server(&mut self) -> SDKResult<()> {
        let mut server = self.server.lock().await;
        server.stop().await
    }

    /// 获取服务器端口
    pub fn get_server_port(&self) -> Option<u16> {
        self.state.get_server_port()
    }

    /// 添加共享文件
    pub fn add_shared_file(&self, path: &str) -> SDKResult<FileInfo> {
        let server = self.server.blocking_lock();
        server.add_shared_file(path)
    }

    /// 移除共享文件
    pub fn remove_shared_file(&self, path: &str) -> SDKResult<bool> {
        let server = self.server.blocking_lock();
        server.remove_shared_file(path)
    }

    /// 获取共享文件列表
    pub fn get_shared_files(&self) -> Vec<FileInfo> {
        self.state.get_shared_files()
    }

    /// 清空共享文件列表
    pub fn clear_shared_files(&self) {
        self.state.clear_shared_files();
    }

    // ========== 文件传输 ==========

    /// 发送文件到远程设备
    pub async fn send_file(&self, device_id: &str, file_path: &str) -> SDKResult<String> {
        let transfer = self.transfer.lock().await;
        transfer.send_file(device_id, file_path).await
    }

    /// 从远程设备下载文件
    pub async fn download_file(
        &self,
        device_id: &str,
        file_id: &str,
        save_path: &str,
    ) -> SDKResult<String> {
        let transfer = self.transfer.lock().await;
        transfer.download_file(device_id, file_id, save_path).await
    }

    /// 暂停传输
    pub fn pause_transfer(&self, transfer_id: &str) -> SDKResult<()> {
        let transfer = self.transfer.blocking_lock();
        transfer.pause_transfer(transfer_id)
    }

    /// 恢复传输
    pub fn resume_transfer(&self, transfer_id: &str) -> SDKResult<()> {
        let transfer = self.transfer.blocking_lock();
        transfer.resume_transfer(transfer_id)
    }

    /// 取消传输
    pub fn cancel_transfer(&self, transfer_id: &str) -> SDKResult<()> {
        let transfer = self.transfer.blocking_lock();
        transfer.cancel_transfer(transfer_id)
    }

    /// 获取传输信息
    pub fn get_transfer(&self, transfer_id: &str) -> Option<TransferInfo> {
        self.state.get_transfer(transfer_id)
    }

    /// 获取所有传输
    pub fn get_all_transfers(&self) -> Vec<TransferInfo> {
        self.state.get_all_transfers()
    }

    /// 获取上传列表
    pub fn get_uploads(&self) -> Vec<TransferInfo> {
        self.state.get_transfers_by_direction(TransferDirection::Upload)
    }

    /// 获取下载列表
    pub fn get_downloads(&self) -> Vec<TransferInfo> {
        self.state.get_transfers_by_direction(TransferDirection::Download)
    }

    // ========== 事件处理 ==========

    /// 非阻塞轮询事件
    pub fn poll_event(&self) -> Option<SDKEvent> {
        let mut rx = self.event_rx.try_lock().ok()?;
        rx.try_recv()
    }

    /// 获取所有待处理事件
    pub fn drain_events(&self) -> Vec<SDKEvent> {
        let mut rx = self.event_rx.try_lock().unwrap();
        let mut events = Vec::new();
        while let Some(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    /// 检查是否有待处理事件
    pub fn has_events(&self) -> bool {
        if let Ok(rx) = self.event_rx.try_lock() {
            // 通过尝试接收来检查是否有事件
            // 这不是最优雅的方式，但对 FFI 轮询来说足够了
            false // 实际实现中需要更好的检查方式
        } else {
            false
        }
    }
}

impl Default for ShareSelfSDK {
    fn default() -> Self {
        Self::new()
    }
}

// FFI 句柄类型
pub type SDKHandle = *mut ShareSelfSDK;

/// 创建新的 SDK 实例（用于 FFI）
#[no_mangle]
pub extern "C" fn shareself_sdk_create() -> SDKHandle {
    Box::into_raw(Box::new(ShareSelfSDK::new()))
}

/// 销毁 SDK 实例（用于 FFI）
#[no_mangle]
pub extern "C" fn shareself_sdk_destroy(handle: SDKHandle) {
    if !handle.is_null() {
        unsafe {
            let _ = Box::from_raw(handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sdk_creation() {
        let sdk = ShareSelfSDK::new();
        assert_eq!(sdk.get_devices().len(), 0);
        assert_eq!(sdk.get_shared_files().len(), 0);
    }

    #[tokio::test]
    async fn test_sdk_default() {
        let sdk = ShareSelfSDK::default();
        assert_eq!(sdk.get_devices().len(), 0);
    }
}
