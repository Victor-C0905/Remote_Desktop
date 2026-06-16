// agent/src/collectors/file_changes.rs

use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use notify::{Watcher, RecommendedWatcher, Event, EventKind};
use crate::event_bus::EventBus;

/// 文件变化采集器
pub struct FileChangesCollector {
    watcher: RwLock<Option<RecommendedWatcher>>,
    event_bus: Arc<EventBus>,
    running: RwLock<bool>,
}

impl FileChangesCollector {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            watcher: RwLock::new(None),
            event_bus,
            running: RwLock::new(false),
        }
    }

    /// 启动文件变化监听
    pub async fn start(&self, path: &str, recursive: bool) -> anyhow::Result<()> {
        // 检查是否已运行
        {
            let running = self.running.read().await;
            if *running {
                tracing::warn!("FileChangesCollector 已在运行");
                return Ok(());
            }
        }

        // 创建 watcher
        let event_bus = self.event_bus.clone();
        let mut watcher = notify::recommended_watcher(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // 处理文件变化事件
                        if let EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) = event.kind {
                            // 发布事件
                            let paths = event.paths.iter()
                                .map(|p| p.to_string_lossy().to_string())
                                .collect::<Vec<_>>();

                            let data = serde_json::json!({
                                "paths": paths,
                                "kind": match event.kind {
                                    EventKind::Create(_) => "create",
                                    EventKind::Modify(_) => "modify",
                                    EventKind::Remove(_) => "remove",
                                    _ => "unknown",
                                },
                            });

                            // 异步发布事件（使用 tokio::spawn）
                            let event_bus_clone = event_bus.clone();
                            tokio::spawn(async move {
                                event_bus_clone.publish("file_changes", data).await;
                            });
                        }
                    }
                    Err(e) => {
                        tracing::warn!("文件变化监听错误: {}", e);
                    }
                }
            }
        )?;

        // 监听路径
        let watch_path = Path::new(path);
        let mode = if recursive {
            notify::RecursiveMode::Recursive
        } else {
            notify::RecursiveMode::NonRecursive
        };

        watcher.watch(watch_path, mode)?;

        // 保存 watcher
        {
            let mut w = self.watcher.write().await;
            *w = Some(watcher);
        }

        // 标记运行
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        tracing::info!("启动 FileChangesCollector: path={}, recursive={}", path, recursive);
        Ok(())
    }

    /// 停止文件变化监听
    pub async fn stop(&self) {
        // 检查是否已运行
        {
            let running = self.running.read().await;
            if !*running {
                tracing::warn!("FileChangesCollector 未在运行");
                return;
            }
        }

        // 停止 watcher
        {
            let mut w = self.watcher.write().await;
            if let Some(watcher) = w.take() {
                // watcher 会自动停止
            }
        }

        // 标记停止
        {
            let mut running = self.running.write().await;
            *running = false;
        }

        tracing::info!("停止 FileChangesCollector");
    }

    /// 检查是否正在运行
    pub async fn is_running(&self) -> bool {
        let running = self.running.read().await;
        *running
    }
}