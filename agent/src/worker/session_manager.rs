//! 会话管理器 - 管理活动的 PTY 会话
//!
//! 该模块负责：
//! - 跟踪所有活动的 PTY 会话
//! - 处理会话的创建/销毁
//! - 监控子进程退出事件
//! - 清理退出的会话资源
//!
//! # 架构位置
//!
//! ```
//! Manager (Gateway Layer)
//!     ↓ CreateSession 请求
//! Worker (Isolation Layer) ← SessionManager 在此运行
//!     ↓ forkpty
//! Session (Terminal Process)
//!     ↓ 退出
//! SIGCHLD → SessionManager.cleanup_by_pid()
//! ```

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use nix::unistd::Pid;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

/// 会话信息
///
/// 记录单个 PTY 会话的元数据。
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// 会话唯一标识符（UUID）
    pub session_id: String,
    /// 子进程 PID
    pub pid: Pid,
    /// Shell 程序路径
    pub shell: String,
    /// 创建时间
    pub created_at: SystemTime,
}

/// 会话管理器
///
/// 负责跟踪和管理所有活动的 PTY 会话。
///
/// # 线程安全
///
/// 使用 `Arc<RwLock>` 实现多任务间共享，支持并发访问。
///
/// # 示例
///
/// ```rust,ignore
/// let manager = SessionManager::new();
///
/// // 注册会话
/// manager.register("session-123".to_string(), Pid::from_raw(1234), "/bin/bash".to_string()).await;
///
/// // 查询会话
/// let info = manager.get("session-123").await;
///
/// // 启动子进程监控
/// manager.monitor_child_processes().await?;
/// ```
pub struct SessionManager {
    /// 会话注册表（session_id -> SessionInfo）
    sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,
}

impl SessionManager {
    /// 创建会话管理器实例
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let manager = SessionManager::new();
    /// ```
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册新会话
    ///
    /// # 参数
    ///
    /// - `session_id`: 会话唯一标识符
    /// - `pid`: 子进程 PID
    /// - `shell`: Shell 程序路径
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// manager.register("session-123".to_string(), Pid::from_raw(1234), "/bin/bash".to_string()).await;
    /// ```
    pub async fn register(&self, session_id: String, pid: Pid, shell: String) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.clone(), SessionInfo {
            session_id,
            pid,
            shell,
            created_at: SystemTime::now(),
        });
    }

    /// 注销会话
    ///
    /// # 参数
    ///
    /// - `session_id`: 会话唯一标识符
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// manager.unregister("session-123").await;
    /// ```
    pub async fn unregister(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
    }

    /// 查询会话信息
    ///
    /// # 参数
    ///
    /// - `session_id`: 会话唯一标识符
    ///
    /// # 返回
    ///
    /// 如果会话存在，返回 `Some(SessionInfo)`，否则返回 `None`。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let info = manager.get("session-123").await;
    /// ```
    pub async fn get(&self, session_id: &str) -> Option<SessionInfo> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// 列出所有会话
    ///
    /// # 返回
    ///
    /// 返回所有活动会话的列表。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let sessions = manager.list().await;
    /// ```
    pub async fn list(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.read().await;
        sessions.values().cloned().collect()
    }

    /// 根据 PID 清理会话
    ///
    /// # 参数
    ///
    /// - `pid`: 子进程 PID
    ///
    /// # 流程
    ///
    /// 1. 查找匹配 PID 的会话
    /// 2. 从注册表中移除
    /// 3. 记录日志
    async fn cleanup_by_pid(&self, pid: Pid) {
        let mut sessions = self.sessions.write().await;

        // 查找匹配的 session_id
        let session_id = sessions.values()
            .find(|info| info.pid == pid)
            .map(|info| info.session_id.clone());

        // 移除会话
        if let Some(id) = session_id {
            sessions.remove(&id);
            tracing::info!("会话已清理: session_id={}, pid={}", id, pid);
        }
    }

    /// 检查所有会话是否空闲（无未决请求）
    ///
    /// Phase 4 简化实现：始终返回 true（当前 SessionManager 不跟踪请求状态）
    /// 后续可扩展为跟踪每个会话的活跃请求数
    pub async fn all_idle(&self) -> bool {
        // 当前实现：SessionManager 只跟踪 PTY 会话生命周期
        // PTY 会话本身是长连接，不算"未决请求"
        true
    }

    /// 生成当前状态快照（用于状态迁移）
    ///
    /// Phase 4 暂不实现复杂状态迁移，返回空快照
    pub async fn snapshot(&self) -> Vec<u64> {
        // 返回空列表（无未决请求）
        // 后续可扩展为返回正在执行的命令的 request_id 列表
        Vec::new()
    }

    /// 监控子进程退出
    ///
    /// # 返回
    ///
    /// 成功返回 `Ok(())`，失败返回错误。
    ///
    /// # 流程
    ///
    /// 1. 循环调用 `waitpid` 等待子进程退出
    /// 2. 检测到退出时，清理对应会话
    /// 3. 继续监控下一个子进程
    ///
    /// # 注意
    ///
    /// 该方法设计为在独立的 tokio 任务中运行：
    ///
    /// ```rust,ignore
    /// let manager_clone = session_manager.clone();
    /// tokio::spawn(async move {
    ///     manager_clone.monitor_child_processes().await
    /// });
    /// ```
    pub async fn monitor_child_processes(&self) -> Result<()> {
        tracing::info!("子进程监控任务启动");

        loop {
            // 非阻塞等待任意子进程
            match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::Exited(pid, status)) => {
                    tracing::info!("子进程退出: pid={}, status={}", pid, status);
                    // 查找并清理会话
                    self.cleanup_by_pid(pid).await;
                }
                Ok(WaitStatus::Signaled(pid, sig, _)) => {
                    tracing::warn!("子进程被信号终止: pid={}, signal={:?}", pid, sig);
                    self.cleanup_by_pid(pid).await;
                }
                Ok(_) => {
                    // 其他状态（如 StillAlive），没有子进程退出
                    // 等待一段时间后再检查
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                Err(e) => {
                    // ECHILD: 没有子进程可等待
                    // 这是正常情况，继续等待
                    if e == nix::errno::Errno::ECHILD {
                        tracing::trace!("没有子进程可等待，继续监控");
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    } else {
                        tracing::error!("waitpid 错误: {}", e);
                        return Err(anyhow::anyhow!("waitpid error: {}", e));
                    }
                }
            }
        }
    }
}

