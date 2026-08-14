# 连接生命周期统一处理设计

## 背景

当前连接断开处理存在 5 个问题：

1. **`connection-lost` 事件重复 emit**：心跳、status、主循环三处都执行 cleanup + emit，被动断开时最多触发 3 次
2. **心跳失败后主循环不退出**：心跳检测到断连只 emit，不通知主循环，主循环继续存活 30s 直到 QUIC idle_timeout
3. **主动断开没有显式关闭 QUIC**：`remote_disconnect` 不调用 `conn.close()`，依赖 drop 自动关闭，但 status_task 持有 clone 阻止 drop
4. **前端监听器空窗期**：`useEffect` 依赖 `activeServerId`，每次切换服务器时监听器被拆除重建，期间会丢失事件
5. **心跳复用主循环队列**：耗时业务请求（如大文件传输握手）阻塞心跳，实际检测延迟可能远超 10s

## 设计目标

- 单一清理入口：所有断连场景（主动/被动）的清理逻辑集中在主循环退出后的统一块中
- 零重复：cleanup + emit 恰好执行一次
- 独立心跳通道：心跳检测延迟稳定在 10s，不受业务请求影响
- 立即关闭：主动断开时立即 close QUIC，不等响应
- 前端监听零空窗：监听器只注册一次，切换服务器不影响

## 架构总览

主循环是连接生命周期的唯一"清理者"。所有检测点（心跳、QUIC closed、用户主动断开）只负责"通知主循环退出"，不执行任何清理。清理逻辑集中在主循环退出后的统一清理块中。

### 任务拓扑

```
┌─────────────────────────────────────────────────────────────┐
│  主循环（唯一清理者）                                          │
│  while let Some(req) = rx.recv().await { ... }                │
│                                                               │
│  退出条件：                                                   │
│    A. ClientRequest::Disconnect      ← 用户主动断开            │
│    B. ClientRequest::ConnectionLost  ← 心跳/status 检测到      │
│    C. send_and_receive_quic 返回 Err（连接已死，通知自己）     │
│                                                               │
│  退出后统一执行：cleanup_once()                                │
│    1. abort 心跳 + status 任务                                │
│    2. close QUIC 连接（如果尚未关闭）                         │
│    3. cleanup_by_connection (传输任务)                        │
│    4. 从 ConnectionManager 移除                               │
│    5. emit("connection-lost")                                 │
└─────────────────────────────────────────────────────────────┘
        ▲ notify                    ▲ notify
        │                            │
┌───────┴────────┐          ┌───────┴────────┐
│ 心跳任务         │          │ status 任务     │
│ 独立通道发 Ping  │          │ conn.closed()  │
│ 超时 → notify   │          │ 触发 → notify  │
│ 不清理、不 emit │          │ 不清理、不 emit│
└────────────────┘          └────────────────┘
```

## 详细设计

### 1. ClientRequest 扩展

新增 `ConnectionLost` 变体，携带触发源用于日志区分：

```rust
pub enum ClientRequest {
    Send {
        envelope: Envelope,
        response_tx: oneshot::Sender<Result<Vec<u8>, String>>,
    },
    Disconnect,                    // 用户主动断开
    ConnectionLost {               // 被动检测到断连
        source: ConnectionLostSource,
    },
}

pub enum ConnectionLostSource {
    Heartbeat,    // 心跳 Ping 超时
    QuicClosed,   // QUIC 连接关闭（idle_timeout / 对端 close）
    SendFailed,   // 业务请求发送失败
}
```

`ConnectionLost` 是单向通知，不需要响应。三种触发源都走同一个 break 路径，统一进入清理块。

### 2. 主循环处理

```rust
while let Some(req) = rx.recv().await {
    match req {
        ClientRequest::Send { envelope, response_tx } => {
            if conn_clone.close_reason().is_some() {
                let _ = response_tx.send(Err("连接已关闭".into()));
                break;
            }
            let result = send_and_receive_quic(&conn_clone, envelope.request_id, envelope.payload).await;
            if result.is_err() {
                let _ = response_tx.send(Err("连接已断开".into()));
                // 通过 ConnectionLost 通知自己退出，清理逻辑只有一个入口
                let _ = tx_clone.send(ClientRequest::ConnectionLost {
                    source: ConnectionLostSource::SendFailed,
                }).await;
                continue;
            }
            let _ = response_tx.send(result);
        }
        ClientRequest::Disconnect => break,
        ClientRequest::ConnectionLost { source } => {
            tracing::info!("连接丢失（{}）: {}", source, server_id_clone);
            break;
        }
    }
}
```

