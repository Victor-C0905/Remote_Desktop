# 并发 IPC 模型实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除 Manager 侧 IPC 连接取出-放回竞态，引入 Worker 侧 tokio 并发 dispatch，实现真正的并发请求处理。

**Architecture:** 消息驱动模型——Manager 用 mpsc + oneshot + request_id 路由替代 HashMap 取出-放回；Worker 用 `select! { receive → spawn | response_rx → send }` 替代串行循环。UnixStream 通过 `into_split()` 拆分读写端，dispatcher task 独立路由响应。

**Tech Stack:** Rust 2024, tokio 1.52 (rt-multi-thread), dashmap, prost/protobuf, mpsc/oneshot channels

**Spec:** `docs/superpowers/specs/2026-08-19-concurrent-ipc-design.md`

---

## 文件结构

### 新建文件
- 无（不引入新模块，仅重构现有文件）

### 修改文件
| 文件 | 改动 | 职责 |
|---|---|---|
| `agent/Cargo.toml` | 添加 dashmap 依赖 | 第三方库 |
| `agent/src/config.rs` | WorkerConfig 新增 `ipc_channel_capacity` | 配置 |
| `agent/src/main.rs` | `run_worker_mode` 接收 `--ipc-channel-capacity` 参数 | CLI |
| `agent/src/manager/ipc_server.rs` | 核心重构 | dispatcher + 路由 |
| `agent/src/manager/mod.rs` | `Manager::new` 传递 channel 容量 | 装配 |
| `agent/src/worker/mod.rs` | `run()` 重写为并发 dispatch | Worker 主循环 |
| `agent/src/worker/handlers/session.rs` | `handle_create_session` 包装 spawn_blocking | 阻塞隔离 |
| `agent/src/worker/handlers/file.rs` | 10 个 handler 评估 spawn_blocking | 阻塞隔离 |
| `agent/src/worker/handlers/command.rs` | `handle_execute_command` 包装 spawn_blocking | 阻塞隔离 |

### 测试文件
| 文件 | 测试内容 |
|---|---|
| `agent/src/manager/ipc_server.rs` (内联 `#[cfg(test)]`) | request_id 路由、pending 清理 |
| `agent/tests/concurrent_ipc_test.rs` | 并发回归测试 |

---

## Task 1: 添加 dashmap 依赖

**Files:**
- Modify: `agent/Cargo.toml`

- [ ] **Step 1: 添加 dashmap 依赖**

在 `agent/Cargo.toml` 的 `[dependencies]` 段添加：

```toml
dashmap = "6"
```

位置：放在 `tokio` 依赖下方，保持字母序。

- [ ] **Step 2: 验证依赖添加成功**

Run: `cd agent && cargo fetch`
Expected: 无错误，dashmap 6.x 被下载。

- [ ] **Step 3: 提示用户提交**

提示用户执行：
```bash
git add agent/Cargo.toml agent/Cargo.lock
git commit -m "build: 添加 dashmap 依赖用于并发 IPC 路由表"
```

---

## Task 2: 添加 ipc_channel_capacity 配置项

**Files:**
- Modify: `agent/src/config.rs:151-164` (WorkerConfig 定义)
- Modify: `agent/src/config.rs:183-191` (WorkerConfig Default)
- Modify: `agent/src/config.rs:213-228` (default 函数)
- Modify: `agent/src/config.rs:251-287` (default_config)

- [ ] **Step 1: 在 WorkerConfig 添加字段**

在 `agent/src/config.rs` 的 `WorkerConfig` 结构体中，`max_restarts` 字段后添加：

```rust
    /// 最大重启次数
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,

    /// IPC channel 容量（mpsc）
    /// 控制 Manager→Worker 请求通道和 Worker 内部响应通道的缓冲大小
    /// 默认 128，极端高并发场景可调大
    #[serde(default = "default_ipc_channel_capacity")]
    pub ipc_channel_capacity: usize,
```

- [ ] **Step 2: 在 WorkerConfig Default impl 添加字段**

在 `impl Default for WorkerConfig` 中添加：

```rust
impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            agent_binary: default_agent_binary(),
            ipc_socket_path: default_ipc_socket_path(),
            max_restarts: default_max_restarts(),
            ipc_channel_capacity: default_ipc_channel_capacity(),
        }
    }
}
```

- [ ] **Step 3: 添加 default 函数**

在 `agent/src/config.rs` 末尾的 default 函数区域（`default_max_restarts` 下方）添加：

```rust
fn default_ipc_channel_capacity() -> usize { 128 }
```

- [ ] **Step 4: 在 default_config 中添加字段**

在 `default_config()` 函数的 `worker: WorkerConfig::default()` 行确认已是 `WorkerConfig::default()`（若如此则无需改动；若显式列出字段则添加 `ipc_channel_capacity: default_ipc_channel_capacity()`）。

检查 `default_config()` 中 `worker` 字段：
```rust
        worker: WorkerConfig::default(),
```
确认是 `WorkerConfig::default()` → 无需改动。

- [ ] **Step 5: 验证编译**

Run: `cd agent && cargo build --bin agent 2>&1 | head -20`
Expected: 无错误（新字段有 Default，serde default 属性已设置）。

- [ ] **Step 6: 提示用户提交**

提示用户执行：
```bash
git add agent/src/config.rs
git commit -m "feat(config): 添加 ipc_channel_capacity 配置项（默认 128）"
```

---

## Task 3: 重构 IpcServer 结构（移除竞态根源）

**Files:**
- Modify: `agent/src/manager/ipc_server.rs:1-130` (imports + IpcConnection + IpcServer 结构)

- [ ] **Step 1: 更新 imports**

替换 `agent/src/manager/ipc_server.rs` 顶部的 imports 块（第 1-27 行）：

```rust
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
//! 并发模型：消息驱动 + request_id 路由
//! - 多个 QUIC task 通过 mpsc 提交请求（无锁）
//! - dispatcher task 串行化写入 UnixStream
//! - Worker 响应通过 request_id 路由到对应 oneshot
//! - 消除取出-放回竞态

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::task::JoinHandle;
use dashmap::DashMap;
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
```

- [ ] **Step 2: 删除 IpcConnection 结构**

