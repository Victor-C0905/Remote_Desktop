# 阶段 1:IPC 骨架打通 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 `main.rs::run_manager_mode` 真正实例化 `Manager` 并启动 Worker 子进程,建立 IPC 通道,同时不影响现有 QUIC/WebSocket 功能。

**Architecture:** 在 main.rs 中实例化 Manager,调用新增的非阻塞 `Manager::start()` 方法启动 IPC 服务器 + Worker 子进程 + 崩溃检测器。PTY 仍走 quic.rs 旧路径。Manager/Worker 空跑验证 IPC 通道可用。

**Tech Stack:** Rust 1.96.0 (edition 2024), Tokio, nix (Unix), prost (Protobuf), WSL 环境运行

**设计文档:** `docs/superpowers/specs/2026-08-04-manager-worker-integration-design.md`

---

## 文件结构

| 文件 | 操作 | 职责 |
|------|------|------|
| `agent/src/manager/mod.rs` | 修改 | 新增 `Manager::start()` 和 `Manager::shutdown()` 非阻塞方法 |
| `agent/src/main.rs` | 修改 | `run_manager_mode` 实例化 Manager 并启动 |
| `agent/src/manager/worker_manager.rs` | 不修改 | WorkerManager::start/stop 已就绪 |
| `agent/src/manager/ipc_server.rs` | 不修改 | IpcServer::run/stop 已就绪 |
| `agent/src/manager/crash_detector.rs` | 不修改 | WorkerCrashDetector::start/stop 已就绪 |
| `agent/tests/phase1_integration_test.rs` | 新增 | 验证 Manager 启动、Worker 进程、IPC 连接 |

---

## 前置条件

- WSL 环境可用(Rust 1.96.0)
- 当前代码 `cargo build` 通过
- 配置文件 `agent.toml` 中 `[worker]` 段配置正确

---

## Task 1:新增 `Manager::start()` 非阻塞启动方法

**Files:**
- Modify: `agent/src/manager/mod.rs:151-205`(在现有 `run` 方法后新增)

- [ ] **Step 1:阅读现有 `Manager::run` 方法**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && sed -n '151,205p' src/manager/mod.rs"`
Expected: 显示 `run` 方法,包含 IPC 启动、Worker 启动、CrashDetector 启动、SIGHUP、HotUpdateCoordinator、ctrl_c 等待

- [ ] **Step 2:在 `manager/mod.rs` 的 `impl Manager` 块中,`run` 方法之后新增 `start` 和 `shutdown` 方法**

