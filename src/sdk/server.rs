//! 文件服务器模块封装
//!
//! 提供 HTTP 文件服务器，支持文件上传和下载

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use super::{types::FileInfo, state::StateHandle, events::EventSender, SDKResult, SDKError};

/// 文件服务器配置
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_address: String,
    pub port: u16,
    pub upload_enabled: bool,
    pub download_enabled: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: 8080,
            upload_enabled: true,
            download_enabled: true,
        }
    }
}

/// 文件服务器
pub struct FileServerModule {
    handle: Option<ServerHandle>,
    state: StateHandle,
    event_tx: EventSender,
    config: ServerConfig,
}

struct ServerHandle {
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    port: u16,
}

impl FileServerModule {
    pub fn new(state: StateHandle, event_tx: EventSender) -> Self {
        Self {
            handle: None,
            state,
            event_tx,
            config: ServerConfig::default(),
        }
    }

    /// 启动文件服务器
    pub async fn start(&mut self, port: u16) -> SDKResult<u16> {
        if self.state.is_server_running() {
            return Err(SDKError::AlreadyRunning);
        }

        let config = ServerConfig {
            port,
            ..Default::default()
        };

        // 创建 HTTP 服务器
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
        let actual_port = start_http_server(config.clone(), self.state.clone(), self.event_tx.clone(), shutdown_tx.clone()).await?;

        self.handle = Some(ServerHandle {
            shutdown_tx,
            port: actual_port,
        });

        self.state.set_server_running(true);
        self.config = config;

        Ok(actual_port)
    }

    /// 停止文件服务器
    pub async fn stop(&mut self) -> SDKResult<()> {
        if !self.state.is_server_running() {
            return Err(SDKError::NotFound("Server not running".to_string()));
        }

        if let Some(handle) = self.handle.take() {
            let _ = handle.shutdown_tx.send(());
        }

        self.state.set_server_running(false);
        Ok(())
    }

    /// 添加共享文件
    pub fn add_shared_file(&self, path: &str) -> SDKResult<FileInfo> {
        let path = PathBuf::from(path);

        if !path.exists() {
            return Err(SDKError::NotFound(format!("File not found: {}", path.display())));
        }

        let metadata = std::fs::metadata(&path)
            .map_err(|e| SDKError::Io(e))?;

        let file_info = FileInfo {
            id: generate_file_id(&path),
            name: path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string(),
            path: path.clone(),
            size: metadata.len(),
            mime_type: mime_guess::from_path(&path)
                .first_or_octet_type()
                .to_string(),
            hash: calculate_file_hash(&path)?,
        };

        self.state.add_shared_file(file_info.clone());
        Ok(file_info)
    }

    /// 移除共享文件
    pub fn remove_shared_file(&self, path: &str) -> SDKResult<bool> {
        let path = PathBuf::from(path);
        Ok(self.state.remove_shared_file(&path))
    }

    /// 获取共享文件列表
    pub fn get_shared_files(&self) -> Vec<FileInfo> {
        self.state.get_shared_files()
    }

    /// 清空共享文件列表
    pub fn clear_shared_files(&self) {
        self.state.clear_shared_files();
    }

    /// 获取服务器端口
    pub fn get_port(&self) -> Option<u16> {
        self.handle.as_ref().map(|h| h.port)
    }
}

/// 启动 HTTP 服务器
async fn start_http_server(
    config: ServerConfig,
    state: StateHandle,
    event_tx: EventSender,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> SDKResult<u16> {
    use warp::Filter;

    // 文件列表路由
    let files_route = warp::path("files")
        .and(warp::path::end())
        .map({
            let state = state.clone();
            move || {
                let files = state.get_shared_files();
                warp::reply::json(&files)
            }
        });

    // 文件上传路由
    let upload_route = warp::path("upload")
        .and(warp::post())
        .and(warp::multipart::form())
        .map({
            let event_tx = event_tx.clone();
            move |form: warp::multipart::FormData| {
                // TODO: 处理文件上传
                event_tx.send(crate::sdk::types::SDKEvent::Error("Upload not implemented".to_string()));
                warp::reply::json(&"OK")
            }
        });

    // 文件下载路由
    let download_route = warp::path("download")
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .map({
            let state = state.clone();
            move |file_id: String| {
                let files = state.get_shared_files();
                if let Some(file) = files.iter().find(|f| f.id == file_id) {
                    // TODO: 返回文件内容
                    warp::reply::json(&file)
                } else {
                    warp::reply::with_status(warp::reply::json(&"Not found"), 404)
                }
            }
        });

    let routes = files_route
        .or(upload_route)
        .or(download_route);

    // 启动服务器
    let (addr, server) = warp::serve(routes)
        .bind_with_graceful_shutdown(([0, 0, 0, 0], config.port), async move {
            let shutdown_rx = shutdown_tx.subscribe();
            tokio::select! {
                _ = shutdown_rx.recv() => {},
            }
        })
        .await
        .map_err(|e| SDKError::Server(format!("Failed to bind: {}", e)))?;

    tracing::info!("File server started on {}", addr);
    Ok(addr.port())
}

/// 生成文件 ID
fn generate_file_id(path: &std::path::Path) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("{}-{:x}", timestamp, rand::random::<u64>())
}

/// 计算文件哈希
fn calculate_file_hash(path: &std::path::Path) -> SDKResult<String> {
    use sha1::Sha1;
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .map_err(|e| SDKError::Io(e))?;

    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 8192];

    loop {
        let n = file.read(&mut buffer)
            .map_err(|e| SDKError::Io(e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_id_generation() {
        let path = std::path::Path::new("/test/file.txt");
        let id = generate_file_id(path);
        assert!(!id.is_empty());
    }
}
