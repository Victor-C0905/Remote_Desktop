//! IPC 客户端 - 用于 Worker 与 Manager 通信
//!
//! 该模块实现基于 Unix Domain Socket 的 IPC 客户端，支持：
//! - 连接到 Manager 的 IPC Server
//! - 发送/接收 Protobuf 消息
//! - 接收文件描述符（FD Passing）

use anyhow::Result;
use tokio::net::UnixStream;

/// IPC 客户端
///
/// 用于 Worker 进程连接到 Manager 进程的 IPC Server。
pub struct IpcClient {
    /// Unix Socket 连接
    stream: UnixStream,
}

impl IpcClient {
    /// 连接到 Manager 的 IPC Server
    ///
    /// # 参数
    ///
    /// - `socket_path`: Unix Socket 路径（如 `/tmp/agent-worker.sock`）
    ///
    /// # 返回
    ///
    /// 成功返回 `IpcClient` 实例，失败返回错误。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let client = IpcClient::connect("/tmp/agent-worker.sock").await?;
    /// ```
    pub async fn connect(socket_path: &str) -> Result<Self> {
        let stream = UnixStream::connect(socket_path).await?;
        tracing::info!("已连接到 Manager IPC Server: {}", socket_path);
        Ok(Self { stream })
    }

    // TODO: TASK-017 实现以下方法
    // pub async fn receive_request(&self) -> Result<ManagerRequest> { ... }
    // pub async fn send_response(&self, response: WorkerResponse) -> Result<()> { ... }
    // pub async fn send_fd(&self, fd: RawFd) -> Result<()> { ... }
}