在 [manager/mod.rs](file:///e:/MyWork/gnome-remote/agent/src/manager/mod.rs) 的 `run` 方法结束(第 205 行 `}`)之后,`handle_resize_window` 方法之前插入:

```rust
    /// 启动 Manager(非阻塞)
    ///
    /// 启动 IPC 服务器、Worker 子进程、崩溃检测器,然后立即返回。
    /// 不等待 ctrl_c 信号,由调用方决定何时调用 `shutdown`。
    ///
    /// # 流程
    /// 1. 启动 IPC 服务器(后台)
    /// 2. 启动 Worker 子进程
    /// 3. 启动崩溃检测器(后台)
    ///
    /// # 返回
    /// 成功返回 Ok(()),失败返回错误
    #[cfg(unix)]
    pub async fn start(&mut self) -> Result<()> {
        // 启动 IPC 服务器(在后台运行)
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
        // 在后台监控 Worker 进程状态,崩溃时自动重启
        let mut crash_detector = WorkerCrashDetector::new(self.worker_manager.clone());
        crash_detector.start();
        // 注意:crash_detector 在此作用域结束时会被 drop,但 start() 内部 spawn 的任务
        // 会继续运行(JoinHandle 被 abort 需要 stop())。为保留崩溃检测能力,
        // 我们将 crash_detector 存入 self。
        self.crash_detector = Some(crash_detector);

        tracing::info!("Manager 已启动(IPC + Worker + CrashDetector)");

        Ok(())
    }

    /// 停止 Manager(非阻塞)
    ///
    /// 停止崩溃检测器、Worker 进程、IPC 服务器。
    /// 在调用方决定关闭时调用。
    #[cfg(unix)]
    pub async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("正在停止 Manager");

        // 停止崩溃检测器
        if let Some(mut detector) = self.crash_detector.take() {
            detector.stop();
        }

        // 停止 Worker
        self.worker_manager.stop().await?;

        // 停止 IPC 服务器
        self.ipc_server.stop().await?;

        tracing::info!("Manager 已停止");
        Ok(())
    }
```

- [ ] **Step 3:在 `Manager` 结构体中新增 `crash_detector` 字段**

在 [manager/mod.rs:66-86](file:///e:/MyWork/gnome-remote/agent/src/manager/mod.rs#L66-L86) 的 `Manager` 结构体中,`orphan_reaper` 字段之后新增:

```rust
    /// 崩溃检测器
    /// 阶段 1:启动后持续监控 Worker 进程状态
    #[cfg(unix)]
    crash_detector: Option<WorkerCrashDetector>,
```

- [ ] **Step 4:在 `Manager::new` 的 `Ok(Self { ... })` 中初始化 `crash_detector`**

在 [manager/mod.rs:122-128](file:///e:/MyWork/gnome-remote/agent/src/manager/mod.rs#L122-L128) 的 `Ok(Self { ... })` 中,`orphan_reaper` 之后新增:

```rust
            crash_detector: None,
```

- [ ] **Step 4b:新增 `worker_info` 公共 getter 方法,并 re-export `WorkerInfo`**

首先在 [manager/mod.rs:44](file:///e:/MyWork/gnome-remote/agent/src/manager/mod.rs#L44) 的 `pub use worker_manager::{WorkerManager, WorkerStatus, WorkerStatusEvent};` 修改为:

```rust
pub use worker_manager::{WorkerManager, WorkerInfo, WorkerStatus, WorkerStatusEvent};
```

然后在 `start` 方法之前(或 `handle_resize_window` 之后)新增:

```rust
    /// 获取 Worker 进程信息(供外部测试和监控使用)
    #[cfg(unix)]
    pub async fn worker_info(&self) -> Option<WorkerInfo> {
        self.worker_manager.get_info().await
    }
```

- [ ] **Step 5:验证编译**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过,可能有无关的 warning,但无 error

- [ ] **Step 6:提示用户提交**

```bash
git add agent/src/manager/mod.rs
git commit -m "feat(manager): 新增 Manager::start/shutdown 非阻塞方法"
```

---

## Task 2:修改 `main.rs` 实例化 Manager

**Files:**
- Modify: `agent/src/main.rs:200-252`(`run_manager_mode` 函数)

- [ ] **Step 1:阅读当前 `run_manager_mode` 函数**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && sed -n '200,252p' src/main.rs"`
Expected: 显示函数,包含 config 加载、cert、event_bus、subscription_manager、pty_manager、audit_log、authenticator、try_join!

- [ ] **Step 2:在 `main.rs` 顶部新增 Manager 导入**

在 [main.rs:8](file:///e:/MyWork/gnome-remote/agent/src/main.rs#L8) 的 `use gnome_remote_agent::{config, cert, event_bus, subscription, pty, audit, server};` 之后新增:

```rust
#[cfg(unix)]
use gnome_remote_agent::manager::Manager;
```

- [ ] **Step 3:修改 `run_manager_mode` 函数,在 `try_join!` 之前实例化并启动 Manager**

替换 [main.rs:200-252](file:///e:/MyWork/gnome-remote/agent/src/main.rs#L200-L252) 的 `run_manager_mode` 函数为:

```rust
/// Manager 模式入口
///
/// 启动 Manager(网关层)+ QUIC 服务器 + WebSocket 服务器。
/// 阶段 1:Manager 启动 IPC 服务器 + Worker 子进程 + 崩溃检测器,
/// 但 PTY 仍走 quic.rs 旧路径,Manager/Worker 空跑验证 IPC 通道。
async fn run_manager_mode(args: &Args) -> Result<()> {
    let cfg = config::load(&args.config)?;

    init_logging(&args, &cfg);

    let (cert, key) = cert::ensure_certificate(&cfg)?;

    // 日志模式标识
    let log_mode = if std::env::var("RUST_LOG").is_ok() { "debug (RUST_LOG)" } else { "production" };
    tracing::info!("GNOME Remote Agent 启动中...");
    tracing::info!("   日志模式: {}", log_mode);
    tracing::info!("   日志级别: {}", cfg.log.level);
    tracing::info!("   日志目录: {}", if cfg.log.dir == "off" { "关闭".to_string() } else { cfg.log.dir.clone() });
    tracing::info!("   QUIC  监听: udp://{}:{}", cfg.server.bind, cfg.server.quic_port);
    tracing::info!("   WS    监听: tcp://{}:{}", cfg.server.bind, cfg.server.ws_port);

    // 创建 EventBus 和 SubscriptionManager
    let event_bus = Arc::new(event_bus::EventBus::new());
    let subscription_manager = Arc::new(subscription::SubscriptionManager::new(cfg.clone(), event_bus.clone()));

    // 创建 PTY 管理器(阶段 1 临时保留,阶段 2 由 Worker 接管)
    let pty_manager = Arc::new(pty::PtyManager::new());

    // 初始化审计日志
    let audit_log = Arc::new(audit::AuditLogger::new(&cfg.audit.log_path)
        .expect("无法创建审计日志文件"));
    tracing::info!("审计日志已启用: {}", cfg.audit.log_path);

    // 初始化认证器
    let authenticator = Arc::new(CompositeAuthenticator::new(
        cfg.auth.ssh.pam_service.clone(),
        cfg.auth.ssh.enable_pubkey,
        cfg.auth.ssh.enable_password,
    ));
    tracing::info!("认证器已初始化 (公钥认证: {}, 密码认证: {})",
        cfg.auth.ssh.enable_pubkey, cfg.auth.ssh.enable_password);

    // 阶段 1 新增:实例化并启动 Manager(IPC + Worker + CrashDetector)
    #[cfg(unix)]
    let mut manager = {
        let m = Manager::new(&cfg).await?;
        tracing::info!("Manager 已实例化,正在启动...");
        m
    };
    #[cfg(unix)]
    manager.start().await?;

    let key_clone = key.clone_key();
    let result = tokio::try_join!(
        server::quic::run(
            cfg.clone(),
            cert.clone(),
            key_clone,
            subscription_manager.clone(),
            event_bus.clone(),
            pty_manager.clone(),
            authenticator.clone(),
            audit_log.clone(),
        ),
        server::websocket::run(cfg.clone(), cert, key),
    );

    // 阶段 1 新增:QUIC/WS 退出后停止 Manager
    #[cfg(unix)]
    manager.shutdown().await?;

    result?;
    Ok(())
}
```

- [ ] **Step 4:验证编译**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过,可能有无关 warning

- [ ] **Step 5:提示用户提交**

```bash
git add agent/src/main.rs
git commit -m "feat(main): 实例化 Manager 并启动 IPC + Worker(阶段 1)"
```

---

## Task 3:验证 Worker 子进程启动

**Files:**
- 无文件修改,仅运行验证

- [ ] **Step 1:编译 release 版本**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo build --release 2>&1 | tail -5"`
Expected: 编译成功,生成 `target/release/agent`

- [ ] **Step 2:检查 agent 二进制文件存在**

Run: `wsl -e bash -l -c "ls -la /mnt/e/MyWork/gnome-remote/agent/target/release/agent"`
Expected: 文件存在,大小约 5-10MB

- [ ] **Step 3:后台启动 agent,观察日志**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && timeout 5 ./target/release/agent --config agent.toml --log-level info 2>&1 | head -30"`
Expected: 日志包含:
- `GNOME Remote Agent 启动中...`
- `Manager 已实例化,正在启动...`
- `IPC 服务器已启动: path=/tmp/gnome-remote-worker.sock`
- `Worker 进程已启动: pid=XXXX, binary=...`
- `Worker 崩溃检测器启动`
- `Manager 已启动(IPC + Worker + CrashDetector)`
- `QUIC  监听: udp://0.0.0.0:8443`

- [ ] **Step 4:验证 Worker 子进程存在**

在另一个终端(在 agent 运行期间)运行:
Run: `wsl -e bash -l -c "ps aux | grep '[a]gent' | head -5"`
Expected: 显示两个进程:
- 主进程:`./target/release/agent --config agent.toml`
- Worker 子进程:`./target/release/agent --worker --ipc-socket /tmp/gnome-remote-worker.sock`

- [ ] **Step 5:验证 IPC Socket 文件存在**

Run: `wsl -e bash -l -c "ls -la /tmp/gnome-remote-worker.sock"`
Expected: socket 文件存在

---

## Task 4:验证崩溃检测器自动重启

**Files:**
- 无文件修改,仅运行验证

- [ ] **Step 1:后台启动 agent**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && ./target/release/agent --config agent.toml > /tmp/agent-test.log 2>&1 &"`
Expected: 无输出(进程后台运行)

- [ ] **Step 2:获取 Worker 子进程 PID**

Run: `wsl -e bash -l -c "ps aux | grep '[a]gent --worker' | awk '{print \$2}'"`
Expected: 输出一个 PID 数字

- [ ] **Step 3:杀死 Worker 子进程**

将上一步获取的 PID 替换 `<WORKER_PID>`:
Run: `wsl -e bash -l -c "kill -9 <WORKER_PID>"`
Expected: 无输出

- [ ] **Step 4:等待 2 秒后检查日志**

Run: `wsl -e bash -l -c "sleep 2 && tail -20 /tmp/agent-test.log"`
Expected: 日志包含:
- `Worker 进程异常退出` 或类似的崩溃检测日志
- `Worker 崩溃,触发自动重启` 或类似
- `Worker 进程已启动: pid=XXXX`(新 PID)

- [ ] **Step 5:验证新 Worker 子进程存在**

Run: `wsl -e bash -l -c "ps aux | grep '[a]gent --worker' | head -3"`
Expected: 显示一个**新的** Worker 子进程(PID 与 Step 2 不同)

- [ ] **Step 6:清理测试进程**

Run: `wsl -e bash -l -c "pkill -f 'target/release/agent' && rm -f /tmp/gnome-remote-worker.sock /tmp/agent-test.log"`
Expected: 无输出

---

## Task 5:回归测试 — 验证现有功能不受影响

**Files:**
- 无文件修改,仅运行测试

- [ ] **Step 1:运行全部单元测试**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test 2>&1 | tail -20"`
Expected: 所有测试通过(103+ 个非 ignored 测试),无失败

- [ ] **Step 2:手动测试 QUIC 客户端连接(如果有客户端)**

如果客户端可用,启动客户端连接 agent,验证:
- 客户端能正常连接
- 终端能正常打开、输入命令
- 文件管理器能浏览目录
- 系统监控能显示信息

如果客户端不可用,跳过此步骤。

- [ ] **Step 3:验证 Manager 启动不影响 QUIC 功能**

启动 agent,然后用客户端连接(或用 `quic_test.sh`):
Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && bash tests/quic_test.sh 2>&1 | tail -10"`
Expected: QUIC 测试通过(或无连接错误)

- [ ] **Step 4:清理**

Run: `wsl -e bash -l -c "pkill -f 'target/release/agent' 2>/dev/null; rm -f /tmp/gnome-remote-worker.sock"`
Expected: 无输出

---

## Task 6:新增集成测试 — 验证 Manager 启动流程

**Files:**
- Create: `agent/tests/phase1_integration_test.rs`

- [ ] **Step 1:创建测试文件**

创建 `agent/tests/phase1_integration_test.rs`:

```rust
//! 阶段 1 集成测试:验证 Manager 启动流程
//!
//! 测试目标:
//! - Manager::new 能成功实例化
//! - Manager::start 能启动 IPC 服务器
//! - WorkerManager 能启动 Worker 子进程
//! - Worker 子进程能连接到 IPC 服务器

#![cfg(unix)]

use std::time::Duration;
use gnome_remote_agent::config::AgentConfig;
use gnome_remote_agent::manager::Manager;

/// 测试 Manager 能成功实例化
#[tokio::test]
async fn test_manager_creation() {
    let config = AgentConfig::default();
    let manager = Manager::new(&config).await;
    assert!(manager.is_ok(), "Manager 创建失败: {:?}", manager.err());
}

/// 测试 Manager::start 能启动 IPC 服务器和 Worker
#[tokio::test]
async fn test_manager_start_ipc_and_worker() {
    let mut config = AgentConfig::default();
    // 使用唯一的 socket 路径,避免冲突
    config.worker.ipc_socket_path = format!(
        "/tmp/gnome-remote-test-{}.sock",
        std::process::id()
    );
    // 使用当前编译的 agent 二进制
    config.worker.agent_binary = env!("CARGO_BIN_EXE_agent").to_string();
    config.worker.max_restarts = 1;

    let mut manager = Manager::new(&config).await.expect("Manager 创建失败");

    // 启动 Manager
    let start_result = manager.start().await;
    assert!(start_result.is_ok(), "Manager 启动失败: {:?}", start_result.err());

    // 等待 Worker 连接
    tokio::time::sleep(Duration::from_secs(1)).await;

    // 验证 Worker 进程已启动(通过公共 getter)
    let worker_info = manager.worker_info().await;
    assert!(worker_info.is_some(), "Worker 信息不应为空");
    let info = worker_info.unwrap();
    assert!(info.pid > 0, "Worker PID 应大于 0");

    // 停止 Manager
    let shutdown_result = manager.shutdown().await;
    assert!(shutdown_result.is_ok(), "Manager 停止失败: {:?}", shutdown_result.err());

    // 清理 socket 文件
    let _ = std::fs::remove_file(&config.worker.ipc_socket_path);
}

/// 测试 Manager::shutdown 能正确清理资源
#[tokio::test]
async fn test_manager_shutdown_cleanup() {
    let mut config = AgentConfig::default();
    config.worker.ipc_socket_path = format!(
        "/tmp/gnome-remote-test-shutdown-{}.sock",
        std::process::id()
    );
    config.worker.agent_binary = env!("CARGO_BIN_EXE_agent").to_string();
    config.worker.max_restarts = 1;

    let mut manager = Manager::new(&config).await.expect("Manager 创建失败");
    manager.start().await.expect("Manager 启动失败");

    tokio::time::sleep(Duration::from_secs(1)).await;

    // 停止
    manager.shutdown().await.expect("Manager 停止失败");

    // 验证 socket 文件已清理(IpcServer::stop 会关闭监听器,但可能不删除文件)
    // 这里只验证进程已停止
    tokio::time::sleep(Duration::from_millis(500)).await;

    let worker_info = manager.worker_info().await;
    // Worker 信息应该为 None 或状态为 Stopped
    if let Some(info) = worker_info {
        use gnome_remote_agent::manager::WorkerStatus;
        assert!(
            info.status == WorkerStatus::Stopped
                || info.status == WorkerStatus::Stopping,
            "Worker 状态应为 Stopped,实际: {:?}",
            info.status
        );
    }

    let _ = std::fs::remove_file(&config.worker.ipc_socket_path);
}
```

- [ ] **Step 2:运行新增测试**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test --test phase1_integration_test -- --nocapture 2>&1 | tail -30"`
Expected: 3 个测试全部通过:
- `test_manager_creation`
- `test_manager_start_ipc_and_worker`
- `test_manager_shutdown_cleanup`

- [ ] **Step 3:提示用户提交**

```bash
git add agent/tests/phase1_integration_test.rs
git commit -m "test(phase1): 新增 Manager 启动流程集成测试"
```

---

## Task 7:更新配置文件确保 Worker 路径正确

**Files:**
- Modify: `agent/agent.toml`(检查 `[worker]` 段)

- [ ] **Step 1:查看当前 agent.toml 的 worker 配置**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && grep -A 5 '\[worker\]' agent.toml 2>/dev/null || echo 'No [worker] section found'"`
Expected: 显示 worker 配置,或提示无配置

- [ ] **Step 2:如果 `[worker]` 段不存在或 `agent_binary` 路径错误,更新配置**

在 `agent/agent.toml` 中添加或修改(如果已存在):

```toml
[worker]
agent_binary = "./target/release/agent"
ipc_socket_path = "/tmp/gnome-remote-worker.sock"
max_restarts = 3
```

注意:`agent_binary` 路径必须是**相对于 agent 进程工作目录**的路径,或**绝对路径**。

- [ ] **Step 3:验证配置加载**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && ./target/release/agent --config agent.toml --log-dir off 2>&1 | head -15 &"`
Expected: 日志显示配置加载成功,Worker 路径正确

- [ ] **Step 4:清理测试进程**

Run: `wsl -e bash -l -c "pkill -f 'target/release/agent' 2>/dev/null"`
Expected: 无输出

- [ ] **Step 5:提示用户提交(如有改动)**

```bash
git add agent/agent.toml
git commit -m "config: 更新 worker 配置路径"
```

---

## Task 8:最终验证 — 完整流程测试

**Files:**
- 无文件修改,仅运行验证

- [ ] **Step 1:运行全部测试**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test 2>&1 | tail -10"`
Expected: 所有测试通过,包括新增的 phase1_integration_test

- [ ] **Step 2:启动 agent 并验证完整启动流程**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && timeout 10 ./target/release/agent --config agent.toml 2>&1 | grep -E '(Manager|Worker|IPC|QUIC|WS)' | head -15"`
Expected: 日志按顺序显示:
1. `Manager 已实例化,正在启动...`
2. `IPC 服务器已启动: path=/tmp/gnome-remote-worker.sock`
3. `Worker 进程已启动: pid=XXXX`
4. `Worker 崩溃检测器启动`
5. `Manager 已启动(IPC + Worker + CrashDetector)`
6. `QUIC  监听: udp://0.0.0.0:8443`
7. `WS    监听: tcp://0.0.0.0:443`

- [ ] **Step 3:验证 Worker 连接 IPC**

在 agent 运行期间(Step 2 的 10 秒内),在另一个终端运行:
Run: `wsl -e bash -l -c "ps aux | grep '[a]gent' | wc -l"`
Expected: 输出 `2`(主进程 + Worker 子进程)

- [ ] **Step 4:验证崩溃自动恢复**

```bash
# 1. 后台启动 agent
wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && ./target/release/agent --config agent.toml > /tmp/agent-final-test.log 2>&1 &"

# 2. 等待启动
wsl -e bash -l -c "sleep 2"

# 3. 获取 Worker PID 并杀死
wsl -e bash -l -c "WORKER_PID=\$(ps aux | grep '[a]gent --worker' | awk '{print \$2}' | head -1) && echo \"Killing Worker PID: \$WORKER_PID\" && kill -9 \$WORKER_PID"

# 4. 等待自动重启
wsl -e bash -l -c "sleep 3"

# 5. 验证新 Worker 已启动
wsl -e bash -l -c "ps aux | grep '[a]gent --worker' | head -1"

# 6. 查看日志
wsl -e bash -l -c "tail -15 /tmp/agent-final-test.log"

# 7. 清理
wsl -e bash -l -c "pkill -f 'target/release/agent' && rm -f /tmp/agent-final-test.log /tmp/gnome-remote-worker.sock"
```

Expected:
- Step 3 输出被杀死的 Worker PID
- Step 5 显示新的 Worker 进程(PID 不同)
- Step 6 日志显示崩溃检测和重启日志

---

## 完成标准

阶段 1 完成后,必须满足以下所有条件:

1. ✅ `cargo build --release` 零错误
2. ✅ `cargo test` 全部通过(包括新增的 phase1_integration_test)
3. ✅ 启动 agent 后,日志显示 Manager 启动、IPC 服务器启动、Worker 子进程启动
4. ✅ `ps aux` 能看到主进程 + Worker 子进程
5. ✅ 现有 QUIC 客户端连接、PTY、文件操作功能**完全不受影响**
6. ✅ Worker 子进程崩溃后,CrashDetector 自动重启

---

## 风险与回退

### 回退方案

如果阶段 1 导致现有功能异常,可以快速回退:

1. 在 `main.rs::run_manager_mode` 中注释掉 Manager 相关代码(4 行):
   ```rust
   // let mut manager = Manager::new(&cfg).await?;
   // manager.start().await?;
   // manager.shutdown().await?;
   ```
2. 重新编译运行,恢复到集成前状态

### 已知限制

- 阶段 1 期间 Worker 子进程是空跑的(不处理任何业务请求)
- Worker 崩溃后重启,但不会恢复之前的状态(因为阶段 1 没有业务)
- IPC Socket 文件在异常退出时可能残留(下次启动时会自动清理)

---

## 下一步

阶段 1 完成并验证后,进入**阶段 2:PTY 创建迁移到 Worker**(见设计文档第四章)。