删除 `agent/src/manager/ipc_server.rs` 中整个 `IpcConnection` 结构及其 `impl` 块（原第 29-104 行）。这些逻辑被 dispatcher task 取代。

- [ ] **Step 3: 定义 RequestItem 结构**

在删除 IpcConnection 的位置添加：

```rust
/// 请求项（通过 mpsc 提交给 dispatcher）
pub struct RequestItem {
    /// 请求 ID（用于响应路由）
    pub request_id: u64,
    /// 请求 payload
    pub payload: manager_request::Payload,
    /// 响应接收端（dispatcher 完成后通过此 channel 返回响应）
    pub response_tx: oneshot::Sender<Result<WorkerResponse>>,
}
```

- [ ] **Step 4: 重写 IpcServer 结构**

替换 `IpcServer` 结构定义（原第 106-130 行）：

```rust
/// IPC 服务器
///
/// 并发模型：消息驱动 + request_id 路由
/// - request_tx: 多个 QUIC task → dispatcher（mpsc 串行化提交，无锁）
/// - pending: dispatcher → 对应 QUIC task（DashMap 并发读写）
/// - dispatcher task: 单独 task 处理 UnixStream 读写
pub struct IpcServer {
    /// Unix Socket 监听器
    listener: Arc<RwLock<Option<UnixListener>>>,

    /// Socket 路径
    socket_path: String,

    /// PTY 注册表（用于注册接收到的 FD）
    pty_registry: Arc<PtyRegistry>,

    /// Worker 管理器（用于监听 Worker 状态）
    worker_manager: Arc<WorkerManager>,

    /// 请求通道：多个 QUIC task → dispatcher（无锁提交）
    /// 在 Worker 连接建立时创建，dispatcher 退出时 drop
    request_tx: tokio::sync::Mutex<Option<mpsc::Sender<RequestItem>>>,

    /// 响应路由表：request_id → oneshot::Sender
    /// dispatcher 收到响应后通过 request_id 路由
    pending: Arc<DashMap<u64, oneshot::Sender<Result<WorkerResponse>>>>,

    /// 请求 ID 计数器
    request_id_counter: AtomicU64,

    /// channel 容量（可配置，默认 128）
    channel_capacity: usize,

    /// dispatcher task 句柄（用于停机）
    dispatcher_handle: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}
```

- [ ] **Step 5: 验证编译（预期有未实现的方法错误）**

Run: `cd agent && cargo build --bin agent 2>&1 | head -40`
Expected: 大量错误（因为 IpcServer::new 和其他方法仍引用旧的 connections 字段）。这是预期的，后续任务会修复。

- [ ] **Step 6: 提示用户提交**

提示用户执行：
```bash
git add agent/src/manager/ipc_server.rs
git commit -m "refactor(ipc): 重构 IpcServer 结构为消息驱动模型（移除竞态根源）"
```

---

## Task 4: 实现 dispatcher task

**Files:**
- Modify: `agent/src/manager/ipc_server.rs`（在 IpcServer impl 块之前添加私有函数）

- [ ] **Step 1: 添加 write_request 和 read_response 辅助函数**

在 `agent/src/manager/ipc_server.rs` 中，`RequestItem` 定义之后、`IpcServer` impl 块之前添加：

```rust
/// 写入 ManagerRequest 到 UnixStream 写端
///
/// 消息格式：[4字节长度 big-endian] + [protobuf 内容]
async fn write_request(
    write_half: &mut tokio::net::unix::OwnedWriteHalf,
    request: &ManagerRequest,
) -> Result<()> {
    let mut buf = Vec::new();
    request.encode(&mut buf)
        .context("Failed to encode ManagerRequest")?;
    let len = buf.len() as u32;
    write_half.write_all(&len.to_be_bytes()).await
        .context("Failed to write request length")?;
    write_half.write_all(&buf).await
        .context("Failed to write request content")?;
    tracing::debug!("已发送请求到 Worker: request_id={}, len={}", request.request_id, buf.len());
    Ok(())
}

/// 从 UnixStream 读端读取 WorkerResponse
///
/// 消息格式：[4字节长度 big-endian] + [protobuf 内容]
async fn read_response(
    read_half: &mut tokio::net::unix::OwnedReadHalf,
) -> Result<WorkerResponse> {
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

    let msg = WorkerResponse::decode(&msg_buf[..])
        .context("Failed to decode WorkerResponse")?;
    tracing::debug!("已接收 Worker 响应: request_id={}, len={}", msg.request_id, len);
    Ok(msg)
}

/// 清理所有 pending 等待者（连接断开时调用）
///
/// 设计说明：
/// - oneshot::Sender 被 drop 时，对应的 rx.await 返回 Err(RecvError)
/// - 调用方（send_request）统一将 RecvError 映射为 "IPC dispatcher dropped"
/// - 不传递具体错误原因，保持简单
fn cleanup_pending(pending: &Arc<DashMap<u64, oneshot::Sender<Result<WorkerResponse>>>>) {
    pending.clear();
}

/// dispatcher task 主循环
///
/// 职责：
/// 1. 从 request_rx 接收请求 → 注册 pending + 写入 UnixStream
/// 2. 从 UnixStream 读取响应 → 通过 request_id 路由到对应 oneshot
/// 3. 任何一方失败时清理所有 pending 等待者并退出
async fn run_dispatcher(
    mut request_rx: mpsc::Receiver<RequestItem>,
    stream: UnixStream,
    pending: Arc<DashMap<u64, oneshot::Sender<Result<WorkerResponse>>>>,
) {
    let (mut read_half, mut write_half) = stream.into_split();

    info!("IPC dispatcher 启动");

    loop {
        tokio::select! {
            // 新请求：注册 pending + 写入 stream
            Some(item) = request_rx.recv() => {
                let manager_request = ManagerRequest {
                    request_id: item.request_id,
                    payload: Some(item.payload),
                };
                pending.insert(item.request_id, item.response_tx);
                if let Err(e) = write_request(&mut write_half, &manager_request).await {
                    error!("IPC 写失败，清理所有等待者: {:?}", e);
                    cleanup_pending(&pending);
                    break;
                }
            }
            // Worker 响应：路由到对应 oneshot
            response = read_response(&mut read_half) => {
                match response {
                    Ok(resp) => {
                        if let Some((_, tx)) = pending.remove(&resp.request_id) {
                            let _ = tx.send(Ok(resp));
                        } else {
                            warn!("收到未知 request_id 的响应: {}", resp.request_id);
                        }
                    }
                    Err(e) => {
                        error!("IPC 读失败，清理所有等待者: {:?}", e);
                        cleanup_pending(&pending);
                        break;
                    }
                }
            }
        }
    }

    info!("IPC dispatcher 退出");
}
```

