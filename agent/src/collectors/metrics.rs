// agent/src/collectors/metrics.rs

use crate::event_bus::EventBus;
use crate::protocol::MetricsSnapshot;
use sysinfo::{System, Disks, CpuRefreshKind};
use tokio::time::{sleep, Duration};
use std::sync::Arc;
use tokio::sync::Mutex;

/// 系统监控采集器
///
/// 生命周期由采集任务句柄管理：start 幂等（重复调用忽略），stop 立即
/// abort 在跑任务——此前用 running 布尔标志，stop→start 快速切换时旧
/// 任务在 sleep 间隔内看不到 false、醒来后读到新 start 的 true，导致
/// 双任务双倍采集/发布（2026-11 审计 M10）
pub struct MetricsCollector {
    interval_secs: u64,
    event_bus: Arc<EventBus>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl MetricsCollector {
    pub fn new(interval_secs: u64, event_bus: Arc<EventBus>) -> Self {
        Self {
            interval_secs,
            event_bus,
            task: Mutex::new(None),
        }
    }

    /// 启动采集（幂等：已有存活任务时忽略）
    pub async fn start(&self) {
        let mut task_guard = self.task.lock().await;
        if task_guard
            .as_ref()
            .is_some_and(|h| !h.is_finished())
        {
            tracing::debug!("MetricsCollector 已在运行，忽略重复 start");
            return;
        }

        let event_bus = self.event_bus.clone();
        let interval = self.interval_secs;

        // 创建 System 实例（持久化，避免每次创建）
        let mut sys = System::new_all();

        let handle = tokio::spawn(async move {
            // 第一次刷新（初始化 CPU 信息）
            sys.refresh_cpu_specifics(CpuRefreshKind::everything());

            loop {
                // 等待一段时间（让 CPU 使用率计算准确）
                sleep(Duration::from_secs(interval)).await;

                // 刷新 CPU 信息
                sys.refresh_cpu_specifics(CpuRefreshKind::everything());

                // 采集数据
                let metrics = Self::collect(&mut sys);

                // 发布事件。to_value 对可序列化结构不会失败，仍按防御处理：
                // 失败跳过本帧（unwrap 一旦触发即杀掉整个采集任务，审计 L10）
                match serde_json::to_value(&metrics) {
                    Ok(value) => event_bus.publish("metrics", value).await,
                    Err(e) => tracing::warn!("指标序列化失败，跳过本帧: {}", e),
                }
            }

            // 任务结束，System 实例会被自动释放
        });

        *task_guard = Some(handle);
    }

    /// 停止采集（立即 abort 在跑任务；无任务时为空操作）
    pub async fn stop(&self) {
        let mut task_guard = self.task.lock().await;
        if let Some(handle) = task_guard.take() {
            handle.abort();
            tracing::info!("MetricsCollector 任务已停止");
        }
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

        // 直接读取 /proc/net/dev 获取网络数据（兼容 WSL 镜像网络模式）
        let (network_rx, network_tx) = Self::get_network_stats();
        // 网络总计使用 TRACE 级别，避免在 DEBUG 模式下频繁输出
        tracing::trace!("网络总计: 接收 {} bytes, 发送 {} bytes", network_rx, network_tx);

        // 计算真正被进程使用的内存（不包括 buffer/cache）
        // 与 free 命令的 "used" 一致：total - free - buffers - cache
        let mem_used = Self::get_used_memory();

        MetricsSnapshot {
            cpu_percent: sys.global_cpu_usage(),
            mem_used_bytes: mem_used,
            mem_total_bytes: sys.total_memory(),
            swap_used_bytes: sys.used_swap(),
            swap_total_bytes: sys.total_swap(),
            disks,
            network_rx_bytes: network_rx,
            network_tx_bytes: network_tx,
            uptime_secs: System::uptime() as u64,
        }
    }

    /// 从 /proc/net/dev 读取网络统计数据（兼容 WSL 镜像网络模式）
    fn get_network_stats() -> (u64, u64) {
        use std::fs::File;
        use std::io::{BufRead, BufReader};

        let file = File::open("/proc/net/dev");
        if let Ok(file) = file {
            let reader = BufReader::new(file);
            let mut total_rx = 0u64;
            let mut total_tx = 0u64;

            for line in reader.lines() {
                if let Ok(line) = line {
                    // 跳过前两行（标题行）
                    if line.contains("Inter-") || line.contains("face") {
                        continue;
                    }

                    // 解析网络接口行
                    // 格式: "interface: rx_bytes rx_packets ... tx_bytes tx_packets ..."
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 17 {
                        // 接口名称（去掉冒号）
                        let interface = parts[0].replace(':', "");
                        // 接收字节数（第 2 列）
                        let rx_bytes = parts[1].parse::<u64>().unwrap_or(0);
                        // 发送字节数（第 10 列）
                        let tx_bytes = parts[9].parse::<u64>().unwrap_or(0);

                        // 排除 lo（本地回环接口）
                        if interface != "lo" {
                            total_rx += rx_bytes;
                            total_tx += tx_bytes;
                            // 网络接口详情使用 TRACE 级别，避免在 DEBUG 模式下输出过多日志
                            tracing::trace!(
                                "网络接口 {}: 接收 {} bytes, 发送 {} bytes",
                                interface,
                                rx_bytes,
                                tx_bytes
                            );
                        }
                    }
                }
            }

            (total_rx, total_tx)
        } else {
            tracing::warn!("无法读取 /proc/net/dev");
            (0, 0)
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