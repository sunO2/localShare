//! TUI 应用程序

use super::file_browser::FileBrowser;
use sharSelf::discovery::{discovery_service, register_service, DiscoveryEvent, DeviceInfo, SharedFile};
use sharSelf::common::config::{DiscoveryConfig, ServiceConfig};
use sharSelf::common::error::Result;
use sharSelf::torrent::{TorrentFile, PieceManager, Seeder, Downloader, DEFAULT_BT_PORT};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{io, time::Duration};
use tokio::sync::mpsc;

/// 应用焦点
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// 设备列表
    DeviceList,
    /// 文件浏览器
    FileBrowser,
    /// 传输列表
    TransferList,
    /// 共享文件列表
    SharedFiles,
}

/// 传输任务状态
#[derive(Debug, Clone)]
pub enum TransferStatus {
    /// 准备中
    Preparing,
    /// 正在上传
    Uploading { progress: f64 },
    /// 正在下载
    Downloading { progress: f64 },
    /// 已完成
    Completed,
    /// 失败
    Failed { reason: String },
}

/// 传输任务
#[derive(Debug, Clone)]
pub struct TransferTask {
    /// 任务名称
    pub name: String,
    /// 对端设备
    pub peer: String,
    /// 文件大小
    pub size: u64,
    /// 状态
    pub status: TransferStatus,
    /// 是上传还是下载
    pub is_upload: bool,
    /// 任务 ID
    pub id: usize,
}

/// 传输事件（用于异步通信）
#[derive(Debug, Clone)]
pub enum TransferEvent {
    /// 共享开始
    ShareStarted { id: usize, path: PathBuf },
    /// 共享完成
    ShareCompleted { id: usize, info_hash: String },
    /// 共享失败
    ShareFailed { id: usize, reason: String },
    /// 下载开始
    DownloadStarted { id: usize, name: String, device_addr: SocketAddr, info_hash: String },
    /// 下载进度
    DownloadProgress { id: usize, progress: f64 },
    /// 下载完成
    DownloadCompleted { id: usize },
    /// 下载失败
    DownloadFailed { id: usize, reason: String },
}

/// TUI 应用程序
pub struct App {
    /// 设备列表
    devices: Vec<DeviceInfo>,
    /// 设备列表选中索引
    device_selected: usize,
    /// 选中的设备名称集合
    selected_devices: HashSet<String>,
    /// 文件浏览器
    file_browser: FileBrowser,
    /// 当前焦点
    focus: Focus,
    /// 是否运行
    running: bool,
    /// 设备发现事件接收器
    event_rx: Option<mpsc::Receiver<DiscoveryEvent>>,
    /// 发现服务句柄
    _discovery_handle: Option<sharSelf::discovery::service::ShutdownHandle>,
    /// 服务注册句柄
    service_handle: Option<sharSelf::discovery::registrar::ServiceHandle>,
    /// 种子服务列表 (文件路径 -> (Torrent, Seeder句柄))
    seeders: HashMap<PathBuf, (Arc<TorrentFile>, Option<tokio::task::JoinHandle<()>>)>,
    /// 传输任务列表
    transfers: Vec<TransferTask>,
    /// 传输列表选中索引
    transfer_selected: usize,
    /// 当前本机监听地址
    local_addr: SocketAddr,
    /// PieceManager 管理器
    piece_managers: HashMap<PathBuf, Arc<PieceManager>>,
    /// 传输事件发送器
    transfer_tx: mpsc::Sender<TransferEvent>,
    /// 传输事件接收器
    transfer_rx: Option<mpsc::Receiver<TransferEvent>>,
    /// 下一个任务 ID
    next_task_id: usize,
    /// 共享文件列表 (名称 -> info_hash)
    shared_files: HashMap<String, String>,
    /// 是否需要广播共享文件
    need_broadcast: bool,
    /// 当前查看的设备
    viewing_device: Option<DeviceInfo>,
    /// 当前设备的共享文件列表
    device_shared_files: Vec<SharedFile>,
    /// 共享文件列表选中索引
    shared_file_selected: usize,
}