`SendFailed` 不直接 break，而是发 `ConnectionLost` 给自己，避免在 match 分支里写清理代码。

### 3. 心跳任务改造（独立通道）

心跳不再复用主循环队列，直接用 `conn.clone()` 调用 `send_and_receive_quic`：

```rust
let heartbeat_task = tokio::spawn(async move {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;

        if heartbeat_conn.close_reason().is_some() {
            break;
        }

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        let envelope = Envelope::new(0, Payload::Ping { timestamp: now });

        match tokio::time::timeout(
            Duration::from_secs(5),
            send_and_receive_quic(&heartbeat_conn, 0, envelope.payload),
        ).await {
            Ok(Ok(_)) => { /* Pong 正常 */ }
            Ok(Err(_)) | Err(_) => {
                let _ = heartbeat_tx.send(ClientRequest::ConnectionLost {
                    source: ConnectionLostSource::Heartbeat,
                }).await;
                break;
            }
        }
    }
});
```

- `heartbeat_conn` = `conn.clone()`，Quinn 的 `Connection` 内部是 `Arc`，clone 廉价且支持并发 stream
- 不再创建 oneshot channel 走主循环，Ping 直接通过独立的 QUIC stream 发送
- 不再执行 cleanup / emit，只发 `ConnectionLost` 通知主循环
- Quinn 支持同一连接上多个并发 bidirectional stream，心跳 stream 和业务请求 stream 互不干扰

### 4. status 任务改造

```rust
let status_task = tokio::spawn(async move {
    status_conn.closed().await;
    tracing::warn!("QUIC 连接已关闭: {}", status_server_id);
    let _ = status_tx.send(ClientRequest::ConnectionLost {
        source: ConnectionLostSource::QuicClosed,
    }).await;
});
```

只通知主循环，不执行 cleanup / emit。

竞态安全：心跳和 status 可能几乎同时检测到断连，都发 `ConnectionLost` 到 channel。主循环处理第一个后 break，第二个 `ConnectionLost` 永远不会被 `recv()`（channel 被 drop 时未读取的消息自动丢弃，无泄漏）。

### 5. 主循环统一清理块

```rust
// ════════════════════════════════════════════════════════════
// 统一清理块（唯一清理入口）
// ════════════════════════════════════════════════════════════

// 1. 停止检测任务
heartbeat_task.abort();
status_task.abort();

// 2. 显式关闭 QUIC 连接（幂等：已关闭则无操作）
if conn_clone.close_reason().is_none() {
    conn_clone.close(0u32.into(), b"client closing");
    tracing::info!("QUIC 连接已主动关闭: {}", server_id_clone);
}

// 3. 清理传输任务
let tm = app_handle.state::<std::sync::Arc<crate::transfer::TransferManager>>();
tm.cleanup_by_connection(&server_id_clone).await;

// 4. 从 ConnectionManager 移除
if let Ok(mut conns) = app_handle.state::<ConnectionManager>().connections.lock() {
    conns.remove(&server_id_clone);
}

// 5. 通知前端（只 emit 一次）
let _ = app_handle.emit("connection-lost", &server_id_clone);

tracing::info!("连接清理完成: {}", server_id_clone);
```

| 步骤 | 作用 | 幂等性 |
|------|------|--------|
| abort 检测任务 | 防止心跳/status 在清理后还发 `ConnectionLost` | abort 幂等 |
| close QUIC | 主动断开时立即释放资源；被动断开时无操作 | Quinn close 幂等 |
| cleanup 传输 | 取消传输任务，emit `transfer-progress` | 幂等（已清理则 count=0） |
| 移除连接条目 | 释放 ConnectionManager 槽位 | remove 幂等 |
| emit 事件 | 通知前端更新 UI | 前端监听器已做去重处理 |

对比修复前：3 处分散清理（最多 3 次 cleanup + 3 次 emit）→ 修复后：1 处统一清理（恰好 1 次 cleanup + 1 次 emit）。

