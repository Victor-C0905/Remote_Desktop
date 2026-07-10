// agent/src/event_bus.rs

use tokio::sync::broadcast;
use serde_json::Value;

/// 事件数据
#[derive(Debug, Clone)]
pub struct Event {
    pub event_type: String,
    pub data: Value,
    pub timestamp: u64,
}

/// 事件总线，用于发布和订阅事件
pub struct EventBus {
    // 广播通道（用于事件推送）
    broadcaster: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        // 创建广播通道，容量为 100
        let (tx, _) = broadcast::channel(100);
        Self {
            broadcaster: tx,
        }
    }

    /// 发布事件
    pub async fn publish(&self, event_type: &str, data: Value) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let event = Event {
            event_type: event_type.to_string(),
            data,
            timestamp,
        };

        // 广播事件
        if let Err(e) = self.broadcaster.send(event) {
            tracing::warn!("广播事件失败: {}", e);
        }
    }

    /// 订阅事件（返回接收器）
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.broadcaster.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}