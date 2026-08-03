//! 会话处理器 - 处理 PTY 会话相关请求
//!
//! 该模块负责：
//! - 创建 PTY 会话（CreateSession）
//! - 终止会话（KillSession）
//!
//! 注意：ResizeWindow 已从 Worker 移除，由 Manager 直接处理。

use anyhow::Result;
use std::sync::Arc;

use crate::protocol::generated::{
    CreateSession, SessionCreated, KillSession,
    WorkerResponse, worker_response, Error,
};
use super::super::{PtyFactory, SessionManager};
use nix::unistd::Pid;

/// 处理 CreateSession 请求
///
/// # 参数
///
/// - `pty_factory`: PTY 工厂实例
/// - `session_manager`: 会话管理器实例
/// - `req`: CreateSession 请求参数
///
/// # 返回
///
/// 返回 `WorkerResponse`，包含 `SessionCreated` 或 `Error`。
///
/// # 流程
///
/// 1. 调用 `PtyFactory::create()` 创建 PTY
/// 2. master_fd 会自动通过 IPC 发送给 Manager
/// 3. 向 SessionManager 注册会话
/// 4. 返回 session_id 和 pid 给 Manager
///
/// # 示例
///
/// ```rust,ignore
/// let factory = PtyFactory::new(ipc_client);
/// let response = handle_create_session(&factory, &session_manager, req).await;
/// ```
#[tracing::instrument(skip(pty_factory, session_manager), fields(shell = %req.shell, cols = req.cols, rows = req.rows))]
pub async fn handle_create_session(
    pty_factory: &PtyFactory,
    session_manager: &SessionManager,
    req: CreateSession,
) -> WorkerResponse {
    tracing::info!(
        "处理 CreateSession 请求: shell={}, cols={}, rows={}, cwd={}",
        req.shell, req.cols, req.rows, req.working_directory
    );

    // 准备工作目录
    let cwd = if req.working_directory.is_empty() {
        None
    } else {
        Some(req.working_directory.as_str())
    };

    // 调用 PtyFactory 创建 PTY
    // 注意：master_fd 会自动通过 IPC 发送给 Manager
    match pty_factory.create(&req.shell, req.cols, req.rows, cwd) {
        Ok((session_id, pid)) => {
            tracing::info!(
                "PTY 会话创建成功: session_id={}, pid={}",
                session_id, pid
            );

            // 向 SessionManager 注册会话
            session_manager.register(
                session_id.clone(),
                Pid::from_raw(pid),
                req.shell.clone()
            ).await;

            WorkerResponse {
                payload: Some(worker_response::Payload::SessionCreated(SessionCreated {
                    session_id,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("创建 PTY 会话失败: {}", e);

            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 500,
                    message: format!("Failed to create PTY session: {}", e),
                })),
                ..Default::default()
            }
        }
    }
}

/// 处理 KillSession 请求
///
/// # 参数
///
/// - `req`: KillSession 请求参数
///
/// # 返回
///
/// 返回 `WorkerResponse`，包含成功或错误。
///
/// # 注意
///
/// 与 ResizeWindow 类似，Worker 不持有 master_fd，
/// 无法直接向 PTY 发送信号。应由 Manager 处理。
///
/// # 示例
///
/// ```rust,ignore
/// let response = handle_kill_session(req).await;
/// ```
#[tracing::instrument(fields(session_id = %req.session_id))]
pub async fn handle_kill_session(req: KillSession) -> WorkerResponse {
    tracing::warn!(
        "KillSession 请求无效：Worker 不持有 master_fd，应由 Manager 处理: session_id={}",
        req.session_id
    );

    // 返回错误响应，提示该操作应在 Manager 端执行
    WorkerResponse {
        payload: Some(worker_response::Payload::Error(Error {
            code: 400,
            message: "KillSession should be handled by Manager (Worker doesn't hold master_fd)".to_string(),
        })),
        ..Default::default()
    }
}