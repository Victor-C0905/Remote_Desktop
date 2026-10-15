// 进程采集：GetProcesses 请求/响应路径专用（按需拉取，非订阅推送）
//
// 专属 System 实例：与 MetricsCollector 的实例完全分离——
// sysinfo 的 cpu_usage 依赖两次刷新的时间差，共用实例会让
// 订阅推送的刷新节奏污染进程页签的采样窗口

use crate::protocol::ProcessEntry;
use std::sync::{Mutex, OnceLock};
use sysinfo::{ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, Users};

/// 进程查询专属 System 实例（进程内单例）
fn process_system() -> &'static Mutex<System> {
    static SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
    SYSTEM.get_or_init(|| Mutex::new(System::new()))
}

/// sysinfo ProcessStatus → ps 风格单字符
fn status_char(s: ProcessStatus) -> &'static str {
    match s {
        ProcessStatus::Run => "R",
        ProcessStatus::Sleep => "S",
        ProcessStatus::Idle => "I",
        ProcessStatus::Zombie => "Z",
        ProcessStatus::Tracing => "T",
        ProcessStatus::Dead => "D",
        ProcessStatus::UninterruptibleDiskSleep => "D", // 内核不可中断磁盘睡眠，ps 的真 D 态
        ProcessStatus::Stop => "T", // 信号停止/Ctrl+Z 挂起
        _ => "?",
    }
}

/// 枚举全量进程。任何已认证用户可查（与 GNOME 行为一致；
/// 内核自动限制跨用户进程的字段可见性，sysinfo 按权限降级）
pub fn list() -> Vec<ProcessEntry> {
    let mut guard = process_system().lock().unwrap();
    let sys = &mut *guard;

    // cpu_usage 依赖上一次刷新的时间差：客户端 3s 轮询天然构成采样窗口，
    // 首次调用 cpu_percent 为 0，下次轮询即准确（sysinfo 按时间差归一化）
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::everything(),
    );

    let users = Users::new_with_refreshed_list();
    let total_mem = sys.total_memory();

    sys.processes()
        .iter()
        .map(|(_pid, p)| ProcessEntry {
            pid: p.pid().as_u32(),
            name: p.name().to_string_lossy().to_string(),
            user: p
                .user_id()
                .and_then(|uid| users.iter().find(|u| u.id() == uid))
                .map(|u| u.name().to_string())
                .unwrap_or_default(),
            cpu_percent: p.cpu_usage(),
            mem_percent: if total_mem > 0 {
                (p.memory() as f32 / total_mem as f32) * 100.0
            } else {
                0.0
            },
            mem_bytes: p.memory(),
            state: status_char(p.status()).to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_returns_nonempty_with_valid_fields() {
        let list = list();
        assert!(!list.is_empty(), "当前系统至少应有 1 个进程");
        // 容器/WSL 均有 pid 1；首次调用 cpu_percent 允许为 0（无前次采样窗口）
        let init = list.iter().find(|p| p.pid == 1).expect("pid 1 必然存在");
        assert!(!init.name.is_empty());
        assert!((0.0..=100.0).contains(&init.mem_percent));
        assert_eq!(init.state.len(), 1);
    }
}

