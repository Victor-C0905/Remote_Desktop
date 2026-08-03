# Phase 4: Worker 热更新功能设计文档

## 📋 文档信息

- **版本**: v1.0
- **创建日期**: 2026-08-03
- **设计者**: AI Assistant + User
- **状态**: 待实现
- **依赖**: Phase 3 (Worker 实现) 已完成

---

## 🎯 目标

实现 Worker 进程的热更新能力，使其能够在不中断现有终端会话的情况下完成升级。参考 SSH 的 `KillMode=process` + `SIGHUP` 设计哲学。

### 核心原则

1. **Manager 全程在线**：任何热更新过程 Manager 都不退出
2. **触发层统一**：多种触发方式最终都汇聚到 Manager 的 HotUpdateCoordinator
3. **数据平面隔离**：PTY I/O (master_fd) 完全在 Manager 控制，与 Worker 生命周期解耦
4. **会话绝对存活**：Worker 退出只影响新请求处理，不影响已有终端会话

---

## 🏗️ 架构总览

### 分层设计

```
┌─────────────────────────────────────────────────────────────────┐
│                        触发层 (Triggers)                         │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────┐   │
│  │ QUIC     │  │ SIGHUP   │  │ CLI 工具 │  │ apt postinst │   │
│  │ Reload   │  │ 信号     │  │ agent    │  │ 脚本         │   │
│  │ 命令     │  │          │  │ reload   │  │              │   │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └──────┬───────┘   │
│       └─────────────┴────────────┴────────────────┘            │
│                           │                                     │
│                           ↓                                     │
├─────────────────────────────────────────────────────────────────┤
│                    协调层 (Manager)                              │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  HotUpdateCoordinator                                    │    │
│  │  ├── 接收触发信号                                        │    │
│  │  ├── 启动新 Worker (新版二进制)                          │    │
│  │  ├── 等待新 Worker IPC 就绪                              │    │
│  │  ├── 发送 GracefulShutdown 给旧 Worker                   │    │
│  │  ├── 路由切换 (新请求 → 新 Worker)                        │    │
│  │  └── 清理旧 Worker 资源                                 │    │
│  └─────────────────────────────────────────────────────────┘    │
│                           │                                     │
├───────────────────────────┼─────────────────────────────────────┤
│                    执行层 (Worker)                              │
│         ┌─────────────────┴─────────────────┐                   │
│         ↓                                   ↓                   │
│  ┌──────────────┐                   ┌──────────────┐           │
│  │ 旧 Worker    │                   │ 新 Worker    │           │
│  │ (Graceful    │                   │ (新版二进制) │           │
│  │  Shutdown)   │                   │              │           │
│  │ → 退出       │                   │ → 处理新请求 │           │
│  └──────┬───────┘                   └──────────────┘           │
│         │                                                       │
├─────────┼───────────────────────────────────────────────────────┤
│         ↓       生存层 (Sessions)                                │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  Sessions (bash/zsh/python)                             │    │
│  │  ├── 成为孤儿进程，被 init/systemd 领养                  │    │
│  │  ├── Manager 仍持有 master_fd，数据流不中断             │    │
│  │  └── 内核 PTY Buffer 缓存数据                           │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

### 与现有架构的关系

Phase 4 在现有 Manager-Worker-Session 三层架构上新增：
- **Manager 侧**：HotUpdateCoordinator、OrphanProcessReaper、WorkerCrashDetector
- **Worker 侧**：GracefulShutdown 处理器
- **协议侧**：新增 GracefulShutdown、ShutdownAck、WorkerStateSnapshot 消息

---

## 🔌 触发机制层

所有触发方式最终都汇聚到 Manager 的 `HotUpdateCoordinator::trigger_reload()` 方法。触发层只负责"通知"，不负责"执行"。

### 触发源定义

```rust
/// 热更新触发源
#[derive(Debug, Clone)]
pub enum ReloadTrigger {
    /// 客户端通过 QUIC 发送 Reload 命令
    ClientCommand {
        operator: String,  // 操作者用户名
    },
    /// Unix 信号 (SIGHUP)
    UnixSignal,
    /// CLI 工具命令 (agent reload)
    CliTool,
    /// apt postinst 脚本触发
    PackagePostinst,
}
```

### 4 种触发路径实现

| 触发方式 | 实现机制 | 适用场景 |
|---------|---------|---------|
| QUIC 命令 | 客户端发送 `ReloadWorker` 消息 → Manager 收到后调用 `trigger_reload()` | 远程运维，Web UI |
| SIGHUP 信号 | Manager 启动时注册 `signal::unix::signal(SignalKind::SIGHUP)` 监听 | `systemctl reload gnome-remote-agent` |
| CLI 工具 | `agent reload` 子命令通过 Unix Socket 发送命令给运行中的 Manager | 本地运维 |
| apt postinst | 包安装脚本的 `postinst` 钩子执行 `systemctl reload` 或 `agent reload` | 包升级 |

### 信号监听任务

在 Manager 启动时 spawn 独立的信号监听任务：

```rust
async fn watch_sighup(tx: mpsc::Sender<ReloadTrigger>) {
    let mut sig = signal::unix::signal(SignalKind::SIGHUP)
        .expect("Failed to register SIGHUP handler");
    while sig.recv().await.is_some() {
        tracing::info!("收到 SIGHUP 信号，触发热更新");
        let _ = tx.send(ReloadTrigger::UnixSignal).await;
    }
}
```

### 去重与防抖

- `is_reloading` 原子布尔值防止热更新期间重复触发
- 触发信号到达时如果已在热更新中，记录日志并忽略
- 热更新完成后重置标志

```rust
pub struct HotUpdateCoordinator {
    worker_manager: Arc<WorkerManager>,
    ipc_server: Arc<IpcServer>,
    trigger_rx: mpsc::Receiver<ReloadTrigger>,
    /// 当前是否正在执行热更新（防止重复触发）
    is_reloading: Arc<AtomicBool>,
}
```

---

## 🤝 优雅关闭协议

Phase 4 的核心 - 新增 `GracefulShutdown` IPC 消息和 Worker 端的优雅退出逻辑。

### 新增 IPC 消息

在 `protocol/agent.proto` 中添加：

```protobuf
// Manager -> Worker 的优雅关闭请求
message GracefulShutdown {
    /// 宽限期（秒），超时后强制杀死
    uint32 grace_period_secs = 1;
    /// 是否进行状态迁移
    bool migrate_state = 2;
    /// 触发原因
    string reason = 3;
}

