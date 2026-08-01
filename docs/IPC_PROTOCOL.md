# IPC 协议文档

## 📋 文档信息

- **版本**: v1.0
- **协议名称**: Manager-Worker 进程间通信协议
- **创建日期**: 2026-08-01
- **基于架构**: 基于 FD 转移的分层微内核架构
- **实现状态**: ✅ Protobuf 定义已完成

---

## 一、协议概述

### 1.1 设计目标

本 IPC 协议是 GNOME 远程控制系统中 **Manager（网关层）** 和 **Worker（逻辑层）** 之间的通信协议，核心设计目标：

| 目标 | 说明 |
|------|------|
| **数据平面与控制平面分离** | 高吞吐的终端数据流（PTY I/O）**不经过 IPC**，只传输控制指令 |
| **FD 跨进程转移** | Worker 创建 PTY，但将 `master_fd` 通过 `SCM_RIGHTS` 移交给 Manager |
| **低延迟** | Protobuf 序列化 + Unix Domain Socket，延迟 < 1ms |
| **可靠性** | 请求-响应模式，每个请求有唯一 ID 进行匹配 |
| **可扩展性** | 使用 `oneof` 设计，方便添加新的消息类型 |

### 1.2 角色定义

#### Manager（网关层）

- **监听端**: Unix Domain Socket 服务器（`/tmp/gnome-remote-manager.sock`）
- **职责**:
  - 接收 Worker 发送的 `master_fd`（通过 `SCM_RIGHTS`）
  - 持有所有活动 PTY 的文件描述符
  - 直接将 QUIC Stream 的数据读写到 PTY（绕过 Worker）
  - 向 Worker 发送控制指令（创建会话、文件操作、命令执行）
- **生命周期**: 极长（5-10年一次更新）

#### Worker（逻辑层）

- **连接端**: Unix Domain Socket 客户端
- **职责**:
  - 创建 PTY（`forkpty`）
  - 发送 `master_fd` 给 Manager
  - 处理业务逻辑请求（文件操作、命令执行、系统信息查询）
- **生命周期**: 较短（频繁热更新）

### 1.3 数据流架构

```
┌────────────────────────────────────────────────────────────────┐
│                        数据流路径                               │
├────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Client (Tauri)                                                 │
│       │                                                         │
│       │ QUIC Stream (终端输入/输出)                             │
│       ↓                                                         │
│  Manager (网关层)                                               │
│       │                                                         │
│       │ 直接读写 master_fd (不经过 IPC!)                        │
│       ↓                                                         │
│  PTY master_fd ←→ Shell (bash/zsh)                              │
│                                                                 │
│  ───────────────────────────────────────────────────────────   │
│                                                                 │
│  控制流路径 (本协议范围):                                        │
│                                                                 │
│  Manager ──[Unix Socket + Protobuf]──→ Worker                   │
│     │                                      │                    │
│     │ ←─── master_fd (SCM_RIGHTS) ───────┤                    │
│     │                                      │                    │
│     │ ←───── WorkerResponse ─────────────┤                    │
│                                                                 │
└────────────────────────────────────────────────────────────────┘
```

---

## 二、消息格式

### 2.1 Protobuf 定义说明

所有消息使用 **Protobuf 3** 语法定义，文件位置：`protocol/agent.proto`

**消息结构**:

```protobuf
syntax = "proto3";
package agent;

// 请求消息 (Manager -> Worker)
message ManagerRequest {
    uint64 request_id = 1;  // 唯一请求 ID，用于匹配响应
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

// 响应消息 (Worker -> Manager)
message WorkerResponse {
    uint64 request_id = 1;  // 匹配请求 ID
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
```

### 2.2 消息编码方式

**传输格式**:

```
┌─────────────┬──────────────┬────────────────┐
│ 长度 (4B)   │ 类型标志 (1B)│ Protobuf 数据   │
├─────────────┼──────────────┼────────────────┤
│ uint32 LE   │ 0x01=req     │ 变长           │
│             │ 0x02=resp    │                │
└─────────────┴──────────────┴────────────────┘
```

**编码步骤**:

1. 序列化 Protobuf 消息 → `bytes`
2. 计算长度 → `len(bytes)`
3. 写入长度（4 字节，小端序）
4. 写入类型标志（1 字节：`0x01` = 请求，`0x02` = 响应）
5. 写入 Protobuf 数据

**解码步骤**:

1. 读取 4 字节长度
2. 读取 1 字节类型标志
3. 读取 N 字节 Protobuf 数据
4. 反序列化为对应消息类型

### 2.3 特殊数据传输：FD Passing

某些消息（如 `SessionCreated`）需要传输文件描述符（`master_fd`），通过 **Unix Domain Socket 的辅助数据（SCM_RIGHTS）** 发送。

**发送流程**（Worker 端）:

```rust
use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
use std::os::unix::io::AsRawFd;

pub fn send_fd(socket: &UnixStream, fd: RawFd) -> Result<()> {
    let buf = [0u8; 1];  // 伪数据（必须至少 1 字节）
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
```

**接收流程**（Manager 端）:

```rust
use nix::sys::socket::{recvmsg, ControlMessageOwned};

pub fn receive_fd(socket: &UnixStream) -> Result<RawFd> {
    let mut buf = [0u8; 1];
    let mut iov = [IoSlice::new(&mut buf)];

    let msg = recvmsg(
        socket.as_raw_fd(),
        &mut iov,
        Some(&mut [0u8; 64]),  // 辅助数据缓冲区
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

**重要**: FD 通过 `SCM_RIGHTS` 发送，**不在 Protobuf 消息中**。接收方必须：

1. 先读取 Protobuf 消息（`SessionCreated`）
2. 再调用 `recvmsg` 接收 FD

---

## 三、消息流程图

### 3.1 终端会话创建流程

```
Client              Manager              Worker              Shell
  │                   │                    │                  │
  ├─ Open Shell ────→│                    │                  │
  │  (QUIC Stream)    │                    │                  │
  │                   │                    │                  │
  │                   ├─ ManagerRequest ─→│                  │
  │                   │  (CreateSession)   │                  │
  │                   │                    │                  │
  │                   │                    ├─ forkpty() ────→│
  │                   │                    │                  │
  │                   │                    ├─ send_fd() ─────┤
  │                   │                    │  (SCM_RIGHTS)    │
  │                   │                    │                  │
  │                   ├←─── WorkerResponse ┤                  │
  │                   │     (SessionCreated)                  │
  │                   │     + master_fd                       │
  │                   │                    │                  │
  │                   ├─ register_fd()     │                  │
  │                   │  (加入 EventLoop)  │                  │
  │                   │                    │                  │
  │←──── Session ID ──┤                    │                  │
  │  (QUIC Response)   │                    │                  │
  │                   │                    │                  │
  │                   │←─────── PTY output ┼──────────────────┤
  │                   │        (直接读写)   │                  │
  │                   │                    │                  │
  │←───── output ─────┤                    │                  │
  │  (QUIC Stream)     │                    │                  │
```

**步骤说明**:

| 步骤 | 发送方 | 接收方 | 消息类型 | 说明 |
|------|--------|--------|----------|------|
| 1 | Client | Manager | - | QUIC Stream 请求创建终端 |
| 2 | Manager | Worker | `ManagerRequest(CreateSession)` | 发送创建会话请求 |
| 3 | Worker | Shell | - | `forkpty()` 创建 PTY 和子进程 |
| 4 | Worker | Manager | `SCM_RIGHTS` | 通过 Unix Socket 发送 `master_fd` |
| 5 | Worker | Manager | `WorkerResponse(SessionCreated)` | 返回会话信息 |
| 6 | Manager | - | - | 将 `master_fd` 注册到 Event Loop |
| 7 | Manager | Client | - | 通过 QUIC 返回 Session ID |
| 8+ | Shell | Manager | - | PTY 输出直接写入 QUIC Stream |

**关键点**:

- Worker 创建 PTY 后，立即将 `master_fd` 移交给 Manager
- Manager 获得文件描述符后，**直接读写 PTY**，Worker 不再参与数据流
- 后续终端数据流：`Client ↔ QUIC ↔ Manager ↔ master_fd ↔ Shell`，零 IPC 开销

### 3.2 文件操作流程

```
Client              Manager              Worker
  │                   │                    │
  ├─ Read Dir ───────→│                    │
  │                   │                    │
  │                   ├─ ManagerRequest ─→│
  │                   │  (ReadDir)         │
  │                   │                    │
  │                   │                    ├─ fs::read_dir()
  │                   │                    │
  │                   ├←─── WorkerResponse ┤
  │                   │     (DirListing)   │
  │                   │                    │
  │←─── DirListing ───┤                    │
  │                   │                    │
```

**步骤说明**:

| 步骤 | 发送方 | 接收方 | 消息类型 | 说明 |
|------|--------|--------|----------|------|
| 1 | Client | Manager | - | 文件管理器请求读取目录 |
| 2 | Manager | Worker | `ManagerRequest(ReadDir)` | 发送读取目录请求 |
| 3 | Worker | - | - | 执行 `fs::read_dir()` |
| 4 | Worker | Manager | `WorkerResponse(DirListing)` | 返回目录列表 |
| 5 | Manager | Client | - | 通过 QUIC 返回结果 |

**特点**:

- 文件操作是**无状态请求**，不涉及 FD 转移
- Worker 执行操作后立即返回结果
- 适合 SFTP 文件传输、端口转发等场景

### 3.3 命令执行流程

```
Client              Manager              Worker              Process
  │                   │                    │                  │
  ├─ Execute Cmd ────→│                    │                  │
  │                   │                    │                  │
  │                   ├─ ManagerRequest ─→│                  │
  │                   │  (ExecuteCommand)  │                  │
  │                   │                    │                  │
  │                   │                    ├─ spawn() ───────→│
  │                   │                    │                  │
  │                   │                    ├─ wait() ─────────┤
  │                   │                    │                  │
  │                   ├←─── WorkerResponse ┤                  │
  │                   │  (CommandOutput)    │                  │
  │                   │                    │                  │
  │←─── CommandOutput ┤                    │                  │
  │                   │                    │                  │
