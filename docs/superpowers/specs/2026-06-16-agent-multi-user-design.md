# Agent 多账号（SSH-like）支持 — 设计文档

> 版本: v1.0 | 日期: 2026-06-16 | 状态: **可行性分析阶段（未实施）**
>
> 核心定位：本工具的初衷是**远程桌面**，多账号 SSH-like 认证是实现「真正的远程桌面体验」的基础能力。

---

## 一、问题陈述

### 1.1 当前架构的局限

当前 Agent 采用**单用户模型**：所有连接者共享同一个 Token、同一个 OS 用户身份、同一个文件系统视图。

```
Client A ──token──→ Agent(root) ──fs::read_dir()──→ /home  (看到所有用户目录)
Client B ──token──→ Agent(root) ──fs::read_dir()──→ /home  (同上)
Client C ──token──→ Agent(root) ──whoami()──────→ "root"   (不是真实身份)
```

**问题清单**：

| # | 问题 | 影响 |
|---|------|------|
| 1 | `whoami` 返回 `root`/agent 用户，非连接者的真实身份 | 终端环境不正确 |
| 2 | 所有连接者看到相同的文件系统视图 | 无隐私隔离 |
| 3 | 无法以普通用户身份执行操作 | 安全风险（所有操作以 root 执行） |
| 4 | 终端将来实现时，bash 环境变量($HOME, $USER)会错误 | 用户体验差 |
| 5 | 无审计日志（不知道谁做了什么） | 违规无法追溯 |

### 1.2 目标：SSH-like 多用户体验

```
Client Alice ─(alice+pass)──→ Agent ──su alice──→ /home/alice   whoami=alice
Client Bob   ─(bob+pass)────→ Agent ──su bob────→ /home/bob     whoami=bob
Client Admin  ─(root+pass)───→ Agent ──su root───→ /root         whoami=root
```

每个连接者：
- 以自己的 Linux 身份执行文件操作
- 终端拥有正确的 `$HOME`、`.bashrc`、权限
- 只能访问自己有权限的路径
- 操作可审计追溯

---

## 二、技术方案

### 2.1 整体架构

```
┌─ 客户端 (Tauri + React) ─────────────────────────────────┐
│                                                          │
│  ServerConfig:                                           │
│    { host, port, token, username, password }              │  ← 新增 username/password
│                                                          │
│  连接流程:                                               │
│    1. QUIC 握手 (TLS 1.3)                                │
│    2. LoginRequest { username, password }                 │  ← 新增认证步骤
│    3. LoginResponse { success, session_id, home_dir }     │
│    4. 后续所有请求携带 session_id                          │
│                                                          │
└──────────────────────┬───────────────────────────────────┘
                       │ QUIC (TLS 加密)
                       ▼
┌─ Agent (Linux) ─────────────────────────────────────────┐
│                                                          │
│  ┌─ 认证层 (PAM) ──────────────────────────────────┐     │
│  │  LoginRequest → pam_authenticate(username, pass) │     │
│  │  成功 → 创建 UserSession { uid, gid, home_dir }  │     │
│  │  失败 → 返回 AuthError                            │     │
│  └──────────────────────────────────────────────────┘     │
│                          │                                │
│                          ▼                                │
│  ┌─ 会话层 (UserSession) ─────────────────────────┐     │
│  │  per-stream user namespace (CLONE_NEWUSER)      │     │
│  │  setuid(uid), setgid(gid)                       │     │
│  │  $HOME, $USER, $LOGNAME 环境变量                │     │
│  │                                                  │     │
│  │  所有 handle_xxx() 在此 session 上下文中执行     │     │
│  └──────────────────────────────────────────────────┘     │
│                          │                                │
│            ┌─────────────┼─────────────┐                  │
│            ▼             ▼             ▼                  │
│  ┌────────────┐ ┌────────────┐ ┌────────────┐           │
│  │ 文件操作    │ │ 终端 (PTY)  │ │ 系统监控    │           │
│  │ fs::* as   │ │ forkpty +  │ │ sysinfo    │           │
│  │ target_user│ │ setuid +   │ │ (系统级，   │           │
│  │            │ │ bash --login│ │  不受影响)  │           │
│  └────────────┘ └────────────┘ └────────────┘           │
└──────────────────────────────────────────────────────────┘
```

