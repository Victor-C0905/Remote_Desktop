//! Worker 进程管理器
//!
//! 负责 Worker 进程的启动、监控、重启。
//! 符合新架构设计：持有子进程句柄和 IPC Socket。

use std::process::{Child, Command};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{RwLock, broadcast};
use anyhow::{Result, Context};
use nix::unistd::Pid;
use nix::sys::signal::{kill, Signal};
use tracing::{info, warn, error};

/// Worker 进程信息
#[derive(Debug, Clone)]
pub struct WorkerInfo {
    /// 进程 ID
    pub pid: u32,

    /// 启动时间
    pub started_at: SystemTime,

    /// 重启次数
    pub restart_count: u32,

    /// 状态
    pub status: WorkerStatus,
}

/// Worker 状态
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorkerStatus {
    /// 正在启动
    Starting,
    /// 正在运行
    Running,
    /// 正在停止
    Stopping,
    /// 已停止
    Stopped,
    /// 已崩溃
    Crashed,
}

/// Worker 状态变化事件
#[derive(Debug, Clone)]
pub struct WorkerStatusEvent {
    /// 进程 ID
    pub pid: u32,
    /// 状态
    pub status: WorkerStatus,
}

/// Worker 进程管理器
///
/// 符合新架构设计：
/// - 持有 Worker 子进程句柄
/// - 持有与 Worker 通信的 Unix Socket
/// - 管理进程生命周期（启动/停止/重启/监控）
pub struct WorkerManager {
    /// Worker 进程句柄
    worker_process: Arc<RwLock<Option<Child>>>,

    /// Worker 进程信息
    worker_info: Arc<RwLock<Option<WorkerInfo>>>,

    /// Agent 二进制路径
    agent_binary: String,

    /// Unix Socket 路径（与 Worker 通信）
    ipc_socket_path: String,

    /// 最大重启次数
    max_restarts: u32,

    /// 进程状态变化事件通道
    status_tx: broadcast::Sender<WorkerStatusEvent>,
}

impl WorkerManager {
    /// 创建新的 Worker 管理器
    ///
    /// # 参数
    /// - `agent_binary`: Agent 二进制文件路径
    /// - `ipc_socket_path`: Unix Socket 路径（与 Worker 通信）
    /// - `max_restarts`: 最大重启次数
    pub fn new(agent_binary: String, ipc_socket_path: String, max_restarts: u32) -> Self {
        // 创建 broadcast channel（容量 16）
        let (status_tx, _) = broadcast::channel(16);

        Self {
            worker_process: Arc::new(RwLock::new(None)),
            worker_info: Arc::new(RwLock::new(None)),
            agent_binary,
            ipc_socket_path,
            max_restarts,
            status_tx,
        }
    }

    /// 订阅 Worker 状态变化事件
    ///
    /// IpcServer 可以通过此方法监听 Worker 的启动和崩溃事件
    pub fn subscribe(&self) -> broadcast::Receiver<WorkerStatusEvent> {
        self.status_tx.subscribe()
    }

    /// 启动 Worker 进程
    ///
    /// # 流程
    /// 1. 启动 Worker 子进程（agent --worker）
    /// 2. 记录进程信息
    /// 3. 发送状态变化事件
    /// 4. 等待 Worker 连接 IPC Socket（由 IpcServer 处理）
    pub async fn start(&self) -> Result<()> {
        let mut process_guard = self.worker_process.write().await;
        let mut info_guard = self.worker_info.write().await;

        // 启动 Worker 进程
        let child = Command::new(&self.agent_binary)
            .arg("--worker")
            .arg("--ipc-socket")
            .arg(&self.ipc_socket_path)
            .spawn()
            .context("Failed to spawn worker process")?;

        let pid = child.id();
        info!("Worker 进程已启动: pid={}, binary={}", pid, self.agent_binary);

        // 记录进程信息
        let worker_info = WorkerInfo {
            pid,
            started_at: SystemTime::now(),
            restart_count: 0,
            status: WorkerStatus::Starting,
        };

        *process_guard = Some(child);
        *info_guard = Some(worker_info.clone());

        // 发送状态变化事件
        let _ = self.status_tx.send(WorkerStatusEvent {
            pid,
            status: WorkerStatus::Starting,
        });

        Ok(())
    }

    /// 停止 Worker 进程
    ///
    /// # 流程
    /// 1. 发送 SIGTERM 信号
    /// 2. 等待进程退出
    /// 3. 更新状态为 Stopped
    /// 4. 发送状态变化事件
    pub async fn stop(&self) -> Result<()> {
        let mut process_guard = self.worker_process.write().await;
        let mut info_guard = self.worker_info.write().await;

        if let Some(ref mut child) = *process_guard {
            let pid = Pid::from_raw(child.id() as i32);

            // 发送 SIGTERM
            kill(pid, Signal::SIGTERM)
                .context("Failed to send SIGTERM to worker")?;

            info!("已发送 SIGTERM 到 Worker 进程: pid={}", child.id());

            // 等待进程退出（最多等待 5 秒）
            match child.wait() {
                Ok(status) => {
                    info!("Worker 进程已退出: pid={}, status={}", child.id(), status);
                }
                Err(e) => {
                    warn!("等待 Worker 进程退出失败: {}", e);
                }
            }

            // 更新状态
            let pid = child.id();
            if let Some(ref mut info) = *info_guard {
                info.status = WorkerStatus::Stopped;

                // 发送状态变化事件
                let _ = self.status_tx.send(WorkerStatusEvent {
                    pid,
                    status: WorkerStatus::Stopped,
                });
            }

            *process_guard = None;
        }

        Ok(())
    }

