# Manager/Worker 全面迁移实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将所有文件操作从 Manager 进程(handler.rs)迁移到 Worker 子进程,启用 Phase 4 热更新机制,清理死代码,完成 Manager/Worker 架构的全面集成。

**Architecture:** 客户端 JSON 协议(serde)→ Manager 协议适配层 → Worker Protobuf 协议 → UserExecutor(fork+setuid)执行用户隔离操作。流式操作(FileTransfer/Subscribe/Terminal)保留在 quic.rs。

**Tech Stack:** Rust 2024 edition, Tokio, Prost(Protobuf), Serde(JSON), nix(SCM_RIGHTS/setuid)

**设计文档:** [2026-08-04-manager-worker-integration-design.md](file:///e:/MyWork/gnome-remote/docs/superpowers/specs/2026-08-04-manager-worker-integration-design.md)

---

## 当前进度(2026-08-05 自审)

| 阶段 | 任务 | 状态 | 说明 |
|------|------|------|------|
| Phase 3 续 | Task 1-7 (Delete/Mkdir/Rename/Copy/Move/FileExists/ApplyDiff) | ☐ 待办 | proto/generated.rs/handlers/adapter 均未实现 |
| Phase 4 | Task 8 (SIGHUP + HotUpdateCoordinator) | ✓ 已完成 | 实现比计划更简化(见下方说明) |
| Phase 5 | Task 9 (pty_output.rs legacy 清理) | ☐ 待办 | legacy 函数与 `PtyReader for PtyManager` impl 仍存在 |
| Phase 5 | Task 10 (ConnectionContext cleanup → PtyRegistry) | ☐ 待办 | quic.rs 仍使用 `PtyManager` |
| Phase 5 | Task 11 (移除 `_pty_manager` 死参数) | ☐ 待办 | `handle_stream` 等签名仍带 `_pty_manager` |
| Phase 5 | Task 12 (删除 pty.rs) | ☐ 待办 | 文件仍存在,lib.rs 仍有 `pub mod pty;` |
| Phase 5 | Task 13 (清理 handler.rs,可选) | ☐ 待办 | 可选,保留作为非 Unix 回退 |
| Phase 5 | Task 14 (最终回归测试) | ☐ 待办 | 完成所有 Task 后执行 |

### Phase 4 已完成实现说明(与计划差异)

实际代码在 `agent/src/manager/mod.rs` 中以更简化方式接线,**未引入** `sighup_handle` 和 `hot_update_coordinator` 两个 Mutex 字段:

```rust
// 实际实现(agent/src/manager/mod.rs:186-198)
let (trigger_tx, trigger_rx) = tokio::sync::mpsc::channel(16);
let _sighup_handle = watch_sighup(trigger_tx);   // 句柄直接 drop,任务由 tokio::spawn 内部持有
let coordinator = HotUpdateCoordinator::new(
    self.worker_manager.clone(),
    self.ipc_server.clone(),
    trigger_rx,
);
tokio::spawn(coordinator.run());                  // 句柄直接 drop
```

**影响:** SIGHUP 监听和协调器在 Manager 退出时由 tokio runtime 自动回收,无法在 `shutdown()` 中显式 abort。当前实现可接受,因为 Manager 退出即进程退出。

**Task 8 不需要执行**,后续 Task 从 Task 9 开始。

---

## 迁移范围

### 迁移到 Worker(本计划覆盖)
- Delete / Mkdir / Rename / Copy / Move / FileExists(简单文件操作)
- ApplyDiff(差异应用,需 FileDiff 消息)
- ExecuteCommand(命令执行,需添加用户上下文)

### 保留在 quic.rs(设计决策,不迁移)
- **FileTransfer**: 流式大数据传输,经 IPC 转发增加延迟,已有 file_stream.rs 优化
- **Subscribe/Unsubscribe**: 持久 Stream 推送
- **TerminalSpawn/TerminalData**: 已用 PtyRegistry,持久 Stream
- **Ping/GetCurrentUser/GetStats**: Manager 级操作,无需 Worker

### Phase 4(热更新)
- 在 `start()` 中接线 SIGHUP + HotUpdateCoordinator

### Phase 5(死代码清理)
- 移除 `spawn_pty_output_task_legacy`、旧 `PtyManager`、死参数

---

## 文件结构

### 修改的文件

| 文件 | 责任 | 改动类型 |
|------|------|---------|
| `agent/protocol/agent.proto` | Protobuf 消息定义 | 新增消息类型 |
| `agent/src/protocol/generated.rs` | 手动维护的 Protobuf 生成代码 | 同步 proto 变更 |
| `agent/src/worker/handlers/file.rs` | Worker 文件操作 handlers | 新增 handler 函数 |
| `agent/src/worker/handlers/command.rs` | Worker 命令执行 handler | 添加用户隔离 |
| `agent/src/worker/mod.rs` | Worker 请求分发 | 新增 payload 分支 |
| `agent/src/manager/protocol_adapter.rs` | 协议适配层 | 扩展映射 |
| `agent/src/manager/mod.rs` | Manager 主结构 | 接线热更新 |
| `agent/src/server/quic.rs` | QUIC 服务器 | 清理死参数 |
| `agent/src/manager/pty_output.rs` | PTY 输出任务 | 删除 legacy 代码 |
| `agent/src/server/quic.rs` ConnectionContext | 连接清理 | 改用 PtyRegistry |

### 新增的文件
无(所有改动在现有文件上进行)

### 删除的文件
| 文件 | 原因 |
|------|------|
| `agent/src/pty.rs` | 旧 PtyManager,功能已被 PtyRegistry 替代(Phase 5 最后一步) |

---

## Phase 3 续: 简单文件操作迁移

### Task 1: 扩展 Protobuf 消息(Delete/Mkdir/Rename/Copy/Move/FileExists)

**Files:**
- Modify: `agent/protocol/agent.proto`

- [ ] **Step 1: 在 agent.proto 的 `ManagerRequest` oneof 中添加新 payload 类型**

在 `message ManagerRequest` 的 oneof 中,在 `GracefulShutdown` 之前添加:

```protobuf
        Delete delete = 11;
        Mkdir mkdir = 12;
        Rename rename = 13;
        Copy copy = 14;
        Move move = 15;
        FileExists file_exists = 16;
        ApplyDiff apply_diff = 17;
```

- [ ] **Step 2: 在 `WorkerResponse` oneof 中添加新响应类型**

在 `message WorkerResponse` 的 oneof 中,在 `ShutdownAck` 之前添加:

```protobuf
        DeleteResult delete_result = 10;
        MkdirResult mkdir_result = 11;
        RenameResult rename_result = 12;
        CopyResult copy_result = 13;
        MoveResult move_result = 14;
        FileExistsResult file_exists_result = 15;
        ApplyDiffResult apply_diff_result = 16;
```

- [ ] **Step 3: 在文件操作区块后添加新消息定义**

在 `message WriteResult` 之后,`// ===== 命令执行 =====` 之前添加:

```protobuf
// 删除文件/目录
message Delete {
    string path = 1;
    uint32 uid = 2;
    uint32 gid = 3;
    string username = 4;
    string home_dir = 5;
}

message DeleteResult {
    bool success = 1;
}

// 创建目录
message Mkdir {
    string path = 1;
    uint32 uid = 2;
    uint32 gid = 3;
    string username = 4;
    string home_dir = 5;
}

message MkdirResult {
    string path = 1;
}

// 重命名/移动
message Rename {
    string old_path = 1;
    string new_path = 2;
    uint32 uid = 3;
    uint32 gid = 4;
    string username = 5;
    string home_dir = 6;
}

message RenameResult {
    string old_path = 1;
    string new_path = 2;
}

// 复制(仅文件)
message Copy {
    string src = 1;
    string dst = 2;
    uint32 uid = 3;
    uint32 gid = 4;
    string username = 5;
    string home_dir = 6;
}

message CopyResult {
    string src = 1;
    string dst = 2;
}

// 移动(等同于 Rename)
message Move {
    string src = 1;
    string dst = 2;
    uint32 uid = 3;
    uint32 gid = 4;
    string username = 5;
    string home_dir = 6;
}

message MoveResult {
    string src = 1;
    string dst = 2;
}

// 检查文件是否存在
message FileExists {
    string path = 1;
    uint32 uid = 2;
    uint32 gid = 3;
    string username = 4;
    string home_dir = 5;
}

message FileExistsResult {
    bool exists = 1;
    uint64 size = 2;
    uint64 mtime = 3;
}
```

- [ ] **Step 4: 在命令执行区块后添加 ApplyDiff 相关消息**

在 `message CommandOutput` 之后,`// ===== 系统信息 =====` 之前添加:

```protobuf
// ===== 差异应用 =====

// 文件差异项
message FileDiff {
    // 差异类型: 0=insert, 1=delete, 2=replace
    int32 diff_type = 1;
    // 行号(从 1 开始)
    uint32 line_number = 2;
    // 原内容(replace/delete 时存在)
    string old_content = 3;
    // 新内容(replace/insert 时存在)
    string new_content = 4;
}

// 应用差异
message ApplyDiff {
    string path = 1;
    uint64 base_mtime = 2;
    repeated FileDiff diffs = 3;
    uint32 uid = 4;
    uint32 gid = 5;
    string username = 6;
    string home_dir = 7;
}

message ApplyDiffResult {
    string path = 1;
    bool success = 2;
    uint64 new_mtime = 3;
    string error = 4;
}
```

- [ ] **Step 5: 给 ExecuteCommand 添加用户上下文字段**

修改 `message ExecuteCommand`:

```protobuf
message ExecuteCommand {
    string command = 1;
    repeated string args = 2;
    string working_directory = 3;
    // 用户上下文(阶段 3 新增)
    uint32 uid = 4;
    uint32 gid = 5;
    string username = 6;
    string home_dir = 7;
}
```

---

### Task 2: 同步 generated.rs

**Files:**
- Modify: `agent/src/protocol/generated.rs`

- [ ] **Step 1: 更新 ManagerRequest 的 oneof tags**

将 `#[prost(oneof = "manager_request::Payload", tags = "2, 4, 5, 6, 7, 8, 9, 10")]` 改为:

```rust
#[prost(oneof = "manager_request::Payload", tags = "2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17")]
```

- [ ] **Step 2: 在 manager_request::Payload enum 中添加新变体**

在 `GracefulShutdown(super::GracefulShutdown),` 之前添加:

```rust
        #[prost(message, tag = "11")]
        Delete(super::Delete),
        #[prost(message, tag = "12")]
        Mkdir(super::Mkdir),
        #[prost(message, tag = "13")]
        Rename(super::Rename),
        #[prost(message, tag = "14")]
        Copy(super::Copy),
        #[prost(message, tag = "15")]
        Move(super::Move),
        #[prost(message, tag = "16")]
        FileExists(super::FileExists),
        #[prost(message, tag = "17")]
        ApplyDiff(super::ApplyDiff),
```

- [ ] **Step 3: 更新 WorkerResponse 的 oneof tags**

将 `#[prost(oneof = "worker_response::Payload", tags = "2, 3, 4, 5, 6, 7, 8, 9")]` 改为:

```rust
#[prost(oneof = "worker_response::Payload", tags = "2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16")]
```

- [ ] **Step 4: 在 worker_response::Payload enum 中添加新变体**

在 `ShutdownAck(super::ShutdownAck),` 之前添加:

```rust
        #[prost(message, tag = "10")]
        DeleteResult(super::DeleteResult),
        #[prost(message, tag = "11")]
        MkdirResult(super::MkdirResult),
        #[prost(message, tag = "12")]
        RenameResult(super::RenameResult),
        #[prost(message, tag = "13")]
        CopyResult(super::CopyResult),
        #[prost(message, tag = "14")]
        MoveResult(super::MoveResult),
        #[prost(message, tag = "15")]
        FileExistsResult(super::FileExistsResult),
        #[prost(message, tag = "16")]
        ApplyDiffResult(super::ApplyDiffResult),
```

- [ ] **Step 5: 添加新消息结构体定义**

在 `pub struct WriteResult { ... }` 之后添加所有新消息结构体(Delete/DeleteResult/Mkdir/MkdirResult/Rename/RenameResult/Copy/CopyResult/Move/MoveResult/FileExists/FileExistsResult/FileDiff/ApplyDiff/ApplyDiffResult),格式参照已有结构体。

- [ ] **Step 6: 更新 ExecuteCommand 结构体添加用户上下文字段**

```rust
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ExecuteCommand {
    #[prost(string, tag = "1")]
    pub command: ::prost::alloc::string::String,
    #[prost(string, repeated, tag = "2")]
    pub args: ::prost::alloc::vec::Vec<::prost::alloc::string::String>,
    #[prost(string, tag = "3")]
    pub working_directory: ::prost::alloc::string::String,
    /// 用户上下文(阶段 3 新增):UID
    #[prost(uint32, tag = "4")]
    pub uid: u32,
    /// 用户上下文(阶段 3 新增):GID
    #[prost(uint32, tag = "5")]
    pub gid: u32,
    /// 用户上下文(阶段 3 新增):用户名
    #[prost(string, tag = "6")]
    pub username: ::prost::alloc::string::String,
    /// 用户上下文(阶段 3 新增):家目录
    #[prost(string, tag = "7")]
    pub home_dir: ::prost::alloc::string::String,
}
```

- [ ] **Step 7: 编译验证**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过(可能有 warning,不能有 error)

---

### Task 3: 实现 Worker 文件操作 Handlers(Delete/Mkdir/Rename/Copy/Move/FileExists)

**Files:**
- Modify: `agent/src/worker/handlers/file.rs`

- [ ] **Step 1: 添加新消息类型的 import**

在文件顶部 `use crate::protocol::generated::{...}` 中添加:

```rust
    Delete, DeleteResult,
    Mkdir, MkdirResult,
    Rename, RenameResult,
    Copy, CopyResult,
    Move, MoveResult,
    FileExists as FileExistsReq, FileExistsResult,
```

- [ ] **Step 2: 实现 handle_delete 函数**

在 `handle_write_file` 之后添加:

```rust
/// 处理 Delete 请求
///
/// 在目标用户上下文中删除文件或目录。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_delete(req: Delete) -> WorkerResponse {
    tracing::info!("处理 Delete 请求: path={}, uid={}", req.path, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    let executor = UserExecutor::new(&session);

    let path = req.path.clone();
    let result: AnyhowResult<()> = executor.execute_as_user(move || {
        let p = Path::new(&path);
        let metadata = fs::metadata(p).map_err(|e| {
            anyhow::anyhow!("无法访问 '{}': {}", path, e)
        })?;

        if metadata.is_dir() {
            fs::remove_dir_all(p).map_err(|e| anyhow::anyhow!("删除目录失败: {}", e))?;
        } else {
            fs::remove_file(p).map_err(|e| anyhow::anyhow!("删除文件失败: {}", e))?;
        }
        Ok(())
    });

    match result {
        Ok(()) => {
            tracing::info!("删除成功: path={}", req.path);
            WorkerResponse {
                payload: Some(worker_response::Payload::DeleteResult(DeleteResult { success: true })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("删除失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}
```

- [ ] **Step 3: 实现 handle_mkdir 函数**

```rust
/// 处理 Mkdir 请求
///
/// 在目标用户上下文中创建目录(含父目录)。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_mkdir(req: Mkdir) -> WorkerResponse {
    tracing::info!("处理 Mkdir 请求: path={}, uid={}", req.path, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    let executor = UserExecutor::new(&session);

    let path = req.path.clone();
    let result: AnyhowResult<String> = executor.execute_as_user(move || {
        fs::create_dir_all(&path).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("Permission denied") {
                anyhow::anyhow!("权限不足: 无法创建目录 '{}' (需要相应的 Linux 用户权限)", path)
            } else {
                anyhow::anyhow!("无法创建目录 '{}': {}", path, e)
            }
        })?;
        Ok(path)
    });

    match result {
        Ok(path) => {
            tracing::info!("创建目录成功: path={}", path);
            WorkerResponse {
                payload: Some(worker_response::Payload::MkdirResult(MkdirResult { path })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("创建目录失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}
```

- [ ] **Step 4: 实现 handle_rename 函数**

```rust
/// 处理 Rename 请求
///
/// 在目标用户上下文中重命名文件或目录。
#[tracing::instrument(fields(old_path = %req.old_path, new_path = %req.new_path, uid = req.uid))]
pub async fn handle_rename(req: Rename) -> WorkerResponse {
    tracing::info!("处理 Rename 请求: {} -> {}, uid={}", req.old_path, req.new_path, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    let executor = UserExecutor::new(&session);

    let old_path = req.old_path.clone();
    let new_path = req.new_path.clone();
    let result: AnyhowResult<(String, String)> = executor.execute_as_user(move || {
        fs::rename(&old_path, &new_path).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("Permission denied") {
                anyhow::anyhow!("权限不足: 无法重命名 '{}' (需要相应的 Linux 用户权限)", old_path)
            } else {
                anyhow::anyhow!("无法重命名 '{}': {}", old_path, e)
            }
        })?;
        Ok((old_path, new_path))
    });

    match result {
        Ok((old, new)) => {
            tracing::info!("重命名成功: {} -> {}", old, new);
            WorkerResponse {
                payload: Some(worker_response::Payload::RenameResult(RenameResult {
                    old_path: old,
                    new_path: new,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("重命名失败: {} -> {}, error={}", req.old_path, req.new_path, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}
```

- [ ] **Step 5: 实现 handle_copy 函数**

```rust
/// 处理 Copy 请求
///
/// 在目标用户上下文中复制文件(不支持目录)。
#[tracing::instrument(fields(src = %req.src, dst = %req.dst, uid = req.uid))]
pub async fn handle_copy(req: Copy) -> WorkerResponse {
    tracing::info!("处理 Copy 请求: {} -> {}, uid={}", req.src, req.dst, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    let executor = UserExecutor::new(&session);

    let src = req.src.clone();
    let dst = req.dst.clone();
    let result: AnyhowResult<(String, String)> = executor.execute_as_user(move || {
        let metadata = fs::metadata(&src).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("Permission denied") {
                anyhow::anyhow!("权限不足: 无法访问源文件 '{}' (需要相应的 Linux 用户权限)", src)
            } else if msg.contains("No such file") {
                anyhow::anyhow!("源文件 '{}' 不存在", src)
            } else {
                anyhow::anyhow!("无法访问源文件 '{}': {}", src, e)
            }
        })?;

        if metadata.is_dir() {
            anyhow::bail!("不支持复制目录 '{}' (请使用移动功能)", src);
        }

        fs::copy(&src, &dst).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("Permission denied") {
                anyhow::anyhow!("权限不足: 无法复制 '{}' (需要相应的 Linux 用户权限)", src)
            } else {
                anyhow::anyhow!("无法复制 '{}': {}", src, e)
            }
        })?;
        Ok((src, dst))
    });

    match result {
        Ok((s, d)) => {
            tracing::info!("复制成功: {} -> {}", s, d);
            WorkerResponse {
                payload: Some(worker_response::Payload::CopyResult(CopyResult { src: s, dst: d })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("复制失败: {} -> {}, error={}", req.src, req.dst, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}
```

- [ ] **Step 6: 实现 handle_move 函数**

```rust
/// 处理 Move 请求
///
/// Move 等同于 Rename,在目标用户上下文中执行。
#[tracing::instrument(fields(src = %req.src, dst = %req.dst, uid = req.uid))]
pub async fn handle_move(req: Move) -> WorkerResponse {
    tracing::info!("处理 Move 请求: {} -> {}, uid={}", req.src, req.dst, req.uid);

    // Move 本质上是 Rename
    let rename_req = Rename {
        old_path: req.src,
        new_path: req.dst,
        uid: req.uid,
        gid: req.gid,
        username: req.username,
        home_dir: req.home_dir,
    };

    match handle_rename(rename_req).await {
        resp => match resp.payload {
            Some(worker_response::Payload::RenameResult(r)) => WorkerResponse {
                payload: Some(worker_response::Payload::MoveResult(MoveResult {
                    src: r.old_path,
                    dst: r.new_path,
                })),
                ..Default::default()
            },
            other => WorkerResponse { payload: other, ..Default::default() },
        }
    }
}
```

- [ ] **Step 7: 实现 handle_file_exists 函数**

```rust
/// 处理 FileExists 请求
///
/// 在目标用户上下文中检查文件是否存在,返回大小和修改时间。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_file_exists(req: FileExistsReq) -> WorkerResponse {
    tracing::info!("处理 FileExists 请求: path={}, uid={}", req.path, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    let executor = UserExecutor::new(&session);

    let path = req.path.clone();
    let result: AnyhowResult<Option<(u64, u64)>> = executor.execute_as_user(move || {
        match fs::metadata(&path) {
            Ok(meta) => {
                let size = meta.len();
                let mtime = meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                Ok(Some((size, mtime)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(anyhow::anyhow!("无法访问文件: {}", e)),
        }
    });

    match result {
        Ok(Some((size, mtime))) => {
            tracing::debug!("文件存在: path={}, size={}", req.path, size);
            WorkerResponse {
                payload: Some(worker_response::Payload::FileExistsResult(FileExistsResult {
                    exists: true,
                    size,
                    mtime,
                })),
                ..Default::default()
            }
        }
        Ok(None) => {
            tracing::debug!("文件不存在: path={}", req.path);
            WorkerResponse {
                payload: Some(worker_response::Payload::FileExistsResult(FileExistsResult {
                    exists: false,
                    size: 0,
                    mtime: 0,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("检查文件存在失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}
```

- [ ] **Step 8: 编译验证**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过

---

### Task 4: 实现 Worker ApplyDiff Handler

**Files:**
- Modify: `agent/src/worker/handlers/file.rs`

- [ ] **Step 1: 添加 ApplyDiff 相关 import**

在 `use crate::protocol::generated::{...}` 中添加:

```rust
    ApplyDiff as ApplyDiffReq, ApplyDiffResult,
    FileDiff as ProtoFileDiff,
```

在文件顶部添加:

```rust
use crate::diff::{FileDiff, DiffType, apply_diff};
```

- [ ] **Step 2: 实现 handle_apply_diff 函数**

在 `handle_file_exists` 之后添加:

```rust
/// 处理 ApplyDiff 请求
///
/// 在目标用户上下文中应用文件差异(mtime 版本校验 + 应用差异 + 写入)。
#[tracing::instrument(fields(path = %req.path, base_mtime = req.base_mtime, uid = req.uid))]
pub async fn handle_apply_diff(req: ApplyDiffReq) -> WorkerResponse {
    tracing::info!("处理 ApplyDiff 请求: path={}, base_mtime={}, diffs={}, uid={}",
        req.path, req.base_mtime, req.diffs.len(), req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    let executor = UserExecutor::new(&session);

    // 转换 protobuf FileDiff 为内部 FileDiff
    let diffs: Vec<FileDiff> = req.diffs.iter().map(|d| FileDiff {
        diff_type: match d.diff_type {
            0 => DiffType::Insert,
            1 => DiffType::Delete,
            2 => DiffType::Replace,
            _ => DiffType::Replace,
        },
        line_number: d.line_number as usize,
        old_content: if d.old_content.is_empty() { None } else { Some(d.old_content.clone()) },
        new_content: if d.new_content.is_empty() { None } else { Some(d.new_content.clone()) },
    }).collect();

    let path = req.path.clone();
    let base_mtime = req.base_mtime;
    let result: AnyhowResult<u64> = executor.execute_as_user(move || {
        // 获取文件当前 mtime
        let metadata = fs::metadata(&path)
            .map_err(|e| anyhow::anyhow!("无法获取文件信息 '{}': {}", path, e))?;
        let current_mtime = metadata
            .modified()
            .map_err(|e| anyhow::anyhow!("无法获取修改时间: {}", e))?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("时间转换失败: {}", e))?
            .as_secs();

        // 检查版本冲突
        if current_mtime != base_mtime {
            anyhow::bail!(
                "文件版本冲突: 期望 mtime={}, 实际 mtime={}",
                base_mtime, current_mtime
            );
        }

        // 读取原文件内容
        let old_content = fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("读取文件失败: {}", e))?;

        // 应用差异
        let new_content = apply_diff(&old_content, &diffs);

        // 写入文件
        fs::write(&path, &new_content)
            .map_err(|e| anyhow::anyhow!("写入文件失败: {}", e))?;

        // 获取新的 mtime
        let new_metadata = fs::metadata(&path)
            .map_err(|e| anyhow::anyhow!("无法获取新文件信息: {}", e))?;
        let new_mtime = new_metadata
            .modified()
            .map_err(|e| anyhow::anyhow!("无法获取新修改时间: {}", e))?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("时间转换失败: {}", e))?
            .as_secs();

        Ok(new_mtime)
    });

    match result {
        Ok(new_mtime) => {
            tracing::info!("应用差异成功: path={}, new_mtime={}", req.path, new_mtime);
            WorkerResponse {
                payload: Some(worker_response::Payload::ApplyDiffResult(ApplyDiffResult {
                    path: req.path,
                    success: true,
                    new_mtime,
                    error: String::new(),
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("应用差异失败: path={}, error={}", req.path, e);
            WorkerResponse {
                payload: Some(worker_response::Payload::ApplyDiffResult(ApplyDiffResult {
                    path: req.path,
                    success: false,
                    new_mtime: 0,
                    error: e.to_string(),
                })),
                ..Default::default()
            }
        }
    }
}
```

- [ ] **Step 3: 编译验证**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过

---

### Task 5: 在 Worker mod.rs 中注册新 handlers

**Files:**
- Modify: `agent/src/worker/mod.rs`

- [ ] **Step 1: 在 handle_request 函数的 match 中添加新分支**

在 `Some(crate::protocol::generated::manager_request::Payload::WriteFile(req)) => { ... }` 之后添加:

```rust
        Some(crate::protocol::generated::manager_request::Payload::Delete(req)) => {
            handlers::file::handle_delete(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::Mkdir(req)) => {
            handlers::file::handle_mkdir(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::Rename(req)) => {
            handlers::file::handle_rename(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::Copy(req)) => {
            handlers::file::handle_copy(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::Move(req)) => {
            handlers::file::handle_move(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::FileExists(req)) => {
            handlers::file::handle_file_exists(req).await
        }
        Some(crate::protocol::generated::manager_request::Payload::ApplyDiff(req)) => {
            handlers::file::handle_apply_diff(req).await
        }
```

- [ ] **Step 2: 编译验证**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过

---

### Task 6: 扩展协议适配层

**Files:**
- Modify: `agent/src/manager/protocol_adapter.rs`

- [ ] **Step 1: 添加新消息类型的 import**

在 `use crate::protocol::generated::{...}` 中添加:

```rust
    Delete, Mkdir, Rename, Copy, Move,
    FileExists as FileExistsReq, ApplyDiff as ApplyDiffReq,
    FileDiff as ProtoFileDiff,
```

- [ ] **Step 2: 在 serde_to_worker_request 中添加新映射**

在 `Payload::WriteFileRequest { ... }` 分支之后,`_ => None` 之前添加:

```rust
        Payload::DeleteRequest { path } => {
            tracing::debug!("适配 DeleteRequest: path={}", path);
            Some(manager_request::Payload::Delete(Delete {
                path: path.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::MkdirRequest { path } => {
            tracing::debug!("适配 MkdirRequest: path={}", path);
            Some(manager_request::Payload::Mkdir(Mkdir {
                path: path.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::RenameRequest { old_path, new_path } => {
            tracing::debug!("适配 RenameRequest: {} -> {}", old_path, new_path);
            Some(manager_request::Payload::Rename(Rename {
                old_path: old_path.clone(),
                new_path: new_path.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::CopyRequest { src, dst } => {
            tracing::debug!("适配 CopyRequest: {} -> {}", src, dst);
            Some(manager_request::Payload::Copy(Copy {
                src: src.clone(),
                dst: dst.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::MoveRequest { src, dst } => {
            tracing::debug!("适配 MoveRequest: {} -> {}", src, dst);
            Some(manager_request::Payload::Move(Move {
                src: src.clone(),
                dst: dst.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::FileExistsRequest { path } => {
            tracing::debug!("适配 FileExistsRequest: path={}", path);
            Some(manager_request::Payload::FileExists(FileExistsReq {
                path: path.clone(),
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }

        Payload::ApplyDiffRequest { path, base_mtime, diffs } => {
            tracing::debug!("适配 ApplyDiffRequest: path={}, diffs={}", path, diffs.len());
            // 转换 serde FileDiff 为 protobuf FileDiff
            let proto_diffs: Vec<ProtoFileDiff> = diffs.iter().map(|d| ProtoFileDiff {
                diff_type: match d.diff_type {
                    crate::diff::DiffType::Insert => 0,
                    crate::diff::DiffType::Delete => 1,
                    crate::diff::DiffType::Replace => 2,
                },
                line_number: d.line_number as u32,
                old_content: d.old_content.clone().unwrap_or_default(),
                new_content: d.new_content.clone().unwrap_or_default(),
            }).collect();
            Some(manager_request::Payload::ApplyDiff(ApplyDiffReq {
                path: path.clone(),
                base_mtime: *base_mtime,
                diffs: proto_diffs,
                uid: user.uid,
                gid: user.gid,
                username: user.username.clone(),
                home_dir: user.home_dir.clone(),
            }))
        }
```

- [ ] **Step 3: 在 worker_response_to_serde 中添加新响应映射**

在 `Some(worker_response::Payload::WriteResult(result)) => { ... }` 之后添加:

```rust
        Some(worker_response::Payload::DeleteResult(result)) => {
            tracing::debug!("适配 DeleteResult: success={}", result.success);
            Some(Payload::DeleteResponse { success: result.success })
        }

        Some(worker_response::Payload::MkdirResult(result)) => {
            tracing::debug!("适配 MkdirResult: path={}", result.path);
            Some(Payload::MkdirResponse {
                success: true,
                path: result.path,
            })
        }

        Some(worker_response::Payload::RenameResult(result)) => {
            tracing::debug!("适配 RenameResult: {} -> {}", result.old_path, result.new_path);
            Some(Payload::RenameResponse {
                success: true,
                old_path: result.old_path,
                new_path: result.new_path,
            })
        }

        Some(worker_response::Payload::CopyResult(result)) => {
            tracing::debug!("适配 CopyResult: {} -> {}", result.src, result.dst);
            Some(Payload::CopyResponse {
                success: true,
                src: result.src,
                dst: result.dst,
            })
        }

        Some(worker_response::Payload::MoveResult(result)) => {
            tracing::debug!("适配 MoveResult: {} -> {}", result.src, result.dst);
            Some(Payload::MoveResponse {
                success: true,
                src: result.src,
                dst: result.dst,
            })
        }

        Some(worker_response::Payload::FileExistsResult(result)) => {
            tracing::debug!("适配 FileExistsResult: exists={}", result.exists);
            Some(Payload::FileExistsResponse {
                exists: result.exists,
                size: if result.exists { Some(result.size) } else { None },
                mtime: if result.exists { Some(result.mtime) } else { None },
            })
        }

        Some(worker_response::Payload::ApplyDiffResult(result)) => {
            tracing::debug!("适配 ApplyDiffResult: path={}, success={}", result.path, result.success);
            Some(Payload::ApplyDiffResponse {
                path: result.path,
                success: result.success,
                new_mtime: result.new_mtime,
                error: if result.error.is_empty() { None } else { Some(result.error) },
            })
        }
```

- [ ] **Step 4: 编译验证 + 单元测试**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test --lib protocol_adapter 2>&1 | tail -20"`
Expected: 所有现有测试通过

---

### Task 7: Phase 3 续 - 编译 + 完整回归测试

- [ ] **Step 1: 完整编译检查**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -30"`
Expected: 零编译错误

- [ ] **Step 2: 运行所有 lib 测试**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test --lib 2>&1 | tail -15"`
Expected: 所有测试通过

- [ ] **Step 3: 运行 Phase 2 集成测试**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test --test phase2_integration_test 2>&1 | tail -10"`
Expected: 3 个测试通过

- [ ] **Step 4: 提交 Phase 3 续**

```bash
git add agent/protocol/agent.proto agent/src/protocol/generated.rs \
    agent/src/worker/handlers/file.rs agent/src/worker/mod.rs \
    agent/src/manager/protocol_adapter.rs
git commit -m "feat(phase3): migrate file ops (Delete/Mkdir/Rename/Copy/Move/FileExists/ApplyDiff) to Worker"
```

---

## Phase 4: 热更新机制启用

### Task 8: 在 Manager::start() 中接线 SIGHUP 和 HotUpdateCoordinator

**Files:**
- Modify: `agent/src/manager/mod.rs`

- [ ] **Step 1: 在 Manager 结构体中添加热更新相关字段**

在 `crash_detector` 字段之后添加:

```rust
    /// SIGHUP 信号监听句柄(阶段 4 启用)
    #[cfg(unix)]
    sighup_handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,

    /// 热更新协调器(阶段 4 启用)
    #[cfg(unix)]
    hot_update_coordinator: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
```

- [ ] **Step 2: 在 Manager::new() 中初始化新字段**

在 `Ok(Self { ... })` 中添加:

```rust
            sighup_handle: tokio::sync::Mutex::new(None),
            hot_update_coordinator: tokio::sync::Mutex::new(None),
```

- [ ] **Step 3: 在 Manager::start() 中启动 SIGHUP 监听和 HotUpdateCoordinator**

在 `*self.crash_detector.lock().await = Some(crash_detector);` 之后,`tracing::info!("Manager 已启动...")` 之前添加:

```rust
        // 启动 SIGHUP 信号监听(阶段 4 新增)
        // 收到 SIGHUP 时发送 ReloadTrigger 事件给热更新协调器
        let (trigger_tx, trigger_rx) = tokio::sync::mpsc::channel(16);
        let sighup_handle = watch_sighup(trigger_tx);

        // 启动热更新协调器(阶段 4 新增)
        // 监听触发事件,收到时执行 Worker 热更新流程
        let coordinator = HotUpdateCoordinator::new(
            self.worker_manager.clone(),
            self.ipc_server.clone(),
            trigger_rx,
        );
        let coordinator_handle = tokio::spawn(coordinator.run());

        *self.sighup_handle.lock().await = Some(sighup_handle);
        *self.hot_update_coordinator.lock().await = Some(coordinator_handle);
```

- [ ] **Step 4: 更新 start() 的日志消息**

将 `tracing::info!("Manager 已启动(IPC + Worker + CrashDetector)");` 改为:

```rust
        tracing::info!("Manager 已启动(IPC + Worker + CrashDetector + HotUpdate)");
```

- [ ] **Step 5: 在 Manager::shutdown() 中停止热更新组件**

在 `if let Some(mut detector) = self.crash_detector.lock().await.take()` 之前添加:

```rust
        // 停止热更新协调器
        if let Some(handle) = self.hot_update_coordinator.lock().await.take() {
            handle.abort();
            tracing::info!("热更新协调器已停止");
        }

        // 停止 SIGHUP 监听
        if let Some(handle) = self.sighup_handle.lock().await.take() {
            handle.abort();
            tracing::info!("SIGHUP 监听已停止");
        }
```

- [ ] **Step 6: 编译验证**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过

- [ ] **Step 7: 运行测试确保不破坏现有功能**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test --lib 2>&1 | tail -15"`
Expected: 所有测试通过

- [ ] **Step 8: 提交 Phase 4**

```bash
git add agent/src/manager/mod.rs
git commit -m "feat(phase4): enable SIGHUP + HotUpdateCoordinator in Manager::start()"
```

---

## Phase 5: 死代码清理

### Task 9: 清理 pty_output.rs 中的 legacy 函数

**Files:**
- Modify: `agent/src/manager/pty_output.rs`
- Modify: `agent/src/manager/mod.rs`

- [ ] **Step 1: 确认 legacy 函数无外部调用**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && grep -rn 'spawn_pty_output_task_legacy\|spawn_pty_output_task[^_]' src/ --include='*.rs' | grep -v 'pty_output.rs'"`
Expected: 无结果(或只有注释引用)

- [ ] **Step 2: 删除 pty_output.rs 中的 legacy 函数和 impl 块**

需要删除以下三部分(参见 `agent/src/manager/pty_output.rs`):

1. `pub async fn spawn_pty_output_task_legacy(...)` 函数(约 line 127)
2. `pub async fn spawn_pty_output_task(...)` 函数(无后缀版本,约 line 103,保留 `_v2`)
3. `impl PtyReader for PtyManager` 块(约 line 188-190,`#[cfg(unix)]` 标注)

保留:
- `pub struct PtyOutputConfig` 配置结构体
- `pub async fn spawn_pty_output_task_v2(...)` 函数(唯一使用的版本)

同时检查测试模块中的 `_assert_pty_reader_implemented::<PtyManager>();`(约 line 373),如果存在则一并删除。

- [ ] **Step 3: 更新 mod.rs 的 pub use**

将 `agent/src/manager/mod.rs:44`:
```rust
pub use pty_output::{PtyOutputConfig, spawn_pty_output_task, spawn_pty_output_task_legacy, spawn_pty_output_task_v2};
```
改为:
```rust
pub use pty_output::{PtyOutputConfig, spawn_pty_output_task_v2};
```

- [ ] **Step 4: 编译验证**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过

---

### Task 10: 迁移 ConnectionContext cleanup 到 PtyRegistry

**Files:**
- Modify: `agent/src/server/quic.rs`

- [ ] **Step 1: 修改 ConnectionContext::cleanup 签名**

将:
```rust
    pub async fn cleanup(
        &self,
        pty_manager: &PtyManager,
        subscription_manager: &SubscriptionManager,
    ) {
```
改为:
```rust
    pub async fn cleanup(
        &self,
        pty_registry: &crate::manager::PtyRegistry,
        subscription_manager: &SubscriptionManager,
    ) {
```

- [ ] **Step 2: 修改 cleanup 中的 PTY 清理逻辑**

将:
```rust
            match pty_manager.remove(session_id).await {
                Ok(_) => { ... }
                Err(e) => { ... }
            }
```
改为:
```rust
            match pty_registry.unregister(session_id).await {
                Ok(_) => {
                    tracing::info!("[ConnectionContext] 已清理 PTY 会话: {}", session_id);
                }
                Err(e) => {
                    tracing::debug!("[ConnectionContext] PTY 会话清理（可能已移除）: {}: {}", session_id, e);
                }
            }
```

注意: `PtyRegistry::unregister` 返回 `Result<Option<PtySession>>`(见 `agent/src/manager/pty_registry.rs:67`),与旧 `PtyManager::remove` 的 `Result<()>` 不同。session_id 不存在时返回 `Ok(None)` 而非 `Err`,因此清理逻辑中 `Ok(_)` 分支即可覆盖,`Err` 仅在内部锁中毒等异常情况触发。

- [ ] **Step 3: 修改所有 cleanup 调用点**

搜索所有 `ctx.cleanup(pty_manager` 调用,改为 `ctx.cleanup(manager.pty_registry()`。

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && grep -n 'cleanup(pty_manager' src/server/quic.rs"`

将每处调用改为使用 `manager.pty_registry()`。

- [ ] **Step 4: 编译验证**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过

---

### Task 11: 移除 quic.rs 中的 _pty_manager 死参数

**Files:**
- Modify: `agent/src/server/quic.rs`

- [ ] **Step 1: 从 handle_stream 签名中移除 _pty_manager 参数**

将:
```rust
async fn handle_stream(
    stream: (SendStream, RecvStream),
    cfg: &AgentConfig,
    subscription_manager: Arc<SubscriptionManager>,
    event_bus: Arc<EventBus>,
    #[cfg(unix)] _pty_manager: Arc<PtyManager>,
    #[cfg(not(unix))] _pty_manager: Arc<PtyManager>,
    ctx: Arc<ConnectionContext>,
    session: &UserSession,
    stats_manager: Arc<StatsManager>,
    #[cfg(unix)]
    manager: Arc<crate::manager::Manager>,
) -> Result<()> {
```
改为:
```rust
async fn handle_stream(
    stream: (SendStream, RecvStream),
    cfg: &AgentConfig,
    subscription_manager: Arc<SubscriptionManager>,
    event_bus: Arc<EventBus>,
    ctx: Arc<ConnectionContext>,
    session: &UserSession,
    stats_manager: Arc<StatsManager>,
    #[cfg(unix)]
    manager: Arc<crate::manager::Manager>,
) -> Result<()> {
```

- [ ] **Step 2: 修改 handle_stream 的所有调用点**

搜索 `handle_stream(` 调用,移除 `pty_manager.clone()` 参数。

- [ ] **Step 3: 从 handle_connection 和 run 签名中移除 pty_manager 参数**

按调用链追溯,移除 `run()`、`handle_connection()` 中的 `pty_manager` 参数。

- [ ] **Step 4: 修改 main.rs 中的调用**

从 `server::quic::run(...)` 调用中移除 `pty_manager` 参数。

- [ ] **Step 5: 编译验证**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过

---

### Task 12: 删除旧 pty.rs 文件

**Files:**
- Delete: `agent/src/pty.rs`
- Modify: `agent/src/lib.rs`
- Modify: `agent/src/server/quic.rs`(移除 `use crate::pty::PtyManager;` 导入)

- [ ] **Step 1: 确认 pty.rs 无其他引用**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && grep -rn 'crate::pty\|use.*pty::PtyManager' src/ --include='*.rs'"`
Expected: 仅剩 `agent/src/server/quic.rs:16` 的 `use crate::pty::PtyManager;`(Task 10/11 完成后已无使用,需在此步删除)

- [ ] **Step 2: 从 lib.rs 中移除 pty 模块声明**

删除 `pub mod pty;`(位于 `agent/src/lib.rs:17`)。

- [ ] **Step 3: 从 quic.rs 中移除 PtyManager 导入**

删除 `agent/src/server/quic.rs:16` 的 `use crate::pty::PtyManager;` 行(Task 10/11 完成后此导入已无使用,Rust 会产生 unused import warning)。

- [ ] **Step 4: 删除 pty.rs 文件**

```bash
rm agent/src/pty.rs
```

- [ ] **Step 5: 编译验证**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过

---

### Task 13: 清理 handler.rs 中已迁移的函数(可选)

**Files:**
- Modify: `agent/src/handler.rs`

> **注意**: 此 Task 为可选,保留 handler.rs 中的函数作为非 Unix 平台回退。如果确定不需要非 Unix 支持,可以删除。

- [ ] **Step 1: 评估是否保留**

如果不需要非 Unix 平台支持,继续;否则跳过此 Task。

- [ ] **Step 2: 删除已迁移到 Worker 的 handler 函数**

删除以下函数(它们在 Unix 生产路径下不会被调用):
- `handle_read_dir` / `handle_read_file` / `handle_write_file`
- `handle_delete` / `handle_mkdir` / `handle_rename` / `handle_copy` / `handle_move`
- `handle_apply_diff` / `handle_file_exists`
- `format_permissions`(已在 worker/handlers/file.rs 中有副本)

- [ ] **Step 3: 更新 handle_envelope 中的 match 分支**

将已删除函数的 match 分支改为返回错误:
```rust
        Payload::ReadDirRequest { .. } => {
            error_response(envelope.request_id, "ReadDir 已迁移到 Worker,本地 handler 不可用")
        }
```

- [ ] **Step 4: 编译验证**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -20"`
Expected: 编译通过

---

### Task 14: Phase 5 最终回归测试

- [ ] **Step 1: 完整编译检查**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo check 2>&1 | tail -30"`
Expected: 零编译错误

- [ ] **Step 2: 运行所有测试**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo test 2>&1 | tail -20"`
Expected: 所有测试通过

- [ ] **Step 3: 提交 Phase 5**

```bash
git add agent/src/manager/pty_output.rs agent/src/manager/mod.rs \
    agent/src/server/quic.rs agent/src/lib.rs
git rm agent/src/pty.rs
git commit -m "refactor(phase5): remove dead code (legacy pty_output, old PtyManager, _pty_manager param)"
```

---

## 验证清单

完成所有 Task 后,进行以下手动验证:

- [ ] 启动 agent,确认日志显示 "Manager 已启动(IPC + Worker + CrashDetector + HotUpdate)"
- [ ] 客户端连接,测试文件浏览(ReadDir)、读取(ReadFile)、编辑保存(WriteFile)
- [ ] 测试文件删除(Delete)、新建目录(Mkdir)、重命名(Rename)、复制(Copy)、移动(Move)
- [ ] 测试文件存在检查(FileExists)
- [ ] 测试文件差异应用(ApplyDiff)
- [ ] 测试终端创建、输入、resize(Phase 2 功能回归)
- [ ] 测试文件上传/下载(FileTransfer,应不受影响)
- [ ] 测试订阅功能(Subscribe,应不受影响)
- [ ] 执行 `kill -HUP <worker_pid>`,验证 Worker 重启后会话不中断
- [ ] 执行 `kill -9 <worker_pid>`,验证 CrashDetector 自动重启
- [ ] 确认非 root 用户的文件权限隔离正常

---

## 不在本次范围

以下操作**有意保留在 quic.rs / handler.rs**,不迁移到 Worker:

| 操作 | 原因 |
|------|------|
| FileTransfer(FileTransferRequest/FileChunk/FileTransferComplete) | 流式大数据传输,经 IPC 转发增加延迟 |
| Subscribe/Unsubscribe | 持久 Stream 推送,不适合 request-response 模型 |
| TerminalSpawn/TerminalData | 已用 PtyRegistry,持久 Stream |
| Ping/Pong | 轻量级心跳,无需 Worker |
| GetCurrentUser | 读取 session.username,Manager 级操作 |
| GetStats | 读取 StatsManager,Manager 级操作 |
| GetMounts/MetricsSubscribe | 系统信息采集,低频调用,迁移价值低 |

---

## 参考

- [设计文档](file:///e:/MyWork/gnome-remote/docs/superpowers/specs/2026-08-04-manager-worker-integration-design.md)
- [Phase 4 热更新设计](file:///e:/MyWork/gnome-remote/docs/superpowers/specs/2026-08-03-phase4-hot-update-design.md)
