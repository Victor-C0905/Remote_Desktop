# Agent 新架构开发 - 任务分解文档

## 📋 文档信息

- **版本**: v1.0
- **创建日期**: 2026-07-31
- **目标**: 极细粒度任务分解，支持多AI模型协作开发

---

## 🎯 使用说明

### 如何使用本文档

1. **任务粒度**：每个任务设计为 1-2 小时内完成
2. **任务独立性**：每个任务都有明确的输入、输出、验证标准
3. **上下文传递**：完成任务后，更新"进度记录"部分
4. **切换AI模型**：新AI模型通过阅读本文档，可以无缝接手开发

---

## 📊 总体进度

**当前阶段**: Phase 0 - 准备阶段
**当前任务**: TASK-000
**总任务数**: 40+
**已完成**: 0
**进行中**: 0

---

## 📅 Phase 0: 准备阶段

### TASK-000: 创建任务分解文档

**状态**: ✅ 已完成
**预计时间**: 30分钟
**实际时间**: 30分钟

**任务描述**:
- 创建极细粒度的任务分解文档
- 定义任务模板和进度跟踪机制

**输入**:
- 架构设计文档（ARCHITECTURE.md）

**输出**:
- 任务分解文档（TASK_BREAKDOWN.md）

**验证标准**:
- ✅ 文档创建成功
- ✅ 任务分解清晰
- ✅ 进度跟踪机制定义完成

---

### TASK-001: 创建项目目录结构

**状态**: ⬜ 未开始
**预计时间**: 15分钟
**依赖**: 无

**任务描述**:
- 创建新架构所需的目录结构
- 遵循架构设计文档的模块划分

**输入**:
- 架构设计文档（ARCHITECTURE.md）中的模块设计部分

**输出**:
```
agent/
├── src/
│   ├── manager/           # Manager 模块（新建）
│   │   ├── mod.rs
│   │   ├── connection.rs
│   │   ├── session.rs
│   │   ├── auth.rs
│   │   ├── pty_registry.rs
│   │   ├── worker_manager.rs
│   │   └── ipc_server.rs
│   ├── worker/            # Worker 模块（新建）
│   │   ├── mod.rs
│   │   ├── pty_factory.rs
│   │   ├── ipc_client.rs
│   │   └── handlers/
│   │       ├── mod.rs
│   │       ├── file.rs
│   │       ├── command.rs
│   │       └── system.rs
│   └── protocol/          # 协议模块（新建）
│       ├── mod.rs
│       └── generated.rs   # 由 .proto 自动生成
├── protocol/              # Protobuf 定义（新建）
│   └── agent.proto
└── tests/                 # 测试目录（新建）
    ├── manager_test.rs
    ├── worker_test.rs
    └── integration_test.rs
```

**执行步骤**:
```bash
# 1. 创建 Manager 模块目录
cd e:\MyWork\gnome-remote\agent
mkdir -p src/manager
mkdir -p src/worker/handlers
mkdir -p src/protocol
mkdir -p protocol
mkdir -p tests

# 2. 创建空文件（占位）
touch src/manager/mod.rs
touch src/worker/mod.rs
touch src/worker/pty_factory.rs
touch src/worker/ipc_client.rs
touch src/worker/handlers/mod.rs
touch src/protocol/mod.rs
touch protocol/agent.proto
```

**验证标准**:
- ✅ 所有目录创建成功
- ✅ 文件结构符合架构设计
- ✅ 运行 `tree src/` 查看结构正确

**完成记录**:
- 完成时间：
- AI模型：
- 备注：

---

### TASK-002: 配置 Cargo.toml 依赖

**状态**: ⬜ 未开始
**预计时间**: 20分钟
**依赖**: TASK-001

**任务描述**:
- 更新 Cargo.toml，添加新架构所需的依赖
- 配置 Protobuf 编译支持

**输入**:
- 当前 Cargo.toml
- 架构设计文档中的依赖列表

**输出**:
- 更新后的 Cargo.toml

**执行步骤**:
1. 阅读 `agent/Cargo.toml`
2. 添加以下依赖：
   ```toml
   [dependencies]
   # ... 现有依赖 ...

   # Protobuf 支持
   prost = "0.12"
   prost-types = "0.12"

   # Unix Socket 和 FD 传递
   nix = { version = "0.27", features = ["socket", "uio", "term"] }

   # 进程管理
   tokio = { version = "1.52", features = ["process", "signal"] }

   [build-dependencies]
   prost-build = "0.12"
   ```

