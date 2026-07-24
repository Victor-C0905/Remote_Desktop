# Phase 2: 用户隔离实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** 实现用户隔离机制，确保每个用户会话在独立的上下文中运行，具备正确的权限和安全边界。

**Architecture:** User Namespace隔离每stream，Handler注入UserSession参数，PTY支持用户切换。核心是权限安全和并发安全。

**Tech Stack:** Rust (nix, libc), User Namespace, setuid/setgid

---

## File Structure

### Agent侧新增文件

| 文件路径 | 责任 |
|---------|------|
| `agent/src/auth/namespace.rs` | User Namespace隔离实现 |
| `agent/src/auth/executor.rs` | 用户上下文执行器 |

### Agent侧修改文件

| 文件路径 | 改动内容 |
|---------|---------|
| `agent/src/auth/mod.rs` | 导出namespace和executor模块 |
| `agent/src/handler.rs` | 所有handler函数注入UserSession参数 |
| `agent/src/server/quic.rs` | 添加认证流程，绑定会话到stream |
| `agent/src/pty.rs` | 修改spawn方法支持用户切换 |
| `agent/src/main.rs` | 初始化SessionManager |
| `agent/Cargo.toml` | 添加nix依赖 |

---

## Phase 2任务

### Task 7: 实现User Namespace隔离

**Files:**
- Create: `agent/src/auth/namespace.rs`
- Modify: `agent/Cargo.toml`

- [ ] **Step 1: 添加nix依赖**

修改文件 `agent/Cargo.toml`：

```toml
[dependencies]
nix = { version = "0.29", features = ["sched", "process", "user"] }
```

- [ ] **Step 2: 实现User Namespace隔离**

创建文件 `agent/src/auth/namespace.rs`：

```rust
// agent/src/auth/namespace.rs
// User Namespace隔离实现

use anyhow::Result;
use std::fs;

#[cfg(target_os = "linux")]
use nix::sched::{unshare, CloneFlags};
#[cfg(target_os = "linux")]
use nix::unistd::{setuid, setgid, Uid, Gid};

/// User Namespace管理器
pub struct UserNamespace;

impl UserNamespace {
    /// 创建新的User Namespace并切换到目标用户
    #[cfg(target_os = "linux")]
    pub fn create_and_switch(uid: u32, gid: u32) -> Result<()> {
        // 1. 创建新的User Namespace
        unshare(CloneFlags::CLONE_NEWUSER)?;

        // 2. 配置UID/GID映射
        Self::write_uid_map(uid)?;
        Self::write_gid_map(gid)?;

        // 3. 切换到目标用户
        setuid(Uid::from_raw(uid))?;
        setgid(Gid::from_raw(gid))?;

        tracing::debug!("User Namespace创建成功: uid={}, gid={}", uid, gid);

        Ok(())
    }

    /// 写入UID映射
    /// 格式: <container_uid> <host_uid> <length>
    #[cfg(target_os = "linux")]
    fn write_uid_map(uid: u32) -> Result<()> {
        // 设置deny权限(防止在写入映射前提权)
        Self::write_setgroups_deny()?;

        let content = format!("{} {} 1\n", uid, uid);
        fs::write("/proc/self/uid_map", content)?;

        Ok(())
    }

    /// 写入GID映射
    #[cfg(target_os = "linux")]
    fn write_gid_map(gid: u32) -> Result<()> {
        let content = format!("{} {} 1\n", gid, gid);
        fs::write("/proc/self/gid_map", content)?;

        Ok(())
    }

    /// 禁用setgroups(必须在写入gid_map前)
    #[cfg(target_os = "linux")]
    fn write_setgroups_deny() -> Result<()> {
        fs::write("/proc/self/setgroups", "deny\n")?;
        Ok(())
    }

    /// 非Linux平台的stub实现
    #[cfg(not(target_os = "linux"))]
    pub fn create_and_switch(_uid: u32, _gid: u32) -> Result<()> {
        Err(anyhow::anyhow!("User Namespace仅支持Linux平台"))
    }
}
```

- [ ] **Step 3: 在mod.rs中导出**

修改文件 `agent/src/auth/mod.rs`，添加：

```rust
pub mod namespace;
pub use namespace::UserNamespace;
```

