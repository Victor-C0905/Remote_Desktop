//! IPC 服务器
//!
//! 与 WorkerManager 和 PtyRegistry 集成：
//! - 接收 Worker 进程的连接
//! - 发送请求到 Worker（CreateSession/ReadDir 等）并接收响应
//! - CreateSession 响应包含 socket_name，Manager 通过 SessionConnection 连接到 Session 进程
//!
//! 新架构：不再使用 SCM_RIGHTS 传递 master_fd。
//! master_fd 由 Session 进程持有，Manager 通过 UnixSocket 帧协议与 Session 通信。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use anyhow::{Result, Context};
use prost::Message;
use tracing::{info, warn, error};

use super::pty_registry::{PtyRegistry, PtySession, UserInfo};
use super::session_connection::SessionConnection;
use super::worker_manager::{WorkerManager, WorkerStatus};
use crate::protocol::generated::{
    ManagerRequest, WorkerResponse,
    manager_request, worker_response,
};

/// IPC 连接信息
#[derive(Debug)]
pub struct IpcConnection {
    /// 连接 ID
    pub connection_id: String,

    /// Worker 进程 ID
    pub worker_pid: Option<u32>,

    /// Unix Stream 连接（用于 protobuf 通信）
    stream: Option<tokio::net::UnixStream>,
}

impl IpcConnection {
    /// 创建新连接
    fn new(connection_id: String, stream: tokio::net::UnixStream) -> Self {
        Self {
            connection_id,
            worker_pid: None,
            stream: Some(stream),
        }
    }

    /// 发送 ManagerRequest 到 Worker(异步)
    ///
    /// 消息格式:[4字节长度(big-endian)] + [protobuf 内容]
    pub async fn send_request(&mut self, request: &ManagerRequest) -> Result<()> {
        if let Some(ref mut stream) = self.stream {
            let mut buf = Vec::new();
            request.encode(&mut buf)
                .context("Failed to encode ManagerRequest")?;
            let len = buf.len() as u32;
            stream.write_all(&len.to_be_bytes()).await
                .context("Failed to write request length")?;
            stream.write_all(&buf).await
                .context("Failed to write request content")?;
            tracing::debug!("已发送请求到 Worker: request_id={}, len={}", request.request_id, buf.len());
            Ok(())
        } else {
            Err(anyhow::anyhow!("No stream available"))
        }
    }

    /// 接收 WorkerResponse(异步,不含 FD)
    ///
    /// 消息格式:[4字节长度(big-endian)] + [protobuf 内容]
    pub async fn receive_response(&mut self) -> Result<WorkerResponse> {
        if let Some(ref mut stream) = self.stream {
            let mut len_buf = [0u8; 4];
            stream.read_exact(&mut len_buf).await
                .context("Failed to read response length")?;
            let len = u32::from_be_bytes(len_buf) as usize;

            const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;
            if len > MAX_MESSAGE_SIZE {
                anyhow::bail!("Response too large: {} bytes", len);
            }

            let mut msg_buf = vec![0u8; len];
            stream.read_exact(&mut msg_buf).await
                .context("Failed to read response content")?;

            let msg = WorkerResponse::decode(&msg_buf[..])
                .context("Failed to decode WorkerResponse")?;
            tracing::debug!("已接收 Worker 响应: len={}", len);
            Ok(msg)
        } else {
            Err(anyhow::anyhow!("No stream available"))
        }
    }

    /// 关闭连接
    pub fn close(&mut self) {
        self.stream = None;
    }
}

/// IPC 服务器
///
/// 与 WorkerManager 和 PtyRegistry 集成：
/// - 接收 Worker 进程的连接
/// - 接收 Worker 发送的 PTY master_fd
/// - 将 FD 注册到 PtyRegistry
pub struct IpcServer {
    /// Unix Socket 监听器
    listener: Arc<RwLock<Option<UnixListener>>>,

    /// Socket 路径
    socket_path: String,

    /// PTY 注册表（用于注册接收到的 FD）
    pty_registry: Arc<PtyRegistry>,

    /// Worker 管理器（用于监听 Worker 状态）
    worker_manager: Arc<WorkerManager>,

    /// 活动的连接（connection_id -> IpcConnection）
    connections: Arc<RwLock<HashMap<String, IpcConnection>>>,