3. 创建 `build.rs`：
   ```rust
   fn main() {
       prost_build::compile_protos(&["protocol/agent.proto"], &["protocol/"])
           .expect("Failed to compile protos");
   }
   ```

**验证标准**:
- ✅ Cargo.toml 更新成功
- ✅ `cargo check` 通过（暂不编译，只检查依赖）

**完成记录**:
- 完成时间：
- AI模型：
- 备注：

---

## 📅 Phase 1: IPC 协议定义

### TASK-003: 定义基础消息类型

**状态**: ⬜ 未开始
**预计时间**: 30分钟
**依赖**: TASK-001

**任务描述**:
- 编写 `agent.proto` 的基础消息类型
- 定义 ManagerRequest 和 WorkerResponse

**输入**:
- 架构设计文档中的 IPC 协议定义部分（line 384-536）

**输出**:
- `protocol/agent.proto` 文件（部分完成）

**执行步骤**:
1. 创建文件 `protocol/agent.proto`
2. 编写以下内容：
   ```protobuf
   syntax = "proto3";

   package agent;

   // Manager -> Worker 的请求消息
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

   // Worker -> Manager 的响应消息
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

   // 错误响应
   message Error {
       uint32 code = 1;
       string message = 2;
   }
   ```

**验证标准**:
- ✅ 文件创建成功
- ✅ Protobuf 语法正确（无语法错误）

**完成记录**:
- 完成时间：
- AI模型：
- 备注：

---

### TASK-004: 定义终端会话相关消息

**状态**: ⬜ 未开始
**预计时间**: 20分钟
**依赖**: TASK-003

**任务描述**:
- 补充终端会话相关的消息定义
- 定义 CreateSession、ResizeWindow、KillSession、SessionCreated

**输入**:
- 架构设计文档中的终端会话消息定义（line 413-489）

**输出**:
- `protocol/agent.proto` 文件（补充内容）

**执行步骤**:
在 `agent.proto` 中添加：
```protobuf
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

// 终端会话已创建
message SessionCreated {
    string session_id = 1;
    int32 pid = 2;
    // 注意: master_fd 通过 SCM_RIGHTS 发送，不在 Protobuf 中
}
```

**验证标准**:
- ✅ 消息定义完整
- ✅ Protobuf 语法正确

**完成记录**:
- 完成时间：
- AI模型：
- 备注：

---

### TASK-005: 定义文件操作相关消息

**状态**: ⬜ 未开始
**预计时间**: 20分钟
**依赖**: TASK-003

**任务描述**:
- 补充文件操作相关的消息定义
- 定义 ReadDir、ReadFile、WriteFile、DirListing、FileContent、WriteResult

**输入**:
- 架构设计文档中的文件操作消息定义（line 434-513）

**输出**:
- `protocol/agent.proto` 文件（补充内容）

**执行步骤**:
在 `agent.proto` 中添加：
```protobuf
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
```

**验证标准**:
- ✅ 消息定义完整
- ✅ Protobuf 语法正确

**完成记录**:
- 完成时间：
- AI模型：
- 备注：

---

### TASK-006: 定义命令执行和系统信息消息

**状态**: ⬜ 未开始
**预计时间**: 20分钟
**依赖**: TASK-003

**任务描述**:
- 补充命令执行和系统信息相关的消息定义
- 定义 ExecuteCommand、CommandOutput、GetSystemInfo、SystemInfo

**输入**:
- 架构设计文档中的命令执行消息定义（line 453-529）

**输出**:
- `protocol/agent.proto` 文件（补充内容）

**执行步骤**:
在 `agent.proto` 中添加：
```protobuf
// 执行命令
message ExecuteCommand {
    string command = 1;
    repeated string args = 2;
    string working_directory = 3;
    map<string, string> env = 4;
    uint32 timeout_secs = 5;
}

// 命令输出
message CommandOutput {
    bytes stdout = 1;
    bytes stderr = 2;
    int32 exit_code = 3;
}

// 获取系统信息
message GetSystemInfo {
    repeated string info_types = 1; // "cpu", "memory", "disk", "network"
}

// 系统信息
message SystemInfo {
    double cpu_percent = 1;
    uint64 mem_total = 2;
    uint64 mem_used = 3;
    uint64 disk_total = 4;
    uint64 disk_used = 5;
}
```

**验证标准**:
- ✅ 消息定义完整
- ✅ Protobuf 语法正确

**完成记录**:
- 完成时间：
- AI模型：
- 备注：

---

### TASK-007: 编译 Protobuf 文件

**状态**: ⬜ 未开始
**预计时间**: 15分钟
**依赖**: TASK-002, TASK-003, TASK-004, TASK-005, TASK-006

