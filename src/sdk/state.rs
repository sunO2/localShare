//! SDK 状态管理
//!
//! 管理 SDK 的内部状态，包括设备、文件、传输等

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use super::types::{DeviceInfo, FileInfo, TransferInfo, TransferStatus, TransferDirection};

/// SDK 全局状态
#[derive(Debug)]
pub struct SDKState {
    // 运行状态
    pub discovery_running: bool,
    pub server_running: bool,

    // 设备和文件
    pub devices: Vec<DeviceInfo>,
    pub shared_files: Vec<FileInfo>,

    // 传输管理
    pub transfers: HashMap<String, TransferState>,
}

/// 传输状态
#[derive(Debug, Clone)]
pub struct TransferState {
    pub info: TransferInfo,
    pub paused: bool,
    pub created_at: std::time::Instant,
    pub updated_at: std::time::Instant,
}

impl TransferState {
    pub fn new(info: TransferInfo) -> Self {
        let now = std::time::Instant::now();
        Self {
            info,
            paused: false,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn update_progress(&mut self, transferred: u64) {
        self.info.transferred = transferred;
        self.updated_at = std::time::Instant::now();
    }

    pub fn set_status(&mut self, status: TransferStatus) {
        self.info.status = status;
        self.updated_at = std::time::Instant::now();
    }
}

impl Default for SDKState {
    fn default() -> Self {
        Self {
            discovery_running: false,
            server_running: false,
            devices: Vec::new(),
            shared_files: Vec::new(),
            transfers: HashMap::new(),
        }
    }
}

/// 状态句柄，用于线程安全访问
#[derive(Debug, Clone)]
pub struct StateHandle {
    inner: Arc<RwLock<SDKState>>,
}

impl StateHandle {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(SDKState::default())),
        }
    }

    // ========== 设备管理 ==========

    pub fn add_device(&self, device: DeviceInfo) {
        let mut state = self.inner.write().unwrap();
        if !state.devices.iter().any(|d| d.id == device.id) {
            state.devices.push(device);
        }
    }

    pub fn remove_device(&self, id: &str) {
        let mut state = self.inner.write().unwrap();
        state.devices.retain(|d| d.id != id);
    }

    pub fn get_devices(&self) -> Vec<DeviceInfo> {
        let state = self.inner.read().unwrap();
        state.devices.clone()
    }

    // ========== 共享文件管理 ==========

    pub fn add_shared_file(&self, file: FileInfo) {
        let mut state = self.inner.write().unwrap();
        if !state.shared_files.iter().any(|f| f.id == file.id) {
            state.shared_files.push(file);
        }
    }

    pub fn remove_shared_file(&self, path: &std::path::Path) -> bool {
        let mut state = self.inner.write().unwrap();
        let len = state.shared_files.len();
        state.shared_files.retain(|f| f.path != path);
        state.shared_files.len() < len
    }

    pub fn get_shared_files(&self) -> Vec<FileInfo> {
        let state = self.inner.read().unwrap();
        state.shared_files.clone()
    }

    pub fn clear_shared_files(&self) {
        let mut state = self.inner.write().unwrap();
        state.shared_files.clear();
    }

    // ========== 传输管理 ==========

    pub fn add_transfer(&self, transfer: TransferInfo) {
        let mut state = self.inner.write().unwrap();
        let id = transfer.id.clone();
        state.transfers.insert(id, TransferState::new(transfer));
    }

    pub fn get_transfer(&self, id: &str) -> Option<TransferInfo> {
        let state = self.inner.read().unwrap();
        state.transfers.get(id).map(|t| t.info.clone())
    }

    pub fn get_all_transfers(&self) -> Vec<TransferInfo> {
        let state = self.inner.read().unwrap();
        state.transfers.values().map(|t| t.info.clone()).collect()
    }

    pub fn get_transfers_by_direction(&self, direction: TransferDirection) -> Vec<TransferInfo> {
        let state = self.inner.read().unwrap();
        state.transfers.values()
            .filter(|t| t.info.direction == direction)
            .map(|t| t.info.clone())
            .collect()
    }

    pub fn update_transfer_progress(&self, id: &str, transferred: u64) -> bool {
        let mut state = self.inner.write().unwrap();
        if let Some(transfer) = state.transfers.get_mut(id) {
            transfer.update_progress(transferred);
            true
        } else {
            false
        }
    }

    pub fn set_transfer_status(&self, id: &str, status: TransferStatus) -> bool {
        let mut state = self.inner.write().unwrap();
        if let Some(transfer) = state.transfers.get_mut(id) {
            transfer.set_status(status);
            true
        } else {
            false
        }
    }

    pub fn pause_transfer(&self, id: &str) -> bool {
        let mut state = self.inner.write().unwrap();
        if let Some(transfer) = state.transfers.get_mut(id) {
            transfer.paused = true;
            transfer.set_status(TransferStatus::Paused);
            true
        } else {
            false
        }
    }

    pub fn resume_transfer(&self, id: &str) -> bool {
        let mut state = self.inner.write().unwrap();
        if let Some(transfer) = state.transfers.get_mut(id) {
            transfer.paused = false;
            transfer.set_status(TransferStatus::Transferring);
            true
        } else {
            false
        }
    }

    pub fn remove_transfer(&self, id: &str) -> bool {
        let mut state = self.inner.write().unwrap();
        state.transfers.remove(id).is_some()
    }

    // ========== 运行状态 ==========

    pub fn set_discovery_running(&self, running: bool) {
        let mut state = self.inner.write().unwrap();
        state.discovery_running = running;
    }

    pub fn is_discovery_running(&self) -> bool {
        let state = self.inner.read().unwrap();
        state.discovery_running
    }

    pub fn set_server_running(&self, running: bool) {
        let mut state = self.inner.write().unwrap();
        state.server_running = running;
    }

    pub fn is_server_running(&self) -> bool {
        let state = self.inner.read().unwrap();
        state.server_running
    }

    pub fn get_server_port(&self) -> Option<u16> {
        // TODO: 从配置中获取
        None
    }
}

impl Default for StateHandle {
    fn default() -> Self {
        Self::new()
    }
}
