# Phase 3: 集成与安全实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** 完成认证系统的最后集成，包括QUIC认证流程、Session管理、审计日志和systemd服务配置，实现完整的端到端功能。

**Architecture:** QUIC连接→认证→Session创建→业务操作，完整的审计追踪，systemd开机自启。

**Tech Stack:** Rust, systemd, 审计日志, QUIC/TLS

---

## File Structure

### Agent侧新增文件

| 文件路径 | 责任 |
|---------|------|
| `agent/src/audit.rs` | 审计日志模块 |

### Agent侧修改文件

| 文件路径 | 改动内容 |
|---------|---------|
| `agent/src/server/quic.rs` | 添加认证流程，创建Session |
| `agent/src/main.rs` | 初始化SessionManager和审计 |
| `systemd/gnome-remote-agent.service` | systemd服务单元 |
| `pam.d/gnome-remote` | PAM配置文件 |
| `install.sh` | 自动安装脚本 |

---

## Phase 3任务

### Task 11: 实现审计日志模块

**Files:**
- Create: `agent/src/audit.rs`

- [ ] **Step 1: 创建审计日志模块**

创建文件 `agent/src/audit.rs`：

```rust
// agent/src/audit.rs
// 操作审计日志

use anyhow::Result;
use chrono::{DateTime, Local};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// 审计日志记录器
pub struct AuditLogger {
    log_file: Mutex<std::fs::File>,
    enabled: bool,
}

impl AuditLogger {
    /// 创建新的审计日志记录器
    pub fn new(log_path: &str, enabled: bool) -> Result<Self> {
        if !enabled {
            return Ok(Self {
                log_file: Mutex::new(OpenOptions::new().write(true).open("")?),
                enabled: false,
            });
        }

        // 确保日志目录存在
        let log_path = PathBuf::from(log_path);
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        Ok(Self {
            log_file: Mutex::new(file),
            enabled: true,
        })
    }

    /// 记录操作
    pub fn log_operation(
        &self,
        username: &str,
        uid: u32,
        operation: &str,
        details: &str,
    ) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let log_line = format!(
            "{} user={} uid={} op={} details={}\n",
            timestamp, username, uid, operation, details
        );

        let mut file = self.log_file.lock().unwrap();
        file.write_all(log_line.as_bytes())?;

        Ok(())
    }

    /// 记录认证成功
    pub fn log_auth_success(&self, username: &str, uid: u32, method: &str) -> Result<()> {
        self.log_operation(username, uid, "auth_success", &format!("method={}", method))
    }

    /// 记录认证失败
    pub fn log_auth_failure(&self, username: &str, method: &str, reason: &str) -> Result<()> {
        self.log_operation(username, 0, "auth_failure", &format!("method={} reason={}", method, reason))
    }

    /// 记录文件操作
    pub fn log_file_operation(
        &self,
        username: &str,
        uid: u32,
        operation: &str,
        path: &str,
    ) -> Result<()> {
        self.log_operation(username, uid, operation, &format!("path={}", path))
    }
}
```

- [ ] **Step 2: 在main.rs中引入**

修改文件 `agent/src/main.rs`：

```rust
mod audit;
```

- [ ] **Step 3: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译成功

- [ ] **Step 4: 提交审计日志实现**

```bash
git add agent/src/audit.rs
git add agent/src/main.rs
git commit -m "feat(agent): 实现操作审计日志"
```

---

### Task 12: QUIC认证流程集成

**Files:**
- Modify: `agent/src/server/quic.rs`
- Modify: `agent/src/main.rs`

- [ ] **Step 1: 添加SessionManager到QuicServer**

修改文件 `agent/src/server/quic.rs`：

```rust
use crate::auth::{Authenticator, CompositeAuthenticator, UserSession, SessionManager};
use crate::audit::AuditLogger;

pub struct QuicServer {
    // ... 现有字段
    auth: Arc<CompositeAuthenticator>,
    session_manager: Arc<SessionManager>,
    audit: Arc<AuditLogger>,
}

impl QuicServer {
    pub fn new(
        // ... 现有参数
        auth: Arc<CompositeAuthenticator>,
        audit: Arc<AuditLogger>,
    ) -> Self {
        Self {
            // ... 现有字段
            auth,
            session_manager: Arc::new(SessionManager::new()),
            audit,
        }
    }
}
```

