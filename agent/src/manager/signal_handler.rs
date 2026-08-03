//! 信号处理器
//!
//! 监听 Unix 信号（SIGHUP）并触发 Worker 热更新。
//!
//! # 架构位置
//!
//! Manager 启动时 spawn 一个后台任务监听 SIGHUP 信号。
//! 收到信号后通过 mpsc 通道发送触发事件给 HotUpdateCoordinator。
//!
//! # 使用方式
//!
//! ```rust,ignore
//! use manager::signal_handler::watch_sighup;
//!
//! let (tx, rx) = tokio::sync::mpsc::channel(16);
//! let handle = watch_sighup(tx);
//! // rx 接收触发事件
//! ```

use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;
use tracing::{info, error};

/// 热更新触发源
///
/// 标识热更新是由什么触发的，用于日志和审计。
/// Phase 4 新增。
#[derive(Debug, Clone)]
pub enum ReloadTrigger {
    /// 客户端通过 QUIC 发送 Reload 命令
    ClientCommand {
        /// 操作者用户名
        operator: String,
    },
    /// Unix 信号 (SIGHUP)
    UnixSignal,
    /// CLI 工具命令 (agent reload)
    CliTool,
    /// apt postinst 脚本触发
    PackagePostinst,
}

/// 监听 SIGHUP 信号
///
/// 在收到 SIGHUP 时通过 `tx` 发送 `ReloadTrigger::UnixSignal` 事件。
///
/// # 参数
///
/// - `tx`: 触发事件发送通道
///
/// # 返回
///
/// 返回任务句柄，可通过 `handle.await` 等待任务结束。
///
/// # 错误处理
///
/// 如果无法注册 SIGHUP 处理器（极少见），任务会记录错误并立即返回。
///
/// # 示例
///
/// ```rust,ignore
/// let (tx, rx) = tokio::sync::mpsc::channel(16);
/// let handle = watch_sighup(tx);
/// // 在 Manager 中使用 rx 接收触发事件
/// ```
pub fn watch_sighup(tx: mpsc::Sender<ReloadTrigger>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut sig = match signal(SignalKind::hangup()) {
            Ok(sig) => sig,
            Err(e) => {
                error!("无法注册 SIGHUP 处理器: {}", e);
                return;
            }
        };

        info!("SIGHUP 信号监听已启动");

        while sig.recv().await.is_some() {
            info!("收到 SIGHUP 信号，触发热更新");
            if let Err(e) = tx.send(ReloadTrigger::UnixSignal).await {
                error!("发送触发事件失败（接收方已关闭）: {}", e);
                break;
            }
        }

        info!("SIGHUP 信号监听结束");
    })
}

/// 监听 SIGTERM 信号（用于优雅退出）
///
/// 在收到 SIGTERM 时通过 `tx` 发送退出信号。
///
/// # 参数
///
/// - `tx`: 退出事件发送通道
pub fn watch_sigterm(tx: mpsc::Sender<()>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut sig = match signal(SignalKind::terminate()) {
            Ok(sig) => sig,
            Err(e) => {
                error!("无法注册 SIGTERM 处理器: {}", e);
                return;
            }
        };

        info!("SIGTERM 信号监听已启动");

        while sig.recv().await.is_some() {
            info!("收到 SIGTERM 信号，触发优雅退出");
            if let Err(e) = tx.send(()).await {
                error!("发送退出事件失败: {}", e);
                break;
            }
        }

        info!("SIGTERM 信号监听结束");
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_watch_sighup() {
        let (tx, mut rx) = mpsc::channel(16);
        let handle = watch_sighup(tx);

        // 给监听任务一点时间启动
        tokio::time::sleep(Duration::from_millis(50)).await;

        // 发送 SIGHUP 给当前进程
        let pid = nix::unistd::Pid::from_raw(std::process::id() as i32);
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGHUP);

        // 等待接收事件
        let result = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await;

        assert!(result.is_ok(), "应在 1 秒内收到 SIGHUP 事件");
        let trigger = result.unwrap().unwrap();
        assert!(matches!(trigger, ReloadTrigger::UnixSignal));

        handle.abort();
    }
}
