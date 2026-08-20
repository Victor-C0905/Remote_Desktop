//! IPC 服务器
//!
//! 与 WorkerManager 和 PtyRegistry 集成：
//! - 接收 Worker 进程的连接
//! - 发送请求到 Worker（CreateSession/ReadDir 等）并接收响应
//! - CreateSession 响应包含 socket_name，Manager 通过 SessionConnection 连接到 Session 进程
//!
//! 新架构：不再使用 SCM_RIGHTS 传递 master_fd。
//! master_fd 由 Session 进程持有，Manager 通过 UnixSocket 帧协议与 Session 通信。
//!
//! 并发模型重构：消除"取出-await-放回"竞态，改为 mpsc + request_id + dispatcher 模型。
//! - 调用方通过 send_request / create_pty_session 将请求发到 mpsc channel
//! - dispatcher task 从 channel 读取请求，写入 UnixStream，并在 pending 表中注册 oneshot sender
//! - response_router task 从 UnixStream 读取响应，按 request_id 路由到对应 oneshot sender
//! - 等待方通过 oneshot receiver 收到响应

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use anyhow::{Result, Context};
use prost::Message;
use tracing::{info, warn, error};
use dashmap::DashMap;

use super::pty_registry::{PtyRegistry, PtySession, UserInfo};
use super::session_connection::SessionConnection;
use super::worker_manager::{WorkerManager, WorkerStatus};
use crate::protocol::generated::{
    ManagerRequest, WorkerResponse,
    manager_request, worker_response,
};

/// 请求入口 channel 的载荷项
///
/// 调用方（send_request / create_pty_session）构造此项，
/// 通过 mpsc channel 发送给 dispatcher task。
/// dispatcher 将其写入 UnixStream，并在 pending 表中注册 response_tx。
pub struct RequestItem {
    /// 请求 ID（用于响应路由配对）
    pub request_id: u64,
    /// protobuf 请求载荷
    pub payload: manager_request::Payload,
    /// 响应回传的 oneshot sender（由 response_router 填充）
    pub response_tx: oneshot::Sender<WorkerResponse>,
}

/// IPC 服务器
///
/// 与 WorkerManager 和 PtyRegistry 集成：
/// - 接收 Worker 进程的连接
/// - 通过 dispatcher task 转发请求到 Worker，并按 request_id 路由响应
/// - CreateSession 响应包含 socket_name，Manager 通过 SessionConnection 连接到 Session 进程
///
/// 并发模型：mpsc + request_id + dispatcher
/// - 单一 dispatcher task 拥有 UnixStream 的写半部，串行写入请求
/// - response_router task 拥有读半部，按 request_id 分发响应到等待者
/// - 消除"取出-await-放回"竞态
pub struct IpcServer {
    /// Unix Socket 监听器
    listener: Arc<RwLock<Option<UnixListener>>>,

    /// Socket 路径
    socket_path: String,

    /// PTY 注册表（用于注册接收到的 FD）
    pty_registry: Arc<PtyRegistry>,

    /// Worker 管理器（用于监听 Worker 状态）
    worker_manager: Arc<WorkerManager>,

    /// 请求入口 channel（dispatcher task 从此读取请求转发给 Worker）
    /// 使用 Mutex 包装以支持 accept_and_set_pid 中的原子替换
    request_tx: tokio::sync::Mutex<mpsc::Sender<RequestItem>>,

    /// 等待响应的请求表（request_id -> oneshot::Sender）
    /// dispatcher 写入请求前注册 sender，response_router 收到响应后取出并发送
    pending: Arc<DashMap<u64, oneshot::Sender<WorkerResponse>>>,

    /// dispatcher task 的 JoinHandle（stop 时 abort）
    dispatcher_handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,

    /// request_id 计数器(阶段 2 新增,用于生成唯一的 request_id)
    request_id_counter: AtomicU64,

    /// mpsc channel 容量（accept_and_set_pid 创建新 channel 时使用）
    channel_capacity: usize,
}

