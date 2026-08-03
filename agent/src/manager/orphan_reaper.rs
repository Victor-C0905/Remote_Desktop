//! 孤儿进程回收器
//!
//! 负责：
//! - 管理 Session PID -> session_id 的映射
//! - 在 PTY EOF 时回收退出的 Session 僵尸进程
//! - 清理相关资源（PtyRegistry 记录）
//!
//! # 架构位置
//!
//! ```text
//! Worker 退出
//!     ↓
//! Sessions (bash/zsh) 被 init 领养
//!     ↓
//! Manager 仍持有 master_fd
//!     ↓
//! Session 退出 → master_fd 返回 EOF
//!     ↓
//! OrphanProcessReaper.reap_zombie(pid)
//! ```
//!
//! # 关键设计
//!
//! Manager 不是 Sessions 的父进程（Worker fork 的 Sessions）。
//! `waitpid` 可能返回 `ECHILD`（Sessions 已被 init 领养），
//! 此时只需清理 PtyRegistry 中的记录即可。

use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use tokio::sync::RwLock;
use nix::unistd::Pid;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use tracing::{info, warn, debug, error};

use super::pty_registry::PtyRegistry;

/// 孤儿进程回收器
///
/// 负责回收 Worker fork 出的 Session 进程在退出后的僵尸进程。
/// 注意：Manager 不是 Sessions 的父进程，waitpid 可能返回 ECHILD。
///
/// # 线程安全
///
/// 使用 `Arc<RwLock>` 实现多任务间共享，支持并发访问。
pub struct OrphanProcessReaper {
    /// PTY 注册表（用于注销会话）
    pty_registry: Arc<PtyRegistry>,

    /// Session PID -> session_id 的映射
    pid_map: Arc<RwLock<HashMap<Pid, String>>>,
}

