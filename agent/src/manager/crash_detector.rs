//! Worker 崩溃检测器
//!
//! 主动监控 Worker 子进程状态，在 Worker 崩溃时触发自动重启。
//!
//! # 架构位置
//!
//! Manager 启动时 spawn 一个后台任务，定期调用 `waitpid(WNOHANG)`
//! 检测 Worker 进程是否退出。如果退出且不是优雅关闭，则触发自动重启。
//!
//! # 关键设计
//!
//! - 使用 `tokio::task::spawn_blocking` 包装同步的 `waitpid` 调用
//! - 通过 `is_graceful_shutdown` 标志区分优雅关闭和崩溃
//! - 检测间隔 500ms（平衡 CPU 占用和响应速度）

use std::sync::Arc;
use std::time::Duration;
use nix::unistd::Pid;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use tracing::{info, warn, error, debug};

use super::worker_manager::WorkerManager;

/// Worker 崩溃检测器
///
/// 在独立 tokio 任务中运行，监控 Worker 进程状态。
///
/// # 使用方式
///
/// ```rust,ignore
/// use std::sync::Arc;
/// use manager::crash_detector::WorkerCrashDetector;
///
/// let worker_manager = Arc::new(WorkerManager::new(...));
/// let mut detector = WorkerCrashDetector::new(worker_manager);
/// detector.start();
/// // 检测器将持续运行直到 Worker 优雅退出或调用 stop()
/// ```
pub struct WorkerCrashDetector {
    /// Worker 管理器
    worker_manager: Arc<WorkerManager>,
    /// 检测任务句柄
    detector_handle: Option<tokio::task::JoinHandle<()>>,
}

impl WorkerCrashDetector {
    /// 创建新的崩溃检测器
    ///
    /// # 参数
    ///
    /// - `worker_manager`: Worker 管理器
    pub fn new(worker_manager: Arc<WorkerManager>) -> Self {
        Self {
            worker_manager,
            detector_handle: None,
        }
    }

    /// 启动崩溃检测循环
    ///
    /// 在后台 tokio 任务中运行，定期调用 `waitpid` 检测 Worker 状态。
    /// 调用此方法后，检测器将持续运行直到 Worker 优雅退出或调用 `stop()`。
    pub fn start(&mut self) {
        if self.detector_handle.is_some() {
            warn!("崩溃检测器已在运行，忽略重复 start 调用");
            return;
        }

        let worker_manager = self.worker_manager.clone();

        self.detector_handle = Some(tokio::spawn(async move {
            info!("Worker 崩溃检测器启动");

            loop {
                // 获取当前 Worker 信息
                let worker_info = worker_manager.get_info().await;

                if let Some(info) = worker_info {
                    let pid = Pid::from_raw(info.pid as i32);

                    // 使用 spawn_blocking 调用同步的 waitpid
                    let wait_result = tokio::task::spawn_blocking(move || {
                        waitpid(pid, Some(WaitPidFlag::WNOHANG))
                    }).await;

                    match wait_result {
                        Ok(Ok(WaitStatus::Exited(_pid, status))) => {
                            // Worker 进程已退出
                            let is_graceful = worker_manager.is_graceful_shutdown().await;

                            if is_graceful {
                                info!(
                                    "Worker 优雅退出（GracefulShutdown）: status={}",
                                    status
                                );
                                // 优雅关闭，不触发崩溃处理，退出检测循环
                                break;
                            } else {
                                // 异常崩溃
                                error!(
                                    "Worker 异常退出: status={}, 触发自动重启",
                                    status
                                );
                                worker_manager.notify_crash().await;

                                // 尝试自动重启
                                match worker_manager.attempt_restart().await {
                                    Ok(()) => {
                                        info!("Worker 自动重启成功");
                                        // 重置优雅关闭标志（新 Worker 启动）
                                        worker_manager.reset_graceful_shutdown().await;
                                        // 继续监控新 Worker
                                    }
                                    Err(e) => {
                                        error!(
                                            "Worker 自动重启失败，停止检测器: {}",
                                            e
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(Ok(WaitStatus::Signaled(_pid, sig, _))) => {
                            // 被信号杀死
                            let is_graceful = worker_manager.is_graceful_shutdown().await;

                            if is_graceful {
                                info!(
                                    "Worker 被信号终止（优雅关闭）: signal={:?}",
                                    sig
                                );
                                break;
                            } else {
                                error!(
                                    "Worker 被信号杀死: signal={:?}, 触发自动重启",
                                    sig
                                );
                                worker_manager.notify_crash().await;

                                match worker_manager.attempt_restart().await {
                                    Ok(()) => {
                                        info!("Worker 自动重启成功");
                                        worker_manager.reset_graceful_shutdown().await;
                                    }
                                    Err(e) => {
                                        error!(
                                            "Worker 自动重启失败，停止检测器: {}",
                                            e
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(Ok(_)) => {
                            // StillAlive 或其他状态，继续等待
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        Ok(Err(nix::errno::Errno::ECHILD)) => {
                            // 子进程不存在（可能已被回收或 Worker 未启动）
                            debug!("waitpid 返回 ECHILD（无子进程），继续监控");
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        Ok(Err(e)) => {
                            error!("waitpid 错误: {}, 继续监控", e);
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        Err(e) => {
                            error!("spawn_blocking 任务失败: {}", e);
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                    }
                } else {
                    // Worker 未启动，等待
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }

            info!("Worker 崩溃检测器退出");
        }));
    }

    /// 停止崩溃检测器
    ///
    /// 中止检测任务。通常在 Manager 关闭时调用。
    pub fn stop(&mut self) {
        if let Some(handle) = self.detector_handle.take() {
            handle.abort();
            info!("崩溃检测器已停止");
        }
    }
}

impl Drop for WorkerCrashDetector {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_crash_detector_creation() {
        let worker_manager = Arc::new(WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3,
        ));

        let detector = WorkerCrashDetector::new(worker_manager);
        assert!(detector.detector_handle.is_none());
    }

    #[tokio::test]
    async fn test_start_stop() {
        let worker_manager = Arc::new(WorkerManager::new(
            "/nonexistent/binary".to_string(),
            "/tmp/test.sock".to_string(),
            3,
        ));

        let mut detector = WorkerCrashDetector::new(worker_manager);

        // 启动检测器（Worker 未启动，会进入 ECHILD 分支循环）
        detector.start();
        assert!(detector.detector_handle.is_some());

        // 短暂等待让任务运行
        tokio::time::sleep(Duration::from_millis(100)).await;

        // 停止检测器
        detector.stop();
        assert!(detector.detector_handle.is_none());
    }
}
