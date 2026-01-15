//! TUI 应用程序

use super::file_browser::FileBrowser;
use sharSelf::discovery::{discovery_service, register_service, DiscoveryEvent, DeviceInfo};
use sharSelf::common::config::{DiscoveryConfig, ServiceConfig};
use sharSelf::common::error::Result;
use std::collections::HashMap;
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
use std::{io, path::PathBuf, time::Duration};
use tokio::sync::mpsc;

/// 应用焦点
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// 设备列表
    DeviceList,
    /// 文件浏览器
    FileBrowser,
}

/// TUI 应用程序
pub struct App {
    /// 设备列表
    devices: Vec<DeviceInfo>,
    /// 设备列表选中索引
    device_selected: usize,
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
    _service_handle: Option<sharSelf::discovery::registrar::ServiceHandle>,
}

impl App {
    /// 创建新的 TUI 应用
    pub fn new(start_dir: PathBuf) -> Self {
        App {
            devices: Vec::new(),
            device_selected: 0,
            file_browser: FileBrowser::new(start_dir),
            focus: Focus::DeviceList,
            running: true,
            event_rx: None,
            _discovery_handle: None,
            _service_handle: None,
        }
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
                self._service_handle = Some(service);
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
            KeyCode::Enter => {
                // 选择设备，切换焦点到文件浏览器
                self.focus = Focus::FileBrowser;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                // 切换焦点
                self.focus = Focus::FileBrowser;
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
                // 切换焦点
                self.focus = Focus::DeviceList;
            }
            KeyCode::Char('q') | KeyCode::Esc => {
                self.running = false;
            }
            _ => {}
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
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(33), Constraint::Percentage(67)].as_ref())
            .split(f.size());

        self.draw_device_list(f, chunks[0]);
        self.draw_file_browser(f, chunks[1]);
    }

    /// 绘制设备列表
    fn draw_device_list(&self, f: &mut Frame, area: Rect) {
        let title = Line::from(vec![
            Span::raw(" 设备列表 "),
            Span::styled(
                format!("({})", self.devices.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ]);

        let items: Vec<ListItem> = self
            .devices
            .iter()
            .enumerate()
            .map(|(i, device)| {
                let is_selected = i == self.device_selected && self.focus == Focus::DeviceList;
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let content = vec![
                    Line::from(vec![
                        Span::styled(&device.name, style),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            format!("  📡 {}:{}",
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
                    " ↑/k:上 ↓/j:下 Enter:进入 ←/h:返回 Tab:切换焦点 q:退出",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ])
        .block(Block::default().borders(Borders::ALL).title(" 操作提示 "))
        .wrap(Wrap { trim: false });

        f.render_widget(path_text, path_area);
    }
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

    // 运行主循环
    let result = run_app(&mut terminal, &mut app).await;

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

/// 运行应用主循环
async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let tick_rate = Duration::from_millis(250);

    loop {
        // 处理设备发现事件
        app.handle_discovery_events();

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