- [ ] **Step 2: 验证编译**

Run: `cd agent && cargo build --bin agent 2>&1 | head -40`
Expected: 仍有错误（IpcServer::new 等方法未更新），但新添加的函数本身应无错误。

- [ ] **Step 3: 提示用户提交**

提示用户执行：
```bash
git add agent/src/manager/ipc_server.rs
git commit -m "feat(ipc): 实现 dispatcher task（读写分离 + request_id 路由）"
```

---

## Task 5: 重写 IpcServer::new 和公共 API

**Files:**
- Modify: `agent/src/manager/ipc_server.rs`（IpcServer impl 块）

- [ ] **Step 1: 重写 IpcServer::new**

替换 `IpcServer::new` 方法（原第 132-152 行）：

```rust
impl IpcServer {
    /// 创建新的 IPC 服务器
    ///
    /// # 参数
    /// - `socket_path`: Unix Socket 路径
    /// - `pty_registry`: PTY 注册表
    /// - `worker_manager`: Worker 管理器
    /// - `channel_capacity`: mpsc channel 容量（默认 128）
    pub fn new(
        socket_path: String,
        pty_registry: Arc<PtyRegistry>,
        worker_manager: Arc<WorkerManager>,
        channel_capacity: usize,
    ) -> Self {
        Self {
            listener: Arc::new(RwLock::new(None)),
            socket_path,
            pty_registry,
            worker_manager,
            request_tx: tokio::sync::Mutex::new(None),
            pending: Arc::new(DashMap::new()),
            request_id_counter: AtomicU64::new(1),
            channel_capacity,
            dispatcher_handle: tokio::sync::Mutex::new(None),
        }
    }
```

- [ ] **Step 2: 保留 start/accept 方法不变**

`start` 和 `accept` 方法（原第 154-198 行）逻辑不变，保留原样。`accept` 仍由 `accept_and_set_pid` 调用。

- [ ] **Step 3: 重写 accept_and_set_pid**

替换 `accept_and_set_pid` 方法（原第 200-245 行）：

```rust
    /// 接收 Worker 连接并设置 worker_pid，启动 dispatcher
    ///
    /// 当 Worker 启动后自动调用。
    ///
    /// # 流程
    /// 1. accept 新连接
    /// 2. 创建 mpsc channel（channel_capacity）
    /// 3. 启动 dispatcher task
    /// 4. 存储 request_tx 供 send_request 使用
    /// 5. 清理旧 dispatcher（热更新场景）
    pub async fn accept_and_set_pid(&self, worker_pid: u32) -> Result<String> {
        let connection_id = self.accept().await?;

        // 取出 UnixStream（从 connections 临时存储中移除）
        // 注：accept 已将连接存入 connections，这里取出用于 dispatcher
        let stream = {
            let mut conns = self.connections_temp.write().await;
            conns.remove(&connection_id)
                .map(|c| c.stream)
                .flatten()
                .ok_or_else(|| anyhow::anyhow!("accept 后连接丢失: {}", connection_id))?
        };

        info!(
            "Worker 连接已建立并设置 PID: connection_id={}, worker_pid={}",
            connection_id, worker_pid
        );

        // 创建 mpsc channel
        let (request_tx, request_rx) = mpsc::channel::<RequestItem>(self.channel_capacity);

        // 启动 dispatcher task
        let pending = self.pending.clone();
        let handle = tokio::spawn(async move {
            run_dispatcher(request_rx, stream, pending).await;
        });

        // 存储 request_tx 和 dispatcher handle
        {
            let mut tx_guard = self.request_tx.lock().await;
            *tx_guard = Some(request_tx);
        }
        {
            let mut handle_guard = self.dispatcher_handle.lock().await;
            // 若有旧 dispatcher（热更新场景），先 abort
            if let Some(old) = handle_guard.take() {
                old.abort();
                info!("已清理旧 dispatcher（热更新）");
            }
            *handle_guard = Some(handle);
        }

        Ok(connection_id)
    }
```

- [ ] **Step 4: 添加 connections_temp 临时存储字段**

由于 `accept` 方法仍使用 `connections` 字段存储 IpcConnection，但新结构已移除该字段。简化方案：修改 `accept` 直接返回 `UnixStream`，而非存入 HashMap。

重写 `accept` 方法（原第 174-198 行）：

```rust
    /// 接收 Worker 连接
    ///
    /// # 返回
    /// - connection_id 和 UnixStream
    pub async fn accept(&self) -> Result<(String, UnixStream)> {
        let listener_guard = self.listener.read().await;

        if let Some(ref listener) = *listener_guard {
            let (stream, _addr) = listener.accept().await
                .context("Failed to accept connection")?;

            let connection_id = uuid::Uuid::new_v4().to_string();
            info!("Worker 连接已建立: connection_id={}", connection_id);

            Ok((connection_id, stream))
        } else {
            Err(anyhow::anyhow!("IPC server not started"))
        }
    }
```

更新 `accept_and_set_pid` 中的调用（替换 Step 3 中的取出逻辑）：

```rust
    pub async fn accept_and_set_pid(&self, worker_pid: u32) -> Result<String> {
        let (connection_id, stream) = self.accept().await?;

        info!(
            "Worker 连接已建立并设置 PID: connection_id={}, worker_pid={}",
            connection_id, worker_pid
        );

        // 创建 mpsc channel
        let (request_tx, request_rx) = mpsc::channel::<RequestItem>(self.channel_capacity);

        // 启动 dispatcher task
        let pending = self.pending.clone();
        let handle = tokio::spawn(async move {
            run_dispatcher(request_rx, stream, pending).await;
        });

        // 存储 request_tx 和 dispatcher handle
        {
            let mut tx_guard = self.request_tx.lock().await;
            *tx_guard = Some(request_tx);
        }
        {
            let mut handle_guard = self.dispatcher_handle.lock().await;
            if let Some(old) = handle_guard.take() {
                old.abort();
                info!("已清理旧 dispatcher（热更新）");
            }
            *handle_guard = Some(handle);
        }

        Ok(connection_id)
    }
```