impl IpcServer {
    /// 创建新的 IPC 服务器
    ///
    /// # 参数
    /// - `socket_path`: Unix Socket 路径
    /// - `pty_registry`: PTY 注册表
    /// - `worker_manager`: Worker 管理器（用于监听 Worker 状态变化）
    /// - `channel_capacity`: mpsc channel 容量（dispatcher 的请求队列大小）
    pub fn new(
        socket_path: String,
        pty_registry: Arc<PtyRegistry>,
        worker_manager: Arc<WorkerManager>,
        channel_capacity: usize,
    ) -> Self {
        // 创建初始 channel，receiver 立即 drop
        // 此时 request_tx 处于"无接收者"状态，send_request 会立即返回 Err("dispatcher dropped")
        // accept_and_set_pid 时会创建新 channel 并替换
        let (request_tx, _request_rx) = mpsc::channel(channel_capacity);

        Self {
            listener: Arc::new(RwLock::new(None)),
            socket_path,
            pty_registry,
            worker_manager,
            request_tx: tokio::sync::Mutex::new(request_tx),
            pending: Arc::new(DashMap::new()),
            dispatcher_handle: tokio::sync::Mutex::new(None),
            request_id_counter: AtomicU64::new(1),
            channel_capacity,
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

    /// 接收 Worker 连接并设置 worker_pid
    ///
    /// 当 Worker 启动后自动调用。
    ///
    /// 新架构（mpsc + dispatcher 模型）：
    /// 1. accept 新 UnixStream
    /// 2. 创建新 mpsc channel (request_tx, request_rx)
    /// 3. abort 旧 dispatcher（如有）
    /// 4. 启动新 dispatcher task，传入 request_rx + pending + stream
    /// 5. 更新 self.request_tx 为新 channel 的 sender
    ///
    /// # 热更新连接清理
    ///
    /// 热更新流程中,旧 Worker 优雅退出时不会触发 Crashed 事件
    /// (因为 is_graceful_shutdown=true),导致旧连接残留在 connections 中。
    /// 新架构中，abort 旧 dispatcher 会自动触发 pending.clear()，
    /// 所有等待响应的调用方会收到 "response dropped" 错误，
    /// 不再需要显式清理旧连接。
    pub async fn accept_and_set_pid(&self, worker_pid: u32) -> Result<()> {
        // 1. accept 新 UnixStream
        let stream = {
            let listener_guard = self.listener.read().await;
            let listener = listener_guard.as_ref()
                .ok_or_else(|| anyhow::anyhow!("IPC server not started"))?;
            let (stream, _addr) = listener.accept().await
                .context("Failed to accept connection")?;
            stream
        };

        // 2. 创建新 mpsc channel
        let (request_tx, request_rx) = mpsc::channel(self.channel_capacity);

        // 3. abort 旧 dispatcher（热更新时旧 dispatcher 自动清理 pending）
        if let Some(handle) = self.dispatcher_handle.lock().await.take() {
            handle.abort();
            info!("已中止旧 dispatcher task（热更新/重连）");
        }

        // 4. 启动新 dispatcher task，传入 request_rx + pending + stream
        let handle = tokio::spawn(run_dispatcher(
            request_rx,
            self.pending.clone(),
            stream,
        ));

        // 5. 存储 dispatcher handle 并更新 request_tx
        {
            let mut handle_guard = self.dispatcher_handle.lock().await;
            *handle_guard = Some(handle);
            let mut tx_guard = self.request_tx.lock().await;
            *tx_guard = request_tx;
        }

        info!(
            "Worker 连接已建立并设置 PID: worker_pid={}, channel_capacity={}",
            worker_pid, self.channel_capacity
        );

        Ok(())
    }

    /// 发送 CreateSession 请求到 Worker 并连接 Session 进程(新架构)
    ///
    /// 新架构流程:
    /// 1. 发送 CreateSession 请求到 Worker（通过 mpsc channel 转发给 dispatcher）
    /// 2. Worker 创建 Session 进程（openpty+fork），Session 进程 bind abstract socket
    /// 3. Worker 返回 SessionCreated 响应（含 session_id 和 socket_name）
    /// 4. Manager 通过 SessionConnection 连接到 Session 进程的 UnixSocket
    /// 5. 注册到 PtyRegistry
    ///
    /// 不再使用 SCM_RIGHTS 传递 master_fd。master_fd 由 Session 进程持有。
    ///
    /// 并发模型：通过 mpsc + request_id + dispatcher，消除"取出-await-放回"竞态。
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

        // 构造 oneshot channel 用于接收响应
        let (tx, rx) = oneshot::channel();
        let item = RequestItem {
            request_id,
            payload: manager_request::Payload::CreateSession(request),
            response_tx: tx,
        };

        info!("发送 CreateSession 请求到 Worker: request_id={}", request_id);

        // 通过 mpsc channel 发送给 dispatcher task（克隆 sender 避免长时间持有锁）
        let sender = self.request_tx.lock().await.clone();
        sender.send(item).await
            .map_err(|_| anyhow::anyhow!("dispatcher dropped"))?;

        // 等待 dispatcher 路由回来的响应
        let response = rx.await
            .map_err(|_| anyhow::anyhow!("response dropped"))?;

        // 解析响应，连接 Session 进程
        match response.payload {
            Some(worker_response::Payload::SessionCreated(session_created)) => {
                let session_id = session_created.session_id;
                let socket_name = session_created.socket_name;

                info!(
                    "Worker 创建 Session 成功: session_id={}, socket_name={}",
                    session_id, socket_name
                );

                // 连接 Session 进程的 UnixSocket
                //    SessionConnection::connect 会接收 Hello 帧并验证 session_id
                let connection = SessionConnection::connect(&socket_name, &session_id)
                    .await
                    .context("连接 Session 进程失败")?;

                // 注册到 PtyRegistry
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
    /// 2. 通过 mpsc channel 发送 ManagerRequest 到 dispatcher
    /// 3. dispatcher 写入 UnixStream 并在 pending 表注册 oneshot sender
    /// 4. response_router 收到响应后按 request_id 路由回来
    ///
    /// 并发模型：mpsc + request_id + dispatcher，消除"取出-await-放回"竞态。
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

        // 构造 oneshot channel 用于接收响应
        let (tx, rx) = oneshot::channel();
        let item = RequestItem {
            request_id,
            payload,
            response_tx: tx,
        };

        tracing::debug!("发送业务请求到 Worker: request_id={}", request_id);

        // 通过 mpsc channel 发送给 dispatcher task（克隆 sender 避免长时间持有锁）
        let sender = self.request_tx.lock().await.clone();
        sender.send(item).await
            .map_err(|_| anyhow::anyhow!("dispatcher dropped"))?;

        // 等待 dispatcher 路由回来的响应
        let response = rx.await
            .map_err(|_| anyhow::anyhow!("response dropped"))?;

        tracing::debug!("收到 Worker 响应: request_id={}", request_id);

        Ok(response)
    }

    /// 运行 IPC 服务器
    ///
    /// 监听 WorkerManager 状态变化事件：
    /// - Worker 启动时：接受连接并设置 worker_pid（启动 dispatcher）
    /// - Worker 崩溃时：dispatcher 自动断开清理（无需显式 cleanup）
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
                                    // Worker 启动，接受连接（启动 dispatcher）
                                    info!(
                                        "检测到 Worker 启动事件，准备接受连接: pid={}",
                                        worker_event.pid
                                    );

                                    // 等待 Worker 连接（带超时）
                                    match tokio::time::timeout(
                                        std::time::Duration::from_secs(5),
                                        self.accept_and_set_pid(worker_event.pid)
                                    ).await {
                                        Ok(Ok(())) => {
                                            info!(
                                                "Worker 连接建立成功: pid={}",
                                                worker_event.pid
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
                                    // Worker 崩溃，dispatcher 断开自动清理（无需显式 cleanup）
                                    warn!(
                                        "检测到 Worker 崩溃事件: pid={}",
                                        worker_event.pid
                                    );
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
    ///
    /// 1. abort dispatcher task
    /// 2. 清理 pending（所有等待者会收到 "response dropped"）
    /// 3. 关闭 listener
    /// 4. 删除 socket 文件
    pub async fn stop(&self) -> Result<()> {
        // abort dispatcher task
        if let Some(handle) = self.dispatcher_handle.lock().await.take() {
            handle.abort();
        }

        // 清理所有等待响应的调用方（onesot sender drop 后 rx.await 返回 Err）
        self.pending.clear();

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

/// dispatcher task：从 mpsc channel 读取请求，转发到 Worker UnixStream
///
/// 内部启动 response_router 子 task：
/// - response_router 从读半部读取响应，按 request_id 路由到 pending 表中的 oneshot sender
///
/// 请求转发循环：
/// - 从 mpsc channel 接收 RequestItem
/// - 在 pending 表注册 response_tx
/// - 将请求写入 UnixStream 写半部
/// - 写入失败则从 pending 移除并退出循环
///
/// 退出时清理 pending（所有等待者收到 "response dropped"）
async fn run_dispatcher(
    mut rx: mpsc::Receiver<RequestItem>,
    pending: Arc<DashMap<u64, oneshot::Sender<WorkerResponse>>>,
    stream: tokio::net::UnixStream,
) {
    // split UnixStream 为 read_half 和 write_half
    let (read_half, mut write_half) = stream.into_split();

    // 响应路由 task：从 read_half 读响应，按 request_id 路由到对应 oneshot
    let pending_clone = pending.clone();
    let response_router = tokio::spawn(async move {
        let mut read_half = read_half;
        loop {
            match read_response(&mut read_half).await {
                Ok((request_id, response)) => {
                    if let Some((_, sender)) = pending_clone.remove(&request_id) {
                        let _ = sender.send(response);
                    }
                }
                Err(_) => {
                    // 读取失败（EOF/解码错误等），清理所有等待者
                    pending_clone.clear();
                    break;
                }
            }
        }
    });

    // 请求转发循环
    while let Some(item) = rx.recv().await {
        pending.insert(item.request_id, item.response_tx);
        if write_request(&mut write_half, item.request_id, item.payload).await.is_err() {
            // 写入失败，从 pending 移除该请求
            pending.remove(&item.request_id);
            break;
        }
    }

    // 清理所有等待者（oneshot sender drop 后调用方 rx.await 返回 Err）
    pending.clear();
    let _ = response_router.await;
}

/// 写入 ManagerRequest 到 UnixStream 写半部
///
/// 消息格式:[4字节长度(big-endian)] + [protobuf 内容]
async fn write_request(
    write_half: &mut tokio::net::unix::OwnedWriteHalf,
    request_id: u64,
    payload: manager_request::Payload,
) -> Result<()> {
    let manager_request = ManagerRequest {
        request_id,
        payload: Some(payload),
    };
    let mut buf = Vec::new();
    manager_request.encode(&mut buf)
        .context("Failed to encode ManagerRequest")?;
    let len = buf.len() as u32;
    write_half.write_all(&len.to_be_bytes()).await
        .context("Failed to write request length")?;
    write_half.write_all(&buf).await
        .context("Failed to write request content")?;
    Ok(())
}

/// 从 UnixStream 读半部读取 WorkerResponse
///
/// 消息格式:[4字节长度(big-endian)] + [protobuf 内容]
///
/// # 返回
/// (request_id, WorkerResponse) — request_id 用于路由配对
async fn read_response(
    read_half: &mut tokio::net::unix::OwnedReadHalf,
) -> Result<(u64, WorkerResponse)> {
    let mut len_buf = [0u8; 4];
    read_half.read_exact(&mut len_buf).await
        .context("Failed to read response length")?;
    let len = u32::from_be_bytes(len_buf) as usize;

    const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;
    if len > MAX_MESSAGE_SIZE {
        anyhow::bail!("Response too large: {} bytes", len);
    }

    let mut msg_buf = vec![0u8; len];
    read_half.read_exact(&mut msg_buf).await
        .context("Failed to read response content")?;

    let response = WorkerResponse::decode(&msg_buf[..])
        .context("Failed to decode WorkerResponse")?;
    Ok((response.request_id, response))
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
        let server = IpcServer::new("/tmp/test_ipc.sock".to_string(), registry, worker_manager, 128);

        assert!(server.listener.read().await.is_none());
        // 新架构：验证 pending 表初始为空
        assert!(server.pending.is_empty());
    }

    #[tokio::test]
    async fn test_ipc_server_start_stop() {
        let registry = Arc::new(PtyRegistry::new());
        let worker_manager = Arc::new(WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3
        ));
        let server = IpcServer::new("/tmp/test_ipc_start.sock".to_string(), registry.clone(), worker_manager, 128);

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
        let server = IpcServer::new("/tmp/test_worker_event.sock".to_string(), registry, worker_manager.clone(), 128);

        // 启动 IPC 服务器
        server.start().await.unwrap();

        // 订阅事件
        let mut event_rx = worker_manager.subscribe();

        // 模拟发送 Worker 启动事件（通过 update_status）
        worker_manager.update_status(WorkerStatus::Starting).await;

        // 接收事件（添加超时避免死锁：没有真实 Worker 时不会发送事件）
        let _event = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            event_rx.recv()
        ).await;

        // 验证 subscribe() 能正常工作（事件可能为 None，因为 worker_info 为 None）
        // 只要不死锁即可

        // 清理
        server.stop().await.unwrap();
    }

    /// 并发回归测试：验证 10 个并发请求同时到达 IpcServer 时无竞态错误
    ///
    /// 场景：模拟旧的"取出-await-放回"竞态条件已被消除
    /// - 启动 dispatcher task 处理真实 UnixStream
    /// - mock Worker 端按 request_id 回复 Error 响应
    /// - 10 个 send_request 并发发起
    /// - 验证所有响应正确收到，且 request_id 无错乱
    ///
    /// 该测试回归验证 "No active Worker connection" 错误已消除
    #[cfg(unix)]
    #[tokio::test]
    async fn test_concurrent_requests_no_race_condition() {
        use crate::protocol::generated::{Error, GetSystemInfo};

        // 1. 创建 UnixStream pair（manager_side 给 dispatcher，worker_side 给 mock Worker）
        let (manager_side, worker_side) = tokio::net::UnixStream::pair()
            .expect("创建 UnixStream pair 失败");

        // 2. 创建 IpcServer 实例（不调用 start/accept_and_set_pid，手动初始化内部状态）
        let registry = Arc::new(PtyRegistry::new());
        let worker_manager = Arc::new(WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3,
        ));
        let server = Arc::new(IpcServer::new(
            "/tmp/test_concurrent.sock".to_string(),
            registry,
            worker_manager,
            128,
        ));

        // 3. 手动创建 mpsc channel 并启动 dispatcher（绕过 accept_and_set_pid）
        let (request_tx, request_rx) = mpsc::channel(128);
        let pending = server.pending.clone();
        let dispatcher_handle = tokio::spawn(run_dispatcher(request_rx, pending, manager_side));

        // 替换 IpcServer 的 request_tx 和 dispatcher_handle
        {
            let mut tx_guard = server.request_tx.lock().await;
            *tx_guard = request_tx;
            let mut handle_guard = server.dispatcher_handle.lock().await;
            *handle_guard = Some(dispatcher_handle);
        }

        // 4. 启动 mock Worker task：读取 10 个请求，按 request_id 回复 Error 响应
        let worker_handle = tokio::spawn(async move {
            let (mut read_half, mut write_half) = worker_side.into_split();
            for _ in 0..10 {
                // 读取 4 字节长度
                let mut len_buf = [0u8; 4];
                if read_half.read_exact(&mut len_buf).await.is_err() {
                    break;
                }
                let len = u32::from_be_bytes(len_buf) as usize;

                // 读取 protobuf 内容
                let mut msg_buf = vec![0u8; len];
                if read_half.read_exact(&mut msg_buf).await.is_err() {
                    break;
                }

                // 解析 request_id
                let req = match ManagerRequest::decode(&msg_buf[..]) {
                    Ok(r) => r,
                    Err(_) => break,
                };
                let request_id = req.request_id;

                // 构造 Error 响应（带 request_id，用于路由配对）
                let response = WorkerResponse {
                    request_id,
                    payload: Some(worker_response::Payload::Error(Error {
                        code: 0,
                        message: format!("mock-{}", request_id),
                    })),
                };
                let mut resp_buf = Vec::new();
                if response.encode(&mut resp_buf).is_err() {
                    break;
                }
                let resp_len = resp_buf.len() as u32;
                if write_half.write_all(&resp_len.to_be_bytes()).await.is_err() {
                    break;
                }
                if write_half.write_all(&resp_buf).await.is_err() {
                    break;
                }
            }
            // mock Worker 退出时关闭两端，response_router 读 EOF 后 clear pending
        });

        // 5. 并发发起 10 个 send_request
        let mut handles = Vec::new();
        for _ in 0..10 {
            let server_clone = Arc::clone(&server);
            let handle = tokio::spawn(async move {
                let payload = manager_request::Payload::GetSystemInfo(GetSystemInfo {});
                server_clone.send_request(payload).await
            });
            handles.push(handle);
        }

        // 6. 等待所有响应并验证
        let mut success_count = 0;
        let mut request_ids = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(response)) => {
                    success_count += 1;
                    request_ids.push(response.request_id);
                    // 验证响应是 Error 类型
                    assert!(
                        matches!(response.payload, Some(worker_response::Payload::Error(_))),
                        "响应应为 Error 类型"
                    );
                    // 验证 request_id 与响应中的 message 配对（无错乱）
                    if let Some(worker_response::Payload::Error(e)) = response.payload {
                        assert_eq!(
                            e.message, format!("mock-{}", response.request_id),
                            "request_id 与响应 message 应配对，无错乱"
                        );
                    }
                }
                Ok(Err(e)) => {
                    panic!("send_request 失败（竞态错误未消除）: {}", e);
                }
                Err(e) => {
                    panic!("task panicked: {}", e);
                }
            }
        }

        // 验证所有 10 个请求都成功（核心断言：无 "No active Worker connection"）
        assert_eq!(success_count, 10, "所有 10 个并发请求应成功完成，无竞态错误");

        // 验证 request_id 唯一性（无错乱/重复）
        request_ids.sort();
        request_ids.dedup();
        assert_eq!(request_ids.len(), 10, "所有 request_id 应唯一，无错乱");

        // 等待 mock Worker 完成
        let _ = worker_handle.await;

        // 清理
        server.stop().await.ok();
    }
}
