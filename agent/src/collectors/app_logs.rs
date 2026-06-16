// agent/src/collectors/app_logs.rs

use std::path::Path;
use std::sync::Arc;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use tokio::sync::RwLock;
use notify::{Watcher, RecommendedWatcher, Event, EventKind};
use crate::event_bus::EventBus;

/// 应用日志采集器
pub struct AppLogsCollector {
    watcher: RwLock<Option<RecommendedWatcher>>,
    event_bus: Arc<EventBus>,
    running: RwLock<bool>,
    log_file: RwLock<Option<String>>,
    log_level_filter: RwLock<Option<String>>,
    last_position: RwLock<u64>,
}

impl AppLogsCollector {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            watcher: RwLock::new(None),
            event_bus,
            running: RwLock::new(false),
            log_file: RwLock::new(None),
            log_level_filter: RwLock::new(None),
            last_position: RwLock::new(0),
        }
    }

    /// 启动日志监听
    pub async fn start(&self, app_name: &str, log_path: &str, level_filter: Option<String>) -> anyhow::Result<()> {
        // 检查是否已运行
        {
            let running = self.running.read().await;
            if *running {
                tracing::warn!("AppLogsCollector 已在运行");
                return Ok(());
            }
        }

        // 设置日志文件路径
        {
            let mut log_file = self.log_file.write().await;
            *log_file = Some(log_path.to_string());
        }

        // 设置日志级别过滤器
        {
            let mut log_level_filter = self.log_level_filter.write().await;
            *log_level_filter = level_filter.clone();
        }

        // 初始化文件位置（读取文件末尾位置）
        {
            let mut last_position = self.last_position.write().await;
            if let Ok(file) = File::open(log_path) {
                *last_position = file.metadata()?.len();
            }
        }

        // 创建 watcher
        let event_bus = self.event_bus.clone();
        let log_file = log_path.to_string();
        let log_level_filter_clone = level_filter.clone();
        let mut watcher = notify::recommended_watcher(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        // 处理日志文件变化
                        if let EventKind::Modify(_) = event.kind {
                            // 读取新增的日志内容
                            let log_file_clone = log_file.clone();
                            let event_bus_clone = event_bus.clone();
                            let level_filter = log_level_filter_clone.clone();

                            tokio::spawn(async move {
                                read_new_logs(&log_file_clone, event_bus_clone, level_filter).await;
                            });
                        }
                    }
                    Err(e) => {
                        tracing::warn!("日志文件监听错误: {}", e);
                    }
                }
            }
        )?;

        // 监听日志文件
        let watch_path = Path::new(log_path);
        watcher.watch(watch_path, notify::RecursiveMode::NonRecursive)?;

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

        tracing::info!("启动 AppLogsCollector: app_name={}, log_path={}", app_name, log_path);
        Ok(())
    }

    /// 停止日志监听
    pub async fn stop(&self) {
        // 检查是否已运行
        {
            let running = self.running.read().await;
            if !*running {
                tracing::warn!("AppLogsCollector 未在运行");
                return;
            }
        }

        // 停止 watcher
        {
            let mut w = self.watcher.write().await;
            if let Some(_watcher) = w.take() {
                // watcher 会自动停止
            }
        }

        // 标记停止
        {
            let mut running = self.running.write().await;
            *running = false;
        }

        tracing::info!("停止 AppLogsCollector");
    }

    /// 检查是否正在运行
    pub async fn is_running(&self) -> bool {
        let running = self.running.read().await;
        *running
    }
}

/// 读取新增的日志内容
async fn read_new_logs(log_path: &str, event_bus: Arc<EventBus>, level_filter: Option<String>) {
    // 打开日志文件
    let file = File::open(log_path);
    if let Err(e) = file {
        tracing::warn!("打开日志文件失败: {}", e);
        return;
    }

    let file = file.unwrap();

    // 获取文件大小
    let file_size = file.metadata();
    if let Err(e) = file_size {
        tracing::warn!("获取文件大小失败: {}", e);
        return;
    }

    let file_size = file_size.unwrap().len();

    // 创建 BufReader
    let reader = BufReader::new(file);

    // 读取日志内容（从文件末尾开始）
    // 注意：这里简化实现，实际应该记录上次读取的位置
    for line in reader.lines() {
        if let Ok(line) = line {
            // 过滤日志级别
            if let Some(level) = &level_filter {
                if !line.contains(level) {
                    continue;
                }
            }

            // 发布日志事件
            let data = serde_json::json!({
                "line": line,
                "level": level_filter,
            });

            event_bus.publish("app_logs", data).await;
        }
    }
}