//! Manager 侧 Session 连接管理
//!
//! 负责与 Session 进程的 UnixSocket 通信。
//! 替代旧的 master_fd 直接读写模式。
//!
//! 连接建立后，UnixStream 被拆分为读/写两个半部：
//! - 读半部：pty_output 任务循环读取 PTY 输出
//! - 写半部：quic.rs 输入处理发送键盘输入/resize/close
//!
//! 读写半部使用独立 Mutex，允许并发读写（不会互相阻塞）。
//!
//! # 帧协议
//!
//! 二进制帧格式: `[1字节类型][4字节长度(big-endian)][数据]`
//! 与 Session 进程侧（session_process.rs）的同步实现保持一致。

use std::os::fd::AsRawFd;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use tracing::info;

use crate::worker::session_protocol::{msg_type, FRAME_HEADER_SIZE, MAX_FRAME_DATA_SIZE};
use nix::sys::socket::UnixAddr;
use nix::sys::socket::{connect, AddressFamily, SockFlag, SockType};

/// Session 连接
///
/// 封装与 Session 进程的 UnixSocket 连接。
/// 提供异步的 PTY 输入/输出/resize/close 操作。
///
/// 读写半部分离，允许 pty_output 任务持续读取输出的同时，
/// quic.rs 并发发送键盘输入，互不阻塞。
pub struct SessionConnection {
    /// abstract socket 名称（不含 \0 前缀）
    socket_name: String,
    /// 写半部：发送 PTY 输入/resize/close（Mutex 串行化多个发送者）
    write_half: Arc<Mutex<OwnedWriteHalf>>,
    /// 读半部：接收 PTY 输出/EOF（pty_output 任务持有读循环）
    read_half: Arc<Mutex<OwnedReadHalf>>,
}

impl SessionConnection {
    /// 连接到 Session 进程
    ///
    /// # 参数
    ///
    /// - `socket_name`: abstract socket 名称（不含 \0 前缀，`UnixAddr::new_abstract` 会自动添加）
    /// - `session_id`: 期望的 session_id，用于验证 Hello 握手
    ///
    /// # 流程
    ///
    /// 1. 连接 abstract UnixSocket
    /// 2. 接收 Hello 帧，验证 session_id 匹配
    /// 3. 拆分为读/写半部
    ///
    /// # 错误
    ///
    /// - 连接失败：Session 进程未启动或 socket 不存在
    /// - Hello 校验失败：session_id 不匹配（可能是连接到错误的 Session）
    pub async fn connect(socket_name: &str, session_id: &str) -> Result<Self> {
        // 连接 abstract socket（带重试，等待 Session 进程 listen() 就绪）
        let stream = connect_abstract(socket_name)
            .await
            .context("连接 Session socket 失败")?;

        // 拆分为读/写半部，允许并发读写
        let (read_half, write_half) = stream.into_split();

        let conn = Self {
            socket_name: socket_name.to_string(),
            write_half: Arc::new(Mutex::new(write_half)),
            read_half: Arc::new(Mutex::new(read_half)),
        };

        // 接收 Hello（验证 session_id）
        let (msg_type_val, data) = conn.recv_frame().await?;
        if msg_type_val != msg_type::HELLO {
            return Err(anyhow!("期望 Hello 消息, 收到: 0x{:02x}", msg_type_val));
        }
        let hello_session_id = String::from_utf8_lossy(&data);
        if hello_session_id != session_id {
            return Err(anyhow!(
                "session_id 不匹配: 期望 {}, 收到 {}",
                session_id,
                hello_session_id
            ));
        }

        info!("Session 连接建立: session_id={}", session_id);
        Ok(conn)
    }

    /// 发送 PTY 输入（客户端键盘输入）
    ///
    /// # 参数
    ///
    /// - `data`: 键盘输入字节流
    pub async fn send_input(&self, data: &[u8]) -> Result<()> {
        self.send_frame(msg_type::PTY_INPUT, data).await
    }

    /// 发送窗口大小调整
    ///
    /// # 参数
    ///
    /// - `cols`: 列数
    /// - `rows`: 行数
    ///
    /// # 帧格式
    ///
    /// data: 4字节 cols (big-endian) + 4字节 rows (big-endian)
    pub async fn send_resize(&self, cols: u16, rows: u16) -> Result<()> {
        let mut data = [0u8; 8];
        data[0..4].copy_from_slice(&(cols as u32).to_be_bytes());
        data[4..8].copy_from_slice(&(rows as u32).to_be_bytes());
        self.send_frame(msg_type::RESIZE, &data).await
    }

    /// 发送关闭请求
    ///
    /// 通知 Session 进程关闭会话（Session 进程会 kill bash 并退出）。
    pub async fn send_close(&self) -> Result<()> {
        self.send_frame(msg_type::CLOSE, &[]).await
    }

