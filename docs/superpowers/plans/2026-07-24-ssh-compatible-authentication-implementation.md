# Agent SSH兼容认证系统 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现SSH兼容的多用户认证系统，支持systemd开机自启、密码+公钥双认证、用户环境隔离，最终替代SSH用于远程运维。

**Architecture:** 使用russh库实现SSH协议兼容，PAM进行密码认证，User Namespace实现用户隔离，systemd管理服务进程。核心是Agent侧的认证层和用户隔离层，客户端改动较小。

**Tech Stack:** Rust (russh, russh-keys, pam, nix), systemd, PAM, QUIC

---

## File Structure

### Agent侧新增文件

| 文件路径 | 责任 |
|---------|------|
| `agent/src/auth/mod.rs` | 认证模块入口，定义Authenticator trait |
| `agent/src/auth/ssh.rs` | SSH公钥认证实现，读取authorized_keys |
| `agent/src/auth/pam.rs` | PAM密码认证实现 |
| `agent/src/auth/session.rs` | UserSession管理和路径权限检查 |
| `agent/src/auth/namespace.rs` | User Namespace隔离实现 |
| `agent/src/auth/executor.rs` | 用户上下文执行器 |
| `agent/src/audit.rs` | 审计日志记录 |

### Agent侧修改文件

| 文件路径 | 改动内容 |
|---------|---------|
| `agent/src/main.rs` | 初始化认证模块，添加systemd notify支持 |
| `agent/src/config.rs` | 添加SSH/PAM/审计配置结构 |
| `agent/src/handler.rs` | 所有handler函数注入UserSession参数 |
| `agent/src/server/quic.rs` | 添加认证流程，绑定会话到stream |
| `agent/src/pty.rs` | 修改spawn方法支持用户切换 |
| `agent/Cargo.toml` | 添加russh, pam等依赖 |

### 客户端侧修改文件

| 文件路径 | 改动内容 |
|---------|---------|
| `src/stores/serversStore.ts` | 添加username/password/privateKey字段 |
| `src/apps/Settings.tsx` | 服务器配置UI增加认证字段 |
| `src/context/ServerManager.tsx` | 修改连接逻辑传递凭据 |
| `src-tauri/src/connection.rs` | connect命令接受凭据参数 |
| `src-tauri/Cargo.toml` | 添加russh依赖 |

### 测试文件

| 文件路径 | 测试内容 |
|---------|---------|
| `agent/tests/auth_test.rs` | 认证模块单元测试 |
| `agent/tests/namespace_test.rs` | User Namespace测试 |
| `tests/integration_test.sh` | 集成测试脚本 |

### 系统配置文件

| 文件路径 | 用途 |
|---------|------|
| `systemd/gnome-remote-agent.service` | systemd服务单元 |
| `pam.d/gnome-remote` | PAM配置文件 |
| `install.sh` | 自动安装脚本 |

---

## Phase 1: 认证层实现 (第1周)

### Task 1: 创建认证模块基础结构

**Files:**
- Create: `agent/src/auth/mod.rs`
- Create: `agent/src/auth/ssh.rs` (stub)
- Create: `agent/src/auth/pam.rs` (stub)
- Modify: `agent/src/lib.rs`

- [ ] **Step 1: 创建auth模块目录**

```bash
cd e:/MyWork/gnome-remote/agent
mkdir -p src/auth
```

- [ ] **Step 2: 创建认证模块入口文件**

创建文件 `agent/src/auth/mod.rs`:

```rust
// agent/src/auth/mod.rs
// SSH兼容认证模块

pub mod ssh;
pub mod pam;
pub mod session;
pub mod namespace;
pub mod executor;

use anyhow::Result;
use std::path::PathBuf;

/// 用户身份信息
#[derive(Debug, Clone)]
pub struct UserIdentity {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
    pub home_dir: PathBuf,
    pub shell: PathBuf,
}

/// 认证结果
#[derive(Debug)]
pub enum AuthResult {
    Success(UserIdentity),
    Failure(String),
    PartialSuccess {
        methods: Vec<String>,
    },
}

/// 认证器Trait
pub trait Authenticator: Send + Sync {
    /// 尝试公钥认证
    fn authenticate_pubkey(
        &self,
        username: &str,
        pubkey: &[u8],
        signature: Option<&[u8]>,
    ) -> Result<AuthResult>;

    /// 尝试密码认证
    fn authenticate_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<AuthResult>;
}

/// 从/etc/passwd获取用户信息
pub fn get_user_info(username: &str) -> Result<UserIdentity> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let file = File::open("/etc/passwd")?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        let parts: Vec<&str> = line.split(':').collect();

        if parts.len() >= 7 && parts[0] == username {
            return Ok(UserIdentity {
                username: parts[0].to_string(),
                uid: parts[2].parse()?,
                gid: parts[3].parse()?,
                home_dir: PathBuf::from(parts[5]),
                shell: PathBuf::from(parts[6]),
            });
        }
    }

    Err(anyhow::anyhow!("用户不存在: {}", username))
}
```

- [ ] **Step 3: 创建SSH认证模块stub**

创建文件 `agent/src/auth/ssh.rs`:

```rust
// agent/src/auth/ssh.rs
// SSH公钥认证实现

use super::{AuthResult, Authenticator, UserIdentity, get_user_info};
use anyhow::Result;

pub struct SshAuthenticator {
    authorized_keys_template: String,
}

impl SshAuthenticator {
    pub fn new(authorized_keys_template: String) -> Self {
        Self {
            authorized_keys_template,
        }
    }
}

impl Authenticator for SshAuthenticator {
    fn authenticate_pubkey(
        &self,
        username: &str,
        pubkey: &[u8],
        signature: Option<&[u8]>,
    ) -> Result<AuthResult> {
        // TODO: 实现公钥认证
        Ok(AuthResult::Failure("Not implemented".into()))
    }

    fn authenticate_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<AuthResult> {
        // SSH认证器不支持密码认证
        Ok(AuthResult::Failure("SSH authenticator does not support password".into()))
    }
}
```

- [ ] **Step 4: 创建PAM认证模块stub**

创建文件 `agent/src/auth/pam.rs`:

```rust
// agent/src/auth/pam.rs
// PAM密码认证实现

use super::{AuthResult, Authenticator};
use anyhow::Result;

pub struct PamAuthenticator {
    pam_service: String,
}

impl PamAuthenticator {
    pub fn new(pam_service: String) -> Self {
        Self { pam_service }
    }
}

impl Authenticator for PamAuthenticator {
    fn authenticate_pubkey(
        &self,
        username: &str,
        pubkey: &[u8],
        signature: Option<&[u8]>,
    ) -> Result<AuthResult> {
        // PAM不支持公钥认证
        Ok(AuthResult::Failure("PAM does not support pubkey".into()))
    }

    fn authenticate_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<AuthResult> {
        // TODO: 实现PAM认证
        Ok(AuthResult::Failure("Not implemented".into()))
    }
}
```

- [ ] **Step 5: 在main.rs中引入auth模块**

修改文件 `agent/src/main.rs`，在第7行后添加:

```rust
mod auth;
```

- [ ] **Step 6: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译成功，可能有未使用的import警告

- [ ] **Step 7: 提交基础结构**

```bash
git add agent/src/auth/
git add agent/src/main.rs
git commit -m "feat(agent): 添加SSH兼容认证模块基础结构"
```

---

### Task 2: 实现SSH公钥认证

**Files:**
- Modify: `agent/src/auth/ssh.rs`
- Modify: `agent/Cargo.toml`
- Create: `agent/tests/auth_test.rs`

- [ ] **Step 1: 添加russh依赖**

修改文件 `agent/Cargo.toml`，在`[dependencies]`部分添加:

```toml
# SSH协议栈(新增)
russh = "0.42"
russh-keys = "0.42"
```

- [ ] **Step 2: 编写authorized_keys解析测试**

创建文件 `agent/tests/auth_test.rs`:

```rust
// agent/tests/auth_test.rs
use std::fs;
use tempfile::TempDir;

#[test]
fn test_parse_authorized_keys() {
    // 创建临时目录和文件
    let temp_dir = TempDir::new().unwrap();
    let ssh_dir = temp_dir.path().join(".ssh");
    fs::create_dir(&ssh_dir).unwrap();

    let authorized_keys = ssh_dir.join("authorized_keys");

    // 写入测试密钥
    let content = r#"# SSH authorized_keys test
ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzVrQRMlZ6VrTlL3ZqXJ9Hw test@host
ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQD test2@host
"#;
    fs::write(&authorized_keys, content).unwrap();

    // 验证文件存在
    assert!(authorized_keys.exists());

    // TODO: 调用实际的解析函数测试
}
```

- [ ] **Step 3: 运行测试验证**

```bash
cd agent
cargo test test_parse_authorized_keys -- --nocapture
```

Expected: 测试通过

- [ ] **Step 4: 实现authorized_keys加载函数**

修改文件 `agent/src/auth/ssh.rs`:

```rust
// agent/src/auth/ssh.rs
use super::{AuthResult, Authenticator, UserIdentity, get_user_info};
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub struct SshAuthenticator {
    authorized_keys_template: String,
}

impl SshAuthenticator {
    pub fn new(authorized_keys_template: String) -> Self {
        Self {
            authorized_keys_template,
        }
    }

    /// 获取用户的authorized_keys路径
    fn get_authorized_keys_path(&self, username: &str) -> PathBuf {
        PathBuf::from(
            self.authorized_keys_template.replace("{username}", username)
        )
    }

    /// 加载用户的authorized_keys
    fn load_authorized_keys(&self, username: &str) -> Result<Vec<Vec<u8>>> {
        let path = self.get_authorized_keys_path(username);

        if !path.exists() {
            tracing::debug!("用户 {} 没有authorized_keys文件", username);
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&path)?;
        let keys = self.parse_authorized_keys(&content)?;

        Ok(keys)
    }

    /// 解析authorized_keys文件内容(OpenSSH格式)
    fn parse_authorized_keys(&self, content: &str) -> Result<Vec<Vec<u8>>> {
        let mut keys = Vec::new();

        for line in content.lines() {
            let line = line.trim();

            // 跳过空行和注释
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // 简单解析: 提取base64部分
            // 格式: key-type base64-key comment
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                // 将base64解码为bytes(这里简化处理,实际需要完整解析)
                // TODO: 使用russh-keys进行完整解析
                if let Ok(decoded) = base64_decode(parts[1]) {
                    keys.push(decoded);
                }
            }
        }

        Ok(keys)
    }
}

// 简单的base64解码(后续用russh-keys替代)
fn base64_decode(input: &str) -> Result<Vec<u8>> {
    use base64::{Engine as _, engine::general_purpose};
    Ok(general_purpose::STANDARD.decode(input)?)
}

impl Authenticator for SshAuthenticator {
    fn authenticate_pubkey(
        &self,
        username: &str,
        pubkey: &[u8],
        signature: Option<&[u8]>,
    ) -> Result<AuthResult> {
        // 1. 加载用户的authorized_keys
        let authorized_keys = self.load_authorized_keys(username)?;

        // 2. 查找匹配的公钥
        let found = authorized_keys.iter().any(|key| {
            key.as_slice() == pubkey
        });

        if !found {
            return Ok(AuthResult::Failure("公钥未授权".into()));
        }

        // 3. 从/etc/passwd获取用户信息
        match get_user_info(username) {
            Ok(identity) => Ok(AuthResult::Success(identity)),
            Err(e) => Ok(AuthResult::Failure(e.to_string())),
        }
    }

    fn authenticate_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<AuthResult> {
        Ok(AuthResult::Failure("SSH authenticator does not support password".into()))
    }
}
```

- [ ] **Step 5: 添加base64依赖**

修改文件 `agent/Cargo.toml`:

```toml
base64 = "0.22"
```

- [ ] **Step 6: 运行编译验证**

```bash
cd agent
cargo check
```

Expected: 编译成功

- [ ] **Step 7: 提交SSH认证实现**

```bash
git add agent/src/auth/ssh.rs
git add agent/Cargo.toml
git add agent/tests/auth_test.rs
git commit -m "feat(agent): 实现SSH公钥认证基础功能"
```

---

