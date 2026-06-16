// agent/src/collectors/process_events.rs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use sysinfo::{System, Pid, ProcessesToUpdate};
use crate::event_bus::EventBus;

/// 进程事件采集器
pub struct ProcessEventsCollector {
    system: RwLock<System>,
    known_processes: RwLock<HashMap<Pid, ProcessInfo>>,
    event_bus: Arc<EventBus>,
    running: RwLock<bool>,
    interval_secs: RwLock<u64>,
}

/// 进程信息（用于比较）
struct ProcessInfo {
    name: String,
    pid: Pid,
    cpu_usage: f32,
    memory: u64,
}

impl ProcessEventsCollector {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            system: RwLock::new(System::new_all()),
            known_processes: RwLock::new(HashMap::new()),
            event_bus,
            running: RwLock::new(false),
            interval_secs: RwLock::new(2),
        }
    }

    /// 启动进程事件监听
    pub async fn start(&self) {
        // 检查是否已运行
        {
            let running = self.running.read().await;
            if *running {
                tracing::warn!("ProcessEventsCollector 已在运行");
                return;
            }
        }

        // 标记运行
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        // 初始化进程列表
        {
            let mut system = self.system.write().await;
            system.refresh_all();
        }

        // 启动后台任务
        let collector = Arc::new(Self {
            system: RwLock::new(System::new_all()),
            known_processes: RwLock::new(HashMap::new()),
            event_bus: self.event_bus.clone(),
            running: RwLock::new(true),
            interval_secs: RwLock::new(2),
        });

        tokio::spawn(async move {
            loop {
                // 检查是否停止
                {
                    let running = collector.running.read().await;
                    if !*running {
                        break;
                    }
                }

                // 扫描进程
                collector.scan_processes().await;

                // 等待间隔
                let interval = {
                    let interval_secs = collector.interval_secs.read().await;
                    *interval_secs
                };
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            }
        });

        tracing::info!("启动 ProcessEventsCollector");
    }

    /// 停止进程事件监听
    pub async fn stop(&self) {
        // 检查是否已运行
        {
            let running = self.running.read().await;
            if !*running {
                tracing::warn!("ProcessEventsCollector 未在运行");
                return;
            }
        }

        // 标记停止
        {
            let mut running = self.running.write().await;
            *running = false;
        }

        tracing::info!("停止 ProcessEventsCollector");
    }

    /// 扫描进程列表
    async fn scan_processes(&self) {
        let mut system = self.system.write().await;
        system.refresh_processes(ProcessesToUpdate::All, true);

        let current_processes: HashMap<Pid, ProcessInfo> = system.processes()
            .iter()
            .map(|(pid, process)| {
                (*pid, ProcessInfo {
                    name: process.name().to_string_lossy().to_string(),
                    pid: *pid,
                    cpu_usage: process.cpu_usage(),
                    memory: process.memory(),
                })
            })
            .collect();

        // 检测新进程
        {
            let known = self.known_processes.read().await;
            for (pid, info) in &current_processes {
                if !known.contains_key(pid) {
                    // 发布进程创建事件
                    let data = serde_json::json!({
                        "event": "create",
                        "pid": pid.as_u32(),
                        "name": info.name,
                        "cpu_usage": info.cpu_usage,
                        "memory": info.memory,
                    });

                    self.event_bus.publish("process_events", data).await;
                }
            }
        }

        // 检测退出进程
        {
            let known = self.known_processes.read().await;
            for (pid, info) in known.iter() {
                if !current_processes.contains_key(pid) {
                    // 发布进程退出事件
                    let data = serde_json::json!({
                        "event": "exit",
                        "pid": pid.as_u32(),
                        "name": info.name,
                    });

                    self.event_bus.publish("process_events", data).await;
                }
            }
        }

        // 更新已知进程列表
        {
            let mut known = self.known_processes.write().await;
            *known = current_processes;
        }
    }

    /// 设置扫描间隔
    pub async fn set_interval(&self, interval_secs: u64) {
        let mut interval = self.interval_secs.write().await;
        *interval = interval_secs;
    }

    /// 检查是否正在运行
    pub async fn is_running(&self) -> bool {
        let running = self.running.read().await;
        *running
    }
}