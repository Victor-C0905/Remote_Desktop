// agent/src/event_bus.rs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde_json::Value;

type EventCallback = Arc<dyn Fn(Value) + Send + Sync>;

/// 事件总线，用于发布和订阅事件
pub struct EventBus {
    subscribers: Arc<RwLock<HashMap<String, Vec<EventCallback>>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 发布事件
    pub async fn publish(&self, event_type: &str, data: Value) {
        let subs = self.subscribers.read().await;
        if let Some(callbacks) = subs.get(event_type) {
            for callback in callbacks {
                callback(data.clone());
            }
        }
    }

    /// 订阅事件
    pub async fn subscribe(&self, event_type: &str, callback: EventCallback) {
        let mut subs = self.subscribers.write().await;
        subs.entry(event_type.to_string())
            .or_insert_with(Vec::new)
            .push(callback);
    }

    /// 取消订阅（移除所有该类型的订阅）
    pub async fn unsubscribe_all(&self, event_type: &str) {
        let mut subs = self.subscribers.write().await;
        subs.remove(event_type);
    }

    /// 检查是否有订阅者
    pub async fn has_subscribers(&self, event_type: &str) -> bool {
        let subs = self.subscribers.read().await;
        subs.get(event_type).map(|v| !v.is_empty()).unwrap_or(false)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}