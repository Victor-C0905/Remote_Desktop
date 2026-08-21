# Manager/Worker 架构集成设计文档

> **创建日期**: 2026-08-04
> **状态**: 设计阶段(待审阅)
> **背景**: 经分析发现,Phase 2-4 实现的 Manager/Worker 微内核架构(约 5460 行代码)从未集成到 main.rs 实际运行路径中,全部为死代码。本设计描述如何分阶段完成集成。

---

## 一、现状分析(过度设计问题)

### 1.1 核心问题

`main.rs::run_manager_mode` 函数名具有误导性 — 它**不实例化 `Manager` 结构体**,而是直接调用 `server::quic::run` 和 `server::websocket::run`。整个 `Manager::run()` 在代码库中零外部调用。

**实际运行架构**(单进程):
```
main.rs → server::quic::run → pty::PtyManager::spawn_as_user (直接 fork PTY)
                           → protocol::serde (JSON over QUIC)
```

**设计但未启用**(微内核,死代码):
```
Manager::run (永不执行) → WorkerManager::start → Worker 子进程
                       → IpcServer → SCM_RIGHTS FD 传递
                       → HotUpdateCoordinator → SIGHUP 热更新
```

### 1.2 死代码清单

#### Manager 模块(~3427 行,仅 ~100 行被使用)

| 文件 | 行数 | 状态 |
|------|------|------|
| `manager/mod.rs` | 266 | `Manager::run()` 全代码库零外部调用 |
| `manager/connection.rs` | 693 | 仅单元测试使用 |
| `manager/session.rs` | 21 | 空壳,只有 `// TODO` |
| `manager/auth.rs` | 1 | 只有一行注释 |
| `manager/pty_registry.rs` | 222 | 从未在运行路径实例化 |
| `manager/pty_output.rs` | 358 | 仅 `spawn_pty_output_task_legacy` 被 quic.rs 使用 |
| `manager/worker_manager.rs` | 515 | 仅在 `Manager::run` 内(永不执行) |
| `manager/ipc_server.rs` | 478 | 仅在 `Manager::run` 内(永不执行) |
| `manager/orphan_reaper.rs` | 243 | quic.rs 显式传 `None` |
| `manager/crash_detector.rs` | 240 | 仅在 `Manager::run` 内(永不执行) |
| `manager/signal_handler.rs` | 147 | 仅在 `Manager::run` 内(永不执行) |
| `manager/hot_update_coordinator.rs` | 243 | 仅在 `Manager::run` 内(永不执行) |

#### Worker 模块(~2033 行,完整但默认不触发)

`worker/` 目录代码自洽,handlers 被 `worker::run` 调用,但默认 Manager 模式从不启动 Worker 子进程。只有手动执行 `agent --worker --ipc-socket <path>` 才会触发,而此时 Worker 会尝试连接一个不存在的 IPC server。

#### 两套并行的协议体系

| 协议 | 位置 | 实际使用 |
|------|------|---------|
| `protocol/serde.rs`(JSON) | `server/quic.rs` | ✅ 实际运行路径 |
| `protocol/generated.rs`(Protobuf) | 仅 worker 模块 | ❌ 默认不触发 |

两套消息类型完全不互通,不存在运行时转换。

#### Phase 4 热更新机制:完全未工作

TASK_BREAKDOWN.md 声称 Phase 4 "100% 完成",但实际情况:
- `HotUpdateCoordinator::run()` — 从未执行
- `WorkerCrashDetector::start()` — 从未执行
- `watch_sighup()` — 从未执行
- `OrphanProcessReaper` — quic.rs 传 `None`
- `WorkerManager::start()` — 从未执行
- `IpcServer::run()` — 从未执行

**"40 个任务全部完成"的声明具有误导性** — 代码写完了,但从未接入实际运行路径。

#### 测试死代码的测试

| 测试文件 | 行数 | 价值 |
|---------|------|------|
| `tests/hot_update_test.rs` | 161 | 测试未启用的热更新 |
| `tests/ipc_integration_test.rs` | 177 | 测试未启用的 IPC |
| `tests/manager_test.rs` | 1 | 空文件 |
| `tests/worker_test.rs`、`bench_test.rs`、`stress_test.rs` | - | 测试 Worker 路径(默认不触发) |

