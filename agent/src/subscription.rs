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

    // 配置
    config: AgentConfig,
}

impl SubscriptionManager {
    pub fn new(config: AgentConfig, event_bus: Arc<EventBus>) -> Self {
        let metrics_collector = Arc::new(MetricsCollector::new(
            config.collectors.metrics_interval_secs,
            event_bus.clone(),
        ));

        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            type_counts: Arc::new(RwLock::new(HashMap::new())),
            metrics_collector,
            config,
        }
    }

    /// 添加订阅
    pub async fn subscribe(
        &self,
        stream_id: u64,
        types: Vec<SubscriptionType>,
    ) -> Result<Vec<SubscriptionType>> {
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

    /// 移除订阅
    pub async fn unsubscribe(
        &self,
        stream_id: u64,
        types: Vec<SubscriptionType>,
    ) -> Result<()> {
        // 移除订阅者
        {
            let mut subs = self.subscribers.write().await;
            if let Some(subscribed_types) = subs.get_mut(&stream_id) {
                for t in &types {
                    subscribed_types.retain(|st| st != t);
                }

                // 如果订阅类型列表为空，移除订阅者
                if subscribed_types.is_empty() {
                    subs.remove(&stream_id);
                }
            }
        }

        // 更新订阅类型计数
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

        Ok(())
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

    /// 获取订阅者数量
    pub async fn subscriber_count(&self, event_type: &str) -> usize {
        let counts = self.type_counts.read().await;
        counts.get(event_type).copied().unwrap_or(0)
    }
}