### 2.2 方案选型

#### 认证方案：PAM（推荐）

**选择理由**：

| 维度 | PAM | /etc/shadow | SSH 代理 |
|------|-----|-------------|---------|
| 安全性 | 高（系统级安全策略） | 低（需 root + 明文密码） | 最高 |
| 兼容性 | 支持 shadow/LDAP/2FA/OTP | 仅本地用户 | 需要运行 sshd |
| Rust 生态 | `pam` crate / FFI | 自行解析 | russh 库 |
| 部署依赖 | libpam（Linux 标准组件） | 无额外依赖 | sshd 服务 |

**PAM 认证流程**：

```rust
// agent/src/auth/pam.rs（新增模块）
use pam::{Authenticator, PasswordConv};

pub struct PamAuthenticator;

impl PamAuthenticator {
    /// 验证用户名和密码
    /// 成功返回 UserIdentity { uid, gid, home_dir, shell }
    /// 失败返回错误信息
    pub fn authenticate(
        service_name: &str,
        username: &str,
        password: &str,
    ) -> Result<UserIdentity, AuthError> {
        let mut auth = Authenticator::with_password(service_name)?;
        auth.get_username()?.set_username(username)?;
        auth.get_password()?.set_password(password)?;

        match auth.authenticate() {
            Ok(_) => {
                // 从 /etc/passwd 获取 UID/GID/home
                let user_info = getpwnam(username)?;
                Ok(UserIdentity {
                    uid: user_info.uid,
                    gid: user_info.gid,
                    home_dir: user_info.dir,
                    shell: user_info.shell,
                    username: username.to_string(),
                })
            }
            Err(e) => Err(AuthError::AuthenticationFailed(e.to_string())),
        }
    }
}
```

**Fallback 策略**：如果 PAM 不可用（编译时 feature flag 或运行时检测），降级为单用户模式（当前行为）。

#### 权限隔离方案：User Namespace（推荐）

**选择理由**：

| 维度 | User Namespace | 直接 setuid | sudo -u |
|------|---------------|------------|---------|
| 并发安全 | ✅ 每 stream 独立 namespace | ❌ 全局状态竞争 | ✅ 每进程独立 |
| 隔离完整性 | ✅ 完整的 UID/GID 映射 | ⚠️ 仅切换有效 UID | ✅ 进程级隔离 |
| 性能开销 | ~0.5ms/次创建 | ~0 | ~5ms (fork+exec) |
| Root 要求 | 需要 CAP_SYS_ADMIN | 需要 CAP_SETUID | 需要 sudoers 配置 |

**Session 管理**：

```rust
// agent/src/session.rs（新增模块）
use std::os::unix::process::CommandExt;
use std::os::unix::io::AsRawFd;

/// 一个已认证的用户会话，绑定到单个 QUIC Stream
pub struct UserSession {
    pub session_id: String,
    pub username: String,
    pub uid: u32,
    pub gid: u32,
    pub home_dir: PathBuf,
    pub shell: PathBuf,
    created_at: SystemTime,
}

impl UserSession {
    /// 创建新的用户会话（含 user namespace 隔离）
    pub fn new(identity: &UserIdentity) -> Result<Self, SessionError> {
        let session_id = format!("sess-{}", Uuid::new_v4());

        // 方案 A: user namespace（需要 root/CAP_SYS_ADMIN）
        #[cfg(feature = "user_namespace")]
        {
            // 创建 user namespace
            unsafe { libc::unshare(libc::CLONE_NEWUSER) };
            // 写入 uid_map 和 gid_map
            write_uid_gid_maps(identity.uid, identity.gid)?;
            // 切换到目标用户
            unsafe { libc::setuid(identity.uid) };
            unsafe { libc::setgid(identity.gid) };
        }

        // 方案 B: 直接 setuid（fallback，无 namespace）
        #[cfg(not(feature = "user_namespace"))]
        {
            unsafe { libc::setuid(identity.uid) };
            unsafe { libc::setgid(identity.gid) };
        }

        Ok(Self {
            session_id,
            username: identity.username.clone(),
            uid: identity.uid,
            gid: identity.gid,
            home_dir: identity.home_dir.clone(),
            shell: identity.shell.clone(),
            created_at: SystemTime::now(),
        })
    }

    /// 在此 session 上下文中执行文件操作闭包
    /// 确保 setuid 生效后再执行
    pub fn run_as_user<F, T>(&self, f: F) -> Result<T, SessionError>
    where
        F: FnOnce() -> Result<T, String> + Send + 'static,
        T: Send + 'static,
    {
        // TODO: 如果使用 thread-per-session 模型，
        // 每个 session 运行在独立线程中，线程启动时 setuid
        f().map_err(SessionError::OperationFailed)
    }
}
```

