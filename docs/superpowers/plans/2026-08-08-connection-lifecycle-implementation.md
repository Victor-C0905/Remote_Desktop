# 连接生命周期统一处理实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将连接断开处理重构为"主循环统一清理"架构，消除重复清理、心跳阻塞、前端监听空窗等问题。

**Architecture:** 主循环是唯一清理者。心跳/status/业务请求失败只通过 `ClientRequest::ConnectionLost` 通知主循环退出，清理逻辑集中在主循环退出后的统一块中。心跳走独立 QUIC stream，不再复用主循环队列。

**Tech Stack:** Rust + Tokio + Quinn（Tauri 后端），TypeScript + React（前端）

---

## 文件结构

| 文件 | 责任 | 改动类型 |
|------|------|----------|
| `src-tauri/src/connection.rs` | 连接管理、心跳、主循环、断开逻辑 | 修改 |
| `src/context/ServerManager.tsx` | 前端连接丢失监听 | 修改 |

---

## Task 1: 扩展 ClientRequest 枚举

**Files:**
- Modify: `src-tauri/src/connection.rs`（`ClientRequest` 枚举定义处）

- [ ] **Step 1: 定位 ClientRequest 枚举定义**

Run: `grep -n "enum ClientRequest" src-tauri/src/connection.rs`
Expected: 显示枚举定义行号

- [ ] **Step 2: 添加 ConnectionLost 变体和 ConnectionLostSource 枚举**

在 `ClientRequest` 枚举中添加 `ConnectionLost` 变体，并在其上方定义 `ConnectionLostSource` 枚举：

```rust
/// 连接丢失的触发源（用于日志区分）
#[derive(Debug, Clone)]
pub enum ConnectionLostSource {
    /// 心跳 Ping 超时
    Heartbeat,
    /// QUIC 连接关闭（idle_timeout / 对端 close）
    QuicClosed,
    /// 业务请求发送失败
    SendFailed,
}

impl std::fmt::Display for ConnectionLostSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionLostSource::Heartbeat => write!(f, "心跳超时"),
            ConnectionLostSource::QuicClosed => write!(f, "QUIC 连接关闭"),
            ConnectionLostSource::SendFailed => write!(f, "发送失败"),
        }
    }
}
```

在 `ClientRequest` 枚举中添加：

```rust
    /// 被动检测到连接丢失（由心跳/status/发送失败触发）
    /// 仅用于通知主循环退出，不携带响应通道
    ConnectionLost {
        source: ConnectionLostSource,
    },
```

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`
Expected: 编译通过（可能有 unused 警告，因为还没使用新变体）

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/connection.rs
git commit -m "feat: 扩展 ClientRequest 枚举，新增 ConnectionLost 变体"
```

---

## Task 2: 改造心跳任务为独立通道 + 只通知不清理

**Files:**
- Modify: `src-tauri/src/connection.rs`（心跳任务 spawn 块，约 L700-L752）

- [ ] **Step 1: 定位心跳任务代码**

Run: `grep -n "心跳任务\|heartbeat_task" src-tauri/src/connection.rs`
Expected: 显示心跳任务 spawn 块行号

- [ ] **Step 2: 替换心跳任务实现**

将整个心跳任务 spawn 块替换为以下代码。关键变化：
- `heartbeat_tx` 改为持有 `tx`（主循环通道）的 clone，用于发 `ConnectionLost`
- 新增 `heartbeat_conn` = `conn_clone.clone()`，用于独立发送 Ping
- 不再创建 oneshot channel 走主循环队列
- 不再执行 cleanup / emit，只发 `ConnectionLost`
- 在 Ping 超时或发送失败时，发 `ConnectionLost` 通知主循环