- [ ] **Step 5: 重写 send_request 公共 API**

替换 `send_request` 方法（原第 355-424 行）：

```rust
    /// 发送通用请求到 Worker 并接收响应
    ///
    /// 并发安全：多个 QUIC task 可同时调用，无竞态。
    /// 通过 mpsc 提交请求到 dispatcher，dispatcher 串行化写入 UnixStream。
    /// 响应通过 request_id 路由回此调用的 oneshot。
    pub async fn send_request(
        &self,
        payload: manager_request::Payload,
    ) -> Result<WorkerResponse> {
        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);

        let (tx, rx) = oneshot::channel();

        // 提交请求到 dispatcher
        {
            let tx_guard = self.request_tx.lock().await;
            match tx_guard.as_ref() {
                Some(sender) => {
                    sender.send(RequestItem {
                        request_id,
                        payload,
                        response_tx: tx,
                    }).await
                    .map_err(|_| anyhow::anyhow!("IPC dispatcher dropped"))?;
                }
                None => {
                    return Err(anyhow::anyhow!("No active Worker connection"));
                }
            }
        }

        // 等待响应
        rx.await
            .map_err(|_| anyhow::anyhow!("IPC dispatcher dropped"))?
    }
```

- [ ] **Step 6: 重写 create_pty_session 公共 API**

替换 `create_pty_session` 方法（原第 264-353 行）：

```rust
    /// 发送 CreateSession 请求到 Worker 并连接 Session 进程
    ///
    /// 新架构流程:
    /// 1. 通过 send_request 发送 CreateSession 请求到 Worker
    /// 2. Worker 创建 Session 进程（openpty+fork）
    /// 3. Worker 返回 SessionCreated 响应（含 session_id 和 socket_name）
    /// 4. Manager 通过 SessionConnection 连接到 Session 进程
    /// 5. 注册到 PtyRegistry
    pub async fn create_pty_session(
        &self,
        request: crate::protocol::generated::CreateSession,
        user_info: UserInfo,
    ) -> Result<String> {
        // 通过通用 send_request 发送请求
        let response = self.send_request(manager_request::Payload::CreateSession(request)).await?;

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
```

- [ ] **Step 7: 删除旧的 cleanup_connection 和 cleanup_by_worker_pid**

删除 `cleanup_connection`（原第 426-438 行）和 `cleanup_by_worker_pid`（原第 440-460 行）方法——dispatcher 退出时自动清理 pending，不再需要显式连接清理。

- [ ] **Step 8: 重写 active_connection_count**

替换 `active_connection_count` 方法（原第 462-466 行）：

```rust
    /// 获取活动连接数（0 或 1，单 Worker 架构）
    pub async fn active_connection_count(&self) -> usize {
        let tx_guard = self.request_tx.lock().await;
        if tx_guard.is_some() { 1 } else { 0 }
    }
```

- [ ] **Step 9: 验证编译**

Run: `cd agent && cargo build --bin agent 2>&1 | head -40`
Expected: 仍有错误（run/stop 方法引用旧字段），但 send_request/create_pty_session 应无错误。

- [ ] **Step 10: 提示用户提交**

提示用户执行：
```bash
git add agent/src/manager/ipc_server.rs
git commit -m "feat(ipc): 重写 IpcServer 公共 API（send_request + create_pty_session 无锁并发）"
```

---

## Task 6: 修复 IpcServer::run 和 stop

**Files:**
- Modify: `agent/src/manager/ipc_server.rs`（run 和 stop 方法）

- [ ] **Step 1: 重写 run 方法**

替换 `run` 方法（原第 468-557 行）：

```rust
    /// 运行 IPC 服务器
    ///
    /// 监听 WorkerManager 状态变化事件：
    /// - Worker 启动时：接受连接并启动 dispatcher
    /// - Worker 崩溃时：清理 dispatcher（pending 自动清理）
    ///
    /// # 参数
    /// - `ready`: 可选的 oneshot sender，在 IPC bind + subscribe 完成后发送信号
    pub async fn run(&self, ready: Option<tokio::sync::oneshot::Sender<()>>) -> Result<()> {
        // 启动 IPC 监听（bind Unix Socket）
        self.start().await?;

        // 订阅 WorkerManager 事件
        let mut event_rx = self.worker_manager.subscribe();

        // 通知调用方 IPC 已就绪
        if let Some(tx) = ready {
            let _ = tx.send(());
        }

        info!("IPC 服务器开始监听 Worker 状态变化");

        // 处理 Worker 状态变化
        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Ok(worker_event) => {
                            match worker_event.status {
                                WorkerStatus::Starting => {
                                    info!(
                                        "检测到 Worker 启动事件，准备接受连接: pid={}",
                                        worker_event.pid
                                    );

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
                                    warn!(
                                        "检测到 Worker 崩溃事件，清理 dispatcher: pid={}",
                                        worker_event.pid
                                    );
                                    // abort dispatcher，pending 会被 dispatcher 退出时清理
                                    let mut handle_guard = self.dispatcher_handle.lock().await;
                                    if let Some(handle) = handle_guard.take() {
                                        handle.abort();
                                    }
                                    // 清理 request_tx
                                    let mut tx_guard = self.request_tx.lock().await;
                                    *tx_guard = None;
                                    // 清理 pending（dispatcher abort 可能未执行 cleanup_pending）
                                    cleanup_pending(&self.pending);
                                }

                                _ => {
                                    // 其他状态暂不处理
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Worker 事件通道错误: {}", e);
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
```

- [ ] **Step 2: 重写 stop 方法**

替换 `stop` 方法（原第 559-584 行）：

```rust
    /// 停止 IPC 服务器
    pub async fn stop(&self) -> Result<()> {
        // abort dispatcher
        {
            let mut handle_guard = self.dispatcher_handle.lock().await;
            if let Some(handle) = handle_guard.take() {
                handle.abort();
            }
        }

        // 清理 request_tx
        {
            let mut tx_guard = self.request_tx.lock().await;
            *tx_guard = None;
        }

        // 清理 pending
        cleanup_pending(&self.pending);

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
```

