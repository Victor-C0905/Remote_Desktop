# Agent 架构设计文档

## 📋 文档信息

- **版本**: v2.0
- **架构名称**: 基于 FD 转移的分层微内核架构
- **设计日期**: 2026-07-31
- **设计者**: AI Assistant + User

---

## 🧠 核心设计哲学

### 1. 数据平面与控制平面分离

**原则**: 高吞吐、低延迟的终端数据流（PTY I/O）绝不经过 IPC。IPC 只用于传输控制指令。

**理由**:
- 终端数据是最敏感的性能瓶颈
- IPC 序列化/反序列化会引入延迟
- 数据平面应该是最短路径

### 2. FD 跨进程转移

**原则**: 打破"谁创建谁管理"的传统思维，让 Worker 创建 PTY，但将"控制权"移交给 Manager。

**理由**:
- Worker 适合处理业务逻辑（创建 PTY、处理认证）
- Manager 适合处理 I/O（持有 QUIC Stream、读写 PTY）
- FD 转移让职责分离成为可能

### 3. Worker 孤儿化

**原则**: 热更新时，Worker 进程被替换，但其派生的 Shell 进程作为"孤儿"被系统领养，实现终端会话的绝对存活。

**理由**:
- Shell 进程独立于 Worker
- PTY 数据缓存在内核 buffer 中
- Manager 仍然持有 master_fd，可以继续读写

---

## 📐 架构蓝图

整个系统分为三层：**Manager (网关层) -> Worker (逻辑层) -> Session (执行层)**。

### 层级关系图

```
┌─────────────────────────────────────────────────────────────┐
│                      Client (Tauri)                          │
│                   QUIC + 认证 + 业务逻辑                      │
└───────────────────────────┬─────────────────────────────────┘
                            │ QUIC Stream
                            ↓
┌─────────────────────────────────────────────────────────────┐
│                    Manager (网关层)                           │
│  职责: QUIC 监听、认证、持有 PTY master_fd、直接读写 PTY      │
│  热更新级别: 极低 (5-10年一次)                                │
└───────────┬─────────────────────────────────┬───────────────┘
            │ Unix Socket (IPC)                │ master_fd
            ↓                                   │
┌──────────────────────────────────────────────┼──────────────┐
│                Worker (逻辑层)                 │              │
│  职责: 创建 PTY、处理业务逻辑                  │              │
│  热更新级别: 高 (频繁更新)                     │              │
└────────────────┬─────────────────────────────┼──────────────┘
                 │ forkpty                      │
                 ↓                              │
┌─────────────────────────────────────────────────────────────┐
│               Session (执行层 - Shell 进程)                   │
│  职责: 执行用户命令 (bash/zsh/python)                         │
│  生命周期: 独立于 Worker，成为孤儿进程                         │
└─────────────────────────────────────────────────────────────┘
```

---

## 🏗️ 模块设计

### 1. Manager (网关层)

**文件结构**:
```
src/manager/
├── mod.rs              # Manager 主模块
├── connection.rs       # QUIC 连接管理
├── session.rs          # 用户会话管理
├── auth.rs             # 认证处理 (PAM + SSH)
├── pty_registry.rs     # PTY 注册表 (管理所有 master_fd)
├── worker_manager.rs   # Worker 进程管理 (Supervisor)
└── ipc_server.rs       # IPC Server (接收 Worker 的 FD)
```

**核心职责**:
- ✅ 监听 QUIC 端口，处理握手、加密、认证
- ✅ 维护客户端连接的生命周期
- ✅ 持有所有活动 PTY 的 Master FD
- ✅ 直接将 QUIC Stream 的数据读写到 PTY
- ✅ 作为 Supervisor，监控并拉起 Worker 进程

**关键数据结构**:
```rust
/// PTY 注册表
pub struct PtyRegistry {
    /// session_id -> (master_fd, quic_stream)
    sessions: Arc<RwLock<HashMap<String, PtySession>>>,
}

/// PTY 会话
pub struct PtySession {
    pub session_id: String,
    pub master_fd: RawFd,
    pub user_info: UserInfo,
    pub created_at: SystemTime,
}

/// Worker 管理器
pub struct WorkerManager {
    worker_process: Option<Child>,
    ipc_socket: UnixStream,
    restart_count: u32,
}
```

---

### 2. Worker (逻辑层)

**文件结构**:
```
src/worker/
├── mod.rs              # Worker 主模块
├── pty_factory.rs      # PTY 工厂 (创建 PTY、转移 FD)
├── handlers/           # 业务逻辑处理器
│   ├── mod.rs
│   ├── file.rs         # 文件操作
│   ├── command.rs      # 命令执行
│   └── system.rs       # 系统信息
└── ipc_client.rs       # IPC Client (与 Manager 通信)
```