- [ ] **Step 4: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译成功

- [ ] **Step 5: 提交User Namespace实现**

```bash
git add agent/src/auth/namespace.rs
git add agent/src/auth/mod.rs
git add agent/Cargo.toml
git commit -m "feat(agent): 实现User Namespace隔离"
```

---

### Task 8: 实现用户上下文执行器

**Files:**
- Create: `agent/src/auth/executor.rs`
- Modify: `agent/src/auth/mod.rs`

- [ ] **Step 1: 创建用户执行器**

创建文件 `agent/src/auth/executor.rs`：

```rust
// agent/src/auth/executor.rs
// 用户上下文执行器

use super::namespace::UserNamespace;
use super::session::UserSession;
use anyhow::Result;
use std::path::PathBuf;

/// 在指定用户上下文中执行命令
pub struct UserExecutor {
    uid: u32,
    gid: u32,
    home_dir: PathBuf,
}

impl UserExecutor {
    pub fn new(session: &UserSession) -> Self {
        Self {
            uid: session.uid,
            gid: session.gid,
            home_dir: session.home_dir.clone(),
        }
    }

    /// 在用户上下文中执行闭包
    pub fn execute_as_user<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        // 方案1: User Namespace隔离(推荐，Linux)
        #[cfg(target_os = "linux")]
        {
            UserNamespace::create_and_switch(self.uid, self.gid)?;
            return f();
        }

        // 方案2: 直接setuid(Fallback,非Linux)
        #[cfg(not(target_os = "linux"))]
        {
            // Windows/macOS不支持User Namespace，直接执行
            tracing::warn!("User Namespace not supported, executing without user isolation");
            return f();
        }
    }

    /// 在用户上下文中启动进程(用于PTY等)
    #[cfg(unix)]
    pub fn spawn_process(
        &self,
        program: &str,
        args: &[&str],
        working_dir: Option<&PathBuf>,
    ) -> Result<std::process::Child> {
        use std::os::unix::process::CommandExt;
        use std::process::Command;

        let mut cmd = Command::new(program);
        cmd.args(args);

        // 设置用户和组
        cmd.uid(self.uid);
        cmd.gid(self.gid);

        // 设置环境变量
        cmd.env("HOME", &self.home_dir);
        cmd.env("USER", &format!("{}", self.uid));
        cmd.env("LOGNAME", &format!("{}", self.uid));
        cmd.env("TERM", "xterm-256color");

        // 设置工作目录
        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        } else {
            cmd.current_dir(&self.home_dir);
        }

        let child = cmd.spawn()?;

        Ok(child)
    }
}
```

- [ ] **Step 2: 在mod.rs中导出**

修改文件 `agent/src/auth/mod.rs`：

```rust
pub mod executor;
pub use executor::UserExecutor;
```

- [ ] **Step 3: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译成功

- [ ] **Step 4: 提交用户执行器实现**

```bash
git add agent/src/auth/executor.rs
git add agent/src/auth/mod.rs
git commit -m "feat(agent): 实现用户上下文执行器"
```

---

### Task 9: Handler重构（注入UserSession）

**Files:**
- Modify: `agent/src/handler.rs`

- [ ] **Step 1: 修改handle_envelope函数签名**

修改文件 `agent/src/handler.rs`：

```rust
// 原签名
pub async fn handle_envelope(envelope: &Envelope, cfg: &AgentConfig) -> Envelope

// 新签名
pub async fn handle_envelope(
    envelope: &Envelope,
    cfg: &AgentConfig,
    session: &UserSession,  // 新增参数
) -> Envelope
```

- [ ] **Step 2: 在每个handler中注入权限检查**

```rust
pub async fn handle_envelope(
    envelope: &Envelope,
    cfg: &AgentConfig,
    session: &UserSession,
) -> Envelope {
    let executor = UserExecutor::new(session);

    match &envelope.payload {
        Payload::ReadDirRequest { path } => {
            // 检查路径权限
            if !check_path_permission(path, session)? {
                return error_response(envelope.request_id, "权限不足");
            }

            // 在用户上下文中执行
            match executor.execute_as_user(|| handle_read_dir(path, cfg)) {
                Ok(entries) => Envelope::new(
                    envelope.request_id,
                    Payload::ReadDirResponse { path: path.clone(), entries },
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }
        // ... 其他handler类似修改
    }
}

/// 检查路径权限
fn check_path_permission(path: &str, session: &UserSession) -> Result<bool> {
    use std::path::PathBuf;

    let path = PathBuf::from(path);

    // 简单检查：路径必须在家目录下
    if !path.starts_with(&session.home_dir) {
        tracing::warn!(
            "用户 {} 尝试访问未授权路径: {}",
            session.username,
            path.display()
        );
        return Ok(false);
    }

    Ok(true)
}
```

