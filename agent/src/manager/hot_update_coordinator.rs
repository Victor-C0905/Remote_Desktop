//! 热更新协调器
//!
//! 作为 Manager 的核心组件，协调 Worker 进程的优雅热更新。
//! 支持多种触发路径（SIGHUP、QUIC 命令、CLI 工具），统一汇聚到此协调器。
//!
//! # 工作流程
//!
//! 1. 接收触发信号（从 mpsc 通道）
//! 2. 启动新 Worker（新版二进制）
//! 3. 等待新 Worker IPC 连接就绪
//! 4. 向旧 Worker 发送 GracefulShutdown
//! 5. 等待旧 Worker 退出
//! 6. 清理旧 Worker 的 IPC 连接
//! 7. 重置 is_reloading 标志
//!
//! # 关键设计
//!
//! - `is_reloading` 原子布尔值防止热更新期间重复触发
//! - 新旧 Worker 通过不同的 IPC 连接区分
//! - Sessions 在 Worker 退出后成为孤儿进程，Manager 继续持有 master_fd

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, warn, error};

use super::worker_manager::WorkerManager;
use super::signal_handler::ReloadTrigger;

/// 热更新协调器
///
/// 作为 Manager 的组件运行，接收触发信号并协调 Worker 热更新。
///
/// # 使用方式
///
/// ```rust,ignore
/// use manager::hot_update_coordinator::HotUpdateCoordinator;
/// use manager::signal_handler::ReloadTrigger;
///
/// let coordinator = HotUpdateCoordinator::new(
///     worker_manager,
///     trigger_rx,
/// );
/// tokio::spawn(coordinator.run());
/// ```
pub struct HotUpdateCoordinator {
    /// Worker 管理器
    worker_manager: Arc<WorkerManager>,
    /// 触发事件接收器
    trigger_rx: mpsc::Receiver<ReloadTrigger>,
    /// 当前是否正在执行热更新（防止重复触发）
    is_reloading: Arc<AtomicBool>,
}

impl HotUpdateCoordinator {
    /// 创建新的热更新协调器
    ///
    /// # 参数
    ///
    /// - `worker_manager`: Worker 管理器
    /// - `trigger_rx`: 触发事件接收器
    pub fn new(
        worker_manager: Arc<WorkerManager>,
        trigger_rx: mpsc::Receiver<ReloadTrigger>,
    ) -> Self {
        Self {
            worker_manager,
            trigger_rx,
            is_reloading: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 获取热更新状态标志
    ///
    /// 返回 `is_reloading` 的 Arc 引用，可用于外部检查热更新状态。
    pub fn is_reloading_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.is_reloading)
    }

    /// 触发热更新（外部调用入口）
    ///
    /// # 参数
    ///
    /// - `trigger`: 触发源
    ///
    /// # 返回
    ///
    /// 成功返回 `Ok(())`，如果正在热更新中返回 `Err`。
    pub async fn trigger_reload(&self, trigger: ReloadTrigger) -> Result<()> {
        // 检查是否已在热更新中
        if self.is_reloading.load(Ordering::SeqCst) {
            warn!("热更新正在进行中，忽略触发: {:?}", trigger);
            return Err(anyhow::anyhow!("Hot update already in progress"));
        }

        self.execute_reload(trigger).await
    }

    /// 执行热更新流程
    ///
    /// 这是热更新的核心实现，按照设计文档的步骤执行。
    ///
    /// # Phase 4 简化实现
    ///
    /// 完整的 GracefulShutdown IPC 消息流程需要新旧 Worker 共存期管理，
    /// 作为增强功能在后续版本实现。Phase 4 采用简化的 stop → start 流程：
    ///
    /// 1. 标记当前 Worker 为优雅关闭状态
    /// 2. 停止旧 Worker（SIGTERM）
    /// 3. 等待资源清理
    /// 4. 启动新 Worker
    /// 5. 等待新 Worker IPC 连接
    /// 6. 重置标志
    async fn execute_reload(&self, trigger: ReloadTrigger) -> Result<()> {
        info!("开始热更新: trigger={:?}", trigger);

        // 1. 设置 is_reloading 标志
        self.is_reloading.store(true, Ordering::SeqCst);

        // 2. 标记当前 Worker 为优雅关闭状态
        // 这样崩溃检测器在检测到 Worker 退出时不会触发自动重启
        self.worker_manager.mark_graceful_shutdown().await;

        // 3. 停止旧 Worker（会发送 SIGTERM 并等待退出）
        // 注意：Sessions 在 Worker 退出后成为孤儿进程
        // Manager 仍持有 master_fd，PTY 数据流不中断
        info!("停止旧 Worker 进程");
        if let Err(e) = self.worker_manager.stop().await {
            error!("停止旧 Worker 失败: {}", e);
            self.is_reloading.store(false, Ordering::SeqCst);
            return Err(e);
        }

        // 4. 等待一段时间确保旧 Worker 资源清理
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // 5. 启动新 Worker
        info!("启动新 Worker 进程");
        if let Err(e) = self.worker_manager.start().await {
            error!("启动新 Worker 失败: {}", e);
            self.is_reloading.store(false, Ordering::SeqCst);
            return Err(e);
        }

        // 6. 等待新 Worker IPC 连接就绪
        // Phase 4 简化：固定等待 1 秒
        // 后续可改为等待 IPC 服务器通知新 Worker 已连接
        info!("等待新 Worker IPC 连接");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // 7. 重置优雅关闭标志
        self.worker_manager.reset_graceful_shutdown().await;

        // 8. 重置 is_reloading 标志
        self.is_reloading.store(false, Ordering::SeqCst);

        info!("热更新完成: trigger={:?}", trigger);
        Ok(())
    }

    /// 启动协调器主循环
    ///
    /// 在后台监听触发事件，收到事件时调用 `execute_reload`。
    /// 通常在 Manager 启动时通过 `tokio::spawn` 调用。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let coordinator = HotUpdateCoordinator::new(...);
    /// tokio::spawn(coordinator.run());
    /// ```
    pub async fn run(mut self) -> Result<()> {
        info!("热更新协调器启动");

        while let Some(trigger) = self.trigger_rx.recv().await {
            if let Err(e) = self.trigger_reload(trigger.clone()).await {
                warn!("热更新失败: trigger={:?}, error={}", trigger, e);
            }
        }

        info!("热更新协调器退出（触发通道已关闭）");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager::worker_manager::WorkerManager;
    use crate::manager::pty_registry::PtyRegistry;

    #[tokio::test]
    async fn test_coordinator_creation() {
        let worker_manager = Arc::new(WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3,
        ));
        let (_tx, rx) = mpsc::channel(16);

        let coordinator = HotUpdateCoordinator::new(worker_manager, rx);
        assert!(!coordinator.is_reloading.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_trigger_when_reloading() {
        let worker_manager = Arc::new(WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3,
        ));
        let (_tx, rx) = mpsc::channel(16);

        let coordinator = HotUpdateCoordinator::new(worker_manager, rx);

        // 手动设置 is_reloading 标志
        coordinator.is_reloading.store(true, Ordering::SeqCst);

        // 尝试触发，应失败
        let result = coordinator.trigger_reload(ReloadTrigger::UnixSignal).await;
        assert!(result.is_err());
    }
}