    /// 接收一帧数据
    ///
    /// 由 pty_output 任务调用，循环读取 PTY 输出。
    ///
    /// # 返回
    ///
    /// - `(msg_type, data)`: 消息类型和数据
    /// - 消息类型可能是 `PTY_OUTPUT`（终端输出）或 `EOF`（bash 退出）
    pub async fn recv_frame(&self) -> Result<(u8, Vec<u8>)> {
        let mut read_half = self.read_half.lock().await;

        // 读 header
        let mut header = [0u8; FRAME_HEADER_SIZE];
        read_half.read_exact(&mut header).await.context("读取帧头失败")?;

        let msg_type_val = header[0];
        let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;

        if len > MAX_FRAME_DATA_SIZE {
            return Err(anyhow!("帧数据过大: {} > {}", len, MAX_FRAME_DATA_SIZE));
        }

        // 读 data
        let mut data = vec![0u8; len];
        if len > 0 {
            read_half.read_exact(&mut data).await.context("读取帧数据失败")?;
        }

        Ok((msg_type_val, data))
    }

    /// 发送一帧数据
    async fn send_frame(&self, msg_type_val: u8, data: &[u8]) -> Result<()> {
        let mut write_half = self.write_half.lock().await;

        let len = data.len() as u32;
        let mut header = [0u8; FRAME_HEADER_SIZE];
        header[0] = msg_type_val;
        header[1..5].copy_from_slice(&len.to_be_bytes());

        write_half.write_all(&header).await.context("写入帧头失败")?;
        if !data.is_empty() {
            write_half.write_all(data).await.context("写入帧数据失败")?;
        }

        Ok(())
    }

    /// 获取 socket 名称
    pub fn socket_name(&self) -> &str {
        &self.socket_name
    }

    /// 获取写半部的共享句柄
    ///
    /// 用于在多个任务间共享发送能力（如 quic.rs 输入处理 + resize 处理）。
    pub fn write_handle(&self) -> Arc<Mutex<OwnedWriteHalf>> {
        Arc::clone(&self.write_half)
    }
}

impl std::fmt::Debug for SessionConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionConnection")
            .field("socket_name", &self.socket_name)
            .finish_non_exhaustive()
    }
}

/// 连接 abstract UnixSocket（带重试）
///
/// abstract socket 名称不以 `\0` 开头（`UnixAddr::new_abstract` 会自动添加 `\0` 前缀），
/// 这与 Session 进程侧的 bind 实现（session_process.rs::create_abstract_socket）保持一致。
///
/// # 重试机制
///
/// Session 进程在 fork 后需要时间完成 setuid、bind、fork（孙进程）、listen 等步骤。
/// Manager 可能在 Session 进程 listen() 之前就尝试 connect，导致 ECONNREFUSED。
/// 因此在收到 ECONNREFUSED 时重试，最多等待 500ms（50 次 × 10ms）。
///
/// 使用 nix 创建 socket 并 connect，然后转为 tokio 异步 UnixStream。
async fn connect_abstract(socket_name: &str) -> Result<UnixStream> {
    // 构造 abstract socket 地址（UnixAddr::new_abstract 自动添加 \0 前缀）
    let addr = UnixAddr::new_abstract(socket_name.as_bytes())
        .map_err(|e| anyhow!("创建 abstract 地址失败: {}", e))?;

    let max_retries = 50;
    let retry_interval = tokio::time::Duration::from_millis(10);

    for attempt in 0..max_retries {
        // 每次重试创建新 socket（旧 socket fd 在 connect 失败后已不可用）
        let sock = nix::sys::socket::socket(
            AddressFamily::Unix,
            SockType::Stream,
            SockFlag::empty(),
            None,
        )
        .context("创建 socket 失败")?;

        match connect(sock.as_raw_fd(), &addr) {
            Ok(()) => {
                // connect 成功，转为 tokio 异步 UnixStream
                let std_stream: std::os::unix::net::UnixStream = sock.into();
                std_stream
                    .set_nonblocking(true)
                    .context("设置非阻塞失败")?;
                let tokio_stream = UnixStream::from_std(std_stream)
                    .context("转换为 tokio UnixStream 失败")?;
                if attempt > 0 {
                    tracing::debug!(
                        "Session socket 连接成功（重试 {} 次后）",
                        attempt
                    );
                }
                return Ok(tokio_stream);
            }
            Err(nix::errno::Errno::ECONNREFUSED) => {
                // Session 进程尚未 listen()，等待后重试
                // sock 在此处 drop，自动关闭 fd
                tokio::time::sleep(retry_interval).await;
            }
            Err(e) => {
                return Err(anyhow!("connect 失败: {}", e));
            }
        }
    }

    Err(anyhow!(
        "连接 Session socket 失败: Session 进程在 {}ms 内未就绪",
        max_retries * 10
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msg_type_constants() {
        // 验证消息类型常量与 Session 进程侧一致
        assert_eq!(msg_type::PTY_INPUT, 0x01);
        assert_eq!(msg_type::PTY_OUTPUT, 0x02);
        assert_eq!(msg_type::RESIZE, 0x03);
        assert_eq!(msg_type::EOF, 0x04);
        assert_eq!(msg_type::CLOSE, 0x05);
        assert_eq!(msg_type::HELLO, 0x06);
    }

    #[test]
    fn test_frame_header_size() {
        assert_eq!(FRAME_HEADER_SIZE, 5);
    }
}