**任务描述**:
- 使用 `prost-build` 编译 Protobuf 文件
- 生成 Rust 代码到 `src/protocol/generated.rs`

**输入**:
- 完整的 `protocol/agent.proto` 文件
- 配置好的 `build.rs`

**输出**:
- 自动生成的 `src/protocol/generated.rs`

**执行步骤**:
```bash
# 1. 编译项目（会自动触发 build.rs）
cd e:\MyWork\gnome-remote\agent
cargo build

# 2. 查看生成的代码
ls -la src/protocol/generated.rs

# 3. 查看生成的代码内容（可选）
head -50 src/protocol/generated.rs
```

**验证标准**:
- ✅ `cargo build` 成功
- ✅ `src/protocol/generated.rs` 文件生成
- ✅ 生成的代码包含所有定义的消息类型

**完成记录**:
- 完成时间：
- AI模型：
- 备注：

---

### TASK-008: 创建 protocol 模块封装

**状态**: ⬜ 未开始
**预计时间**: 20分钟
**依赖**: TASK-007

**任务描述**:
- 创建 `src/protocol/mod.rs`
- 封装生成的 Protobuf 代码
- 提供易用的 Rust API

**输入**:
- 自动生成的 `src/protocol/generated.rs`

**输出**:
- `src/protocol/mod.rs`
- 易用的 Rust API

**执行步骤**:
1. 创建 `src/protocol/mod.rs`：
   ```rust
   //! IPC 协议模块
   //!
   //! 该模块封装了 Protobuf 生成的消息类型，提供易用的 Rust API。

   // 导入自动生成的代码
   pub mod generated;

   // 重新导出常用的消息类型
   pub use generated::{
       ManagerRequest, WorkerResponse, Error,
       CreateSession, ResizeWindow, KillSession, SessionCreated,
       ReadDir, ReadFile, WriteFile, DirListing, FileContent, WriteResult,
       ExecuteCommand, CommandOutput, GetSystemInfo, SystemInfo,
   };

   /// 辅助函数：创建错误响应
   pub fn create_error_response(request_id: u64, code: u32, message: &str) -> WorkerResponse {
       WorkerResponse {
           request_id,
           payload: Some(generated::worker_response::Payload::Error(Error {
               code,
               message: message.to_string(),
           })),
       }
   }
   ```

2. 更新 `src/main.rs`（如果需要）：
   ```rust
   mod protocol;

   // 在其他地方使用
   use protocol::{ManagerRequest, WorkerResponse};
   ```

**验证标准**:
- ✅ `cargo build` 成功
- ✅ 可以在代码中使用 `use protocol::ManagerRequest;`
- ✅ 辅助函数可用

**完成记录**:
- 完成时间：
- AI模型：
- 备注：

---

### TASK-009: 编写 IPC 协议文档

**状态**: ⬜ 未开始
**预计时间**: 30分钟
**依赖**: TASK-008

**任务描述**:
- 编写详细的 IPC 协议文档
- 包含消息格式、流程图、示例代码

**输入**:
- 架构设计文档
- 编译成功的 Protobuf 定义

**输出**:
- `docs/IPC_PROTOCOL.md`

**执行步骤**:
创建 `docs/IPC_PROTOCOL.md`，包含：
1. 协议概述
2. 消息格式（Protobuf 定义）
3. 消息流程图
4. 每种消息类型的详细说明
5. 示例代码（如何发送/接收消息）
6. 错误处理规范

**验证标准**:
- ✅ 文档创建成功
- ✅ 内容完整（包含上述所有部分）
- ✅ 示例代码可运行

**完成记录**:
- 完成时间：
- AI模型：
- 备注：

---

## 📅 Phase 2: Manager 实现

### TASK-010: 创建 Manager 模块基础结构

**状态**: ⬜ 未开始
**预计时间**: 20分钟
**依赖**: TASK-008

**任务描述**:
- 创建 `src/manager/mod.rs`
- 定义 Manager 的核心结构

**输入**:
- 架构设计文档中的 Manager 模块设计

**输出**:
- `src/manager/mod.rs`

