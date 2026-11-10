// agent/src/subscription.rs

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use crate::protocol::SubscriptionType;
#[cfg(unix)]
use crate::auth::UserExecutor;
use crate::auth::UserSession;
use crate::event_bus::EventBus;
use crate::collectors::MetricsCollector;
#[cfg(unix)]
use crate::collectors::FileWatcher;
use crate::config::AgentConfig;

/// 订阅管理器
pub struct SubscriptionManager {
    // StreamId -> 订阅类型列表
    subscribers: Arc<RwLock<HashMap<u64, Vec<SubscriptionType>>>>,

    // 订阅类型 -> 订阅者数量
    type_counts: Arc<RwLock<HashMap<String, usize>>>,

    // StreamId -> 该流注册的目录监听（规范化路径）
    #[cfg(unix)]
    stream_watches: Arc<RwLock<HashMap<u64, Vec<PathBuf>>>>,

    // 目录（规范化路径）-> 引用计数：多窗口/多流看同一目录只保留一个 watch
    #[cfg(unix)]
    watch_counts: Arc<RwLock<HashMap<PathBuf, usize>>>,

    // 采集器实例
    metrics_collector: Arc<MetricsCollector>,
    #[cfg(unix)]
    file_watcher: Arc<FileWatcher>,
}

impl SubscriptionManager {
    pub fn new(_config: AgentConfig, event_bus: Arc<EventBus>) -> Self {
        let metrics_collector = Arc::new(MetricsCollector::new(
            _config.collectors.metrics_interval_secs,
            event_bus.clone(),
        ));
        #[cfg(unix)]
        let file_watcher = Arc::new(FileWatcher::new(
            _config.collectors.file_changes_delay_ms,
            event_bus,
        ));

        Self {
            subscribers: Arc::new(RwLock::new(HashMap::new())),
            type_counts: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(unix)]
            stream_watches: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(unix)]
            watch_counts: Arc::new(RwLock::new(HashMap::new())),
            metrics_collector,
            #[cfg(unix)]
            file_watcher,
        }
    }

    /// 添加订阅
    ///
    /// `session` 用于 FileChanges：目录必须在该用户权限范围内，且 stat/可读性
    /// 探测都在目标用户上下文中执行（采集器以 root 运行，不能泄露越权元数据）。
    pub async fn subscribe(
        &self,
        stream_id: u64,
        types: Vec<SubscriptionType>,
        session: &UserSession,
    ) -> Result<Vec<SubscriptionType>> {
        // 回收同 stream_id 的旧订阅计数：insert 直接覆盖会让旧类型计数
        // 只增不减，采集器永不停止（2026-11 审计 M10）。无旧订阅时为空操作
        self.remove_all(stream_id).await?;

        // 先整体校验 FileChanges 路径，避免部分注册成功后失败留下脏 watch
        let watches = self.collect_file_watches(&types, session).await?;

        // 记录订阅者
        {
            let mut subs = self.subscribers.write().await;
            subs.insert(stream_id, types.clone());
        }

        // 注册目录监听（引用计数）
        self.register_file_watches(stream_id, watches, session).await;

        // 更新订阅类型计数
        for t in &types {
            let type_name = t.type_name();
            let mut counts = self.type_counts.write().await;
            let count = counts.entry(type_name.clone()).or_insert(0);
            *count += 1;

            // 如果是第一个订阅者，启动对应的采集器
            if *count == 1 {
                self.start_collector(t).await?;
            }
        }

        Ok(types)
    }

    /// 移除所有订阅（Stream 关闭时）
    pub async fn remove_all(&self, stream_id: u64) -> Result<()> {
        // 先回收目录监听引用（FileChanges 按路径计数，不按类型计数）
        self.unregister_file_watches(stream_id).await;

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
                        self.stop_collector(t).await?;
                    }
                }
            }
        }

        Ok(())
    }

    /// 校验 FileChanges 路径：规范化 + 以目标用户身份探测可读性。
    /// 采集器以 root 运行，若不预先降权校验，会把用户无权查看的目录事件推给前端。
    #[cfg(unix)]
    async fn collect_file_watches(
        &self,
        types: &[SubscriptionType],
        session: &UserSession,
    ) -> Result<Vec<(PathBuf, PathBuf)>> {
        let mut watches: Vec<(PathBuf, PathBuf)> = Vec::new();
        for t in types {
            if let SubscriptionType::FileChanges { path, recursive } = t {
                if recursive == &Some(true) {
                    // 递归监听需要为每个子目录建 watch，成本随目录树膨胀；
                    // 文件管理器只展示直属条目，先按非递归处理
                    tracing::warn!("file_changes 递归监听未实现，按非递归处理: {}", path);
                }
                let safe = crate::auth::validate_path(path, session.home_dir.as_path(), session.uid)
                    .map_err(|e| anyhow::anyhow!("目录监听路径校验失败: {}: {}", path, e))?;
                let safe_path = safe.as_path().to_path_buf();
                let probe = safe_path.clone();
                UserExecutor::new(session)
                    .execute_as_user_blocking(move || {
                        std::fs::read_dir(&probe)?;
                        Ok(())
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("目录监听不可读: {}: {}", path, e))?;
                watches.push((safe_path, PathBuf::from(path)));
            }
        }

        // 上限：单订阅最多 8 个目录、连接/进程全局最多 256 个 watch。
        // inotify 本身有 max_user_watches，但恶意/异常客户端应在协议层先拒绝
        const MAX_WATCHES_PER_STREAM: usize = 8;
        const MAX_WATCHES_TOTAL: usize = 256;
        if watches.len() > MAX_WATCHES_PER_STREAM {
            anyhow::bail!("单次订阅目录监听数量超限");
        }
        let existing = self.watch_counts.read().await.len();
        if existing + watches.len() > MAX_WATCHES_TOTAL {
            anyhow::bail!("目录监听总数超限");
        }

        Ok(watches)
    }

    #[cfg(not(unix))]
    async fn collect_file_watches(
        &self,
        types: &[SubscriptionType],
        _session: &UserSession,
    ) -> Result<Vec<(PathBuf, PathBuf)>> {
        if types.iter().any(|t| matches!(t, SubscriptionType::FileChanges { .. })) {
            anyhow::bail!("目录监听仅支持 Unix 平台");
        }
        Ok(Vec::new())
    }

    /// 注册目录监听（同一规范化路径多流订阅只保留一个 watch）
    #[cfg(unix)]
    async fn register_file_watches(
        &self,
        stream_id: u64,
        watches: Vec<(PathBuf, PathBuf)>,
        session: &UserSession,
    ) {
        if watches.is_empty() {
            return;
        }
        let mut registered = Vec::with_capacity(watches.len());
        {
            let mut counts = self.watch_counts.write().await;
            for (safe, display) in watches {
                let count = counts.entry(safe.clone()).or_insert(0);
                *count += 1;
                if *count == 1 {
                    self.file_watcher
                        .watch(safe.clone(), display, session.clone())
                        .await;
                }
                registered.push(safe);
            }
        }
        self.stream_watches
            .write()
            .await
            .insert(stream_id, registered);
    }

    #[cfg(not(unix))]
    async fn register_file_watches(
        &self,
        _stream_id: u64,
        _watches: Vec<(PathBuf, PathBuf)>,
        _session: &UserSession,
    ) {
    }

    /// 回收目录监听（引用计数归零时真正移除 watch）
    #[cfg(unix)]
    async fn unregister_file_watches(&self, stream_id: u64) {
        if let Some(paths) = self.stream_watches.write().await.remove(&stream_id) {
            let mut counts = self.watch_counts.write().await;
            for path in paths {
                if let Some(count) = counts.get_mut(&path) {
                    *count -= 1;
                    if *count == 0 {
                        counts.remove(&path);
                        self.file_watcher.unwatch(&path);
                    }
                }
            }
        }
    }

    #[cfg(not(unix))]
    async fn unregister_file_watches(&self, _stream_id: u64) {}

    /// 启动采集器
    async fn start_collector(&self, subscription_type: &SubscriptionType) -> Result<()> {
        match subscription_type {
            SubscriptionType::Metrics { .. } => {
                self.metrics_collector.start().await;
                tracing::info!("启动 MetricsCollector");
                Ok(())
            }
            // FileChanges 的 watch 生命周期按路径引用计数管理（见 subscribe），
            // 这里无需额外启动动作
            SubscriptionType::FileChanges { .. } => Ok(()),
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
            SubscriptionType::FileChanges { .. } => Ok(()),
            _ => anyhow::bail!("订阅类型尚未实现"),
        }
    }

    /// 检查是否有订阅者
    pub async fn has_subscribers(&self, event_type: &str) -> bool {
        let counts = self.type_counts.read().await;
        counts.get(event_type).map(|v| *v > 0).unwrap_or(false)
    }

    /// 某个流是否订阅了指定事件类型。
    /// 多订阅流共存（metrics + file_changes）时按流过滤，避免把无关事件
    /// 推给不该收到它的流。
    pub async fn stream_subscribes(&self, stream_id: u64, event_type: &str) -> bool {
        let subs = self.subscribers.read().await;
        subs.get(&stream_id)
            .map(|types| types.iter().any(|t| t.type_name() == event_type))
            .unwrap_or(false)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::SubscriptionType;

    /// 测试用会话：uid=0 → UserExecutor 跳过 fork；本组用例只订阅 metrics
    fn test_session() -> crate::auth::UserSession {
        crate::auth::UserSession {
            session_id: "test".to_string(),
            username: "root".to_string(),
            uid: 0,
            gid: 0,
            home_dir: std::path::PathBuf::from("/"),
            shell: std::path::PathBuf::from("/bin/bash"),
            created_at: std::time::SystemTime::now(),
        }
    }

    /// 订阅覆盖泄漏（M10）：同 stream_id 重复订阅必须先回收旧计数，
    /// 否则旧类型计数只增不减、采集器永不停止
    #[tokio::test]
    async fn test_resubscribe_reclaims_old_counts() {
        let manager = SubscriptionManager::new(
            crate::config::AgentConfig::default(),
            Arc::new(EventBus::new()),
        );
        let session = test_session();
        let metrics = || SubscriptionType::Metrics { interval_secs: Some(1) };

        manager.subscribe(1, vec![metrics()], &session).await.unwrap();
        assert!(manager.has_subscribers("metrics").await);

        // 同 stream 重复订阅（覆盖路径）：计数必须仍为 1
        manager.subscribe(1, vec![metrics()], &session).await.unwrap();
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
        let session = test_session();
        let metrics = || SubscriptionType::Metrics { interval_secs: Some(1) };

        manager.subscribe(1, vec![metrics()], &session).await.unwrap();
        manager.subscribe(2, vec![metrics()], &session).await.unwrap();
        assert!(manager.has_subscribers("metrics").await);

        manager.remove_all(1).await.unwrap();
        assert!(manager.has_subscribers("metrics").await, "另一 stream 仍订阅");

        manager.remove_all(2).await.unwrap();
        assert!(!manager.has_subscribers("metrics").await);
    }
}
