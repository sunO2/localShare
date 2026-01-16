//! ShareSelf SDK
//!
//! 统一的 SDK 接口，封装设备发现、文件共享、文件传输等功能

pub mod types;
pub mod state;
pub mod events;
pub mod discovery;
pub mod server;
pub mod transfer;

pub use types::*;
pub use state::{StateHandle, SDKState, TransferState};
pub use events::{EventSender, EventReceiver, EventQueue, event_channel};
pub use discovery::DiscoveryModule;
pub use server::{FileServerModule, ServerConfig};
pub use transfer::TransferModule;