**执行步骤**:
创建 `src/manager/mod.rs`：
```rust
//! Manager 模块（网关层）
//!
//! 负责监听 QUIC 端口，管理客户端连接，持有 PTY master_fd，直接读写 PTY。

pub mod connection;
pub mod session;
pub mod auth;
pub mod pty_registry;
pub mod worker_manager;
pub mod ipc_server;

pub use connection::ConnectionManager;
pub use session::SessionManager;
pub use pty_registry::PtyRegistry;
pub use worker_manager::WorkerManager;
pub use ipc_server::IpcServer;

use std::sync::Arc;

/// Manager 主结构
pub struct Manager {
    /// PTY 注册表（管理所有 master_fd）
    pty_registry: Arc<PtyRegistry>,

    /// Worker 进程管理器
    worker_manager: Arc<WorkerManager>,

    /// 用户会话管理器
    session_manager: Arc<SessionManager>,

    /// IPC Server（接收 Worker 的 FD）
    ipc_server: Arc<IpcServer>,
}

impl Manager {
    /// 创建新的 Manager
    pub async fn new(config: &Config) -> Result<Self> {
        // TODO: 实现
        unimplemented!()
    }

    /// 启动 Manager
    pub async fn run(&self) -> Result<()> {
        // TODO: 实现
        unimplemented!()
    }
}
```

**验证标准**:
- ✅ 文件创建成功
- ✅ `cargo check` 通过（允许未实现的函数）

**完成记录**:
- 完成时间：
- AI模型：
- 备注：

---

### TASK-011: 实现 PtyRegistry 基础结构

**状态**: ⬜ 未开始
**预计时间**: 30分钟
**依赖**: TASK-010

**任务描述**:
- 创建 `src/manager/pty_registry.rs`
- 实现 PTY 注册表的基础结构

**输入**:
- 架构设计文档中的 PtyRegistry 定义（line 104-108）

**输出**:
- `src/manager/pty_registry.rs`

**执行步骤**:
创建 `src/manager/pty_registry.rs`：
```rust
//! PTY 注册表
//!
//! 管理所有活动的 PTY master_fd。

use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use anyhow::Result;

/// PTY 会话信息
#[derive(Debug, Clone)]
pub struct PtySession {
    /// 会话 ID
    pub session_id: String,

    /// PTY master 文件描述符
    pub master_fd: RawFd,

    /// 用户信息
    pub user_info: UserInfo,

    /// 创建时间
    pub created_at: SystemTime,
}

/// 用户信息（简化版）
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
}

/// PTY 注册表
pub struct PtyRegistry {
    /// session_id -> PtySession
    sessions: Arc<RwLock<HashMap<String, PtySession>>>,
}

impl PtyRegistry {
    /// 创建新的 PTY 注册表
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册 PTY 会话
    pub async fn register(&self, session: PtySession) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.session_id.clone(), session);
        Ok(())
    }

    /// 注销 PTY 会话
    pub async fn unregister(&self, session_id: &str) -> Result<Option<PtySession>> {
        let mut sessions = self.sessions.write().await;
        Ok(sessions.remove(session_id))
    }

    /// 获取 PTY 会话
    pub async fn get(&self, session_id: &str) -> Option<PtySession> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// 获取所有活动会话的 ID
    pub async fn list_sessions(&self) -> Vec<String> {
        let sessions = self.sessions.read().await;
        sessions.keys().cloned().collect()
    }
}
```

**验证标准**:
- ✅ 文件创建成功
- ✅ `cargo check` 通过

**完成记录**:
- 完成时间：
- AI模型：
- 备注：

---

---

## 📝 进度记录

### 完成的任务

| 任务ID | 任务名称 | 完成时间 | AI模型 | 备注 |
|--------|---------|---------|--------|------|
| TASK-000 | 创建任务分解文档 | 2026-07-31 | AI-1 | 完成 |

---

## 🚀 如何接手开发（给下一个AI模型）

### 步骤 1: 阅读文档

1. 阅读本文档（TASK_BREAKDOWN.md）
2. 阅读架构设计文档（docs/ARCHITECTURE.md）
3. 查看当前进度（"总体进度"部分）

### 步骤 2: 找到下一个任务

1. 查看"总体进度"部分，找到第一个未完成的任务
2. 阅读任务的详细说明（输入、输出、执行步骤、验证标准）

### 步骤 3: 执行任务

1. 按照执行步骤完成任务
2. 运行验证标准中的命令，确保任务完成

### 步骤 4: 记录进度

1. 在任务说明部分，更新"完成记录"
2. 在"进度记录"部分，添加完成的任务

### 步骤 5: 提交代码

```bash
# 提交代码
git add .
git commit -m "完成 TASK-XXX: 任务名称"

# 推送到远程（可选）
git push origin feature/microkernel-architecture
```

---

## 📞 联系方式

如有问题，请：
1. 查看架构设计文档（docs/ARCHITECTURE.md）
2. 查看已有代码的实现
3. 在任务说明部分添加备注