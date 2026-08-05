# 阶段 2:PTY 创建迁移到 Worker

## 目标

将 PTY 创建从 Manager(quic.rs 直接 forkpty)迁移到 Worker 子进程,通过 IPC + SCM_RIGHTS 传递 master_fd。

## 当前状态

- `quic.rs:1149` 直接调用 `pty_manager.spawn_as_user()` 在 Manager 进程中 forkpty
- Worker 的 `PtyFactory::create()` 已实现但从未被调用(Worker 主循环从未收到 CreateSession 请求)
- `IpcServer` 只有 `receive_and_register_fd()`(单向接收 FD),**没有 `send_request()` 方法**
- `IpcConnection` 每次 `receive_and_register_fd` 后被 `remove` 从连接表,不支持复用
- Worker 的 `PtyFactory::create()` 不支持用户隔离(没有 setuid/setgid)

## 关键技术决策

1. **IPC 双向通信**:Worker 的 `IpcClient` 已支持双向(`receive_request` + `send_response` + `send_fd`)。Manager 的 `IpcServer` 需要新增 `send_request` + `receive_response`。
2. **FD 与响应协调**:Worker 先 `send_fd(master_fd)`,再 `send_response(SessionCreated)`。Manager 先 `receive_fd`(recvmsg),再 `receive_response`(read_exact)。UnixStream 保证顺序。
3. **连接复用**:`IpcConnection` 不再在 `receive_and_register_fd` 后 remove,保持连接活跃。
4. **用户隔离迁移**:`CreateSession` 消息新增 `uid/gid/username/home_dir/shell` 字段,Worker 的 `PtyFactory::create` 在 forkpty 子进程中 setgid/setuid。

## 任务

### Task 1:扩展 CreateSession 协议消息

**文件**: `agent/protocol/agent.proto`

修改 `CreateSession` 消息,新增用户信息字段:

```protobuf
message CreateSession {
    uint32 cols = 1;
    uint32 rows = 2;
    string shell = 3;
    string working_directory = 4;
    // 阶段 2 新增:用户隔离信息
    uint32 uid = 5;
    uint32 gid = 6;
    string username = 7;
    string home_dir = 8;
}
```

**验证**: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo build --build 2>&1 | tail -5"`(重新生成 prost 代码并编译)

### Task 2:Worker PtyFactory 增加用户隔离

**文件**: `agent/src/worker/pty_factory.rs`

修改 `PtyFactory::create` 方法签名,新增用户信息参数:

```rust
pub fn create(
    &self,
    ipc_client: &IpcClient,
    shell: &str,
    cols: u32,
    rows: u32,
    cwd: Option<&str>,
    user_info: Option<&UserInfo>,  // 新增
) -> Result<(String, i32)>
```

在 `ForkptyResult::Child` 分支中,如果 `user_info` 是 `Some`,则:
- `setgid(user_info.gid)` 然后 `setuid(user_info.uid)`(顺序重要)
- 设置 `HOME/USER/LOGNAME/SHELL` 环境变量
- 使用 `user_info.home_dir` 作为默认工作目录

从 `pty.rs` 的 `spawn_as_user` 方法复制用户切换逻辑(已验证可用)。

**验证**: `cargo check`

### Task 3:Worker session handler 传递用户信息

**文件**: `agent/src/worker/handlers/session.rs`

修改 `handle_create_session`:
- 从 `CreateSession` 请求中提取 `uid/gid/username/home_dir`
- 构造 `UserInfo`(从 `pty_registry` 模块导入或定义在 protocol 中)
- 传递给 `pty_factory.create()`

**验证**: `cargo check`

### Task 4:IpcServer 新增双向通信能力(核心)

**文件**: `agent/src/manager/ipc_server.rs`

#### 4.1 IpcConnection 新增方法

```rust
impl IpcConnection {
    /// 发送 ManagerRequest 到 Worker
    pub async fn send_request(&mut self, request: &ManagerRequest) -> Result<()> {
        // 复用 IpcClient 的消息格式:4字节长度 + protobuf
        // 但这里是从 Manager 端发送,需要自己实现
        if let Some(ref stream) = self.stream {
            let mut buf = Vec::new();
            request.encode(&mut buf)?;
            let len = buf.len() as u32;
            stream.write_all(&len.to_be_bytes()).await?;
            stream.write_all(&buf).await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No stream available"))
        }
    }

    /// 接收 WorkerResponse(普通 protobuf 消息,不含 FD)
    pub async fn receive_response(&mut self) -> Result<WorkerResponse> {
        if let Some(ref stream) = self.stream {
            let mut len_buf = [0u8; 4];
            stream.read_exact(&mut len_buf).await?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut msg_buf = vec![0u8; len];
            stream.read_exact(&mut msg_buf).await?;
            let msg = WorkerResponse::decode(&msg_buf[..])?;
            Ok(msg)
        } else {
            Err(anyhow::anyhow!("No stream available"))
        }
    }
}
```

#### 4.2 IpcServer 新增方法

