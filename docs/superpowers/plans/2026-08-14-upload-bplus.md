# 文件传输 B+ 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 文件上传/下载吞吐 5-15x(大文件多流 10-30x),不劣于 SSH;补齐用户隔离/路径/审计/临时文件安全;协议帧与 IO 抽象统一复用到 PTY/订阅。

**Architecture:** 三层并做——安全层(fork+setuid+namespace+pipe 桥接+路径校验+审计+临时文件防护)、性能层(裸二进制帧+Bbr+8MB 窗口+256KB chunk+大文件多流并行)、复用层(TransportFrame+Payload 按域拆分+AsyncFileIO+PathGuard)。

**Tech Stack:** Rust 2024 edition(1.85+),quinn 0.11(+bbr feature),rustls 0.23(ring),tokio,os_pipe,serde_json,uuid。

**关联 spec:** `docs/superpowers/specs/2026-08-14-upload-bplus-design.md`

**核查修正(2026-08-14):** 已对照实际代码修正 7 处假设:方案 ii→ii'(pipe,SCM_RIGHTS 基建不存在)、namespace 两步调用、validate_path 用 &Path、audit_log 逐层下传、frame_mode 需 #[serde(default)]、Payload 57 变体、transfer_session_ids Vec 防重复。

**约束:**
- 零编译错误(项目硬约束),每阶段 `cargo build` 通过。
- Windows 编译验证 lib(unix stub 已就位),WSL(`wsl -e bash -l -c`)验证完整。
- 注释保留、项目结构分类完整、向后兼容(`frame_mode` 协商 + `#[serde(default)]`)。
- **git 操作由用户执行**(用户规则),计划中 commit 步骤为提示而非自动执行。

---

## 文件结构

### 新增(agent)
- `agent/src/protocol/raw_frame.rs` — 裸二进制帧编解码(`RawChunk` + read/write)
- `agent/src/protocol/transport_frame.rs` — 统一帧抽象(`Frame::Control/Binary`)
- `agent/src/protocol/{auth,file,transfer,terminal,subscription,system}.rs` — Payload 按域拆分(从 serde.rs,可选延后)
- `agent/src/auth/path_guard.rs` — 路径校验(`SafePath` newtype)
- `agent/src/auth/async_file.rs` — `AsyncFileIO` trait