- [ ] **Step 3: 删除 Drop impl（若存在）**

检查文件末尾是否有 `impl Drop for IpcServer`，若有则删除——dispatcher 由 abort 清理，无需 Drop。

- [ ] **Step 4: 验证编译**

Run: `cd agent && cargo build --bin agent 2>&1 | head -40`
Expected: IpcServer 模块编译通过。可能仍有 Manager::new 调用签名不匹配的错误（下个任务修复）。

- [ ] **Step 5: 提示用户提交**

提示用户执行：
```bash
git add agent/src/manager/ipc_server.rs
git commit -m "feat(ipc): 重写 IpcServer::run/stop（dispatcher 生命周期管理）"
```

---

## Task 7: 更新 Manager::new 传递 channel 容量

**Files:**
- Modify: `agent/src/manager/mod.rs:115-144` (Manager::new)

- [ ] **Step 1: 更新 IpcServer::new 调用**

在 `agent/src/manager/mod.rs` 的 `Manager::new` 方法中，找到 IpcServer::new 调用（约第 127-131 行）：

```rust
        // 创建 IPC 服务器（集成 WorkerManager）
        let ipc_server = Arc::new(IpcServer::new(
            config.worker.ipc_socket_path.clone(),
            pty_registry.clone(),
            worker_manager.clone(),
        ));
```

替换为：

```rust
        // 创建 IPC 服务器（集成 WorkerManager，传递 channel 容量）
        let ipc_server = Arc::new(IpcServer::new(
            config.worker.ipc_socket_path.clone(),
            pty_registry.clone(),
            worker_manager.clone(),
            config.worker.ipc_channel_capacity,
        ));
```

- [ ] **Step 2: 验证编译**

Run: `cd agent && cargo build --bin agent 2>&1 | head -20`
Expected: Manager 侧编译通过。Worker 侧仍有错误（下个任务修复）。

- [ ] **Step 3: 提示用户提交**

提示用户执行：
```bash
git add agent/src/manager/mod.rs
git commit -m "feat(manager): 传递 ipc_channel_capacity 到 IpcServer"
```

---

## Task 8: 重写 Worker run() 为并发 dispatch

**Files:**
- Modify: `agent/src/worker/mod.rs:1-224`（run 和 handle_request）

- [ ] **Step 1: 更新 imports**

在 `agent/src/worker/mod.rs` 顶部 imports 中添加 `Arc` 和 `mpsc`（若未存在）：

```rust
use std::sync::Arc;
use anyhow::Result;
use tokio::sync::{mpsc, Notify};
use crate::protocol::generated::{ManagerRequest, WorkerResponse, worker_response, Error};
```

- [ ] **Step 2: 重写 run 函数**

替换整个 `run` 函数（原第 53-131 行）：

```rust
pub async fn run(mut ipc_client: IpcClient) -> Result<()> {
    tracing::info!("Worker 消息处理循环启动（并发 dispatch 模型）");

    // 共享状态（Arc 包装，供 spawned task 使用）
    let pty_factory = Arc::new(PtyFactory::new());
    let session_manager = SessionManager::new();  // 内部已是 Arc<RwLock>

    // 启动子进程监控任务
    let session_manager_clone = session_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = session_manager_clone.monitor_child_processes().await {
            tracing::error!("子进程监控任务异常退出: {}", e);
        }
    });

    // shutdown 信号通道
    let shutdown_notify = Arc::new(Notify::new());

    // 响应收集通道（与 Manager 侧 channel 容量一致，默认 128）
    let (response_tx, mut response_rx) = mpsc::channel::<WorkerResponse>(128);

    loop {
        tokio::select! {
            // 收到 shutdown 信号
            _ = shutdown_notify.notified() => {
                tracing::info!("收到 shutdown 信号，Worker 主循环退出");
                break;
            }
            // 接收 Manager 请求 → spawn 独立处理（并发）
            request_result = ipc_client.receive_request() => {
                match request_result {
                    Ok(request) => {
                        tracing::debug!("收到 Manager 请求: request_id={}", request.request_id);

                        let tx = response_tx.clone();
                        let (pty, sm, shutdown) = (
                            pty_factory.clone(),
                            session_manager.clone(),
                            shutdown_notify.clone(),
                        );
                        tokio::spawn(async move {
                            let response = handle_request(
                                request, &pty, &sm, &shutdown,
                            ).await;
                            if let Err(e) = tx.send(response).await {
                                tracing::error!("发送响应到 channel 失败: {:?}", e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("接收消息失败: {}", e);
                        break;
                    }
                }
            }
            // 处理完成的响应 → 串行化发回 Manager
            Some(response) = response_rx.recv() => {
                tracing::debug!(
                    "准备发送响应到 Manager: request_id={}, payload_type={}",
                    response.request_id,
                    response.payload.as_ref().map(|p| match p {
                        worker_response::Payload::DirListing(_) => "DirListing",
                        worker_response::Payload::FileContent(_) => "FileContent",
                        worker_response::Payload::WriteResult(_) => "WriteResult",
                        worker_response::Payload::Error(_) => "Error",
                        worker_response::Payload::SessionCreated(_) => "SessionCreated",
                        _ => "Other",
                    }).unwrap_or("None")
                );
                if let Err(e) = ipc_client.send_response(&response).await {
                    tracing::error!(
                        "发送响应失败: request_id={}, error={:?}",
                        response.request_id, e
                    );
                    break;
                }
                tracing::debug!("响应发送成功: request_id={}", response.request_id);
            }
        }
    }

    tracing::info!("Worker 消息处理循环结束");

    Ok(())
}
```

- [ ] **Step 3: 更新 handle_request 签名**

替换 `handle_request` 函数签名（原第 152-156 行）：

```rust
async fn handle_request(
    request: ManagerRequest,
    pty_factory: &Arc<PtyFactory>,
    session_manager: &SessionManager,
    shutdown_notify: &Arc<Notify>,
) -> WorkerResponse {
```