**核心职责**:
- ✅ 创建 PTY (forkpty)
- ✅ 将 master_fd 通过 Unix Socket 发送给 Manager
- ✅ 处理无状态请求：SFTP 文件传输、端口转发、系统信息查询
- ✅ 处理复杂业务逻辑：Tab 补全、命令拦截、审计日志

**关键数据结构**:
```rust
/// PTY 工厂
pub struct PtyFactory {
    ipc_client: Arc<IpcClient>,
}

/// IPC Client
pub struct IpcClient {
    socket: UnixStream,
}
```

---

### 3. Session (执行层)

**描述**: 由 Worker fork 出来的子进程（如 bash, zsh, python）。

**生命周期**: 独立于 Worker。即使 Worker 被杀掉，Session 依然作为孤儿进程继续运行，数据缓存在内核的 PTY Buffer 中等待 Manager 读取。

---

## 🔄 核心机制运行流

### 场景 A: 建立终端会话

**流程图**:
```
Client              Manager              Worker              Shell
  │                   │                    │                  │
  ├─ Open Shell ────→│                    │                  │
  │                   ├─ CreateSession ──→│                  │
  │                   │                    ├─ forkpty() ────→│
  │                   │                    │                  │
  │                   │                    ├─ send_fd() ─────┤
  │                   ├←───── master_fd ───┤                  │
  │                   │                    │                  │
  │                   ├─ register_fd()     │                  │
  │                   │                    │                  │
  │←──── Session ID ──┤                    │                  │
  │                   │                    │                  │
  │                   │←─────── data ──────┼──────────────────┤
  │                   │                    │                  │
  │←───── output ─────┤                    │                  │
```

**详细步骤**:
1. **Client -> Manager**: 客户端通过 QUIC 发起 Open Shell 请求。
2. **Manager -> Worker**: Manager 通过 Unix Socket 发送 `CreateSession(cmd="/bin/bash", term="xterm")`。
3. **Worker 作业**:
   - Worker 调用 `forkpty()`，得到 `master_fd` 和子进程 `bash`。
   - Worker 将 `master_fd` 通过 `sendmsg (SCM_RIGHTS)` 发送给 Manager。
   - Worker 关闭自己的 `master_fd`，并记录 `bash` 的 PID。
4. **Manager 接管**: Manager 收到 `master_fd`，将其注册到自己的 Event Loop 中（如 epoll/kqueue）。
5. **数据流成型**: `Client <-> QUIC <-> Manager <-> master_fd <-> bash`。Worker 完全不参与数据流。

---

### 场景 B: 无缝热更新

**流程图**:
```
Manager              Worker_v1           Worker_v2          Shell
  │                    │                    │                 │
  ├─ Reload cmd ─────→│                    │                 │
  │                    │                    │                 │
  ├───────────── start worker_v2 ─────────→│                 │
  │                    │                    │                 │
  ├─ GracefulShutdown →│                    │                 │
  │                    ├─ exit              │                 │
  │                    │                    │                 │
  │                    │                    │                 │
  │←───── data ────────┼────────────────────┼─────────────────┤
  │                    │                    │                 │
  │                    │   (Shell 成为孤儿)  │                 │
  │                    │                    │                 │
  │←───── data ────────┼────────────────────┼─────────────────┤
```

**详细步骤**:
1. **触发更新**: 管理员上传新版本 Worker 二进制，发送 Reload 命令给 Manager。
2. **启动新 Worker**: Manager 启动 `worker_v2` 进程，建立新的 IPC Socket。
3. **状态迁移**:
   - Manager 向旧 Worker 发送 `GracefulShutdown`。
   - 旧 Worker 将尚未完成的任务状态序列化发给新 Worker（如果需要）。
   - 旧 Worker 退出。
4. **终端会话保持**:
   - 旧 Worker 退出时，它 fork 出来的 `bash` 进程并不会死，而是变成孤儿进程，被 `init/systemd` (PID 1) 领养。
   - `bash` 继续向 `master_fd` 写入数据。因为 Manager 还持有 `master_fd` 的引用，数据不会丢失，继续流向客户端。
5. **新请求路由**: Manager 将所有新的请求，路由给 `worker_v2`。

**结果**: 旧终端连接不断、不丢字，新功能在 `worker_v2` 中生效。

---

### 场景 C: 故障隔离

