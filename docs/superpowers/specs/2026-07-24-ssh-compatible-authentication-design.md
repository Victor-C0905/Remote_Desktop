# Agent SSH兼容认证系统 — 设计文档

> 版本: v1.0 | 日期: 2026-07-24 | 状态: **设计阶段**
>
> 核心定位：将Agent改造成类似SSH的系统服务，支持开机自启、多用户环境、密码+公钥双认证，最终替代SSH用于远程运维。

---

## 一、问题陈述

### 1.1 当前架构的局限

当前Agent采用**单用户模型**，存在以下问题：

| # | 问题 | 影响 |
|---|------|------|
| 1 | 单Token认证，无法区分用户身份 | 无法实现用户隔离 |
| 2 | Agent以固定用户运行，无法切换用户 | 所有操作以相同权限执行 |
| 3 | 无开机自启机制 | 需要手动启动，不适合生产环境 |
| 4 | 无法替代SSH | 用户需要同时维护SSH和Agent两套系统 |

### 1.2 目标：SSH-like远程运维体验

**核心需求：**
- ✅ systemd开机自启
- ✅ 密码认证(PAM)
- ✅ 公钥认证(兼容SSH authorized_keys)
- ✅ 用户环境隔离(User Namespace)
- ✅ 权限正确切换(setuid/setgid)
- ✅ 操作审计日志

**用户体验目标：**
```
# 像SSH一样使用
$ gnome-remote-client connect --host server --username alice \
    --identity ~/.ssh/id_ed25519

# 终端中自动切换用户
$ whoami
alice

# 只能访问自己有权限的文件
$ cat /home/bob/.bashrc
Permission denied
```

---

## 二、技术方案

### 2.1 方案选型：完整SSH兼容(方案A)

**核心设计：**
- 使用 `russh` 库实现SSH协议兼容
- 直接读取 `~/.ssh/authorized_keys` 进行公钥认证
- PAM进行密码认证
- User Namespace实现用户隔离
- systemd系统服务管理

**选择理由：**

| 维度 | 方案A(SSH兼容) | 方案B(自主实现) | 方案C(渐进式) |
|------|---------------|----------------|--------------|
| SSH兼容性 | ✅ 完全兼容 | ⚠️ 部分兼容 | ❌ 不兼容 |
| 安全性 | ✅ 成熟库保证 | ⚠️ 依赖实现质量 | ✅ 阶段验证 |
| 用户迁移成本 | ✅ 零成本 | ⚠️ 需要配置密钥 | ✅ 渐进迁移 |
| 开发成本 | ⚠️ 初期较高 | ✅ 较低 | ✅ 分摊 |
| 长期维护成本 | ✅ 低 | ⚠️ 中等 | ✅ 低 |

**最终选择：方案A(完整SSH兼容)**

### 2.2 整体架构

```
┌─────────────────────────────────────┐
│   Client (Tauri + React)            │
│                                     │
│  ServerConfig:                      │
│    { host, port, username,          │
│      password/privateKey }          │
│                                     │
│  认证流程:                          │
│    1. QUIC握手(TLS 1.3)             │
│    2. SSH认证(公钥/密码)            │
│    3. 获取session_id                │
└──────────────┬──────────────────────┘
               │ QUIC (TLS加密)
               ▼
┌─────────────────────────────────────┐
│   Agent (systemd, root)             │
│                                     │
│  ┌────────────────────────────┐    │
│  │   SSH兼容认证层(russh)      │    │
│  │  - 密码认证(PAM)            │    │
│  │  - 公钥认证(authorized_keys) │    │
│  └───────────┬────────────────┘    │
│              │                      │
│              ▼                      │
│  ┌────────────────────────────┐    │
│  │   会话管理       │    │
│  │  - UserSession创建         │    │
│  │  - User Namespace隔离      │    │
│  │  - setuid/setgid切换       │    │
│  └───────────┬────────────────┘    │
│              │                      │
│    ┌─────────┴──────────┐          │
│    ▼                     ▼          │
│ ┌──────────┐      ┌──────────┐    │
│ │文件操作   │      │ 终端(PTY) │    │
│ │as user   │      │ as user  │    │
│ └──────────┘      └──────────┘    │
└─────────────────────────────────────┘
```

### 2.3 认证流程