impl OrphanProcessReaper {
    /// 创建新的孤儿进程回收器
    ///
    /// # 参数
    ///
    /// - `pty_registry`: PTY 注册表（共享引用）
    pub fn new(pty_registry: Arc<PtyRegistry>) -> Self {
        Self {
            pty_registry,
            pid_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册 Session PID 与 session_id 的映射
    ///
    /// 在 Worker 创建新 PTY 会话后调用，记录 pid -> session_id 映射，
    /// 以便后续在 PTY EOF 时能找到对应的 session_id。
    ///
    /// # 参数
    ///
    /// - `pid`: Session 子进程的 PID
    /// - `session_id`: 对应的会话 ID
    pub async fn register(&self, pid: Pid, session_id: String) {
        let mut map = self.pid_map.write().await;
        map.insert(pid, session_id);
        debug!("已注册 Session PID 映射: pid={}", pid);
    }

    /// 根据 session_id 获取对应的 PID
    ///
    /// # 参数
    ///
    /// - `session_id`: 会话 ID
    ///
    /// # 返回
    ///
    /// 如果找到返回 `Some(Pid)`，否则返回 `None`。
    pub async fn get_pid(&self, session_id: &str) -> Option<Pid> {
        let map = self.pid_map.read().await;
        map.iter()
            .find(|(_, sid)| sid.as_str() == session_id)
            .map(|(pid, _)| *pid)
    }

    /// 回收僵尸进程
    ///
    /// 使用 `waitpid(WNOHANG)` 非阻塞回收。
    ///
    /// # 参数
    ///
    /// - `pid`: 要回收的进程 PID
    ///
    /// # 返回
    ///
    /// 成功返回 `Ok(())`。即使 waitpid 返回 ECHILD（已被 init 回收）也视为成功。
    pub async fn reap_zombie(&self, pid: Pid) -> Result<()> {
        debug!("尝试回收僵尸进程: pid={}", pid);

        // 使用 spawn_blocking 包装同步的 waitpid 调用
        let result = tokio::task::spawn_blocking(move || {
            waitpid(pid, Some(WaitPidFlag::WNOHANG))
        }).await;

        match result {
            Ok(Ok(WaitStatus::Exited(pid, status))) => {
                info!("僵尸进程已回收: pid={}, status={}", pid, status);
                self.cleanup_session(pid).await;
            }
            Ok(Ok(WaitStatus::Signaled(pid, sig, _))) => {
                warn!("进程被信号终止: pid={}, signal={:?}", pid, sig);
                self.cleanup_session(pid).await;
            }
            Ok(Ok(WaitStatus::StillAlive)) => {
                // 进程还活着，PTY EOF 可能是其他原因
                warn!("PTY EOF 但进程仍存活: pid={}", pid);
                // 不清理，进程可能还会继续运行
            }
            Ok(Ok(_)) => {
                // 其他状态（Continue、Stopped 等）
                debug!("进程处于其他状态: pid={}", pid);
            }
            Ok(Err(nix::errno::Errno::ECHILD)) => {
                // 子进程已被 init 领养并回收
                debug!("进程已被 init 回收: pid={}", pid);
                self.cleanup_session(pid).await;
            }
            Ok(Err(e)) => {
                error!("waitpid 失败: pid={}, error={}", pid, e);
                // 仍然清理映射，避免内存泄漏
                self.cleanup_session(pid).await;
            }
            Err(e) => {
                error!("spawn_blocking 任务失败: pid={}, error={}", pid, e);
                self.cleanup_session(pid).await;
            }
        }

        Ok(())
    }

    /// 清理 session 映射和 PtyRegistry 记录
    ///
    /// # 参数
    ///
    /// - `pid`: 已退出的进程 PID
    async fn cleanup_session(&self, pid: Pid) {
        // 从 pid_map 中移除
        let session_id = {
            let mut map = self.pid_map.write().await;
            map.remove(&pid)
        };

        // 从 PtyRegistry 中注销
        if let Some(session_id) = session_id {
            debug!("清理会话资源: session_id={}", session_id);
            let _ = self.pty_registry.unregister(&session_id).await;
        }
    }

    /// 获取当前跟踪的会话数量
    pub async fn session_count(&self) -> usize {
        let map = self.pid_map.read().await;
        map.len()
    }
}

impl Clone for OrphanProcessReaper {
    fn clone(&self) -> Self {
        Self {
            pty_registry: Arc::clone(&self.pty_registry),
            pid_map: Arc::clone(&self.pid_map),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_get_pid() {
        let pty_registry = Arc::new(PtyRegistry::new());
        let reaper = OrphanProcessReaper::new(pty_registry);

        let pid = Pid::from_raw(12345);
        reaper.register(pid, "session-test-1".to_string()).await;

        let found = reaper.get_pid("session-test-1").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap(), pid);
    }

    #[tokio::test]
    async fn test_get_pid_not_found() {
        let pty_registry = Arc::new(PtyRegistry::new());
        let reaper = OrphanProcessReaper::new(pty_registry);

        let found = reaper.get_pid("nonexistent-session").await;
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_session_count() {
        let pty_registry = Arc::new(PtyRegistry::new());
        let reaper = OrphanProcessReaper::new(pty_registry);

        assert_eq!(reaper.session_count().await, 0);

        reaper.register(Pid::from_raw(111), "session-1".to_string()).await;
        reaper.register(Pid::from_raw(222), "session-2".to_string()).await;

        assert_eq!(reaper.session_count().await, 2);
    }

    #[tokio::test]
    async fn test_reap_nonexistent_process() {
        // 回收一个不存在的 PID，应返回 ECHILD 而非 panic
        let pty_registry = Arc::new(PtyRegistry::new());
        let reaper = OrphanProcessReaper::new(pty_registry);

        reaper.register(Pid::from_raw(99999), "ghost-session".to_string()).await;

        // waitpid 对不存在的子进程返回 ECHILD
        let result = reaper.reap_zombie(Pid::from_raw(99999)).await;
        assert!(result.is_ok());

        // 映射应已被清理
        assert_eq!(reaper.session_count().await, 0);
    }
}