- [ ] **Step 2: 实现认证流程**

在handle_connection中添加认证：

```rust
async fn handle_connection(&self, conn: quinn::Connection) {
    // 1. 创建认证流
    let (mut send, mut recv) = match conn.accept_bi().await {
        Ok(stream) => stream,
        Err(e) => {
            tracing::error!("接受认证流失败: {}", e);
            return;
        }
    };

    // 2. 读取认证请求
    let auth_request = match self.read_auth_request(&mut recv).await {
        Ok(req) => req,
        Err(e) => {
            tracing::error!("读取认证请求失败: {}", e);
            conn.close(1u32.into(), b"Auth request failed");
            return;
        }
    };

    // 3. 执行认证
    let auth_result = match auth_request {
        AuthRequest::PublicKey { username, pubkey, signature } => {
            self.audit.log_auth_failure(&username, "pubkey", "attempting")?;
            self.auth.authenticate_pubkey(&username, &pubkey, signature.as_deref())
        }
        AuthRequest::Password { username, password } => {
            self.audit.log_auth_failure(&username, "password", "attempting")?;
            self.auth.authenticate_password(&username, &password)
        }
    };

    // 4. 处理认证结果
    let identity = match auth_result {
        Ok(AuthResult::Success(identity)) => {
            self.audit.log_auth_success(&identity.username, identity.uid, "pubkey")?;
            identity
        }
        Ok(AuthResult::Failure(reason)) => {
            self.audit.log_auth_failure(&username, "pubkey", &reason)?;
            send_auth_failure(&mut send, &reason).await?;
            conn.close(1u32.into(), b"Authentication failed");
            return;
        }
        Err(e) => {
            send_auth_failure(&mut send, &e.to_string()).await?;
            conn.close(1u32.into(), b"Authentication error");
            return;
        }
    };

    // 5. 创建Session
    let session = match self.session_manager.create_session(identity, &self.cfg).await {
        Ok(session) => session,
        Err(e) => {
            tracing::error!("创建Session失败: {}", e);
            conn.close(1u32.into(), b"Session creation failed");
            return;
        }
    };

    tracing::info!(
        "用户认证成功: {} (uid={}, session_id={})",
        session.username, session.uid, session.session_id
    );

    // 6. 发送认证成功响应
    send_auth_success(&mut send, &session).await?;

    // 7. 处理后续请求（使用session）
    self.handle_requests(conn, session).await;
}
```

- [ ] **Step 3: 更新handle_requests**

修改handle_requests使用session：

```rust
async fn handle_requests(&self, conn: quinn::Connection, session: UserSession) {
    while let Ok(stream) = conn.accept_bi().await {
        let (mut send, mut recv) = stream;

        // 读取请求
        let envelope = match read_envelope(&mut recv).await {
            Ok(env) => env,
            Err(e) => {
                tracing::error!("读取请求失败: {}", e);
                break;
            }
        };

        // 使用session处理请求（恢复之前注释的代码）
        let response = handler::handle_envelope(&envelope, &self.cfg, &session).await;

        // 发送响应
        if let Err(e) = write_envelope(&mut send, &response).await {
            tracing::error!("发送响应失败: {}", e);
            break;
        }
    }

    // 清理Session
    self.session_manager.remove_session(&session.session_id).await;
}
```

- [ ] **Step 4: 在main.rs中初始化**

修改文件 `agent/src/main.rs`：

```rust
use std::sync::Arc;

fn main() -> Result<()> {
    // ... 现有初始化代码

    // 创建认证器
    let auth = Arc::new(CompositeAuthenticator::new(
        cfg.auth.ssh.pam_service.clone(),
        cfg.auth.ssh.enable_pubkey,
        cfg.auth.ssh.enable_password,
    ));

    // 创建审计日志
    let audit = Arc::new(AuditLogger::new(
        &cfg.audit.log_file,
        cfg.audit.enabled,
    )?);

    // 创建QUIC服务器
    let server = QuicServer::new(cfg.clone(), auth, audit)?;

    // ... 运行服务器
}
```

- [ ] **Step 5: 编译验证**

```bash
cd agent
cargo check
```

Expected: 编译成功

- [ ] **Step 6: 提交QUIC认证集成**