**流程图**:
```
Manager              Worker              Shell
  │                    │                   │
  │                    ├─ panic!           │
  │                    │                   │
  ├─ detect crash ─────┤                   │
  │                    │                   │
  │                    │                   │
  │←───── data ────────┼───────────────────┤
  │  (终端不受影响)      │                   │
  │                    │                   │
  ├─ restart worker    │                   │
```

**详细步骤**:
1. **Worker 崩溃**: 由于 SFTP 解析一个恶意构造的文件名导致 Worker Panic。
2. **Manager 感知**: Manager 监听到 Worker 的 IPC Socket 断开。
3. **影响范围**:
   - 正在进行的 SFTP 传输失败并回报错误给客户端。
   - 所有正在运行的终端会话完全不受影响，因为 Manager 还在直接读写 `master_fd`。
4. **自动恢复**: Manager 自动重启 Worker，接受后续新请求。

---

## 🛠️ 关键技术实现

### 1. 跨进程传递 FD (FD Passing)

**原理**: 使用 Unix Domain Socket 的辅助数据机制 (SCM_RIGHTS)。

**实现代码**:
```rust
use nix::sys::socket::{sendmsg, recvmsg, ControlMessage, ControlMessageOwned, MsgFlags};
use std::os::unix::io::AsRawFd;

/// 发送 FD 给 Manager
pub fn send_fd(socket: &UnixStream, fd: RawFd) -> Result<()> {
    let buf = [0u8; 1];
    let iov = [IoSlice::new(&buf)];

    let fds = [fd];
    let cmsg = ControlMessage::ScmRights(&fds);

    sendmsg(
        socket.as_raw_fd(),
        &iov,
        &[cmsg],
        MsgFlags::empty(),
        None,
    )?;

    Ok(())
}

/// 从 Worker 接收 FD
pub fn receive_fd(socket: &UnixStream) -> Result<RawFd> {
    let mut buf = [0u8; 1];
    let mut iov = [IoSlice::new(&mut buf)];

    let msg = recvmsg(
        socket.as_raw_fd(),
        &mut iov,
        Some(&mut [0u8; 64]),
        MsgFlags::empty(),
    )?;

    for cmsg in msg.cmsgs()? {
        if let ControlMessageOwned::ScmRights(fds) = cmsg {
            return Ok(fds[0]);
        }
    }

    Err(anyhow!("No FD received"))
}
```

---

### 2. PTY 信号控制 (Window Resize / Kill)

**问题**: 终端窗口大小改变了，需要向 PTY 发送 `TIOCSWINSZ` ioctl。但 Worker 没有 `master_fd` 了，怎么发？

**解决**: 通过 IPC 指令。Worker 收到客户端的窗口改变请求，向 Manager 发送 `ResizeWindow(session_id, rows, cols)` 指令。Manager 持有 `master_fd`，由 Manager 执行 ioctl。

**实现代码**:
```rust
use nix::ioctl_write_ptr_bad;
use libc::{TIOCSWINSZ, winsize};

ioctl_write_ptr_bad!(ioctl_resize, TIOCSWINSZ, winsize);

/// 调整 PTY 大小 (Manager 端)
pub fn resize_pty(master_fd: RawFd, cols: u16, rows: u16) -> Result<()> {
    let win = winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    ioctl_resize(master_fd, &win)?;

    Ok(())
}
```

---

### 3. 僵尸进程回收

**问题**: 旧 Worker 退出了，它 fork 出来的 `bash` 进程退出后，谁去回收僵尸进程？

**解决**: Manager 在读取 `master_fd` 遇到 EOF 时，说明 `bash` 已退出。Manager 此时调用 `waitpid` 或依靠系统 init 机制回收该 PID。

**实现代码**:
```rust
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};

/// 回收僵尸进程 (Manager 端)
pub fn reap_zombie(pid: Pid) -> Result<()> {
    match waitpid(pid, Some(WaitPidFlag::WNOHANG))? {
        WaitStatus::Exited(pid, status) => {
            info!("Process {} exited with status {}", pid, status);
        }
        WaitStatus::Signaled(pid, sig, _) => {
            info!("Process {} killed by signal {:?}", pid, sig);
        }
        _ => {}
    }

    Ok(())
}
```

---

## 📡 IPC 协议定义

### 消息格式 (使用 Protobuf)

**文件**: `protocol/agent.proto`

