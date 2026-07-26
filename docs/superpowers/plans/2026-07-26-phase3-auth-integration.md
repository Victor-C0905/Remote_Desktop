# Phase 3: 认证流程集成实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将已实现的SSH兼容认证模块集成到实际的QUIC连接流程，实现多用户认证和权限隔离

**Architecture:** 客户端发送认证凭据（密码/私钥），Agent端使用CompositeAuthenticator验证身份，创建UserSession，后续操作在用户上下文中执行

**Tech Stack:** Rust (QUIC/PAM/SSH), TypeScript (Tauri/React)

---

## 前置条件检查

**已完成：**
- ✅ Agent端：SshAuthenticator、PamAuthenticator、CompositeAuthenticator已实现
- ✅ Agent端：UserSession、UserExecutor、UserNamespace已实现
- ✅ 客户端：Credentials结构体、AuthMethod枚举已定义
- ✅ 类型定义：ServerConfig.auth字段已添加

**当前缺失：**
- ❌ 客户端：remote_connect不接受credentials参数
- ❌ Agent端：认证器未集成到QUIC连接流程
- ❌ 实际的认证流程没有被调用

---

## 文件结构映射

**客户端修改：**
- `src-tauri/src/connection.rs` - 接受credentials参数，构造认证Payload
- `src/context/ServerManager.tsx` - 已完成（传递credentials）

**Agent端修改：**
- `agent/src/server/quic.rs` - 集成CompositeAuthenticator，创建UserSession
- `agent/src/protocol.rs` - 扩展AuthRequest payload支持多种认证方式

---

## Task 1: 扩展协议层支持多认证方式

**Files:**
- Modify: `agent/src/protocol.rs`

**背景：** 当前Payload::AuthRequest只支持token，需要扩展支持password和pubkey

- [ ] **Step 1: 扩展AuthRequest payload定义**

查看当前的AuthRequest定义：

```rust
// agent/src/protocol.rs 当前定义
pub enum Payload {
    AuthRequest { token: String },
    // ...
}
```

扩展为支持多种认证方式：

```rust
pub enum Payload {
    // 旧版Token认证（保持向后兼容）
    AuthRequest { token: String },

    // 新版：用户名+密码认证
    AuthPasswordRequest {
        username: String,
        password: String,
    },

    // 新版：用户名+公钥认证
    AuthPubKeyRequest {
        username: String,
        public_key: Vec<u8>,      // SSH公钥字节
        signature: Vec<u8>,       // 签名数据
        challenge: Vec<u8>,       // 挑战数据（用于签名）
    },

    // 统一的认证响应
    AuthResponse {
        success: bool,
        error: Option<String>,
        session_id: Option<String>,  // 成功时返回会话ID
    },
    // ...
}
```

- [ ] **Step 2: 验证编译通过**

Run: `cd agent && cargo check`
Expected: 编译通过，无错误

---

## Task 2: 客户端传递认证凭据

**Files:**
- Modify: `src-tauri/src/connection.rs`

- [ ] **Step 1: 修改remote_connect函数签名**

当前签名：

```rust
pub async fn remote_connect(
    server_id: String,
    host: String,
    port: u16,
    token: Option<String>,
    app: tauri::AppHandle,
) -> Result<ConnectionInfo, String>
```

修改为：

```rust
use crate::Credentials;

pub async fn remote_connect(
    server_id: String,
    host: String,
    port: u16,
    credentials: Option<Credentials>,  // 新参数
    app: tauri::AppHandle,
) -> Result<ConnectionInfo, String>
```

- [ ] **Step 2: 实现认证Payload构造逻辑**

在QUIC连接成功后（第437-449行），替换旧的Token认证逻辑：