impl Clone for SessionManager {
    fn clone(&self) -> Self {
        Self {
            sessions: Arc::clone(&self.sessions),
        }
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_get() {
        let manager = SessionManager::new();

        // 注册会话
        manager.register("session-1".to_string(), Pid::from_raw(1234), "/bin/bash".to_string()).await;

        // 查询会话
        let info = manager.get("session-1").await;
        assert!(info.is_some());

        let info = info.unwrap();
        assert_eq!(info.session_id, "session-1");
        assert_eq!(info.pid, Pid::from_raw(1234));
        assert_eq!(info.shell, "/bin/bash");
    }

    #[tokio::test]
    async fn test_unregister() {
        let manager = SessionManager::new();

        // 注册会话
        manager.register("session-2".to_string(), Pid::from_raw(5678), "/bin/zsh".to_string()).await;

        // 验证存在
        assert!(manager.get("session-2").await.is_some());

        // 注销会话
        manager.unregister("session-2").await;

        // 验证已移除
        assert!(manager.get("session-2").await.is_none());
    }

    #[tokio::test]
    async fn test_list() {
        let manager = SessionManager::new();

        // 注册多个会话
        manager.register("session-3".to_string(), Pid::from_raw(1111), "/bin/bash".to_string()).await;
        manager.register("session-4".to_string(), Pid::from_raw(2222), "/bin/zsh".to_string()).await;

        // 列出所有会话
        let sessions = manager.list().await;
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_concurrent_access() {
        let manager = Arc::new(SessionManager::new());

        // 并发注册会话
        let mut tasks = vec![];

        for i in 0..10 {
            let manager_clone = Arc::clone(&manager);
            let task = tokio::spawn(async move {
                manager_clone.register(
                    format!("session-{}", i),
                    Pid::from_raw(i),
                    "/bin/bash".to_string()
                ).await;
            });
            tasks.push(task);
        }

        // 等待所有任务完成
        for task in tasks {
            task.await.unwrap();
        }

        // 验证所有会话都已注册
        let sessions = manager.list().await;
        assert_eq!(sessions.len(), 10);
    }
}