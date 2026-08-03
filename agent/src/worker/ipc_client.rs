//! IPC 客户端 - 用于 Worker 与 Manager 通信
//!
//! 该模块实现基于 Unix Domain Socket 的 IPC 客户端，支持：
//! - 连接到 Manager 的 IPC Server
//! - 发送/接收 Protobuf 消息
//! - 发送文件描述符（FD Passing）

use anyhow::{Result, Context};
use tokio::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use prost::Message;
use std::os::unix::io::AsRawFd;
use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
use nix::sys::uio::IoVec;

use crate::protocol::generated::{ManagerRequest, WorkerResponse};

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
        let stream = UnixStream::connect(socket_path).await
            .context(format!("Failed to connect to {}", socket_path))?;

        tracing::info!("已连接到 Manager IPC Server: {}", socket_path);

        Ok(Self { stream })
    }

    /// 发送文件描述符（SCM_RIGHTS）
    ///
    /// # 参数
    ///
    /// - `fd`: 要发送的文件描述符
    ///
    /// # 返回
    ///
    /// 成功返回 `Ok(())`，失败返回错误。
    ///
    /// # 说明
    ///
    /// 该方法使用 `sendmsg` 系统调用发送控制消息（SCM_RIGHTS），
    /// 将文件描述符从一个进程传递到另一个进程。
    ///
    /// **注意**: 这是一个同步方法，因为底层使用的是同步的 `nix::sys::socket::sendmsg`。
    ///
    /// # Warning
    ///
    /// 此方法使用同步的 `sendmsg` 系统调用。在异步上下文中调用可能阻塞 Tokio 运行时。
    /// 建议在独立的线程或 `spawn_blocking` 中调用。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let client = IpcClient::connect("/tmp/agent-worker.sock").await?;
    /// let master_fd = create_pty()?;
    /// client.send_fd(master_fd)?;  // 同步方法，不需要 await
    /// ```
    pub fn send_fd(&self, fd: std::os::unix::io::RawFd) -> Result<()> {
        // 准备控制消息（SCM_RIGHTS）
        let fds = [fd];
        let cmsg = ControlMessage::ScmRights(&fds);

        // 发送一个字节的 dummy 数据（必须发送至少一个字节）
        let dummy_data = [1u8];
        let iov = [IoVec::from_slice(&dummy_data)];

        // 使用 sendmsg 发送控制消息
        sendmsg(
            self.stream.as_raw_fd(),
            &iov,
            &[cmsg],
            MsgFlags::empty(),
            None,
        ).context("Failed to send FD via SCM_RIGHTS")?;

        tracing::debug!("已发送文件描述符: fd={}", fd);

        Ok(())
    }

    /// 发送 Protobuf 消息
    ///
    /// # 参数
    ///
    /// - `msg`: 要发送的 Protobuf 消息
    ///
    /// # 返回
    ///
    /// 成功返回 `Ok(())`，失败返回错误。
    ///
    /// # 消息格式
    ///
    /// ```
    /// [4 字节: 消息长度（big-endian）]
    /// [N 字节: Protobuf 编码的消息内容]
    /// ```
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let response = WorkerResponse { ... };
    /// client.send_message(&response).await?;
    /// ```
    pub async fn send_message<T: Message>(&self, msg: &T) -> Result<()> {
        // 编码 Protobuf 消息
        let mut buf = Vec::new();
        msg.encode(&mut buf)
            .context("Failed to encode Protobuf message")?;

        // 发送消息长度（4字节 big-endian）
        let len = buf.len() as u32;
        self.stream.write_all(&len.to_be_bytes()).await
            .context("Failed to write message length")?;

        // 发送消息内容
        self.stream.write_all(&buf).await
            .context("Failed to write message content")?;

        tracing::trace!("已发送消息: len={}", buf.len());

        Ok(())
    }

    /// 接收 Protobuf 消息
    ///
    /// # 返回
    ///
    /// 成功返回解码后的消息，失败返回错误。
    ///
    /// # 消息格式
    ///
    /// ```
    /// [4 字节: 消息长度（big-endian）]
    /// [N 字节: Protobuf 编码的消息内容]
    /// ```
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let request: ManagerRequest = client.receive_message().await?;
    /// ```
    pub async fn receive_message<T: Message + Default>(&self) -> Result<T> {
        // 读取消息长度（4字节 big-endian）
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await
            .context("Failed to read message length")?;
        let len = u32::from_be_bytes(len_buf) as usize;

        // 限制消息大小（防止恶意消息导致内存溢出）
        const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10 MB
        if len > MAX_MESSAGE_SIZE {
            anyhow::bail!("Message too large: {} bytes (max: {})", len, MAX_MESSAGE_SIZE);
        }

        // 读取消息内容
        let mut msg_buf = vec![0u8; len];
        self.stream.read_exact(&mut msg_buf).await
            .context("Failed to read message content")?;

        // 解码 Protobuf 消息
        let msg = T::decode(&msg_buf[..])
            .context("Failed to decode Protobuf message")?;

        tracing::trace!("已接收消息: len={}", len);

        Ok(msg)
    }

    /// 接收 Manager 的请求
    ///
    /// # 返回
    ///
    /// 成功返回 `ManagerRequest`，失败返回错误。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let request = client.receive_request().await?;
    /// match request.payload {
    ///     Some(manager_request::Payload::CreateSession(req)) => { ... }
    ///     _ => { ... }
    /// }
    /// ```
    pub async fn receive_request(&self) -> Result<ManagerRequest> {
        self.receive_message().await
    }

    /// 发送响应给 Manager
    ///
    /// # 参数
    ///
    /// - `response`: 要发送的响应
    ///
    /// # 返回
    ///
    /// 成功返回 `Ok(())`，失败返回错误。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let response = WorkerResponse {
    ///     request_id: 123,
    ///     payload: Some(worker_response::Payload::SessionCreated(...)),
    /// };
    /// client.send_response(&response).await?;
    /// ```
    pub async fn send_response(&self, response: &WorkerResponse) -> Result<()> {
        self.send_message(response).await
    }
}