### Task 3: 实现PAM密码认证

**Files:**
- Modify: `agent/src/auth/pam.rs`
- Modify: `agent/Cargo.toml`

- [ ] **Step 1: 添加pam依赖**

修改文件 `agent/Cargo.toml`:

```toml
# PAM认证(新增)
pam = "0.8"
```

- [ ] **Step 2: 实现PAM认证**

修改文件 `agent/src/auth/pam.rs`:

```rust
// agent/src/auth/pam.rs
use super::{AuthResult, Authenticator, UserIdentity, get_user_info};
use anyhow::Result;

pub struct PamAuthenticator {
    pam_service: String,
}

impl PamAuthenticator {
    pub fn new(pam_service: String) -> Self {
        Self { pam_service }
    }

    fn do_pam_authenticate(&self, username: &str, password: &str) -> Result<bool> {
        use pam::{Authenticator as PamAuthLib, PasswordConv};

        // 创建PAM认证器
        let mut auth = PamAuthLib::with_password(&self.pam_service)?;

        // 设置用户名和密码
        auth.get_username()?.set_username(username)?;
        auth.get_password()?.set_password(password)?;

        // 执行认证
        match auth.authenticate() {
            Ok(_) => Ok(true),
            Err(e) => {
                tracing::warn!("PAM认证失败: {} - {}", username, e);
                Ok(false)
            }
        }
    }
}

impl Authenticator for PamAuthenticator {
    fn authenticate_pubkey(
        &self,
        username: &str,
        pubkey: &[u8],
        signature: Option<&[u8]>,
    ) -> Result<AuthResult> {
        Ok(AuthResult::Failure("PAM does not support pubkey".into()))
    }

    fn authenticate_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<AuthResult> {
        // 执行PAM认证
        match self.do_pam_authenticate(username, password)? {
            true => {
                // 认证成功,获取用户信息
                match get_user_info(username) {
                    Ok(identity) => Ok(AuthResult::Success(identity)),
                    Err(e) => Ok(AuthResult::Failure(e.to_string())),
                }
            }
            false => Ok(AuthResult::Failure("用户名或密码错误".into())),
        }
    }
}
```

- [ ] **Step 3: 运行编译验证**

```bash
cd agent
cargo check
```

Expected: 在Linux环境下编译成功。在Windows下会报错(仅支持Unix)

- [ ] **Step 4: 添加平台判断**

修改文件 `agent/src/auth/pam.rs`,在文件顶部添加:

```rust
// agent/src/auth/pam.rs
#[cfg(unix)]
use super::{AuthResult, Authenticator, UserIdentity, get_user_info};
#[cfg(unix)]
use anyhow::Result;

#[cfg(unix)]
pub struct PamAuthenticator {
    pam_service: String,
}

#[cfg(unix)]
impl PamAuthenticator {
    // ... 其他代码
}

#[cfg(unix)]
impl Authenticator for PamAuthenticator {
    // ... 其他代码
}

// 非Unix平台的stub实现
#[cfg(not(unix))]
pub struct PamAuthenticator;

#[cfg(not(unix))]
impl PamAuthenticator {
    pub fn new(_pam_service: String) -> Self {
        Self {}
    }
}

#[cfg(not(unix))]
impl Authenticator for PamAuthenticator {
    fn authenticate_pubkey(&self, _username: &str, _pubkey: &[u8], _signature: Option<&[u8]>) -> Result<AuthResult> {
        Ok(AuthResult::Failure("PAM仅支持Unix平台".into()))
    }

    fn authenticate_password(&self, _username: &str, _password: &str) -> Result<AuthResult> {
        Ok(AuthResult::Failure("PAM仅支持Unix平台".into()))
    }
}
```

- [ ] **Step 5: 运行编译验证**

```bash
cd agent
cargo check
```

Expected: 所有平台编译成功

- [ ] **Step 6: 提交PAM认证实现**

```bash
git add agent/src/auth/pam.rs
git add agent/Cargo.toml
git commit -m "feat(agent): 实现PAM密码认证"
```

---

### Task 4: 实现组合认证器

**Files:**
- Modify: `agent/src/auth/mod.rs`

