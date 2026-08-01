//! IPC 服务器
//!
//! 接收 Worker 发送的 PTY master_fd。

use std::os::unix::io::RawFd;
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::RwLock;
use anyhow::Result;
use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags};
use nix::sys::uio::IoVec;

/// IPC 连接信息
#[derive(Debug, Clone)]
pub struct IpcConnection {
    /// 连接 ID
    pub connection_id: String,

    /// Worker 进程 ID
    pub worker_pid: Option<u32>,
}

/// IPC 服务器
pub struct IpcServer {
    /// Unix Socket 监听器
    listener: Arc<RwLock<Option<UnixListener>>>,

    /// Socket 路径
    socket_path: String,

    /// 活动的连接
    connections: Arc<RwLock<Vec<IpcConnection>>>,
}

impl IpcServer {
    /// 创建新的 IPC 服务器
    pub fn new(socket_path: String) -> Self {
        Self {
            listener: Arc::new(RwLock::new(None)),
            socket_path,
            connections: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 启动 IPC 服务器
    pub async fn start(&self) -> Result<()> {
        // 删除旧的 Socket 文件
        if std::path::Path::new(&self.socket_path).exists() {
            std::fs::remove_file(&self.socket_path)?;
        }

        // 绑定 Unix Socket
        let listener = UnixListener::bind(&self.socket_path)?;

        let mut listener_guard = self.listener.write().await;
        *listener_guard = Some(listener);

        Ok(())
    }

    /// 接收 Worker 连接
    pub async fn accept(&self) -> Result<IpcConnection> {
        let listener_guard = self.listener.read().await;

        if let Some(ref listener) = *listener_guard {
            let (stream, _addr) = listener.accept().await?;

            let connection = IpcConnection {
                connection_id: uuid::Uuid::new_v4().to_string(),
                worker_pid: None,
            };

            // 添加到活动连接列表
            let mut connections = self.connections.write().await;
            connections.push(connection.clone());

            Ok(connection)
        } else {
            Err(anyhow::anyhow!("IPC server not started"))
        }
    }

    /// 接收文件描述符（FD Passing）
    pub fn receive_fd(stream: &tokio::net::UnixStream) -> Result<RawFd> {
        use std::os::unix::io::AsRawFd;

        let raw_fd = stream.as_raw_fd();
        let mut buf = [0u8; 1];
        let mut iov = [IoVec::from_mut_slice(&mut buf)];
        let mut cmsg_buf = [0u8; 64];

        let msg = recvmsg(
            raw_fd,
            &mut iov,
            Some(&mut cmsg_buf),
            MsgFlags::empty(),
        )?;

        for cmsg in msg.cmsgs()? {
            if let ControlMessageOwned::ScmRights(fds) = cmsg {
                if !fds.is_empty() {
                    return Ok(fds[0]);
                }
            }
        }

        Err(anyhow::anyhow!("No FD received"))
    }

    /// 停止 IPC 服务器
    pub async fn stop(&self) -> Result<()> {
        let mut listener_guard = self.listener.write().await;
        *listener_guard = None;

        // 删除 Socket 文件
        if std::path::Path::new(&self.socket_path).exists() {
            std::fs::remove_file(&self.socket_path)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ipc_server_creation() {
        let server = IpcServer::new("/tmp/test.sock".to_string());
        assert!(server.listener.read().await.is_none());
    }
}