```
客户端连接流程:
1. QUIC握手(TLS 1.3)
   ↓
2. SSH兼容认证(russh)
   ├─ 尝试公钥认证
   │  └─ 查找 ~/.ssh/authorized_keys
   │     └─ 验证签名
   │
   └─ 失败则尝试密码认证
      └─ PAM认证
         └─ 检查 /etc/shadow
            └─ 成功:返回UserIdentity
   ↓
3. 创建UserSession
   ├─ unshare(CLONE_NEWUSER)
   ├─ 写入uid_map/gid_map
   └─ setuid/setgid到目标用户
   ↓
4. 后续操作在UserSession上下文中执行
```

---

## 三、认证层设计

### 3.1 SSH公钥认证

**核心实现：**

```rust
// agent/src/auth/ssh.rs
pub struct SshAuthenticator {
    authorized_keys_template: String,
}

impl Authenticator for SshAuthenticator {
    fn authenticate_pubkey(
        &self,
        username: &str,
        pubkey: &PublicKey,
        signature: Option<&[u8]>,
    ) -> Result<AuthResult> {
        // 1. 加载用户的authorized_keys
        let authorized_keys = self.load_authorized_keys(username)?;

        // 2. 查找匹配的公钥
        let found = authorized_keys.iter().any(|key| {
            key.to_bytes() == pubkey.to_bytes()
        });

        if !found {
            return Ok(AuthResult::Failure("公钥未授权".into()));
        }

        // 3. 验证签名(防止重放攻击)
        if let Some(sig) = signature {
            if !verify_signature(pubkey, sig)? {
                return Ok(AuthResult::Failure("签名验证失败".into()));
            }
        }

        // 4. 从/etc/passwd获取用户信息
        let user_info = get_user_info(username)?;

        Ok(AuthResult::Success(UserIdentity {
            username: username.to_string(),
            uid: user_info.uid,
            gid: user_info.gid,
            home_dir: user_info.home_dir,
            shell: user_info.shell,
        }))
    }
}
```

**authorized_keys路径配置：**

```toml
# 直接读取用户SSH密钥(方案1)
[auth.ssh]
authorized_keys_path = "/home/{username}/.ssh/authorized_keys"
```

**权限处理：**
- ✅ root进程可以读取任何用户的authorized_keys
- ⚠️ SELinux环境需要配置策略或使用专用密钥目录
- ✅ 自动处理文件不存在、用户目录不存在等边缘情况

### 3.2 PAM密码认证

```rust
// agent/src/auth/pam.rs
pub struct PamAuthenticator {
    pam_service: String,
}

impl Authenticator for PamAuthenticator {
    fn authenticate_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<AuthResult> {
        // 创建PAM认证器
        let mut auth = Authenticator::with_password(&self.pam_service)?;

        // 设置用户名和密码
        auth.get_username()?.set_username(username)?;
        auth.get_password()?.set_password(password)?;

        // 执行认证
        match auth.authenticate() {
            Ok(_) => {
                let user_info = get_user_info(username)?;
                Ok(AuthResult::Success(UserIdentity {
                    username: username.to_string(),
                    uid: user_info.uid,
                    gid: user_info.gid,
                    home_dir: user_info.home_dir,
                    shell: user_info.shell,
                }))
            }
            Err(e) => {
                Ok(AuthResult::Failure("用户名或密码错误".into()))
            }
        }
    }
}
```

**PAM配置文件：**

```bash
# /etc/pam.d/gnome-remote
auth        required      pam_unix.so nullok
account     required      pam_unix.so
session     required      pam_unix.so
```

### 3.3 组合认证器

```rust
// agent/src/auth/mod.rs
pub struct CompositeAuthenticator {
    ssh_auth: Arc<SshAuthenticator>,
    pam_auth: Arc<PamAuthenticator>,
    enable_pubkey: bool,
    enable_password: bool,
}

impl Authenticator for CompositeAuthenticator {
    fn authenticate_pubkey(&self, username: &str, pubkey: &PublicKey, signature: Option<&[u8]>) -> Result<AuthResult> {
        if !self.enable_pubkey {
            return Ok(AuthResult::Failure("公钥认证未启用".into()));
        }
        self.ssh_auth.authenticate_pubkey(username, pubkey, signature)
    }

    fn authenticate_password(&self, username: &str, password: &str) -> Result<AuthResult> {
        if !self.enable_password {
            return Ok(AuthResult::Failure("密码认证未启用".into()));
        }
        self.pam_auth.authenticate_password(username, password)
    }
}
```

---

