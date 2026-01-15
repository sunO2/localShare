//! 终端 UI 界面
//!
//! 提供交互式的 TUI 界面用于设备发现和文件浏览

mod app;
mod file_browser;

pub use app::run_tui;