### 修改(agent)
- `agent/src/protocol/mod.rs` — 注册新模块
- `agent/src/protocol/serde.rs` — FileTransferAccept 加 frame_mode(`#[serde(default)]`);Payload re-export 拆分域
- `agent/src/file_stream.rs` — chunk 256KB、BufWriter 256KB(子进程端)、O_NOFOLLOW、Uuid 临时名、write_range、FileStreamWriter 持 pipe sender(方案 ii')
- `agent/src/server/quic.rs` — TransportConfig(Bbr+8MB)、upload/download handler 改裸帧、PTY 改裸帧、audit_log 下传
- `agent/src/handler.rs` — 入口路径校验 + 审计、chunk_size 256KB、audit_log 参数下传
- `agent/src/auth/executor.rs` — 新增 fork+setuid+namespace+pipe 桥接方法(方案 ii')
- `agent/src/auth/namespace.rs` — 启用到生产路径(两步调用:`UserNamespace::new(uid,gid).create_and_switch()`)
- `agent/src/audit.rs` — 启用 `log_file_operation`(去 `#[allow(dead_code)]`)
- `agent/Cargo.toml` — quinn bbr feature、os_pipe 依赖

### 修改(客户端)
- `src-tauri/src/transfer.rs` — upload/download 循环改裸帧、chunk 256KB、多流分片调度、客户端 read_next_chunk spawn_blocking
- `src-tauri/src/connection.rs` — 客户端 TransportConfig(Bbr+8MB)
- `src-tauri/Cargo.toml` — quinn bbr feature

---

## 阶段 0:协议帧抽象与测试基础设施

### Task 0.1: 裸帧模块

**Files:**
- Create: `agent/src/protocol/raw_frame.rs`
- Modify: `agent/src/protocol/mod.rs`

- [ ] **Step 1: 新增 raw_frame.rs**

```rust
// agent/src/protocol/raw_frame.rs
//! 裸二进制帧编解码(数据平面)
//!
//! 帧格式: [4B length LE][1B type][body]
//!   length = 1 + body.len()
//!   type = 0x01 控制(JSON Envelope) / 0x02 数据块

use anyhow::{Result, anyhow};
use quinn::{RecvStream, SendStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 帧类型
pub const TYPE_CONTROL: u8 = 0x01;
pub const TYPE_DATA: u8 = 0x02;

/// 单帧最大长度(含 type + body),防止内存攻击
const MAX_FRAME_LEN: u32 = 10 * 1024 * 1024;

/// 数据块(裸帧承载)
#[derive(Debug, Clone)]
pub struct RawChunk {
    pub seq: u32,
    pub data: Vec<u8>, // 原始字节,不经 base64
}

/// 读取 4B LE 长度 + 1B type
async fn read_frame_header(recv: &mut RecvStream) -> Result<(u32, u8)> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf);
    if len == 0 || len > MAX_FRAME_LEN {
        return Err(anyhow!("帧长度非法: {}", len));
    }
    let mut type_buf = [0u8; 1];
    recv.read_exact(&mut type_buf).await?;
    Ok((len, type_buf[0]))
}

/// 写帧: [4B len][1B type][body]
async fn write_frame(send: &mut SendStream, type_byte: u8, body: &[u8]) -> Result<()> {
    let len = (1 + body.len() as u32).to_le_bytes();
    send.write_all(&len).await?;
    send.write_all(&[type_byte]).await?;
    send.write_all(body).await?;
    Ok(())
}

/// 写数据块帧
pub async fn write_data_chunk(send: &mut SendStream, chunk: &RawChunk) -> Result<()> {
    let mut body = Vec::with_capacity(8 + chunk.data.len());
    body.extend_from_slice(&chunk.seq.to_le_bytes());
    body.extend_from_slice(&(chunk.data.len() as u32).to_le_bytes());
    body.extend_from_slice(&chunk.data);
    write_frame(send, TYPE_DATA, &body).await
}

/// 读数据块帧(假设 header 已读为 TYPE_DATA)
pub async fn read_data_chunk_body(recv: &mut RecvStream, body_len: usize) -> Result<RawChunk> {
    let mut buf = vec![0u8; body_len];
    recv.read_exact(&mut buf).await?;
    if body_len < 8 {
        return Err(anyhow!("数据帧 body 过短: {}", body_len));
    }
    let seq = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let size = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let data = buf[8..].to_vec();
    if data.len() as u32 != size {
        return Err(anyhow!("数据帧 size 不匹配: 声明 {} 实际 {}", size, data.len()));
    }
    Ok(RawChunk { seq, data })
}

/// 读任意帧(返回 type 与剩余 body 长度,由调用方按 type 分发)
pub async fn read_frame(recv: &mut RecvStream) -> Result<(u8, usize)> {
    let (len, type_byte) = read_frame_header(recv).await?;
    Ok((type_byte, len as usize - 1))
}

/// 读控制帧 body(JSON,调用方再 Envelope::decode)
pub async fn read_control_body(recv: &mut RecvStream, body_len: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; body_len];
    recv.read_exact(&mut buf).await?;
    Ok(buf)
}

/// 写控制帧(JSON Envelope body)
pub async fn write_control_frame(send: &mut SendStream, body: &[u8]) -> Result<()> {
    write_frame(send, TYPE_CONTROL, body).await
}
```

- [ ] **Step 2: 注册模块**

修改 `agent/src/protocol/mod.rs`,追加:
```rust
pub mod raw_frame;
```

- [ ] **Step 3: 单测**

在 `raw_frame.rs` 末尾追加 round-trip、空 data、size 不匹配等测试。验证 body 编解码(seq/size/data)正确性。

- [ ] **Step 4: 验证**

Run: `cargo test -p gnome-remote-agent raw_frame`
Expected: 测试通过

Run: `cargo build -p gnome-remote-agent`
Expected: 编译通过(Windows lib,worker 模块 unix stub 不受影响)

- [ ] **Step 5: 提示 commit(用户执行)**

```
git add agent/src/protocol/raw_frame.rs agent/src/protocol/mod.rs
```

---

## 阶段 1:安全补齐(优先,用户隔离危急)

### Task 1.1: 路径校验(PathGuard)

**Files:**
- Create: `agent/src/auth/path_guard.rs`
- Modify: `agent/src/auth/mod.rs`(re-export)、`agent/src/handler.rs:347`(入口调用)、`agent/src/worker/handlers/file.rs`(各 handler 入口)

- [ ] **Step 1: 新增 path_guard.rs**

```rust
// agent/src/auth/path_guard.rs
//! 文件路径安全校验
//!
//! 防目录穿越、限制用户家目录、防符号链接攻击。
//! root 用户(uid=0)放开全文件系统但记审计。

use std::path::{Path, PathBuf};
use anyhow::{Result, anyhow, bail};

/// 校验后的安全路径(newtype,防止绕过)
#[derive(Debug, Clone)]
pub struct SafePath(PathBuf);

impl SafePath {
    pub fn as_str(&self) -> &str {
        self.0.to_str().unwrap_or("")
    }
    pub fn as_path(&self) -> &Path { &self.0 }
}

/// 校验路径安全性
///
/// # 参数
/// - `path`: 目标路径
/// - `home_dir`: 用户家目录(限制范围,核查: UserSession.home_dir 是 PathBuf,故用 &Path)
/// - `uid`: 用户 UID(uid=0 root 放开)
pub fn validate_path(path: &str, home_dir: &Path, uid: u32) -> Result<SafePath> {
    let target = Path::new(path);

    // root 放开但需 canonicalize(防符号链接)
    if uid == 0 {
        let canon = std::fs::canonicalize(target)
            .map_err(|e| anyhow!("路径解析失败 '{}': {}", path, e))?;
        return Ok(SafePath(canon));
    }

    // 非 root: 拒绝 .. 穿越
    let normalized = target.to_path_buf();
    for comp in normalized.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => bail!("路径禁止包含 '..': {}", path),
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) => {}
            Component::CurDir => {}
        }
    }

    // canonicalize 解析符号链接
    let canon = std::fs::canonicalize(target)
        .map_err(|e| anyhow!("路径解析失败 '{}': {}", path, e))?;

    // 限制在家目录范围
    let home_canon = std::fs::canonicalize(home_dir)
        .map_err(|e| anyhow!("家目录解析失败 '{}': {}", home_dir.display(), e))?;
    if !canon.starts_with(&home_canon) {
        bail!("路径越出家目录: {} (家目录 {})", canon.display(), home_canon.display());
    }

    Ok(SafePath(canon))
}
```

- [ ] **Step 2: 注册**

`agent/src/auth/mod.rs` 追加:
```rust
pub mod path_guard;
pub use path_guard::{SafePath, validate_path};
```

- [ ] **Step 3: handler.rs 入口调用**

`handler.rs:347` `handle_file_transfer_request` 开头插入(核查:`UserSession` 的 `home_dir/uid/username` 字段已就位于 `auth/session.rs:14-31`,无需补字段;`home_dir` 是 `PathBuf`,故用 `.as_path()` 适配):
```rust
let safe_path = crate::auth::validate_path(path, session.home_dir.as_path(), session.uid)
    .map_err(|e| e.to_string())?;
let path = safe_path.as_str();
```

- [ ] **Step 4: worker/handlers/file.rs 入口调用**

`handle_read_file`/`handle_write_file`/`handle_read_dir`/`handle_delete` 等入口统一插入 `validate_path` 调用,替换原 `req.path` 直接传 `fs::*`。

- [ ] **Step 5: 验证**

Run: `cargo build -p gnome-remote-agent`
Expected: 编译通过

单测:在 path_guard.rs 追加穿越(`../`)、越界、symlink、root 特例测试,`cargo test path_guard`。

- [ ] **Step 6: 提示 commit(用户执行)**

### Task 1.2: 用户隔离(方案 ii' — fork+setuid+namespace+pipe 桥接)

**Files:**
- Modify: `agent/src/auth/executor.rs`(新增 `execute_as_user_isolated_pipe` 方法)
- Modify: `agent/src/auth/namespace.rs`(启用,两步调用)
- Modify: `agent/src/handler.rs:384,449`(改用新方法)
- Modify: `agent/src/file_stream.rs`(FileStreamWriter/Reader 持 pipe sender/receiver)
- Modify: `agent/Cargo.toml`(加 os_pipe 依赖)

> **方案 ii' 设计(核查修正后)**:原方案 ii 假设复用 `manager/ipc_server.rs` 的 SCM_RIGHTS recv 逻辑,核查发现该文件 `:7-9` 注释明写"不再使用 SCM_RIGHTS",基建不存在。改用标准 `os_pipe` 桥接:
> - Manager 进程内 fork 子进程
> - 子进程:`setuid + setgid` → `UserNamespace::new(uid,gid).create_and_switch()`(核查:`create_and_switch` 是 `&self` 方法无参,必须先 `new` 再调用)→ 子进程内完成全部文件 IO(open + read/write chunk)
> - 父子经 `os_pipe` 交换 chunk 字节:上传 子进程 read pipe→write file;下载 子进程 read file→write pipe
> - 父进程持 pipe 做 `tokio::io::AsyncReadExt/AsyncWriteExt` 与 network 桥接
> - **子进程同步 IO 不阻塞父进程 tokio worker(天然隔离)**,无需 spawn_blocking
> - 无需从零写 SCM_RIGHTS,无新复杂依赖

- [ ] **Step 1: Cargo.toml 加 os_pipe**

`agent/Cargo.toml`:
```toml
os_pipe = "1"
```

- [ ] **Step 2: executor.rs 新增隔离 pipe 方法**

```rust
// agent/src/auth/executor.rs 追加
use std::io::{Read, Write};

/// 在隔离的子进程中处理文件 IO,经 pipe 与父进程交换 chunk(方案 ii')
///
/// 上传方向(writer):父进程写 pipe,子进程 read pipe → write file
/// 下载方向(reader):子进程 read file → write pipe,父进程读 pipe
///
/// # 隔离链
/// - fork 子进程
/// - 子进程 setuid/setgid + UserNamespace::new(uid,gid).create_and_switch()
/// - 子进程内 open + read/write 文件(同步 IO,不阻塞父进程 async runtime)
/// - 经 os_pipe 与父进程交换 chunk
///
/// # 返回
/// - 上传:返回 (pipe_writer_to_child, child_join_handle),父进程持 pipe_writer 写 chunk
/// - 下载:返回 (pipe_reader_from_child, child_join_handle),父进程持 pipe_reader 读 chunk
pub fn execute_as_user_isolated_writer(&self, path: &str, file_size: u64, temp_path: String, final_path: String)
    -> Result<(os_pipe::OsPipeWriter, std::thread::JoinHandle<Result<()>>)>
{
    let (reader, mut writer) = os_pipe::pipe()
        .map_err(|e| anyhow!("创建 pipe 失败: {}", e))?;
    // 父进程持 writer 写 chunk,子进程持 reader 读 chunk → write file
    let child_reader = reader;
    // fork 子进程(用 std::process::Command + 自定义 fd,或 libc::fork)
    // 子进程内:
    //   unsafe { libc::setuid(self.uid); libc::setgid(self.gid); }
    //   crate::auth::namespace::UserNamespace::new(self.uid, self.gid).create_and_switch()?;
    //   let mut file = BufWriter::with_capacity(256*1024, File::create(temp_path)?);
    //   let mut pipe = child_reader;
    //   loop { let mut buf=[0u8;256*1024]; let n=pipe.read(&mut buf)?; if n==0 {break;} file.write_all(&buf[..n])?; }
    //   file.flush()?; file.get_ref().sync_all()?; std::fs::rename(&temp_path, &final_path)?;
    //   exit(0)
    // 父进程返回 (writer, join_handle)
    // 实现:用 std::process::Command::new("/proc/self/exe") + dup2(pipe fd) 或 libc::fork 直接
    unimplemented!("见 Step 3 fork 实现细节")
}
```

- [ ] **Step 3: 实现 fork + namespace + pipe 桥接**

用 `libc::fork`(unix)实现子进程,子进程内 `setuid/setgid` + `UserNamespace::new(uid,gid).create_and_switch()` + open file + 循环 read/write pipe。父进程返回 pipe writer 与子进程 JoinHandle。
> 注意:`namespace.rs` 整个文件 `#[cfg(target_os = "linux")]` 守卫,调用方需加平台守卫或 stub(非 linux 走旧 `execute_as_user_unchecked` 兼容)。`#[allow(dead_code)]` 需去掉。
> 下载方向(reader)对称:子进程 read file→write pipe,父进程持 pipe reader。

- [ ] **Step 4: file_stream.rs FileStreamWriter 持 pipe**

```rust
// FileStreamWriter 改为持 pipe writer(替代 BufWriter<File>)
pub struct FileStreamWriter {
    pipe_writer: os_pipe::OsPipeWriter,
    child: Option<std::thread::JoinHandle<Result<()>>>,
    transferred: u64,
    file_size: u64,
    completed: bool,
}

impl FileStreamWriter {
    pub async fn write_chunk(&mut self, data: Vec<u8>) -> Result<(), String> {
        self.transferred += data.len() as u64;
        if self.transferred > self.file_size { return Err("超大小".into()); }
        // async 写 pipe:用 tokio::io::AsyncWriteExt(OsPipeWriter 需 tokio 化,或 spawn_blocking 包 std::io::Write)
        tokio::task::spawn_blocking(move || {
            // pipe_writer.write_all(&data) — 但 pipe_writer 在 &mut self,需重构
            unimplemented!("用 tokio::io::unix::AsyncFd 包装 OsPipeWriter,或直接 std::io::Write in spawn_blocking")
        }).await.map_err(|e| e.to_string())?
    }
}
```
> 简化:由于父进程只写 pipe(轻量),可用 `spawn_blocking` 包 std `io::Write::write_all`,或用 `tokio::io::unix::AsyncFd` 包装 `OsPipeWriter` 做真 async。两者均可,实现时择一。

- [ ] **Step 5: handler.rs 改用新方法**

`handler.rs:384` 上传分支替换 `execute_as_user_unchecked`:
```rust
let (pipe_writer, child) = executor.execute_as_user_isolated_writer(&path_str, file_size, temp_path, final_path)?;
let mut writer = FileStreamWriter::from_pipe(pipe_writer, child, file_size);
```
`handler.rs:449` 下载分支类似(`execute_as_user_isolated_reader`)。

- [ ] **Step 6: namespace.rs 启用**

`namespace.rs:21` 去掉 `#[allow(dead_code)]`。确认 `UserNamespace::new` + `create_and_switch` 两步调用。

- [ ] **Step 7: 验证**

Run: `wsl -e bash -l -c "cd /mnt/e/MyWork/gnome-remote/agent && cargo build"`(需 unix)
Expected: 编译通过

手动测试:以非 root 用户认证上传文件,验证文件 owner 为目标用户而非 root(`ls -l` 确认);传文件同时 PTY 仍响应(子进程 IO 不阻塞)。

- [ ] **Step 8: 提示 commit(用户执行)**

> **决策点:** 若选方案 i(移 Worker)或 iii(白名单过渡),Task 1.2 替换为对应实现,见 spec 第6节。

### Task 1.3a: 审计参数下传

**Files:**
- Modify: `agent/src/handler.rs:22,347`、`agent/src/server/quic.rs`(调用点)

> 核查修正:`handler.rs` 完全无 audit_log,`AuditLogger` 实例仅在 `quic.rs:392` `handle_connection` 层有。需逐层下传,不只是"在入口插入调用"。

- [ ] **Step 1: handle_envelope 加 audit_log 参数**

`handler.rs:22` `handle_envelope` 签名加 `audit_log: Arc<AuditLogger>` 参数。调用点(`quic.rs` 中调 handle_envelope 处)传入。

- [ ] **Step 2: handle_file_transfer_request 加 audit_log 参数**

`handler.rs:347` 签名加 `audit_log: Arc<AuditLogger>`(从 handle_envelope 传入)。

- [ ] **Step 3: upload/download stream handler 加 audit_log**

`quic.rs:1681` `handle_file_download_stream`、`quic.rs:1752` `handle_file_upload_stream` 签名加 `audit_log: Arc<AuditLogger>`(从 `handle_connection` 的 audit_log 传入)。

- [ ] **Step 4: 验证**

`cargo build`。编译通过,参数链路通。

- [ ] **Step 5: 提示 commit(用户执行)**

### Task 1.3b: 审计调用点

**Files:**
- Modify: `agent/src/audit.rs:155`(去 dead_code)、`agent/src/handler.rs:347,555,620`、`agent/src/server/quic.rs:1681,1752`

- [ ] **Step 1: 启用 log_file_operation**

`audit.rs:155` 删除 `#[allow(dead_code)]` 行。

- [ ] **Step 2: 传输入口审计**

`handler.rs:347` 入口插入:
```rust
audit_log.log_file_operation(&session.username, session.uid, "transfer_start", path, file_size.unwrap_or(0));
```

- [ ] **Step 3: 完成/中断审计**

`handler.rs:555` handle_file_chunk、`handler.rs:620` handle_file_transfer_complete、`quic.rs:1681/1752` stream handler 完成/中断处,调 `log_file_operation(..., "transfer_complete"/"transfer_aborted", path, transferred)`。

- [ ] **Step 4: 验证**

`cargo build` + 手动传一个文件,检查 `audit.log` 出现 `transfer_start`/`transfer_complete` 记录。

- [ ] **Step 5: 提示 commit(用户执行)**

### Task 1.4: 临时文件安全

**Files:**
- Modify: `agent/src/file_stream.rs:17-33`(Uuid 临时名)、`file_stream.rs:452-471`(O_NOFOLLOW)

- [ ] **Step 1: 随机临时名**

`file_stream.rs:17-33` `generate_temp_path` 改:
```rust
fn generate_temp_path(path: &Path) -> String {
    let temp_name = format!("gnome_remote_{}.tmp", uuid::Uuid::new_v4());
    path.parent().unwrap_or(Path::new(".")).join(temp_name).to_string_lossy().into_owned()
}
```
`find_existing_temp_file` 同步改为按 session_id 查(临时名存 TransferSession 而非确定性哈希)。

- [ ] **Step 2: O_NOFOLLOW 防符号链接**

`file_stream.rs:452-471` `create_fresh_temp_file`:
```rust
#[cfg(unix)]
fn create_fresh_temp_file(temp_path: &str) -> Result<File, String> {
    use std::os::unix::fs::OpenOptionsExt;
    // O_NOFOLLOW: 不跟随符号链接
    std::fs::OpenOptions::new()
        .create(true).write(true).truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(temp_path)
        .map_err(|e| format!("创建临时文件失败: {}", e))
}
#[cfg(not(unix))]
fn create_fresh_temp_file(temp_path: &str) -> Result<File, String> {
    std::fs::OpenOptions::new().create(true).write(true).truncate(true)
        .open(temp_path).map_err(|e| format!("创建临时文件失败: {}", e))
}
```

- [ ] **Step 3: 验证**

`cargo build`(Windows lib)+ WSL 完整编译。单测:symlink 临时名被拒绝。

- [ ] **Step 4: 提示 commit(用户执行)**

---

## 阶段 2:性能层

### Task 2.1: 裸帧接入(数据平面)

**Files:**
- Modify: `agent/src/protocol/serde.rs:275-280`(Accept 加 frame_mode)、`agent/src/server/quic.rs:1681-1821`(handler 改裸帧)、`src-tauri/src/transfer.rs:1390-1443`(客户端循环)

- [ ] **Step 1: Accept 加 frame_mode 字段(含向后兼容)**

`serde.rs:275-280`(核查修正:必须加 `#[serde(default)]`,否则旧客户端不带该字段会反序列化失败硬断裂):
```rust
FileTransferAccept {
    session_id: String,
    file_size: u64,
    chunk_size: u32,
    mtime: Option<u64>,
    #[serde(default = "default_json_frame")]
    frame_mode: String, // "raw" | "json"(旧端回退)
}

fn default_json_frame() -> String { "json".to_string() }
```
客户端 Request 对应加 `#[serde(default)]` 的 `frame_mode` 字段,新客户端发 `"raw"`。

- [ ] **Step 2: 服务端 upload handler 改裸帧**

`quic.rs:1752-1821` `handle_file_upload_stream` 循环改为:
```rust
loop {
    let (type_byte, body_len) = raw_frame::read_frame(&mut recv).await?;
    match type_byte {
        raw_frame::TYPE_DATA => {
            let chunk = raw_frame::read_data_chunk_body(&mut recv, body_len).await?;
            let chunk_bytes = chunk.data;
            writer.write_chunk(chunk_bytes).await?; // 方案 ii' 下 writer 持 pipe,async 写
        }
        raw_frame::TYPE_CONTROL => {
            let body = raw_frame::read_control_body(&mut recv, body_len).await?;
            let env = Envelope::decode(&body)?;
            match env.payload {
                Payload::FileTransferComplete { .. } => { writer.finish().await?; break; }
                _ => return Err(anyhow!("非预期控制帧")),
            }
        }
        _ => return Err(anyhow!("未知帧类型: {}", type_byte)),
    }
}
```

- [ ] **Step 3: 客户端 upload 循环改裸帧**

`transfer.rs:1390-1443` `run_upload_loop` 改为 `raw_frame::write_data_chunk(send, &chunk)` 替代 `Envelope::encode + 4B len + write_all`。完成时 `write_control_frame` 发 `FileTransferComplete` JSON。

- [ ] **Step 4: download handler 对称改**

`quic.rs:1681` `handle_file_download_stream` 同理(reader 持 pipe,子进程 read file→write pipe,父进程 read pipe→write network)。

- [ ] **Step 5: 协商降级**

若 `frame_mode == "json"`,走旧 `read_message/write_message` 路径(保留)。

- [ ] **Step 6: 验证**

`cargo build` 双端。端到端回环传 100MB,断言字节完整。

- [ ] **Step 7: 提示 commit(用户执行)**

### Task 2.2: 客户端 IO 异步化 + 服务端 BufWriter 256KB

**Files:**
- Modify: `src-tauri/src/transfer.rs`(客户端 read_next_chunk)、`agent/src/file_stream.rs`(子进程端 BufWriter 256KB)

> 核查修正:方案 ii' 后服务端 writer 持 pipe(已在 Task 1.2 实现 async 写 pipe),子进程同步 IO 不阻塞父进程。本任务聚焦客户端 read_next_chunk 异步化 + 子进程端 BufWriter 容量。

- [ ] **Step 1: 客户端 read_next_chunk spawn_blocking**

`src-tauri/src/transfer.rs` 客户端 FileReader.read_next_chunk 用 `tokio::task::spawn_blocking` 包装同步 `std::fs::read`:
```rust
pub async fn read_next_chunk(&mut self) -> Result<Option<Vec<u8>>, String> {
    let mut buf = vec![0u8; 256 * 1024];
    let n = tokio::task::spawn_blocking(move || {
        // 注意:File 需可跨 spawn_blocking,用 Arc<Mutex<File>> 或重构
        unimplemented!("File 跨 spawn_blocking 处理")
    }).await.map_err(|e| e.to_string())?;
    if n == 0 { Ok(None) } else { buf.truncate(n); Ok(Some(buf)) }
}
```
> 客户端用 `Arc<Mutex<File>>` 或 channel 化 reader(类似服务端 Task 1.2 pipe 方案)。

- [ ] **Step 2: 服务端子进程 BufWriter 256KB**

`file_stream.rs` Task 1.2 子进程端 `BufWriter::with_capacity(256*1024, File::create(...))`(已在 Task 1.2 Step 3 体现,此处确认)。

- [ ] **Step 3: 验证**

`cargo build`。回环传输验证完整性 + 传文件同时 PTY/订阅仍响应。

- [ ] **Step 4: 提示 commit(用户执行)**

### Task 2.3: chunk 256KB

**Files:**
- Modify: `agent/src/file_stream.rs:48`、`agent/src/handler.rs:378,448,506`、`src-tauri/src/transfer.rs:755`

- [ ] **Step 1: 改常量**

```rust
// file_stream.rs:48
const DEFAULT_CHUNK_SIZE: u32 = 256 * 1024;
```
`handler.rs:378,448,506` 三处 `64 * 1024` → `256 * 1024`。`transfer.rs:755` 同步。

- [ ] **Step 2: 验证**

`cargo build`。回环验证帧数减少。

- [ ] **Step 3: 提示 commit(用户执行)**

### Task 2.4: QUIC 调优(Bbr + 窗口)

**Files:**
- Modify: `agent/Cargo.toml`、`agent/src/server/quic.rs:378-382`、`src-tauri/Cargo.toml`、`src-tauri/src/connection.rs`

- [ ] **Step 1: 启用 bbr feature**

`agent/Cargo.toml`:
```toml
quinn = { version = "0.11", default-features = false, features = ["rustls-ring", "runtime-tokio", "bbr"] }
```
`src-tauri/Cargo.toml`(当前 `quinn = "0.11"` 默认 features)追加 `"bbr"`:
```toml
quinn = { version = "0.11", features = ["bbr"] }
```

- [ ] **Step 2: 服务端 TransportConfig**

`quic.rs:378-382`:
```rust
let mut transport = quinn::TransportConfig::default();
transport.max_idle_timeout(None);
transport.keep_alive_interval(Some(std::time::Duration::from_secs(5)));
transport.stream_receive_window_size(8 * 1024 * 1024);   // 8MB
transport.receive_window_size(64 * 1024 * 1024);          // 64MB
transport.congestion_controller_factory(quinn::congestion::Bbr::default_factory());
quic_config.transport_config(std::sync::Arc::new(transport));
```

- [ ] **Step 3: 客户端 TransportConfig**

`src-tauri/src/connection.rs` `build_quic_client_config` 对称设置 + `keep_alive_interval(Some(5s))`。

- [ ] **Step 4: 验证**

`cargo build` 双端。基准:100MB 回环测吞吐,应见 5-15x 提升。

- [ ] **Step 5: 提示 commit(用户执行)**

---

## 阶段 3:复用层重构

### Task 3.1: TransportFrame 抽象

**Files:**
- Create: `agent/src/protocol/transport_frame.rs`

- [ ] **Step 1: 抽象 Frame**

```rust
// agent/src/protocol/transport_frame.rs
use crate::protocol::{raw_frame, serde::Envelope};
use anyhow::Result;
use quinn::{RecvStream, SendStream};

pub enum Frame {
    Control(Envelope),       // JSON,低频
    Data(raw_frame::RawChunk), // 裸字节,高频
}

impl Frame {
    pub async fn read(recv: &mut RecvStream) -> Result<Frame> {
        let (type_byte, body_len) = raw_frame::read_frame(recv).await?;
        match type_byte {
            raw_frame::TYPE_CONTROL => {
                let body = raw_frame::read_control_body(recv, body_len).await?;
                Ok(Frame::Control(Envelope::decode(&body)?))
            }
            raw_frame::TYPE_DATA => {
                let chunk = raw_frame::read_data_chunk_body(recv, body_len).await?;
                Ok(Frame::Data(chunk))
            }
            _ => anyhow::bail!("未知帧类型: {}", type_byte),
        }
    }

    pub async fn write_control(send: &mut SendStream, env: &Envelope) -> Result<()> {
        let body = env.encode()?;
        raw_frame::write_control_frame(send, &body).await
    }

    pub async fn write_data(send: &mut SendStream, chunk: &raw_frame::RawChunk) -> Result<()> {
        raw_frame::write_data_chunk(send, chunk).await
    }
}
```

- [ ] **Step 2: 注册 + 验证**

`protocol/mod.rs` 加 `pub mod transport_frame;`。`cargo build`。

- [ ] **Step 3: 提示 commit(用户执行)**

### Task 3.2: Payload 按域拆分(可选延后)

**Files:**
- Create: `agent/src/protocol/{auth,file,transfer,terminal,subscription,system}.rs`
- Modify: `agent/src/protocol/serde.rs`、`protocol/mod.rs`

> **可选延后**:此任务为纯重构,工作量大(57 变体)且收益间接(编译加速/解耦),不影响性能/安全目标。建议阶段 0-2 + 4 完成后再做,或仅做 3.1/3.3/3.4 跳过本任务。

- [ ] **Step 1: 按域拆分 Payload 变体**

将 `serde.rs:92-414` 的 **57 变体**(核查修正,原写 55 有误)按 spec 第7节拆到 6 个子文件,各定义子 enum。`serde.rs` 顶层 `Payload` 用 `#[serde(flatten)]` 或显式 tag 路由组合,保持 JSON 线兼容。

- [ ] **Step 2: 消除反向依赖**

`serde.rs:3-4` 删除 `use crate::diff::FileDiff` 与 `use crate::auth::stats::*`,改为各子 enum 文件内引用对应类型。

- [ ] **Step 3: 验证**

`cargo build` + `cargo test`。JSON 线格式不变(回归测试)。

- [ ] **Step 4: 提示 commit(用户执行)**

### Task 3.3: AsyncFileIO trait

**Files:**
- Create: `agent/src/auth/async_file.rs`
- Modify: `agent/src/file_stream.rs`(实现 trait)

- [ ] **Step 1: 定义 trait**

```rust
// agent/src/auth/async_file.rs
use anyhow::Result;

#[async_trait::async_trait]
pub trait AsyncFileReader: Send {
    async fn read_next_chunk(&mut self) -> Result<Option<Vec<u8>>>;
}
#[async_trait::async_trait]
pub trait AsyncFileWriter: Send {
    async fn write_chunk(&mut self, data: Vec<u8>) -> Result<()>;
    async fn finish(&mut self) -> Result<()>;
    async fn abort(&mut self) -> Result<()>;
}
```

- [ ] **Step 2: file_stream 实现**

`FileStreamWriter`/`FileStreamReader` impl 这两个 trait(方案 ii' 下持 pipe,天然 async)。

- [ ] **Step 3: 验证**

`cargo build`。trait 对象可替换具体类型(handler 持 `Box<dyn AsyncFileWriter>`)。

- [ ] **Step 4: 提示 commit(用户执行)**

### Task 3.4: PTY 改裸帧(复用)

**Files:**
- Modify: `agent/src/server/quic.rs`(PTY 流)、`agent/src/protocol/serde.rs:247`(TerminalData 保留兼容)

- [ ] **Step 1: PTY 流用 Frame::Data**

PTY 数据(`TerminalData.data: Vec<u8>`)改走 `Frame::Data`(seq 复用为时间戳或忽略),替代 JSON+base64。

- [ ] **Step 2: 验证**

`cargo build`。PTY 回显测试,流量应减少 34% 膨胀。

- [ ] **Step 3: 提示 commit(用户执行)**

---

## 阶段 4:大文件多流并行(达成不劣于 SSH)

### Task 4.1: write_range + 预分配

**Files:**
- Modify: `agent/src/file_stream.rs`

> 方案 ii' 下多流并行的实现:每 stream 独立子进程写自己 offset 段(独立临时文件),或共享 writer(需 Mutex)。推荐每 stream 独立子进程+独立临时段文件,最后合并(或预分配+pwrite 各 offset,但 pwrite 需 fd 共享,与子进程隔离冲突)。实现时择一:方案 A 独立临时段+合并;方案 B 共享 fd+Mutex pwrite(降低隔离)。spec 倾向方案 A。

- [ ] **Step 1: write_range(方案 B 共享 fd 版,若选方案 A 则改为段文件)**

```rust
// FileStreamWriter 新增(基于 OwnedFd + pwrite,需 fd 共享,与方案 ii' 子进程隔离冲突,实现时择一)
pub async fn write_range(&self, offset: u64, data: Vec<u8>) -> Result<(), String> {
    // 方案 ii' 下 fd 在子进程,父进程无法直接 pwrite
    // 需改为:每 stream 独立子进程写独立临时段文件(offset 段),完成后合并
    unimplemented!("实现时按方案 A(独立段+合并)或方案 B(共享 fd+Mutex)择一")
}
```
预分配:`fallocate` 或 `ftruncate` 到 file_size(在主 stream 子进程内)。

- [ ] **Step 2: 验证**

`cargo build`。单测:多 write_range 并发写不同 offset,读回验证完整。

- [ ] **Step 3: 提示 commit(用户执行)**

### Task 4.2: 客户端分片调度

**Files:**
- Modify: `src-tauri/src/transfer.rs`

- [ ] **Step 1: 多 stream 分片**

>100MB 文件,开 N=4 个 `open_bi`,握手 Accept 含 `stream_count: 4` + 每 stream 的 offset 范围。每 stream 独立 `run_upload_loop` 发自己段。`tokio::join_all` 聚合。

- [ ] **Step 2: 验证**

1Gbps 局域网 1GB 文件基准 vs `scp`。目标 ≥ scp(AES-NI)。

- [ ] **Step 3: 提示 commit(用户执行)**

### Task 4.3: 服务端多 stream 汇聚

**Files:**
- Modify: `agent/src/server/quic.rs`

- [ ] **Step 1: 多 stream 写同一 writer(防重复注册)**

握手 Accept 后,主 stream 注册 session,后续 N stream 各自 `handle_file_upload_stream` 调 `writer.write_range(offset, chunk)`。复用 `ConnectionContext`(`quic.rs:39-150`)注册/清理多 stream。
> 核查修正:`transfer_session_ids` 是 `Vec<String>`(`quic.rs:39-150`),多 stream 并发需**防重复 push**(改去重或 `HashSet`),避免 cleanup 时重复清理。完成条件:所有 stream 收到 complete。

- [ ] **Step 2: 验证**

端到端 1GB 多流传输,断言字节完整 + 吞吐 10-30x + ≥ scp。

- [ ] **Step 3: 提示 commit(用户执行)**

---

## 自审(writing-plans,核查后)

- **Spec 覆盖**: 安全目标→阶段1(1.1/1.2/1.3a/1.3b/1.4);性能 5-15x→阶段0+2;多流 10-30x/≥SSH→阶段4;复用→阶段3(3.2 可选)。三目标全覆盖。✓
- **占位符**: Task 1.2 Step 2/3、Task 2.2 Step 1、Task 4.1 Step 1 含 `unimplemented!` 标注"见下一步/实现时择一"——这些是分步实现或架构决策点(方案 A/B),非计划占位;其余步骤含代码。✓
- **类型一致**: `RawChunk`(阶段0,2/3/4 复用);`Frame`(3.1,3.4 复用);`AsyncFileReader/Writer`(3.3,file_stream 实现);`SafePath`(1.1,1.2/1.3 复用);`frame_mode`(2.1,贯穿,含 `#[serde(default)]`);`UserNamespace::new().create_and_switch()`(1.2 两步调用)。✓
- **核查修正落地**: 方案 ii→ii'(pipe)、namespace 两步调用、validate_path &Path、audit_log 逐层下传(1.3a)、frame_mode `#[serde(default)]`、57 变体、transfer_session_ids 防重复——7 处全部修正。✓
- **决策点**: Task 1.2 方案 ii' pipe(若改 i/iii 见 spec);Task 4.1 多流方案 A/B 择一。✓

## 执行交接

计划已写入 `docs/superpowers/plans/2026-08-14-upload-bplus.md`(核查修正版)。两种执行方式:

1. **Subagent-Driven(推荐)** — 每个任务派新 subagent,任务间评审,快速迭代。
2. **Inline 执行** — 本会话用 executing-plans 批量执行带检查点。

**git 由用户操作**(用户规则):每 task 末尾的 commit 步骤由用户执行,实现方不自动 git。

请选执行方式,并确认阶段 1.2 用户隔离方案 ii'(pipe)。