impl App {
    /// 创建新的 TUI 应用
    pub fn new(start_dir: PathBuf) -> Self {
        // 获取本机 IP 地址
        let local_ip = Self::get_local_ip().unwrap_or_else(|| "0.0.0.0".to_string());
        let local_addr = format!("{}:{}", local_ip, DEFAULT_BT_PORT)
            .parse::<SocketAddr>()
            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], DEFAULT_BT_PORT)));

        // 创建传输事件通道
        let (transfer_tx, transfer_rx) = mpsc::channel::<TransferEvent>(100);

        App {
            devices: Vec::new(),
            device_selected: 0,
            selected_devices: HashSet::new(),
            file_browser: FileBrowser::new(start_dir),
            focus: Focus::DeviceList,
            running: true,
            event_rx: None,
            _discovery_handle: None,
            service_handle: None,
            seeders: HashMap::new(),
            transfers: Vec::new(),
            transfer_selected: 0,
            local_addr,
            piece_managers: HashMap::new(),
            transfer_tx,
            transfer_rx: Some(transfer_rx),
            next_task_id: 0,
            shared_files: HashMap::new(),
            need_broadcast: false,
            viewing_device: None,
            device_shared_files: Vec::new(),
            shared_file_selected: 0,
        }
    }

    /// 获取传输事件发送器
    pub fn transfer_tx(&self) -> mpsc::Sender<TransferEvent> {
        self.transfer_tx.clone()
    }

    /// 分配新的任务 ID
    fn allocate_task_id(&mut self) -> usize {
        let id = self.next_task_id;
        self.next_task_id += 1;
        id
    }

    /// 获取本机 IP 地址
    fn get_local_ip() -> Option<String> {
        use std::net::UdpSocket;
        let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("8.8.8.8:80").ok()?;
        let local_addr = socket.local_addr().ok()?;
        Some(local_addr.ip().to_string())
    }

    /// 启动设备发现和服务注册
    pub async fn start_discovery(&mut self) -> Result<()> {
        // 1. 注册自己的服务
        let hostname = gethostname::gethostname()
            .into_string()
            .unwrap_or_else(|_| "Unknown".to_string());

        let mut txt_records = HashMap::new();
        txt_records.insert("version".to_string(), "0.1.0".to_string());
        txt_records.insert("platform".to_string(), Self::get_platform().to_string());

        let service_config = ServiceConfig {
            service_name: hostname.clone(),
            service_type: sharSelf::DEFAULT_SERVICE_TYPE.to_string(),
            port: 8080,
            txt_records,
            hostname: Some(hostname),
            ttl: 120,
            ..Default::default()
        };

        match register_service(service_config) {
            Ok(service) => {
                tracing::info!("Registered as mDNS service");
                self.service_handle = Some(service);
            }
            Err(e) => {
                tracing::warn!("Failed to register service: {}, continuing with discovery only", e);
            }
        }

        // 2. 启动设备发现
        let config = DiscoveryConfig::default();
        let discovery = discovery_service(config)?;

        // 分离事件接收器和关闭句柄
        let (event_rx, shutdown_handle) = discovery.split();
        self.event_rx = Some(event_rx);
        self._discovery_handle = Some(shutdown_handle);

        Ok(())
    }

    /// 广播共享文件信息到 mDNS
    pub async fn broadcast_shared_files(&mut self) {
        if self.shared_files.is_empty() {
            return;
        }

        if let Some(service) = &mut self.service_handle {
            // 构建新的 TXT 记录，包含共享文件信息
            let mut txt_records = HashMap::new();
            txt_records.insert("version".to_string(), "0.1.0".to_string());
            txt_records.insert("platform".to_string(), Self::get_platform().to_string());

            // 添加共享文件列表
            for (name, hash) in &self.shared_files {
                txt_records.insert(format!("file_{}", name), hash.clone());
            }

            // 更新 mDNS TXT 记录
            if let Err(e) = service.update_txt(txt_records).await {
                tracing::warn!("Failed to update mDNS TXT records: {}", e);
            } else {
                tracing::info!("Broadcasted {} shared files via mDNS", self.shared_files.len());
            }
        }
    }

    /// 获取平台信息
    fn get_platform() -> &'static str {
        #[cfg(target_os = "android")]
        return "Android";
        #[cfg(target_os = "ios")]
        return "iOS";
        #[cfg(target_os = "linux")]
        return "Linux";
        #[cfg(target_os = "macos")]
        return "macOS";
        #[cfg(target_os = "windows")]
        return "Windows";
        #[cfg(target_os = "freebsd")]
        return "FreeBSD";
        #[cfg(not(any(
            target_os = "android",
            target_os = "ios",
            target_os = "linux",
            target_os = "macos",
            target_os = "windows",
            target_os = "freebsd"
        )))]
        return "Unknown";
    }

    /// 处理设备发现事件
    pub fn handle_discovery_events(&mut self) {
        if let Some(rx) = &mut self.event_rx {
            while let Ok(event) = rx.try_recv() {
                match event {
                    DiscoveryEvent::DeviceFound(device) => {
                        self.devices.push(device);
                    }
                    DiscoveryEvent::DeviceLost(name) => {
                        self.devices.retain(|d| d.name != name);
                    }
                    DiscoveryEvent::DeviceUpdated(device) => {
                        if let Some(pos) = self.devices.iter().position(|d| d.name == device.name) {
                            self.devices[pos] = device;
                        }
                    }
                    DiscoveryEvent::Error(_) => {}
                }
            }
        }
    }

    /// 处理键盘事件
    pub fn handle_key_event(&mut self, key: KeyEvent) {
        match self.focus {
            Focus::DeviceList => self.handle_device_list_keys(key),
            Focus::FileBrowser => self.handle_file_browser_keys(key),
            Focus::TransferList => self.handle_transfer_list_keys(key),
            Focus::SharedFiles => self.handle_shared_files_keys(key),
        }
    }

    /// 处理设备列表键盘事件
    fn handle_device_list_keys(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.device_selected > 0 {
                    self.device_selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.device_selected + 1 < self.devices.len() {
                    self.device_selected += 1;
                }
            }
            KeyCode::Char(' ') => {
                // 空格键切换选择状态
                if let Some(device) = self.devices.get(self.device_selected) {
                    if self.selected_devices.contains(&device.name) {
                        self.selected_devices.remove(&device.name);
                    } else {
                        self.selected_devices.insert(device.name.clone());
                    }
                }
            }
            KeyCode::Enter => {
                // 查看设备共享的文件
                if let Some(device) = self.devices.get(self.device_selected) {
                    self.viewing_device = Some(device.clone());
                    self.device_shared_files = device.get_shared_files();
                    self.shared_file_selected = 0;
                    self.focus = Focus::SharedFiles;
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // 切换焦点
                self.focus = Focus::FileBrowser;
            }
            KeyCode::Char('a') => {
                // 全选/取消全选
                if self.selected_devices.len() == self.devices.len() {
                    self.selected_devices.clear();
                } else {
                    for device in &self.devices {
                        self.selected_devices.insert(device.name.clone());
                    }
                }
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                self.running = false;
            }
            _ => {}
        }
    }

    /// 处理文件浏览器键盘事件
    fn handle_file_browser_keys(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.file_browser.select_previous();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.file_browser.select_next();
            }
            KeyCode::Enter => {
                // 进入目录
                self.file_browser.enter_directory();
            }
            KeyCode::Left | KeyCode::Char('h') => {
                // 返回上级
                self.file_browser.go_up();
            }
            KeyCode::Home => {
                self.file_browser.select_first();
            }
            KeyCode::End => {
                self.file_browser.select_last();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // 切换焦点：设备列表 -> 文件浏览器 -> 传输列表
                self.focus = match self.focus {
                    Focus::DeviceList => Focus::FileBrowser,
                    Focus::FileBrowser => Focus::TransferList,
                    Focus::TransferList => Focus::DeviceList,
                    Focus::SharedFiles => Focus::TransferList,
                };
            }
            KeyCode::Char('s') => {
                // 共享选中的文件/目录
                self.start_sharing_selected_file();
            }
            KeyCode::Char('t') => {
                // 切换到传输列表
                self.focus = Focus::TransferList;
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                self.running = false;
            }
            _ => {}
        }
    }

    /// 处理传输列表键盘事件
    fn handle_transfer_list_keys(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.transfer_selected > 0 {
                    self.transfer_selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.transfer_selected + 1 < self.transfers.len() {
                    self.transfer_selected += 1;
                }
            }
            KeyCode::Char('d') => {
                // 删除选中的传输任务
                if self.transfer_selected < self.transfers.len() {
                    self.transfers.remove(self.transfer_selected);
                    if self.transfer_selected > 0 && self.transfer_selected >= self.transfers.len() {
                        self.transfer_selected = self.transfers.len().saturating_sub(1);
                    }
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // 切换焦点
                self.focus = Focus::DeviceList;
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                self.running = false;
            }
            _ => {}
        }
    }

    /// 处理共享文件列表键盘事件
    fn handle_shared_files_keys(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.shared_file_selected > 0 {
                    self.shared_file_selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.shared_file_selected + 1 < self.device_shared_files.len() {
                    self.shared_file_selected += 1;
                }
            }
            KeyCode::Enter => {
                // 开始下载选中的文件
                if let Some(shared_file) = self.device_shared_files.get(self.shared_file_selected).cloned() {
                    self.start_download(&shared_file);
                }
            }
            KeyCode::Char('d') => {
                // 开始下载选中的文件
                if let Some(shared_file) = self.device_shared_files.get(self.shared_file_selected).cloned() {
                    self.start_download(&shared_file);
                }
            }
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                // 返回设备列表
                self.focus = Focus::DeviceList;
                self.viewing_device = None;
                self.device_shared_files.clear();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // 切换焦点
                self.focus = Focus::TransferList;
            }
            KeyCode::Char('q') => {
                self.running = false;
            }
            _ => {}
        }
    }

    /// 启动共享选中的文件
    fn start_sharing_selected_file(&mut self) {
        // 先克隆需要的数据
        let (item_name, item_size) = match self.file_browser.selected_file() {
            Some(item) => (item.name.clone(), item.size),
            None => return,
        };

        let item_path = self.file_browser.current_dir().join(&item_name);

        // 检查是否已经在共享
        if self.seeders.contains_key(&item_path) {
            return;
        }

        // 分配任务 ID
        let task_id = self.allocate_task_id();

        // 创建传输任务
        let task = TransferTask {
            name: item_name.clone(),
            peer: "所有人".to_string(),
            size: item_size,
            status: TransferStatus::Preparing,
            is_upload: true,
            id: task_id,
        };
        self.transfers.push(task);

        // 发送共享开始事件（克隆 path 避免移动）
        let _ = self.transfer_tx.try_send(TransferEvent::ShareStarted {
            id: task_id,
            path: item_path.clone(),
        });

        tracing::info!("准备共享文件: {:?}", item_path);
    }

    /// 开始下载共享文件
    fn start_download(&mut self, shared_file: &SharedFile) {
        // 获取设备信息
        let device_name = self.viewing_device.as_ref()
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "未知设备".to_string());

        // 获取设备地址和端口
        let device_addr = self.viewing_device.as_ref()
            .and_then(|d| d.get_address(false))
            .or_else(|| self.viewing_device.as_ref().and_then(|d| d.get_address(true)))
            .cloned()
            .unwrap_or_else(|| {
                let port = self.viewing_device.as_ref()
                    .and_then(|d| d.get_bt_port())
                    .unwrap_or(6881);
                SocketAddr::from(([0, 0, 0, 0], port))
            });

        // 分配任务 ID
        let task_id = self.allocate_task_id();

        // 创建传输任务
        let task = TransferTask {
            name: shared_file.name.clone(),
            peer: device_name.clone(),
            size: shared_file.size.unwrap_or(0),
            status: TransferStatus::Preparing,
            is_upload: false,
            id: task_id,
        };
        self.transfers.push(task);

        // 发送下载开始事件
        let _ = self.transfer_tx.try_send(TransferEvent::DownloadStarted {
            id: task_id,
            name: shared_file.name.clone(),
            device_addr,
            info_hash: shared_file.info_hash.clone(),
        });

        tracing::info!("开始下载: {} from {}", shared_file.name, device_name);
    }

    /// 处理传输事件
    pub fn handle_transfer_events(&mut self) {
        if let Some(rx) = &mut self.transfer_rx {
            while let Ok(event) = rx.try_recv() {
                match event {
                    TransferEvent::ShareStarted { id, path } => {
                        tracing::info!("共享开始: id={}, path={:?}", id, path);
                        // 更新任务状态
                        if let Some(task) = self.transfers.iter_mut().find(|t| t.id == id) {
                            task.status = TransferStatus::Preparing;
                        }
                    }
                    TransferEvent::ShareCompleted { id, info_hash } => {
                        tracing::info!("共享完成: id={}, hash={}", id, info_hash);
                        if let Some(task) = self.transfers.iter_mut().find(|t| t.id == id) {
                            task.status = TransferStatus::Uploading { progress: 0.0 };
                            task.peer = format!("Hash: {}", &info_hash[..16]);

                            // 添加到共享文件列表
                            self.shared_files.insert(task.name.clone(), info_hash);

                            // 标记需要广播
                            self.need_broadcast = true;
                        }
                    }
                    TransferEvent::ShareFailed { id, reason } => {
                        tracing::warn!("共享失败: id={}, reason={}", id, reason);
                        if let Some(task) = self.transfers.iter_mut().find(|t| t.id == id) {
                            task.status = TransferStatus::Failed { reason };
                        }
                    }
                    TransferEvent::DownloadStarted { id, name, device_addr: _, info_hash: _ } => {
                        tracing::info!("下载开始: id={}, name={}", id, name);
                        if let Some(task) = self.transfers.iter_mut().find(|t| t.id == id) {
                            task.status = TransferStatus::Downloading { progress: 0.0 };
                        }
                    }
                    TransferEvent::DownloadProgress { id, progress } => {
                        if let Some(task) = self.transfers.iter_mut().find(|t| t.id == id) {
                            task.status = TransferStatus::Downloading { progress };
                        }
                    }
                    TransferEvent::DownloadCompleted { id } => {
                        tracing::info!("下载完成: id={}", id);
                        if let Some(task) = self.transfers.iter_mut().find(|t| t.id == id) {
                            task.status = TransferStatus::Completed;
                        }
                    }
                    TransferEvent::DownloadFailed { id, reason } => {
                        tracing::warn!("下载失败: id={}, reason={}", id, reason);
                        if let Some(task) = self.transfers.iter_mut().find(|t| t.id == id) {
                            task.status = TransferStatus::Failed { reason };
                        }
                    }
                }
            }
        }
    }

    /// 是否正在运行
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// 获取焦点
    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// 绘制 UI
    pub fn draw(&self, f: &mut Frame) {
        match self.focus {
            Focus::TransferList => {
                // 传输列表全屏显示
                self.draw_transfer_list(f, f.size());
            }
            Focus::SharedFiles => {
                // 共享文件列表全屏显示
                self.draw_shared_files(f, f.size());
            }
            _ => {
                // 设备列表和文件浏览器
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(33), Constraint::Percentage(67)].as_ref())
                    .split(f.size());

                self.draw_device_list(f, chunks[0]);
                self.draw_file_browser(f, chunks[1]);
            }
        }
    }

    /// 绘制设备列表
    fn draw_device_list(&self, f: &mut Frame, area: Rect) {
        let title = Line::from(vec![
            Span::raw(" 设备列表 "),
            Span::styled(
                format!("({}/{})", self.selected_devices.len(), self.devices.len()),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                if self.focus == Focus::DeviceList { "[聚焦]" } else { "" },
                Style::default().fg(Color::Cyan),
            ),
        ]);

        let items: Vec<ListItem> = self
            .devices
            .iter()
            .enumerate()
            .map(|(i, device)| {
                let is_cursor_selected = i == self.device_selected && self.focus == Focus::DeviceList;
                let is_checked = self.selected_devices.contains(&device.name);

                let style = if is_cursor_selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else if is_checked {
                    Style::default()
                        .fg(Color::Green)
                } else {
                    Style::default()
                };

                // 选择标记
                let check_mark = if is_checked { "[✓] " } else { "[ ] " };

                let content = vec![
                    Line::from(vec![
                        Span::styled(check_mark, Style::default().fg(Color::Yellow)),
                        Span::styled(&device.name, style),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("    📡 {}:{}",
                                device.addresses.first().map(|a: &std::net::SocketAddr| a.ip().to_string()).unwrap_or_else(|| "未知".to_string()),
                                device.port
                            ),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]),
                ];

                ListItem::new(content)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );

        f.render_widget(list, area);
    }

    /// 绘制文件浏览器
    fn draw_file_browser(&self, f: &mut Frame, area: Rect) {
        let current_dir = self.file_browser.current_dir().display().to_string();

        let title = Line::from(vec![
            Span::raw(" 文件浏览器 "),
            Span::styled(
                if self.focus == Focus::FileBrowser { "[聚焦]" } else { "" },
                Style::default().fg(Color::Cyan),
            ),
        ]);

        // 根据焦点显示不同的操作提示
        let help_text = if self.focus == Focus::DeviceList {
            " ↑/k:上 ↓/j:下 Space:选择 a:全选 Tab:切换 q:退出"
        } else {
            " ↑/k:上 ↓/j:下 Enter:进入 ←/h:返回 s:共享 t:传输 Tab:切换 q:退出"
        };

        let items: Vec<ListItem> = self
            .file_browser
            .files()
            .iter()
            .enumerate()
            .map(|(i, file)| {
                let is_selected = i == self.file_browser.selected_index()
                    && self.focus == Focus::FileBrowser;

                let style = if is_selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let mut text = file.display_name();

                // 显示文件大小
                if !file.is_dir && file.size > 0 {
                    text.push_str(&format!(" ({})", format_size(file.size)));
                }

                ListItem::new(Text::styled(text, style))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title));

        f.render_widget(list, area);

        // 绘制当前路径
        let path_area = Rect {
            y: area.y + area.height - 3,
            height: 3,
            ..area
        };

        let path_text = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("📍 ", Style::default().fg(Color::Yellow)),
                Span::raw(&current_dir),
            ]),
            Line::from(vec![
                Span::styled(
                    help_text,
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ])
        .block(Block::default().borders(Borders::ALL).title(" 操作提示 "))
        .wrap(Wrap { trim: false });

        f.render_widget(path_text, path_area);
    }

    /// 绘制传输列表
    fn draw_transfer_list(&self, f: &mut Frame, area: Rect) {
        let title = Line::from(vec![
            Span::raw(" 传输列表 "),
            Span::styled(
                format!("({})", self.transfers.len()),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                if self.focus == Focus::TransferList { "[聚焦]" } else { "" },
                Style::default().fg(Color::Cyan),
            ),
        ]);

        let items: Vec<ListItem> = self
            .transfers
            .iter()
            .enumerate()
            .map(|(i, transfer)| {
                let is_selected = i == self.transfer_selected && self.focus == Focus::TransferList;

                let style = if is_selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                // 方向图标
                let direction = if transfer.is_upload { "↑ " } else { "↓ " };
                let direction_style = Style::default().fg(if transfer.is_upload { Color::Green } else { Color::Blue });

                // 状态图标和颜色
                let (status_icon, status_color) = match &transfer.status {
                    TransferStatus::Preparing => ("⏳", Color::Yellow),
                    TransferStatus::Uploading { .. } => ("↑", Color::Green),
                    TransferStatus::Downloading { .. } => ("↓", Color::Blue),
                    TransferStatus::Completed => ("✓", Color::Green),
                    TransferStatus::Failed { .. } => ("✗", Color::Red),
                };

                // 进度百分比
                let progress = match &transfer.status {
                    TransferStatus::Uploading { progress } |
                    TransferStatus::Downloading { progress } => *progress,
                    TransferStatus::Completed => 1.0,
                    _ => 0.0,
                };

                let content = vec![
                    Line::from(vec![
                        Span::styled(direction, direction_style),
                        Span::styled(&transfer.name, style),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("    {} {} | ", status_icon, transfer.peer),
                            Style::default().fg(status_color),
                        ),
                        Span::styled(
                            format!("{} | {}", format_size(transfer.size), format_progress(progress)),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]),
                ];

                ListItem::new(content)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title));

        f.render_widget(list, area);

        // 绘制操作提示
        let help_area = Rect {
            y: area.y + area.height - 3,
            height: 3,
            ..area
        };

        let help_text = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    " ↑/k:上 ↓/j:下 d:删除 Tab:切换 t:返回 q:退出",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ])
        .block(Block::default().borders(Borders::ALL).title(" 操作提示 "))
        .wrap(Wrap { trim: false });

        f.render_widget(help_text, help_area);
    }

    /// 绘制共享文件列表
    fn draw_shared_files(&self, f: &mut Frame, area: Rect) {
        let device_name = self.viewing_device.as_ref()
            .map(|d| d.name.as_str())
            .unwrap_or("未知设备");

        let title = Line::from(vec![
            Span::raw(" 共享文件 "),
            Span::styled(
                format!("@ {} ({})", device_name, self.device_shared_files.len()),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                if self.focus == Focus::SharedFiles { "[聚焦]" } else { "" },
                Style::default().fg(Color::Cyan),
            ),
        ]);

        let items: Vec<ListItem> = self
            .device_shared_files
            .iter()
            .enumerate()
            .map(|(i, file)| {
                let is_selected = i == self.shared_file_selected && self.focus == Focus::SharedFiles;

                let style = if is_selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                // 文件图标
                let icon = "📄";

                let content = vec![
                    Line::from(vec![
                        Span::styled(icon, Style::default().fg(Color::Yellow)),
                        Span::styled(" ", Style::default()),
                        Span::styled(&file.name, style),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("    🔖 Hash: {}...", &file.info_hash[..16.min(file.info_hash.len())]),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]),
                ];

                ListItem::new(content)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );

        f.render_widget(list, area);

        // 绘制操作提示
        let help_area = Rect {
            y: area.y + area.height - 3,
            height: 3,
            ..area
        };

        let help_text = Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    " ↑/k:上 ↓/j:下 Enter/d:下载 Esc/h:返回 Tab:传输 t:返回 q:退出",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ])
        .block(Block::default().borders(Borders::ALL).title(" 操作提示 "))
        .wrap(Wrap { trim: false });

        f.render_widget(help_text, help_area);
    }
}

