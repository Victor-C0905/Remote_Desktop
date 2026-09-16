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
pub mod session_process;
pub mod session_protocol;

pub use ipc_client::IpcClient;
pub use pty_factory::PtyFactory;
pub use session_manager::{SessionManager, SessionInfo};

use std::sync::Arc;
use anyhow::Result;
use tokio::sync::Notify;
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
pub async fn run(mut ipc_client: IpcClient) -> Result<()> {
    tracing::info!("Worker 消息处理循环启动");

    // 创建 PtyFactory（不持有 IpcClient，在 create 时借用）
    let pty_factory = PtyFactory::new();

    // 创建会话管理器
    let session_manager = SessionManager::new();

    // 启动子进程监控任务
    let session_manager_clone = session_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = session_manager_clone.monitor_child_processes().await {
            tracing::error!("子进程监控任务异常退出: {}", e);
        }
    });

    // 创建 shutdown 信号通道（GracefulShutdown 处理器通过此通道通知主循环退出）
    // 使用 Notify 而非 oneshot：oneshot::Sender 只能 send 一次，
    // 而 Notify 可在 select! 循环中反复创建 notified() future
    let shutdown_notify = Arc::new(Notify::new());

    loop {
        // 使用 select! 同时监听 shutdown 信号和 IPC 请求
        // 注意：receive_request 和 send_response 需要 &mut ipc_client
        tokio::select! {
            // 接收到 shutdown 信号，退出主循环
            _ = shutdown_notify.notified() => {
                tracing::info!("收到 shutdown 信号，Worker 主循环退出");
                break;
            }
            // 接收 Manager 的请求
            request_result = ipc_client.receive_request() => {
                match request_result {
                    Ok(request) => {
                        tracing::debug!("收到 Manager 请求: request_id={}", request.request_id);

                        // 处理请求
                        // 如果是 GracefulShutdown，handle_request 会调用
                        // shutdown_notify.notify_one()，下一轮 select! 将退出
                        let response = handle_request(
                            request,
                            &pty_factory,
                            &session_manager,
                            &shutdown_notify,
                        ).await;

                        // 发送响应
                        tracing::info!(
                            "准备发送响应到 Manager: request_id={}, payload_type={}",
                            response.request_id,
                            response.payload.as_ref().map(|p| match p {
                                worker_response::Payload::DirListing(_) => "DirListing",
                                worker_response::Payload::FileContent(_) => "FileContent",
                                worker_response::Payload::WriteResult(_) => "WriteResult",
                                worker_response::Payload::Error(_) => "Error",
                                worker_response::Payload::SessionCreated(_) => "SessionCreated",
                                _ => "Other",
                            }).unwrap_or("None")
                        );
                        if let Err(e) = ipc_client.send_response(&response).await {
                            tracing::error!("发送响应失败: request_id={}, error={:?}", response.request_id, e);
                            break;
                        }
                        tracing::info!("响应发送成功: request_id={}", response.request_id);
                    }
                    Err(e) => {
                        tracing::error!("接收消息失败: {}", e);
                        break;
                    }
                }
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
/// - `shutdown_notify`: 通知主循环退出的 Notify 实例（仅 GracefulShutdown 使用）
///
/// # 返回
///
/// 返回 `WorkerResponse`（包含成功结果或错误）
///
/// # 示例
///
/// ```rust,ignore
/// let request = ManagerRequest { request_id: 1, payload: Some(...) };
/// let response = handle_request(request, &pty_factory, &session_manager, &shutdown_notify).await;
/// ```
async fn handle_request(
    request: ManagerRequest,
    pty_factory: &PtyFactory,
    session_manager: &SessionManager,
    shutdown_notify: &Arc<Notify>,
) -> WorkerResponse {
    // 提取 request_id
    let request_id = request.request_id;

    // 处理请求
    let mut response = match request.payload {
        Some(crate::protocol::generated::manager_request::Payload::CreateSession(req)) => {
            handlers::session::handle_create_session(pty_factory, session_manager, req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::KillSession(req)) => {
            handlers::session::handle_kill_session(req, session_manager).await
        }
        Some(crate::protocol::generated::manager_request::Payload::ReadDir(req)) => {
            handlers::file::handle_read_dir(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::ReadFile(req)) => {
            handlers::file::handle_read_file(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::FileInfo(req)) => {
            handlers::file::handle_file_info(req).await
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
        Some(crate::protocol::generated::manager_request::Payload::GracefulShutdown(req)) => {
            handlers::shutdown::handle_graceful_shutdown(req, session_manager, shutdown_notify).await
        }
        Some(crate::protocol::generated::manager_request::Payload::Delete(req)) => {
            handlers::file::handle_delete(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::Mkdir(req)) => {
            handlers::file::handle_mkdir(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::Rename(req)) => {
            handlers::file::handle_rename(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::Copy(req)) => {
            handlers::file::handle_copy(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::Move(req)) => {
            handlers::file::handle_move(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::Chmod(req)) => {
            handlers::file::handle_chmod(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::Chown(req)) => {
            handlers::file::handle_chown(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::FileExists(req)) => {
            handlers::file::handle_file_exists(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::ApplyDiff(req)) => {
            handlers::file::handle_apply_diff(req).await
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