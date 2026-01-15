//! Torrent 元数据服务器
//!
//! 为下载者提供 torrent 元数据

use crate::common::error::{Error, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{info, warn, error};

/// 元数据服务器
pub struct MetadataServer {
    /// torrent 数据 (info_hash -> torrent_data)
    torrents: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    /// 是否运行
    running: Arc<RwLock<bool>>,
}

impl MetadataServer {
    /// 创建新的元数据服务器
    pub fn new() -> Self {
        MetadataServer {
            torrents: Arc::new(RwLock::new(HashMap::new())),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// 添加 torrent
    pub async fn add_torrent(&self, info_hash: String, data: Vec<u8>) {
        let mut torrents = self.torrents.write().await;
        torrents.insert(info_hash, data);
        info!("已添加 torrent 到元数据服务器");
    }

    /// 获取 torrent 数据
    pub async fn get_torrent(&self, info_hash: &str) -> Option<Vec<u8>> {
        let torrents = self.torrents.read().await;
        torrents.get(info_hash).cloned()
    }

    /// 移除 torrent
    pub async fn remove_torrent(&self, info_hash: &str) {
        let mut torrents = self.torrents.write().await;
        torrents.remove(info_hash);
    }

    /// 启动元数据服务器
    pub async fn start(&self, port: u16) -> Result<()> {
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| Error::Network(format!("Failed to bind metadata server {}: {}", addr, e)))?;

        info!("元数据服务器启动，监听: {}", addr);

        let torrents = Arc::clone(&self.torrents);
        let running = Arc::clone(&self.running);

        loop {
            // 检查是否应该停止
            {
                let is_running = *running.read().await;
                if !is_running {
                    break;
                }
            }

            // 接受连接
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    let torrents = Arc::clone(&torrents);
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_client(stream, peer_addr, torrents).await {
                            error!("处理元数据请求失败 {}: {}", peer_addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("接受连接失败: {}", e);
                }
            }
        }

        Ok(())
    }

    /// 停止元数据服务器
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }

    /// 处理客户端请求
    async fn handle_client(
        mut stream: tokio::net::TcpStream,
        peer_addr: SocketAddr,
        torrents: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    ) -> Result<()> {
        let (reader, mut writer) = stream.split();
        let mut reader = BufReader::new(reader);
        let mut request_line = String::new();

        reader.read_line(&mut request_line).await
            .map_err(|e| Error::Network(format!("Failed to read request: {}", e)))?;

        info!("元数据请求来自 {}: {}", peer_addr, request_line.trim());

        // 解析请求: 格式为 "GET /<info_hash>\n"
        let info_hash = if request_line.starts_with("GET /") {
            let path = request_line[5..].trim();
            if path == "/" {
                return Err(Error::Other("Missing info_hash".to_string()));
            }
            path.to_string()
        } else {
            return Err(Error::Other("Invalid request format".to_string()));
        };

        // 查找 torrent 数据
        let torrent_data = {
            let torrents = torrents.read().await;
            torrents.get(&info_hash).cloned()
        };

        match torrent_data {
            Some(data) => {
                // 发送数据长度 (4 字节)
                let len = data.len() as u32;
                writer.write_all(&len.to_be_bytes()).await
                    .map_err(|e| Error::Network(format!("Failed to write length: {}", e)))?;

                // 发送数据
                writer.write_all(&data).await
                    .map_err(|e| Error::Network(format!("Failed to write data: {}", e)))?;

                writer.flush().await
                    .map_err(|e| Error::Network(format!("Failed to flush: {}", e)))?;

                info!("已发送 {} 字节的元数据到 {}", data.len(), peer_addr);
            }
            None => {
                // 发送 0 长度表示未找到
                writer.write_all(&0u32.to_be_bytes()).await
                    .map_err(|e| Error::Network(format!("Failed to write not-found: {}", e)))?;

                warn!("元数据未找到: {}", info_hash);
            }
        }

        Ok(())
    }
}

impl Default for MetadataServer {
    fn default() -> Self {
        Self::new()
    }
}

/// 全局元数据服务器单例
static mut GLOBAL_METADATA_SERVER: Option<MetadataServer> = None;

/// 初始化全局元数据服务器
pub fn init_global_metadata_server() {
    unsafe {
        if GLOBAL_METADATA_SERVER.is_none() {
            GLOBAL_METADATA_SERVER = Some(MetadataServer::new());
            info!("全局元数据服务器已初始化");
        }
    }
}

/// 获取全局元数据服务器
pub fn global_metadata_server() -> &'static MetadataServer {
    unsafe {
        GLOBAL_METADATA_SERVER.as_ref()
            .expect("全局元数据服务器未初始化，请先调用 init_global_metadata_server()")
    }
}
