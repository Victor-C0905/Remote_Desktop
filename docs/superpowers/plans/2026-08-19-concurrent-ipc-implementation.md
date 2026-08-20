# 并发 IPC 与 fork 安全实现计划

## 概述

**目标**：消除 IpcServer 竞态 + 消除 Manager fork 隐患（posix_spawn 替代）+ Worker 保持串行不变。

**设计文档**：[2026-08-19-concurrent-ipc-design.md](../specs/2026-08-19-concurrent-ipc-design.md)

## 任务列表（15 个分层小任务）

### 基础设施层

#### Task 1: 添加 dashmap 依赖

**文件**：[Cargo.toml](file:///e:\MyWork\gnome-remote\agent\Cargo.toml)

**操作**：
```toml
[dependencies]
# 新增
dashmap = "6"
```

**验证**：`cargo build` 成功

**Git**：`git add agent/Cargo.toml agent/Cargo.lock`

---

#### Task 2: 添加 ipc_channel_capacity 配置项

**文件**：
- [config.rs](file:///e:\MyWork\gnome-remote\agent\src\config.rs)：WorkerConfig 新增字段
- [agent.dev.toml](file:///e:\MyWork\gnome-remote\agent\agent.dev.toml) / [agent.prod.toml](file:///e:\MyWork\gnome-remote\agent\agent.prod.toml)：示例配置

**操作**：
```rust
// config.rs WorkerConfig
pub struct WorkerConfig {
    // 现有字段...
    /// IPC 请求 channel 容量（默认 128）
    #[serde(default = "default_ipc_channel_capacity")]
    pub ipc_channel_capacity: usize,
}

fn default_ipc_channel_capacity() -> usize { 128 }
```

**验证**：`cargo build` 成功，配置文件解析正常

**Git**：`git add agent/src/config.rs agent/agent.dev.toml agent/agent.prod.toml`

---

### Manager 侧重构

#### Task 3: IpcServer 结构改造

**文件**：[ipc_server.rs](file:///e:\MyWork\gnome-remote\agent\src\manager\ipc_server.rs)

**操作**：
1. 新增 `RequestItem` 结构
2. `IpcServer` 结构改为 `request_tx + pending + dispatcher_handle`
3. `IpcServer::new` 签名增加 `channel_capacity: usize` 参数
4. 删除 `connections` 字段
5. 新增 `next_request_id()` 原子计数器

**验证**：`cargo check`（编译通过，可能有未使用警告）

**Git**：`git add agent/src/manager/ipc_server.rs`

---

#### Task 4: 实现 dispatcher task

**文件**：[ipc_server.rs](file:///e:\MyWork\gnome-remote\agent\src\manager\ipc_server.rs)

**操作**：
1. 新增 `run_dispatcher` 私有异步函数
2. 实现 request_id 路由逻辑（read_response → pending.remove → sender.send）
3. 实现连接断开清理逻辑（pending.clear）

**验证**：`cargo check` 通过

**Git**：`git add agent/src/manager/ipc_server.rs`

---

#### Task 5: 重写 send_request / create_pty_session

**文件**：[ipc_server.rs](file:///e:\MyWork\gnome-remote\agent\src\manager\ipc_server.rs)

**操作**：
1. `send_request` 改为 mpsc::send + oneshot::recv 模式
2. `create_pty_session` 委托给 send_request
3. 更新单元测试调用（IpcServer::new 新签名）

**验证**：`cargo check` 通过，单元测试编译通过

**Git**：`git add agent/src/manager/ipc_server.rs`

---

#### Task 6: 修复 run / stop / accept_and_set_pid

**文件**：[ipc_server.rs](file:///e:\MyWork\gnome-remote\agent\src\manager\ipc_server.rs)

**操作**：
1. `accept_and_set_pid`：Worker 连接到达时，split UnixStream，启动新 dispatcher，abort 旧 dispatcher
2. `stop`：abort dispatcher，清理 pending，关闭 socket
3. 保留 `Drop` impl 的 socket 文件清理逻辑

**验证**：`cargo check` 通过

**Git**：`git add agent/src/manager/ipc_server.rs`

---

#### Task 7: Manager::new 传递 channel_capacity

**文件**：[mod.rs](file:///e:\MyWork\gnome-remote\agent\src\manager\mod.rs)

**操作**：
1. `Manager::new` 读取 `config.worker.ipc_channel_capacity`
2. 传给 `IpcServer::new`

**验证**：`cargo check` 通过

**Git**：`git add agent/src/manager/mod.rs`

---

### Manager fork 改造（posix_spawn）

#### Task 8: ForkGuard 引入

**文件**：[executor.rs](file:///e:\MyWork\gnome-remote\agent\src\auth\executor.rs)

**操作**：
1. 新增 `static FORK_GUARD: std::sync::Mutex<()>`
2. 新增 `fn with_fork_guard() -> MutexGuard`
3. 不修改现有 fork 代码（后续 Task 替换时使用）

**验证**：`cargo check` 通过

**Git**：`git add agent/src/auth/executor.rs`

---

#### Task 9: 子命令实现（main.rs）

**文件**：[main.rs](file:///e:\MyWork\gnome-remote\agent\src\main.rs)

**操作**：
1. Args 新增 5 个子命令参数（`--isolated-writer` 等）和通用参数（`--uid` `--gid` `--final-path` `--part-paths`）
2. 实现 `dispatch_isolated_command` 分发函数
3. 实现 `run_isolated_writer` / `run_isolated_reader` / `run_isolated_writer_part` / `run_isolated_merger` / `run_metadata`
4. main 中调用 `dispatch_isolated_command`，返回 true 时直接退出

**验证**：`cargo build` 成功，手动测试 `agent --isolated-writer /tmp/test --final-path /tmp/final --uid 1000 --gid 1000`

**Git**：`git add agent/src/main.rs`

---

#### Task 10: executor.rs 改造 spawn_isolated_*

**文件**：[executor.rs](file:///e:\MyWork\gnome-remote\agent\src\auth\executor.rs)

**操作**：
1. `spawn_isolated_writer`：fork + os_pipe → posix_spawn + std::process::Command
2. `spawn_isolated_reader`：同上
3. `spawn_isolated_writer_part`：同上
4. `spawn_isolated_merger`：同上
5. `get_metadata_async` 新增（spawn_blocking 包装）
6. 保留 `execute_as_user` 不变（Worker 仍用）
7. `wait_isolated_child` 不变（仍用 libc::waitpid）
8. 保留 `#[cfg(not(unix))]` 的 stub 实现，更新返回类型

**验证**：`cargo build` 成功

**Git**：`git add agent/src/auth/executor.rs`

---

#### Task 11: file_stream.rs 类型适配

**文件**：[file_stream.rs](file:///e:\MyWork\gnome-remote\agent\src\file_stream.rs)

**操作**：
1. `PipeFileStreamWriter.pipe_writer` 字段类型：`os_pipe::PipeWriter` → `std::process::ChildStdin`
2. `PipeFileStreamReader.pipe_reader` 字段类型：`os_pipe::PipeReader` → `std::process::ChildStdout`
3. `PipeFileStreamWriter::new` 参数类型变更
4. `PipeFileStreamReader::new` 参数类型变更
5. 所有方法逻辑不变（`std::io::Write` / `std::io::Read` trait 方法兼容）

**验证**：`cargo build` 成功

**Git**：`git add agent/src/file_stream.rs`

---

#### Task 12: handler.rs / quic.rs 适配

**文件**：
- [handler.rs](file:///e:\MyWork\gnome-remote\agent\src\handler.rs)
- [quic.rs](file:///e:\MyWork\gnome-remote\agent\src\server\quic.rs)

**操作**：
1. handler.rs:519 的 `execute_as_user` 改为 `get_metadata_async`
2. handler.rs:418/469/699 的 `spawn_isolated_*` 调用（类型自动推导，无需改动逻辑）
3. handler.rs:550 的 `spawn_isolated_reader` 调用（同上）
4. quic.rs:2086 的 `spawn_isolated_merger` 调用（接口不变）

**验证**：`cargo build` 成功

**Git**：`git add agent/src/handler.rs agent/src/server/quic.rs`

---

### 测试验证层

#### Task 13: 并发回归测试

**文件**：[ipc_server.rs](file:///e:\MyWork\gnome-remote\agent\src\manager\ipc_server.rs)（测试模块）

**操作**：
1. 新增 `test_concurrent_requests_no_race` 测试
2. 10 个并发 send_request，验证全部成功（无 "No active Worker connection" 错误）

**验证**：`cargo test --test concurrent_ipc` 通过

**Git**：`git add agent/src/manager/ipc_server.rs`

---

#### Task 14: 子命令单元测试

**文件**：[main.rs](file:///e:\MyWork\gnome-remote\agent\src\main.rs)（测试模块）或新建 tests/isolated_command.rs

**操作**：
1. 测试 `run_isolated_writer`：写 stdin → 验证文件内容 + rename
2. 测试 `run_isolated_reader`：写文件 → 读 stdout 验证内容
3. 测试 `run_metadata`：创建文件 → 验证 size/mtime

**验证**：`cargo test` 通过

**Git**：`git add agent/src/main.rs` 或 `agent/tests/isolated_command.rs`

---

#### Task 15: 最终编译 + 零警告检查

**操作**：
1. `cargo build --release`（零错误）
2. `cargo build --release 2>&1 | grep warning`（零警告）
3. `cargo test`（全部通过）
4. 移除 `os_pipe` 依赖（Cargo.toml）

**文件**：
- [Cargo.toml](file:///e:\MyWork\gnome-remote\agent\Cargo.toml)：移除 `os_pipe = "1"`
- [executor.rs](file:///e:\MyWork\gnome-remote\agent\src\auth\executor.rs)：移除 `use os_pipe` 或 `extern crate os_pipe`

**验证**：
- `cargo build --release` 零错误零警告
- `cargo test` 全部通过

**Git**：`git add agent/Cargo.toml agent/Cargo.lock agent/src/auth/executor.rs`

---

## 依赖关系

```
Task 1 (dashmap) ─┐
Task 2 (config)  ─┼─→ Task 3 (结构) ─→ Task 4 (dispatcher) ─→ Task 5 (send_request) ─→ Task 6 (lifecycle) ─→ Task 7 (Manager::new)
                  │                                                                                              │
                  └─→ Task 8 (ForkGuard) ─→ Task 10 (executor) ─→ Task 11 (file_stream) ─→ Task 12 (handler/quic) │
                                            ↑                                          ↑                      │
                                  Task 9 (子命令)                                     │                      │
                                                                                       └──→ Task 13 (并发测试) ─┤
                                                                                                               ↓
                                                                                  Task 14 (子命令测试) ─→ Task 15 (最终验证)
```

- Task 1, 2, 8, 9 可并行
- Task 3 依赖 Task 1, 2
- Task 10 依赖 Task 8, 9
- Task 11 依赖 Task 10
- Task 12 依赖 Task 10, 11
- Task 13 依赖 Task 6
- Task 14 依赖 Task 9
- Task 15 依赖所有前置 Task

## 不在范围内

- Worker 侧改动（保持串行不变）
- Worker 的 execute_as_user 改造
- 热更新流程改造
- IPC 协议变更
- 客户端改动

## 执行方式建议

推荐 **Subagent-Driven Development**：
- 每个 Task 派发独立 subagent
- Task 间有依赖，需顺序执行
- 每 3-4 个 Task 设置检查点审查
- Task 15 作为最终验证关卡