```rust
            // ── 心跳任务（Watchdog）：快速检测连接断开 ────────
            // 业界标准（TeamViewer/AnyDesk 级别）：
            //   - 间隔 5s：每 5 秒发一次 Ping 探测连接活性
            //   - 超时 5s：等待 Pong 响应的最长时间
            //   - 最坏延迟：5s(间隔) + 5s(超时) = **10s**
            //   - 最佳延迟：conn.closed() 瞬间触发
            //
            // 独立通道设计：
            //   - 心跳直接用 conn.clone() 调用 send_and_receive_quic
            //   - 不走主循环队列，避免被耗时业务请求阻塞
            //   - Quinn 支持同一连接上多个并发 stream，互不干扰
            //
            // 只通知不清理：
            //   - 检测到断连后只发 ConnectionLost 给主循环
            //   - 不执行 cleanup / emit，由主循环统一清理
            let heartbeat_tx = tx.clone();
            let heartbeat_app = app_handle.clone();
            let heartbeat_server_id = server_id_clone.clone();
            let heartbeat_conn = conn_clone.clone();
            let heartbeat_task = tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
                loop {
                    interval.tick().await;

                    // 快速退出：连接已关闭
                    if heartbeat_conn.close_reason().is_some() {
                        break;
                    }

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let envelope = Envelope::new(0, Payload::Ping { timestamp: now });

                    // 独立通道：直接调用 send_and_receive_quic，不走主循环队列
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        send_and_receive_quic(&heartbeat_conn, 0, envelope.payload),
                    ).await {
                        Ok(Ok(_)) => { /* Pong 正常收到，连接存活 */ }
                        Ok(Err(e)) => {
                            tracing::warn!("心跳发送失败，连接可能已断开: {}", e);
                            let _ = heartbeat_tx.send(ClientRequest::ConnectionLost {
                                source: ConnectionLostSource::Heartbeat,
                            }).await;
                            break;
                        }
                        Err(_) => {
                            tracing::warn!("Ping 超时 (5s)，连接无响应: {}", heartbeat_server_id);
                            let _ = heartbeat_tx.send(ClientRequest::ConnectionLost {
                                source: ConnectionLostSource::Heartbeat,
                            }).await;
                            break;
                        }
                    }
                }
                // 心跳任务结束：不执行 cleanup / emit，由主循环统一清理
                let _ = heartbeat_app; // 抑制未使用警告（保留用于未来扩展）
            });
```

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`
Expected: 编译通过

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/connection.rs
git commit -m "refactor: 心跳任务改为独立通道，只通知主循环不清理"
```

---

## Task 3: 改造 status 任务为只通知不清理

**Files:**
- Modify: `src-tauri/src/connection.rs`（status 任务 spawn 块，约 L754-L764）

- [ ] **Step 1: 替换 status 任务实现**

将 status 任务 spawn 块替换为以下代码。关键变化：
- 不再执行 cleanup / emit
- 只发 `ConnectionLost` 通知主循环

```rust
            // 连接状态监听任务
            // 只通知主循环，不执行 cleanup / emit
            // 竞态安全：心跳和 status 可能同时检测到断连，都发 ConnectionLost。
            // 主循环处理第一个后 break，第二个消息随 channel drop 自动丢弃。
            let status_conn = conn_clone.clone();
            let status_tx = tx.clone();
            let status_server_id = server_id_clone.clone();
            let status_task = tokio::spawn(async move {
                status_conn.closed().await;
                tracing::warn!("QUIC 连接已关闭: {}", status_server_id);
                let _ = status_tx.send(ClientRequest::ConnectionLost {
                    source: ConnectionLostSource::QuicClosed,
                }).await;
            });
```

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`
Expected: 编译通过

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/connection.rs
git commit -m "refactor: status 任务改为只通知主循环不清理"
```

---

## Task 4: 改造主循环处理 ConnectionLost + 统一清理块

**Files:**
- Modify: `src-tauri/src/connection.rs`（主循环 + 清理块，约 L766-L792）

- [ ] **Step 1: 替换主循环和清理块**

将主循环 `while let Some(req) = rx.recv().await` 块及其后的清理块整体替换。关键变化：
- 主循环 match 新增 `ConnectionLost` 分支
- `SendFailed` 不直接 break，发 `ConnectionLost` 给自己
- 统一清理块：abort 任务 → close QUIC → cleanup 传输 → 移除连接 → emit（恰好一次）

需要先在主循环外获取 `tx` 的 clone（用于 SendFailed 时通知自己）。注意 `tx` 本身已被 spawn 任务 move，需要在 spawn 前 clone。