    /// 请求 ID 计数器(阶段 2 新增,用于生成唯一的 request_id)
    request_id_counter: AtomicU64,
}

impl IpcServer {
    /// 创建新的 IPC 服务器
    ///
    /// # 参数
    /// - `socket_path`: Unix Socket 路径
    /// - `pty_registry`: PTY 注册表
    /// - `worker_manager`: Worker 管理器（用于监听 Worker 状态变化）
    pub fn new(
        socket_path: String,
        pty_registry: Arc<PtyRegistry>,
        worker_manager: Arc<WorkerManager>,
    ) -> Self {
        Self {
            listener: Arc::new(RwLock::new(None)),
            socket_path,
            pty_registry,
            worker_manager,
            connections: Arc::new(RwLock::new(HashMap::new())),
            request_id_counter: AtomicU64::new(1),
        }
    }

    /// 启动 IPC 服务器
    pub async fn start(&self) -> Result<()> {
        // 删除旧的 Socket 文件
        if std::path::Path::new(&self.socket_path).exists() {
            std::fs::remove_file(&self.socket_path)
                .context("Failed to remove old socket file")?;
        }

        // 绑定 Unix Socket
        let listener = UnixListener::bind(&self.socket_path)
            .context("Failed to bind Unix socket")?;

        let mut listener_guard = self.listener.write().await;
        *listener_guard = Some(listener);

        info!("IPC 服务器已启动: path={}", self.socket_path);

        Ok(())
    }

    /// 接收 Worker 连接
    ///
    /// # 返回
    /// - 连接 ID（用于后续接收 FD）
    pub async fn accept(&self) -> Result<String> {
        let listener_guard = self.listener.read().await;

        if let Some(ref listener) = *listener_guard {
            let (stream, _addr) = listener.accept().await
                .context("Failed to accept connection")?;

            let connection_id = uuid::Uuid::new_v4().to_string();
            let connection = IpcConnection::new(connection_id.clone(), stream);

            // 添加到活动连接列表
            let mut connections = self.connections.write().await;
            connections.insert(connection_id.clone(), connection);

            info!("Worker 连接已建立: connection_id={}", connection_id);

            Ok(connection_id)
        } else {
            Err(anyhow::anyhow!("IPC server not started"))
        }
    }