#### 终端方案：forkpty + setuid + bash --login

```rust
// agent/src/handler.rs — TerminalSpawnRequest 处理
Payload::TerminalSpawnRequest { shell, cols, rows } => {
    let session = user_session.ok_or("未登录")?;

    use nix::pty::{forkpty, ForkResult};
    use nix::unistd::{setuid, setgid, execvp, setenv};

    match forkpty(Some(&winsize))? {
        ForkResult::Parent { child, .. } => {
            // 父进程：保存 child pid，管理 PTY I/O
            pty_sessions.insert(session_id, child);
            Envelope::new(request_id, Payload::TerminalSpawnResponse {
                session_id: session_id.clone(),
                success: true,
            })
        }
        ForkResult::Child => {
            // 子进程：切换用户身份并启动 shell
            unsafe {
                setuid(Uid::from_raw(session.uid)).unwrap();
                setgid(Gid::from_raw(session.gid)).unwrap();
                setenv("HOME", session.home_dir.to_str().unwrap(), true);
                setenv("USER", &session.username, true);
                setenv("LOGNAME", &session.username, true);
                setenv("SHELL", session.shell.to_str().unwrap(), true);
                setenv("TERM", "xterm-256color", true);

                let shell_c = CString::new(session.shell.to_str().unwrap()).unwrap();
                let login_arg = CString::new("--login").unwrap();
                execvp(&shell_c, &[&shell_c, &login_arg])?;
            }
        }
    }
}
```

### 2.3 协议扩展

```protobuf
// 新增 Payload 类型
message LoginRequest {
    string username = 1;
    string password = 2;  // TLS 加密传输中
}

message LoginResponse {
    bool success = 1;
    optional string error = 2;
    optional string session_id = 3;       // 后续请求携带
    optional string home_dir = 4;          // 客户端用于初始化路径
    optional string username = 5;           // 确认实际登录用户
    optional uint32 uid = 6;
    optional uint32 gid = 7;
}

// 所有现有 Request 新增可选字段
message ReadDirRequest {
    string path = 1;
    string session_id = 2;  // 新增：标识用户会话
}
```

### 2.4 安全模型演进

```
当前:
  Token(连接准入)
    → allowed_paths (全局白名单)
    → blocked_commands (全局黑名单)

目标:
  Token(QUIC 连接准入)
    + PAM(用户身份验证)
      → per-user allowed_paths (默认: /home/{user})
      → per-user blocked_commands (继承全局 + 可覆盖)
      → max_sessions_per_user (防滥用)
      → audit_log (谁/何时/做了什么)
```

**默认 per-user allowed_paths**：

```toml
# agent.toml
[security]
allowed_paths = ["/home", "/etc", "/var/log", "/opt"]  # 全局上限

[security.per_user_defaults]
# 普通用户默认只能访问自己的家目录
restrict_to_home = true          # 默认限制为 /home/{username}
allow_home_only = true            # 只允许 /home/{username} 及子目录

[security.audit]
enabled = true
log_file = "/var/log/gnome-remote/audit.log"
log_operations = ["read_file", "write_file", "delete", "mkdir", "rename", "terminal_spawn"]
```

---

## 三、改动范围评估

### 3.1 Agent 侧（主要工作量）

| 文件/模块 | 改动类型 | 复杂度 | 说明 |
|-----------|---------|--------|------|
| **新增 `auth/pam.rs`** | 新建 | 中 | PAM 认证封装 |
| **新增 `session.rs`** | 新建 | 中高 | UserSession 管理 |
| `config.rs` | 修改 | 低 | AuthConfig 增加 pam/user_namespace 配置 |
| `protocol.rs` | 修改 | 低 | 新增 LoginRequest/Response payload |
| `handler.rs` | **重构** | **高** | 所有 handle 函数接受 `&UserSession` 参数；实现终端 forkpty |
| `server/quic.rs` | 修改 | 中 | 连接建立时增加 login 步骤；per-stream session 管理 |
| `Cargo.toml` | 修改 | 低 | 新增 pam/nix/who 依赖 |

