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

// 阶段 3 新增:协议适配层(跨平台,纯转换逻辑)
pub mod protocol_adapter;

pub use connection::ConnectionManager;
pub use session::SessionManager;

#[cfg(unix)]
pub use pty_registry::{PtyRegistry, PtySession, UserInfo};

#[cfg(unix)]
pub use pty_output::{PtyOutputConfig, spawn_pty_output_task_v2};

#[cfg(unix)]
pub use worker_manager::{WorkerManager, WorkerInfo, WorkerStatus, WorkerStatusEvent};

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

// 阶段 3 新增:协议适配层导出
pub use protocol_adapter::{UserContext, serde_to_worker_request, worker_response_to_serde};

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

    /// IPC Server（接收 Worker 的 FD）
    #[cfg(unix)]
    ipc_server: Arc<IpcServer>,

    /// 孤儿进程回收器
    /// Phase 4 新增：在 PTY EOF 时回收 Session 僵尸进程
    #[cfg(unix)]
    orphan_reaper: Arc<OrphanProcessReaper>,

    /// 崩溃检测器
    /// 阶段 1:启动后持续监控 Worker 进程状态
    /// 使用 Mutex 包装以支持 `&self` 的 start/shutdown(内部可变性)
    #[cfg(unix)]
    crash_detector: tokio::sync::Mutex<Option<WorkerCrashDetector>>,
}

impl Manager {
    /// 创建新的 Manager
    ///
    /// # 流程
    /// 1. 创建 PtyRegistry
    /// 2. 创建 WorkerManager
    /// 3. 创建 IpcServer（集成 WorkerManager）
    /// 4. 创建 OrphanProcessReaper
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

        // 创建孤儿进程回收器
        // Phase 4 新增：用于在 PTY EOF 时回收 Session 僵尸进程
        let orphan_reaper = Arc::new(OrphanProcessReaper::new(pty_registry.clone()));