    /// 接收 Worker 连接并设置 worker_pid
    ///
    /// 当 Worker 启动后自动调用。
    ///
    /// # 热更新连接清理
    ///
    /// 热更新流程中,旧 Worker 优雅退出时不会触发 Crashed 事件
    /// (因为 is_graceful_shutdown=true),导致旧连接残留在 connections 中。
    /// 新 Worker 连接时,必须清理所有不同 PID 的旧连接,防止
    /// send_request 取到已断开的旧连接导致 receive_response EOF。
    pub async fn accept_and_set_pid(&self, worker_pid: u32) -> Result<String> {
        let connection_id = self.accept().await?;

        let mut connections = self.connections.write().await;

        // 设置新连接的 worker_pid
        if let Some(conn) = connections.get_mut(&connection_id) {
            conn.worker_pid = Some(worker_pid);
            info!(
                "Worker 连接已建立并设置 PID: connection_id={}, worker_pid={}",
                connection_id, worker_pid
            );
        }

        // 清理所有不同 PID 的旧连接（热更新后旧连接残留防护）
        let stale_ids: Vec<String> = connections
            .iter()
            .filter(|(id, conn)| {
                *id != &connection_id && conn.worker_pid != Some(worker_pid)
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in stale_ids {
            if let Some(mut conn) = connections.remove(&id) {
                let old_pid = conn.worker_pid;
                conn.close();
                info!(
                    "清理旧 Worker 残留连接（热更新）: connection_id={}, old_pid={:?}, new_pid={}",
                    id, old_pid, worker_pid
                );
            }
        }

        Ok(connection_id)
    }

    /// 发送 CreateSession 请求到 Worker 并连接 Session 进程(新架构)
    ///
    /// 新架构流程:
    /// 1. 发送 CreateSession 请求到 Worker
    /// 2. Worker 创建 Session 进程（openpty+fork），Session 进程 bind abstract socket
    /// 3. Worker 返回 SessionCreated 响应（含 session_id 和 socket_name）
    /// 4. Manager 通过 SessionConnection 连接到 Session 进程的 UnixSocket
    /// 5. 注册到 PtyRegistry
    ///
    /// 不再使用 SCM_RIGHTS 传递 master_fd。master_fd 由 Session 进程持有。
    ///
    /// # 参数
    /// - `request`: CreateSession 请求(含 shell/cols/rows/uid/gid 等)
    /// - `user_info`: 用户信息(用于 PtyRegistry 注册)
    ///
    /// # 返回
    /// 成功返回 session_id,失败返回错误
    pub async fn create_pty_session(
        &self,
        request: crate::protocol::generated::CreateSession,
        user_info: UserInfo,
    ) -> Result<String> {
        // 生成唯一 request_id
        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);

        // 构造 ManagerRequest
        let manager_request = ManagerRequest {
            request_id,
            payload: Some(manager_request::Payload::CreateSession(request)),
        };

        // 取出连接（不持有锁整个 await 过程，避免阻塞 IPC server run 循环）
        let connection_id;
        let mut connection = {
            let mut connections = self.connections.write().await;
            connection_id = connections.keys().next().cloned()
                .ok_or_else(|| anyhow::anyhow!("No active Worker connection"))?;
            connections.remove(&connection_id)
                .expect("connection must exist after keys().next()")
        };

        info!("发送 CreateSession 请求到 Worker: request_id={}", request_id);

        // 1. 发送请求
        if let Err(e) = connection.send_request(&manager_request).await {
            error!("发送 CreateSession 请求失败,清理连接: connection_id={}, error={:?}", connection_id, e);
            connection.close();
            return Err(e.context("Failed to send CreateSession request to Worker"));
        }

        // 2. 接收 WorkerResponse（新架构：不再接收 FD，只接收 protobuf 响应）
        let response = match connection.receive_response().await {
            Ok(resp) => resp,
            Err(e) => {
                error!("接收 CreateSession 响应失败,清理连接: connection_id={}, error={:?}", connection_id, e);
                connection.close();
                return Err(e.context("Failed to receive response from Worker"));
            }
        };

        // 将连接放回 connections（连接仍然可用）
        {
            let mut connections = self.connections.write().await;
            connections.insert(connection_id.clone(), connection);
        }

        // 3. 解析响应，连接 Session 进程
        match response.payload {
            Some(worker_response::Payload::SessionCreated(session_created)) => {
                let session_id = session_created.session_id;
                let socket_name = session_created.socket_name;

                info!(
                    "Worker 创建 Session 成功: session_id={}, socket_name={}",
                    session_id, socket_name
                );

                // 4. 连接 Session 进程的 UnixSocket
                //    SessionConnection::connect 会接收 Hello 帧并验证 session_id
                let connection = SessionConnection::connect(&socket_name, &session_id)
                    .await
                    .context("连接 Session 进程失败")?;

                // 5. 注册到 PtyRegistry
                let pty_session = PtySession {
                    session_id: session_id.clone(),
                    connection: Arc::new(connection),
                    user_info,
                    created_at: std::time::SystemTime::now(),
                };

                if let Err(e) = self.pty_registry.register(pty_session).await {
                    error!("注册 PTY 会话失败: session_id={}, error={:?}", session_id, e);
                    return Err(e.context("Failed to register PTY session"));
                }

                info!("PTY 会话创建成功: session_id={}", session_id);
                Ok(session_id)
            }
            Some(worker_response::Payload::Error(err)) => {
                Err(anyhow::anyhow!("Worker error: code={}, message={}", err.code, err.message))
            }
            _ => {
                Err(anyhow::anyhow!("Unexpected response from Worker: {:?}", response.payload))
            }
        }
    }

    /// 发送通用请求到 Worker 并接收响应(阶段 3 新增)
    ///
    /// 适用于不需要 FD 传递的业务操作(ReadDir/ReadFile/WriteFile 等)。
    /// 这是原子的 request-response 操作:
    /// 1. 生成唯一 request_id
    /// 2. 发送 ManagerRequest 到 Worker
    /// 3. 接收 WorkerResponse
    ///
    /// # 参数
    /// - `payload`: manager_request::Payload(具体的请求类型)
    ///
    /// # 返回
    /// 成功返回 WorkerResponse,失败返回错误
    pub async fn send_request(
        &self,
        payload: manager_request::Payload,
    ) -> Result<crate::protocol::generated::WorkerResponse> {
        // 生成唯一 request_id
        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);

        // 构造 ManagerRequest
        let manager_request = ManagerRequest {
            request_id,
            payload: Some(payload),
        };

        // 取出连接（不持有锁整个 await 过程，避免阻塞 IPC server run 循环）
        let connection_id;
        let mut connection = {
            let mut connections = self.connections.write().await;
            connection_id = connections.keys().next().cloned()
                .ok_or_else(|| anyhow::anyhow!("No active Worker connection"))?;
            connections.remove(&connection_id)
                .expect("connection must exist after keys().next()")
        };

        tracing::debug!("发送业务请求到 Worker: request_id={}", request_id);

        // 1. 发送请求
        if let Err(e) = connection.send_request(&manager_request).await {
            // 发送失败,连接可能已断开,清理坏连接
            error!("发送请求到 Worker 失败,清理连接: connection_id={}, error={:?}", connection_id, e);
            connection.close();
            return Err(e.context("Failed to send request to Worker"));
        }

        // 2. 接收响应(不含 FD)
        let response = match connection.receive_response().await {
            Ok(resp) => resp,
            Err(e) => {
                // receive_response 失败(EOF/解码失败等),连接已不可用
                error!(
                    "接收 Worker 响应失败,清理连接: connection_id={}, request_id={}, error={:?}",
                    connection_id, request_id, e
                );
                connection.close();
                return Err(e.context("Failed to receive response from Worker"));
            }
        };

        tracing::debug!("收到 Worker 响应: request_id={}", request_id);

        // 将连接放回 connections
        {
            let mut connections = self.connections.write().await;
            connections.insert(connection_id, connection);
        }

        Ok(response)
    }