函数体不变（原第 158-223 行），但需要确认 `handlers::session::handle_create_session` 等调用的签名——它们接收 `&PtyFactory`，而现在是 `&Arc<PtyFactory>`。需要在调用处解引用：

```rust
        Some(crate::protocol::generated::manager_request::Payload::CreateSession(req)) => {
            handlers::session::handle_create_session(&pty_factory, session_manager, req).await
        }
```

注：`&Arc<PtyFactory>` 自动 deref 为 `&PtyFactory`（Rust 自动解引用），无需显式 `&**pty_factory`。确认编译器接受。若不接受，改为 `pty_factory.as_ref()` 或 `&**pty_factory`。

- [ ] **Step 4: 验证编译**

Run: `cd agent && cargo build --bin agent 2>&1 | head -40`
Expected: Worker 侧编译通过。可能仍有 handler 签名不匹配的错误（若 handler 接收 `&PtyFactory` 而 `&Arc<PtyFactory>` 无法自动解引用）。

- [ ] **Step 5: 提示用户提交**

提示用户执行：
```bash
git add agent/src/worker/mod.rs
git commit -m "feat(worker): 重写 run() 为并发 dispatch 模型（select! + spawn）"
```

---

## Task 9: 包装阻塞操作 spawn_blocking（session.rs）

**Files:**
- Modify: `agent/src/worker/handlers/session.rs:37-104` (handle_create_session)

- [ ] **Step 1: 包装 PtyFactory::create 调用**

`PtyFactory::create` 内部调用 `create_session`（openpty + fork），是阻塞操作。在 `agent/src/worker/handlers/session.rs` 的 `handle_create_session` 中，找到 `pty_factory.create(...)` 调用（约第 69 行）：

```rust
    match pty_factory.create(&req.shell, req.cols, req.rows, cwd, user_context.as_ref()) {
```

替换为 `spawn_blocking` 包装：

```rust
    // PtyFactory::create 内部调用 openpty + fork，是阻塞操作
    // 使用 spawn_blocking 避免占用 tokio async worker thread
    let pty_factory_clone = pty_factory.clone();
    let shell = req.shell.clone();
    let cols = req.cols;
    let rows = req.rows;
    let cwd_str = cwd.map(|s| s.to_string());
    let user_ctx = user_context.as_ref().map(|u| u.clone());
    let create_result = tokio::task::spawn_blocking(move || {
        pty_factory_clone.create(&shell, cols, rows, cwd_str.as_deref(), user_ctx.as_ref())
    }).await
    .context("spawn_blocking for PtyFactory::create failed")?;

    match create_result {
```

注：`PtyFactory` 需要实现 `Send + Sync`。由于 `PtyFactory` 是空结构体（`pub struct PtyFactory;`），Rust 自动派生 `Send + Sync`。`Arc<PtyFactory>` 满足 `Send + Sync`。

- [ ] **Step 2: 验证编译**

Run: `cd agent && cargo build --bin agent 2>&1 | head -20`
Expected: session.rs 编译通过。

- [ ] **Step 3: 提示用户提交**

提示用户执行：
```bash
git add agent/src/worker/handlers/session.rs
git commit -m "perf(worker): handle_create_session 使用 spawn_blocking 包装 fork"
```

---

## Task 10: 包装阻塞操作 spawn_blocking（command.rs）

**Files:**
- Modify: `agent/src/worker/handlers/command.rs:60-164` (handle_execute_command)

- [ ] **Step 1: 包装 execute_as_user 调用**

`UserExecutor::execute_as_user` 内部调用 `fork + std::process::Command`，是阻塞操作。在 `agent/src/worker/handlers/command.rs` 的 `handle_execute_command` 中，找到 `executor.execute_as_user(...)` 调用（约第 91 行）：

```rust
    let result = executor.execute_as_user(move || {
        let mut cmd = std::process::Command::new(&command);
        // ...
    });
```

替换为 `spawn_blocking` 包装：

```rust
    // execute_as_user 内部 fork + std::process::Command，是阻塞操作
    // 使用 spawn_blocking 避免占用 tokio async worker thread
    let result = tokio::task::spawn_blocking(move || {
        executor.execute_as_user(move || {
            let mut cmd = std::process::Command::new(&command);

            if !args.is_empty() {
                cmd.args(&args);
            }

            if !working_directory.is_empty() {
                cmd.current_dir(&working_directory);
            }

            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());

            let output = cmd.output()?;

            Ok(CommandResult {
                stdout: output.stdout,
                stderr: output.stderr,
                exit_code: output.status.code().unwrap_or(-1),
            })
        })
    }).await
    .context("spawn_blocking for execute_as_user failed")?;
```

注：`UserExecutor` 是 `Clone`，内部含 `u32`/`PathBuf`，满足 `Send + 'static`。

- [ ] **Step 2: 验证编译**

Run: `cd agent && cargo build --bin agent 2>&1 | head -20`
Expected: command.rs 编译通过。

- [ ] **Step 3: 提示用户提交**

提示用户执行：
```bash
git add agent/src/worker/handlers/command.rs
git commit -m "perf(worker): handle_execute_command 使用 spawn_blocking 包装 fork+exec"
```

---

## Task 11: 评估 file.rs 的 spawn_blocking 需求

**Files:**
- Modify: `agent/src/worker/handlers/file.rs`（10 个 handler）

- [ ] **Step 1: 分析 file.rs 中的 execute_as_user 调用**

`file.rs` 中 10 个 handler 都通过 `executor.execute_as_user(...)` 执行阻塞 I/O。每个调用的闭包内部是同步文件 I/O（`std::fs::read_dir`、`std::fs::read`、`std::fs::write` 等）。

策略：为简化改动，统一在 `execute_as_user` 调用处包装 `spawn_blocking`。

- [ ] **Step 2: 选择代表性 handler 验证模式**

先对 `handle_read_dir`（第 63-173 行）做改造，找到 `executor.execute_as_user(...)`（约第 86 行）：

```rust
    let result: AnyhowResult<Vec<DirEntry>> = executor.execute_as_user(move || {
        // ... 同步 I/O
    });
```

替换为：

