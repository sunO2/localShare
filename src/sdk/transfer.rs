//! 文件传输模块封装
//!
//! 处理文件的上传和下载，支持暂停、恢复、取消

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use super::{types::*, state::StateHandle, events::EventSender, SDKResult, SDKError};

/// 传输管理器
pub struct TransferModule {
    state: StateHandle,
    event_tx: EventSender,
}

impl TransferModule {
    pub fn new(state: StateHandle, event_tx: EventSender) -> Self {
        Self {
            state,
            event_tx,
        }
    }

    /// 发送文件到远程设备
    pub async fn send_file(
        &self,
        device_id: &str,
        file_path: &str,
    ) -> SDKResult<String> {
        // 验证设备是否存在
        let devices = self.state.get_devices();
        let device = devices.iter()
            .find(|d| d.id == device_id)
            .ok_or_else(|| SDKError::NotFound(format!("Device not found: {}", device_id)))?;

        // 验证文件是否存在
        let path = PathBuf::from(file_path);
        if !path.exists() {
            return Err(SDKError::NotFound(format!("File not found: {}", path.display())));
        }

        let metadata = std::fs::metadata(&path)
            .map_err(|e| SDKError::Io(e))?;
        let file_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // 创建传输记录
        let transfer_id = generate_transfer_id();
        let transfer_info = TransferInfo {
            id: transfer_id.clone(),
            direction: TransferDirection::Upload,
            file_name: file_name.clone(),
            file_size: metadata.len(),
            transferred: 0,
            status: TransferStatus::Pending,
            remote_device: device_id.to_string(),
            local_path: path.clone(),
            error_message: None,
        };

        self.state.add_transfer(transfer_info.clone());
        self.event_tx.send(SDKEvent::SendStarted(transfer_id.clone()));

        // 启动上传任务
        let state = self.state.clone();
        let event_tx = self.event_tx.clone();
        let transfer_id_clone = transfer_id.clone();
        let device_addr = device.addresses.first()
            .cloned()
            .unwrap_or_else(|| format!("{}:{}", device.hostname, device.port));

        tokio::spawn(async move {
            Self::do_upload(
                state,
                event_tx,
                transfer_id_clone,
                path,
                file_name,
                device_addr,
            ).await;
        });

        Ok(transfer_id)
    }