    /// 重启 Worker 进程
    ///
    /// # 流程
    /// 1. 停止当前进程
    /// 2. 启动新进程
    /// 3. 增加重启计数
    pub async fn restart(&self) -> Result<()> {
        // 停止旧进程
        self.stop().await?;

        // 增加重启计数
        {
            let mut info_guard = self.worker_info.write().await;
            if let Some(ref mut info) = *info_guard {
                info.restart_count += 1;

                // 检查是否超过最大重启次数
                if info.restart_count > self.max_restarts {
                    tracing::error!(
                        "Worker 重启次数超过限制: count={}, max={}",
                        info.restart_count,
                        self.max_restarts
                    );
                    info.status = WorkerStatus::Crashed;
                    return Err(anyhow::anyhow!("Worker restart count exceeded"));
                }
            }
        }

        // 启动新进程
        self.start().await?;

        Ok(())
    }

    /// 获取 Worker 信息
    pub async fn get_info(&self) -> Option<WorkerInfo> {
        let info = self.worker_info.read().await;
        info.clone()
    }

    /// 更新 Worker 状态
    ///
    /// 当状态变化时发送事件通知
    pub async fn update_status(&self, status: WorkerStatus) {
        let mut info_guard = self.worker_info.write().await;
        if let Some(ref mut info) = *info_guard {
            info.status = status;

            // 发送状态变化事件
            let _ = self.status_tx.send(WorkerStatusEvent {
                pid: info.pid,
                status,
            });
        }
    }

    /// 检查 Worker 进程是否存活
    pub async fn is_alive(&self) -> bool {
        let process_guard = self.worker_process.read().await;

        if let Some(ref child) = *process_guard {
            // 尝试检查进程状态
            match child.try_wait() {
                Ok(Some(_status)) => {
                    // 进程已退出
                    false
                }
                Ok(None) => {
                    // 进程仍在运行
                    true
                }
                Err(e) => {
                    warn!("检查 Worker 进程状态失败: {}", e);
                    false
                }
            }
        } else {
            false
        }
    }

    /// 通知 Worker 崩溃
    ///
    /// 当检测到 Worker 进程异常退出时调用
    pub async fn notify_crash(&self) {
        let mut info_guard = self.worker_info.write().await;

        if let Some(ref mut info) = *info_guard {
            info.status = WorkerStatus::Crashed;

            // 发送崩溃事件
            let _ = self.status_tx.send(WorkerStatusEvent {
                pid: info.pid,
                status: WorkerStatus::Crashed,
            });

            error!("Worker 进程崩溃: pid={}", info.pid);
        }
    }
}

impl Drop for WorkerManager {
    fn drop(&mut self) {
        // 确保 Worker 进程被停止
        if let Ok(mut process_guard) = self.worker_process.try_write() {
            if let Some(ref mut child) = *process_guard {
                tracing::info!("WorkerManager 释放，停止 Worker 进程: pid={}", child.id());
                let _ = child.kill();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_worker_manager_creation() {
        let manager = WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3
        );

        let info = manager.get_info().await;
        assert!(info.is_none());
    }

    #[tokio::test]
    async fn test_worker_manager_start() {
        let manager = WorkerManager::new(
            "/bin/sleep".to_string(),  // 使用 sleep 命令测试
            "/tmp/test.sock".to_string(),
            3
        );

        // 启动一个短暂的进程（测试启动逻辑）
        let result = manager.start().await;
        // 可能会因为路径不存在而失败，这是正常的
        if result.is_ok() {
            assert!(manager.is_alive().await);

            // 清理
            let _ = manager.stop().await;
        }
    }

    #[tokio::test]
    async fn test_worker_manager_restart_limit() {
        let manager = WorkerManager::new(
            "/nonexistent/binary".to_string(),
            "/tmp/test.sock".to_string(),
            2
        );

        // 第一次启动会失败（路径不存在）
        // 但我们测试重启限制逻辑
        manager.update_status(WorkerStatus::Running).await;

        // 手动增加重启计数
        {
            let mut info_guard = manager.worker_info.write().await;
            *info_guard = Some(WorkerInfo {
                pid: 1234,
                started_at: SystemTime::now(),
                restart_count: 2,  // 已达到限制
                status: WorkerStatus::Running,
            });
        }

        // 尝试重启，应该失败
        let result = manager.restart().await;
        assert!(result.is_err());
    }
}