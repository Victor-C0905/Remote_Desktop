# 并发 IPC 模型设计

## 1. 背景与问题

### 1.1 问题描述

打开终端时有一定概率触发错误：

```
❌ 初始化失败: 创建终端会话失败: No active Worker connection
```

该问题为旧问题，在并发请求场景下间歇性出现。

### 1.2 根本原因：连接取出-放回竞态

当前 `IpcServer` 维护单一 Worker 连接，采用 "取出 → await → 放回" 模式访问：

```rust
// ipc_server.rs:278-311
let mut connection = {
    let mut connections = self.connections.write().await;
    connection_id = connections.keys().next().cloned()
        .ok_or_else(|| anyhow!("No active Worker connection"))?;  // ← 竞态点
    connections.remove(&connection_id)
};
// 漫长的 await（发送请求 + 接收响应）
connection.send_request(...).await;
connection.receive_response().await;
// 放回连接
connections.insert(connection_id, connection);
```

并发请求场景下存在竞态窗口：

```
T0: connections = {conn_A}
T1: 请求1 取出 conn_A → connections = {}（空）
T2: 请求2 尝试取出 → keys().next() = None → "No active Worker connection"
T3: 请求1 完成，放回 conn_A
```

终端创建是耗时操作（openpty + fork + Session 进程 bind abstract socket + Hello 校验），`await` 时间长，竞态窗口大；前端打开终端时常同时发起多个请求（创建会话 + 列目录 + 读配置），竞态触发概率高。

### 1.3 次要问题：Worker 串行瓶颈