// Worker -> Manager 的关闭确认
message ShutdownAck {
    /// 是否成功完成所有未决任务
    bool all_tasks_completed = 1;
    /// 退出的会话数量（应为 0，不杀死 Sessions）
    uint32 sessions_count = 2;
}

// Worker -> Manager 的状态迁移数据
message WorkerStateSnapshot {
    /// 所有活动会话的元数据
    repeated SessionMetadata sessions = 1;
    /// 待处理的请求 ID 列表
    repeated uint64 pending_requests = 2;
}

message SessionMetadata {
    string session_id = 1;
    int32 pid = 2;
    string shell = 3;
    uint64 created_at = 4;
}
```

在 `ManagerRequest` 的 oneof 中添加：

```protobuf
message ManagerRequest {
    uint64 request_id = 1;
    oneof payload {
        // ... 现有消息 ...
        GracefulShutdown graceful_shutdown = 10;  // 新增
    }
}
```

在 `WorkerResponse` 的 oneof 中添加：

```protobuf
message WorkerResponse {
    uint64 request_id = 1;
    oneof payload {
        // ... 现有消息 ...
        ShutdownAck shutdown_ack = 9;  // 新增
    }
}
```

### Worker 端优雅退出流程

```rust
/// Worker 优雅关闭处理器
async fn handle_graceful_shutdown(
    req: GracefulShutdown,
    session_manager: &SessionManager,
    shutdown_tx: oneshot::Sender<WorkerStateSnapshot>,
) -> WorkerResponse {
    tracing::info!(
        "收到 GracefulShutdown: grace_period={}s, migrate={}, reason={}",
        req.grace_period_secs, req.migrate_state, req.reason
    );

    // 1. 停止接收新请求（关闭消息循环）
    // 2. 等待未决请求完成（最多 grace_period_secs 秒）
    let deadline = Instant::now() + Duration::from_secs(req.grace_period_secs);
    while !session_manager.all_idle().await && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // 3. 状态迁移（可选）
    let snapshot = if req.migrate_state {
        session_manager.snapshot().await
    } else {
        WorkerStateSnapshot::default()
    };

    // 4. 发送状态快照给 Manager
    let _ = shutdown_tx.send(snapshot);

    // 5. 退出 Worker 进程
    // 注意：不杀死 Sessions（bash/zsh），它们成为孤儿进程
    // Manager 仍持有 master_fd，数据流不中断

    WorkerResponse {
        payload: Some(worker_response::Payload::ShutdownAck(ShutdownAck {
            all_tasks_completed: session_manager.all_idle().await,
            sessions_count: session_manager.list().await.len() as u32,
        })),
        ..Default::default()
    }
    // Worker 主循环检测到 shutdown 信号后退出
}
```

### Manager 端协调流程

```rust
impl HotUpdateCoordinator {
    async fn execute_reload(&self, trigger: ReloadTrigger) -> Result<()> {
        // 1. 启动新 Worker（新版二进制）
        self.worker_manager.start().await?;

        // 2. 等待新 Worker IPC 连接就绪
        self.ipc_server.wait_for_new_worker().await?;

        // 3. 向旧 Worker 发送 GracefulShutdown
        let (snapshot_tx, snapshot_rx) = oneshot::channel();
        self.worker_manager.send_graceful_shutdown(
            GracefulShutdown {
                grace_period_secs: 10,
                migrate_state: false,  // Phase 4 暂不实现状态迁移
                reason: format!("{:?}", trigger),
            },
            snapshot_tx,
        ).await?;

        // 4. 接收旧 Worker 的状态快照（即使 migrate_state=false 也会发送空快照）
        let _snapshot = snapshot_rx.await.unwrap_or_default();

        // 5. 等待旧 Worker 退出
        self.worker_manager.wait_for_exit().await?;

        // 6. 清理旧 Worker 的 IPC 连接
        self.ipc_server.cleanup_old_worker_connection().await?;

        // 7. 重置 is_reloading 标志
        self.is_reloading.store(false, Ordering::SeqCst);

        Ok(())
    }
}
```

### 关键设计点

1. **新旧 Worker 共存期**：启动新 Worker 后，旧 Worker 仍存活短暂时间（宽限期），两者通过不同的 IPC 连接区分
2. **宽限期机制**：默认 10 秒，超时后 Manager 发送 SIGKILL 强制杀死旧 Worker
3. **不杀死 Sessions**：Worker 退出时只关闭自己的资源，不向 Sessions 发送信号
4. **状态迁移可选**：通过 `migrate_state` 字段控制，Phase 4 中始终设为 `false`

---

## 👻 孤儿进程处理

Worker 退出后，其 fork 的 Sessions（bash/zsh）成为孤儿进程。Manager 需要处理这些孤儿进程的回收。

### 孤儿进程生命周期

```
Worker 退出
    ↓
