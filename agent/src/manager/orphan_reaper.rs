//! 孤儿进程回收器（简化版）
//!
//! # 新架构说明
//!
//! Session 进程独立运行（不依赖 Worker 或 Manager），持有 master_fd 并自行管理
//! bash 进程的 waitpid。Worker/Manager 重启不影响 Session 进程。
//!
//! Manager 只需在检测到 socket 断开时清理 PtyRegistry 记录（发送 Close 帧给
//! Session 进程，断开 UnixSocket 连接）。
//!
//! 旧架构中 Manager 通过 waitpid 回收 Session 僵尸进程的逻辑已移除：
//! - Session 进程的父进程是 Worker，Worker 退出后 Session 被 init 领养
//! - Session 进程退出后由 init 自动回收，Manager 无需 waitpid
//! - Manager 不是 Session 的父进程，waitpid 会返回 ECHILD

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn};

use super::pty_registry::PtyRegistry;

/// 孤儿进程回收器（简化版）
///
/// 新架构中 Session 进程独立运行，Manager 不再需要 waitpid 回收僵尸进程。
/// 此结构保留为 PtyRegistry 的轻量包装，提供 session 清理接口。
///
/// # 线程安全
///
/// 使用 `Arc` 共享 PtyRegistry，支持并发访问。
pub struct OrphanProcessReaper {
    /// PTY 注册表（用于注销会话）
    pty_registry: Arc<PtyRegistry>,
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
        }
    }

    /// 清理已断开的 Session
    ///
    /// 从 PtyRegistry 注销会话（内部会发送 Close 帧给 Session 进程）。
    /// 在检测到 socket 断开或 Session 异常时调用。
    ///
    /// # 参数
    ///
    /// - `session_id`: 会话 ID
    pub async fn cleanup_session(&self, session_id: &str) -> Result<()> {
        warn!("清理已断开的 Session: session_id={}", session_id);
        self.pty_registry.unregister(session_id).await?;
        info!("Session 清理完成: session_id={}", session_id);
        Ok(())
    }
}

impl Clone for OrphanProcessReaper {
    fn clone(&self) -> Self {
        Self {
            pty_registry: Arc::clone(&self.pty_registry),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cleanup_nonexistent_session() {
        // 清理不存在的会话应返回 Ok(None) 而非 panic
        let pty_registry = Arc::new(PtyRegistry::new());
        let reaper = OrphanProcessReaper::new(pty_registry);

        let result = reaper.cleanup_session("nonexistent-session").await;
        assert!(result.is_ok());
    }
}