---

## 二、集成方案:自底向上分阶段

### 方案选择

经对比 3 种方案(自底向上/垂直切片/双路径并行),选择**方案 A:自底向上分阶段**:

```
阶段 1 (骨架): main.rs → Manager::run() → 启动 Worker 子进程 → IPC 通道验证
阶段 2 (PTY 迁移): Worker::PtyFactory 创建 PTY → FD 转移给 Manager → quic.rs 改用 PtyRegistry
阶段 3 (业务迁移): 文件操作/命令执行/系统信息 → 路由到 Worker handlers
阶段 4 (热更新): 启用 SIGHUP、CrashDetector、OrphanReaper、HotUpdateCoordinator
```

**理由**:
1. Manager↔Worker IPC 是整个架构的地基,必须先验证
2. PTY 迁移涉及 FD 转移(SCM_RIGHTS),技术风险最高,应单独处理
3. 热更新依赖前面所有阶段稳定,放最后启用最安全
4. 项目对稳定性要求高(网络中断自动重连等),渐进式更稳妥

---

## 三、阶段 1:IPC 骨架打通

### 目标

让 `main.rs::run_manager_mode` 真正实例化 `Manager` 并调用 `Manager::run()`,启动 Worker 子进程,建立 IPC 通道。**此阶段 PTY 仍走 quic.rs 旧路径,Manager/Worker 空跑验证 IPC。**

### 改动点

#### 1.1 main.rs 改造

```rust
async fn run_manager_mode(args: &Args) -> Result<()> {
    let cfg = config::load(&args.config)?;
    init_logging(&args, &cfg);
    let (cert, key) = cert::ensure_certificate(&cfg)?;

    // 初始化认证器、EventBus、SubscriptionManager(留 Manager 使用)
    let authenticator = Arc::new(CompositeAuthenticator::new(...));
    let event_bus = Arc::new(event_bus::EventBus::new());
    let subscription_manager = Arc::new(subscription::SubscriptionManager::new(...));
    let pty_manager = Arc::new(pty::PtyManager::new());  // 临时保留,阶段 2 移除
    let audit_log = Arc::new(audit::AuditLogger::new(...)?);

    // 新增:实例化 Manager
    let mut manager = Manager::new(&cfg, authenticator.clone(), event_bus.clone(),
                                    subscription_manager.clone(), audit_log.clone()).await?;

    tokio::try_join!(
        manager.run(),                           // 新增:启动 Manager(含 Worker 子进程、IPC、崩溃检测)
        server::quic::run(cfg, cert, key, ...),  // 临时保留,阶段 2 改造
        server::websocket::run(cfg, cert, key),  // 保持不变
    )?;
}
```

#### 1.2 Manager::new / Manager::run 改造

当前 `Manager::new` 签名只接收 `&Config`,需要扩展为接收认证器、EventBus 等。`Manager::run` 当前启动 IpcServer、WorkerManager、CrashDetector、SighupListener、HotUpdateCoordinator,**阶段 1 只启用 IpcServer + WorkerManager + CrashDetector,暂不启用 SIGHUP/HotUpdateCoordinator**。

#### 1.3 WorkerManager::start 验证

`WorkerManager::start()` 会 spawn 子进程执行 `agent --worker --ipc-socket <path>`。需要验证:
- 子进程能正常启动
- IpcClient 能连接到 IpcServer
- FD 传递通道可用(发送一个测试 FD)

#### 1.4 quic.rs 临时保留

quic.rs **完全不改**,继续走 `pty::PtyManager::spawn_as_user` 直接 fork PTY。Manager/Worker 空跑,不影响现有功能。

### 验证标准

1. `cargo build` 通过
2. 启动 agent 后,日志显示 "Worker 子进程启动" + "IPC 连接建立"
3. `ps aux | grep agent` 能看到主进程 + Worker 子进程
4. 现有 QUIC 客户端连接、PTY、文件操作功能**完全不受影响**(回归测试)
5. Worker 子进程崩溃后,CrashDetector 自动重启(测试:手动 kill Worker PID)