```protobuf
syntax = "proto3";

package agent;

// ============================================
// Manager -> Worker 的请求消息
// ============================================

message ManagerRequest {
    uint64 request_id = 1;
    oneof payload {
        CreateSession create_session = 2;
        ResizeWindow resize_window = 3;
        KillSession kill_session = 4;
        ReadDir read_dir = 5;
        ReadFile read_file = 6;
        WriteFile write_file = 7;
        ExecuteCommand execute_command = 8;
        GetSystemInfo get_system_info = 9;
    }
}

// 创建终端会话
message CreateSession {
    string shell = 1;
    uint16 cols = 2;
    uint16 rows = 3;
    string working_directory = 4;
    map<string, string> env = 5;
}

// 调整终端窗口大小
message ResizeWindow {
    string session_id = 1;
    uint16 cols = 2;
    uint16 rows = 3;
}

// 终止终端会话
message KillSession {
    string session_id = 1;
}

// 读取目录
message ReadDir {
    string path = 1;
}

// 读取文件
message ReadFile {
    string path = 1;
    uint64 offset = 2;
    uint32 length = 3;
}

// 写入文件
message WriteFile {
    string path = 1;
    uint64 offset = 2;
    bytes data = 3;
}

// 执行命令
message ExecuteCommand {
    string command = 1;
    repeated string args = 2;
    string working_directory = 3;
    map<string, string> env = 4;
    uint32 timeout_secs = 5;
}

// 获取系统信息
message GetSystemInfo {
    repeated string info_types = 1; // "cpu", "memory", "disk", "network"
}

// ============================================
// Worker -> Manager 的响应消息
// ============================================

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
    }
}

// 终端会话已创建 (包含 master_fd)
message SessionCreated {
    string session_id = 1;
    int32 pid = 2;
    // 注意: master_fd 通过 SCM_RIGHTS 发送，不在 Protobuf 中
}

// 目录列表
message DirListing {
    repeated DirEntry entries = 1;
}

message DirEntry {
    string name = 1;
    bool is_dir = 2;
    uint64 size = 3;
    uint64 modified_time = 4;
    uint32 mode = 5;
}

// 文件内容
message FileContent {
    bytes data = 1;
    bool eof = 2;
}

// 写入结果
message WriteResult {
    uint32 bytes_written = 1;
}

// 命令输出
message CommandOutput {
    bytes stdout = 1;
    bytes stderr = 2;
    int32 exit_code = 3;
}

// 系统信息
message SystemInfo {
    double cpu_percent = 1;
    uint64 mem_total = 2;
    uint64 mem_used = 3;
    uint64 disk_total = 4;
    uint64 disk_used = 5;
}

// 错误响应
message Error {
    uint32 code = 1;
    string message = 2;
}
```

---

## 📝 代码规范

### 1. 命名规范

**Rust 命名规范**:
- **类型**: PascalCase (如 `PtyRegistry`, `WorkerManager`)
- **函数**: snake_case (如 `create_pty`, `send_fd`)
- **常量**: SCREAMING_SNAKE_CASE (如 `MAX_RETRY_COUNT`)
- **模块**: snake_case (如 `pty_factory`, `ipc_client`)

**文件命名**:
- 模块文件: snake_case.rs (如 `pty_factory.rs`)
- 测试文件: snake_case_test.rs (如 `pty_factory_test.rs`)

---

### 2. 错误处理

**使用 `anyhow` 进行错误处理**:
```rust
use anyhow::{Result, Context, anyhow};

pub fn create_pty(shell: &str, cols: u16, rows: u16) -> Result<(RawFd, Pid)> {
    // 使用 context 添加上下文信息
    let master_fd = forkpty(cols, rows)
        .context("Failed to create PTY")?;

    // 使用 anyhow! 创建自定义错误
    if master_fd < 0 {
        return Err(anyhow!("Invalid master_fd: {}", master_fd));
    }

    Ok((master_fd, pid))
}
```

---

### 3. 日志规范

**使用 `tracing` 进行日志记录**:
```rust
use tracing::{info, warn, error, debug, trace};

// 日志级别使用规范:
// - error: 严重错误，影响系统运行
// - warn: 警告，不影响系统运行
// - info: 重要信息（启动、停止、关键操作）
// - debug: 调试信息（详细流程）
// - trace: 跟踪信息（函数调用、变量值）

pub fn handle_terminal_input(session_id: &str, data: &[u8]) -> Result<()> {
    debug!("[Manager] Received terminal input: session_id={}, len={}", session_id, data.len());

    match write_to_pty(session_id, data) {
        Ok(_) => {
            trace!("[Manager] Terminal input written successfully");
            Ok(())
        }
        Err(e) => {
            error!("[Manager] Failed to write terminal input: {}", e);
            Err(e)
        }
    }
}
```

---

### 4. 注释规范