## 四、用户隔离设计

### 4.1 UserSession管理

```rust
// agent/src/auth/session.rs
pub struct UserSession {
    pub session_id: String,
    pub username: String,
    pub uid: u32,
    pub gid: u32,
    pub home_dir: PathBuf,
    pub shell: PathBuf,
    pub allowed_paths: Vec<PathBuf>,
    pub created_at: SystemTime,
}

impl SessionManager {
    pub async fn create_session(&self, identity: UserIdentity, cfg: &AgentConfig) -> Result<UserSession> {
        // 1. 计算用户允许的路径
        let allowed_paths = self.calculate_allowed_paths(&identity, cfg)?;

        // 2. 创建会话
        let session = UserSession {
            session_id: format!("sess-{}", Uuid::new_v4()),
            username: identity.username,
            uid: identity.uid,
            gid: identity.gid,
            home_dir: identity.home_dir,
            shell: identity.shell,
            allowed_paths,
            created_at: SystemTime::now(),
        };

        // 3. 存储会话
        self.sessions.write().await.insert(session.session_id.clone(), session.clone());

        Ok(session)
    }

    fn calculate_allowed_paths(&self, identity: &UserIdentity, cfg: &AgentConfig) -> Result<Vec<PathBuf>> {
        let mut paths = vec![identity.home_dir.clone()];

        // 添加全局允许路径(验证用户权限)
        for path_template in &cfg.security.global_allowed_paths {
            let path = PathBuf::from(path_template);
            if self.check_path_permission(&path, identity.uid, identity.gid)? {
                paths.push(path);
            }
        }

        Ok(paths)
    }
}
```

### 4.2 User Namespace隔离

```rust
// agent/src/auth/namespace.rs
pub struct UserNamespace;

impl UserNamespace {
    pub fn create_and_switch(uid: u32, gid: u32) -> Result<()> {
        // 1. 创建新的User Namespace
        unshare(CloneFlags::CLONE_NEWUSER)?;

        // 2. 配置UID/GID映射
        Self::write_uid_map(uid)?;
        Self::write_gid_map(gid)?;

        // 3. 切换到目标用户
        setuid(Uid::from_raw(uid))?;
        setgid(Gid::from_raw(gid))?;

        Ok(())
    }

    fn write_uid_map(uid: u32) -> Result<()> {
        let content = format!("{} {} 1\n", uid, uid);
        Self::write_setgroups_deny()?;
        std::fs::write("/proc/self/uid_map", content)?;
        Ok(())
    }

    fn write_gid_map(gid: u32) -> Result<()> {
        let content = format!("{} {} 1\n", gid, gid);
        std::fs::write("/proc/self/gid_map", content)?;
        Ok(())
    }

    fn write_setgroups_deny() -> Result<()> {
        std::fs::write("/proc/self/setgroups", "deny\n")?;
        Ok(())
    }
}
```

**User Namespace优势：**
- ✅ 每 stream 独立 namespace，避免并发setuid竞争
- ✅ 完整的 UID/GID 映射，隔离更彻底
- ⚠️ 需要 CAP_SYS_ADMIN 权限
- ⚠️ 性能开销约 0.5ms/次创建

### 4.3 Handler集成

```rust
// agent/src/handler.rs
pub async fn handle_envelope(
    envelope: &Envelope,
    cfg: &AgentConfig,
    session: &UserSession,
) -> Envelope {
    let executor = UserExecutor::new(session.uid, session.gid, session.home_dir.clone());

    match &envelope.payload {
        Payload::ReadDirRequest { path } => {
            // 检查路径权限
            if !check_path_permission(path, session)? {
                return error_response(envelope.request_id, "权限不足");
            }

            // 在用户上下文中执行
            match executor.execute_as_user(|| handle_read_dir(path, cfg, session)) {
                Ok(entries) => Envelope::new(
                    envelope.request_id,
                    Payload::ReadDirResponse { path: path.clone(), entries },
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }
        // ... 其他handler
    }
}
```

### 4.4 PTY用户切换