### 6. remote_disconnect 改造（立即 close）

```rust
pub async fn remote_disconnect(server_id: String, app: tauri::AppHandle) -> Result<(), String> {
    tracing::info!("[Connection] 主动断开: {}", server_id);

    // 1. 获取连接句柄
    let manager = app.state::<ConnectionManager>();
    let conn_info = {
        let conns = manager.connections.lock().unwrap();
        conns.get(&server_id).map(|c| (c.conn.clone(), c.tx.clone()))
    };

    let (conn, tx) = match conn_info {
        Some(v) => v,
        None => return Err("未找到该服务器的连接".into()),
    };

    // 2. 发送 DisconnectRequest（fire-and-forget，不等待响应）
    //    Agent 收到后清理传输会话等资源；收不到则靠 conn.closed() 兜底
    let request_id = manager.next_request_id();
    let envelope = Envelope::new(request_id, Payload::DisconnectRequest {});
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let _ = tx.try_send(ClientRequest::Send { envelope, response_tx });

    // 3. 立即关闭 QUIC 连接
    conn.close(0u32.into(), b"client disconnect");

    // 4. 发送 Disconnect 通知主循环退出（触发统一清理块）
    let _ = tx.send(ClientRequest::Disconnect).await;

    tracing::info!("[Connection] 主动断开完成: {}", server_id);
    Ok(())
}
```

| 项目 | 修复前 | 修复后 |
|------|--------|--------|
| DisconnectRequest 响应 | 等 3s 超时 | fire-and-forget |
| QUIC close | 不显式 close | 立即 close |
| cleanup 传输任务 | remote_disconnect 里调用 | 统一清理块执行 |
| 移除 ConnectionManager | remote_disconnect 里移除 | 统一清理块执行 |

fire-and-forget 安全性：Agent 端有两层兜底——①收到 `DisconnectRequest` 时清理 ②`conn.closed()` 触发 `cleanup_transfer_sessions`。即使 `DisconnectRequest` 丢失，Agent 仍会清理。

### 7. 前端监听器优化（消除空窗期）

```typescript
// 用 ref 持有最新值，监听器只注册一次
const activeServerIdRef = useRef(activeServerId);
activeServerIdRef.current = activeServerId;

useEffect(() => {
    log.info("设置连接丢失监听器");

    const setupListener = async () => {
        const unlisten = await listen<string>("connection-lost", (event) => {
            const lostServerId = event.payload;
            log.info("收到连接丢失事件:", lostServerId);

            setServerStatus(lostServerId, "disconnected");

            // 通过 ref 读取最新值
            if (activeServerIdRef.current === lostServerId) {
                log.info("当前活跃服务器断开，清空 activeServerId");
                setActiveServerId(null);
            }

            log.warn(`服务器 ${lostServerId} 连接已断开`);
        });
        return unlisten;
    };

    let unlistenFn: (() => void) | undefined;
    setupListener().then((fn) => { unlistenFn = fn; });

    return () => {
        if (unlistenFn) unlistenFn();
    };
    // 空依赖：监听器只注册一次
}, [setServerStatus, setActiveServerId]);
```

- `activeServerId` → `activeServerIdRef.current`，闭包不再捕获过时值
- 依赖数组移除 `activeServerId`，监听器只注册一次
- `setServerStatus` / `setActiveServerId` 是 Zustand 的稳定引用，可留在依赖中

## 涉及文件

| 文件 | 改动 |
|------|------|
| `src-tauri/src/connection.rs` | ClientRequest 新增 ConnectionLost；心跳/status/主循环/remote_disconnect 改造 |
| `src/context/ServerManager.tsx` | useEffect 依赖优化，useRef 持有 activeServerId |

## 问题修复对照

| 问题 | 修复方式 |
|------|----------|
| `connection-lost` 重复 emit | 统一清理块只 emit 一次 |
| 心跳失败后主循环不退出 | `ConnectionLost` 通知主循环立即退出 |
| 主动断开没有显式关闭 QUIC | `remote_disconnect` 立即 `conn.close()` |
| 前端监听器空窗期 | useRef + 空依赖，监听器只注册一次 |
| 心跳复用主循环队列被阻塞 | 心跳独立通道，直接调用 `send_and_receive_quic` |