```bash
git add agent/src/server/quic.rs
git add agent/src/main.rs
git commit -m "feat(quic): 集成认证流程和Session管理"
```

---

### Task 13: 创建systemd服务配置

**Files:**
- Create: `systemd/gnome-remote-agent.service`
- Create: `pam.d/gnome-remote`
- Create: `install.sh`

- [ ] **Step 1: 创建systemd服务单元**

创建文件 `systemd/gnome-remote-agent.service`：

```ini
[Unit]
Description=GNOME Remote Agent - SSH-like Remote Management
Documentation=https://github.com/yourname/gnome-remote
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
StartLimitBurst=3
StartLimitIntervalSec=60

# 安全配置
NoNewPrivileges=false
CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_SYS_ADMIN CAP_NET_BIND_SERVICE

# 资源限制
LimitNOFILE=65536
LimitNPROC=unlimited

# 环境变量
Environment="RUST_LOG=info"
Environment="HOME=/var/lib/gnome-remote"

# 日志
StandardOutput=journal
StandardError=journal
SyslogIdentifier=gnome-remote-agent

[Install]
WantedBy=multi-user.target
```

- [ ] **Step 2: 创建PAM配置**

创建文件 `pam.d/gnome-remote`：

```bash
# /etc/pam.d/gnome-remote
# GNOME Remote Agent PAM配置

# 认证
auth        required      pam_env.so
auth        required      pam_unix.so nullok try_first_pass
auth        requisite     pam_succeed_if.so uid >= 1000 quiet_success
auth        required      pam_deny.so

# 账户
account     required      pam_unix.so
account     sufficient    pam_localuser.so
account     sufficient    pam_succeed_if.so uid < 1000 quiet
account     required      pam_permit.so

# 密码
password    requisite     pam_pwquality.so try_first_pass local_users_only retry=3 authtok_type=
password    sufficient    pam_unix.so sha512 shadow nullok try_first_pass use_authtok
password    required      pam_deny.so

# 会话
session     optional      pam_keyinit.so revoke
session     required      pam_limits.so
-session    optional      pam_systemd.so
session     required      pam_unix.so
```

- [ ] **Step 3: 创建安装脚本**

创建文件 `install.sh`：

```bash
#!/bin/bash
# install.sh - GNOME Remote Agent安装脚本

set -e

echo "=== GNOME Remote Agent 安装脚本 ==="

# 1. 创建目录
echo "创建目录..."
mkdir -p /etc/gnome-remote
mkdir -p /var/log/gnome-remote
mkdir -p /var/lib/gnome-remote

# 2. 复制二进制文件
echo "安装二进制文件..."
cp target/release/gnome-remote-agent /usr/local/bin/
chmod 755 /usr/local/bin/gnome-remote-agent

# 3. 复制配置文件
echo "安装配置文件..."
cp agent.toml /etc/gnome-remote/
chmod 644 /etc/gnome-remote/agent.toml

# 4. 生成自签名证书
if [ ! -f /etc/gnome-remote/cert.pem ]; then
    echo "生成TLS证书..."
    openssl req -x509 -newkey rsa:4096 -keyout /etc/gnome-remote/key.pem \
        -out /etc/gnome-remote/cert.pem -days 365 -nodes \
        -subj "/CN=gnome-remote-agent"
    chmod 600 /etc/gnome-remote/key.pem
    chmod 644 /etc/gnome-remote/cert.pem
fi

# 5. 安装PAM配置
echo "配置PAM..."
cp pam.d/gnome-remote /etc/pam.d/

# 6. 安装systemd服务
echo "安装systemd服务..."
cp systemd/gnome-remote-agent.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable gnome-remote-agent.service

# 7. 启动服务
echo "启动服务..."
systemctl start gnome-remote-agent.service

# 8. 检查状态
systemctl status gnome-remote-agent.service --no-pager

echo "=== 安装完成 ==="
```

- [ ] **Step 4: 提交systemd配置**

```bash
git add systemd/
git add pam.d/
git add install.sh
git commit -m "feat(deploy): 添加systemd服务配置和安装脚本"
```

---

## 执行建议

Phase 3是最后的集成阶段，建议：
1. 逐步集成，每步验证编译
2. 充分测试认证流程
3. 确保审计日志正确记录
4. 在Linux环境测试systemd服务

**下一步**: 开始Task 11（审计日志模块）