    /// 清理指定 Worker 的连接
    ///
    /// 当 Worker 进程崩溃时调用
    pub async fn cleanup_connection(&self, connection_id: &str) -> Result<()> {
        let mut connections = self.connections.write().await;

        if let Some(mut connection) = connections.remove(connection_id) {
            connection.close();
            info!("连接已清理: connection_id={}", connection_id);
        }

        Ok(())
    }

    /// 根据 worker_pid 清理所有连接
    ///
    /// 当 Worker 进程崩溃时调用
    pub async fn cleanup_by_worker_pid(&self, worker_pid: u32) -> Result<()> {
        let mut connections = self.connections.write().await;

        let to_remove: Vec<String> = connections
            .iter()
            .filter(|(_, conn)| conn.worker_pid == Some(worker_pid))
            .map(|(id, _)| id.clone())
            .collect();

        for id in to_remove {
            if let Some(mut conn) = connections.remove(&id) {
                conn.close();
                info!("已清理 Worker 连接: worker_pid={}, connection_id={}", worker_pid, id);
            }
        }

        Ok(())
    }

    /// 获取活动连接数
    pub async fn active_connection_count(&self) -> usize {
        let connections = self.connections.read().await;
        connections.len()
    }

    /// 运行 IPC 服务器
    ///
    /// 监听 WorkerManager 状态变化事件：
    /// - Worker 启动时：接受连接并设置 worker_pid
    /// - Worker 崩溃时：清理连接
    ///
    /// # 参数
    /// - `ready`: 可选的 oneshot sender，在 IPC bind + subscribe 完成后发送信号
    ///           调用方（Manager::start）等待此信号后才启动 Worker，避免竞态条件
    pub async fn run(&self, ready: Option<tokio::sync::oneshot::Sender<()>>) -> Result<()> {
        // 启动 IPC 监听（bind Unix Socket）
        self.start().await?;

        // 订阅 WorkerManager 事件（必须在 Worker 启动前完成，否则会错过 Worker 启动事件）
        let mut event_rx = self.worker_manager.subscribe();

        // 通知调用方 IPC 已就绪（socket 已 bind + 事件已订阅）
        if let Some(tx) = ready {
            let _ = tx.send(());
        }

        info!("IPC 服务器开始监听 Worker 状态变化");

        // 处理 Worker 状态变化
        loop {
            tokio::select! {
                // 接收 Worker 状态变化事件
                event = event_rx.recv() => {
                    match event {
                        Ok(worker_event) => {
                            match worker_event.status {
                                WorkerStatus::Starting => {
                                    // Worker 启动，接受连接
                                    info!(
                                        "检测到 Worker 启动事件，准备接受连接: pid={}",
                                        worker_event.pid
                                    );

                                    // 等待 Worker 连接（带超时）
                                    match tokio::time::timeout(
                                        std::time::Duration::from_secs(5),
                                        self.accept_and_set_pid(worker_event.pid)
                                    ).await {
                                        Ok(Ok(connection_id)) => {
                                            info!(
                                                "Worker 连接建立成功: pid={}, connection_id={}",
                                                worker_event.pid, connection_id
                                            );
                                        }
                                        Ok(Err(e)) => {
                                            error!(
                                                "Worker 连接建立失败: pid={}, error={}",
                                                worker_event.pid, e
                                            );
                                        }
                                        Err(_) => {
                                            warn!(
                                                "Worker 连接超时: pid={}",
                                                worker_event.pid
                                            );
                                        }
                                    }
                                }

                                WorkerStatus::Crashed => {
                                    // Worker 崩溃，清理连接
                                    warn!(
                                        "检测到 Worker 崩溃事件，清理连接: pid={}",
                                        worker_event.pid
                                    );
                                    self.cleanup_by_worker_pid(worker_event.pid).await?;
                                }

                                _ => {
                                    // 其他状态暂不处理
                                }
                            }
                        }
                        Err(e) => {
                            // broadcast channel 错误（可能是发送端已关闭）
                            warn!("Worker 事件通道错误: {}", e);
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 停止 IPC 服务器
    pub async fn stop(&self) -> Result<()> {
        // 清理所有连接
        {
            let mut connections = self.connections.write().await;
            for (_, mut conn) in connections.drain() {
                conn.close();
            }
        }

        // 关闭监听器
        {
            let mut listener_guard = self.listener.write().await;
            *listener_guard = None;
        }

        // 删除 Socket 文件
        if std::path::Path::new(&self.socket_path).exists() {
            std::fs::remove_file(&self.socket_path)
                .context("Failed to remove socket file")?;
        }

        info!("IPC 服务器已停止");

        Ok(())
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        // 尝试清理 Socket 文件
        if std::path::Path::new(&self.socket_path).exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::worker_manager::WorkerStatus;

    #[tokio::test]
    async fn test_ipc_server_creation() {
        let registry = Arc::new(PtyRegistry::new());
        let worker_manager = Arc::new(WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3
        ));
        let server = IpcServer::new("/tmp/test_ipc.sock".to_string(), registry, worker_manager);

        assert!(server.listener.read().await.is_none());
        assert_eq!(server.active_connection_count().await, 0);
    }

    #[tokio::test]
    async fn test_ipc_server_start_stop() {
        let registry = Arc::new(PtyRegistry::new());
        let worker_manager = Arc::new(WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3
        ));
        let server = IpcServer::new("/tmp/test_ipc_start.sock".to_string(), registry.clone(), worker_manager);

        // 启动
        let result = server.start().await;
        assert!(result.is_ok());

        // 停止
        let result = server.stop().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_worker_status_event_handling() {
        let registry = Arc::new(PtyRegistry::new());
        let worker_manager = Arc::new(WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test_worker.sock".to_string(),
            3
        ));
        let server = IpcServer::new("/tmp/test_worker_event.sock".to_string(), registry, worker_manager.clone());

        // 启动 IPC 服务器
        server.start().await.unwrap();

        // 订阅事件
        let mut event_rx = worker_manager.subscribe();

        // 模拟发送 Worker 启动事件（通过 update_status）
        worker_manager.update_status(WorkerStatus::Starting).await;

        // 接收事件（添加超时避免死锁：没有真实 Worker 时不会发送事件）
        let event = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            event_rx.recv()
        ).await;

        // 验证 subscribe() 能正常工作（事件可能为 None，因为 worker_info 为 None）
        // 只要不死锁即可

        // 清理
        server.stop().await.unwrap();
    }
}