**预估工作量**：Agent 侧约 **3-5 天**

### 3.2 客户端侧（较小工作量）

| 文件/模块 | 改动类型 | 复杂度 | 说明 |
|-----------|---------|--------|------|
| `serversStore.ts` | 修改 | 低 | ServerConfig 新增 username/password(加密存储) |
| `Settings.tsx` | 修改 | 中 | 服务器编辑弹窗增加 用户名/密码 输入框 |
| `ServerManager.tsx` | 修改 | 低 | connectServer 传递 credentials |
| `connection.rs`(Tauri) | 修改 | 低 | remote_connect 命令增加 user/pass 参数 |
| `FileManager.tsx` | **无需改动** | — | 路径已经是 `/home/{username}` 格式 |
| `Terminal.tsx` | **无需改动** | — | PTY 身份由 Agent 侧控制 |
| `SystemMonitor.tsx` | **无需改动** | — | 系统级指标与用户无关 |

**预估工作量**：客户端侧约 **0.5-1 天**

### 3.3 渐进式实施路线

```
Phase 1: 基础认证（1-2天）
  ├─ PAM 认证模块
  ├─ LoginRequest/Response 协议
  ├─ whoami 修正（返回真实用户名）
  └─ 客户端 username/password UI

Phase 2: 文件隔离（1-2天）
  ├─ UserSession 模块
  ├─ handler 函数注入 session
  ├─ per-user allowed_paths
  └─ FileManager 初始化路径自动匹配 /home/{user}

Phase 3: 终端集成（1天）
  ├─ forkpty + setuid + bash --login
  ├─ 正确的环境变量 ($HOME/$USER/$LOGNAME)
  └─ Terminal.tsx 无需改动（Agent 侧透明处理）

Phase 4: 安全加固（1天）
  ├─ user namespace 隔离（feature flag）
  ├─ 审计日志
  └─ per-user 速率限制
```

---

## 四、风险与约束

| 风险 | 等级 | 缓解措施 |
|------|------|---------|
| **Agent 必须以 root/CAP_SETUID 运行** | 高 | 提供 non-root fallback（降级为单用户模式）；文档明确标注 |
| **password 明文传输/存储** | 高 | QUIC TLS 1.3 加密传输；前端用 Tauri 的加密存储 API |
| **setuid 并发安全问题** | 中 | per-stream user namespace 隔离；或 thread-per-session + 启动时 setuid |
| **PAM 依赖** | 低 | Linux 标准组件；feature flag 可禁用 |
| **性能开销** | 低 | user namespace 创建 ~0.5ms；文件操作本身远大于此开销 |
| **兼容性** | 低 | kernel ≥ 3.8 (2013)；主流发行版均满足 |

---

## 五、结论

| 维度 | 结论 |
|------|------|
| **技术可行性** | **完全可行**。Linux 原生能力（PAM + user namespace + setuid）完全支撑 |
| **复杂度** | **中高**。核心工作在 Agent 侧（session 管理 + handler 改造），客户端改动很小 |
| **与现有架构兼容性** | **好**。可做渐进式升级，每 Phase 都能独立交付价值 |
| **对用户体验的提升** | **巨大**。从「远程文件浏览器」变成真正的「远程桌面」——这是工具的核心定位 |
| **优先级建议** | **高**。作为远程桌面工具，多账号是基础能力，应在终端功能实现前完成 |

---

## 附录：参考资源

| 资源 | 用途 |
|------|------|
| Linux PAM | https://www.linux-pam.org/Linux-PAM-html/sys-modules.html |
| `pam` Rust crate | https://crates.io/crates/pam |
| `nix` crate (PTY/unix) | https://crates.io/crates/nix |
| Linux User Namespaces | https://man7.org/man/man7/user_namespaces.7.html |
| `russh` (SSH 库) | https://crates.io/crates/russh |
| GNOME Remote Desktop (GNOME 官方) | https://gitlab.gnome.org/GNOME/gnome-remote-desktop |