```rust
    let result: AnyhowResult<Vec<DirEntry>> = tokio::task::spawn_blocking(move || {
        executor.execute_as_user(move || {
            // ... 同步 I/O（原逻辑不变）
        })
    }).await
    .context("spawn_blocking for handle_read_dir failed")?;
```

- [ ] **Step 3: 对其余 9 个 handler 应用相同模式**

对以下 handler 重复 Step 2 的模式（在 `executor.execute_as_user(...)` 外层包装 `spawn_blocking`）：

1. `handle_read_file` (line 174) - `executor.execute_as_user` at ~line 196
2. `handle_write_file` (line 263) - at ~line 286
3. `handle_delete` (line 351) - at ~line 370
4. `handle_mkdir` (line 407) - at ~line 426
5. `handle_rename` (line 461) - at ~line 492
6. `handle_copy` (line 530) - at ~line 561
7. `handle_move` (line 611) - at ~line 680
8. `handle_file_exists` (line 661) - at ~line 768
9. `handle_apply_diff` (line 734) - at ~line 768

每个 handler 的改造模式一致：
- 将 `executor.execute_as_user(...)` 外层包装 `tokio::task::spawn_blocking(move || { ... }).await.context("...")?;`
- 注意闭包捕获的变量需要 `move`（已是 `move ||`）
- `executor` 是 `Clone`，可直接 move 到 spawn_blocking 闭包

- [ ] **Step 4: 验证编译**

Run: `cd agent && cargo build --bin agent 2>&1 | head -30`
Expected: 全部编译通过。

- [ ] **Step 5: 提示用户提交**

提示用户执行：
```bash
git add agent/src/worker/handlers/file.rs
git commit -m "perf(worker): file handlers 使用 spawn_blocking 包装 execute_as_user"
```

---

## Task 12: 添加 main.rs 的 --ipc-channel-capacity 参数

**Files:**
- Modify: `agent/src/main.rs:20-36` (Args 结构)
- Modify: `agent/src/main.rs:182-202` (run_worker_mode)

- [ ] **Step 1: 添加 CLI 参数**

在 `agent/src/main.rs` 的 `Args` 结构中添加：

```rust
    /// IPC Socket 路径（Worker 模式必须指定）
    #[arg(long, hide = true)]
    ipc_socket: Option<String>,

    /// IPC channel 容量（Worker 模式，由 Manager 启动时传递）
    #[arg(long, hide = true, default_value = "128")]
    ipc_channel_capacity: usize,
```

- [ ] **Step 2: 传递参数到 Worker run()**

修改 `run_worker_mode`（原第 182-202 行）：

```rust
async fn run_worker_mode(args: &Args) -> Result<()> {
    let ipc_socket_path = args.ipc_socket.clone()
        .ok_or_else(|| anyhow::anyhow!("Worker 模式必须指定 --ipc-socket 参数"))?;

    // 初始化简单的日志（Worker 模式使用简化日志）
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!(
        "Worker 模式启动，连接到: {}, channel_capacity: {}",
        ipc_socket_path, args.ipc_channel_capacity
    );

    // 初始化 IpcClient 并连接到 Manager
    let ipc_client = gnome_remote_agent::worker::IpcClient::connect(&ipc_socket_path).await?;
    gnome_remote_agent::worker::run(ipc_client).await?;

    tracing::info!("Worker 进程已退出");

    Ok(())
}
```

注：当前 `run()` 签名未接收 `channel_capacity`，内部硬编码 128。若要让 Worker 侧使用配置值，需更新 `run()` 签名为 `pub async fn run(mut ipc_client: IpcClient, channel_capacity: usize) -> Result<()>`。

- [ ] **Step 3: 更新 WorkerManager::start 传递参数**

在 `agent/src/manager/worker_manager.rs` 的 `start` 方法中，找到 `Command::new(...)` 调用（约第 183 行）：

```rust
        let child = Command::new(&worker_binary)
            .arg("--worker")
            .arg("--ipc-socket")
            .arg(&self.ipc_socket_path)
            .spawn()
```

需要传递 `--ipc-channel-capacity` 参数。但 `WorkerManager` 当前不持有 `channel_capacity`。

简化方案：Worker 侧 `run()` 内部仍硬编码 128（与默认值一致），不传递参数。若用户配置非默认值，需要 Manager 在 spawn 时传递。

**本计划采用简化方案**：Worker 侧 `run()` 保持硬编码 128，与 `default_ipc_channel_capacity` 一致。配置项主要影响 Manager 侧的 `request_tx` channel 容量。Worker 侧的 `response_tx` 容量使用默认 128。

因此 Step 1 和 Step 2 的改动可省略（`--ipc-channel-capacity` 参数暂不传递给 Worker）。回滚 Step 1 的改动，仅保留 Manager 侧使用 `config.worker.ipc_channel_capacity`。

- [ ] **Step 4: 验证编译**

Run: `cd agent && cargo build --bin agent 2>&1 | head -10`
Expected: 编译通过。

- [ ] **Step 5: 提示用户提交**

提示用户执行：
```bash
git add agent/src/main.rs
git commit -m "feat(cli): 添加 --ipc-channel-capacity 参数（预留，Worker 侧暂用默认值）"
```

---

## Task 13: 并发回归测试

**Files:**
- Create: `agent/tests/concurrent_ipc_test.rs`

- [ ] **Step 1: 创建测试文件**

创建 `agent/tests/concurrent_ipc_test.rs`：