- [ ] **Step 3: 更新所有handler函数**

每个handler函数都需要添加session参数：

```rust
fn handle_read_dir(path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<Vec<FileEntry>> {
    // 实现逻辑
}

fn handle_read_file(path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<(String, u64, u64)> {
    // 实现逻辑
}
```

- [ ] **Step 4: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译成功，可能有类型不匹配的编译错误需要修复

- [ ] **Step 5: 提交Handler重构**

```bash
git add agent/src/handler.rs
git commit -m "refactor(handler): 注入UserSession参数实现权限隔离"
```

---

### Task 10: PTY用户切换支持

**Files:**
- Modify: `agent/src/pty.rs`

- [ ] **Step 1: 修改PtySession::spawn方法**

修改文件 `agent/src/pty.rs`，添加session参数：

```rust
impl PtySession {
    /// 在指定用户上下文中创建PTY会话
    pub fn spawn_as_user(
        shell: &str,
        cols: u16,
        rows: u16,
        working_directory: Option<&str>,
        session: &UserSession,  // 新增参数
    ) -> Result<Self> {
        use nix::pty::{forkpty, Winsize};
        use std::os::fd::IntoRawFd;

        let shell = if shell.is_empty() {
            session.shell.to_string_lossy().to_string()
        } else {
            shell.to_string()
        };

        let winsize = Winsize {
            ws_col: cols,
            ws_row: rows,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let result = unsafe { forkpty(Some(&winsize), None)? };

        match result {
            nix::pty::ForkptyResult::Parent { child, master } => {
                let master_fd = master.into_raw_fd();

                // 设置非阻塞模式
                let flags = nix::fcntl::OFlag::from_bits_truncate(
                    nix::fcntl::fcntl(master_fd, nix::fcntl::FcntlArg::F_GETFL)?
                );
                let new_flags = flags | nix::fcntl::OFlag::O_NONBLOCK;
                nix::fcntl::fcntl(master_fd, nix::fcntl::FcntlArg::F_SETFL(new_flags))?;

                info!(
                    "PTY创建成功: user={}, uid={}, master_fd={}, child_pid={}",
                    session.username, session.uid, master_fd, child
                );

                Ok(Self {
                    master_fd,
                    child_pid: child.as_raw() as i32,
                    cols,
                    rows,
                })
            }
            nix::pty::ForkptyResult::Child => {
                // 子进程：切换用户并启动shell
                unsafe {
                    libc::setgid(session.gid);
                    libc::setuid(session.uid);
                }

                let mut cmd = std::process::Command::new(&shell);

                // 设置工作目录
                let work_dir = if let Some(path) = working_directory {
                    PathBuf::from(path)
                } else {
                    session.home_dir.clone()
                };

                cmd.current_dir(&work_dir);

                // 设置用户环境变量
                cmd.env("HOME", &session.home_dir)
                   .env("USER", &session.username)
                   .env("LOGNAME", &session.username)
                   .env("SHELL", &session.shell)
                   .env("TERM", "xterm-256color")
                   .env("PWD", work_dir.to_string_lossy().to_string());

                // 启动login shell
                cmd.spawn()?;
                std::process::exit(0);
            }
        }
    }
}
```

- [ ] **Step 2: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译成功

- [ ] **Step 3: 提交PTY用户切换**

```bash
git add agent/src/pty.rs
git commit -m "feat(pty): 支持用户切换的PTY会话"
```

---

## 执行建议

Phase 2涉及底层系统调用和Handler重构，建议：
1. 分Task逐个实施
2. 充分测试编译
3. 注意平台兼容性（Linux vs Windows）
4. Handler重构影响较大，需要仔细检查所有调用点

**下一步**: 开始Task 7（User Namespace隔离）