```rust
impl IpcServer {
    /// 发送 CreateSession 请求到 Worker 并接收 FD + 响应
    ///
    /// 这是原子的 request-response 操作:
    /// 1. 发送 CreateSession 请求
    /// 2. 接收 master_fd (SCM_RIGHTS)
    /// 3. 接收 SessionCreated 响应
    /// 4. 注册到 PtyRegistry
    pub async fn create_pty_session(
        &self,
        request: CreateSession,
        user_info: UserInfo,
    ) -> Result<String> {
        // 获取活跃连接(不 remove,只 get_mut)
        let mut connections = self.connections.write().await;
        let connection = connections.values_mut().next()
            .ok_or_else(|| anyhow::anyhow!("No active Worker connection"))?;

        // 1. 发送请求
        let manager_request = ManagerRequest {
            request_id: ...,
            payload: Some(ManagerRequestPayload::CreateSession(request)),
        };
        connection.send_request(&manager_request).await?;

        // 2. 接收 FD
        let master_fd = connection.receive_fd()?;

        // 3. 接收响应
        let response = connection.receive_response().await?;

        // 4. 解析 session_id 并注册
        match response.payload {
            Some(SessionCreated { session_id }) => {
                let pty_session = PtySession {
                    session_id: session_id.clone(),
                    master_fd,
                    user_info,
                    created_at: SystemTime::now(),
                };
                self.pty_registry.register(pty_session).await?;
                Ok(session_id)
            }
            Some(Error { code, message }) => {
                close(master_fd);
                Err(anyhow!("Worker error: {} {}", code, message))
            }
            _ => Err(anyhow!("Unexpected response")),
        }
    }
}
```

**关键**: `receive_and_register_fd` 不再使用(它 remove 连接)。新的 `create_pty_session` 只 get_mut,不 remove。

**验证**: `cargo check`

### Task 5:Manager 暴露 create_pty_session 给 quic.rs

**文件**: `agent/src/manager/mod.rs`

在 `Manager` 上新增公共方法:

```rust
/// 创建 PTY 会话(通过 Worker)
pub async fn create_pty_session(
    &self,
    shell: &str,
    cols: u32,
    rows: u32,
    cwd: Option<&str>,
    user_session: &UserSession,
) -> Result<String> {
    let request = CreateSession {
        cols, rows, shell: shell.to_string(),
        working_directory: cwd.unwrap_or("").to_string(),
        uid: user_session.uid,
        gid: user_session.gid,
        username: user_session.username.clone(),
        home_dir: user_session.home_dir.to_string_lossy().to_string(),
    };
    let user_info = UserInfo::new(
        user_session.username.clone(),
        user_session.uid,
        user_session.gid,
    );
    self.ipc_server.create_pty_session(request, user_info).await
}

/// 获取 PtyRegistry 引用(供 quic.rs 读写 PTY)
pub fn pty_registry(&self) -> &Arc<PtyRegistry> {
    &self.pty_registry
}
```

**验证**: `cargo check`

### Task 6:quic.rs 改造 — PTY 创建走 Worker

**文件**: `agent/src/server/quic.rs`

#### 6.1 修改函数签名

`handle_terminal_stream` 和 `TerminalSpawnRequest` 处理分支需要访问 Manager(或 IpcServer + PtyRegistry)。

在 `run` 函数中,将 `manager: Option<Arc<Manager>>` 传递到 `handle_payload`。

#### 6.2 修改 TerminalSpawnRequest 处理

将:
```rust
let session_id = pty_manager.spawn_as_user(&shell, *cols, *rows, working_directory.as_deref(), session).await?;
```

改为:
```rust
#[cfg(unix)]
let session_id = if let Some(ref manager) = manager {
    // 阶段 2:通过 Worker 创建 PTY
    manager.create_pty_session(&shell, *cols as u32, *rows as u32, working_directory.as_deref(), session).await?
} else {
    // 回退:直接创建(不应到达此处,Manager 总是存在)
    pty_manager.spawn_as_user(&shell, *cols, *rows, working_directory.as_deref(), session).await?
};
```

#### 6.3 修改 PTY 读写

`handle_terminal_stream` 中的 `pty_manager.write/read/remove` 改用 `pty_registry`:

```rust
// 从 PtyRegistry 获取 FD 进行读写
let registry = manager.pty_registry();
registry.write(&session_id, &data).await?;
registry.read(&session_id).await?;
registry.unregister(&session_id).await?;
```

`spawn_pty_output_task_legacy` 改为从 PtyRegistry 读取。

**验证**: `cargo build --release`

### Task 7:pty_output.rs 适配 PtyRegistry

**文件**: `agent/src/manager/pty_output.rs`

修改 `spawn_pty_output_task_legacy` 或新增 `spawn_pty_output_task_v2`,从 `PtyRegistry` 读取 PTY 输出而非 `PtyManager`。

**验证**: `cargo check`

### Task 8:集成测试

**文件**: `agent/tests/phase2_integration_test.rs`

测试:
1. Manager 启动 → create_pty_session → 获取 session_id
2. PtyRegistry 中能找到 session
3. 写入数据到 PTY → 读取输出
4. resize PTY
5. 注销 session

**验证**: `cargo test --test phase2_integration_test`

### Task 9:回归测试 + 端到端验证

- `cargo test --lib --tests`(所有现有测试通过)
- 启动 agent,客户端连接,创建终端会话,验证 shell 可交互
- 验证用户隔离(`whoami` 输出正确用户)

## 风险与缓解

1. **FD 接收和消息接收的顺序**:Worker 先 send_fd(1字节+SCM_RIGHTS)再 send_response(4字节+protobuf)。Manager 端 recvmsg 读取 1 字节+FD,然后 read_exact 读取 4 字节+protobuf。需验证 UnixStream 的 recvmsg 和 read_exact 能正确协调(不吞字节)。
   - **缓解**: Task 4 实现后先写专门的 IPC 双向通信测试
2. **quic.rs 改动范围大**:quic.rs 有 1500+ 行,改动涉及函数签名传递。
   - **缓解**: 使用 `Option<Arc<Manager>>` 保持向后兼容,逐步替换
3. **Worker 崩溃后 PTY 丢失**:Worker 崩溃时,所有由它创建的 PTY 子进程成为孤儿。
   - **缓解**: 阶段 2 暂不处理(CrashDetector 重启 Worker 后,旧 PTY 由孤儿收割器处理),阶段 3 解决
