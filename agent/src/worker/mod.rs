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

pub use ipc_client::IpcClient;
pub use pty_factory::PtyFactory;

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

    loop {
        // 接收 Manager 的请求
        match ipc_client.receive_request().await {
            Ok(request) => {
                tracing::debug!("收到 Manager 请求: request_id={}", request.request_id);

                // 处理请求
                let response = handle_request(request).await;

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
///
/// # 返回
///
/// 返回 `WorkerResponse`（包含成功结果或错误）
///
/// # 示例
///
/// ```rust,ignore
/// let request = ManagerRequest { request_id: 1, payload: Some(...) };
/// let response = handle_request(request).await;
/// ```
async fn handle_request(request: ManagerRequest) -> WorkerResponse {
    // TODO: TASK-019 实现具体的请求处理逻辑
    // 当前只返回一个默认的空响应

    match request.payload {
        Some(crate::protocol::generated::manager_request::Payload::CreateSession(_)) => {
            // TODO: 调用 PtyFactory 创建 PTY 会话
            tracing::warn!("CreateSession 请求暂未实现");

            WorkerResponse {
                request_id: request.request_id,
                payload: Some(worker_response::Payload::Error(Error {
                    code: 501,
                    message: "Not Implemented".to_string(),
                })),
            }
        }
        Some(crate::protocol::generated::manager_request::Payload::ResizeWindow(_)) => {
            tracing::warn!("ResizeWindow 请求暂未实现");

            WorkerResponse {
                request_id: request.request_id,
                payload: Some(worker_response::Payload::Error(Error {
                    code: 501,
                    message: "Not Implemented".to_string(),
                })),
            }
        }
        Some(crate::protocol::generated::manager_request::Payload::KillSession(_)) => {
            tracing::warn!("KillSession 请求暂未实现");

            WorkerResponse {
                request_id: request.request_id,
                payload: Some(worker_response::Payload::Error(Error {
                    code: 501,
                    message: "Not Implemented".to_string(),
                })),
            }
        }
        Some(crate::protocol::generated::manager_request::Payload::ReadDir(_)) => {
            tracing::warn!("ReadDir 请求暂未实现");

            WorkerResponse {
                request_id: request.request_id,
                payload: Some(worker_response::Payload::Error(Error {
                    code: 501,
                    message: "Not Implemented".to_string(),
                })),
            }
        }
        Some(crate::protocol::generated::manager_request::Payload::ReadFile(_)) => {
            tracing::warn!("ReadFile 请求暂未实现");

            WorkerResponse {
                request_id: request.request_id,
                payload: Some(worker_response::Payload::Error(Error {
                    code: 501,
                    message: "Not Implemented".to_string(),
                })),
            }
        }
        Some(crate::protocol::generated::manager_request::Payload::WriteFile(_)) => {
            tracing::warn!("WriteFile 请求暂未实现");

            WorkerResponse {
                request_id: request.request_id,
                payload: Some(worker_response::Payload::Error(Error {
                    code: 501,
                    message: "Not Implemented".to_string(),
                })),
            }
        }
        Some(crate::protocol::generated::manager_request::Payload::ExecuteCommand(_)) => {
            tracing::warn!("ExecuteCommand 请求暂未实现");

            WorkerResponse {
                request_id: request.request_id,
                payload: Some(worker_response::Payload::Error(Error {
                    code: 501,
                    message: "Not Implemented".to_string(),
                })),
            }
        }
        Some(crate::protocol::generated::manager_request::Payload::GetSystemInfo(_)) => {
            tracing::warn!("GetSystemInfo 请求暂未实现");

            WorkerResponse {
                request_id: request.request_id,
                payload: Some(worker_response::Payload::Error(Error {
                    code: 501,
                    message: "Not Implemented".to_string(),
                })),
            }
        }
        None => {
            tracing::warn!("收到空请求 payload");

            WorkerResponse {
                request_id: request.request_id,
                payload: Some(worker_response::Payload::Error(Error {
                    code: 400,
                    message: "Empty request payload".to_string(),
                })),
            }
        }
    }
}