//! 系统信息处理器 - 处理系统信息查询请求
//!
//! 该模块负责：
//! - 收集系统信息（GetSystemInfo）
//! - CPU、内存、磁盘等指标采集

use sysinfo::System;

use crate::protocol::generated::{
    GetSystemInfo, SystemInfo, CpuInfo, MemoryInfo, DiskInfo,
    WorkerResponse, worker_response, Error,
};

/// 处理 GetSystemInfo 请求
///
/// # 参数
///
/// - `_req`: GetSystemInfo 请求参数（目前为空）
///
/// # 返回
///
/// 返回 `WorkerResponse`，包含 `SystemInfo` 或 `Error`。
///
/// # 示例
///
/// ```rust,ignore
/// let response = handle_get_system_info(req).await;
/// match response.payload {
///     Some(worker_response::Payload::SystemInfo(info)) => {
///         println!("主机名: {}", info.hostname);
///         println!("CPU 使用率: {}%", info.cpu.unwrap().usage_percent);
///     }
///     _ => { /* 错误处理 */ }
/// }
/// ```
#[tracing::instrument]
pub async fn handle_get_system_info(_req: GetSystemInfo) -> WorkerResponse {
    tracing::info!("处理 GetSystemInfo 请求");

    // 创建 System 实例并刷新数据
    let mut sys = System::new_all();
    sys.refresh_all();

    // 获取主机名
    let hostname = System::host_name()
        .unwrap_or_else(|| "unknown".to_string());

    // 获取操作系统信息
    let os_name = System::name()
        .unwrap_or_else(|| "unknown".to_string());

    let os_version = System::os_version()
        .unwrap_or_else(|| "unknown".to_string());

    let kernel_version = System::kernel_version()
        .unwrap_or_else(|| "unknown".to_string());

    // 获取系统运行时间（秒）
    let uptime_secs = System::uptime();

    // 获取 CPU 信息
    let cpu_info = collect_cpu_info(&sys);

    // 获取内存信息
    let memory_info = collect_memory_info(&sys);

    // 获取磁盘信息
    let disks = collect_disk_info(&sys);

    tracing::debug!(
        "系统信息采集完成: hostname={}, os={}, uptime={}s",
        hostname, os_name, uptime_secs
    );

    WorkerResponse {
        payload: Some(worker_response::Payload::SystemInfo(SystemInfo {
            hostname,
            os_name,
            os_version,
            kernel_version,
            uptime_secs,
            cpu: cpu_info,
            memory: memory_info,
            disks,
        })),
        ..Default::default()
    }
}

/// 收集 CPU 信息
///
/// # 参数
///
/// - `sys`: System 实例
///
/// # 返回
///
/// 返回 `CpuInfo` 实例。
fn collect_cpu_info(sys: &System) -> Option<CpuInfo> {
    // 获取 CPU 使用率（全局）
    let cpu_usage = sys.global_cpu_usage();

    // 获取 CPU 核心数
    let cores = sys.cpus().len() as u32;

    // 获取 CPU 型号
    let model = sys.cpus()
        .first()
        .map(|cpu| cpu.brand().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    Some(CpuInfo {
        model,
        cores,
        usage_percent: cpu_usage as f64,
    })
}

/// 收集内存信息
///
/// # 参数
///
/// - `sys`: System 实例
///
/// # 返回
///
/// 返回 `MemoryInfo` 实例。
fn collect_memory_info(sys: &System) -> Option<MemoryInfo> {
    let total = sys.total_memory();
    let used = sys.used_memory();
    let available = sys.available_memory();

    // 计算使用百分比
    let usage_percent = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    Some(MemoryInfo {
        total_bytes: total,
        used_bytes: used,
        available_bytes: available,
        usage_percent,
    })
}

/// 收集磁盘信息
///
/// # 参数
///
/// - `sys`: System 实例
///
/// # 返回
///
/// 返回 `DiskInfo` 列表。
fn collect_disk_info(sys: &System) -> Vec<DiskInfo> {
    use sysinfo::Disks;

    let disks = Disks::new_with_refreshed_list();

    disks.iter()
        .map(|disk| {
            let mount_point = disk.mount_point().to_string_lossy().to_string();
            let device = disk.name().to_string_lossy().to_string();
            let filesystem = disk.file_system().to_string_lossy().to_string();

            let total = disk.total_space();
            let available = disk.available_space();
            let used = total - available;

            // 计算使用百分比
            let usage_percent = if total > 0 {
                (used as f64 / total as f64) * 100.0
            } else {
                0.0
            };

            DiskInfo {
                mount_point,
                device,
                filesystem,
                total_bytes: total,
                used_bytes: used,
                available_bytes: available,
                usage_percent,
            }
        })
        .collect()
}