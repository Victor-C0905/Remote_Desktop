//! 会话处理器 - 处理 PTY 会话相关请求
//!
//! 该模块负责：
//! - 创建 PTY 会话（CreateSession）→ fork Session 进程
//! - 终止会话（KillSession）→ SIGTERM 给 Session 进程
//!
//! 注意：ResizeWindow 已从 Worker 移除，由 Manager 直接处理。

use crate::protocol::generated::{
    CreateSession, SessionCreated, KillSession,
    WorkerResponse, worker_response, Error,
};
use super::super::{PtyFactory, SessionManager};
use super::super::pty_factory::UserContext;
use nix::unistd::Pid;
use nix::sys::signal::{kill, Signal};

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
/// 返回 `WorkerResponse`，包含 `SessionCreated`（含 session_id 和 socket_name）或 `Error`。
///
/// # 流程
///
/// 1. 调用 `PtyFactory::create()` 创建 Session 进程（openpty+fork）
/// 2. Session 进程持有 master_fd，通过 abstract UnixSocket 与 Manager 通信
/// 3. 向 SessionManager 注册会话
/// 4. 返回 session_id 和 socket_name 给 Manager
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

    // 构造用户上下文
    // 注意:root 用户(uid=0)也需要设置环境变量(HOME/USER)和工作目录
    // setuid(0)/setgid(0) 对 root 是 no-op,不会失败
    let user_context = if !req.username.is_empty() {
        Some(UserContext {
            uid: req.uid,
            gid: req.gid,
            username: req.username.clone(),
            home_dir: req.home_dir.clone(),
        })
    } else {
        None
    };

    // 调用 PtyFactory 创建 Session 进程
    match pty_factory.create(&req.shell, req.cols, req.rows, cwd, user_context.as_ref()) {
        Ok(result) => {
            tracing::info!(
                "Session 进程创建成功: session_id={}, socket_name={}, pid={}",
                result.session_id, result.socket_name, result.session_pid
            );

            // 向 SessionManager 注册会话
            session_manager.register(
                result.session_id.clone(),
                Pid::from_raw(result.session_pid),
                result.socket_name.clone(),
                req.shell.clone(),
            ).await;

            WorkerResponse {
                payload: Some(worker_response::Payload::SessionCreated(SessionCreated {
                    session_id: result.session_id,
                    socket_name: result.socket_name,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("创建 Session 进程失败: {}", e);

            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 500,
                    message: format!("Failed to create session process: {}", e),
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
/// - `session_manager`: 会话管理器实例
///
/// # 返回
///
/// 返回 `WorkerResponse`，包含成功或错误。
///
/// # 流程
///
/// 1. 从 SessionManager 查找会话
/// 2. 发送 SIGTERM 给 Session 进程
/// 3. 从 SessionManager 注销会话
#[tracing::instrument(skip(session_manager), fields(session_id = %req.session_id))]
pub async fn handle_kill_session(
    req: KillSession,
    session_manager: &SessionManager,
) -> WorkerResponse {
    tracing::info!("处理 KillSession 请求: session_id={}", req.session_id);

    // 查找会话
    if let Some(info) = session_manager.get(&req.session_id).await {
        // 发送 SIGTERM 给 Session 进程
        let pid = Pid::from_raw(info.pid.as_raw());
        match kill(pid, Signal::SIGTERM) {
            Ok(()) => {
                tracing::info!(
                    "已发送 SIGTERM 到 Session 进程: pid={}, session_id={}",
                    info.pid, req.session_id
                );
            }
            Err(e) => {
                tracing::warn!(
                    "SIGTERM 发送失败（进程可能已退出）: pid={}, session_id={}, error={}",
                    info.pid, req.session_id, e
                );
            }
        }

        // 从 SessionManager 注销
        session_manager.unregister(&req.session_id).await;

        WorkerResponse {
            payload: Some(worker_response::Payload::Error(Error {
                code: 0,
                message: String::new(),
            })),
            ..Default::default()
        }
    } else {
        WorkerResponse {
            payload: Some(worker_response::Payload::Error(Error {
                code: 404,
                message: format!("Session not found: {}", req.session_id),
            })),
            ..Default::default()
        }
    }
}