```rust
//! 并发 IPC 回归测试
//!
//! 验证多个并发请求不会因竞态导致 "No active Worker connection" 错误。
//! 这是原 bug 的核心回归测试。

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Barrier;

use gnome_remote_agent::manager::ipc_server::IpcServer;
use gnome_remote_agent::manager::pty_registry::PtyRegistry;
use gnome_remote_agent::manager::worker_manager::WorkerManager;
use gnome_remote_agent::protocol::generated::{manager_request, ReadDir};

/// 辅助：创建 mock Worker，监听 Unix Socket 并响应请求
async fn spawn_mock_worker(socket_path: &str, barrier: Arc<Barrier>) {
    use tokio::net::UnixListener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use prost::Message;

    // 删除旧 socket
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path).unwrap();
    barrier.wait().await;

    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();

        // 持续读取请求并返回 mock 响应
        loop {
            let mut len_buf = [0u8; 4];
            if stream.read_exact(&mut len_buf).await.is_err() {
                break;
            }
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut msg_buf = vec![0u8; len];
            if stream.read_exact(&mut msg_buf).await.is_err() {
                break;
            }

            let request = gnome_remote_agent::protocol::generated::ManagerRequest::decode(&msg_buf[..]).unwrap();

            // 构造 mock 响应（Error，测试只需验证路由正确）
            let response = gnome_remote_agent::protocol::generated::WorkerResponse {
                request_id: request.request_id,
                payload: Some(gnome_remote_agent::protocol::generated::worker_response::Payload::Error(
                    gnome_remote_agent::protocol::generated::Error {
                        code: 0,
                        message: "mock".to_string(),
                    }
                )),
            };

            let mut resp_buf = Vec::new();
            response.encode(&mut resp_buf).unwrap();
            let resp_len = resp_buf.len() as u32;
            stream.write_all(&resp_len.to_be_bytes()).await.unwrap();
            stream.write_all(&resp_buf).await.unwrap();
        }
    });
}

#[tokio::test]
async fn test_concurrent_send_request_no_race() {
    let socket_path = "/tmp/test-concurrent-ipc.sock";
    let pty_registry = Arc::new(PtyRegistry::new());
    let worker_manager = Arc::new(WorkerManager::new(
        "/bin/true".to_string(),
        socket_path.to_string(),
        3,
    ));
    let ipc_server = Arc::new(IpcServer::new(
        socket_path.to_string(),
        pty_registry,
        worker_manager,
        128,
    ));

    // 启动 IPC 服务器
    ipc_server.start().await.unwrap();

    // 启动 mock Worker
    let barrier = Arc::new(Barrier::new(2));
    spawn_mock_worker(socket_path, barrier.clone()).await;
    barrier.wait().await;

    // 模拟 Worker 连接（绕过 WorkerManager::start）
    tokio::time::sleep(Duration::from_millis(100)).await;
    // 手动触发 accept_and_set_pid
    ipc_server.accept_and_set_pid(12345).await.unwrap();

    // 并发发起 10 个请求
    let mut handles = vec![];
    for i in 0..10 {
        let server = ipc_server.clone();
        handles.push(tokio::spawn(async move {
            let payload = manager_request::Payload::ReadDir(ReadDir {
                path: format!("/tmp/{}", i),
                uid: 0,
                gid: 0,
                username: String::new(),
                home_dir: String::new(),
            });
            server.send_request(payload).await
        }));
    }

    // 全部应成功（原实现会因竞态部分失败）
    let mut success_count = 0;
    for handle in handles {
        match handle.await.unwrap() {
            Ok(_) => success_count += 1,
            Err(e) => tracing::error!("请求失败: {:?}", e),
        }
    }

    assert_eq!(success_count, 10, "所有并发请求都应成功，但有部分失败");

    // 清理
    ipc_server.stop().await.unwrap();
}
```

- [ ] **Step 2: 运行测试**

Run: `cd agent && cargo test --test concurrent_ipc_test -- --nocapture`
Expected: 测试通过，10 个并发请求全部成功。

- [ ] **Step 3: 提示用户提交**

提示用户执行：
```bash
git add agent/tests/concurrent_ipc_test.rs
git commit -m "test(ipc): 添加并发回归测试（10 并发请求无竞态）"
```

---

## Task 14: 最终编译验证与零警告检查

**Files:**
- 无（仅验证）

- [ ] **Step 1: 完整编译**

Run: `cd agent && cargo build --bin agent 2>&1`
Expected: 编译通过，零错误。

- [ ] **Step 2: 警告检查**

Run: `cd agent && cargo build --bin agent 2>&1 | grep -i warning`
Expected: 允许 future features 的临时警告，但不应有未使用导入/变量警告。

若有未使用导入警告（如 `use std::collections::HashMap` 在 ipc_server.rs 中不再使用），删除对应 import。

- [ ] **Step 3: 运行所有测试**

Run: `cd agent && cargo test 2>&1 | tail -20`
Expected: 现有测试不回归，新测试通过。

- [ ] **Step 4: 提示用户提交**

提示用户执行：
```bash
git add -A
git commit -m "chore: 清理未使用导入，零警告编译通过"
```

---

## Self-Review

### Spec 覆盖检查

| Spec 章节 | 覆盖任务 |
|---|---|
| 3.2 关键设计决策 | Task 3-6（mpsc + oneshot + DashMap + into_split） |
| 4. Manager 侧 IpcServer 重构 | Task 3-7 |
| 4.1 结构变化 | Task 3 |
| 4.2 公共 API | Task 5 |
| 4.3 dispatcher task | Task 4 |
| 4.4 连接生命周期 | Task 5（accept_and_set_pid） |
| 4.5 热更新兼容 | Task 6（run 中 Crashed 事件清理 dispatcher） |
| 5. Worker 侧并发 dispatch | Task 8 |
| 5.1 run() 重写 | Task 8 |
| 5.2 handle_request 签名 | Task 8 |
| 5.3 共享状态分析 | Task 8（Arc 包装） |
| 5.4 阻塞操作处理 | Task 9-11 |
| 6. 错误处理 | Task 4（cleanup_pending）、Task 6（Crashed 事件） |
| 7. 配置项 | Task 2 |
| 9. 测试策略 | Task 13 |

### Placeholder 扫描

无 TBD/TODO。所有代码块完整。

### Type 一致性

- `RequestItem` 在 Task 3 定义，Task 4/5 使用 ✓
- `run_dispatcher` 在 Task 4 定义，Task 5 调用 ✓
- `cleanup_pending` 在 Task 4 定义，Task 6 调用 ✓
- `IpcServer::new` 签名在 Task 5 定义，Task 7 调用 ✓
- `handle_request` 签名在 Task 8 更新为 `&Arc<PtyFactory>` ✓

### 已知简化

- Worker 侧 `response_tx` channel 容量硬编码 128（Task 12 说明）
- panic 防护的 `catch_unwind` 未实现（spec 6.1 的备选方案：依赖 `tokio::spawn` 默认隔离）
- `handle_graceful_shutdown` 的 `all_idle()` 仍返回 true（原逻辑不变）
