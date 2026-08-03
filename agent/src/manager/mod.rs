//! Manager 模块（网关层）
//!
//! 负责监听 QUIC 端口，管理客户端连接，持有 PTY master_fd，直接读写 PTY。

pub mod connection;
pub mod session;
pub mod auth;

// Unix-only 模块
#[cfg(unix)]
pub mod pty_registry;

#[cfg(unix)]
pub mod pty_output;

#[cfg(unix)]
pub mod worker_manager;

#[cfg(unix)]
pub mod ipc_server;

#[cfg(unix)]
pub mod orphan_reaper;

#[cfg(unix)]
pub mod crash_detector;

#[cfg(unix)]
pub mod signal_handler;

#[cfg(unix)]
pub mod hot_update_coordinator;

pub use connection::ConnectionManager;
pub use session::SessionManager;

#[cfg(unix)]
pub use pty_registry::{PtyRegistry, PtySession, UserInfo};

#[cfg(unix)]
pub use pty_output::{PtyOutputConfig, spawn_pty_output_task, spawn_pty_output_task_legacy};

#[cfg(unix)]
pub use worker_manager::{WorkerManager, WorkerStatus, WorkerStatusEvent};

#[cfg(unix)]
pub use ipc_server::IpcServer;

#[cfg(unix)]
pub use orphan_reaper::OrphanProcessReaper;

#[cfg(unix)]
pub use crash_detector::WorkerCrashDetector;

#[cfg(unix)]
pub use signal_handler::{ReloadTrigger, watch_sighup, watch_sigterm};

#[cfg(unix)]
pub use hot_update_coordinator::HotUpdateCoordinator;

use std::sync::Arc;
use anyhow::Result;
use crate::config::AgentConfig;

/// Manager 主结构
pub struct Manager {
    /// PTY 注册表（管理所有 master_fd）
    #[cfg(unix)]
    pty_registry: Arc<PtyRegistry>,

    /// Worker 进程管理器
    #[cfg(unix)]
    worker_manager: Arc<WorkerManager>,

    /// 用户会话管理器
    session_manager: Arc<SessionManager>,

    /// IPC Server（接收 Worker 的 FD）
    #[cfg(unix)]
    ipc_server: Arc<IpcServer>,

    /// 孤儿进程回收器
    /// Phase 4 新增：在 PTY EOF 时回收 Session 僵尸进程
    #[cfg(unix)]
    orphan_reaper: Arc<OrphanProcessReaper>,
}

impl Manager {
    /// 创建新的 Manager
    ///
    /// # 流程
    /// 1. 创建 PtyRegistry
    /// 2. 创建 WorkerManager
    /// 3. 创建 IpcServer（集成 WorkerManager）
    /// 4. 创建 SessionManager
    #[cfg(unix)]
    pub async fn new(config: &AgentConfig) -> Result<Self> {
        // 创建 PTY 注册表
        let pty_registry = Arc::new(PtyRegistry::new());

        // 创建 Worker 管理器
        let worker_manager = Arc::new(WorkerManager::new(
            config.worker.agent_binary.clone(),
            config.worker.ipc_socket_path.clone(),
            config.worker.max_restarts,
        ));

        // 创建 IPC 服务器（集成 WorkerManager）
        let ipc_server = Arc::new(IpcServer::new(
            config.worker.ipc_socket_path.clone(),
            pty_registry.clone(),
            worker_manager.clone(),
        ));

        // 创建会话管理器
        let session_manager = Arc::new(SessionManager::new());

        // 创建孤儿进程回收器
        // Phase 4 新增：用于在 PTY EOF 时回收 Session 僵尸进程
        let orphan_reaper = Arc::new(OrphanProcessReaper::new(pty_registry.clone()));

        Ok(Self {
            pty_registry,
            worker_manager,
            session_manager,
            ipc_server,
            orphan_reaper,
        })
    }

    /// 创建新的 Manager（非 Unix 平台）
    #[cfg(not(unix))]
    pub async fn new(config: &AgentConfig) -> Result<Self> {
        // 创建会话管理器
        let session_manager = Arc::new(SessionManager::new());

        Ok(Self {
            session_manager,
        })
    }