Sessions (bash/zsh) 被 init/systemd 领养
    ↓
Manager 仍持有 master_fd
    ↓
Manager 继续读写 PTY (数据流不中断)
    ↓
Session 退出 (用户输入 exit 或 Ctrl-D)
    ↓
master_fd 返回 EOF
    ↓
Manager 检测到 EOF → 回收僵尸进程
```

### 新增 Manager 模块：OrphanProcessReaper

```rust
/// 孤儿进程回收器
///
/// 负责：
/// - 监控 master_fd 的 EOF 事件
/// - 回收退出的 Session 僵尸进程
/// - 清理 PtyRegistry 中的相关记录
pub struct OrphanProcessReaper {
    /// PTY 注册表（共享引用）
    pty_registry: Arc<PtyRegistry>,
    /// Session PID -> session_id 的映射
    pid_map: Arc<RwLock<HashMap<Pid, String>>>,
}
```

### EOF 检测机制

在 Manager 的 PTY 输出推送任务中（`pty_output.rs`），当 `read()` 返回 0 字节（EOF）时：

```rust
// 在 pty_output.rs 的 spawn_pty_output_task 中
match nix::unistd::read(master_fd, &mut buf) {
    Ok(0) => {
        // EOF：Session 已退出
        tracing::info!("PTY EOF: session_id={}, 回收僵尸进程", session_id);

        // 1. 回收僵尸进程
        if let Some(pid) = orphan_reaper.get_pid(&session_id).await {
            let _ = orphan_reaper.reap_zombie(pid).await;
        }

        // 2. 从 PtyRegistry 注销
        pty_registry.unregister(&session_id).await;

        // 3. 通知客户端会话已关闭
        notify_client_session_closed(&session_id).await;

        // 4. 关闭 master_fd
        let _ = nix::unistd::close(master_fd);
    }
    Ok(n) => { /* 正常数据流 */ }
    Err(e) => { /* 错误处理 */ }
}
```

### 僵尸进程回收

```rust
impl OrphanProcessReaper {
    /// 回收僵尸进程
    ///
    /// 使用 waitpid(WNOHANG) 非阻塞回收
    pub async fn reap_zombie(&self, pid: Pid) -> Result<()> {
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(pid, status)) => {
                tracing::info!("僵尸进程已回收: pid={}, status={}", pid, status);
                self.cleanup_session(pid).await;
            }
            Ok(WaitStatus::Signaled(pid, sig, _)) => {
                tracing::warn!("进程被信号终止: pid={}, signal={:?}", pid, sig);
                self.cleanup_session(pid).await;
            }
            Ok(WaitStatus::StillAlive) => {
                // 进程还活着，可能 EOF 是其他原因
                tracing::warn!("PTY EOF 但进程仍存活: pid={}", pid);
            }
            Err(nix::errno::Errno::ECHILD) => {
                // 子进程已被 init 领养并回收
                tracing::debug!("进程已被 init 回收: pid={}", pid);
                self.cleanup_session(pid).await;
            }
            Err(e) => {
                tracing::error!("waitpid 失败: {}", e);
            }
        }
        Ok(())
    }
}
```

### 关键设计点

1. **Manager 不是 Sessions 的父进程**：Worker fork 的 Sessions，Manager 不是其父进程。`waitpid` 可能返回 `ECHILD`（因为 Sessions 已被 init 领养）
2. **依靠 init 回收**：如果 Manager `waitpid` 返回 `ECHILD`，说明 init 已经回收了僵尸进程，只需清理 PtyRegistry
3. **EOF 是主要信号**：通过 master_fd 的 EOF 检测 Session 退出，而非通过 SIGCHLD
4. **跨 Worker 存活**：Sessions 在 Worker 退出后继续运行，Manager 的 master_fd 不受影响

---

## 💥 崩溃检测与自动恢复

现有 `WorkerManager` 的 `notify_crash()` 是被动的，需要外部调用。Phase 4 需要主动的崩溃检测循环。

### 崩溃检测机制

```rust
/// Worker 崩溃检测器
pub struct WorkerCrashDetector {
    worker_manager: Arc<WorkerManager>,
    /// 子进程 wait 任务
    child_wait_handle: Option<tokio::task::JoinHandle<()>>,
}
```

### 检测循环

```rust
impl WorkerCrashDetector {
    /// 启动崩溃检测循环
    ///
    /// 使用 tokio::task::spawn_blocking 包装同步的 waitpid 调用
    pub fn start(&mut self) {
        let worker_manager = self.worker_manager.clone();

        self.child_wait_handle = Some(tokio::spawn(async move {
            loop {
                if let Some(info) = worker_manager.get_info().await {
                    let pid = Pid::from_raw(info.pid as i32);

                    // 非阻塞等待子进程状态变化
                    let result = tokio::task::spawn_blocking(move || {
                        waitpid(pid, Some(WaitPidFlag::WNOHANG))
                    }).await;

                    match result {
                        Ok(Ok(WaitStatus::Exited(_, status))) => {
                            // Worker 退出
                            if worker_manager.is_graceful_shutdown().await {
                                // 优雅关闭，不触发崩溃处理
                                break;
                            } else {
                                // 异常崩溃
                                worker_manager.notify_crash().await;
                                worker_manager.attempt_restart().await;
                            }
                        }
                        Ok(Ok(WaitStatus::Signaled(_, sig, _))) => {
                            // 被信号杀死
                            worker_manager.notify_crash().await;
                            worker_manager.attempt_restart().await;
                        }
                        Ok(Ok(_)) => {
                            // StillAlive 或其他状态，继续等待
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        Ok(Err(errno)) => {
                            if errno == Errno::ECHILD {
                                // 子进程不存在，可能已被回收
                                tokio::time::sleep(Duration::from_millis(500)).await;
                            }
                        }
                        Err(e) => {
                            tracing::error!("waitpid 任务失败: {}", e);
                        }
                    }
                } else {
                    // Worker 未启动
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }));
    }
}
```

### WorkerManager 扩展

在现有 `WorkerManager` 上添加：

```rust
impl WorkerManager {
    /// 尝试自动重启（崩溃后）
    pub async fn attempt_restart(&self) -> Result<()> {
        let info = self.worker_info.read().await;
        if let Some(ref info) = *info {
            if info.restart_count >= self.max_restarts {
                tracing::error!(
                    "Worker 崩溃且重启次数已达上限 ({}), 不再重启",
                    self.max_restarts
                );
                return Err(anyhow!("Max restart count exceeded"));
            }
        }
        drop(info);

        // 指数退避重启
        let delay = 2u64.pow(info.restart_count.min(5));
        tokio::time::sleep(Duration::from_secs(delay)).await;

        self.restart().await
    }

    /// 标记为优雅关闭（用于区分崩溃和正常退出）
    pub async fn mark_graceful_shutdown(&self) {
        self.is_graceful_shutdown.store(true, Ordering::SeqCst);
    }

    /// 检查是否是优雅关闭
    pub async fn is_graceful_shutdown(&self) -> bool {
        self.is_graceful_shutdown.load(Ordering::SeqCst)
    }
}
```

### 崩溃场景处理矩阵

| 场景 | 检测方式 | 响应动作 |
|------|---------|---------|
| Worker 正常退出（GracefulShutdown） | waitpid 返回 Exited | 不重启，清理 IPC 连接 |
| Worker 崩溃（panic） | waitpid 返回 Exited (非零) 或 Signaled | 自动重启（指数退避） |
| Worker 被 SIGKILL | waitpid 返回 Signaled | 自动重启 |
| Worker 卡死（不响应） | 心跳超时 | 发送 SIGTERM → 等待 → SIGKILL |
| IPC 连接断开 | IpcServer 检测到 EOF | 触发崩溃处理 |

### 心跳检测（可选增强）

```rust
/// Worker 心跳配置
pub struct HeartbeatConfig {
    /// 心跳间隔（默认 30 秒）
    pub interval_secs: u64,
    /// 超时阈值（默认 90 秒，3 次心跳）
    pub timeout_secs: u64,
}
```

如果 Worker 卡死（如死锁、无限循环），waitpid 无法检测到。通过心跳机制，Manager 定期发送 Ping，Worker 响应 Pong。超时后 Manager 主动杀死 Worker。

**Phase 4 实现建议**：心跳检测作为可选增强，先实现基础的 waitpid 检测，后续根据需要添加。

---

## 📦 状态迁移（可选）

### 必要性分析

在当前架构中：
- **Manager 的 PtyRegistry**：持有 session_id → master_fd 映射（热更新时不丢失）
- **Worker 的 SessionManager**：持有 session_id → pid 映射（Worker 退出时丢失）

关键洞察：**Worker 的 SessionManager 在新 Worker 中不需要迁移**，因为：
1. 新 Worker 不是旧 Sessions 的父进程，无法 waitpid
2. 旧 Sessions 的监控责任已转移到 Manager 的 OrphanProcessReaper
3. 新 Worker 只需要处理新请求，不需要知道旧 Sessions 的存在

### 简化的状态迁移

只迁移"未完成的请求"列表，不迁移 Session 状态：

```rust
/// 状态迁移数据（极简版）
pub struct WorkerStateSnapshot {
    /// 未完成的请求 ID 列表
    pub pending_request_ids: Vec<u64>,
}
```

**迁移流程**：
1. 旧 Worker 收到 GracefulShutdown 后，等待未决请求完成或超时
2. 将仍无法完成的请求 ID 列表发送给 Manager
3. Manager 将未完成请求的客户端通知"请重试"
4. 新 Worker 启动后，这些请求可由客户端重新发起

### Phase 4 建议

**Phase 4 暂不实现复杂状态迁移**，原因：
1. 大部分请求是短时操作（读文件、执行命令），宽限期内可完成
2. Session 状态由 Manager 持有，无需迁移
3. 复杂度低，YAGNI 原则

只在 `GracefulShutdown` 消息中保留 `migrate_state` 字段作为未来扩展点，当前实现中始终设为 `false`。

---

## 🧪 测试策略

### 测试分层

| 层级 | 测试内容 | 文件 |
|------|---------|------|
| 单元测试 | HotUpdateCoordinator 状态机 | `manager/hot_update_coordinator.rs` |
| 单元测试 | OrphanProcessReaper 僵尸回收 | `manager/orphan_reaper.rs` |
| 单元测试 | WorkerCrashDetector 检测逻辑 | `manager/crash_detector.rs` |
| 集成测试 | 完整热更新流程 | `tests/hot_update_test.rs` |
| 集成测试 | Worker 崩溃自动恢复 | `tests/crash_recovery_test.rs` |
| E2E 测试 | apt 升级触发（需 WSL） | 手动验证 |

### 关键集成测试用例

```rust
#[tokio::test]
async fn test_graceful_hot_update() {
    // 1. 启动 Manager + Worker_v1
    // 2. 创建 PTY 会话
    // 3. 触发热更新（启动 Worker_v2）
    // 4. 验证：PTY 会话数据流不中断
    // 5. 验证：新请求路由到 Worker_v2
    // 6. 验证：Worker_v1 优雅退出
}

#[tokio::test]
async fn test_worker_crash_recovery() {
    // 1. 启动 Manager + Worker
    // 2. kill -9 Worker 进程
    // 3. 验证：Manager 检测到崩溃
    // 4. 验证：Manager 自动重启 Worker
    // 5. 验证：PTY 会话数据流不中断
}

#[tokio::test]
async fn test_sighup_trigger() {
    // 1. 启动 Manager
    // 2. 发送 SIGHUP 给 Manager
    // 3. 验证：触发热更新流程
}

#[tokio::test]
async fn test_orphan_process_reaping() {
    // 1. 启动 Manager + Worker
    // 2. 创建 PTY 会话
    // 3. 杀死 Worker（模拟崩溃）
    // 4. 在 PTY 中输入 exit
    // 5. 验证：Manager 检测到 EOF
    // 6. 验证：僵尸进程被回收
    // 7. 验证：PtyRegistry 清理记录
}
```

---

## 📁 文件结构

### 新增文件

```
agent/src/manager/
├── hot_update_coordinator.rs   # 热更新协调器
├── orphan_reaper.rs            # 孤儿进程回收器
├── crash_detector.rs           # 崩溃检测器
└── signal_handler.rs           # 信号处理（SIGHUP）

agent/src/worker/handlers/
└── shutdown.rs                 # GracefulShutdown 处理器

agent/tests/
├── hot_update_test.rs          # 热更新集成测试
└── crash_recovery_test.rs      # 崩溃恢复集成测试

agent/protocol/
└── agent.proto                 # 新增 GracefulShutdown 等消息
```

### 修改文件

```
agent/src/manager/mod.rs              # 注册新模块
agent/src/manager/worker_manager.rs   # 添加 attempt_restart、mark_graceful_shutdown
agent/src/manager/pty_output.rs       # 添加 EOF 检测和僵尸回收
agent/src/worker/mod.rs               # 注册 shutdown handler
agent/src/worker/handlers/mod.rs      # 导出 shutdown handler
agent/src/main.rs                     # 启动信号监听任务
```

---

## 🔗 任务分解建议

Phase 4 建议分解为以下任务（供 writing-plans 技能细化）：

| 任务 ID | 任务名称 | 依赖 |
|---------|---------|------|
| TASK-021 | 新增 IPC 消息（GracefulShutdown、ShutdownAck、WorkerStateSnapshot）+ proto 编译 | 无 |
| TASK-022 | 实现 GracefulShutdown 处理器（Worker 端） | TASK-021 |
| TASK-023 | 实现 HotUpdateCoordinator（Manager 端） | TASK-021 |
| TASK-024 | 实现多触发机制（SIGHUP、QUIC、CLI） | TASK-023 |
| TASK-025 | 实现 OrphanProcessReaper | 无 |
| TASK-026 | 集成 EOF 检测到 pty_output | TASK-025 |
| TASK-027 | 实现 WorkerCrashDetector | 无 |
| TASK-028 | 扩展 WorkerManager（attempt_restart、mark_graceful_shutdown） | 无 |
| TASK-029 | 编写集成测试 | TASK-022, TASK-023, TASK-026, TASK-027 |
| TASK-030 | 更新 systemd 服务配置（KillMode=process、ExecReload） | TASK-024 |

---

## ✅ 验收标准

### 功能验收

1. **热更新不中断会话**：执行热更新时，已有终端会话数据流不中断
2. **新请求路由正确**：热更新后，新请求由新 Worker 处理
3. **崩溃自动恢复**：Worker 崩溃后，Manager 自动重启 Worker
4. **孤儿进程回收**：Session 退出后，僵尸进程被正确回收
5. **多种触发方式**：支持 SIGHUP、QUIC 命令、CLI 工具、apt postinst 触发

### 代码质量

1. **零编译错误**：`cargo build` 通过
2. **测试覆盖**：集成测试覆盖核心场景
3. **日志完善**：热更新全流程有详细日志
4. **错误处理**：所有错误路径有合理处理

### 文档

1. **ARCHITECTURE.md 更新**：反映 Phase 4 完成后的架构变化
2. **TASK_BREAKDOWN.md 更新**：补充 TASK-021 到 TASK-030 的完成记录
3. **systemd 服务配置说明**：KillMode、ExecReload 配置说明
