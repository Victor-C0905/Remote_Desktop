// agent/src/collectors/service_status.rs

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::process::Command;
use crate::event_bus::EventBus;

/// 服务状态采集器
pub struct ServiceStatusCollector {
    event_bus: Arc<EventBus>,
    running: RwLock<bool>,
    service_name: RwLock<Option<String>>,
    interval_secs: RwLock<u64>,
    last_status: RwLock<Option<String>>,
}

impl ServiceStatusCollector {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            event_bus,
            running: RwLock::new(false),
            service_name: RwLock::new(None),
            interval_secs: RwLock::new(5),
            last_status: RwLock::new(None),
        }
    }

    /// 启动服务状态监听
    pub async fn start(&self, service: &str, interval_secs: Option<u64>) {
        // 检查是否已运行
        {
            let running = self.running.read().await;
            if *running {
                tracing::warn!("ServiceStatusCollector 已在运行");
                return;
            }
        }

        // 设置服务名称
        {
            let mut service_name = self.service_name.write().await;
            *service_name = Some(service.to_string());
        }

        // 设置检查间隔
        {
            let mut interval = self.interval_secs.write().await;
            *interval = interval_secs.unwrap_or(5);
        }

        // 标记运行
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        // 启动后台任务
        let collector = Arc::new(Self {
            event_bus: self.event_bus.clone(),
            running: RwLock::new(true),
            service_name: RwLock::new(Some(service.to_string())),
            interval_secs: RwLock::new(interval_secs.unwrap_or(5)),
            last_status: RwLock::new(None),
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

                // 检查服务状态
                collector.check_service_status().await;

                // 等待间隔
                let interval = {
                    let interval_secs = collector.interval_secs.read().await;
                    *interval_secs
                };
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            }
        });

        tracing::info!("启动 ServiceStatusCollector: service={}", service);
    }

    /// 停止服务状态监听
    pub async fn stop(&self) {
        // 检查是否已运行
        {
            let running = self.running.read().await;
            if !*running {
                tracing::warn!("ServiceStatusCollector 未在运行");
                return;
            }
        }

        // 标记停止
        {
            let mut running = self.running.write().await;
            *running = false;
        }

        tracing::info!("停止 ServiceStatusCollector");
    }

    /// 检查服务状态
    async fn check_service_status(&self) {
        // 获取服务名称
        let service_name = {
            let service_name = self.service_name.read().await;
            service_name.clone()
        };

        if let Some(service) = service_name {
            // 执行 systemctl is-active 命令
            let output = Command::new("systemctl")
                .arg("is-active")
                .arg(&service)
                .output()
                .await;

            if let Ok(output) = output {
                let status = String::from_utf8_lossy(&output.stdout).trim().to_string();

                // 检查状态是否变化
                let last_status = {
                    let last_status = self.last_status.read().await;
                    last_status.clone()
                };

                if last_status != Some(status.clone()) {
                    // 发布服务状态变化事件
                    let data = serde_json::json!({
                        "service": service,
                        "status": status,
                        "previous_status": last_status,
                    });

                    self.event_bus.publish("service_status", data).await;

                    // 更新上次状态
                    {
                        let mut last_status = self.last_status.write().await;
                        *last_status = Some(status);
                    }
                }
            }
        }
    }

    /// 设置检查间隔
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