```rust
// 旧代码（删除）
if let Some(ref tk) = token {
    let auth_result = send_and_receive_quic(&conn, manager.next_request_id(), Payload::AuthRequest { token: tk.clone() }).await;
    // ...
}

// 新代码
let auth_payload = match credentials {
    Some(creds) => {
        match creds.method.as_str() {
            "password" => {
                Payload::AuthPasswordRequest {
                    username: creds.username.unwrap_or_default(),
                    password: creds.password.unwrap_or_default(),
                }
            }
            "pubkey" => {
                // TODO: 实现SSH签名验证
                Payload::AuthPubKeyRequest {
                    username: creds.username.unwrap_or_default(),
                    public_key: vec![],  // 从private_key解析
                    signature: vec![],   // 生成签名
                    challenge: vec![],   // 生成挑战
                }
            }
            _ => {
                // 默认：尝试Token认证
                Payload::AuthRequest {
                    token: creds.password.unwrap_or_default(),
                }
            }
        }
    }
    None => {
        return Err("未提供认证凭据".into());
    }
};

let auth_result = send_and_receive_quic(&conn, manager.next_request_id(), auth_payload).await;
if let Ok(resp) = auth_result {
    if let Ok(envelope) = Envelope::decode(&resp) {
        if let Payload::AuthResponse { success, error, .. } = envelope.payload {
            if !success {
                return Err(error.unwrap_or("认证失败".into()));
            }
        }
    }
}
```

- [ ] **Step 3: 验证编译通过**

Run: `cd src-tauri && cargo check`
Expected: 编译通过，无错误

---

## Task 3: Agent端集成认证流程

**Files:**
- Modify: `agent/src/server/quic.rs`

- [ ] **Step 1: 在handle_connection中集成认证器**

当前认证流程（第277-337行）使用简单的Token认证。需要替换为：

```rust
// 接收认证流
let auth_stream = connection.accept_bi().await?;
let (mut auth_send, mut auth_recv) = auth_stream;

// 读取认证请求
let auth_data = read_message(&mut auth_recv).await?;
if auth_data.is_none() {
    connection.close(0u32.into(), b"auth stream closed");
    return Ok(());
}

let auth_data = auth_data.unwrap();
let auth_envelope = Envelope::decode(&auth_data).map_err(|e| anyhow::anyhow!(e))?;

// 根据payload类型执行不同的认证逻辑
let session = match &auth_envelope.payload {
    // 旧版Token认证（保持向后兼容）
    Payload::AuthRequest { token } => {
        tracing::info!("收到Token认证请求: remote={}", remote);

        let token_valid = !cfg.auth.token.is_empty() && token == &cfg.auth.token;
        if token_valid {
            // 创建默认用户会话
            let default_identity = crate::auth::UserIdentity::new(
                whoami::username(),
                1000, 1000,
                format!("/home/{}", whoami::username()),
                "/bin/bash".to_string(),
            );
            let session = UserSession::new(default_identity);

            audit_log.log_auth_success(&session.username, session.uid, "token");

            send_auth_response(&mut auth_send, auth_envelope.request_id, true, None).await?;

            tracing::info!("✅ Token认证成功: remote={}", remote);
            session
        } else {
            audit_log.log_auth_failure("unknown", 0, "invalid_token");
            send_auth_response(&mut auth_send, auth_envelope.request_id, false, Some("Token 无效")).await?;
            connection.close(0u32.into(), b"authentication failed");
            return Ok(());
        }
    }

    // 新版：密码认证
    Payload::AuthPasswordRequest { username, password } => {
        tracing::info!("收到密码认证请求: username={}, remote={}", username, remote);

        // 调用CompositeAuthenticator进行认证
        match authenticator.authenticate_password(username, password) {
            Ok(auth_result) => {
                match auth_result {
                    crate::auth::AuthResult::Success(identity) => {
                        let session = UserSession::new(identity);

                        audit_log.log_auth_success(&session.username, session.uid, "password");

                        send_auth_response(&mut auth_send, auth_envelope.request_id, true, Some(&session.session_id)).await?;

                        tracing::info!("✅ 密码认证成功: username={}, uid={}", session.username, session.uid);
                        session
                    }
                    crate::auth::AuthResult::Failure(reason) => {
                        audit_log.log_auth_failure(username, 0, "invalid_password");
                        send_auth_response(&mut auth_send, auth_envelope.request_id, false, Some(&reason)).await?;
                        connection.close(0u32.into(), b"authentication failed");
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                tracing::error!("密码认证异常: {}", e);
                audit_log.log_auth_failure(username, 0, "auth_error");
                send_auth_response(&mut auth_send, auth_envelope.request_id, false, Some("认证失败")).await?;
                connection.close(0u32.into(), b"authentication failed");
                return Ok(());
            }
        }
    }

    // 新版：公钥认证
    Payload::AuthPubKeyRequest { username, public_key, signature, challenge } => {
        tracing::info!("收到公钥认证请求: username={}, remote={}", username, remote);

        // TODO: 实现SSH公钥验证
        // 1. 解析public_key
        // 2. 验证signature
        // 3. 调用authenticator.authenticate_pubkey()

        audit_log.log_auth_failure(username, 0, "pubkey_not_implemented");
        send_auth_response(&mut auth_send, auth_envelope.request_id, false, Some("公钥认证暂未实现")).await?;
        connection.close(0u32.into(), b"authentication failed");
        return Ok(());
    }

    other => {
        tracing::warn!("期望认证请求, 收到: {:?}", other);
        connection.close(0u32.into(), b"expected auth request");
        return Ok(());
    }
};
```