/// 格式化进度百分比
fn format_progress(progress: f64) -> String {
    format!("{:.1}%", progress * 100.0)
}

/// 格式化文件大小
fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if size >= GB {
        format!("{:.1} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.1} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

/// 运行 TUI 应用
pub async fn run_tui() -> Result<()> {
    // 初始化终端
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 获取当前目录作为起始目录
    let start_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));

    // 创建应用
    let mut app = App::new(start_dir);

    // 启动设备发现
    app.start_discovery().await?;

    // 获取传输事件接收器和发送器，并启动传输服务
    let transfer_rx = app.transfer_rx.take().unwrap();
    let transfer_tx = app.transfer_tx();
    let transfer_service = tokio::spawn(async move {
        transfer_service_handler(transfer_rx, transfer_tx).await;
    });

    // 运行主循环
    let result = run_app(&mut terminal, &mut app).await;

    // 取消传输服务
    transfer_service.abort();

    // 恢复终端
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

/// 传输服务处理器 - 在后台处理所有文件传输任务
async fn transfer_service_handler(mut rx: mpsc::Receiver<TransferEvent>, tx: mpsc::Sender<TransferEvent>) {
    let mut pending_shares: HashMap<usize, PathBuf> = HashMap::new();
    let mut active_seeders: HashMap<PathBuf, (Arc<TorrentFile>, tokio::task::JoinHandle<()>)> = HashMap::new();
    let mut active_downloads: HashMap<usize, tokio::task::JoinHandle<()>> = HashMap::new();

    while let Some(event) = rx.recv().await {
        match event {
            TransferEvent::ShareStarted { id, path } => {
                tracing::info!("开始处理共享请求: id={}, path={:?}", id, path);

                // 保存路径以便后续使用
                pending_shares.insert(id, path.clone());

                // 创建种子服务
                match TorrentFile::create(&path, None) {
                    Ok(torrent) => {
                        let info_hash = hex::encode(torrent.info_hash().unwrap_or([0u8; 20]));
                        let file_name = torrent.metainfo.info.name.clone();

                        // 创建 PieceManager
                        let storage_path = if path.is_dir() {
                            path.clone()
                        } else {
                            path.parent().unwrap_or(&path).to_path_buf()
                        };
                        let piece_manager = Arc::new(PieceManager::new(
                            torrent.metainfo.clone(),
                            storage_path,
                        ));

                        // 启动 Seeder
                        let local_ip = get_local_ip_for_seeder().unwrap_or_else(|| "0.0.0.0".to_string());
                        let listen_addr = format!("{}:{}", local_ip, DEFAULT_BT_PORT)
                            .parse::<SocketAddr>()
                            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], DEFAULT_BT_PORT)));

                        let seeder = Seeder::new(
                            torrent.metainfo.clone(),
                            piece_manager.clone(),
                            listen_addr,
                        );

                        // 在后台启动 seeder
                        let seeder_handle = tokio::spawn(async move {
                            let _ = seeder.start().await;
                        });

                        active_seeders.insert(path.clone(), (Arc::new(torrent.clone()), seeder_handle));

                        // 发送完成事件（克隆 info_hash）
                        let _ = tx.send(TransferEvent::ShareCompleted {
                            id,
                            info_hash: info_hash.clone(),
                        });

                        tracing::info!("共享完成: id={}, file={}, hash={}", id, file_name, info_hash);
                    }
                    Err(e) => {
                        tracing::error!("共享失败: id={}, reason={}", id, e);
                        // 发送失败事件
                        let _ = tx.send(TransferEvent::ShareFailed {
                            id,
                            reason: e.to_string(),
                        });
                    }
                }
            }
            TransferEvent::DownloadStarted { id, name, device_addr, info_hash } => {
                tracing::info!("开始处理下载请求: id={}, name={}, addr={}, hash={}",
                    id, name, device_addr, info_hash);

                // 启动下载任务
                let tx_clone = tx.clone();
                let handle = tokio::spawn(async move {
                    // 简化的下载模拟
                    // TODO: 实现真正的 BitTorrent 下载逻辑
                    let total_pieces = 100;
                    for piece in 0..total_pieces {
                        // 模拟下载延迟
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                        let progress = (piece + 1) as f64 / total_pieces as f64;
                        let _ = tx_clone.send(TransferEvent::DownloadProgress {
                            id,
                            progress,
                        });

                        tracing::debug!("下载进度: {} - {:.1}%", name, progress * 100.0);
                    }

                    // 下载完成
                    let _ = tx_clone.send(TransferEvent::DownloadCompleted { id });
                    tracing::info!("下载完成: id={}, name={}", id, name);
                });

                active_downloads.insert(id, handle);
            }
            _ => {
                // 其他事件已经在 App 中处理
            }
        }
    }
}

/// 获取本机 IP（用于种子服务）
fn get_local_ip_for_seeder() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let local_addr = socket.local_addr().ok()?;
    Some(local_addr.ip().to_string())
}

/// 运行应用主循环
async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let tick_rate = Duration::from_millis(250);

    loop {
        // 处理设备发现事件
        app.handle_discovery_events();

        // 处理传输事件
        app.handle_transfer_events();

        // 检查是否需要广播共享文件
        if app.need_broadcast {
            app.broadcast_shared_files().await;
            app.need_broadcast = false;
        }

        // 绘制 UI
        terminal.draw(|f| app.draw(f))?;

        // 处理输入
        if event::poll(tick_rate)? {
            if let event::Event::Key(key) = event::read()? {
                app.handle_key_event(key);

                if !app.is_running() {
                    return Ok(());
                }
            }
        }
    }
}
