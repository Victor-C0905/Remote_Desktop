// agent/src/collectors/metrics.rs

use crate::event_bus::EventBus;
use crate::protocol::MetricsSnapshot;
use sysinfo::{System, Disks, Networks, CpuRefreshKind};
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

        // 创建 System 实例（持久化，避免每次创建）
        let mut sys = System::new_all();

        tokio::spawn(async move {
            // 第一次刷新（初始化 CPU 信息）
            sys.refresh_cpu_specifics(CpuRefreshKind::everything());

            loop {
                // 检查是否有订阅者
                let should_run = {
                    let r = running.read().await;
                    *r
                };

                if !should_run {
                    // 任务停止，释放 System 实例
                    tracing::info!("MetricsCollector 任务停止，释放 System 实例");
                    break;
                }

                // 等待一段时间（让 CPU 使用率计算准确）
                sleep(Duration::from_secs(interval)).await;

                // 刷新 CPU 信息
                sys.refresh_cpu_specifics(CpuRefreshKind::everything());

                // 采集数据
                let metrics = Self::collect(&mut sys);

                // 发布事件
                event_bus.publish("metrics", serde_json::to_value(metrics).unwrap()).await;
            }

            // 任务结束，System 实例会被自动释放
        });
    }

    /// 停止采集
    pub async fn stop(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }

    /// 采集系统指标
    fn collect(sys: &mut System) -> MetricsSnapshot {
        // 刷新内存信息
        sys.refresh_memory();

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

        // 计算真正被进程使用的内存（不包括 buffer/cache）
        // 与 free 命令的 "used" 一致：total - free - buffers - cache
        let mem_used = Self::get_used_memory();

        MetricsSnapshot {
            cpu_percent: sys.global_cpu_usage(),
            mem_used_bytes: mem_used,
            mem_total_bytes: sys.total_memory(),
            swap_used_bytes: sys.used_swap(),
            disks,
            network_rx_bytes: network_rx,
            network_tx_bytes: network_tx,
            uptime_secs: System::uptime() as u64,
        }
    }

    /// 从 /proc/meminfo 读取真正被进程使用的内存（不包括 buffer/cache）
    /// 使用 free 命令的官方计算方法：
    /// USED = TOTAL - FREE - BUFFERS - CACHE
    /// 其中 CACHE = Cached + SReclaimable
    fn get_used_memory() -> u64 {
        use std::fs::File;
        use std::io::{BufRead, BufReader};

        let file = File::open("/proc/meminfo");
        if let Ok(file) = file {
            let reader = BufReader::new(file);
            let mut mem_total = 0u64;
            let mut mem_free = 0u64;
            let mut buffers = 0u64;
            let mut cached = 0u64;
            let mut sreclaimable = 0u64;

            for line in reader.lines() {
                if let Ok(line) = line {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let key = parts[0];
                        let value = parts[1].parse::<u64>().unwrap_or(0);

                        match key {
                            "MemTotal:" => mem_total = value * 1024, // KB to Bytes
                            "MemFree:" => mem_free = value * 1024,
                            "Buffers:" => buffers = value * 1024,
                            "Cached:" => cached = value * 1024,
                            "SReclaimable:" => sreclaimable = value * 1024,
                            _ => {}
                        }
                    }
                }
            }

            // 使用 free 命令的官方计算方法
            // CACHE = Cached + SReclaimable
            // USED = TOTAL - FREE - BUFFERS - CACHE
            let cache = cached + sreclaimable;
            mem_total - mem_free - buffers - cache
        } else {
            // 如果无法读取 /proc/meminfo（不应该发生），返回 0
            tracing::warn!("无法读取 /proc/meminfo");
            0
        }
    }
}