```rust
// agent/src/pty.rs
impl PtySession {
    pub fn spawn_as_user(shell: &str, cols: u16, rows: u16, working_directory: Option<&str>, session: &UserSession) -> Result<Self> {
        // ... forkpty逻辑 ...

        match result {
            ForkptyResult::Parent { child, master } => {
                // 父进程：管理PTY I/O
                Ok(Self { master_fd, child_pid, cols, rows })
            }
            ForkptyResult::Child => {
                // 子进程：切换用户并启动shell
                unsafe {
                    libc::setgid(session.gid);
                    libc::setuid(session.uid);
                }

                let mut cmd = std::process::Command::new(&shell);
                cmd.current_dir(&session.home_dir)
                   .env("HOME", &session.home_dir)
                   .env("USER", &session.username)
                   .env("LOGNAME", &session.username)
                   .env("SHELL", &session.shell)
                   .env("TERM", "xterm-256color");

                // 启动login shell
                CommandExt::exec(&mut cmd);
            }
        }
    }
}
```

---

## 五、systemd服务集成

### 5.1 服务单元配置

```ini
# /etc/systemd/system/gnome-remote-agent.service
[Unit]
Description=GNOME Remote Agent - SSH-like Remote Management
After=network.target network-online.target
Wants=network-online.target

[Service]
Type=notify
User=root
Group=root

ExecStart=/usr/local/bin/gnome-remote-agent \
    --config /etc/gnome-remote/agent.toml \
    --log-dir /var/log/gnome-remote

Restart=on-failure
RestartSec=5s

# 安全配置
NoNewPrivileges=false
CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_SYS_ADMIN CAP_NET_BIND_SERVICE

# 资源限制
LimitNOFILE=65536

# 日志
StandardOutput=journal
StandardError=journal
SyslogIdentifier=gnome-remote-agent

[Install]
WantedBy=multi-user.target
```

### 5.2 配置文件

```toml
# /etc/gnome-remote/agent.toml
[server]
bind = "0.0.0.0"
quic_port = 8443
cert_path = "/etc/gnome-remote/cert.pem"
key_path = "/etc/gnome-remote/key.pem"

[auth]
mode = "ssh-compatible"

[auth.ssh]
enable_pubkey = true
authorized_keys_path = "/home/{username}/.ssh/authorized_keys"
enable_password = true
pam_service = "gnome-remote"

[security]
global_allowed_paths = ["/home", "/etc", "/var/log", "/opt"]
restrict_to_home = true

[audit]
enabled = true
log_file = "/var/log/gnome-remote/audit.log"
log_operations = ["auth_success", "auth_failure", "read_file", "write_file", "delete", "terminal_spawn"]

[limits]
max_terminal_sessions = 10
connection_idle_timeout_secs = 300
```

---

## 六、改动范围评估

### 6.1 Agent侧改动

| 类别 | 文件/模块 | 工作量 | 复杂度 |
|------|----------|--------|--------|
| **新增** | src/auth/mod.rs | 0.5天 | 低 |
| **新增** | src/auth/ssh.rs | 2天 | 中高 |
| **新增** | src/auth/pam.rs | 1天 | 中 |
| **新增** | src/auth/session.rs | 1天 | 中 |
| **新增** | src/auth/namespace.rs | 1天 | 中 |
| **新增** | src/auth/executor.rs | 1天 | 中 |
| **新增** | src/audit.rs | 0.5天 | 低 |
| **修改** | src/main.rs | 0.5天 | 低 |
| **修改** | src/config.rs | 0.5天 | 低 |
| **重构** | src/handler.rs | 2天 | 高 |
| **修改** | src/server/quic.rs | 1天 | 中 |
| **修改** | src/pty.rs | 0.5天 | 中 |

**Agent侧总计：11天**

### 6.2 客户端侧改动

| 文件 | 改动内容 | 工作量 |
|------|---------|--------|
| src/stores/serversStore.ts | 添加认证字段 | 0.5天 |
| src/apps/Settings.tsx | 认证配置UI | 1天 |
| src/context/ServerManager.tsx | 连接逻辑 | 0.5天 |
| src-tauri/src/connection.rs | 凭据传递 | 0.5天 |

**客户端侧总计：2.5天**

### 6.3 系统配置

| 配置文件 | 说明 |
|---------|------|
| systemd服务单元 | 开机自启配置 |
| PAM配置 | 密码认证支持 |
| 安装脚本 | 自动化部署 |

**系统配置总计：1天**

### 6.4 依赖库添加

```toml
# Cargo.toml
[dependencies]
russh = "0.42"
russh-keys = "0.42"
pam = "0.8"
nix = { version = "0.29", features = ["sched", "process"] }
systemd = "0.10"  # 可选
```

**预估新增体积：2-3MB**

---

## 七、实施路线

