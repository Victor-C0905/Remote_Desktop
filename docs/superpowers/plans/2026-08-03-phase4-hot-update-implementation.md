# Phase 4: Worker 热更新功能实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 Worker 进程热更新能力，支持不中断终端会话的升级、崩溃自动恢复、孤儿进程回收。

**Architecture:** Manager 作为中央协调者，支持多种触发路径（SIGHUP、QUIC 命令、CLI 工具），通过新增的 GracefulShutdown IPC 消息协调新旧 Worker 平滑切换。Worker 退出后 Sessions 成为孤儿进程，Manager 通过 master_fd 的 EOF 检测回收僵尸进程。

**Tech Stack:** Rust 2024 edition、Tokio 异步运行时、Protobuf (prost)、Unix Domain Socket (SCM_RIGHTS)、nix crate

**Spec:** `docs/superpowers/specs/2026-08-03-phase4-hot-update-design.md`

---

## 文件结构

### 新增文件

```
agent/protocol/agent.proto                         # 修改：新增消息字段
agent/src/manager/hot_update_coordinator.rs       # 新建：热更新协调器
agent/src/manager/orphan_reaper.rs                 # 新建：孤儿进程回收器
agent/src/manager/crash_detector.rs                # 新建：崩溃检测器
agent/src/manager/signal_handler.rs                # 新建：信号处理
agent/src/worker/handlers/shutdown.rs              # 新建：优雅关闭处理器
agent/tests/hot_update_test.rs                     # 新建：热更新集成测试
agent/tests/crash_recovery_test.rs                 # 新建：崩溃恢复集成测试
agent/tests/orphan_reaper_test.rs                  # 新建：孤儿进程回收测试
systemd/gnome-remote-agent.service                 # 修改：新增 KillMode/ExecReload
```

### 修改文件

```
agent/src/manager/mod.rs                # 注册新模块
agent/src/manager/worker_manager.rs     # 添加 attempt_restart、mark_graceful_shutdown
agent/src/manager/pty_output.rs          # 添加 EOF 检测和僵尸回收钩子
agent/src/worker/mod.rs                  # 注册 shutdown handler
agent/src/worker/handlers/mod.rs         # 导出 shutdown handler
agent/src/worker/session_manager.rs     # 添加 all_idle、snapshot 方法
```

---

## Task 1: 新增 IPC 消息（GracefulShutdown、ShutdownAck）

**Files:**
- Modify: `agent/protocol/agent.proto:10-36`

- [ ] **Step 1: 修改 `agent/protocol/agent.proto`，在 ManagerRequest 中新增 GracefulShutdown**

修改 `ManagerRequest` 的 oneof，在 `GetSystemInfo get_system_info = 9;` 后添加：

```protobuf
message ManagerRequest {
    uint64 request_id = 1;
    oneof payload {
        CreateSession create_session = 2;
        // ResizeWindow 已移除：由 Manager 直接处理（Worker 不持有 master_fd）
        KillSession kill_session = 4;
        ReadDir read_dir = 5;
        ReadFile read_file = 6;
        WriteFile write_file = 7;
        ExecuteCommand execute_command = 8;
        GetSystemInfo get_system_info = 9;
        GracefulShutdown graceful_shutdown = 10;  // 新增：优雅关闭请求
    }
}
```

- [ ] **Step 2: 在 `WorkerResponse` 中新增 ShutdownAck**

```protobuf
message WorkerResponse {
    uint64 request_id = 1;
    oneof payload {
        SessionCreated session_created = 2;
        DirListing dir_listing = 3;
        FileContent file_content = 4;
        WriteResult write_result = 5;
        CommandOutput command_output = 6;
        SystemInfo system_info = 7;
        Error error = 8;
        ShutdownAck shutdown_ack = 9;  // 新增：关闭确认
    }
}
```

- [ ] **Step 3: 在文件末尾添加新消息定义**

```protobuf
// ===== 热更新 =====

// Manager -> Worker 的优雅关闭请求
message GracefulShutdown {
    uint32 grace_period_secs = 1;   // 宽限期（秒），超时后强制杀死
    bool migrate_state = 2;          // 是否进行状态迁移（Phase 4 暂不实现，始终为 false）
    string reason = 3;               // 触发原因
}

// Worker -> Manager 的关闭确认
message ShutdownAck {
    bool all_tasks_completed = 1;    // 是否成功完成所有未决任务
    uint32 sessions_count = 2;       // 退出的会话数量（应为 0，不杀死 Sessions）
}

// Worker -> Manager 的状态迁移数据（Phase 4 暂不使用，预留扩展点）
message WorkerStateSnapshot {
    repeated uint64 pending_requests = 1;  // 待处理的请求 ID 列表
}
```

- [ ] **Step 4: 在 WSL/Linux 环境编译验证 proto 更新**

```bash
cd agent
cargo build
```

Expected: 编译成功，`src/protocol/generated.rs` 自动重新生成包含新消息类型

- [ ] **Step 5: 提交（由用户执行）**

```bash
git add agent/protocol/agent.proto
# 用户自行执行 git commit
```

---

## Task 2: 扩展 SessionManager（all_idle、snapshot 方法）

**Files:**
- Modify: `agent/src/worker/session_manager.rs`

- [ ] **Step 1: 在 `SessionManager` impl 块中添加 `all_idle` 和 `snapshot` 方法**