```rust
            // 主循环需要 tx clone 用于 SendFailed 时通知自己
            let main_tx = tx.clone();

            // 主消息循环
            while let Some(req) = rx.recv().await {
                match req {
                    ClientRequest::Send { envelope, response_tx } => {
                        // 检查连接状态
                        if conn_clone.close_reason().is_some() {
                            let _ = response_tx.send(Err("连接已关闭".into()));
                            break;
                        }
                        let result = send_and_receive_quic(&conn_clone, envelope.request_id, envelope.payload).await;
                        if result.is_err() {
                            let _ = response_tx.send(Err("连接已断开".into()));
                            // 通过 ConnectionLost 通知自己退出，清理逻辑只有一个入口
                            let _ = main_tx.send(ClientRequest::ConnectionLost {
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

            // ════════════════════════════════════════════════════════════
            // 统一清理块（唯一清理入口）
            // 退出原因：Disconnect / ConnectionLost / 主循环自然结束
            // 所有清理集中在此处，保证只执行一次
            // ════════════════════════════════════════════════════════════

            // 1. 停止检测任务（防止心跳/status 在清理后还发 ConnectionLost）
            heartbeat_task.abort();
            status_task.abort();

            // 2. 显式关闭 QUIC 连接（幂等：已关闭则无操作）
            //    主动断开：确保连接立即关闭，释放资源
            //    被动断开：close_reason() 已有值，close() 是无操作
            if conn_clone.close_reason().is_none() {
                conn_clone.close(0u32.into(), b"client closing");
                tracing::info!("QUIC 连接已主动关闭: {}", server_id_clone);
            }

            // 3. 清理传输任务（取消该连接所有未完成的传输）
            let tm = app_handle.state::<std::sync::Arc<crate::transfer::TransferManager>>();
            tm.cleanup_by_connection(&server_id_clone).await;

            // 4. 从 ConnectionManager 移除连接条目
            if let Ok(mut conns) = app_handle.state::<ConnectionManager>().connections.lock() {
                conns.remove(&server_id_clone);
            }

            // 5. 通知前端（只 emit 一次）
            let _ = app_handle.emit("connection-lost", &server_id_clone);

            tracing::info!("连接清理完成: {}", server_id_clone);
```

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`
Expected: 编译通过（注意：`rx` 所有权问题——`tx` 被 clone 给 main_tx 和 heartbeat/status，`rx` 仍在主循环中使用）

- [ ] **Step 3: 检查是否有未使用的变量警告**

Run: `cd src-tauri && cargo check 2>&1 | grep -i "warning\|unused"`
Expected: 无 unused 警告，或仅有无关警告

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/connection.rs
git commit -m "refactor: 主循环统一清理块，所有断连场景只清理一次"
```

---

## Task 5: 改造 remote_disconnect 为立即 close + fire-and-forget

**Files:**
- Modify: `src-tauri/src/connection.rs`（`remote_disconnect` 函数，约 L801-L863）

- [ ] **Step 1: 替换 remote_disconnect 实现**

将整个 `remote_disconnect` 函数体替换。关键变化：
- 不再等 DisconnectRequest 响应 3s（fire-and-forget）
- 立即 `conn.close()`
- 不再调用 `cleanup_by_connection`（由统一清理块执行）
- 不再移除 ConnectionManager 条目（由统一清理块执行）

```rust
pub async fn remote_disconnect(server_id: String, app: tauri::AppHandle) -> Result<(), String> {
    tracing::info!("[Connection] 主动断开: {}", server_id);

    // 1. 获取连接句柄（conn + tx）
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
    // 不等待 response_rx —— 立即关闭连接

    // 3. 立即关闭 QUIC 连接
    //    主循环会在下次 send 时检测到 close_reason，或 status_task 的
    //    conn.closed() 触发，任一都会通过统一清理块完成清理
    conn.close(0u32.into(), b"client disconnect");

    // 4. 发送 Disconnect 通知主循环退出（触发统一清理块）
    let _ = tx.send(ClientRequest::Disconnect).await;

    tracing::info!("[Connection] 主动断开完成: {}", server_id);
    Ok(())
}
```

- [ ] **Step 2: 确认 ActiveConnection 结构体有 conn 字段**

Run: `grep -n "struct ActiveConnection" -A 10 src-tauri/src/connection.rs`
Expected: 显示 `ActiveConnection` 结构体定义。如果没有 `conn` 字段，需要添加（类型为 `quinn::Connection`）。

如果 `ActiveConnection` 没有 `conn` 字段，在结构体中添加：

```rust
pub struct ActiveConnection {
    pub info: ConnectionInfo,
    pub tx: mpsc::Sender<ClientRequest>,
    pub conn: quinn::Connection,  // 新增：用于 remote_disconnect 时立即 close
}
```

并在连接创建处（`conns.insert`）添加 `conn: conn_clone.clone()`。

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`
Expected: 编译通过

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/connection.rs
git commit -m "refactor: remote_disconnect 立即 close，清理交给统一清理块"
```

---

## Task 6: 前端监听器优化（useRef 消除空窗期）

**Files:**
- Modify: `src/context/ServerManager.tsx`（`useEffect` 监听 connection-lost，约 L70-L107）

- [ ] **Step 1: 确认 useRef 已导入**

Run: `grep -n "import.*useRef\|from.*react" src/context/ServerManager.tsx | head -5`
Expected: 显示 React import 行。如果没有 `useRef`，在 import 中添加。

- [ ] **Step 2: 添加 useRef 并修改 useEffect**

在组件函数体顶部（`activeServer` 定义附近）添加 useRef：

```typescript
const activeServer = servers.find((s) => s.id === activeServerId) || null;

// 用 ref 持有最新值，避免 useEffect 闭包捕获过时值
const activeServerIdRef = useRef(activeServerId);
activeServerIdRef.current = activeServerId;
```

