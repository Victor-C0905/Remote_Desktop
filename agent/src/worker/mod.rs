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

    // TODO: TASK-017 实现消息处理
    // loop {
    //     let request = ipc_client.receive_request().await?;
    //     let response = handle_request(request).await?;
    //     ipc_client.send_response(response).await?;
    // }

    tracing::info!("Worker 消息处理循环结束");

    Ok(())
}