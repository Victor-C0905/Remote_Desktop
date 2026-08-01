//! Worker 进程管理器
//!
//! 负责 Worker 进程的启动、监控、重启。

use std::sync::Arc;
use std::process::Command;
use tokio::sync::RwLock;
use anyhow::Result;
use std::time::SystemTime;

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

/// Worker 进程管理器
pub struct WorkerManager {
    /// Worker 进程信息
    worker_info: Arc<RwLock<Option<WorkerInfo>>>,

    /// Agent 二进制路径
    agent_binary: String,

    /// 最大重启次数
    max_restarts: u32,
}

impl WorkerManager {
    /// 创建新的 Worker 管理器
    pub fn new(agent_binary: String, max_restarts: u32) -> Self {
        Self {
            worker_info: Arc::new(RwLock::new(None)),
            agent_binary,
            max_restarts,
        }
    }

    /// 启动 Worker 进程
    pub async fn start(&self) -> Result<()> {
        let mut info = self.worker_info.write().await;
        
        // 启动 Worker 进程
        let child = Command::new(&self.agent_binary)
            .arg("--worker")
            .spawn()?;
        
        let worker_info = WorkerInfo {
            pid: child.id(),
            started_at: SystemTime::now(),
            restart_count: 0,
            status: WorkerStatus::Running,
        };
        
        *info = Some(worker_info);
        
        Ok(())
    }

    /// 停止 Worker 进程
    pub async fn stop(&self) -> Result<()> {
        let mut info = self.worker_info.write().await;
        
        if let Some(ref mut worker_info) = *info {
            worker_info.status = WorkerStatus::Stopped;
        }
        
        Ok(())
    }

    /// 重启 Worker 进程
    pub async fn restart(&self) -> Result<()> {
        self.stop().await?;
        self.start().await?;
        Ok(())
    }

    /// 获取 Worker 信息
    pub async fn get_info(&self) -> Option<WorkerInfo> {
        let info = self.worker_info.read().await;
        info.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_worker_manager_creation() {
        let manager = WorkerManager::new("/usr/bin/agent".to_string(), 3);
        let info = manager.get_info().await;
        assert!(info.is_none());
    }
}