```

**步骤说明**:

| 步骤 | 发送方 | 接收方 | 消息类型 | 说明 |
|------|--------|--------|----------|------|
| 1 | Client | Manager | - | 执行一次性命令 |
| 2 | Manager | Worker | `ManagerRequest(ExecuteCommand)` | 发送命令执行请求 |
| 3 | Worker | Process | - | `spawn()` 创建子进程 |
| 4 | Worker | Process | - | `wait()` 等待命令完成 |
| 5 | Worker | Manager | `WorkerResponse(CommandOutput)` | 返回输出和退出码 |
| 6 | Manager | Client | - | 通过 QUIC 返回结果 |

**特点**:

- 适合执行一次性命令（如 `ls -la`, `df -h`）
- 返回 `stdout`, `stderr`, `exit_code`
- 支持超时设置（`timeout_secs`）

---

## 四、消息类型详细说明

### 4.1 ManagerRequest 消息类型

#### 4.1.1 CreateSession（创建终端会话）

**消息定义**:

```protobuf
message CreateSession {
    string shell = 1;              // Shell 程序路径（如 "/bin/bash"）
    uint16 cols = 2;               // 终端列数
    uint16 rows = 3;               // 终端行数
    string working_directory = 4;  // 工作目录（可选）
    map<string, string> env = 5;   // 环境变量（可选）
}
```

**字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `shell` | `string` | ✅ | Shell 程序路径，如 `/bin/bash`, `/bin/zsh` |
| `cols` | `uint16` | ✅ | 终端窗口列数（字符宽度） |
| `rows` | `uint16` | ✅ | 终端窗口行数（字符高度） |
| `working_directory` | `string` | ❌ | Shell 启动的工作目录，默认 `$HOME` |
| `env` | `map<string, string>` | ❌ | 额外环境变量，如 `{"TERM": "xterm-256color"}` |

**响应消息**: `SessionCreated`

**使用场景**:
- 用户打开新终端标签页
- 调整终端窗口大小后创建新会话

**示例**:

```rust
let request = ManagerRequest {
    request_id: 12345,
    payload: Some(ManagerRequestPayload::CreateSession(CreateSession {
        shell: "/bin/bash".to_string(),
        cols: 120,
        rows: 36,
        working_directory: "/home/user".to_string(),
        env: HashMap::from([
            ("TERM".to_string(), "xterm-256color".to_string()),
            ("LANG".to_string(), "en_US.UTF-8".to_string()),
        ]),
    })),
};
```

---

#### 4.1.2 ResizeWindow（调整终端窗口大小）

**消息定义**:

```protobuf
message ResizeWindow {
    string session_id = 1;  // 会话 ID
    uint16 cols = 2;        // 新的列数
    uint16 rows = 3;        // 新的行数
}
```

**字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `session_id` | `string` | ✅ | 要调整的会话 ID |
| `cols` | `uint16` | ✅ | 新的列数 |
| `rows` | `uint16` | ✅ | 新的行数 |

**响应消息**: `WorkerResponse`（空响应或 `Error`）

**使用场景**:
- 用户调整终端窗口大小
- 响应窗口大小变化事件

**实现说明**:

Worker 收到此请求后，通过 IPC 指令通知 Manager，由 Manager 执行 `ioctl(master_fd, TIOCSWINSZ, &win)`。

**示例**:

```rust
let request = ManagerRequest {
    request_id: 12346,
    payload: Some(ManagerRequestPayload::ResizeWindow(ResizeWindow {
        session_id: "sess_abc123".to_string(),
        cols: 160,
        rows: 48,
    })),
};
```

---

#### 4.1.3 KillSession（终止终端会话）

**消息定义**:

```protobuf
message KillSession {
    string session_id = 1;  // 要终止的会话 ID
}
```

**字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `session_id` | `string` | ✅ | 要终止的会话 ID |

**响应消息**: `WorkerResponse`（空响应或 `Error`）

**使用场景**:
- 用户关闭终端标签页
- 强制结束会话

**实现说明**:

Worker 向 Shell 进程发送 `SIGTERM` 或 `SIGKILL`，Manager 关闭 `master_fd`。

**示例**:

```rust
let request = ManagerRequest {
    request_id: 12347,
    payload: Some(ManagerRequestPayload::KillSession(KillSession {
        session_id: "sess_abc123".to_string(),
    })),
};
```

---

#### 4.1.4 ReadDir（读取目录）

**消息定义**:

```protobuf
message ReadDir {
    string path = 1;  // 目录路径
}
```

**字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `path` | `string` | ✅ | 要读取的目录路径 |

**响应消息**: `DirListing` 或 `Error`

**使用场景**:
- 文件管理器浏览远程目录
- SFTP 文件传输

**示例**:

```rust
let request = ManagerRequest {
    request_id: 12348,
    payload: Some(ManagerRequestPayload::ReadDir(ReadDir {
        path: "/home/user/projects".to_string(),
    })),
};
```

---

#### 4.1.5 ReadFile（读取文件）

**消息定义**:

```protobuf
message ReadFile {
    string path = 1;     // 文件路径
    uint64 offset = 2;   // 读取偏移量
    uint32 length = 3;   // 读取长度
}
```

**字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `path` | `string` | ✅ | 文件路径 |
| `offset` | `uint64` | ❌ | 读取偏移量（字节），默认 0 |
| `length` | `uint32` | ❌ | 读取长度（字节），默认读取全部 |

**响应消息**: `FileContent` 或 `Error`

**使用场景**:
- 下载远程文件
- 流式传输大文件

**示例**:

```rust
let request = ManagerRequest {
    request_id: 12349,
    payload: Some(ManagerRequestPayload::ReadFile(ReadFile {
        path: "/home/user/file.tar.gz".to_string(),
        offset: 0,
        length: 1024 * 1024,  // 读取前 1MB
    })),
};
```

---

#### 4.1.6 WriteFile（写入文件）

**消息定义**:

```protobuf
message WriteFile {
    string path = 1;     // 文件路径
    uint64 offset = 2;   // 写入偏移量
    bytes data = 3;      // 文件数据
}
```

**字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `path` | `string` | ✅ | 文件路径 |
| `offset` | `uint64` | ❌ | 写入偏移量（字节），默认 0（追加模式） |
| `data` | `bytes` | ✅ | 文件数据 |

**响应消息**: `WriteResult` 或 `Error`

**使用场景**:
- 上传文件到远程服务器
- SFTP 文件传输

**示例**:

```rust
let request = ManagerRequest {
    request_id: 12350,
    payload: Some(ManagerRequestPayload::WriteFile(WriteFile {
        path: "/home/user/upload.tar.gz".to_string(),
        offset: 0,
        data: file_bytes.to_vec(),
    })),
};
```

---

#### 4.1.7 ExecuteCommand（执行命令）

**消息定义**:

```protobuf
message ExecuteCommand {
    string command = 1;              // 命令名称
    repeated string args = 2;        // 命令参数
    string working_directory = 3;    // 工作目录
    map<string, string> env = 4;     // 环境变量
    uint32 timeout_secs = 5;         // 超时时间（秒）
}
```

**字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `command` | `string` | ✅ | 命令名称（如 `ls`, `df`） |
| `args` | `repeated string` | ❌ | 命令参数列表 |
| `working_directory` | `string` | ❌ | 工作目录，默认 `$HOME` |
| `env` | `map<string, string>` | ❌ | 额外环境变量 |
| `timeout_secs` | `uint32` | ❌ | 超时时间（秒），默认 60 秒 |

**响应消息**: `CommandOutput` 或 `Error`

**使用场景**:
- 执行一次性命令
- 系统管理任务（如 `systemctl status nginx`）

**示例**:

```rust
let request = ManagerRequest {
    request_id: 12351,
    payload: Some(ManagerRequestPayload::ExecuteCommand(ExecuteCommand {
        command: "df".to_string(),
        args: vec!["-h".to_string()],
        working_directory: "/".to_string(),
        env: HashMap::new(),
        timeout_secs: 30,
    })),
};
```

---

#### 4.1.8 GetSystemInfo（获取系统信息）

**消息定义**:

```protobuf
message GetSystemInfo {
    repeated string info_types = 1;  // 信息类型列表
}
```

**字段说明**:

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `info_types` | `repeated string` | ❌ | 信息类型，可选值：`"cpu"`, `"memory"`, `"disk"`, `"network"` |

**响应消息**: `SystemInfo` 或 `Error`

**使用场景**:
- 系统监控面板
- 获取服务器状态

**示例**:

```rust
let request = ManagerRequest {
    request_id: 12352,
    payload: Some(ManagerRequestPayload::GetSystemInfo(GetSystemInfo {
        info_types: vec!["cpu".to_string(), "memory".to_string()],
    })),
};
```

---

### 4.2 WorkerResponse 消息类型

#### 4.2.1 SessionCreated（终端会话已创建）

**消息定义**:

```protobuf
message SessionCreated {
    string session_id = 1;  // 会话 ID
    int32 pid = 2;          // Shell 进程 PID
    // 注意: master_fd 通过 SCM_RIGHTS 发送，不在 Protobuf 中
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `session_id` | `string` | 唯一会话标识符（UUID） |
| `pid` | `int32` | Shell 进程的 PID（用于进程管理） |

**重要**: `master_fd` 通过 `SCM_RIGHTS` 单独发送，不在 Protobuf 消息中。

**使用场景**:
- 响应 `CreateSession` 请求
- Manager 收到后注册 FD 到 Event Loop

---

#### 4.2.2 DirListing（目录列表）

**消息定义**:

```protobuf
message DirListing {
    repeated DirEntry entries = 1;
}

message DirEntry {
    string name = 1;          // 文件名
    bool is_dir = 2;          // 是否为目录
    uint64 size = 3;          // 文件大小（字节）
    uint64 modified_time = 4; // 修改时间（Unix timestamp）
    uint32 mode = 5;          // 文件权限（如 0o755）
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `entries` | `repeated DirEntry` | 目录条目列表 |

**DirEntry 字段**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | `string` | 文件名（不含路径） |
| `is_dir` | `bool` | 是否为目录 |
| `size` | `uint64` | 文件大小（字节） |
| `modified_time` | `uint64` | 修改时间（Unix timestamp） |
| `mode` | `uint32` | 文件权限（Unix mode，如 `0o755`） |

**使用场景**:
- 响应 `ReadDir` 请求
- 文件管理器显示目录内容

---

#### 4.2.3 FileContent（文件内容）

**消息定义**:

```protobuf
message FileContent {
    bytes data = 1;  // 文件数据
    bool eof = 2;    // 是否到达文件末尾
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `data` | `bytes` | 文件数据片段 |
| `eof` | `bool` | 是否到达文件末尾（流式传输时使用） |

**使用场景**:
- 响应 `ReadFile` 请求
- 文件下载

---

#### 4.2.4 WriteResult（写入结果）

**消息定义**:

```protobuf
message WriteResult {
    uint32 bytes_written = 1;  // 实际写入的字节数
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `bytes_written` | `uint32` | 实际写入的字节数 |

**使用场景**:
- 响应 `WriteFile` 请求
- 文件上传确认

---

#### 4.2.5 CommandOutput（命令输出）

**消息定义**:

```protobuf
message CommandOutput {
    bytes stdout = 1;     // 标准输出
    bytes stderr = 2;     // 标准错误输出
    int32 exit_code = 3;  // 退出码
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `stdout` | `bytes` | 标准输出（`stdout`） |
| `stderr` | `bytes` | 标准错误输出（`stderr`） |
| `exit_code` | `int32` | 命令退出码（`0` = 成功） |

**使用场景**:
- 响应 `ExecuteCommand` 请求
- 返回命令执行结果

---

#### 4.2.6 SystemInfo（系统信息）

**消息定义**:

```protobuf
message SystemInfo {
    double cpu_percent = 1;    // CPU 使用率（0.0-100.0）
    uint64 mem_total = 2;      // 总内存（字节）
    uint64 mem_used = 3;       // 已用内存（字节）
    uint64 disk_total = 4;     // 总磁盘空间（字节）
    uint64 disk_used = 5;      // 已用磁盘空间（字节）
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `cpu_percent` | `double` | CPU 使用率（百分比，如 `23.5`） |
| `mem_total` | `uint64` | 总内存（字节） |
| `mem_used` | `uint64` | 已用内存（字节） |
| `disk_total` | `uint64` | 总磁盘空间（字节） |
| `disk_used` | `uint64` | 已用磁盘空间（字节） |

**使用场景**:
- 响应 `GetSystemInfo` 请求
- 系统监控面板

---

#### 4.2.7 Error（错误响应）

**消息定义**:

```protobuf
message Error {
    uint32 code = 1;     // 错误码
    string message = 2;  // 错误消息
}
```

**字段说明**:

| 字段 | 类型 | 说明 |
|------|------|------|
| `code` | `uint32` | 错误码（参见错误码定义） |
| `message` | `string` | 人类可读的错误消息 |

**使用场景**:
- 请求处理失败
- 返回错误详情

---

## 五、示例代码

### 5.1 如何发送请求

**Manager 端发送请求**:

```rust
use prost::Message;
use std::os::unix::net::UnixStream;

// 1. 构造请求消息
let request = ManagerRequest {
    request_id: 12345,
    payload: Some(ManagerRequestPayload::CreateSession(CreateSession {
        shell: "/bin/bash".to_string(),
        cols: 120,
        rows: 36,
        working_directory: "/home/user".to_string(),
        env: HashMap::new(),
    })),
};

// 2. 序列化为 Protobuf
let mut buf = Vec::new();
request.encode(&mut buf)?;

// 3. 发送到 Unix Socket
let mut socket = UnixStream::connect("/tmp/gnome-remote-manager.sock")?;

// 写入长度（4 字节）
socket.write_all(&(buf.len() as u32).to_le_bytes())?;

// 写入类型标志（1 字节）
socket.write_all(&[0x01])?;  // 0x01 = Request

// 写入 Protobuf 数据
socket.write_all(&buf)?;
```

### 5.2 如何接收响应

**Manager 端接收响应**:

```rust
use std::io::Read;

// 1. 读取长度（4 字节）
let mut len_buf = [0u8; 4];
socket.read_exact(&mut len_buf)?;
let len = u32::from_le_bytes(len_buf) as usize;

// 2. 读取类型标志（1 字节）
let mut type_buf = [0u8; 1];
socket.read_exact(&mut type_buf)?;
let msg_type = type_buf[0];

// 3. 读取 Protobuf 数据
let mut data_buf = vec![0u8; len];
socket.read_exact(&mut data_buf)?;

// 4. 反序列化
let response = WorkerResponse::decode(&data_buf[..])?;

// 5. 匹配 request_id
if response.request_id == 12345 {
    match response.payload {
        Some(WorkerResponsePayload::SessionCreated(sess)) => {
            println!("Session created: {}", sess.session_id);
        }
        Some(WorkerResponsePayload::Error(err)) => {
            eprintln!("Error: {} - {}", err.code, err.message);
        }
        _ => {}
    }
}
```

### 5.3 如何处理错误

**完整错误处理示例**:

```rust
use anyhow::{Context, Result};

async fn handle_request(socket: &mut UnixStream, request: ManagerRequest) -> Result<()> {
    // 发送请求
    send_request(socket, request).await
        .context("Failed to send request")?;

    // 接收响应
    let response = receive_response(socket).await
        .context("Failed to receive response")?;

    // 处理响应
    match response.payload {
        Some(WorkerResponsePayload::SessionCreated(sess)) => {
            tracing::info!("Session created: id={}, pid={}", sess.session_id, sess.pid);
            Ok(())
        }
        Some(WorkerResponsePayload::Error(err)) => {
            tracing::error!("Request failed: code={}, msg={}", err.code, err.message);
            Err(anyhow::anyhow!("Worker error {}: {}", err.code, err.message))
        }
        Some(WorkerResponsePayload::DirListing(listing)) => {
            tracing::info!("Directory listing: {} entries", listing.entries.len());
            Ok(())
        }
        _ => {
            tracing::warn!("Unexpected response type");
            Err(anyhow::anyhow!("Unexpected response type"))
        }
    }
}
```

### 5.4 接收文件描述符

**接收 master_fd（Manager 端）**:

```rust
use nix::sys::socket::{recvmsg, ControlMessageOwned, MsgFlags};
use std::os::unix::io::{AsRawFd, FromRawFd};

// 1. 先接收 Protobuf 消息
let response = receive_response(&socket)?;

// 2. 如果是 SessionCreated，接收 FD
if let Some(WorkerResponsePayload::SessionCreated(sess)) = response.payload {
    // 接收 FD
    let mut buf = [0u8; 1];
    let mut iov = [IoSlice::new(&mut buf)];

    let msg = recvmsg(
        socket.as_raw_fd(),
        &mut iov,
        Some(&mut [0u8; 64]),
        MsgFlags::empty(),
    )?;

    let master_fd = msg.cmsgs()?
        .find_map(|cmsg| {
            if let ControlMessageOwned::ScmRights(fds) = cmsg {
                Some(fds[0])
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow!("No FD received"))?;

    tracing::info!("Received master_fd: {}", master_fd);

    // 3. 将 FD 转换为 File 并注册到 Event Loop
    let master_file = unsafe { std::fs::File::from_raw_fd(master_fd) };
    // ... 注册到 epoll/kqueue ...
}
```

---

## 六、错误处理规范

### 6.1 错误码定义

| 错误码 | 名称 | 说明 | HTTP 等价 |
|--------|------|------|-----------|
| `0` | `SUCCESS` | 成功（不使用 Error 消息） | 200 |
| `1001` | `INVALID_REQUEST` | 请求格式错误 | 400 |
| `1002` | `UNKNOWN_REQUEST` | 未知的请求类型 | 400 |
| `1003` | `MISSING_FIELD` | 缺少必填字段 | 400 |
| `2001` | `SESSION_NOT_FOUND` | 会话不存在 | 404 |
| `2002` | `SESSION_ALREADY_EXISTS` | 会话已存在 | 409 |
| `2003` | `SHELL_LAUNCH_FAILED` | Shell 启动失败 | 500 |
| `3001` | `FILE_NOT_FOUND` | 文件不存在 | 404 |
| `3002` | `PERMISSION_DENIED` | 权限不足 | 403 |
| `3003` | `PATH_NOT_ALLOWED` | 路径不在白名单中 | 403 |
| `3004` | `READ_ERROR` | 读取文件失败 | 500 |
| `3005` | `WRITE_ERROR` | 写入文件失败 | 500 |
| `4001` | `COMMAND_TIMEOUT` | 命令执行超时 | 504 |
| `4002` | `COMMAND_FAILED` | 命令执行失败 | 500 |
| `5001` | `INTERNAL_ERROR` | 内部错误 | 500 |

### 6.2 错误处理流程

```
Manager                Worker
   │                     │
   ├─ ManagerRequest ───→│
   │                     │
   │                     ├─ 处理请求
   │                     │
   │                     ├─ 发生错误？
   │                     │  ├─ 是 → 构造 Error 响应
   │                     │  │       WorkerResponse { error: { code, message } }
   │                     │  │
   │                     │  └─ 否 → 构造正常响应
   │                     │          WorkerResponse { ... }
   │                     │
   │←── WorkerResponse ──┤
   │                     │
   ├─ 检查 payload       │
   │  ├─ Error?          │
   │  │  └─ 记录日志     │
   │  │     返回错误     │
   │  │                  │
   │  └─ 正常响应?       │
   │     └─ 继续处理     │
   │                     │
```

### 6.3 错误处理最佳实践

#### 6.3.1 Worker 端错误处理

```rust
pub async fn handle_read_dir(req: ReadDirRequest) -> Result<DirListing, Error> {
    // 权限检查
    if !is_path_allowed(&req.path) {
        return Err(Error {
            code: 3003,  // PATH_NOT_ALLOWED
            message: format!("Path '{}' is not in allowed list", req.path),
        });
    }

    // 执行操作
    let entries = match fs::read_dir(&req.path) {
        Ok(entries) => entries,
        Err(e) => {
            return Err(Error {
                code: match e.kind() {
                    io::ErrorKind::NotFound => 3001,  // FILE_NOT_FOUND
                    io::ErrorKind::PermissionDenied => 3002,  // PERMISSION_DENIED
                    _ => 3004,  // READ_ERROR
                },
                message: format!("Failed to read directory '{}': {}", req.path, e),
            });
        }
    };

    // 构造响应
    let dir_listing = DirListing {
        entries: entries
            .filter_map(|e| e.ok())
            .map(|e| DirEntry {
                name: e.file_name().to_string_lossy().to_string(),
                is_dir: e.file_type().map(|t| t.is_dir()).unwrap_or(false),
                size: e.metadata().map(|m| m.len()).unwrap_or(0),
                modified_time: e.metadata()
                    .and_then(|m| m.modified())
                    .map(|t| t.duration_since(UNIX_EPOCH).unwrap().as_secs())
                    .unwrap_or(0),
                mode: e.metadata().map(|m| m.mode()).unwrap_or(0),
            })
            .collect(),
    };

    Ok(dir_listing)
}
```

#### 6.3.2 Manager 端错误处理

```rust
pub async fn handle_response(response: WorkerResponse) -> Result<(), anyhow::Error> {
    match response.payload {
        Some(WorkerResponsePayload::Error(err)) => {
            // 记录错误日志
            tracing::error!(
                "Request {} failed: code={}, message={}",
                response.request_id,
                err.code,
                err.message
            );

            // 根据 error code 返回不同错误
            match err.code {
                3001..=3005 => Err(anyhow!("File operation failed: {}", err.message)),
                2001..=2003 => Err(anyhow!("Session operation failed: {}", err.message)),
                4001..=4002 => Err(anyhow!("Command execution failed: {}", err.message)),
                _ => Err(anyhow!("Internal error: {}", err.message)),
            }
        }
        Some(WorkerResponsePayload::SessionCreated(sess)) => {
            tracing::info!("Session created: id={}, pid={}", sess.session_id, sess.pid);
            Ok(())
        }
        Some(WorkerResponsePayload::DirListing(listing)) => {
            tracing::info!("Directory listing: {} entries", listing.entries.len());
            Ok(())
        }
        _ => {
            tracing::warn!("Unexpected response type for request {}", response.request_id);
            Err(anyhow!("Unexpected response type"))
        }
    }
}
```

---

## 七、协议扩展指南

### 7.1 添加新消息类型

**步骤**:

1. 在 `protocol/agent.proto` 中添加新消息定义
2. 在 `ManagerRequest` 或 `WorkerResponse` 的 `oneof payload` 中添加新字段
3. 重新生成 Rust 代码（`cargo build`）
4. 在 Worker 中实现新的 handler 函数
5. 在 Manager 中添加调用入口

**示例：添加「重启服务」功能**:

```protobuf
// 1. 在 agent.proto 中添加新消息
message RestartService {
    string service_name = 1;  // 服务名称（如 "nginx"）
}

message ServiceStatus {
    string name = 1;          // 服务名称
    string status = 2;        // 状态（running/stopped）
}

// 2. 在 ManagerRequest 中添加
message ManagerRequest {
    // ... existing fields ...
    oneof payload {
        // ... existing payloads ...
        RestartService restart_service = 10;  // 新增
    }
}

// 3. 在 WorkerResponse 中添加
message WorkerResponse {
    // ... existing fields ...
    oneof payload {
        // ... existing payloads ...
        ServiceStatus service_status = 9;  // 新增
    }
}
```

### 7.2 版本兼容性

**向后兼容规则**:

1. ✅ **可以添加新字段**（使用新的 field number）
2. ✅ **可以添加新的 `oneof` 分支**
3. ❌ **不要删除或重命名现有字段**
4. ❌ **不要改变现有字段的类型**
5. ❌ **不要改变现有字段的编号**

**版本协商**:

建议在 Manager 和 Worker 启动时交换协议版本号：

```rust
// Manager -> Worker: ProtocolVersion { version: "1.0.0" }
// Worker -> Manager: ProtocolVersion { version: "1.0.0", supported: true }
```

---

## 八、性能优化建议

### 8.1 批量处理

对于大量文件列表，建议分批传输：

```rust
// 分批发送目录列表（每批 100 条）
for chunk in entries.chunks(100) {
    let response = WorkerResponse {
        request_id: request_id,
        payload: Some(WorkerResponsePayload::DirListing(DirListing {
            entries: chunk.to_vec(),
        })),
    };
    send_response(&socket, &response)?;
}
```

### 8.2 流式传输

对于大文件，使用流式传输（分块读取）：

```rust
// 流式传输文件（每块 64KB）
let chunk_size = 64 * 1024;
let mut offset = 0;

loop {
    let mut buf = vec![0u8; chunk_size];
    let n = file.read(&mut buf)?;
    if n == 0 {
        break;
    }

    let response = WorkerResponse {
        request_id: request_id,
        payload: Some(WorkerResponsePayload::FileContent(FileContent {
            data: buf[..n].to_vec(),
            eof: offset + n >= file_size,
        })),
    };
    send_response(&socket, &response)?;

    offset += n;
}
```

### 8.3 连接复用

- **Unix Socket 连接池**: Worker 维护多个到 Manager 的连接
- **请求流水线**: 发送多个请求而不等待响应

---

## 九、安全考虑

### 9.1 输入验证

**必须验证的字段**:

- 文件路径：检查是否在白名单中
- 命令参数：检查是否包含危险命令
- 环境变量：过滤敏感变量（如 `SSH_AUTH_SOCK`）

**示例**:

```rust
fn validate_path(path: &str) -> Result<(), Error> {
    // 检查路径是否在白名单中
    let allowed_paths = ["/home", "/etc", "/var/log"];

    if !allowed_paths.iter().any(|p| path.starts_with(p)) {
        return Err(Error {
            code: 3003,
            message: format!("Path '{}' is not allowed", path),
        });
    }

    // 检查路径注入（如 "../../../etc/passwd"）
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| Error {
            code: 3001,
            message: format!("Invalid path: {}", e),
        })?;

    if !allowed_paths.iter().any(|p| canonical.starts_with(p)) {
        return Err(Error {
            code: 3003,
            message: format!("Path '{}' resolves to forbidden location", path),
        });
    }

    Ok(())
}
```

### 9.2 权限隔离

- Worker 应该以低权限用户运行（如 `nobody`）
- 敏感操作需要 Manager 授权
- 文件操作路径白名单强制执行

---

## 十、附录

### 10.1 完整 Protobuf 定义

参见：`protocol/agent.proto`

### 10.2 消息类型速查表

| 请求类型 | 响应类型 | 说明 |
|----------|----------|------|
| `CreateSession` | `SessionCreated` | 创建终端会话 |
| `ResizeWindow` | - | 调整终端窗口大小 |
| `KillSession` | - | 终止终端会话 |
| `ReadDir` | `DirListing` | 读取目录 |
| `ReadFile` | `FileContent` | 读取文件 |
| `WriteFile` | `WriteResult` | 写入文件 |
| `ExecuteCommand` | `CommandOutput` | 执行命令 |
| `GetSystemInfo` | `SystemInfo` | 获取系统信息 |

### 10.3 相关文档

- [架构设计文档](./ARCHITECTURE.md)
- [开发日志](./DEVELOPMENT_LOG.md)
- [代码规范](./.trae/rules/项目规范.md)

---

**文档结束**