- [ ] **Step 1: 添加组合认证器**

修改文件 `agent/src/auth/mod.rs`,在文件末尾添加:

```rust
// agent/src/auth/mod.rs
// ... 之前的代码 ...

use std::sync::Arc;

/// 组合认证器(支持多种认证方式)
pub struct CompositeAuthenticator {
    ssh_auth: Arc<ssh::SshAuthenticator>,
    pam_auth: Arc<pam::PamAuthenticator>,
    enable_pubkey: bool,
    enable_password: bool,
}

impl CompositeAuthenticator {
    pub fn new(
        authorized_keys_template: String,
        pam_service: String,
        enable_pubkey: bool,
        enable_password: bool,
    ) -> Self {
        Self {
            ssh_auth: Arc::new(ssh::SshAuthenticator::new(authorized_keys_template)),
            pam_auth: Arc::new(pam::PamAuthenticator::new(pam_service)),
            enable_pubkey,
            enable_password,
        }
    }
}

impl Authenticator for CompositeAuthenticator {
    fn authenticate_pubkey(
        &self,
        username: &str,
        pubkey: &[u8],
        signature: Option<&[u8]>,
    ) -> Result<AuthResult> {
        if !self.enable_pubkey {
            return Ok(AuthResult::Failure("公钥认证未启用".into()));
        }
        self.ssh_auth.authenticate_pubkey(username, pubkey, signature)
    }

    fn authenticate_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<AuthResult> {
        if !self.enable_password {
            return Ok(AuthResult::Failure("密码认证未启用".into()));
        }
        self.pam_auth.authenticate_password(username, password)
    }
}
```

- [ ] **Step 2: 运行编译验证**

```bash
cd agent
cargo check
```

Expected: 编译成功

- [ ] **Step 3: 提交组合认证器**

```bash
git add agent/src/auth/mod.rs
git commit -m "feat(agent): 实现组合认证器"
```

---

## Phase 2: 用户隔离实现 (第2周)

### Task 5: 实现UserSession管理

**Files:**
- Create: `agent/src/auth/session.rs`

- [ ] **Step 1: 创建UserSession模块**

创建文件 `agent/src/auth/session.rs`:

```rust
// agent/src/auth/session.rs
use crate::config::AgentConfig;
use super::UserIdentity;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use uuid::Uuid;

/// 用户会话状态
#[derive(Debug, Clone)]
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

/// 全局会话管理器
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, UserSession>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 创建新的用户会话
    pub async fn create_session(
        &self,
        identity: UserIdentity,
        cfg: &AgentConfig,
    ) -> Result<UserSession> {
        let session_id = format!("sess-{}", Uuid::new_v4());

        // 计算用户允许的路径
        let allowed_paths = self.calculate_allowed_paths(&identity, cfg)?;

        let session = UserSession {
            session_id: session_id.clone(),
            username: identity.username,
            uid: identity.uid,
            gid: identity.gid,
            home_dir: identity.home_dir,
            shell: identity.shell,
            allowed_paths,
            created_at: SystemTime::now(),
        };

        // 存储会话
        self.sessions.write().await.insert(session_id.clone(), session.clone());

        tracing::info!(
            "创建用户会话: {} (uid={}, gid={}, home={})",
            session.username, session.uid, session.gid, session.home_dir.display()
        );

        Ok(session)
    }

    /// 计算用户允许的路径
    fn calculate_allowed_paths(
        &self,
        identity: &UserIdentity,
        cfg: &AgentConfig,
    ) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();

        // 添加用户的家目录
        paths.push(identity.home_dir.clone());

        // 添加全局允许路径(需要验证用户是否有权限访问)
        for path_template in &cfg.security.allowed_paths {
            let path = PathBuf::from(path_template);

            // 检查用户是否有权限访问此路径
            if self.check_path_permission(&path, identity.uid, identity.gid)? {
                paths.push(path);
            }
        }

        Ok(paths)
    }

    /// 检查用户对路径的权限
    fn check_path_permission(
        &self,
        path: &PathBuf,
        uid: u32,
        gid: u32,
    ) -> Result<bool> {
        use std::fs;
        use std::os::unix::fs::MetadataExt;

        if !path.exists() {
            return Ok(false);
        }

        let metadata = fs::metadata(path)?;
        let mode = metadata.mode();
        let file_uid = metadata.uid();
        let file_gid = metadata.gid();

        // 检查用户权限(Owner/Group/Other)
        let has_permission = if file_uid == uid {
            // 用户是文件所有者
            (mode & 0o400) != 0 // 用户读权限
        } else if file_gid == gid {
            // 用户在文件所属组
            (mode & 0o040) != 0 // 组读权限
        } else {
            // 其他用户
            (mode & 0o004) != 0 // 其他读权限
        };

        Ok(has_permission)
    }

    /// 获取会话
    pub async fn get_session(&self, session_id: &str) -> Option<UserSession> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// 删除会话(连接断开时)
    pub async fn remove_session(&self, session_id: &str) {
        if let Some(session) = self.sessions.write().await.remove(session_id) {
            tracing::info!("清理用户会话: {}", session.username);
        }
    }
}
```

