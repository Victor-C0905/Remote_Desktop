# 并发 IPC 与 fork 安全设计（路径 C 最终版）

## 1. 背景与问题

### 1.1 报告问题
- **症状**：打开终端有一定概率失败，报错 `创建终端会话失败: No active Worker connection`
- **根因**：[ipc_server.rs:278-311](file:///e:\MyWork\gnome-remote\agent\src\manager\ipc_server.rs) 的 `send_request` / `create_pty_session` 采用"取出 → await → 放回"模式。并发请求时，连接被请求1 取走后，请求2 发现 HashMap 为空，直接报错

### 1.2 发现的潜在隐患（代码审查）
- **Manager 进程的 fork 隐患**：5 处 `fork()` 调用（4 个 `spawn_isolated_*` + 1 个 `execute_as_user`）在多线程 tokio runtime 中执行，fork 时其他 task 可能持有 runtime 内部锁（malloc arena 锁、IO driver 锁），子进程可能死锁
- **Worker 进程的 fork 隐患**：11 处 `fork()`（10 个 `execute_as_user` + 1 个 `create_session`），当前串行模式下 runtime 静止、fork 时无并发 task，隐患被掩盖

### 1.3 不在本次范围
- Worker 侧并发 dispatch（会暴露 fork 隐患，需等 posix_spawn 完全替代后再做）
- Worker 的 `execute_as_user` / `create_session` 改造（串行模式下已安全，本次不改）
- 热更新流程改造
- IPC 协议变更

## 2. 设计目标

1. **消除竞态**：消除 `IpcServer` 的"取出-放回"竞态
2. **消除 Manager fork 隐患**：Manager 侧所有 fork 改用 posix_spawn
3. **不引入新隐患**：不引入 fd 管理复杂度，不破坏旧功能
4. **旧功能完全不变**：所有对外接口（WorkerResponse、IPC 协议、CLI）保持不变
5. **部署结构不变**：不增加新二进制，沿用 `agent` 单二进制 + clap 子命令模式
6. **Worker 侧零改动**：保持串行 run() 循环，不暴露 fork 隐患

## 3. 架构设计

### 3.1 总体方案

```
路径 A + ForkGuard + Manager posix_spawn
├── Manager 侧
│   ├── mpsc + request_id + dispatcher（消除竞态）
│   ├── 5 个 fork 点改 posix_spawn（消除 Manager fork 隐患）
│   └── ForkGuard（tokio::sync::Mutex，额外保险）
├── Worker 侧
│   ├── 保持串行 run()（不改，runtime 静止保证 fork 安全）
│   └── ForkGuard（额外保险）
└── agent 子命令
    ├── --isolated-writer（替代 spawn_isolated_writer 的 fork）
    ├── --isolated-reader（替代 spawn_isolated_reader 的 fork）
    ├── --isolated-writer-part（替代 spawn_isolated_writer_part 的 fork）
    ├── --isolated-merger（替代 spawn_isolated_merger 的 fork）
    └── --metadata（替代 handler.rs:519 的 execute_as_user fork）
```

### 3.2 关键决策

| 决策点 | 选择 | 理由 |
|---|---|---|
| 二进制拆分方式 | agent clap 子命令 | 不改部署结构，install.sh 不需更新 |
| Worker 是否并发 | **否**，保持串行 | 串行模式下 fork 安全，不引入新隐患 |
| Manager fork 替代 | posix_spawn（Command::new + spawn） | 消除 fork+多线程死锁风险 |
| Worker fork 替代 | **否** | 串行模式下已安全，改造成本过高 |
| execute_as_user 处理 | Manager 侧改 posix_spawn，Worker 侧不改 | Manager 多线程需消除，Worker 串行已安全 |
| ForkGuard | 引入，作为额外保险 | 防御未来意外引入的并发 fork |

### 3.3 为什么 Worker 不并发

- Worker 串行模式下，fork 时 tokio runtime 处于静止状态（无并发 task 持有锁），fork 安全
- 若引入并发 dispatch，fork 时其他 task 可能持有 runtime 锁，**暴露 fork 隐患**
- 当前用户报告的问题（竞态）在 Manager 侧，与 Worker 并发无关
- Worker 并发需先完成所有 fork 点的 posix_spawn 改造，属于未来工作

## 4. Manager 侧重构

### 4.1 IpcServer 结构变化

```rust
// 旧结构（竞态根源）
pub struct IpcServer {
    connections: Arc<RwLock<HashMap<u64, IpcConnection>>>,
    // ...
}

// 新结构（消除竞态）
pub struct IpcServer {
    // 请求入口：dispatcher task 从此 channel 取请求转发给 Worker
    request_tx: mpsc::Sender<RequestItem>,
    // 等待中的请求：request_id → oneshot::Sender
    pending: Arc<DashMap<u64, oneshot::Sender<WorkerResponse>>>,
    // dispatcher task 的 JoinHandle（用于 stop 时 abort）
    dispatcher_handle: Mutex<Option<JoinHandle<()>>>,
    // 其他不变字段
    socket_path: PathBuf,
    // ...
}

pub struct RequestItem {
    pub request_id: u64,
    pub payload: ClientRequest,
    pub response_tx: oneshot::Sender<WorkerResponse>,
}
```

### 4.2 dispatcher task

```rust
async fn run_dispatcher(
    mut rx: mpsc::Receiver<RequestItem>,
    pending: Arc<DashMap<u64, oneshot::Sender<WorkerResponse>>>,
    mut write_half: OwnedWriteHalf,
    mut read_half: OwnedReadHalf,
) {
    let (response_tx, mut response_rx) = mpsc::channel::<(u64, WorkerResponse)>(128);

    // 响应路由 task：从 IPC read，按 request_id 路由到对应 oneshot
    let response_router = tokio::spawn(async move {
        loop {
            match read_response(&mut read_half).await {
                Ok((request_id, response)) => {
                    if let Some((_, sender)) = pending.remove(&request_id) {
                        let _ = sender.send(response);
                    }
                }
                Err(_) => {
                    // 连接断开，通知所有等待者
                    pending.clear();
                    break;
                }
            }
        }
    });

    // 请求转发循环
    while let Some(item) = rx.recv().await {
        pending.insert(item.request_id, item.response_tx);
        if write_request(&mut write_half, item.request_id, item.payload).await.is_err() {
            pending.remove(&item.request_id);
            break;
        }
    }

    // 清理所有等待者
    pending.clear();
    response_router.await.ok();
}
```

### 4.3 send_request / create_pty_session 重写

```rust
pub async fn send_request(
    &self,
    payload: ClientRequest,
) -> Result<WorkerResponse> {
    let request_id = next_request_id();
    let (tx, rx) = oneshot::channel();
    let item = RequestItem { request_id, payload, response_tx: tx };

    self.request_tx.send(item).await
        .map_err(|_| anyhow!("dispatcher dropped"))?;

    rx.await.map_err(|_| anyhow!("response dropped"))
}

pub async fn create_pty_session(
    &self,
    req: CreatePtySessionRequest,
) -> Result<WorkerResponse> {
    self.send_request(ClientRequest::CreatePtySession(req)).await
}
```

### 4.4 连接生命周期

- `accept_and_set_pid`：Worker 连接到达时，split UnixStream，启动 dispatcher task，替换旧 dispatcher（abort 旧的）
- `stop`：abort dispatcher，清理 pending，关闭 socket
- 热更新：`WorkerStatus::Starting` 事件触发 `accept_and_set_pid`，旧 dispatcher 被 abort，pending 中的等待者收到"dispatcher dropped"错误

## 5. Manager fork 改造（posix_spawn）

### 5.1 ForkGuard

`FORK_GUARD` 用于串行化所有 `spawn_isolated_*` 调用（posix_spawn 后子进程是独立新程序不继承 runtime 状态，ForkGuard 仅作为防止并发 spawn 带来 fd 资源竞争的额外保险）。

由于 `spawn_isolated_*` 是同步函数（保持原签名不变），ForkGuard 用 `std::sync::Mutex`：

```rust
// executor.rs
use std::sync::Mutex;

static FORK_GUARD: Mutex<()> = Mutex::new(());

/// 串行化所有 spawn_isolated_* 调用
/// 在 posix_spawn 方案下，子进程是独立新程序不继承 runtime 状态，
/// ForkGuard 仅作为防止并发 spawn 带来 fd 资源竞争的额外保险
fn with_fork_guard() -> std::sync::MutexGuard<'static, ()> {
    FORK_GUARD.lock().unwrap()
}
```

**为什么用 `std::sync::Mutex` 而非 `tokio::sync::Mutex`**：
- `spawn_isolated_*` 保持同步签名不变（不破坏调用方）
- `posix_spawn` + pipe 建立过程是毫秒级，阻塞 tokio worker thread 可接受
- 若用 `tokio::sync::Mutex`，则 `spawn_isolated_*` 必须改 async，破坏所有调用方

**使用方式**：
```rust
pub fn spawn_isolated_writer(&self, ...) -> Result<(ChildStdin, i32)> {
    let _guard = with_fork_guard();  // 短暂持有，spawn 完成即释放
    // ... posix_spawn ...
}
```

**注意**：Worker 的 `execute_as_user`（10 处）仍用 fork，但 Worker 串行模式下无需 ForkGuard 保护（runtime 静止）。ForkGuard 仅保护 Manager 侧的 `spawn_isolated_*` 系列。

### 5.2 子命令分发（main.rs）

子命令采用 clap 互斥分组：同一时间只执行一个子命令，避免参数混乱。

```rust
#[derive(Parser, Debug)]
struct Args {
    // ── 现有参数 ──
    #[arg(short, long, default_value = "agent.toml")]
    config: String,
    #[arg(long, default_value = "logs")]
    log_dir: String,
    #[arg(long, default_value = "info")]
    log_level: String,
    #[arg(long, hide = true)]
    worker: bool,
    #[arg(long, hide = true)]
    ipc_socket: Option<String>,

    // ── 新增子命令（替代 Manager 侧 fork，互斥）──
    /// 启动隔离 writer 子进程（参数: temp_path）
    #[arg(long, hide = true)]
    isolated_writer: Option<String>,
    /// 启动隔离 reader 子进程（参数: path）
    #[arg(long, hide = true)]
    isolated_reader: Option<String>,
    /// 启动隔离 writer_part 子进程（参数: part_path）
    #[arg(long, hide = true)]
    isolated_writer_part: Option<String>,
    /// 启动隔离 merger 子进程（part_paths 通过 --part-paths 传递）
    #[arg(long, hide = true)]
    isolated_merger: bool,
    /// 启动 metadata 查询子进程（参数: path）
    #[arg(long, hide = true)]
    metadata: Option<String>,

    // ── 子命令通用参数 ──
    #[arg(long, hide = true)]
    uid: Option<u32>,
    #[arg(long, hide = true)]
    gid: Option<u32>,
    /// writer 的 final_path
    #[arg(long, hide = true)]
    final_path: Option<String>,
    /// merger 的 part_paths（逗号分隔）
    #[arg(long, hide = true)]
    part_paths: Option<String>,
}

/// 子命令分发入口：返回 true 表示已处理子命令，main 应直接退出
async fn dispatch_isolated_command(args: &Args) -> Result<bool> {
    // 互斥检查：最多一个子命令被激活
    let active: Vec<&str> = [
        args.isolated_writer.as_ref().map(|_| "writer"),
        args.isolated_reader.as_ref().map(|_| "reader"),
        args.isolated_writer_part.as_ref().map(|_| "writer_part"),
        args.isolated_merger.then(|| "merger"),
        args.metadata.as_ref().map(|_| "metadata"),
    ].into_iter().flatten().collect();

    match active.len() {
        0 => Ok(false),  // 无子命令激活，正常启动
        1 => {
            let uid = args.uid.unwrap_or(0);
            let gid = args.gid.unwrap_or(0);
            // 降权（setgid 先于 setuid，顺序重要）
            unsafe {
                if libc::setgid(gid) != 0 {
                    std::process::exit(1);
                }
                if libc::setuid(uid) != 0 {
                    std::process::exit(1);
                }
            }
            // 验证降权成功
            if unsafe { libc::getuid() } != uid || unsafe { libc::getgid() } != gid {
                std::process::exit(1);
            }

            // 按激活的子命令分发
            if let Some(temp_path) = &args.isolated_writer {
                run_isolated_writer(temp_path, args.final_path.as_ref().unwrap()).await?;
            } else if let Some(path) = &args.isolated_reader {
                run_isolated_reader(path).await?;
            } else if let Some(part_path) = &args.isolated_writer_part {
                run_isolated_writer_part(part_path).await?;
            } else if args.isolated_merger {
                let parts: Vec<String> = args.part_paths.as_ref().unwrap()
                    .split(',').map(String::from).collect();
                run_isolated_merger(&parts, args.final_path.as_ref().unwrap()).await?;
            } else if let Some(path) = &args.metadata {
                run_metadata(path).await?;
            }
            Ok(true)
        }
        _ => Err(anyhow!("只能指定一个隔离子命令")),
    }
}

async fn run_isolated_writer(temp_path: &str, final_path: &str) -> Result<()> {
    use std::io::{Read, Write};
    // 打开临时文件（O_NOFOLLOW 防符号链接劫持，0o600 限属主读写）
    let file = std::fs::OpenOptions::new()
        .create(true).write(true).truncate(true)
        .mode(0o600).custom_flags(libc::O_NOFOLLOW)
        .open(temp_path)?;
    let mut buf_writer = std::io::BufWriter::with_capacity(256 * 1024, file);
    let mut stdin = std::io::stdin();
    let mut buf = [0u8; 256 * 1024];
    loop {
        let n = stdin.read(&mut buf)?;
        if n == 0 { break; }
        buf_writer.write_all(&buf[..n])?;
    }
    buf_writer.flush()?;
    buf_writer.get_ref().sync_all()?;
    drop(buf_writer);
    std::fs::rename(temp_path, final_path)?;
    std::process::exit(0);
}

// run_isolated_reader / run_isolated_writer_part / run_isolated_merger / run_metadata 类似实现
// 全部使用 std::io（同步 I/O），不用 tokio（子进程不需要 async runtime）
```

**关键设计**：
- 子进程**不启动 tokio runtime**（避免引入不必要的 runtime 状态）
- 全部使用 `std::io` 同步 I/O（与原 fork 子进程行为一致）
- 降权逻辑集中在 `dispatch_isolated_command` 中（子命令实现不重复降权代码）
- 退出码与原 `_exit(N)` 保持一致（见 executor.rs:796-810 注释）

### 5.3 executor.rs 改造

**关键约束**：file_stream.rs 的 `PipeFileStreamWriter.write_chunk` 和 `PipeFileStreamReader.read_chunk` 使用**同步 I/O**（`std::io::Write/Read`）。因此新方案必须返回实现同步 I/O 的类型。

使用 `std::process::Command`（而非 `tokio::process::Command`），因为：
1. `std::process::ChildStdin` 实现 `std::io::Write`（与 `os_pipe::PipeWriter` 接口兼容）
2. `std::process::ChildStdout` 实现 `std::io::Read`（与 `os_pipe::PipeReader` 接口兼容）
3. file_stream.rs 的同步 API 无需改动

```rust
use std::process::{Command, ChildStdin, ChildStdout};

impl UserExecutor {
    /// 隔离写入：替代原 fork + os_pipe::PipeWriter
    /// 返回 (ChildStdin, child_pid) — ChildStdin 实现 std::io::Write
    pub fn spawn_isolated_writer(
        &self,
        temp_path: &str,
        final_path: &str,
        _file_size: u64,
    ) -> Result<(ChildStdin, i32)> {
        let current_exe = std::env::current_exe()?;
        let mut cmd = Command::new(current_exe);
        cmd.arg("--isolated-writer").arg(temp_path)
           .arg("--final-path").arg(final_path)
           .arg("--uid").arg(self.uid.to_string())
           .arg("--gid").arg(self.gid.to_string())
           .stdin(std::process::Stdio::piped())
           .stdout(std::process::Stdio::null())
           .stderr(std::process::Stdio::null());
        let child = cmd.spawn()?;
        let pid = child.id() as i32;
        let stdin = child.stdin.take().unwrap();
        Ok((stdin, pid))
    }

    /// 隔离读取：替代原 fork + os_pipe::PipeReader
    /// 返回 (ChildStdout, child_pid) — ChildStdout 实现 std::io::Read
    pub fn spawn_isolated_reader(&self, path: &str) -> Result<(ChildStdout, i32)> {
        let current_exe = std::env::current_exe()?;
        let mut cmd = Command::new(current_exe);
        cmd.arg("--isolated-reader").arg(path)
           .arg("--uid").arg(self.uid.to_string())
           .arg("--gid").arg(self.gid.to_string())
           .stdin(std::process::Stdio::null())
           .stdout(std::process::Stdio::piped())  // 子进程 stdout → 父进程读取
           .stderr(std::process::Stdio::null());
        let child = cmd.spawn()?;
        let pid = child.id() as i32;
        let stdout = child.stdout.take().unwrap();
        Ok((stdout, pid))
    }

    // spawn_isolated_writer_part / merger 类似改造（返回类型与 writer 一致）

    /// 获取文件元数据：替代原 handler.rs:519 的 execute_as_user + fs::metadata
    /// 在 Manager 异步上下文中用 spawn_blocking 包装同步 Command
    pub async fn get_metadata_async(&self, path: &str) -> Result<(u64, u64)> {
        let path = path.to_string();
        let uid = self.uid;
        let gid = self.gid;
        tokio::task::spawn_blocking(move || {
            let current_exe = std::env::current_exe()?;
            let output = Command::new(current_exe)
                .arg("--metadata").arg(&path)
                .arg("--uid").arg(uid.to_string())
                .arg("--gid").arg(gid.to_string())
                .output()?;
            if !output.status.success() {
                return Err(anyhow!("metadata 子进程失败: exit={}", output.status));
            }
            #[derive(serde::Deserialize)]
            struct MetadataResult { size: u64, mtime: u64 }
            let meta: MetadataResult = serde_json::from_slice(&output.stdout)?;
            Ok((meta.size, meta.mtime))
        }).await?
    }
}
```

**关键点**：
- `spawn_isolated_writer` 返回 `std::process::ChildStdin`（实现 `std::io::Write`），与 `os_pipe::PipeWriter` 接口兼容
- `spawn_isolated_reader` 返回 `std::process::ChildStdout`（实现 `std::io::Read`），与 `os_pipe::PipeReader` 接口兼容
- file_stream.rs 的 `write_chunk` / `read_chunk` / `finish` / `abort` **代码逻辑完全不变**，仅类型标注从 `os_pipe::PipeWriter` → `std::process::ChildStdin`
- `spawn_isolated_*` 仍为同步函数（与原签名一致），不改变调用方的 `.await` 模式
- `get_metadata_async` 是新方法，用 `spawn_blocking` 包装同步 `Command::output()`（避免阻塞 tokio worker thread）
- Rust 1.65+ 的 `std::process::Command` 自动用 `posix_spawn`（当不配置 pre_exec 时）

**ForkGuard 的使用**：见 5.1 节，`spawn_isolated_*` 内部调用 `with_fork_guard()` 串行化。

### 5.4 handler.rs 改造

```rust
// handler.rs:519 改造前
let (file_size, mtime) = executor.execute_as_user(move || {
    let metadata = fs::metadata(&path_str)?;
    // ...
}).map_err(|e| e.to_string())?;

// 改造后
let (file_size, mtime) = executor.get_metadata_async(&path_str).await
    .map_err(|e| e.to_string())?;

// executor.rs 新增
impl UserExecutor {
    pub async fn get_metadata_async(&self, path: &str) -> Result<(u64, u64)> {
        let _guard = with_fork_guard(...).await;
        let current_exe = std::env::current_exe()?;
        let output = Command::new(current_exe)
            .arg("--metadata").arg(path)
            .arg("--uid").arg(self.uid.to_string())
            .arg("--gid").arg(self.gid.to_string())
            .output().await?;
        // 解析 stdout JSON: {"size": 123, "mtime": 456}
        let meta: MetadataResult = serde_json::from_slice(&output.stdout)?;
        Ok((meta.size, meta.mtime))
    }
}
```

### 5.5 改造覆盖表

| fork 点 | 旧实现 | 新实现 | 接口变化 |
|---|---|---|---|
| spawn_isolated_writer | fork + pipe | posix_spawn + stdin pipe | 返回 `(ChildStdin, i32)` 替代 `(PipeWriter, i32)` |
| spawn_isolated_reader | fork + pipe | posix_spawn + stdout pipe | 返回 `(ChildStdout, i32)` 替代 `(PipeReader, i32)` |
| spawn_isolated_writer_part | fork + pipe | posix_spawn + stdin pipe | 同 writer |
| spawn_isolated_merger | fork | posix_spawn | 不变 |
| handler.rs:519 execute_as_user | fork + 闭包 | posix_spawn + --metadata | `get_metadata_async` 替代 |

### 5.6 调用方适配

- [handler.rs:418](file:///e:\MyWork\gnome-remote\agent\src\handler.rs): `spawn_isolated_writer_part` 调用点，`pipe_writer` 类型从 `os_pipe::PipeWriter` → `std::process::ChildStdin`
- [handler.rs:469](file:///e:\MyWork\gnome-remote\agent\src\handler.rs): `spawn_isolated_writer` 调用点，同上
- [handler.rs:550](file:///e:\MyWork\gnome-remote\agent\src\handler.rs): `spawn_isolated_reader` 调用点，`pipe_reader` 类型从 `os_pipe::PipeReader` → `std::process::ChildStdout`
- [handler.rs:699](file:///e:\MyWork\gnome-remote\agent\src\handler.rs): `spawn_isolated_writer_part` 调用点，同 writer
- [quic.rs:2086](file:///e:\MyWork\gnome-remote\agent\src\server\quic.rs): `spawn_isolated_merger` 调用点，接口不变

### 5.7 file_stream.rs 改造

file_stream.rs 中的 `PipeFileStreamWriter` 和 `PipeFileStreamReader` 直接使用 `os_pipe::PipeWriter/PipeReader` 作为字段类型。改造仅涉及类型替换，逻辑完全不变：

```rust
// 改造前
pub struct PipeFileStreamWriter {
    pipe_writer: Option<os_pipe::PipeWriter>,
    // ...
}
impl PipeFileStreamWriter {
    pub fn new(
        pipe_writer: os_pipe::PipeWriter,
        // ...
    ) -> Self { ... }

    pub fn write_chunk(&mut self, data: &[u8]) -> Result<(), String> {
        use std::io::Write;
        let writer = self.pipe_writer.as_mut().ok_or("管道已关闭")?;
        writer.write_all(data).map_err(...)?;  // std::io::Write::write_all
        // ...
    }
}

// 改造后（仅类型替换）
pub struct PipeFileStreamWriter {
    pipe_writer: Option<std::process::ChildStdin>,  // os_pipe::PipeWriter → ChildStdin
    // ...
}
impl PipeFileStreamWriter {
    pub fn new(
        pipe_writer: std::process::ChildStdin,  // 类型变更
        // ...
    ) -> Self { ... }

    pub fn write_chunk(&mut self, data: &[u8]) -> Result<(), String> {
        use std::io::Write;
        let writer = self.pipe_writer.as_mut().ok_or("管道已关闭")?;
        writer.write_all(data).map_err(...)?;  // 逻辑不变（ChildStdin 实现 std::io::Write）
        // ...
    }
}
```

**关键**：
- `os_pipe::PipeWriter` 和 `std::process::ChildStdin` 都实现 `std::io::Write`
- `os_pipe::PipeReader` 和 `std::process::ChildStdout` 都实现 `std::io::Read`
- file_stream.rs 的所有方法（`write_chunk` / `read_chunk` / `finish` / `abort`）逻辑完全不变
- 仅字段类型和构造函数参数类型变更

### 5.8 wait_isolated_child 保持不变

`wait_isolated_child` 仍用 `libc::waitpid(child_pid, ...)`，**无需改造**：
- `spawn_isolated_*` 返回的 `child_pid` 仍可用 `waitpid` 等待（POSIX 语义，任何子进程都可用 pid wait）
- `std::process::Child` 的 `id()` 返回的 pid 与 `fork` 返回的 pid 语义一致
- 退出码翻译逻辑完全不变

### 5.9 os_pipe 依赖移除

改造完成后，`os_pipe` 不再使用，从 Cargo.toml 移除依赖：

```toml
# Cargo.toml 移除
- os_pipe = "1"
```

## 6. Worker 侧 ForkGuard

### 6.1 不改 run() 循环

Worker 保持现有串行循环，不引入并发 dispatch。fork 时 runtime 静止，fork 安全。

### 6.2 ForkGuard 保护

```rust
// executor.rs（Worker 和 Manager 共享同一份 executor.rs）
// Worker 进程内 FORK_GUARD 与 Manager 进程内 FORK_GUARD 是独立的（各自进程的 static）
// Worker 串行模式下 FORK_GUARD 永远不会争用（单线程访问）
```

## 7. 错误处理

### 7.1 故障场景

| 场景 | 行为 | 影响 |
|---|---|---|
| dispatcher task panic | pending 中所有等待者收到 `response dropped` | 客户端收到错误，可重试 |
| Worker 连接断开 | dispatcher 的 read_half 返回 Err，清理 pending | 所有在途请求失败，客户端收到错误 |
| 热更新期间在途请求 | 旧 dispatcher abort，pending 清理 | 在途请求失败，客户端重试后连新 Worker |
| posix_spawn 失败 | 返回 Err，调用方按原 fork 失败逻辑处理 | 与原行为一致 |
| 子命令子进程崩溃 | 退出码非 0，wait_isolated_child 翻译为描述性错误 | 与原行为一致 |

### 7.2 退出码兼容

子命令子进程的退出码与原 fork 子进程保持一致（见 executor.rs:796-810 注释），`wait_isolated_child` 逻辑不变。

## 8. 配置项

新增 `ipc_channel_capacity`（默认 128）：

```toml
[worker]
ipc_channel_capacity = 128
```

- Manager 的 `request_tx` channel 容量
- 满时 `send().await` 等待，提供背压
- 无需用户调整，默认值足够

## 9. 向后兼容

- IPC 协议：不变
- WorkerResponse / ClientRequest：不变
- Worker run() 循环：不变
- install.sh：不变（仍只拷贝一个 `agent` 二进制）
- systemd service：不变
- 客户端：完全无感知

## 10. 测试策略

### 10.1 回归测试

**核心回归测试**：10 个并发请求同时到达 IpcServer，全部成功响应（验证竞态消除）

```rust
#[tokio::test]
async fn test_concurrent_requests_no_race() {
    let server = IpcServer::new(...);
    let handles: Vec<_> = (0..10).map(|i| {
        let server = server.clone();
        tokio::spawn(async move {
            server.send_request(ClientRequest::Ping(i)).await
        })
    }).collect();
    for handle in handles {
        assert!(handle.await.unwrap().is_ok());
    }
}
```

### 10.2 单元测试

- `run_dispatcher`：mock write/read half，验证 request_id 路由正确
- `spawn_isolated_writer` 子命令：写入测试数据，验证文件内容正确
- `get_metadata_async`：验证返回正确的 size/mtime

### 10.3 集成测试

- 端到端文件上传（单流 + 多流）
- 端到端文件下载
- 热更新期间文件传输
- 子命令子进程崩溃后错误码翻译

## 11. 不在范围内（明确排除）

- Worker 侧并发 dispatch（未来工作，需先完成所有 fork 点 posix_spawn 替代）
- Worker 的 `execute_as_user`（10 处）和 `create_session` 改造（串行模式下已安全）
- 客户端代码改动
- IPC 协议变更
- 热更新流程改造（保持现有 HotUpdateCoordinator 逻辑）

## 12. 实现顺序（高层）

1. 基础设施：dashmap 依赖、ipc_channel_capacity 配置
2. Manager IpcServer 重构：mpsc + request_id + dispatcher
3. ForkGuard 引入（executor.rs）
4. 子命令实现（main.rs）：5 个子命令分发 + 子命令逻辑
5. executor.rs 改造：spawn_isolated_* 改 posix_spawn、get_metadata_async 新增
6. file_stream.rs 适配：字段类型从 os_pipe::Pipe* → std::process::ChildStd*
7. handler.rs 适配：handler.rs:519 改 get_metadata_async
8. 测试：回归 + 单元 + 集成
9. 编译验证 + 零警告检查
10. os_pipe 依赖移除（Cargo.toml）