### 风险与缓解

| 风险 | 缓解 |
|------|------|
| Worker 子进程启动失败 | CrashDetector 自动重启;日志详细记录 |
| IPC Socket 文件残留 | 启动时清理旧 socket 文件 |
| Manager::run 阻塞 main | 用 tokio::spawn 包装,不阻塞 try_join |
| 双进程内存占用增加 | Worker 是轻量级进程,主要内存仍在 Manager |

---

## 四、阶段 2:PTY 创建迁移到 Worker

### 目标

将 PTY 创建从 `quic.rs` 直接 fork,改为通过 Worker 子进程创建并 FD 转移给 Manager。**这是技术风险最高的阶段。**

### 数据流变化

**改动前**(当前):
```
quic.rs::handle_terminal_stream
  → pty::PtyManager::spawn_as_user(uid/gid/shell)  [主进程直接 fork]
  → 主进程持有 master_fd
  → spawn_pty_output_task_legacy 读 master_fd 推送给客户端
```

**改动后**:
```
quic.rs::handle_terminal_stream
  → Manager 发送 ManagerRequest::CreateSession{shell,cols,rows,uid,gid,env} 给 Worker
  → Worker::PtyFactory::create 执行 forkpty(含用户隔离)
  → Worker 通过 SCM_RIGHTS 把 master_fd 传给 Manager::IpcServer
  → IpcServer 注册到 PtyRegistry(session_id → master_fd)
  → Manager 返回 session_id 给 quic.rs
  → quic.rs 从 PtyRegistry 获取 master_fd
  → spawn_pty_output_task(新架构版本)读 PtyRegistry 推送给客户端
```

### 改动点

#### 2.1 Worker::PtyFactory 增强用户隔离

当前 `worker/pty_factory.rs` 使用 `forkpty` 但**没有用户隔离逻辑**。需要把 `pty.rs` 中的 `spawn_as_user` 的用户隔离逻辑(UserNamespace/setuid/setgid)迁移到 PtyFactory。

#### 2.2 CreateSession 消息扩展

`agent.proto` 的 `CreateSession` 消息需要扩展,增加用户信息字段:
```protobuf
message CreateSession {
    string shell = 1;
    uint16 cols = 2;
    uint16 rows = 3;
    string working_directory = 4;
    map<string, string> env = 5;
    // 新增字段
    uint32 uid = 6;
    uint32 gid = 7;
    string username = 8;
    bool use_user_namespace = 9;
}
```

#### 2.3 quic.rs::handle_terminal_stream 改造

当前 `quic.rs:1411-1542` 直接调用 `pty_manager.spawn_as_user`。改为:
1. 构造 `ManagerRequest::CreateSession` 消息
2. 通过 Manager 的 IPC 通道发送给 Worker
3. 等待 Worker 返回 `SessionCreated{session_id, pid}`
4. 从 PtyRegistry 获取 master_fd(IpcServer 已注册)
5. 启动 `spawn_pty_output_task`(新架构版本,从 PtyRegistry 读)

#### 2.4 PtyRegistry 集成

`manager/pty_registry.rs` 已有 register/get/read/write/resize 方法。需要:
- IpcServer 接收 FD 后自动调用 `pty_registry.register()`
- quic.rs 通过 `pty_registry.get(session_id)` 获取 master_fd
- `spawn_pty_output_task` 改用 PtyRegistry 作为数据源

#### 2.5 临时保留 pty::PtyManager

`pty::PtyManager` 在阶段 2 期间仍保留(供 WebSocket 服务器使用),但 quic.rs 不再调用它。阶段 3 评估是否完全移除。

### 验证标准

1. 客户端连接 → 打开终端 → 能正常输入命令
2. 多个并发终端会话正常工作
3. 终端 resize 正常
4. 终端关闭后,Worker::SessionManager 检测到子进程退出
5. `ps aux` 能看到:Manager 主进程 + Worker 子进程 + Shell 进程(由 Worker fork)
6. Shell 进程的 uid/gid 正确(用户隔离生效)
7. FD 泄漏检测:多次创建/关闭终端,Manager 的 fd 数量稳定

