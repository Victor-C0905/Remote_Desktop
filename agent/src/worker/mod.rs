//! Worker 模块 - 负责处理 PTY 和具体任务
//!
//! 该模块在 Worker 进程中运行，通过 Unix Socket 与 Manager 进程通信。
//!
//! # 架构位置
//!
//! ```
//! Manager (Gateway Layer)
//!     ↓ Unix Socket + FD Passing
//! Worker (Isolation Layer) ← 当前模块
//!     ↓ fork + PTY
//! Session (Terminal Process)
//! ```
//!
//! # 核心职责
//!
//! - Fork PTY 子进程
//! - 通过 Unix Socket 传递 master_fd 给 Manager
//! - 处理 Manager 的请求（文件操作、命令执行等）

pub mod handlers;
pub mod ipc_client;
pub mod pty_factory;
pub mod session_manager;

pub use ipc_client::IpcClient;
pub use pty_factory::PtyFactory;
pub use session_manager::{SessionManager, SessionInfo};

use anyhow::Result;
use crate::protocol::generated::{ManagerRequest, WorkerResponse, worker_response, Error};

/// Worker 主逻辑
///
/// # 参数
///
/// - `ipc_client`: IPC 客户端，用于与 Manager 通信
///
/// # 返回
///
/// 成功返回 `Ok(())`，失败返回错误。
///
/// # 示例
///
/// ```rust,ignore
/// let ipc_client = IpcClient::connect("/tmp/agent-worker.sock").await?;
/// worker::run(ipc_client).await?;
/// ```
pub async fn run(ipc_client: IpcClient) -> Result<()> {
    tracing::info!("Worker 消息处理循环启动");

    // 创建 PtyFactory
    let pty_factory = PtyFactory::new(std::sync::Arc::new(ipc_client.clone()));

    // 创建会话管理器
    let session_manager = SessionManager::new();

    // 启动子进程监控任务
    let session_manager_clone = session_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = session_manager_clone.monitor_child_processes().await {
            tracing::error!("子进程监控任务异常退出: {}", e);
        }
    });

    loop {
        // 接收 Manager 的请求
        match ipc_client.receive_request().await {
            Ok(request) => {
                tracing::debug!("收到 Manager 请求: request_id={}", request.request_id);

                // 处理请求
                let response = handle_request(request, &pty_factory, &session_manager).await;

                // 发送响应
                if let Err(e) = ipc_client.send_response(&response).await {
                    tracing::error!("发送响应失败: {}", e);
                    break;
                }
            }
            Err(e) => {
                tracing::error!("接收消息失败: {}", e);
                break;
            }
        }
    }

    tracing::info!("Worker 消息处理循环结束");

    Ok(())
}

/// 处理 Manager 的请求
///
/// # 参数
///
/// - `request`: Manager 发送的请求
/// - `pty_factory`: PTY 工厂实例
/// - `session_manager`: 会话管理器实例
///
/// # 返回
///
/// 返回 `WorkerResponse`（包含成功结果或错误）
///
/// # 示例
///
/// ```rust,ignore
/// let request = ManagerRequest { request_id: 1, payload: Some(...) };
/// let response = handle_request(request, &pty_factory, &session_manager).await;
/// ```
async fn handle_request(
    request: ManagerRequest,
    pty_factory: &PtyFactory,
    session_manager: &SessionManager,
) -> WorkerResponse {
    // 提取 request_id
    let request_id = request.request_id;

    // 处理请求
    let mut response = match request.payload {
        Some(crate::protocol::generated::manager_request::Payload::CreateSession(req)) => {
            handlers::session::handle_create_session(pty_factory, session_manager, req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::KillSession(req)) => {
            handlers::session::handle_kill_session(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::ReadDir(req)) => {
            handlers::file::handle_read_dir(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::ReadFile(req)) => {
            handlers::file::handle_read_file(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::WriteFile(req)) => {
            handlers::file::handle_write_file(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::ExecuteCommand(req)) => {
            handlers::command::handle_execute_command(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::GetSystemInfo(req)) => {
            handlers::system::handle_get_system_info(req).await
        }
        None => {
            tracing::warn!("收到空请求 payload");

            WorkerResponse {
                request_id,
                payload: Some(worker_response::Payload::Error(Error {
                    code: 400,
                    message: "Empty request payload".to_string(),
                })),
            }
        }
    };

    // 统一设置 request_id
    response.request_id = request_id;
    response
}