**模块注释**:
```rust
//! PTY 工厂模块
//!
//! 该模块负责创建 PTY 会话，并将 master_fd 转移给 Manager。
//!
//! # 示例
//!
//! ```rust
//! use worker::pty_factory::PtyFactory;
//!
//! let factory = PtyFactory::new(ipc_client);
//! let (master_fd, pid) = factory.create("/bin/bash", 80, 24)?;
//! ```
```

**函数注释**:
```rust
/// 创建 PTY 会话
///
/// # 参数
///
/// - `shell`: Shell 程序路径 (如 "/bin/bash")
/// - `cols`: 终端列数
/// - `rows`: 终端行数
/// - `cwd`: 工作目录 (可选)
///
/// # 返回
///
/// 返回 (master_fd, pid)，其中:
/// - `master_fd`: PTY master 文件描述符
/// - `pid`: Shell 进程的 PID
///
/// # 错误
///
/// 如果 forkpty 失败，返回错误。
///
/// # 示例
///
/// ```rust
/// let (master_fd, pid) = create_pty("/bin/bash", 80, 24)?;
/// ```
pub fn create_pty(shell: &str, cols: u16, rows: u16, cwd: Option<&str>) -> Result<(RawFd, Pid)> {
    // ...
}
```

---

### 5. 测试规范

**单元测试**:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_pty() {
        let (master_fd, pid) = create_pty("/bin/bash", 80, 24, None)
            .expect("Failed to create PTY");

        assert!(master_fd >= 0);
        assert!(pid > 0);

        // 清理
        nix::unistd::close(master_fd).unwrap();
        nix::sys::signal::kill(pid, Some(nix::sys::signal::SIGTERM)).unwrap();
    }
}
```

---

## 🎯 开发原则

### 1. SOLID 原则

- **S (单一职责)**: 每个模块只负责一件事（如 `PtyFactory` 只负责创建 PTY）
- **O (开放封闭)**: 对扩展开放，对修改封闭（使用 trait 定义接口）
- **L (里氏替换)**: 子类可以替换父类（Worker 可以被任何实现 `Worker` trait 的类型替换）
- **I (接口隔离)**: 接口应该小而专一（不要设计"上帝接口"）
- **D (依赖倒置)**: 依赖抽象，不依赖具体实现（Manager 依赖 `IpcClient` trait，而不是具体的 `UnixIpcClient`）

---

### 2. 性能原则

- **零拷贝**: 尽量使用引用和切片，避免数据克隆
- **异步优先**: I/O 操作使用异步（如 tokio::io）
- **批量处理**: 批量读取/写入数据，减少系统调用次数

---

### 3. 安全原则

- **最小权限**: Worker 应该以最低权限运行（如 `nobody` 用户）
- **输入验证**: 验证所有来自客户端的输入
- **错误处理**: 永远不要忽略错误

---

## 📅 实施计划

### Phase 1: 定义 IPC 协议 (1-2天)

**任务**:
- 编写 `protocol/agent.proto`
- 生成 Rust 代码 (`prost` 编译器)
- 编写 IPC 协议文档

**验证**:
- Protobuf 文件编译通过
- 生成的 Rust 代码可用

---

### Phase 2: 实现 Manager (3-5天)

**任务**:
- 实现 `PtyRegistry`
- 实现 `WorkerManager`
- 实现 `IpcServer` (接收 FD)
- 实现 PTY 输入/输出处理

**验证**:
- Manager 可以启动 Worker
- Manager 可以接收 master_fd
- 终端输入输出正常

---

### Phase 3: 实现 Worker (2-3天)

**任务**:
- 实现 `PtyFactory`
- 实现 `IpcClient`
- 实现业务逻辑处理器

**验证**:
- Worker 可以创建 PTY
- Worker 可以发送 master_fd
- 所有业务逻辑正常

---

### Phase 4: 热更新功能 (1-2天)

**任务**:
- 实现 Worker 优雅重启
- 实现孤儿进程处理
- 实现状态迁移（可选）

**验证**:
- Worker 重启，终端保持连接
- 终端输入输出不中断

---

### Phase 5: 功能完整性验证 (2-3天)

**任务**:
- 验证所有旧架构功能
- 性能测试
- 压力测试

**验证**:
- 所有功能正常
- 性能满足要求
- 无严重 bug

---

## 🎉 总结

该架构设计实现了:
- ✅ 性能极致 (零 IPC 开销)
- ✅ 热更新完美 (终端绝对存活)
- ✅ 架构清晰 (职责分离)
- ✅ 故障隔离 (Worker 崩溃不影响终端)
- ✅ 可扩展性强 (支持沙箱、分布式)

这是一个真正能存活"数十年"的架构！