### 风险与缓解

| 风险 | 缓解 |
|------|------|
| FD 转移失败(SCM_RIGHTS) | 现有 tests/integration_test.rs 已验证 FD passing;增加错误处理 |
| 用户隔离逻辑迁移出错 | 对比 pty.rs 和 pty_factory.rs 的实现,确保逻辑一致 |
| IPC 延迟影响终端响应 | Manager↔Worker IPC 仅用于创建/resize/kill,数据 I/O 直接读 PtyRegistry |
| PtyRegistry 并发访问 | 已用 `Arc<RwLock<HashMap>>`,天然线程安全 |
| Shell 进程成为孤儿(Worker 崩溃) | 阶段 4 启用 OrphanReaper;阶段 2 期间 Worker 崩溃由 CrashDetector 重启,但该会话丢失 |

---

## 五、阶段 3:业务操作迁移到 Worker

### 目标

将文件操作、命令执行、系统信息查询从 quic.rs 直接处理,改为路由到 Worker handlers。

### 改动点

#### 3.1 quic.rs::handle_stream 改造

当前 `quic.rs:983-1347` 在 `handle_stream` 中直接处理 `FileTransfer`、`Disconnect` 等。需要把业务请求改为通过 Manager→Worker IPC 路由:

- 文件读写 → `ManagerRequest::ReadFile`/`WriteFile`/`ReadDir`
- 命令执行 → `ManagerRequest::ExecuteCommand`
- 系统信息 → `ManagerRequest::GetSystemInfo`

#### 3.2 协议适配层

quic.rs 接收 `protocol::serde::Payload`(JSON),Worker 处理 `protocol::generated::ManagerRequest`(Protobuf)。需要在 Manager 中新增**协议适配层**:

```rust
// manager/protocol_adapter.rs (新增)
fn serde_to_worker_request(envelope: serde::Envelope, session: &Session) -> generated::ManagerRequest
fn worker_response_to_serde(resp: generated::WorkerResponse) -> serde::Payload
```

#### 3.3 请求-响应映射

Manager 需要维护 `request_id → QUIC Stream` 的映射,Worker 返回响应时路由回正确的 Stream。当前 `ConnectionManager` 有类似设计,可复用。

#### 3.4 文件传输保留

大文件传输(分块上传/下载)仍走 quic.rs 直接处理,因为:
- 文件数据量大,经 IPC 转发会增加延迟
- 现有 `file_stream.rs` 已优化
- 只有文件元信息(列表、权限)走 Worker

### 验证标准

1. 文件管理器:浏览目录、读写文件正常
2. 命令执行:返回 stdout/stderr/exit_code
3. 系统监控:CPU/内存/磁盘信息正常
4. 文件上传/下载:大文件传输不受影响
5. 并发请求:多个客户端同时操作不冲突

---

## 六、阶段 4:启用热更新机制

### 目标

启用 Phase 4 已实现但未启用的热更新组件,实现 Worker 进程热更新不中断会话。

### 启用组件

#### 4.1 SignalHandler(SIGHUP)

`manager/signal_handler.rs` 已实现。在 `Manager::run` 中启动 `watch_sighup`,收到 SIGHUP 后发送 `ReloadTrigger::UnixSignal` 事件。

#### 4.2 HotUpdateCoordinator

`manager/hot_update_coordinator.rs` 已实现。接收 `ReloadTrigger` 后:
1. 标记 `is_graceful_shutdown`
2. 发送 `GracefulShutdown` 给 Worker
3. Worker 等待未决任务完成,返回 `ShutdownAck`
4. Worker 退出,Shell 进程成为孤儿(由 init 领养)
5. Manager 持有的 master_fd 仍可读写,会话不中断
6. WorkerManager 启动新 Worker
7. 重置标志

#### 4.3 OrphanProcessReaper