将 `connection-lost` 的 `useEffect` 替换为：

```typescript
  // 监听连接丢失事件
  useEffect(() => {
    log.info("设置连接丢失监听器");

    const setupListener = async () => {
      const unlisten = await listen<string>("connection-lost", (event) => {
        const lostServerId = event.payload;
        log.info("收到连接丢失事件:", lostServerId);

        // 更新服务器状态为 disconnected
        setServerStatus(lostServerId, "disconnected");

        // 通过 ref 读取最新值，而非闭包捕获
        if (activeServerIdRef.current === lostServerId) {
          log.info("当前活跃服务器断开，清空 activeServerId");
          setActiveServerId(null);
        }

        log.warn(`服务器 ${lostServerId} 连接已断开`);
      });

      return unlisten;
    };

    let unlistenFn: (() => void) | undefined;
    setupListener().then((fn) => {
      unlistenFn = fn;
    }).catch((e) => log.error('连接丢失监听设置失败:', e));

    // 清理监听器
    return () => {
      if (unlistenFn) {
        log.info("清理连接丢失监听器");
        unlistenFn();
      }
    };
    // 空依赖：监听器只注册一次，组件生命周期内不变
    // setServerStatus / setActiveServerId 是 Zustand 的稳定引用
  }, [setServerStatus, setActiveServerId]);
```

- [ ] **Step 3: 前端编译验证**

Run: `cd src-tauri && npm run build 2>&1 | tail -10`（或在项目根目录 `npm run build`）
Expected: 编译通过，无 TypeScript 错误

- [ ] **Step 4: Commit**

```bash
git add src/context/ServerManager.tsx
git commit -m "fix: 前端 connection-lost 监听器用 useRef 消除空窗期"
```

---

## Task 7: 集成验证

**Files:**
- 无文件修改，仅验证

- [ ] **Step 1: 后端完整编译**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: 编译通过，无错误

- [ ] **Step 2: 前端完整编译**

Run: `npm run build 2>&1 | tail -10`
Expected: 编译通过

- [ ] **Step 3: 手动测试——主动断开**

1. 启动应用，连接服务器
2. 打开文件管理器，开始一个大文件传输
3. 主动点击断开连接
4. 验证：传输任务立即变为 cancelled，服务器状态变为 disconnected，QUIC 连接立即关闭

- [ ] **Step 4: 手动测试——被动断开（服务器重启）**

1. 启动应用，连接服务器
2. 打开终端，开始一个命令（如 `top`）
3. 在服务器上执行 `systemctl restart gnome-remote-agent`
4. 验证：10s 内检测到断连（心跳超时），传输任务变为 cancelled，服务器状态变为 disconnected

- [ ] **Step 5: 手动测试——心跳不被业务请求阻塞**

1. 启动应用，连接服务器
2. 开始一个超大文件传输（几 GB）
3. 观察心跳日志：Ping 应每 5s 发送一次，Pong 应正常返回
4. 断开服务器网络
5. 验证：10s 内检测到断连，不受文件传输影响

- [ ] **Step 6: 手动测试——切换服务器不丢失监听**

1. 连接服务器 A
2. 断开 A，连接服务器 B
3. 在连接 B 期间，模拟 A 的 connection-lost 事件（如通过开发工具）
4. 验证：前端不崩溃，监听器仍正常工作

- [ ] **Step 7: 验证日志无重复**

检查日志中 `cleanup_by_connection` 和 `connection-lost` 的调用次数：
- 主动断开：各 1 次
- 被动断开：各 1 次（之前是最多 3 次）

---

## Self-Review 结果

**1. Spec coverage:**
- ✅ 统一清理入口 → Task 4（主循环统一清理块）
- ✅ ClientRequest 扩展 → Task 1（ConnectionLost + ConnectionLostSource）
- ✅ 心跳独立通道 → Task 2（直接调用 send_and_receive_quic）
- ✅ status 只通知 → Task 3（只发 ConnectionLost）
- ✅ remote_disconnect 立即 close → Task 5（fire-and-forget + conn.close）
- ✅ 前端监听器优化 → Task 6（useRef + 空依赖）
- ✅ 集成验证 → Task 7

**2. Placeholder scan:** 无 TODO / 无 "appropriate error handling" / 无 "similar to Task N"

**3. Type consistency:**
- `ConnectionLostSource` 在 Task 1 定义，Task 2/3/4 使用，名称一致
- `ClientRequest::ConnectionLost { source }` 在 Task 1 定义，Task 2/3/4 使用，字段名一致
- `activeServerIdRef` 在 Task 6 定义并使用，一致