Worker 当前是 `receive → handle → send` 串行循环（[worker/mod.rs:75-126](../../../agent/src/worker/mod.rs#L75)），使用 `&mut ipc_client`，无法在处理一个请求时接收下一个。单个慢请求（大文件 I/O、exec 命令、Session 创建）阻塞所有后续请求。

### 1.4 方案选择过程

已评估三个方案：

| 方案 | 描述 | 结论 |
|---|---|---|
| A. 多 Worker 进程 | Manager 启动 N 个 Worker，进程池调度 | 否决：引入调度器职责违反单一职责，N 倍内存，路由复杂度 |
| B. 单 Worker + 并发 dispatch | Worker 用 tokio::spawn 并发处理，Manager 用 mpsc + request_id 路由 | **采用** |
| C. 单 Worker + 多 IPC 连接 | Worker 仍串行，仅多连接 | 否决：伪解决方案，无并行收益 |

方案 B 选择理由：
- 符合 Rust + Tokio 生态标准做法（tokio runtime 提供并发，对应 GMainLoop 角色）
- Worker 保持纯业务逻辑，并发由 runtime 提供（单一职责）
- Manager 只做路由（网关固有职责），不引入调度器
- 单进程，无路由复杂度（解耦、开箱即用）
- tokio multi-thread runtime 已跨核并行（项目已启用 `rt-multi-thread`），性能不劣于多进程方案

## 2. 设计目标

1. **消除竞态**：并发请求不再因连接取出-放回失败
2. **并发处理**：Worker 并发 dispatch 请求，慢请求不阻塞快请求
3. **符合架构原则**：业务逻辑与底层实现分离、单一职责、解耦、开箱即用
4. **零 Worker 业务改动**：handler 逻辑不变，仅共享状态包装调整
5. **故障兼容**：热更新、Worker 崩溃场景下的正确错误处理

## 3. 架构设计

### 3.1 核心模型：消息驱动 + request_id 路由

从"连接池 + 取出-放回"改为"mpsc + oneshot + request_id 路由"的消息驱动模型。

**请求路径**：
```
QUIC Task → oneshot::channel + request_tx.send() → dispatcher → write_half → Worker recv → tokio::spawn(handle)
```

**响应路径**：
```
Worker spawn done → response_tx.send() → Worker send_response → read_half → pending[id] → oneshot → QUIC Task
```

### 3.2 关键设计决策

| 决策点 | 选择 | 理由 |
|---|---|---|
| 并发 dispatch 机制 | spawn + mpsc 收集响应（B2） | 无锁、背压自然、复杂度适中 |
| UnixStream 访问 | `into_split()` 拆分 read/write | 读端独立 task 路由响应，写端串行化 |
| channel 容量 | 可配置，默认 128 | 开箱即用，极端场景可调 |
| 热更新在途请求 | 立即失败 | 简单可预测，客户端可重试 |
| pending 表 | `Arc<DashMap<u64, oneshot::Sender>>` | 并发读写无锁 |

## 4. Manager 侧 IpcServer 重构

### 4.1 结构变化

```rust
// 当前（问题根源）
pub struct IpcServer {
    listener: Arc<RwLock<Option<UnixListener>>>,
    socket_path: String,
    pty_registry: Arc<PtyRegistry>,
    worker_manager: Arc<WorkerManager>,
    connections: Arc<RwLock<HashMap<String, IpcConnection>>>,  // ← 竞态根源
    request_id_counter: AtomicU64,
}

// 新设计
pub struct IpcServer {
    listener: Arc<RwLock<Option<UnixListener>>>,
    socket_path: String,
    pty_registry: Arc<PtyRegistry>,
    worker_manager: Arc<WorkerManager>,

    /// 请求通道：多个 QUIC task → dispatcher（无锁提交）
    request_tx: mpsc::Sender<RequestItem>,

    /// 响应路由表：dispatcher → 对应 QUIC task
    pending: Arc<DashMap<u64, oneshot::Sender<Result<WorkerResponse>>>>,

    /// channel 容量（可配置，默认 128）
    channel_capacity: usize,

    request_id_counter: AtomicU64,

    /// dispatcher task 句柄（用于停机）
    dispatcher_handle: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

/// 请求项（通过 mpsc 提交给 dispatcher）
struct RequestItem {
    request_id: u64,
    payload: manager_request::Payload,
    response_tx: oneshot::Sender<Result<WorkerResponse>>,
}
```

### 4.2 公共 API（调用方无感知）

```rust
impl IpcServer {
    /// 发送通用请求到 Worker 并接收响应
    pub async fn send_request(
        &self,
        payload: manager_request::Payload,
    ) -> Result<WorkerResponse> {
        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.request_tx.send(RequestItem {
            request_id,
            payload,
            response_tx: tx,
        }).await.map_err(|_| anyhow!("IPC dispatcher dropped"))?;
        rx.await.map_err(|_| anyhow!("IPC dispatcher dropped"))?
    }

    /// 创建 PTY 会话
    pub async fn create_pty_session(
        &self,
        request: CreateSession,
        user_info: UserInfo,
    ) -> Result<String> {
        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let payload = manager_request::Payload::CreateSession(request);
        self.request_tx.send(RequestItem {
            request_id,
            payload,
            response_tx: tx,
        }).await.map_err(|_| anyhow!("IPC dispatcher dropped"))?;

        let response = rx.await
            .map_err(|_| anyhow!("IPC dispatcher dropped"))??;

        // 解析响应，连接 Session 进程（逻辑不变）
        match response.payload {
            Some(worker_response::Payload::SessionCreated(session_created)) => {
                // ... 原有 SessionConnection::connect + PtyRegistry::register 逻辑不变
            }
            Some(worker_response::Payload::Error(err)) => {
                Err(anyhow!("Worker error: code={}, message={}", err.code, err.message))
            }
            _ => Err(anyhow!("Unexpected response from Worker")),
        }
    }
}
```

### 4.3 dispatcher task

Worker 连接建立后（`accept_and_set_pid`），启动 dispatcher task：

```rust
async fn run_dispatcher(
    mut request_rx: mpsc::Receiver<RequestItem>,
    stream: UnixStream,
    pending: Arc<DashMap<u64, oneshot::Sender<Result<WorkerResponse>>>>,
) {
    let (mut read_half, mut write_half) = stream.into_split();

    loop {
        tokio::select! {
            // 新请求：注册 pending + 写入 stream
            Some(item) = request_rx.recv() => {
                pending.insert(item.request_id, item.response_tx);
                let manager_request = ManagerRequest {
                    request_id: item.request_id,
                    payload: Some(item.payload),
                };
                if let Err(e) = write_request(&mut write_half, &manager_request).await {
                    tracing::error!("IPC 写失败，清理所有等待者: {:?}", e);
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
                            tracing::warn!("收到未知 request_id 的响应: {}", resp.request_id);
                        }
                    }
                    Err(e) => {
                        tracing::error!("IPC 读失败，清理所有等待者: {:?}", e);
                        cleanup_pending(&pending);
                        break;
                    }
                }
            }
        }
    }
}

/// 清理所有 pending 等待者（连接断开时调用）
///
/// 设计说明：
/// - oneshot::Sender 被 drop 时，对应的 rx.await 返回 Err(RecvError)
/// - 调用方（send_request）统一将 RecvError 映射为 "IPC dispatcher dropped"
/// - 不传递具体错误原因（如 EOF/write error），保持简单
/// - 若未来需区分错误原因，可改用 mpsc 替代 oneshot（代价是管理 mpsc 生命周期）
fn cleanup_pending(pending: &Arc<DashMap<u64, oneshot::Sender<Result<WorkerResponse>>>>) {
    pending.clear();
}
```

### 4.4 连接生命周期

`accept_and_set_pid` 仍由 `run()` 中的 Worker 事件循环调用。建立连接后：

1. 创建 `mpsc::channel(channel_capacity)`，保留 `request_tx`
2. 启动 dispatcher task，传入 `request_rx` + `UnixStream` + `pending`
3. 旧 dispatcher（若有，热更新场景）通过 drop `request_tx` 自然终止

### 4.5 热更新兼容

`HotUpdateCoordinator` 的 `stop → start` 流程不变。热更新期间：

- 旧 Worker 退出 → dispatcher 读端 EOF → `cleanup_pending` 清理所有等待者
- 等待中的 QUIC task 收到 `Err`（oneshot sender dropped）→ 返回 Error 给客户端
- 新 Worker 启动 → 新连接 → 新 dispatcher → 后续请求正常

配置在途请求策略为"立即失败"：客户端收到 Error 后可重试（新 Worker 已就绪）。

## 5. Worker 侧并发 dispatch

### 5.1 run() 重写

```rust
pub async fn run(mut ipc_client: IpcClient) -> Result<()> {
    tracing::info!("Worker 消息处理循环启动（并发 dispatch 模型）");

    let pty_factory = Arc::new(PtyFactory::new());
    let session_manager = SessionManager::new();  // 内部已是 Arc<RwLock>

    // 子进程监控任务（逻辑不变）
    let session_manager_clone = session_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = session_manager_clone.monitor_child_processes().await {
            tracing::error!("子进程监控任务异常退出: {}", e);
        }
    });

    let shutdown_notify = Arc::new(Notify::new());

    // 响应收集通道（容量由 Manager 启动时通过 --ipc-channel-capacity 传递）
    let (response_tx, mut response_rx) = mpsc::channel(channel_capacity);

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
                            // channel 满时 send().await 自动背压
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
                if let Err(e) = ipc_client.send_response(&response).await {
                    tracing::error!("发送响应失败: request_id={}, error={:?}",
                        response.request_id, e);
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

### 5.2 handle_request 签名调整

`handle_request` 现接收 `Arc` 共享状态而非 `&`：

```rust
// 当前签名
async fn handle_request(
    request: ManagerRequest,
    pty_factory: &PtyFactory,
    session_manager: &SessionManager,
    shutdown_notify: &Arc<Notify>,
) -> WorkerResponse

// 新签名
async fn handle_request(
    request: ManagerRequest,
    pty_factory: &Arc<PtyFactory>,        // PtyFactory 是空结构体，Send+Sync
    session_manager: &SessionManager,      // 内部 Arc<RwLock>，已 Send+Sync
    shutdown_notify: &Arc<Notify>,
) -> WorkerResponse
```

各 handler 签名同步调整。handler 业务逻辑不变。

### 5.3 共享状态分析

| 组件 | 当前类型 | 并发安全性 | 改动 |
|---|---|---|---|
| `PtyFactory` | `pub struct PtyFactory;`（空结构体） | Send+Sync | 包装为 `Arc<PtyFactory>`，无锁 |
| `SessionManager` | 内部 `Arc<RwLock<HashMap>>` | Send+Sync | 零改动，clone Arc |
| `shutdown_notify` | `Arc<Notify>` | Send+Sync | clone Arc |
| `IpcClient` | 持有 `UnixStream` | 仅主循环持有 `&mut` | 不共享，select! 中串行访问 |

### 5.4 阻塞操作处理

以下 handler 含阻塞操作，需用 `spawn_blocking` 包装以避免占用 async worker thread：

| Handler | 阻塞操作 | 处理方式 |
|---|---|---|
| `handle_create_session` | `PtyFactory::create()`（openpty + fork） | `tokio::task::spawn_blocking` |
| `handle_read_file` | 大文件磁盘 I/O | `tokio::fs` 异步 API 或 `spawn_blocking` |
| `handle_write_file` | 大文件磁盘 I/O | `tokio::fs` 异步 API 或 `spawn_blocking` |
| `handle_execute_command` | `exec` 子进程 | `tokio::process::Command` 异步 API |
| `handle_apply_diff` | CPU 密集 diff 计算 | `spawn_blocking` |
| 其他 file handler | 小文件 I/O | 评估，必要时 `spawn_blocking` |

`spawn_blocking` 在独立阻塞线程池执行，不占用 tokio async worker thread，保证并发性能。

## 6. 错误处理与故障恢复

| 场景 | 处理 | 影响 |
|---|---|---|
| Manager 写失败 | dispatcher `cleanup_pending`，所有等待者收到 Err | QUIC task 返回 Error 给客户端 |
| Worker 连接断开 | dispatcher 读端 EOF，`cleanup_pending` | 同上 |
| Worker 崩溃 | CrashDetector 重启 → 新连接 → 新 dispatcher | 旧 pending 已清理，新请求正常 |
| 单个 handler 失败 | 返回 Error payload | 不影响其他请求 |
| 单个 handler panic | `tokio::spawn` 的 JoinHandle 捕获，response_tx 发 Error | 不影响其他请求 |
| Worker `receive_request` EOF | 主循环 break，Worker 退出 | CrashDetector 接管 |
| mpsc channel 满 | `send_request().await` 等待 | 自然背压，不丢请求 |
| 热更新期间在途请求 | 旧 dispatcher 清理 pending，客户端收到 Err | 客户端可重试 |

### 6.1 panic 防护

`tokio::spawn` 的任务若 panic，默认会打印 panic 但不影响其他任务。为防止单个 handler panic 导致响应丢失：

```rust
tokio::spawn(async move {
    let response = std::panic::AssertUnwindSafe(
        handle_request(request, &pty, &sm, &shutdown).await
    ).catch_unwind()
     .await
     .map_err(|_| {
        tracing::error!("handler panic: request_id={}", request.request_id);
        WorkerResponse {
            request_id: request.request_id,
            payload: Some(worker_response::Payload::Error(Error {
                code: 500,
                message: "Internal handler error".to_string(),
            })),
        }
     })
     .into_ok_or_err();  // 简化：panic 时返回 Error response
    let _ = tx.send(response).await;
});
```

注：需评估 `catch_unwind` 对 `Future` 的 `UnwindSafe` 约束。若复杂度过高，备选方案为依赖 `tokio::spawn` 默认隔离 + handler 内部自检。

## 7. 配置项

`WorkerConfig` 新增 `ipc_channel_capacity`：

```rust
// config.rs
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkerConfig {
    #[serde(default = "default_agent_binary")]
    pub agent_binary: String,

    #[serde(default = "default_ipc_socket_path")]
    pub ipc_socket_path: String,

    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,

    /// IPC channel 容量（mpsc）
    /// 控制 Manager→Worker 请求通道和 Worker 内部响应通道的缓冲大小
    /// 默认 128，极端高并发场景可调大
    #[serde(default = "default_ipc_channel_capacity")]
    pub ipc_channel_capacity: usize,
}

fn default_ipc_channel_capacity() -> usize { 128 }
```

配置文件示例：
```toml
[worker]
ipc_channel_capacity = 128
```

Manager 和 Worker 使用同一配置值（Worker 通过 `--ipc-channel-capacity` 参数接收，或 Manager 在 spawn 时传递）。

## 8. 向后兼容

- **IPC 协议不变**：仍是 `[4字节长度 big-endian] + [protobuf 内容]`，无需版本协商
- **Manager/Worker 接口不变**：`send_request` / `create_pty_session` 签名不变，仅内部实现重构
- **配置项有默认值**：`ipc_channel_capacity` 默认 128，开箱即用
- **热更新流程不变**：`HotUpdateCoordinator` 的 `stop → start` 逻辑无需改动

## 9. 测试策略

### 9.1 关键回归测试

**并发 send_request 不再竞态**（核心回归）：

```rust
#[tokio::test]
async fn test_concurrent_send_request_no_race() {
    // 启动 mock Worker，模拟 receive → handle → send
    let ipc_server = setup_ipc_server_with_mock_worker().await;

    // 并发发起 N 个请求
    let mut handles = vec![];
    for i in 0..10 {
        let server = ipc_server.clone();
        handles.push(tokio::spawn(async move {
            server.send_request(payload::ReadDir { ... }).await
        }));
    }

    // 全部应成功（当前实现会因竞态部分失败）
    for handle in handles {
        assert!(handle.await.unwrap().is_ok());
    }
}
```

### 9.2 单元测试

- `request_id` 路由正确性：N 个请求的响应正确配对
- `pending` 清理逻辑：连接断开后所有等待者收到 Err
- `mpsc` 背压：channel 满时 `send_request` 等待而非失败

### 9.3 集成测试

- Worker 崩溃后请求失败而非 hang（dispatcher 清理 pending）
- 热更新期间在途请求收到 Error
- 新 Worker 就绪后请求恢复正常

### 9.4 压力测试

- 100 个并发请求全部成功
- 大文件 SFTP 传输期间，终端创建请求不被阻塞
- `handle_execute_command` 长时间执行期间，其他请求正常处理

## 10. 不在范围内

以下内容本设计不涉及，留待后续：

- 多 Worker 进程（方案 A，已否决）
- 完全 reader/writer 独立 task（方案 B3，过度工程）
- IPC 协议升级（如多路复用流、QUIC stream 复用）
- Worker 间负载均衡（单 Worker 无此需求）
- DashMap vs RwLock<HashMap> 性能基准（DashMap 并发读写更优，直接采用）

## 11. 实现顺序建议

1. **Manager 侧 IpcServer 重构**（核心）：结构改造 + dispatcher + send_request/create_pty_session 重写
2. **Worker 侧 run() 重写**：select! 结构 + spawn dispatch
3. **handler 签名调整**：`&PtyFactory` → `&Arc<PtyFactory>`，阻塞操作 `spawn_blocking`
4. **配置项**：`ipc_channel_capacity` 添加
5. **测试**：回归测试 + 集成测试
6. **验证**：`cargo build` 零错误，手动并发场景验证