    /// 从远程设备下载文件
    pub async fn download_file(
        &self,
        device_id: &str,
        file_id: &str,
        save_path: &str,
    ) -> SDKResult<String> {
        // 验证设备是否存在
        let devices = self.state.get_devices();
        let device = devices.iter()
            .find(|d| d.id == device_id)
            .ok_or_else(|| SDKError::NotFound(format!("Device not found: {}", device_id)))?;

        // 验证保存路径
        let save_path = PathBuf::from(save_path);
        if let Some(parent) = save_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| SDKError::Io(e))?;
            }
        }

        // 创建传输记录
        let transfer_id = generate_transfer_id();
        let transfer_info = TransferInfo {
            id: transfer_id.clone(),
            direction: TransferDirection::Download,
            file_name: file_id.to_string(),
            file_size: 0, // 从服务器获取
            transferred: 0,
            status: TransferStatus::Pending,
            remote_device: device_id.to_string(),
            local_path: save_path.clone(),
            error_message: None,
        };

        self.state.add_transfer(transfer_info.clone());
        self.event_tx.send(SDKEvent::DownloadStarted(transfer_id.clone()));

        // 启动下载任务
        let state = self.state.clone();
        let event_tx = self.event_tx.clone();
        let transfer_id_clone = transfer_id.clone();
        let device_addr = device.addresses.first()
            .cloned()
            .unwrap_or_else(|| format!("{}:{}", device.hostname, device.port));

        tokio::spawn(async move {
            Self::do_download(
                state,
                event_tx,
                transfer_id_clone,
                file_id.to_string(),
                save_path,
                device_addr,
            ).await;
        });

        Ok(transfer_id)
    }

    /// 暂停传输
    pub fn pause_transfer(&self, transfer_id: &str) -> SDKResult<()> {
        if self.state.pause_transfer(transfer_id) {
            let transfer = self.state.get_transfer(transfer_id)
                .ok_or_else(|| SDKError::NotFound(format!("Transfer not found: {}", transfer_id)))?;

            match transfer.direction {
                TransferDirection::Upload => {
                    // 上传暂不实现暂停
                }
                TransferDirection::Download => {
                    // 下载暂不实现暂停
                }
            }
            Ok(())
        } else {
            Err(SDKError::NotFound(format!("Transfer not found: {}", transfer_id)))
        }
    }

    /// 恢复传输
    pub fn resume_transfer(&self, transfer_id: &str) -> SDKResult<()> {
        if self.state.resume_transfer(transfer_id) {
            Ok(())
        } else {
            Err(SDKError::NotFound(format!("Transfer not found: {}", transfer_id)))
        }
    }

    /// 取消传输
    pub fn cancel_transfer(&self, transfer_id: &str) -> SDKResult<()> {
        if self.state.remove_transfer(transfer_id) {
            self.event_tx.send(SDKEvent::Error(format!("Transfer cancelled: {}", transfer_id)));
            Ok(())
        } else {
            Err(SDKError::NotFound(format!("Transfer not found: {}", transfer_id)))
        }
    }

    /// 获取传输信息
    pub fn get_transfer(&self, transfer_id: &str) -> Option<TransferInfo> {
        self.state.get_transfer(transfer_id)
    }

    /// 获取所有传输
    pub fn get_all_transfers(&self) -> Vec<TransferInfo> {
        self.state.get_all_transfers()
    }

    /// 执行文件上传
    async fn do_upload(
        state: StateHandle,
        event_tx: EventSender,
        transfer_id: String,
        file_path: PathBuf,
        file_name: String,
        device_addr: String,
    ) {
        // 更新状态为准备中
        state.set_transfer_status(&transfer_id, TransferStatus::Preparing);

        let result = async {
            // 读取文件
            let file_content = tokio::fs::read(&file_path).await?;
            let total = file_content.len() as u64;

            // 构造上传 URL
            let url = format!("http://{}/upload", device_addr);

            // 创建 multipart form data
            let part = reqwest::multipart::Part::bytes(file_content)
                .file_name(file_name.clone());
            let form = reqwest::multipart::Form::new()
                .part("file", part);

            // 发送请求
            let client = reqwest::Client::new();
            let response = client.post(&url)
                .multipart(form)
                .send()
                .await?;

            if response.status().is_success() {
                Ok(total)
            } else {
                Err(format!("Upload failed with status: {}", response.status()).into())
            }
        }.await;

        match result {
            Ok(total) => {
                state.update_transfer_progress(&transfer_id, total);
                state.set_transfer_status(&transfer_id, TransferStatus::Completed);
                event_tx.send(SDKEvent::SendCompleted(transfer_id));
            }
            Err(e) => {
                let error_msg = format!("{:?}", e);
                state.set_transfer_status(&transfer_id, TransferStatus::Failed(error_msg.clone()));
                event_tx.send(SDKEvent::SendFailed(transfer_id, error_msg));
            }
        }
    }

    /// 执行文件下载
    async fn do_download(
        state: StateHandle,
        event_tx: EventSender,
        transfer_id: String,
        file_id: String,
        save_path: PathBuf,
        device_addr: String,
    ) {
        // 更新状态为准备中
        state.set_transfer_status(&transfer_id, TransferStatus::Preparing);

        let result = async {
            // 构造下载 URL
            let url = format!("http://{}/download/{}", device_addr, file_id);

            // 下载文件
            let client = reqwest::Client::new();
            let response = client.get(&url)
                .send()
                .await?;

            if !response.status().is_success() {
                return Err(format!("Download failed with status: {}", response.status()).into());
            }

            let total = response.content_length().unwrap_or(0);
            let mut downloaded = 0u64;
            let mut bytes = vec![];

            while let Some(chunk) = response.chunk().await? {
                downloaded += chunk.len() as u64;
                bytes.extend_from_slice(&chunk);

                // 更新进度
                state.update_transfer_progress(&transfer_id, downloaded);

                if downloaded % (1024 * 100) == 0 {
                    event_tx.send(SDKEvent::DownloadProgress(
                        transfer_id.clone(),
                        downloaded,
                        total,
                    ));
                }
            }

            // 保存文件
            tokio::fs::write(&save_path, bytes).await?;

            Ok(total)
        }.await;

        match result {
            Ok(_) => {
                state.set_transfer_status(&transfer_id, TransferStatus::Completed);
                let path_str = save_path.to_string_lossy().to_string();
                event_tx.send(SDKEvent::DownloadCompleted(transfer_id, path_str));
            }
            Err(e) => {
                let error_msg = format!("{:?}", e);
                state.set_transfer_status(&transfer_id, TransferStatus::Failed(error_msg.clone()));
                event_tx.send(SDKEvent::DownloadFailed(transfer_id, error_msg));
            }
        }
    }
}

/// 生成传输 ID
fn generate_transfer_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("transfer-{}-{:x}", timestamp, rand::random::<u32>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_id_generation() {
        let id = generate_transfer_id();
        assert!(id.starts_with("transfer-"));
        assert!(id.len() > 10);
    }
}
