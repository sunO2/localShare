//! SDK 事件系统
//!
//! 管理事件的产生、分发和轮询

use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use super::types::SDKEvent;

/// 事件发送器
#[derive(Debug, Clone)]
pub struct EventSender {
    tx: mpsc::UnboundedSender<SDKEvent>,
}

impl EventSender {
    pub fn new(tx: mpsc::UnboundedSender<SDKEvent>) -> Self {
        Self { tx }
    }

    pub fn send(&self, event: SDKEvent) {
        let _ = self.tx.send(event);
    }
}

/// 事件接收器
#[derive(Debug)]
pub struct EventReceiver {
    rx: Arc<Mutex<mpsc::UnboundedReceiver<SDKEvent>>>,
}

impl EventReceiver {
    pub fn new(rx: mpsc::UnboundedReceiver<SDKEvent>) -> Self {
        Self {
            rx: Arc::new(Mutex::new(rx)),
        }
    }

    /// 非阻塞轮询事件
    pub fn try_recv(&self) -> Option<SDKEvent> {
        let mut rx = self.rx.lock().unwrap();
        rx.try_recv().ok()
    }

    /// 阻塞等待事件
    pub async fn recv(&self) -> Option<SDKEvent> {
        let mut rx = self.rx.lock().unwrap();
        rx.recv().await
    }
}

/// 创建事件通道
pub fn event_channel() -> (EventSender, EventReceiver) {
    let (tx, rx) = mpsc::unbounded_channel();
    (EventSender::new(tx), EventReceiver::new(rx))
}

/// 事件队列，用于 FFI 轮询
#[derive(Debug)]
pub struct EventQueue {
    events: Arc<Mutex<Vec<SDKEvent>>>,
}

impl EventQueue {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 添加事件到队列
    pub fn push(&self, event: SDKEvent) {
        let mut events = self.events.lock().unwrap();
        events.push(event);
    }

    /// 轮询事件（非阻塞）
    pub fn poll(&self) -> Option<SDKEvent> {
        let mut events = self.events.lock().unwrap();
        if events.is_empty() {
            None
        } else {
            Some(events.remove(0))
        }
    }

    /// 获取所有待处理事件
    pub fn drain(&self) -> Vec<SDKEvent> {
        let mut events = self.events.lock().unwrap();
        std::mem::take(&mut *events)
    }

    /// 获取待处理事件数量
    pub fn len(&self) -> usize {
        let events = self.events.lock().unwrap();
        events.len()
    }
}

impl Default for EventQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_queue() {
        let queue = EventQueue::new();

        assert_eq!(queue.poll(), None);
        assert_eq!(queue.len(), 0);

        queue.push(SDKEvent::Error("test".to_string()));
        assert_eq!(queue.len(), 1);

        let event = queue.poll();
        assert!(event.is_some());
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn test_event_channel() {
        let (tx, rx) = event_channel();

        tx.send(SDKEvent::Error("test".to_string()));

        let event = rx.try_recv();
        assert!(event.is_some());
    }
}