- [ ] **Step 2: 在mod.rs中导出session**

修改文件 `agent/src/auth/mod.rs`:

```rust
pub mod session;

// 在文件末尾添加
pub use session::{UserSession, SessionManager};
```

- [ ] **Step 3: 运行编译验证**

```bash
cd agent
cargo check
```

Expected: 编译成功

- [ ] **Step 4: 提交UserSession实现**

```bash
git add agent/src/auth/session.rs
git add agent/src/auth/mod.rs
git commit -m "feat(agent): 实现UserSession管理"
```

---

## Phase 3: 配置和集成 (第3周)

### Task 6: 更新配置文件结构

**Files:**
- Modify: `agent/src/config.rs`

- [ ] **Step 1: 添加SSH/PAM配置**

修改文件 `agent/src/config.rs`:

```rust
// 在AgentConfig结构体中添加
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub security: SecurityConfig,
    pub limits: LimitsConfig,
    pub collectors: CollectorsConfig,
    pub audit: AuditConfig, // 新增
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub token: String,

    // 新增: 认证模式
    #[serde(default = "default_auth_mode")]
    pub mode: String,

    // 新增: SSH配置
    #[serde(default)]
    pub ssh: SshAuthConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SshAuthConfig {
    #[serde(default = "default_enable_pubkey")]
    pub enable_pubkey: bool,

    #[serde(default = "default_authorized_keys_path")]
    pub authorized_keys_path: String,

    #[serde(default = "default_enable_password")]
    pub enable_password: bool,

    #[serde(default = "default_pam_service")]
    pub pam_service: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuditConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_audit_log_file")]
    pub log_file: String,

    #[serde(default = "default_audit_operations")]
    pub log_operations: Vec<String>,
}

fn default_auth_mode() -> String { "ssh-compatible".into() }
fn default_enable_pubkey() -> bool { true }
fn default_authorized_keys_path() -> String { "/home/{username}/.ssh/authorized_keys".into() }
fn default_enable_password() -> bool { true }
fn default_pam_service() -> String { "gnome-remote".into() }
fn default_audit_log_file() -> String { "/var/log/gnome-remote/audit.log".into() }
fn default_audit_operations() -> Vec<String> {
    vec![
        "auth_success".into(),
        "auth_failure".into(),
        "read_file".into(),
        "write_file".into(),
        "delete".into(),
        "terminal_spawn".into(),
    ]
}
```

- [ ] **Step 2: 运行编译验证**

```bash
cd agent
cargo check
```

Expected: 编译成功

- [ ] **Step 3: 提交配置更新**

```bash
git add agent/src/config.rs
git commit -m "feat(agent): 添加SSH/PAM/审计配置"
```

---

## 执行建议

由于本计划涉及大量文件修改和复杂的系统集成，建议采用以下执行策略:

1. **分Phase执行**: 每个Phase独立开发、测试、验证
2. **频繁提交**: 每个Task完成后立即提交
3. **充分测试**: 每个模块都要编写单元测试
4. **渐进式验证**: 先在开发环境验证，再集成到生产环境

**下一步建议**: 选择Task 1开始执行，或者根据实际优先级调整任务顺序。