### 7.1 渐进式实施

```
Phase 1: 认证层 (第1周)
├─ PAM认证模块 (Day 1-2)
├─ SSH公钥认证模块 (Day 3-4)
├─ 协议层集成 (Day 5)
└─ 客户端认证UI (Day 5)

Phase 2: 用户隔离 (第2周)
├─ UserSession模块 (Day 6)
├─ User Namespace隔离 (Day 7)
├─ Handler重构 (Day 8-9)
└─ PTY用户切换 (Day 10)

Phase 3: 安全加固 (第3周)
├─ 审计日志 (Day 11)
├─ systemd服务配置 (Day 12)
└─ 集成测试 (Day 13)
```

### 7.2 测试计划

**单元测试：**
- authorized_keys解析测试
- 用户权限检查测试
- User Namespace隔离测试

**集成测试：**
- 密码认证流程测试
- 公钥认证流程测试
- 权限隔离测试(访问其他用户文件应失败)
- 终端用户切换测试(whoami应返回正确用户)

---

## 八、安全考虑

### 8.1 安全模型

```
当前:
  Token(连接准入)
    → allowed_paths (全局白名单)

目标:
  QUIC连接(TLS 1.3加密)
    + SSH认证(公钥/密码)
      → per-user allowed_paths
      → per-user 文件系统权限
      → audit_log(审计追踪)
```

### 8.2 安全加固措施

| 措施 | 实现方式 |
|------|---------|
| 密码加密存储 | 客户端使用Tauri加密存储API |
| 防重放攻击 | 公钥认证验证签名 |
| 并发安全 | User Namespace隔离，避免setuid竞争 |
| 操作审计 | 记录所有敏感操作到audit.log |
| 错误脱敏 | 生产环境不泄露敏感路径信息 |
| 权限最小化 | 默认限制到家目录 |

### 8.3 权限需求

**Agent运行权限：**
- ✅ 以root运行，需要CAP_SETUID/CAP_SETGID/CAP_SYS_ADMIN
- ✅ 可以切换到任意用户
- ✅ 可以读取任意用户的authorized_keys

**客户端权限：**
- ✅ 普通用户权限即可
- ✅ 只能访问自己有权限的文件
- ✅ 受Linux文件系统权限控制

---

## 九、风险与缓解

| 风险 | 等级 | 缓解措施 |
|------|------|---------|
| russh库不成熟 | 中 | 准备Fallback方案(自己解析authorized_keys) |
| User Namespace性能开销 | 低 | 测量性能，如果>10ms则考虑直接setuid |
| SELinux/AppArmor限制 | 中 | 提供SELinux策略文档，使用方案1 |
| 并发安全问题 | 高 | User Namespace隔离，避免全局setuid竞争 |
| 密码明文存储 | 高 | 客户端使用Tauri加密存储API |
| Handler重构影响现有功能 | 中 | 编写完整回归测试，分阶段重构 |

---

## 十、总结

### 10.1 技术可行性

- ✅ Linux原生能力完全支撑(PAM + User Namespace + setuid)
- ✅ russh库成熟度足够，有活跃维护
- ✅ systemd集成简单直接
- ✅ 客户端改动较小

### 10.2 复杂度评估

- **中高复杂度**：核心工作在Agent侧(session管理 + handler改造)
- **客户端改动小**：主要是认证UI和凭据传递
- **可渐进式实施**：每个Phase都能独立交付价值

### 10.3 对用户体验的提升

- **巨大**：从「远程文件浏览器」变成真正的「SSH-like远程运维工具」
- **零迁移成本**：用户可以直接使用现有SSH密钥
- **无缝替代SSH**：认证体验与SSH完全一致

### 10.4 实施建议

- **优先级：高**。作为远程运维工具，多用户认证是基础能力
- **实施方式：渐进式**。分3个Phase，每个Phase都能独立验证
- **风险控制：充分测试**。特别是权限隔离和并发安全

---

## 附录：参考资源

| 资源 | 用途 |
|------|------|
| russh crate | https://crates.io/crates/russh |
| russh-keys crate | https://crates.io/crates/russh-keys |
| pam crate | https://crates.io/crates/pam |
| Linux User Namespaces | https://man7.org/man/man7/user_namespaces.7.html |
| systemd.service | https://www.freedesktop.org/software/systemd/man/systemd.service.html |
| Linux PAM | https://www.linux-pam.org/Linux-PAM-html/ |