        Ok(Self {
            pty_registry,
            worker_manager,
            ipc_server,
            orphan_reaper,
            crash_detector: tokio::sync::Mutex::new(None),
        })
    }

    /// 创建新的 Manager（非 Unix 平台）
    #[cfg(not(unix))]
    pub async fn new(config: &AgentConfig) -> Result<Self> {
        Ok(Self {})
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

    /// 启动 Manager(非阻塞)
    ///
    /// 启动 IPC 服务器、Worker 子进程、崩溃检测器,然后立即返回。
    /// 不等待 ctrl_c 信号,由调用方决定何时调用 `shutdown`。
    ///
    /// 注意:签名改为 `&self`(阶段 2),通过内部可变性(Mutex)管理 crash_detector,
    /// 以便 Manager 可以被 `Arc` 共享给 quic::run 等多个并发任务。
    #[cfg(unix)]
    pub async fn start(&self) -> Result<()> {
        // 启动 IPC 服务器(在后台运行)
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

        // 启动崩溃检测器
        let mut crash_detector = WorkerCrashDetector::new(self.worker_manager.clone());
        crash_detector.start();
        *self.crash_detector.lock().await = Some(crash_detector);

        tracing::info!("Manager 已启动(IPC + Worker + CrashDetector)");

        Ok(())
    }

    /// 停止 Manager(非阻塞)
    ///
    /// 注意:签名改为 `&self`(阶段 2),通过内部可变性(Mutex)取出 crash_detector。
    #[cfg(unix)]
    pub async fn shutdown(&self) -> Result<()> {
        tracing::info!("正在停止 Manager");

        // 停止崩溃检测器
        if let Some(mut detector) = self.crash_detector.lock().await.take() {
            detector.stop();
        }

        // 停止 Worker
        self.worker_manager.stop().await?;

        // 停止 IPC 服务器
        self.ipc_server.stop().await?;

        tracing::info!("Manager 已停止");
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

    /// 获取 Worker 进程信息(供外部测试和监控使用)
    #[cfg(unix)]
    pub async fn worker_info(&self) -> Option<WorkerInfo> {
        self.worker_manager.get_info().await
    }

    /// 通过 Worker 创建 PTY 会话(阶段 2 新增)
    ///
    /// 将 PTY 创建请求发送到 Worker 子进程,Worker forkpty 并通过 SCM_RIGHTS 传递 master_fd。
    /// 创建成功后,master_fd 自动注册到 PtyRegistry。
    ///
    /// # 参数
    /// - `shell`: Shell 路径(如 /bin/bash),空字符串则使用用户默认 shell
    /// - `cols`: 终端列数
    /// - `rows`: 终端行数
    /// - `cwd`: 工作目录(可选)
    /// - `user_session`: 用户会话(含 uid/gid/username 等)
    ///
    /// # 返回
    /// 成功返回 session_id,失败返回错误
    #[cfg(unix)]
    pub async fn create_pty_session(
        &self,
        shell: &str,
        cols: u32,
        rows: u32,
        cwd: Option<&str>,
        user_session: &crate::auth::session::UserSession,
    ) -> Result<String> {
        // 如果客户端未指定 shell,使用用户会话中的默认 shell(从 /etc/passwd 读取)
        let shell = if shell.is_empty() {
            user_session.shell.to_string_lossy().to_string()
        } else {
            shell.to_string()
        };

        tracing::info!(
            "创建 PTY 会话: shell={}, uid={}, username={}",
            shell, user_session.uid, user_session.username
        );

        let request = crate::protocol::generated::CreateSession {
            cols,
            rows,
            shell: shell.clone(),
            working_directory: cwd.unwrap_or("").to_string(),
            uid: user_session.uid,
            gid: user_session.gid,
            username: user_session.username.clone(),
            home_dir: user_session.home_dir.to_string_lossy().to_string(),
        };

        let user_info = UserInfo::new(
            user_session.username.clone(),
            user_session.uid,
            user_session.gid,
        );

        self.ipc_server.create_pty_session(request, user_info).await
    }

    /// 路由业务请求到 Worker(阶段 3 新增)
    ///
    /// 将客户端 serde Payload 通过协议适配层转换为 Worker protobuf 请求,
    /// 发送到 Worker 处理,再将 Worker 响应转换回客户端 serde Payload。
    ///
    /// # 参数
    /// - `payload`: 客户端发送的 serde Payload(如 ReadDirRequest/ReadFileRequest/WriteFileRequest)
    /// - `user_session`: 用户会话(含 uid/gid/username 等,用于 Worker 用户隔离)
    ///
    /// # 返回
    /// - `Ok(Some(Payload))`: Worker 已处理,返回转换后的客户端响应
    /// - `Ok(None)`: 该 payload 不需要路由到 Worker(如 Ping/Subscribe),由调用方直接处理
    /// - `Err(_)`: 路由或 Worker 处理失败
    #[cfg(unix)]
    pub async fn route_to_worker(
        &self,
        payload: &crate::protocol::Payload,
        user_session: &crate::auth::session::UserSession,
    ) -> Result<Option<crate::protocol::Payload>> {
        // 1. 协议适配:serde Payload → Worker manager_request::Payload
        let user_context = protocol_adapter::UserContext::from(user_session);
        let worker_payload = protocol_adapter::serde_to_worker_request(payload, &user_context);

        // 不需要路由到 Worker 的请求,返回 None
        let worker_payload = match worker_payload {
            Some(p) => p,
            None => return Ok(None),
        };

        // 2. 通过 IPC 发送到 Worker,接收响应
        let worker_response = self.ipc_server.send_request(worker_payload).await
            .map_err(|e| {
                // 使用 {:?} 打印完整错误链,定位 receive_response 失败的根本原因
                // (EOF/解码失败/连接断开等)
                tracing::error!("Worker 请求失败: {:?}", e);
                e
            })?;

        // 3. 协议适配:Worker WorkerResponse → serde Payload
        let serde_payload = protocol_adapter::worker_response_to_serde(&worker_response)
            .ok_or_else(|| anyhow::anyhow!("协议适配层无法转换 Worker 响应"))?;

        Ok(Some(serde_payload))
    }

    /// 获取 PtyRegistry 引用(供 quic.rs 读写 PTY)
    #[cfg(unix)]
    pub fn pty_registry(&self) -> &Arc<PtyRegistry> {
        &self.pty_registry
    }

    /// 获取 OrphanProcessReaper 引用(供 PTY 输出任务在 EOF 时回收僵尸进程)
    #[cfg(unix)]
    pub fn orphan_reaper(&self) -> &Arc<OrphanProcessReaper> {
        &self.orphan_reaper
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