`manager/orphan_reaper.rs` 已实现。在 PTY EOF 时回收僵尸 Shell 进程。阶段 2 传 `None`,阶段 4 改为传入实际实例。

#### 4.4 systemd 配置

`systemd/quireld.service` 已有 `ExecReload=/bin/kill -HUP $MAINPID`。启用后 `systemctl reload quireld` 触发热更新。

### 验证标准

1. `systemctl reload quireld` 后,Worker PID 变化,但终端会话不中断
2. 热更新期间,客户端输入命令仍能响应(数据缓存在内核 buffer)
3. Shell 进程在 Worker 重启后仍存活(`ps aux` 确认)
4. PTY EOF 后,僵尸进程被 OrphanReaper 回收
5. Worker 崩溃(非优雅关闭)后,CrashDetector 自动重启

### 关于 Agent 自更新

`2026-08-04-agent-self-update-design.md` 的自更新机制(二进制替换 + systemctl restart)**与阶段 4 热更新是两个不同层次**:
- 阶段 4 热更新:Worker 进程重启,会话不中断(Manager 不变)
- Agent 自更新:整个 agent 二进制替换,所有进程重启(会话中断,客户端重连)

建议阶段 4 完成后再实施 Agent 自更新,作为补充机制(用于 Manager 本身需要更新的场景)。

---

## 七、关键技术决策

### 7.1 认证位置:留 Manager

认证是连接级别的(QUIC 连接建立时认证一次),不是会话级别。认证逻辑留 Manager,认证通过后把用户信息(uid/gid/username)通过 IPC 传给 Worker,Worker 用此信息创建 PTY。

### 7.2 协议适配:Manager 内部转换

客户端↔Manager 通信用 `protocol::serde`(JSON,保持向后兼容);Manager↔Worker 通信用 `protocol::generated`(Protobuf,高效)。Manager 内部新增协议适配层做转换。

### 7.3 WebSocket 服务器:暂不集成

`server/websocket.rs` 是备选协议,使用率低。集成期间保持现状,继续走旧路径。未来如果需要,再单独集成。

### 7.4 pty::PtyManager:渐进移除

- 阶段 1-2:保留(quic.rs 和 websocket.rs 使用)
- 阶段 2 后:quic.rs 不再使用,仅 websocket.rs 使用
- 未来:评估是否完全移除

### 7.5 回归测试策略

每个阶段完成后,必须运行完整回归测试:
- `cargo test`(单元 + 集成测试)
- 手动测试:客户端连接、终端、文件管理、系统监控
- 性能对比:集成前后延迟、内存占用对比

---

## 八、阶段间依赖与时间预估

```
阶段 1 (IPC 骨架) ──┐
                   ├─ 阶段 2 (PTY 迁移) ──┐
                   │                      ├─ 阶段 3 (业务迁移) ──┐
                   │                      │                      ├─ 阶段 4 (热更新)
                   │                      │                      │
                   ▼                      ▼                      ▼
              验证 IPC 通道          验证 PTY 可用          验证业务正常
```

**每个阶段必须完全稳定后才能进入下一阶段。** 如果某个阶段遇到无法解决的问题,可以停留在当前阶段,系统仍可正常运行(因为旧路径保留)。

---

## 九、不在本次范围

- Agent 自更新机制(二进制替换 + systemctl restart)— 阶段 4 完成后再实施
- 配置热重载(reload 机制)— 未来增强
- 自动下载源(GitHub Releases / apt 仓库)— 未来增强
- WebSocket 服务器集成 — 暂不集成
- pty::PtyManager 完全移除 — 阶段 3 后评估

---

## 十、参考文档

- [架构设计文档](../../ARCHITECTURE.md) — 原始微内核架构设计
- [任务分解文档](../../TASK_BREAKDOWN.md) — Phase 0-5 任务记录(注意:完成状态有误导性)
- [Agent 自更新设计](./2026-08-04-agent-self-update-design.md) — 二进制更新机制(阶段 4 后实施)
- [Phase 4 热更新设计](./2026-08-03-phase4-hot-update-design.md) — 原始热更新设计
