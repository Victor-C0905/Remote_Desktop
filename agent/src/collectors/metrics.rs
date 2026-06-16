// agent/src/collectors/metrics.rs

use crate::event_bus::EventBus;
use crate::protocol::MetricsSnapshot;
use sysinfo::{System, Disks, Networks};
use tokio::time::{sleep, Duration};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 系统监控采集器
pub struct MetricsCollector {
    interval_secs: u64,
    event_bus: Arc<EventBus>,
    running: Arc<RwLock<bool>>,
}

impl MetricsCollector {
    pub fn new(interval_secs: u64, event_bus: Arc<EventBus>) -> Self {
        Self {
            interval_secs,
            event_bus,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// 启动采集
    pub async fn start(&self) {
        let running = self.running.clone();
        let event_bus = self.event_bus.clone();
        let interval = self.interval_secs;

        // 设置运行状态
        {
            let mut r = running.write().await;
            *r = true;
        }

        tokio::spawn(async move {
            loop {
                // 检查是否有订阅者
                let should_run = {
                    let r = running.read().await;
                    *r
                };

                if !should_run {
                    break;
                }

                // 采集数据
                let metrics = Self::collect();

                // 发布事件
                event_bus.publish("metrics", serde_json::to_value(metrics).unwrap()).await;

                // 等待下一次采集
                sleep(Duration::from_secs(interval)).await;
            }
        });
    }

    /// 停止采集
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }

    /// 采集系统指标
    fn collect() -> MetricsSnapshot {
        let mut sys = System::new_all();
        sys.refresh_all();

        let disks_obj = Disks::new_with_refreshed_list();
        let disks: Vec<_> = disks_obj
            .iter()
            .map(|d| crate::protocol::DiskInfo {
                mount_point: d.mount_point().to_string_lossy().to_string(),
                total_bytes: d.total_space(),
                used_bytes: d.total_space() - d.available_space(),
            })
            .collect();

        let networks = Networks::new_with_refreshed_list();
        let mut network_rx = 0u64;
        let mut network_tx = 0u64;
        for (_name, data) in &networks {
            network_rx += data.received();
            network_tx += data.transmitted();
        }

        MetricsSnapshot {
            cpu_percent: sys.global_cpu_usage(),
            mem_used_bytes: sys.used_memory(),
            mem_total_bytes: sys.total_memory(),
            swap_used_bytes: sys.used_swap(),
            disks,
            network_rx_bytes: network_rx,
            network_tx_bytes: network_tx,
            uptime_secs: System::uptime() as u64,
        }
    }
}