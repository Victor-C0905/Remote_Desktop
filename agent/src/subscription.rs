// agent/src/subscription.rs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use crate::protocol::SubscriptionType;
use crate::event_bus::EventBus;
use crate::collectors::MetricsCollector;
use crate::config::AgentConfig;

/// 订阅管理器
pub struct SubscriptionManager {
    // StreamId -> 订阅类型列表
    subscribers: Arc<RwLock<HashMap<u64, Vec<SubscriptionType>>>>,

    // 订阅类型 -> 订阅者数量
    type_counts: Arc<RwLock<HashMap<String, usize>>>,

    // 采集器实例
    metrics_collector: Arc<MetricsCollector>,
}

impl SubscriptionManager {
    pub fn new(_config: AgentConfig, event_bus: Arc<EventBus>) -> Self {
        let metrics_collector = Arc::new(MetricsCollector::new(
            _config.collectors.metrics_interval_secs,
            event_bus.clone(),
        ));

        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            type_counts: Arc::new(RwLock::new(HashMap::new())),
            metrics_collector,
        }
    }

    /// 添加订阅
    pub async fn subscribe(
        &self,
        stream_id: u64,
        types: Vec<SubscriptionType>,
    ) -> Result<Vec<SubscriptionType>> {
        // 回收同 stream_id 的旧订阅计数：insert 直接覆盖会让旧类型计数
        // 只增不减，采集器永不停止（2026-11 审计 M10）。无旧订阅时为空操作
        self.remove_all(stream_id).await?;

        // 记录订阅者
        {
            let mut subs = self.subscribers.write().await;
            subs.insert(stream_id, types.clone());
        }

        // 更新订阅类型计数
        for t in &types {
            let type_name = t.type_name();
            let mut counts = self.type_counts.write().await;
            let count = counts.entry(type_name.clone()).or_insert(0);
            *count += 1;

            // 如果是第一个订阅者，启动对应的采集器
            if *count == 1 {
                self.start_collector(&t).await?;
            }
        }

        Ok(types)
    }

    /// 移除所有订阅（Stream 关闭时）
    pub async fn remove_all(&self, stream_id: u64) -> Result<()> {
        // 获取订阅者的所有订阅类型
        let types = {
            let subs = self.subscribers.read().await;
            subs.get(&stream_id).cloned()
        };

        // 移除订阅者
        {
            let mut subs = self.subscribers.write().await;
            subs.remove(&stream_id);
        }

        // 更新订阅类型计数
        if let Some(types) = types {
            for t in &types {
                let type_name = t.type_name();
                let mut counts = self.type_counts.write().await;
                if let Some(count) = counts.get_mut(&type_name) {
                    *count -= 1;

                    // 如果无订阅者，停止对应的采集器
                    if *count == 0 {
                        counts.remove(&type_name);
                        self.stop_collector(&t).await?;
                    }
                }
            }
        }

        Ok(())
    }

    /// 启动采集器
    async fn start_collector(&self, subscription_type: &SubscriptionType) -> Result<()> {
        match subscription_type {
            SubscriptionType::Metrics { .. } => {
                self.metrics_collector.start().await;
                tracing::info!("启动 MetricsCollector");
                Ok(())
            }
            _ => anyhow::bail!("订阅类型尚未实现"),
        }
    }

    /// 停止采集器
    async fn stop_collector(&self, subscription_type: &SubscriptionType) -> Result<()> {
        match subscription_type {
            SubscriptionType::Metrics { .. } => {
                self.metrics_collector.stop().await;
                tracing::info!("停止 MetricsCollector");
                Ok(())
            }
            _ => anyhow::bail!("订阅类型尚未实现"),
        }
    }

    /// 检查是否有订阅者
    pub async fn has_subscribers(&self, event_type: &str) -> bool {
        let counts = self.type_counts.read().await;
        counts.get(event_type).map(|v| *v > 0).unwrap_or(false)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::SubscriptionType;

    /// 订阅覆盖泄漏（M10）：同 stream_id 重复订阅必须先回收旧计数，
    /// 否则旧类型计数只增不减、采集器永不停止
    #[tokio::test]
    async fn test_resubscribe_reclaims_old_counts() {
        let manager = SubscriptionManager::new(
            crate::config::AgentConfig::default(),
            Arc::new(EventBus::new()),
        );
        let metrics = || SubscriptionType::Metrics { interval_secs: Some(1) };

        manager.subscribe(1, vec![metrics()]).await.unwrap();
        assert!(manager.has_subscribers("metrics").await);

        // 同 stream 重复订阅（覆盖路径）：计数必须仍为 1
        manager.subscribe(1, vec![metrics()]).await.unwrap();
        assert!(manager.has_subscribers("metrics").await);

        // 全部移除后归零（若泄漏，这里计数仍为 1 → 采集器永不停止）
        manager.remove_all(1).await.unwrap();
        assert!(!manager.has_subscribers("metrics").await);
    }

    /// 多 stream 计数：互不影响，逐一移除后归零
    #[tokio::test]
    async fn test_multiple_streams_counted_independently() {
        let manager = SubscriptionManager::new(
            crate::config::AgentConfig::default(),
            Arc::new(EventBus::new()),
        );
        let metrics = || SubscriptionType::Metrics { interval_secs: Some(1) };

        manager.subscribe(1, vec![metrics()]).await.unwrap();
        manager.subscribe(2, vec![metrics()]).await.unwrap();
        assert!(manager.has_subscribers("metrics").await);

        manager.remove_all(1).await.unwrap();
        assert!(manager.has_subscribers("metrics").await, "另一 stream 仍订阅");

        manager.remove_all(2).await.unwrap();
        assert!(!manager.has_subscribers("metrics").await);
    }
}