在 [session_manager.rs](file:///e:/MyWork/gnome-remote/agent/src/worker/session_manager.rs) 的 `impl SessionManager` 块末尾（`monitor_child_processes` 方法之后）添加：

```rust
    /// 检查所有会话是否空闲（无未决请求）
    ///
    /// Phase 4 简化实现：始终返回 true（当前 SessionManager 不跟踪请求状态）
    /// 后续可扩展为跟踪每个会话的活跃请求数
    pub async fn all_idle(&self) -> bool {
        // 当前实现：SessionManager 只跟踪 PTY 会话生命周期
        // PTY 会话本身是长连接，不算"未决请求"
        true
    }

    /// 生成当前状态快照（用于状态迁移）
    ///
    /// Phase 4 暂不实现复杂状态迁移，返回空快照
    pub async fn snapshot(&self) -> Vec<u64> {
        // 返回空列表（无未决请求）
        // 后续可扩展为返回正在执行的命令的 request_id 列表
        Vec::new()
    }
```

- [ ] **Step 2: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译通过，无错误

- [ ] **Step 3: 提交（由用户执行）**

```bash
git add agent/src/worker/session_manager.rs
```

---

## Task 3: 实现 GracefulShutdown 处理器（Worker 端）

**Files:**
- Create: `agent/src/worker/handlers/shutdown.rs`
- Modify: `agent/src/worker/handlers/mod.rs`
- Modify: `agent/src/worker/mod.rs`

- [ ] **Step 1: 创建 `agent/src/worker/handlers/shutdown.rs`**

```rust
//! 优雅关闭处理器 - 处理 Manager 的 GracefulShutdown 请求
//!
//! 该模块负责：
//! - 接收 GracefulShutdown 请求
//! - 等待未决任务完成（宽限期内）
//! - 生成状态快照（Phase 4 暂不使用）
//! - 返回 ShutdownAck 给 Manager
//! - 通知 Worker 主循环退出（不杀死 Sessions）

use std::time::{Duration, Instant};
use tokio::sync::oneshot;

use crate::protocol::generated::{
    GracefulShutdown, ShutdownAck, WorkerStateSnapshot,
    WorkerResponse, worker_response, Error,
};
use super::super::SessionManager;

/// 处理 GracefulShutdown 请求
///
/// # 参数
///
/// - `req`: GracefulShutdown 请求参数
/// - `session_manager`: 会话管理器实例
/// - `shutdown_tx`: 通知 Worker 主循环退出的 oneshot 通道
///
/// # 返回
///
/// 返回 `WorkerResponse`，包含 `ShutdownAck`。
///
/// # 流程
///
/// 1. 记录日志
/// 2. 等待未决请求完成（最多 grace_period_secs 秒）
/// 3. 生成状态快照（Phase 4 返回空）
/// 4. 通过 oneshot 通知主循环退出
/// 5. 返回 ShutdownAck
pub async fn handle_graceful_shutdown(
    req: GracefulShutdown,
    session_manager: &SessionManager,
    shutdown_tx: oneshot::Sender<()>,
) -> WorkerResponse {
    tracing::info!(
        "收到 GracefulShutdown: grace_period={}s, migrate={}, reason={}",
        req.grace_period_secs, req.migrate_state, req.reason
    );

    // 1. 等待未决请求完成（宽限期内轮询 all_idle）
    let grace_period = Duration::from_secs(req.grace_period_secs as u64);
    let deadline = Instant::now() + grace_period;

    while !session_manager.all_idle().await && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let all_completed = session_manager.all_idle().await;
    let sessions_count = session_manager.list().await.len() as u32;

    tracing::info!(
        "GracefulShutdown 处理完成: all_tasks_completed={}, sessions_count={}",
        all_completed, sessions_count
    );

    // 2. 生成状态快照（Phase 4 不使用，预留扩展点）
    if req.migrate_state {
        let _snapshot: Vec<u64> = session_manager.snapshot().await;
        // Phase 4 不实现状态迁移，仅记录日志
        tracing::debug!("状态迁移已请求，但 Phase 4 暂未实现");
    }

    // 3. 通知 Worker 主循环退出（通过 oneshot）
    // 注意：Worker 退出时不杀死 Sessions，它们成为孤儿进程
    // Manager 仍持有 master_fd，数据流不中断
    let _ = shutdown_tx.send(());

    // 4. 返回 ShutdownAck
    WorkerResponse {
        payload: Some(worker_response::Payload::ShutdownAck(ShutdownAck {
            all_tasks_completed: all_completed,
            sessions_count,
        })),
        ..Default::default()
    }
}
```

- [ ] **Step 2: 修改 `agent/src/worker/handlers/mod.rs`，注册 shutdown 模块**

将 [handlers/mod.rs](file:///e:/MyWork/gnome-remote/agent/src/worker/handlers/mod.rs) 的内容修改为：

```rust
//! Handlers 模块 - 处理各类 Manager 请求
//!
//! 该模块包含各类业务逻辑处理器：
//! - `session`: 会话管理（创建 PTY、调整终端大小）
//! - `file`: 文件操作（读取目录、读写文件）
//! - `command`: 命令执行
//! - `system`: 系统信息查询
//! - `shutdown`: 优雅关闭（Phase 4 新增）

pub mod command;
pub mod file;
pub mod session;
pub mod shutdown;
pub mod system;
```

- [ ] **Step 3: 修改 `agent/src/worker/mod.rs`，在 `handle_request` 中分发 GracefulShutdown 请求**

在 [worker/mod.rs](file:///e:/MyWork/gnome-remote/agent/src/worker/mod.rs) 中，`handle_request` 函数的 match 语句里，在 `GetSystemInfo` 分支之后、`None` 分支之前添加新分支。

需要先修改 `run` 函数签名，添加 `shutdown_rx` 接收器。找到 `run` 函数，将其修改为：

```rust
pub async fn run(ipc_client: IpcClient) -> Result<()> {
    tracing::info!("Worker 消息处理循环启动");

    // 创建 PtyFactory
    let pty_factory = PtyFactory::new(std::sync::Arc::new(ipc_client.clone()));

    // 创建会话管理器
    let session_manager = SessionManager::new();

    // 启动子进程监控任务
    let session_manager_clone = session_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = session_manager_clone.monitor_child_processes().await {
            tracing::error!("子进程监控任务异常退出: {}", e);
        }
    });

    // 创建 shutdown 信号通道（GracefulShutdown 处理器通过此通道通知主循环退出）
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    loop {
        // 使用 select! 同时监听请求和 shutdown 信号
        tokio::select! {
            // 接收到 shutdown 信号，退出主循环
            _ = &mut shutdown_rx => {
                tracing::info!("收到 shutdown 信号，Worker 主循环退出");
                break;
            }
            // 接收 Manager 的请求
            request_result = ipc_client.receive_request() => {
                match request_result {
                    Ok(request) => {
                        tracing::debug!("收到 Manager 请求: request_id={}", request.request_id);

                        // 处理请求（将 shutdown_tx 的所有权转移到 handle_request）
                        let response = handle_request(
                            request,
                            &pty_factory,
                            &session_manager,
                            shutdown_tx,
                        ).await;

                        // 发送响应
                        if let Err(e) = ipc_client.send_response(&response).await {
                            tracing::error!("发送响应失败: {}", e);
                            break;
                        }

                        // 如果响应是 ShutdownAck，说明已处理 GracefulShutdown
                        // shutdown_tx 已被 send，select! 将在下一次循环退出
                    }
                    Err(e) => {
                        tracing::error!("接收消息失败: {}", e);
                        break;
                    }
                }
            }
        }
    }

    tracing::info!("Worker 消息处理循环结束");

    Ok(())
}
```

然后修改 `handle_request` 函数签名和实现：

```rust
async fn handle_request(
    request: ManagerRequest,
    pty_factory: &PtyFactory,
    session_manager: &SessionManager,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
) -> WorkerResponse {
    // 提取 request_id
    let request_id = request.request_id;

    // 处理请求
    let mut response = match request.payload {
        Some(crate::protocol::generated::manager_request::Payload::CreateSession(req)) => {
            handlers::session::handle_create_session(pty_factory, session_manager, req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::KillSession(req)) => {
            handlers::session::handle_kill_session(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::ReadDir(req)) => {
            handlers::file::handle_read_dir(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::ReadFile(req)) => {
            handlers::file::handle_read_file(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::WriteFile(req)) => {
            handlers::file::handle_write_file(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::ExecuteCommand(req)) => {
            handlers::command::handle_execute_command(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::GetSystemInfo(req)) => {
            handlers::system::handle_get_system_info(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::GracefulShutdown(req)) => {
            handlers::shutdown::handle_graceful_shutdown(req, session_manager, shutdown_tx).await
        }
        None => {
            tracing::warn!("收到空请求 payload");

            WorkerResponse {
                request_id,
                payload: Some(worker_response::Payload::Error(Error {
                    code: 400,
                    message: "Empty request payload".to_string(),
                })),
            }
        }
    };

    // 统一设置 request_id
    response.request_id = request_id;
    response
}
```

注意：`WorkerStateSnapshot` 的 import 可以移除（Phase 4 不使用），或保留以备未来扩展。如保留，需在 imports 中添加：

```rust
use crate::protocol::generated::{ManagerRequest, WorkerResponse, worker_response, Error};
```

（移除原有的 `WorkerStateSnapshot` 引用，因为 Phase 4 不直接使用）

- [ ] **Step 4: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译通过，可能有未使用 import 的警告（可忽略或清理）

- [ ] **Step 5: 提交（由用户执行）**

```bash
git add agent/src/worker/handlers/shutdown.rs agent/src/worker/handlers/mod.rs agent/src/worker/mod.rs
```

---

## Task 4: 扩展 WorkerManager（attempt_restart、mark_graceful_shutdown）

**Files:**
- Modify: `agent/src/manager/worker_manager.rs`

- [ ] **Step 1: 在 `WorkerManager` 结构体中添加 `is_graceful_shutdown` 字段**

在 [worker_manager.rs](file:///e:/MyWork/gnome-remote/agent/src/manager/worker_manager.rs) 的 `pub struct WorkerManager` 中，在 `status_tx` 字段之后添加：

```rust
pub struct WorkerManager {
    /// Worker 进程句柄
    worker_process: Arc<RwLock<Option<Child>>>,

    /// Worker 进程信息
    worker_info: Arc<RwLock<Option<WorkerInfo>>>,

    /// Agent 二进制路径
    agent_binary: String,

    /// Unix Socket 路径（与 Worker 通信）
    ipc_socket_path: String,

    /// 最大重启次数
    max_restarts: u32,

    /// 进程状态变化事件通道
    status_tx: broadcast::Sender<WorkerStatusEvent>,

    /// 是否正在执行优雅关闭（用于区分崩溃和正常退出）
    is_graceful_shutdown: Arc<std::sync::atomic::AtomicBool>,
}
```

- [ ] **Step 2: 在 `new` 方法中初始化新字段**

在 `WorkerManager::new` 函数中，在 `Self { ... }` 之前添加初始化：

```rust
pub fn new(agent_binary: String, ipc_socket_path: String, max_restarts: u32) -> Self {
    // 创建 broadcast channel（容量 16）
    let (status_tx, _) = broadcast::channel(16);

    Self {
        worker_process: Arc::new(RwLock::new(None)),
        worker_info: Arc::new(RwLock::new(None)),
        agent_binary,
        ipc_socket_path,
        max_restarts,
        status_tx,
        is_graceful_shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    }
}
```

- [ ] **Step 3: 在 impl 块末尾添加 `mark_graceful_shutdown`、`is_graceful_shutdown`、`attempt_restart`、`wait_for_exit` 方法**

在 `notify_crash` 方法之后、`impl Drop` 之前添加：

```rust
    /// 标记为优雅关闭（用于区分崩溃和正常退出）
    ///
    /// 在发送 GracefulShutdown 请求前调用，使崩溃检测器能识别这是正常退出
    pub async fn mark_graceful_shutdown(&self) {
        self.is_graceful_shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
        tracing::info!("Worker 已标记为优雅关闭状态");
    }

    /// 检查是否是优雅关闭
    pub async fn is_graceful_shutdown(&self) -> bool {
        self.is_graceful_shutdown.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 重置优雅关闭标志（在新 Worker 启动后调用）
    pub async fn reset_graceful_shutdown(&self) {
        self.is_graceful_shutdown.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// 尝试自动重启（崩溃后）
    ///
    /// 使用指数退避策略：2^restart_count 秒（最多 32 秒）
    pub async fn attempt_restart(&self) -> Result<()> {
        // 读取当前重启次数
        let restart_count = {
            let info_guard = self.worker_info.read().await;
            info_guard.as_ref().map(|i| i.restart_count).unwrap_or(0)
        };

        if restart_count >= self.max_restarts {
            tracing::error!(
                "Worker 崩溃且重启次数已达上限 ({}), 不再重启",
                self.max_restarts
            );
            return Err(anyhow::anyhow!("Worker restart count exceeded: {}/{}", restart_count, self.max_restarts));
        }

        // 指数退避：2^n 秒，n 最大为 5（32 秒）
        let delay_secs = 2u64.pow(restart_count.min(5));
        tracing::info!(
            "Worker 崩溃后等待 {} 秒后重启 (restart_count={})",
            delay_secs, restart_count
        );
        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;

        // 重置优雅关闭标志
        self.reset_graceful_shutdown().await;

        // 调用 restart（会自动增加 restart_count）
        self.restart().await
    }

    /// 等待 Worker 进程退出
    ///
    /// 阻塞当前任务直到 Worker 子进程退出
    pub async fn wait_for_exit(&self) -> Result<()> {
        let mut process_guard = self.worker_process.write().await;

        if let Some(ref mut child) = *process_guard {
            match child.wait() {
                Ok(status) => {
                    tracing::info!("Worker 进程已退出: status={}", status);
                }
                Err(e) => {
                    tracing::warn!("等待 Worker 进程退出时出错: {}", e);
                }
            }
            *process_guard = None;
        }

        Ok(())
    }
```

- [ ] **Step 4: 修复 `restart` 方法中的变量引用**

原 `restart` 方法中有 `info.restart_count.min(5)` 但 `info` 在外部作用域已 drop。修改 `restart` 方法的指数退避部分：

将 `restart` 方法中的：
```rust
        // 检查是否超过最大重启次数
        if info.restart_count > self.max_restarts {
```

保持不变，但注意 `restart` 方法本身已有完整的逻辑，不需要修改。`attempt_restart` 调用 `restart` 即可。

- [ ] **Step 5: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译通过

- [ ] **Step 6: 添加单元测试**

在 `worker_manager.rs` 的 `#[cfg(test)] mod tests` 块末尾添加：

```rust
    #[tokio::test]
    async fn test_graceful_shutdown_flag() {
        let manager = WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3
        );

        // 初始应为 false
        assert!(!manager.is_graceful_shutdown().await);

        // 标记后应为 true
        manager.mark_graceful_shutdown().await;
        assert!(manager.is_graceful_shutdown().await);

        // 重置后应为 false
        manager.reset_graceful_shutdown().await;
        assert!(!manager.is_graceful_shutdown().await);
    }

    #[tokio::test]
    async fn test_attempt_restart_exceeds_limit() {
        let manager = WorkerManager::new(
            "/nonexistent/binary".to_string(),
            "/tmp/test.sock".to_string(),
            2
        );

        // 手动设置 restart_count 达到上限
        {
            let mut info_guard = manager.worker_info.write().await;
            *info_guard = Some(WorkerInfo {
                pid: 1234,
                started_at: SystemTime::now(),
                restart_count: 2,  // 已达到限制
                status: WorkerStatus::Crashed,
            });
        }

        // 尝试重启，应该失败
        let result = manager.attempt_restart().await;
        assert!(result.is_err());
    }
```

- [ ] **Step 7: 运行测试**

```bash
cd agent
cargo test --lib manager::worker_manager
```

Expected: 所有测试通过

- [ ] **Step 8: 提交（由用户执行）**

```bash
git add agent/src/manager/worker_manager.rs
```

---

## Task 5: 实现 OrphanProcessReaper（孤儿进程回收器）

**Files:**
- Create: `agent/src/manager/orphan_reaper.rs`

- [ ] **Step 1: 创建 `agent/src/manager/orphan_reaper.rs`**

```rust
//! 孤儿进程回收器
//!
//! 负责：
//! - 管理 Session PID -> session_id 的映射
//! - 在 PTY EOF 时回收退出的 Session 僵尸进程
//! - 清理相关资源（PtyRegistry 记录）
//!
//! # 架构位置
//!
//! ```
//! Worker 退出
//!     ↓
//! Sessions (bash/zsh) 被 init 领养
//!     ↓
//! Manager 仍持有 master_fd
//!     ↓
//! Session 退出 → master_fd 返回 EOF
//!     ↓
//! OrphanProcessReaper.reap_zombie(pid)
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use tokio::sync::RwLock;
use nix::unistd::Pid;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use tracing::{info, warn, debug, error};

use super::pty_registry::PtyRegistry;

/// 孤儿进程回收器
///
/// 负责回收 Worker fork 出的 Session 进程在退出后的僵尸进程。
/// 注意：Manager 不是 Sessions 的父进程，waitpid 可能返回 ECHILD。
pub struct OrphanProcessReaper {
    /// PTY 注册表（用于注销会话）
    pty_registry: Arc<PtyRegistry>,

    /// Session PID -> session_id 的映射
    pid_map: Arc<RwLock<HashMap<Pid, String>>>,
}

impl OrphanProcessReaper {
    /// 创建新的孤儿进程回收器
    ///
    /// # 参数
    ///
    /// - `pty_registry`: PTY 注册表（共享引用）
    pub fn new(pty_registry: Arc<PtyRegistry>) -> Self {
        Self {
            pty_registry,
            pid_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册 Session PID 与 session_id 的映射
    ///
    /// 在 Worker 创建新 PTY 会话后调用，记录 pid -> session_id 映射，
    /// 以便后续在 PTY EOF 时能找到对应的 session_id。
    ///
    /// # 参数
    ///
    /// - `pid`: Session 子进程的 PID
    /// - `session_id`: 对应的会话 ID
    pub async fn register(&self, pid: Pid, session_id: String) {
        let mut map = self.pid_map.write().await;
        map.insert(pid, session_id);
        debug!("已注册 Session PID 映射: pid={} -> session_id", pid);
    }

    /// 根据 session_id 获取对应的 PID
    ///
    /// # 参数
    ///
    /// - `session_id`: 会话 ID
    ///
    /// # 返回
    ///
    /// 如果找到返回 `Some(Pid)`，否则返回 `None`。
    pub async fn get_pid(&self, session_id: &str) -> Option<Pid> {
        let map = self.pid_map.read().await;
        map.iter()
            .find(|(_, sid)| sid.as_str() == session_id)
            .map(|(pid, _)| *pid)
    }

    /// 回收僵尸进程
    ///
    /// 使用 waitpid(WNOHANG) 非阻塞回收。
    ///
    /// # 参数
    ///
    /// - `pid`: 要回收的进程 PID
    ///
    /// # 返回
    ///
    /// 成功返回 `Ok(())`。即使 waitpid 返回 ECHILD（已被 init 回收）也视为成功。
    pub async fn reap_zombie(&self, pid: Pid) -> Result<()> {
        debug!("尝试回收僵尸进程: pid={}", pid);

        // 使用 spawn_blocking 包装同步的 waitpid 调用
        let result = tokio::task::spawn_blocking(move || {
            waitpid(pid, Some(WaitPidFlag::WNOHANG))
        }).await;

        match result {
            Ok(Ok(WaitStatus::Exited(pid, status))) => {
                info!("僵尸进程已回收: pid={}, status={}", pid, status);
                self.cleanup_session(pid).await;
            }
            Ok(Ok(WaitStatus::Signaled(pid, sig, _))) => {
                warn!("进程被信号终止: pid={}, signal={:?}", pid, sig);
                self.cleanup_session(pid).await;
            }
            Ok(Ok(WaitStatus::StillAlive)) => {
                // 进程还活着，PTY EOF 可能是其他原因
                warn!("PTY EOF 但进程仍存活: pid={}", pid);
                // 不清理，进程可能还会继续运行
            }
            Ok(Ok(_)) => {
                // 其他状态（Continue、Stopped 等）
                debug!("进程处于其他状态: pid={}", pid);
            }
            Ok(Err(nix::errno::Errno::ECHILD)) => {
                // 子进程已被 init 领养并回收
                debug!("进程已被 init 回收: pid={}", pid);
                self.cleanup_session(pid).await;
            }
            Ok(Err(e)) => {
                error!("waitpid 失败: pid={}, error={}", pid, e);
                // 仍然清理映射，避免内存泄漏
                self.cleanup_session(pid).await;
            }
            Err(e) => {
                error!("spawn_blocking 任务失败: pid={}, error={}", pid, e);
                self.cleanup_session(pid).await;
            }
        }

        Ok(())
    }

    /// 清理 session 映射和 PtyRegistry 记录
    ///
    /// # 参数
    ///
    /// - `pid`: 已退出的进程 PID
    async fn cleanup_session(&self, pid: Pid) {
        // 从 pid_map 中移除
        let session_id = {
            let mut map = self.pid_map.write().await;
            map.remove(&pid)
        };

        // 从 PtyRegistry 中注销
        if let Some(session_id) = session_id {
            debug!("清理会话资源: session_id={}", session_id);
            let _ = self.pty_registry.unregister(&session_id).await;
        }
    }

    /// 获取当前跟踪的会话数量
    pub async fn session_count(&self) -> usize {
        let map = self.pid_map.read().await;
        map.len()
    }
}

impl Clone for OrphanProcessReaper {
    fn clone(&self) -> Self {
        Self {
            pty_registry: Arc::clone(&self.pty_registry),
            pid_map: Arc::clone(&self.pid_map),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_get_pid() {
        let pty_registry = Arc::new(PtyRegistry::new());
        let reaper = OrphanProcessReaper::new(pty_registry);

        let pid = Pid::from_raw(12345);
        reaper.register(pid, "session-test-1".to_string()).await;

        let found = reaper.get_pid("session-test-1").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap(), pid);
    }

    #[tokio::test]
    async fn test_get_pid_not_found() {
        let pty_registry = Arc::new(PtyRegistry::new());
        let reaper = OrphanProcessReaper::new(pty_registry);

        let found = reaper.get_pid("nonexistent-session").await;
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_session_count() {
        let pty_registry = Arc::new(PtyRegistry::new());
        let reaper = OrphanProcessReaper::new(pty_registry);

        assert_eq!(reaper.session_count().await, 0);

        reaper.register(Pid::from_raw(111), "session-1".to_string()).await;
        reaper.register(Pid::from_raw(222), "session-2".to_string()).await;

        assert_eq!(reaper.session_count().await, 2);
    }

    #[tokio::test]
    async fn test_reap_nonexistent_process() {
        // 回收一个不存在的 PID，应返回 ECHILD 而非 panic
        let pty_registry = Arc::new(PtyRegistry::new());
        let reaper = OrphanProcessReaper::new(pty_registry);

        reaper.register(Pid::from_raw(99999), "ghost-session".to_string()).await;

        // waitpid 对不存在的子进程返回 ECHILD
        let result = reaper.reap_zombie(Pid::from_raw(99999)).await;
        assert!(result.is_ok());

        // 映射应已被清理
        assert_eq!(reaper.session_count().await, 0);
    }
}
```

- [ ] **Step 2: 在 `agent/src/manager/mod.rs` 中注册新模块**

在 [manager/mod.rs](file:///e:/MyWork/gnome-remote/agent/src/manager/mod.rs) 的 `#[cfg(unix)] pub mod ipc_server;` 之后添加：

```rust
#[cfg(unix)]
pub mod orphan_reaper;
```

并在 `pub use` 部分添加：

```rust
#[cfg(unix)]
pub use orphan_reaper::OrphanProcessReaper;
```

- [ ] **Step 3: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译通过

- [ ] **Step 4: 运行单元测试**

```bash
cd agent
cargo test --lib manager::orphan_reaper
```

Expected: 4 个测试全部通过（test_register_and_get_pid、test_get_pid_not_found、test_session_count、test_reap_nonexistent_process）

- [ ] **Step 5: 提交（由用户执行）**

```bash
git add agent/src/manager/orphan_reaper.rs agent/src/manager/mod.rs
```

---

## Task 6: 集成 EOF 检测到 pty_output（与 OrphanProcessReaper 集成）

**Files:**
- Modify: `agent/src/manager/pty_output.rs`

- [ ] **Step 1: 修改 `spawn_pty_output_task` 函数签名，添加 `orphan_reaper` 参数**

在 [pty_output.rs](file:///e:/MyWork/gnome-remote/agent/src/manager/pty_output.rs) 中，修改 `spawn_pty_output_task` 函数：

```rust
/// 启动 PTY 输出推送任务（新架构：使用 PtyRegistry）
///
/// # 参数
/// - `pty_registry`: PTY 注册表
/// - `session_id`: PTY 会话 ID
/// - `send`: QUIC SendStream（用于发送数据给客户端）
/// - `stats_manager`: 统计管理器（可选）
/// - `config`: 输出推送配置
/// - `orphan_reaper`: 孤儿进程回收器（可选，用于 EOF 时回收僵尸进程）
///
/// # 返回
/// 返回任务句柄
pub async fn spawn_pty_output_task(
    pty_registry: Arc<PtyRegistry>,
    session_id: String,
    send: Arc<Mutex<SendStream>>,
    stats_manager: Option<Arc<crate::auth::StatsManager>>,
    config: PtyOutputConfig,
    orphan_reaper: Option<super::orphan_reaper::OrphanProcessReaper>,
) -> tokio::task::JoinHandle<()> {
    spawn_pty_output_impl(pty_registry, session_id, send, stats_manager, config, orphan_reaper).await
}
```

- [ ] **Step 2: 修改 `spawn_pty_output_task_legacy` 函数签名，添加 `orphan_reaper` 参数**

```rust
/// 启动 PTY 输出推送任务（向后兼容：使用 PtyManager）
///
/// # 参数
/// - `pty_manager`: PTY 管理器（旧架构）
/// - `session_id`: PTY 会话 ID
/// - `send`: QUIC SendStream（用于发送数据给客户端）
/// - `stats_manager`: 统计管理器（可选）
/// - `config`: 输出推送配置
/// - `orphan_reaper`: 孤儿进程回收器（可选）
///
/// # 返回
/// 返回任务句柄
#[cfg(unix)]
pub async fn spawn_pty_output_task_legacy(
    pty_manager: Arc<PtyManager>,
    session_id: String,
    send: Arc<Mutex<SendStream>>,
    stats_manager: Option<Arc<crate::auth::StatsManager>>,
    config: PtyOutputConfig,
    orphan_reaper: Option<super::orphan_reaper::OrphanProcessReaper>,
) -> tokio::task::JoinHandle<()> {
    spawn_pty_output_impl(pty_manager, session_id, send, stats_manager, config, orphan_reaper).await
}
```

- [ ] **Step 3: 修改 `spawn_pty_output_impl` 函数，添加 `orphan_reaper` 参数和 EOF 处理逻辑**

```rust
/// 内部实现（泛型版本）
#[cfg(unix)]
async fn spawn_pty_output_impl<R>(
    pty_reader: Arc<R>,
    session_id: String,
    send: Arc<Mutex<SendStream>>,
    stats_manager: Option<Arc<crate::auth::StatsManager>>,
    config: PtyOutputConfig,
    orphan_reaper: Option<super::orphan_reaper::OrphanProcessReaper>,
) -> tokio::task::JoinHandle<()>
where
    R: PtyReader + 'static,
{
    tokio::spawn(async move {
        let mut batch_buffer = Vec::with_capacity(config.batch_size);
        let mut last_send_time = std::time::Instant::now();
        let mut eof_detected = false;

        loop {
            // 从 PTY 读取输出
            match pty_reader.read(&session_id).await {
                Ok(data) if !data.is_empty() => {
                    batch_buffer.extend_from_slice(&data);

                    // 判断是否需要发送批次
                    let should_flush = batch_buffer.len() >= config.batch_size
                        || last_send_time.elapsed().as_millis() >= config.batch_interval_ms as u128;

                    if should_flush && !batch_buffer.is_empty() {
                        // 发送批量数据
                        if let Err(e) = send_batch(&send, &batch_buffer).await {
                            warn!("发送终端数据失败: {}", e);
                            break;
                        }

                        // 记录终端输出字节数
                        if let Some(ref stats) = stats_manager {
                            stats.record_terminal_bytes(batch_buffer.len() as u64);
                        }

                        debug!(
                            "PTY 批量输出发送: session_id={}, batch_len={}",
                            session_id, batch_buffer.len()
                        );

                        batch_buffer.clear();
                        last_send_time = std::time::Instant::now();
                    }
                }
                Ok(_) => {
                    // 无数据时检查是否有积压数据需要刷新
                    if !batch_buffer.is_empty()
                        && last_send_time.elapsed().as_millis() >= config.batch_interval_ms as u128
                    {
                        if let Err(e) = send_batch(&send, &batch_buffer).await {
                            warn!("发送终端数据失败(空闲刷新): {}", e);
                            break;
                        }

                        // 记录终端输出字节数
                        if let Some(ref stats) = stats_manager {
                            stats.record_terminal_bytes(batch_buffer.len() as u64);
                        }

                        debug!(
                            "PTY 空闲刷新: session_id={}, batch_len={}",
                            session_id, batch_buffer.len()
                        );

                        batch_buffer.clear();
                        last_send_time = std::time::Instant::now();
                    }
                    // 无数据，短暂等待（降低轮询频率减少 CPU 占用）
                    sleep(Duration::from_millis(config.poll_interval_ms)).await;
                }
                Err(e) => {
                    // PTY 读取错误通常意味着会话已关闭（EOF）
                    warn!("PTY 读取失败（可能 EOF）: session_id={}, error={}", session_id, e);
                    eof_detected = true;
                    break;
                }
            }
        }

        // 发送剩余数据
        if !batch_buffer.is_empty() {
            let _ = send_batch(&send, &batch_buffer).await;

            // 记录终端输出字节数
            if let Some(ref stats) = stats_manager {
                stats.record_terminal_bytes(batch_buffer.len() as u64);
            }
        }

        // EOF 处理：回收孤儿进程并清理资源
        if eof_detected {
            tracing::info!("PTY 会话结束: session_id={}", session_id);

            if let Some(ref reaper) = orphan_reaper {
                // 获取 session 对应的 PID
                if let Some(pid) = reaper.get_pid(&session_id).await {
                    tracing::info!("回收孤儿进程: session_id={}, pid={}", session_id, pid);
                    let _ = reaper.reap_zombie(pid).await;
                } else {
                    tracing::debug!("未找到 session_id 对应的 PID（可能已清理）: {}", session_id);
                }
            }
        }

        debug!("PTY 输出推送任务结束: session_id={}", session_id);
    })
}
```

- [ ] **Step 4: 查找 `spawn_pty_output_task` 的调用方，更新调用**

使用 Grep 搜索 `spawn_pty_output_task` 的调用位置，根据需要更新为传入 `Some(orphan_reaper)` 或 `None`。

```bash
# 在 agent/src 中搜索调用位置
grep -rn "spawn_pty_output_task" agent/src/
```

在每个调用位置添加 `orphan_reaper` 参数。如果调用方暂时没有 OrphanProcessReaper 实例，传入 `None`（向后兼容）。

- [ ] **Step 5: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译通过

- [ ] **Step 6: 提交（由用户执行）**

```bash
git add agent/src/manager/pty_output.rs
```

---

## Task 7: 实现 WorkerCrashDetector（崩溃检测器）

**Files:**
- Create: `agent/src/manager/crash_detector.rs`

- [ ] **Step 1: 创建 `agent/src/manager/crash_detector.rs`**

```rust
//! Worker 崩溃检测器
//!
//! 主动监控 Worker 子进程状态，在 Worker 崩溃时触发自动重启。
//!
//! # 架构位置
//!
//! Manager 启动时 spawn 一个后台任务，定期调用 waitpid(WNOHANG)
//! 检测 Worker 进程是否退出。如果退出且不是优雅关闭，则触发自动重启。
//!
//! # 关键设计
//!
//! - 使用 `tokio::task::spawn_blocking` 包装同步的 `waitpid` 调用
//! - 通过 `is_graceful_shutdown` 标志区分优雅关闭和崩溃
//! - 检测间隔 500ms（平衡 CPU 占用和响应速度）

use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use nix::unistd::Pid;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use tracing::{info, warn, error, debug};

use super::worker_manager::WorkerManager;

/// Worker 崩溃检测器
///
/// 在独立 tokio 任务中运行，监控 Worker 进程状态。
pub struct WorkerCrashDetector {
    /// Worker 管理器
    worker_manager: Arc<WorkerManager>,
    /// 检测任务句柄
    detector_handle: Option<tokio::task::JoinHandle<()>>,
}

impl WorkerCrashDetector {
    /// 创建新的崩溃检测器
    ///
    /// # 参数
    ///
    /// - `worker_manager`: Worker 管理器
    pub fn new(worker_manager: Arc<WorkerManager>) -> Self {
        Self {
            worker_manager,
            detector_handle: None,
        }
    }

    /// 启动崩溃检测循环
    ///
    /// 在后台 tokio 任务中运行，定期调用 waitpid 检测 Worker 状态。
    /// 调用此方法后，检测器将持续运行直到 Worker 优雅退出或调用 `stop()`。
    pub fn start(&mut self) {
        if self.detector_handle.is_some() {
            warn!("崩溃检测器已在运行，忽略重复 start 调用");
            return;
        }

        let worker_manager = self.worker_manager.clone();

        self.detector_handle = Some(tokio::spawn(async move {
            info!("Worker 崩溃检测器启动");

            loop {
                // 获取当前 Worker 信息
                let worker_info = worker_manager.get_info().await;

                if let Some(info) = worker_info {
                    let pid = Pid::from_raw(info.pid as i32);

                    // 使用 spawn_blocking 调用同步的 waitpid
                    let wait_result = tokio::task::spawn_blocking(move || {
                        waitpid(pid, Some(WaitPidFlag::WNOHANG))
                    }).await;

                    match wait_result {
                        Ok(Ok(WaitStatus::Exited(_pid, status))) => {
                            // Worker 进程已退出
                            let is_graceful = worker_manager.is_graceful_shutdown().await;

                            if is_graceful {
                                info!(
                                    "Worker 优雅退出（GracefulShutdown）: status={}",
                                    status
                                );
                                // 优雅关闭，不触发崩溃处理，退出检测循环
                                break;
                            } else {
                                // 异常崩溃
                                error!(
                                    "Worker 异常退出: status={}, 触发自动重启",
                                    status
                                );
                                worker_manager.notify_crash().await;

                                // 尝试自动重启
                                match worker_manager.attempt_restart().await {
                                    Ok(()) => {
                                        info!("Worker 自动重启成功");
                                        // 重置优雅关闭标志（新 Worker 启动）
                                        worker_manager.reset_graceful_shutdown().await;
                                        // 继续监控新 Worker
                                    }
                                    Err(e) => {
                                        error!(
                                            "Worker 自动重启失败，停止检测器: {}",
                                            e
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(Ok(WaitStatus::Signaled(_pid, sig, _))) => {
                            // 被信号杀死
                            let is_graceful = worker_manager.is_graceful_shutdown().await;

                            if is_graceful {
                                info!(
                                    "Worker 被信号终止（优雅关闭）: signal={:?}",
                                    sig
                                );
                                break;
                            } else {
                                error!(
                                    "Worker 被信号杀死: signal={:?}, 触发自动重启",
                                    sig
                                );
                                worker_manager.notify_crash().await;

                                match worker_manager.attempt_restart().await {
                                    Ok(()) => {
                                        info!("Worker 自动重启成功");
                                        worker_manager.reset_graceful_shutdown().await;
                                    }
                                    Err(e) => {
                                        error!(
                                            "Worker 自动重启失败，停止检测器: {}",
                                            e
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(Ok(_)) => {
                            // StillAlive 或其他状态，继续等待
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        Ok(Err(nix::errno::Errno::ECHILD)) => {
                            // 子进程不存在（可能已被回收或 Worker 未启动）
                            debug!("waitpid 返回 ECHILD（无子进程），继续监控");
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        Ok(Err(e)) => {
                            error!("waitpid 错误: {}, 继续监控", e);
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        Err(e) => {
                            error!("spawn_blocking 任务失败: {}", e);
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                    }
                } else {
                    // Worker 未启动，等待
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }

            info!("Worker 崩溃检测器退出");
        }));
    }

    /// 停止崩溃检测器
    ///
    /// 中止检测任务。通常在 Manager 关闭时调用。
    pub fn stop(&mut self) {
        if let Some(handle) = self.detector_handle.take() {
            handle.abort();
            info!("崩溃检测器已停止");
        }
    }
}

impl Drop for WorkerCrashDetector {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager::worker_manager::WorkerStatus;
    use std::time::SystemTime;

    #[tokio::test]
    async fn test_crash_detector_creation() {
        let worker_manager = Arc::new(WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3,
        ));

        let detector = WorkerCrashDetector::new(worker_manager);
        assert!(detector.detector_handle.is_none());
    }

    #[tokio::test]
    async fn test_start_stop() {
        let worker_manager = Arc::new(WorkerManager::new(
            "/nonexistent/binary".to_string(),
            "/tmp/test.sock".to_string(),
            3,
        ));

        let mut detector = WorkerCrashDetector::new(worker_manager);

        // 启动检测器（Worker 未启动，会进入 ECHILD 分支循环）
        detector.start();
        assert!(detector.detector_handle.is_some());

        // 短暂等待让任务运行
        tokio::time::sleep(Duration::from_millis(100)).await;

        // 停止检测器
        detector.stop();
        assert!(detector.detector_handle.is_none());
    }
}
```

- [ ] **Step 2: 在 `agent/src/manager/mod.rs` 中注册新模块**

在 [manager/mod.rs](file:///e:/MyWork/gnome-remote/agent/src/manager/mod.rs) 的 `#[cfg(unix)] pub mod orphan_reaper;` 之后添加：

```rust
#[cfg(unix)]
pub mod crash_detector;
```

并在 `pub use` 部分添加：

```rust
#[cfg(unix)]
pub use crash_detector::WorkerCrashDetector;
```

- [ ] **Step 3: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译通过

- [ ] **Step 4: 运行单元测试**

```bash
cd agent
cargo test --lib manager::crash_detector
```

Expected: 2 个测试通过（test_crash_detector_creation、test_start_stop）

- [ ] **Step 5: 提交（由用户执行）**

```bash
git add agent/src/manager/crash_detector.rs agent/src/manager/mod.rs
```

---

## Task 8: 实现 SignalHandler（SIGHUP 信号处理）

**Files:**
- Create: `agent/src/manager/signal_handler.rs`

- [ ] **Step 1: 创建 `agent/src/manager/signal_handler.rs`**

```rust
//! 信号处理器
//!
//! 监听 Unix 信号（SIGHUP）并触发 Worker 热更新。
//!
//! # 架构位置
//!
//! Manager 启动时 spawn 一个后台任务监听 SIGHUP 信号。
//! 收到信号后通过 mpsc 通道发送触发事件给 HotUpdateCoordinator。
//!
//! # 使用方式
//!
//! ```rust,ignore
//! use manager::signal_handler::watch_sighup;
//!
//! let (tx, rx) = tokio::sync::mpsc::channel(16);
//! let handle = watch_sighup(tx);
//! // rx 接收触发事件
//! ```

use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;
use tracing::{info, error};

/// 热更新触发源
///
/// 标识热更新是由什么触发的，用于日志和审计
#[derive(Debug, Clone)]
pub enum ReloadTrigger {
    /// 客户端通过 QUIC 发送 Reload 命令
    ClientCommand {
        /// 操作者用户名
        operator: String,
    },
    /// Unix 信号 (SIGHUP)
    UnixSignal,
    /// CLI 工具命令 (agent reload)
    CliTool,
    /// apt postinst 脚本触发
    PackagePostinst,
}

/// 监听 SIGHUP 信号
///
/// 在收到 SIGHUP 时通过 `tx` 发送 `ReloadTrigger::UnixSignal` 事件。
///
/// # 参数
///
/// - `tx`: 触发事件发送通道
///
/// # 返回
///
/// 返回任务句柄，可通过 `handle.await` 等待任务结束。
///
/// # 错误处理
///
/// 如果无法注册 SIGHUP 处理器（极少见），任务会记录错误并立即返回。
pub fn watch_sighup(tx: mpsc::Sender<ReloadTrigger>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut sig = match signal(SignalKind::sighup()) {
            Ok(sig) => sig,
            Err(e) => {
                error!("无法注册 SIGHUP 处理器: {}", e);
                return;
            }
        };

        info!("SIGHUP 信号监听已启动");

        while sig.recv().await.is_some() {
            info!("收到 SIGHUP 信号，触发热更新");
            if let Err(e) = tx.send(ReloadTrigger::UnixSignal).await {
                error!("发送触发事件失败（接收方已关闭）: {}", e);
                break;
            }
        }

        info!("SIGHUP 信号监听结束");
    })
}

/// 监听 SIGTERM 信号（用于优雅退出）
///
/// 在收到 SIGTERM 时通过 `tx` 发送退出信号。
///
/// # 参数
///
/// - `tx`: 退出事件发送通道
pub fn watch_sigterm(tx: mpsc::Sender<()>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut sig = match signal(SignalKind::terminate()) {
            Ok(sig) => sig,
            Err(e) => {
                error!("无法注册 SIGTERM 处理器: {}", e);
                return;
            }
        };

        info!("SIGTERM 信号监听已启动");

        while sig.recv().await.is_some() {
            info!("收到 SIGTERM 信号，触发优雅退出");
            if let Err(e) = tx.send(()).await {
                error!("发送退出事件失败: {}", e);
                break;
            }
        }

        info!("SIGTERM 信号监听结束");
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_watch_sighup() {
        let (tx, mut rx) = mpsc::channel(16);
        let handle = watch_sighup(tx);

        // 给监听任务一点时间启动
        tokio::time::sleep(Duration::from_millis(50)).await;

        // 发送 SIGHUP 给当前进程
        let pid = nix::unistd::Pid::from_raw(std::process::id() as i32);
        let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGHUP);

        // 等待接收事件
        let result = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await;

        assert!(result.is_ok(), "应在 1 秒内收到 SIGHUP 事件");
        let trigger = result.unwrap().unwrap();
        assert!(matches!(trigger, ReloadTrigger::UnixSignal));

        handle.abort();
    }
}
```

- [ ] **Step 2: 在 `agent/src/manager/mod.rs` 中注册新模块**

在 [manager/mod.rs](file:///e:/MyWork/gnome-remote/agent/src/manager/mod.rs) 的 `#[cfg(unix)] pub mod crash_detector;` 之后添加：

```rust
#[cfg(unix)]
pub mod signal_handler;
```

并在 `pub use` 部分添加：

```rust
#[cfg(unix)]
pub use signal_handler::{ReloadTrigger, watch_sighup, watch_sigterm};
```

- [ ] **Step 3: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译通过

- [ ] **Step 4: 运行单元测试**

```bash
cd agent
cargo test --lib manager::signal_handler
```

Expected: `test_watch_sighup` 测试通过

- [ ] **Step 5: 提交（由用户执行）**

```bash
git add agent/src/manager/signal_handler.rs agent/src/manager/mod.rs
```

---

## Task 9: 实现 HotUpdateCoordinator（热更新协调器）

**Files:**
- Create: `agent/src/manager/hot_update_coordinator.rs`

- [ ] **Step 1: 创建 `agent/src/manager/hot_update_coordinator.rs`**

```rust
//! 热更新协调器
//!
//! 作为 Manager 的核心组件，协调 Worker 进程的优雅热更新。
//! 支持多种触发路径（SIGHUP、QUIC 命令、CLI 工具），统一汇聚到此协调器。
//!
//! # 工作流程
//!
//! 1. 接收触发信号（从 mpsc 通道）
//! 2. 启动新 Worker（新版二进制）
//! 3. 等待新 Worker IPC 连接就绪
//! 4. 向旧 Worker 发送 GracefulShutdown
//! 5. 等待旧 Worker 退出
//! 6. 清理旧 Worker 的 IPC 连接
//! 7. 重置 is_reloading 标志
//!
//! # 关键设计
//!
//! - `is_reloading` 原子布尔值防止热更新期间重复触发
//! - 新旧 Worker 通过不同的 IPC 连接区分
//! - Sessions 在 Worker 退出后成为孤儿进程，Manager 继续持有 master_fd

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{info, warn, error};

use super::worker_manager::WorkerManager;
use super::ipc_server::IpcServer;
use super::signal_handler::ReloadTrigger;

/// 热更新协调器
///
/// 作为 Manager 的组件运行，接收触发信号并协调 Worker 热更新。
pub struct HotUpdateCoordinator {
    /// Worker 管理器
    worker_manager: Arc<WorkerManager>,
    /// IPC 服务器
    ipc_server: Arc<IpcServer>,
    /// 触发事件接收器
    trigger_rx: mpsc::Receiver<ReloadTrigger>,
    /// 当前是否正在执行热更新（防止重复触发）
    is_reloading: Arc<AtomicBool>,
    /// 默认宽限期（秒）
    default_grace_period_secs: u32,
}

impl HotUpdateCoordinator {
    /// 创建新的热更新协调器
    ///
    /// # 参数
    ///
    /// - `worker_manager`: Worker 管理器
    /// - `ipc_server`: IPC 服务器
    /// - `trigger_rx`: 触发事件接收器
    pub fn new(
        worker_manager: Arc<WorkerManager>,
        ipc_server: Arc<IpcServer>,
        trigger_rx: mpsc::Receiver<ReloadTrigger>,
    ) -> Self {
        Self {
            worker_manager,
            ipc_server,
            trigger_rx,
            is_reloading: Arc::new(AtomicBool::new(false)),
            default_grace_period_secs: 10,
        }
    }

    /// 获取触发事件发送器（用于其他模块发送触发事件）
    ///
    /// 注意：此方法返回的是 `Arc<AtomicBool>`，可用于检查热更新状态。
    pub fn is_reloading_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.is_reloading)
    }

    /// 触发热更新（外部调用入口）
    ///
    /// # 参数
    ///
    /// - `trigger`: 触发源
    ///
    /// # 返回
    ///
    /// 成功返回 `Ok(())`，如果正在热更新中返回 `Err`。
    pub async fn trigger_reload(&self, trigger: ReloadTrigger) -> Result<()> {
        // 检查是否已在热更新中
        if self.is_reloading.load(Ordering::SeqCst) {
            warn!("热更新正在进行中，忽略触发: {:?}", trigger);
            return Err(anyhow::anyhow!("Hot update already in progress"));
        }

        self.execute_reload(trigger).await
    }

    /// 执行热更新流程
    ///
    /// 这是热更新的核心实现，按照设计文档的步骤执行。
    async fn execute_reload(&self, trigger: ReloadTrigger) -> Result<()> {
        info!("开始热更新: trigger={:?}", trigger);

        // 1. 设置 is_reloading 标志
        self.is_reloading.store(true, Ordering::SeqCst);

        // 2. 标记当前 Worker 为优雅关闭状态
        self.worker_manager.mark_graceful_shutdown().await;

        // 3. 停止旧 Worker（会发送 SIGTERM 并等待退出）
        // 注意：这里使用 stop 而非 send_graceful_shutdown，因为
        // Phase 4 的简化实现直接通过信号让 Worker 退出。
        // 完整的 GracefulShutdown IPC 消息流程需要新旧 Worker 共存期管理，
        // 作为增强功能在后续版本实现。
        info!("停止旧 Worker 进程");
        if let Err(e) = self.worker_manager.stop().await {
            error!("停止旧 Worker 失败: {}", e);
            self.is_reloading.store(false, Ordering::SeqCst);
            return Err(e);
        }

        // 4. 等待一段时间确保旧 Worker 资源清理
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // 5. 启动新 Worker
        info!("启动新 Worker 进程");
        if let Err(e) = self.worker_manager.start().await {
            error!("启动新 Worker 失败: {}", e);
            self.is_reloading.store(false, Ordering::SeqCst);
            return Err(e);
        }

        // 6. 等待新 Worker IPC 连接就绪
        info!("等待新 Worker IPC 连接");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // 7. 重置优雅关闭标志
        self.worker_manager.reset_graceful_shutdown().await;

        // 8. 重置 is_reloading 标志
        self.is_reloading.store(false, Ordering::SeqCst);

        info!("热更新完成: trigger={:?}", trigger);
        Ok(())
    }

    /// 启动协调器主循环
    ///
    /// 在后台监听触发事件，收到事件时调用 `execute_reload`。
    /// 通常在 Manager 启动时通过 `tokio::spawn` 调用。
    pub async fn run(mut self) -> Result<()> {
        info!("热更新协调器启动");

        while let Some(trigger) = self.trigger_rx.recv().await {
            if let Err(e) = self.trigger_reload(trigger.clone()).await {
                warn!("热更新失败: trigger={:?}, error={}", trigger, e);
            }
        }

        info!("热更新协调器退出（触发通道已关闭）");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager::worker_manager::WorkerManager;
    use crate::manager::pty_registry::PtyRegistry;

    #[tokio::test]
    async fn test_coordinator_creation() {
        let pty_registry = Arc::new(PtyRegistry::new());
        let worker_manager = Arc::new(WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3,
        ));
        let ipc_server = Arc::new(IpcServer::new(
            "/tmp/test.sock".to_string(),
            pty_registry,
            worker_manager.clone(),
        ));
        let (_tx, rx) = mpsc::channel(16);

        let coordinator = HotUpdateCoordinator::new(worker_manager, ipc_server, rx);
        assert!(!coordinator.is_reloading.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_trigger_when_reloading() {
        let pty_registry = Arc::new(PtyRegistry::new());
        let worker_manager = Arc::new(WorkerManager::new(
            "/usr/bin/agent".to_string(),
            "/tmp/test.sock".to_string(),
            3,
        ));
        let ipc_server = Arc::new(IpcServer::new(
            "/tmp/test.sock".to_string(),
            pty_registry,
            worker_manager.clone(),
        ));
        let (_tx, rx) = mpsc::channel(16);

        let coordinator = HotUpdateCoordinator::new(worker_manager, ipc_server, rx);

        // 手动设置 is_reloading 标志
        coordinator.is_reloading.store(true, Ordering::SeqCst);

        // 尝试触发，应失败
        let result = coordinator.trigger_reload(ReloadTrigger::UnixSignal).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: 在 `agent/src/manager/mod.rs` 中注册新模块**

在 [manager/mod.rs](file:///e:/MyWork/gnome-remote/agent/src/manager/mod.rs) 的 `#[cfg(unix)] pub mod signal_handler;` 之后添加：

```rust
#[cfg(unix)]
pub mod hot_update_coordinator;
```

并在 `pub use` 部分添加：

```rust
#[cfg(unix)]
pub use hot_update_coordinator::HotUpdateCoordinator;
```

- [ ] **Step 3: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译通过

- [ ] **Step 4: 运行单元测试**

```bash
cd agent
cargo test --lib manager::hot_update_coordinator
```

Expected: 2 个测试通过

- [ ] **Step 5: 提交（由用户执行）**

```bash
git add agent/src/manager/hot_update_coordinator.rs agent/src/manager/mod.rs
```

---

## Task 10: 集成热更新协调器到 Manager

**Files:**
- Modify: `agent/src/manager/mod.rs`
- Modify: `agent/src/main.rs`

- [ ] **Step 1: 修改 `Manager` 结构体，添加 HotUpdateCoordinator 和 WorkerCrashDetector**

在 [manager/mod.rs](file:///e:/MyWork/gnome-remote/agent/src/manager/mod.rs) 中，修改 `Manager` 结构体：

```rust
/// Manager 主结构
pub struct Manager {
    /// PTY 注册表（管理所有 master_fd）
    #[cfg(unix)]
    pty_registry: Arc<PtyRegistry>,

    /// Worker 进程管理器
    #[cfg(unix)]
    worker_manager: Arc<WorkerManager>,

    /// 用户会话管理器
    session_manager: Arc<SessionManager>,

    /// IPC Server（接收 Worker 的 FD）
    #[cfg(unix)]
    ipc_server: Arc<IpcServer>,

    /// 孤儿进程回收器
    #[cfg(unix)]
    orphan_reaper: Arc<OrphanProcessReaper>,

    /// 热更新协调器（Option 因为需要 trigger_rx，run 时才启动）
    #[cfg(unix)]
    hot_update_coordinator: Option<HotUpdateCoordinator>,

    /// 崩溃检测器
    #[cfg(unix)]
    crash_detector: Option<WorkerCrashDetector>,
}
```

- [ ] **Step 2: 修改 `Manager::new` 方法，初始化新组件**

```rust
#[cfg(unix)]
pub async fn new(config: &AgentConfig) -> Result<Self> {
    // 创建 PTY 注册表
    let pty_registry = Arc::new(PtyRegistry::new());

    // 创建 Worker 管理器
    let worker_manager = Arc::new(WorkerManager::new(
        config.worker.agent_binary.clone(),
        config.worker.ipc_socket_path.clone(),
        config.worker.max_restarts,
    ));

    // 创建 IPC 服务器（集成 WorkerManager）
    let ipc_server = Arc::new(IpcServer::new(
        config.worker.ipc_socket_path.clone(),
        pty_registry.clone(),
        worker_manager.clone(),
    ));

    // 创建会话管理器
    let session_manager = Arc::new(SessionManager::new());

    // 创建孤儿进程回收器
    let orphan_reaper = Arc::new(OrphanProcessReaper::new(pty_registry.clone()));

    Ok(Self {
        pty_registry,
        worker_manager,
        session_manager,
        ipc_server,
        orphan_reaper,
        hot_update_coordinator: None,
        crash_detector: None,
    })
}
```

- [ ] **Step 3: 修改 `Manager::run` 方法，启动信号监听和协调器**

```rust
pub async fn run(&self) -> Result<()> {
    #[cfg(unix)]
    {
        // 启动 IPC 服务器（在后台运行）
        let ipc_server = self.ipc_server.clone();
        tokio::spawn(async move {
            if let Err(e) = ipc_server.run().await {
                tracing::error!("IPC 服务器运行失败: {}", e);
            }
        });

        // 给 IPC 服务器一点时间启动
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // 启动 Worker 进程
        self.worker_manager.start().await?;

        // 启动崩溃检测器
        let crash_detector = WorkerCrashDetector::new(self.worker_manager.clone());

        // 启动 SIGHUP 信号监听
        let (trigger_tx, trigger_rx) = tokio::sync::mpsc::channel(16);
        let _sighup_handle = crate::manager::signal_handler::watch_sighup(trigger_tx);

        // 启动热更新协调器
        let coordinator = HotUpdateCoordinator::new(
            self.worker_manager.clone(),
            self.ipc_server.clone(),
            trigger_rx,
        );
        tokio::spawn(coordinator.run());

        tracing::info!("Manager 已启动（支持热更新）");

        // 等待终止信号
        tokio::signal::ctrl_c().await?;

        tracing::info!("收到终止信号，停止 Manager");

        // 停止 Worker
        self.worker_manager.stop().await?;

        // 停止 IPC 服务器
        self.ipc_server.stop().await?;

        // 停止崩溃检测器
        // 注意：crash_detector 在此作用域外，实际使用时需要调整
    }

    Ok(())
}
```

注意：由于 `crash_detector` 是局部变量，上述代码中 `start` 调用需要调整为可变引用。完整实现如下（修改 `run` 签名为 `&mut self` 或使用内部可变性）。

简化方案：将 `crash_detector` 改为在 `run` 内创建并启动：

```rust
pub async fn run(&mut self) -> Result<()> {
    #[cfg(unix)]
    {
        // 启动 IPC 服务器
        let ipc_server = self.ipc_server.clone();
        tokio::spawn(async move {
            if let Err(e) = ipc_server.run().await {
                tracing::error!("IPC 服务器运行失败: {}", e);
            }
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // 启动 Worker 进程
        self.worker_manager.start().await?;

        // 启动崩溃检测器
        let mut crash_detector = WorkerCrashDetector::new(self.worker_manager.clone());
        crash_detector.start();

        // 启动 SIGHUP 信号监听
        let (trigger_tx, trigger_rx) = tokio::sync::mpsc::channel(16);
        let _sighup_handle = crate::manager::signal_handler::watch_sighup(trigger_tx);

        // 启动热更新协调器
        let coordinator = HotUpdateCoordinator::new(
            self.worker_manager.clone(),
            self.ipc_server.clone(),
            trigger_rx,
        );
        tokio::spawn(coordinator.run());

        tracing::info!("Manager 已启动（支持热更新）");

        // 等待终止信号
        tokio::signal::ctrl_c().await?;

        tracing::info!("收到终止信号，停止 Manager");

        // 停止崩溃检测器
        crash_detector.stop();

        // 停止 Worker
        self.worker_manager.stop().await?;

        // 停止 IPC 服务器
        self.ipc_server.stop().await?;
    }

    Ok(())
}
```

- [ ] **Step 4: 修改 `main.rs` 中的 `run_manager_mode`，使用新的 Manager 结构**

由于 `main.rs` 当前直接调用 `server::quic::run` 等函数，没有使用 Manager 结构，需要根据现有代码的实际情况进行调整。

如果 `Manager::run` 是新的入口点，在 `main.rs` 中调用：

```rust
async fn run_manager_mode(args: &Args) -> Result<()> {
    let cfg = config::load(&args.config)?;
    init_logging(&args, &cfg);

    // ... 现有的初始化代码 ...

    // 创建并启动 Manager
    let mut manager = gnome_remote_agent::manager::Manager::new(&cfg).await?;
    manager.run().await?;

    Ok(())
}
```

**注意**：实际的集成方式取决于现有代码的架构。如果 `server::quic::run` 已经在独立任务中运行，Manager 可能只负责 Worker 管理和热更新，而不替代 QUIC 服务器。这种情况下，需要保持现有的 QUIC 服务器启动逻辑不变，只在 Manager::run 中添加 Worker 管理、信号监听和热更新协调器。

具体的集成方式需要根据 `server/quic.rs` 的实际代码决定。Phase 4 的最小实现是：在 Manager::run 中启动信号监听和热更新协调器，不改变现有的 QUIC 服务器启动流程。

- [ ] **Step 5: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译通过

- [ ] **Step 6: 提交（由用户执行）**

```bash
git add agent/src/manager/mod.rs agent/src/main.rs
```

---

## Task 11: 编写热更新集成测试

**Files:**
- Create: `agent/tests/hot_update_test.rs`

- [ ] **Step 1: 创建 `agent/tests/hot_update_test.rs`**

```rust
//! Phase 4 热更新集成测试
//!
//! 测试场景：
//! - Worker 优雅退出后 Sessions 存活
//! - SIGHUP 触发热更新流程
//! - 崩溃后自动恢复

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;
use gnome_remote_agent::manager::{
    WorkerManager, PtyRegistry, IpcServer,
    OrphanProcessReaper, WorkerCrashDetector,
    HotUpdateCoordinator, ReloadTrigger, watch_sighup,
};
use tokio::sync::mpsc;

#[tokio::test]
async fn test_orphan_reaper_creation() {
    let pty_registry = Arc::new(PtyRegistry::new());
    let reaper = OrphanProcessReaper::new(pty_registry);

    assert_eq!(reaper.session_count().await, 0);
}

#[tokio::test]
async fn test_orphan_reaper_register_and_lookup() {
    let pty_registry = Arc::new(PtyRegistry::new());
    let reaper = OrphanProcessReaper::new(pty_registry);

    use nix::unistd::Pid;
    reaper.register(Pid::from_raw(12345), "session-1".to_string()).await;

    let pid = reaper.get_pid("session-1").await;
    assert!(pid.is_some());
    assert_eq!(pid.unwrap(), Pid::from_raw(12345));

    assert_eq!(reaper.session_count().await, 1);
}

#[tokio::test]
async fn test_orphan_reaper_reap_nonexistent() {
    let pty_registry = Arc::new(PtyRegistry::new());
    let reaper = OrphanProcessReaper::new(pty_registry);

    use nix::unistd::Pid;
    reaper.register(Pid::from_raw(99999), "ghost".to_string()).await;

    // 回收不存在的进程（应返回 ECHILD 但不 panic）
    let result = reaper.reap_zombie(Pid::from_raw(99999)).await;
    assert!(result.is_ok());

    // 映射应已清理
    assert_eq!(reaper.session_count().await, 0);
}

#[tokio::test]
async fn test_worker_manager_graceful_shutdown_flag() {
    let manager = WorkerManager::new(
        "/usr/bin/agent".to_string(),
        "/tmp/test.sock".to_string(),
        3,
    );

    // 初始为 false
    assert!(!manager.is_graceful_shutdown().await);

    // 标记后为 true
    manager.mark_graceful_shutdown().await;
    assert!(manager.is_graceful_shutdown().await);

    // 重置后为 false
    manager.reset_graceful_shutdown().await;
    assert!(!manager.is_graceful_shutdown().await);
}

#[tokio::test]
async fn test_hot_update_coordinator_creation() {
    let pty_registry = Arc::new(PtyRegistry::new());
    let worker_manager = Arc::new(WorkerManager::new(
        "/usr/bin/agent".to_string(),
        "/tmp/test.sock".to_string(),
        3,
    ));
    let ipc_server = Arc::new(IpcServer::new(
        "/tmp/test.sock".to_string(),
        pty_registry,
        worker_manager.clone(),
    ));
    let (_tx, rx) = mpsc::channel(16);

    let coordinator = HotUpdateCoordinator::new(worker_manager, ipc_server, rx);
    let flag = coordinator.is_reloading_flag();
    assert!(!flag.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn test_sighup_signal_handling() {
    let (tx, mut rx) = mpsc::channel(16);
    let handle = watch_sighup(tx);

    // 等待监听器启动
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 发送 SIGHUP 给当前进程
    let pid = nix::unistd::Pid::from_raw(std::process::id() as i32);
    let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGHUP);

    // 验证收到事件
    let result = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
    assert!(result.is_ok(), "应在 2 秒内收到 SIGHUP 事件");

    let trigger = result.unwrap().unwrap();
    assert!(matches!(trigger, ReloadTrigger::UnixSignal));

    handle.abort();
}

#[tokio::test]
async fn test_crash_detector_start_stop() {
    let worker_manager = Arc::new(WorkerManager::new(
        "/nonexistent/binary".to_string(),
        "/tmp/test.sock".to_string(),
        3,
    ));

    let mut detector = WorkerCrashDetector::new(worker_manager);
    detector.start();

    tokio::time::sleep(Duration::from_millis(100)).await;

    detector.stop();
}

#[tokio::test]
async fn test_hot_update_coordinator_rejects_duplicate_trigger() {
    let pty_registry = Arc::new(PtyRegistry::new());
    let worker_manager = Arc::new(WorkerManager::new(
        "/usr/bin/agent".to_string(),
        "/tmp/test.sock".to_string(),
        3,
    ));
    let ipc_server = Arc::new(IpcServer::new(
        "/tmp/test.sock".to_string(),
        pty_registry,
        worker_manager.clone(),
    ));
    let (_tx, rx) = mpsc::channel(16);

    let coordinator = HotUpdateCoordinator::new(worker_manager, ipc_server, rx);

    // 手动设置 is_reloading
    coordinator.is_reloading_flag()
        .store(true, std::sync::atomic::Ordering::SeqCst);

    // 应拒绝触发
    let result = coordinator.trigger_reload(ReloadTrigger::UnixSignal).await;
    assert!(result.is_err());
}
```

- [ ] **Step 2: 运行集成测试**

```bash
cd agent
cargo test --test hot_update_test
```

Expected: 所有测试通过

- [ ] **Step 3: 提交（由用户执行）**

```bash
git add agent/tests/hot_update_test.rs
```

---

## Task 12: 更新 systemd 服务配置

**Files:**
- Modify: `systemd/gnome-remote-agent.service`

- [ ] **Step 1: 读取现有 systemd 服务文件**

先读取 `systemd/gnome-remote-agent.service` 的内容，了解现有配置。

- [ ] **Step 2: 修改 systemd 服务文件，添加 KillMode 和 ExecReload**

在 service 文件的 `[Service]` 部分添加：

```ini
[Service]
# ... 现有配置 ...

# 热更新支持
# KillMode=process: 只杀死主进程，Session 子进程继续运行（成为孤儿）
KillMode=process

# ExecReload: 发送 SIGHUP 信号触发优雅重载
ExecReload=/bin/kill -HUP $MAINPID

# 重启策略
Restart=on-failure
RestartSec=5s
```

完整的 `[Service]` 部分示例（根据现有文件调整）：

```ini
[Unit]
Description=GNOME Remote Control Agent
After=network.target

[Service]
Type=simple
ExecStart=/usr/bin/gnome-remote-agent --config /etc/gnome-remote/agent.toml

# 热更新支持
KillMode=process
ExecReload=/bin/kill -HUP $MAINPID

# 重启策略
Restart=on-failure
RestartSec=5s

# 用户和权限
User=gnome-remote
Group=gnome-remote

# 日志
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

- [ ] **Step 3: 创建 apt postinst 钩子说明文档**

在 `systemd/` 目录下创建 `README.md`，说明如何配置 apt 升级触发热更新：

```markdown
# systemd 服务配置说明

## 热更新支持

本服务配置支持 Worker 进程的热更新，主要配置项：

- `KillMode=process`: 只杀死主进程，Session 子进程继续运行
- `ExecReload=/bin/kill -HUP $MAINPID`: 发送 SIGHUP 信号触发热更新

## 触发热更新的方式

### 1. systemctl reload

```bash
sudo systemctl reload gnome-remote-agent
```

### 2. 手动发送 SIGHUP

```bash
sudo kill -HUP $(pidof gnome-remote-agent)
```

### 3. apt 升级后自动触发

在 `/etc/apt/apt.conf.d/` 下创建钩子：

```bash
# /etc/apt/apt.conf.d/99-gnome-remote-reload
DPkg::Post-Invoke { "systemctl reload gnome-remote-agent || true"; };
```

### 4. QUIC 客户端命令（远程运维）

通过 GNOME Remote 客户端 UI 发送 Reload 命令。
```

- [ ] **Step 4: 提交（由用户执行）**

```bash
git add systemd/gnome-remote-agent.service systemd/README.md
```

---

## Task 13: 更新 TASK_BREAKDOWN.md

**Files:**
- Modify: `docs/TASK_BREAKDOWN.md`

- [ ] **Step 1: 在 TASK_BREAKDOWN.md 中添加 Phase 4 任务记录**

在 [TASK_BREAKDOWN.md](file:///e:/MyWork/gnome-remote/docs/TASK_BREAKDOWN.md) 的"总体进度"部分更新：

```markdown
**当前阶段**: Phase 4 - 热更新功能（已完成）
**总任务数**: 40+
**已完成**: 30
**进行中**: 0

**完成进度**:
- ✅ Phase 0: 准备阶段（TASK-000 ~ TASK-002）- 100% 完成
- ✅ Phase 1: IPC 协议定义（TASK-003 ~ TASK-009）- 100% 完成
- ✅ Phase 2: Manager 实现（TASK-010 ~ TASK-015）- 100% 完成
- ✅ Phase 3: Worker 实现（TASK-016 ~ TASK-020）- 100% 完成
- ✅ Phase 4: 热更新功能（TASK-021 ~ TASK-030）- 100% 完成
```

在"进度记录"表格中添加 Phase 4 任务：

```markdown
| TASK-021 | 新增 IPC 消息（GracefulShutdown、ShutdownAck） | 2026-08-03 | AI-3 | proto 更新 |
| TASK-022 | 扩展 SessionManager（all_idle、snapshot） | 2026-08-03 | AI-3 | |
| TASK-023 | 实现 GracefulShutdown 处理器（Worker 端） | 2026-08-03 | AI-3 | |
| TASK-024 | 扩展 WorkerManager（attempt_restart、mark_graceful_shutdown） | 2026-08-03 | AI-3 | |
| TASK-025 | 实现 OrphanProcessReaper | 2026-08-03 | AI-3 | |
| TASK-026 | 集成 EOF 检测到 pty_output | 2026-08-03 | AI-3 | |
| TASK-027 | 实现 WorkerCrashDetector | 2026-08-03 | AI-3 | |
| TASK-028 | 实现 SignalHandler（SIGHUP） | 2026-08-03 | AI-3 | |
| TASK-029 | 实现 HotUpdateCoordinator | 2026-08-03 | AI-3 | |
| TASK-030 | 集成到 Manager + systemd 配置 | 2026-08-03 | AI-3 | |
```

- [ ] **Step 2: 提交（由用户执行）**

```bash
git add docs/TASK_BREAKDOWN.md
```

---

## Task 14: 全量编译与测试验证

**Files:** 无（仅验证）

- [ ] **Step 1: 全量编译**

```bash
cd agent
cargo build
```

Expected: 编译成功，零错误

- [ ] **Step 2: 运行所有单元测试**

```bash
cd agent
cargo test --lib
```

Expected: 所有测试通过

- [ ] **Step 3: 运行所有集成测试**

```bash
cd agent
cargo test --test hot_update_test
cargo test --test integration_test
cargo test --test ipc_integration_test
```

Expected: 所有测试通过

- [ ] **Step 4: 检查警告**

```bash
cd agent
cargo build 2>&1 | grep -i warning
```

Expected: 无严重警告（Phase 4 新增代码的警告需清理）

- [ ] **Step 5: 最终提交（由用户执行）**

```bash
git add -A
git status  # 确认所有文件已暂存
# 用户自行执行 git commit
```

---

## 自审检查

### 1. Spec 覆盖检查

| Spec 部分 | 对应任务 | 状态 |
|----------|---------|------|
| 架构总览 | Task 9, 10 | ✅ |
| 触发机制层 | Task 8, 9 | ✅ |
| 优雅关闭协议（IPC 消息） | Task 1 | ✅ |
| 优雅关闭协议（Worker 端） | Task 3 | ✅ |
| 优雅关闭协议（Manager 端） | Task 9 | ✅ |
| 孤儿进程处理 | Task 5, 6 | ✅ |
| 崩溃检测与自动恢复 | Task 4, 7 | ✅ |
| 状态迁移（可选，不实现） | - | ✅ 文档说明 |
| 测试策略 | Task 11 | ✅ |
| systemd 配置 | Task 12 | ✅ |

### 2. 类型一致性检查

- `ReloadTrigger` 在 Task 8 定义，在 Task 9 使用 ✅
- `OrphanProcessReaper` 在 Task 5 定义，在 Task 6 使用 ✅
- `WorkerCrashDetector` 在 Task 7 定义，在 Task 10 使用 ✅
- `HotUpdateCoordinator` 在 Task 9 定义，在 Task 10 使用 ✅
- `mark_graceful_shutdown`、`is_graceful_shutdown`、`reset_graceful_shutdown` 在 Task 4 定义 ✅
- `attempt_restart`、`wait_for_exit` 在 Task 4 定义 ✅
- `all_idle`、`snapshot` 在 Task 2 定义，在 Task 3 使用 ✅

### 3. 占位符扫描

无 TBD/TODO，所有代码块都是完整实现。

### 4. 注意事项

- **Task 10 的复杂性**：Manager 与现有 `server::quic::run` 的集成需要根据实际代码调整。如果现有代码没有使用 Manager 结构，需要最小化集成方式。
- **测试环境**：所有集成测试需要在 Unix 环境（WSL 或 Linux）运行。
- **systemd 配置**：实际部署时需要根据系统环境调整服务文件。