- [ ] **Step 2: 修改send_auth_response函数签名**

当前函数只接受success和error参数，需要添加session_id：

```rust
async fn send_auth_response(
    send: &mut SendStream,
    request_id: u64,
    success: bool,
    session_id: Option<&str>,  // 新增参数
) -> Result<()> {
    let response = Envelope::new(
        request_id,
        Payload::AuthResponse {
            success,
            error: None,
            session_id: session_id.map(|s| s.to_string()),
        },
    );

    send_message(send, &response.encode()?).await?;
    Ok(())
}
```

- [ ] **Step 3: 验证编译通过**

Run: `cd agent && cargo check`
Expected: 编译通过，有未使用警告（正常）

---

## Task 4: 集成测试

**Files:**
- Create: `agent/tests/auth_integration_test.rs`

- [ ] **Step 1: 编写密码认证集成测试**

```rust
use gnome_remote_agent::{AgentConfig, CompositeAuthenticator};
use gnome_remote_agent::auth::{PamAuthenticator, SshAuthenticator};

#[tokio::test]
async fn test_password_auth_integration() {
    // 创建测试配置
    let mut cfg = AgentConfig::default();
    cfg.auth.mode = "ssh-compatible".to_string();

    // 创建认证器
    let pam_auth = Arc::new(PamAuthenticator::new("gnome-remote"));
    let ssh_auth = Arc::new(SshAuthenticator::new("/home/{username}/.ssh/authorized_keys"));
    let authenticator = Arc::new(CompositeAuthenticator::new(ssh_auth, pam_auth, true, true));

    // 测试有效用户认证（需要系统中有测试用户）
    // 注意：此测试需要在真实Linux环境中运行

    // TODO: 添加具体测试逻辑
}
```

- [ ] **Step 2: 手动测试流程**

测试步骤：
1. 启动Agent：`sudo ./target/release/agent --config config.toml`
2. 使用客户端连接：选择"密码认证"，输入用户名和密码
3. 验证：执行文件操作，检查权限隔离是否生效
4. 日志：检查 `/var/log/gnome-remote/audit.log` 是否有认证记录

Expected behavior:
- ✅ 认证成功：可以访问用户家目录
- ✅ 权限隔离：不能访问其他用户文件
- ✅ 审计日志：记录认证成功/失败

---

## Task 5: 文档更新

**Files:**
- Create: `agent/README.md`
- Create: `pam.d/README.md`

- [ ] **Step 1: 编写部署文档**

```markdown
# Agent部署指南

## 安装PAM配置

```bash
sudo cp pam.d/gnome-remote /etc/pam.d/
```

## 启动Agent

```bash
sudo ./agent --config config.toml
```

## 测试认证

使用客户端连接，选择认证方式：
- 密码认证：输入Linux用户名和密码
- 公钥认证：使用SSH私钥
```

- [ ] **Step 2: 提交修改**

```bash
git add -A
git commit -m "feat(phase3): 集成认证流程到QUIC连接"
```

---

## 执行方式选择

**Plan complete and saved to `docs/superpowers/plans/2026-07-26-phase3-auth-integration.md`.**

**Two execution options:**

**1. Subagent-Driven (recommended)** - 我为每个Task分派独立子代理，Task之间进行审查，快速迭代

**2. Inline Execution** - 在当前会话中使用executing-plans技能执行，批量执行并设置检查点

**Which approach?**