    /// 启动 Manager
    ///
    /// # 流程
    /// 1. 启动 IPC 服务器（监听 Worker 状态变化）
    /// 2. 启动 Worker 进程
    /// 3. 启动崩溃检测器（Phase 4 新增）
    /// 4. 启动 SIGHUP 信号监听（Phase 4 新增）
    /// 5. 启动热更新协调器（Phase 4 新增）
    /// 6. 等待终止信号
    pub async fn run(&mut self) -> Result<()> {
        #[cfg(unix)]
        {
            // 启动 IPC 服务器（在后台运行）
            let ipc_server = self.ipc_server.clone();
            tokio::spawn(async move {
                if let Err(e) = ipc_server.run().await {
                    tracing::error!("IPC 服务器运行失败: {}", e);
                }
            });

            // 给 IPC 服务器一点时间启动
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // 启动 Worker 进程
            self.worker_manager.start().await?;

            // 启动崩溃检测器（Phase 4 新增）
            // 在后台监控 Worker 进程状态，崩溃时自动重启
            let mut crash_detector = WorkerCrashDetector::new(self.worker_manager.clone());
            crash_detector.start();

            // 启动 SIGHUP 信号监听（Phase 4 新增）
            // 收到 SIGHUP 时发送 ReloadTrigger 事件给热更新协调器
            let (trigger_tx, trigger_rx) = tokio::sync::mpsc::channel(16);
            let _sighup_handle = watch_sighup(trigger_tx);

            // 启动热更新协调器（Phase 4 新增）
            // 监听触发事件，收到时执行 Worker 热更新流程
            let coordinator = HotUpdateCoordinator::new(
                self.worker_manager.clone(),
                self.ipc_server.clone(),
                trigger_rx,
            );
            tokio::spawn(coordinator.run());

            tracing::info!("Manager 已启动（支持热更新）");

            // 等待终止信号
            tokio::signal::ctrl_c().await?;

            tracing::info!("收到终止信号，停止 Manager");

            // 停止崩溃检测器
            crash_detector.stop();

            // 停止 Worker
            self.worker_manager.stop().await?;

            // 停止 IPC 服务器
            self.ipc_server.stop().await?;
        }

        Ok(())
    }

    /// 处理终端窗口大小调整（由 QUIC 层调用）
    ///
    /// # 参数
    /// - `session_id`: PTY 会话 ID
    /// - `cols`: 列数
    /// - `rows`: 行数
    ///
    /// # 返回
    /// 成功返回 Ok(())，失败返回错误
    ///
    /// # 架构说明
    /// ResizeWindow 需要操作 master_fd（通过 ioctl），
    /// 而 master_fd 由 Manager 持有（在 PtyRegistry 中），
    /// 因此由 Manager 直接处理，不通过 Worker。
    #[cfg(unix)]
    pub async fn handle_resize_window(&self, session_id: &str, cols: u32, rows: u32) -> Result<()> {
        tracing::debug!("ResizeWindow: session_id={}, cols={}, rows={}", session_id, cols, rows);

        // 从 PtyRegistry 获取 master_fd
        let master_fd = self.pty_registry.get_fd(session_id).await
            .map_err(|e| {
                tracing::warn!("获取 master_fd 失败: session_id={}, error={}", session_id, e);
                e
            })?;

        // 调用 ioctl 调整终端大小
        self.set_window_size(master_fd, cols, rows)?;

        tracing::info!("终端窗口大小调整成功: session_id={}, cols={}, rows={}", session_id, cols, rows);
        Ok(())
    }

    /// 设置终端窗口大小（内部辅助函数）
    ///
    /// # 参数
    /// - `fd`: master_fd
    /// - `cols`: 列数
    /// - `rows`: 行数
    #[cfg(unix)]
    fn set_window_size(&self, fd: std::os::unix::io::RawFd, cols: u32, rows: u32) -> Result<()> {
        use nix::libc::{ioctl, winsize, TIOCSWINSZ};

        let ws = winsize {
            ws_col: cols as u16,
            ws_row: rows as u16,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let ret = unsafe { ioctl(fd, TIOCSWINSZ, &ws) };

        if ret < 0 {
            let err = nix::errno::Errno::last();
            tracing::error!("ioctl(TIOCSWINSZ) 失败: fd={}, error={}", fd, err);
            return Err(anyhow::anyhow!("Failed to set window size: {}", err));
        }

        Ok(())
    }
}