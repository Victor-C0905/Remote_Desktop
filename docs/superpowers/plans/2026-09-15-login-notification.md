# 登录通知回应体系 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立协议层错误码（AuthErrorCode）+ 客户端结构化错误（ConnectError）+ 前端映射表与通知中心的完整登录通知回应，消除「读取挑战长度失败： connection lost」式指代不明。

**Architecture:** 错误码定义在 quirel-protocol 共享 crate（单一真相源，Agent 只管分类、前端管文案）；Agent 每个认证失败点补发结构化 code，挑战阶段的「直接 close 无通知」黑洞改为先发 `Payload::Error` 帧；客户端 `remote_connect` 错误通道从裸 String 升级为 `ConnectError { code, detail }`；前端以映射表为唯一文案源，激活通知中心并按 code 查表驱动自动重连。

**Tech Stack:** Rust（quirel-protocol / agent / src-tauri，serde + quinn）、React + TypeScript + zustand + vitest。

**⚠️ 用户规则（最高优先级）：**
- **禁止任何 git 操作**（不 add、不 commit、不 push）——计划中的「检查点」步骤只做构建/测试验证并提示用户自行提交
- 保留注释：修改现有代码时保留原有注释，新增代码带中文注释
- 所有代码必须编译零错误

**规格文档:** `docs/superpowers/specs/2026-09-15-login-notification-design.md`

**验证命令速查（Windows PowerShell，工作目录 `e:\MyWork\gnome-remote`）:**
- 协议 crate: `cargo test --manifest-path quirel-protocol/Cargo.toml`
- Agent: `cargo build --manifest-path agent/Cargo.toml`（Windows 有 stub 实现，可编译；Unix 专属测试如失败改用 `wsl -e bash -l -c "cd ~/gnome-remote && cargo test --manifest-path agent/Cargo.toml"`，注意必须带 `-l`）
- 客户端: `cargo test --manifest-path src-tauri/Cargo.toml`
- 前端: `npm run test:run` 与 `npx tsc --noEmit`

---

## 文件结构总览

| 文件 | 职责 | 动作 |
|---|---|---|
| `quirel-protocol/src/envelope.rs` | AuthErrorCode 枚举 + AuthResponse.code 字段 | 修改 |
| `quirel-protocol/tests/wire_compat.rs` | golden 兼容性测试 | 修改 |
| `agent/src/server/quic.rs` | 失败点补发 code + 归因黑洞补丁 | 修改 |
| `src-tauri/src/connection.rs` | ConnectError + 错误分支收敛 + 事件扩展 | 修改 |
| `src/types/errors.ts` | 错误码常量 + 映射表（唯一文案源）+ 解析/格式化 | 新建 |
| `src/types/errors.test.ts` | 映射表全覆盖 + 解析/格式化单测 | 新建 |
| `src/stores/notificationStore.ts` | 通知状态（zustand 内存瞬态） | 新建 |
| `src/shell/NotificationCenter.tsx` | demo → 真实数据源 | 修改 |
| `src/shell/Desktop.tsx` | 通知计数接线（去硬编码） | 修改 |
| `src/context/ServerManager.tsx` | ConnectError 解析 + retryable 查表 + 通知派发 | 修改 |
| `src/apps/Settings.css` | 错误多行显示 | 修改 |
| `src/apps/Terminal.tsx` | 降级提示文案升级 | 修改 |

注：`src-tauri/src/lib.rs` 无需改动（`#[tauri::command]` 对 `Result<T, E: Serialize>` 自动处理，注册路径不变）。

---

### Task 1: quirel-protocol — AuthErrorCode 枚举 + AuthResponse.code 字段

**Files:**
- Modify: `quirel-protocol/src/envelope.rs`
- Test: `quirel-protocol/tests/wire_compat.rs`

- [ ] **Step 1: 写失败的 golden 测试**

在 `quirel-protocol/tests/wire_compat.rs` 末尾追加：

```rust
// ── 登录通知回应体系：AuthResponse.code 字段（2026-09-15 设计） ──

use quirel_protocol::AuthErrorCode;

#[test]
fn auth_response_with_code_wire_format() {
    // 新版 Agent 发送带结构化错误码的认证失败响应
    let env = Envelope::new(
        7,
        Payload::AuthResponse {
            success: false,
            error: Some("用户名或密码错误".to_string()),
            session_id: None,
            code: Some(AuthErrorCode::InvalidCredentials),
        },
    );
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":7,"payload":{"type":"auth_response","data":{"success":false,"error":"用户名或密码错误","session_id":null,"code":"InvalidCredentials"}}}"#
    );
}

#[test]
fn auth_response_without_code_compat() {
    // 旧版 Agent 的响应不含 code 字段 → 新客户端必须可解码且回退 None
    let raw = r#"{"request_id":7,"payload":{"type":"auth_response","data":{"success":false,"error":"账户暂时锁定，请15分钟后再试","session_id":null}}}"#;
    let env: Envelope = serde_json::from_str(raw).expect("旧格式必须可解码");
    match env.payload {
        Payload::AuthResponse { success, error, session_id: _, code } => {
            assert!(!success);
            assert_eq!(error.as_deref(), Some("账户暂时锁定，请15分钟后再试"));
            assert_eq!(code, None);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn auth_error_code_numeric_roundtrip() {
    // AuthErrorCode 数值一旦发布不可变更：as_i32/from_i32 必须稳定往返
    for code in [
        AuthErrorCode::MissingCredentials,
        AuthErrorCode::InvalidKeyFormat,
        AuthErrorCode::KeyParseFailed,
        AuthErrorCode::CertificateRejected,
        AuthErrorCode::DnsFailed,
        AuthErrorCode::ConnectTimeout,
        AuthErrorCode::TlsHandshakeFailed,
        AuthErrorCode::NetworkUnreachable,
        AuthErrorCode::StreamTimeout,
        AuthErrorCode::ConnectionLost,
        AuthErrorCode::RateLimited,
        AuthErrorCode::AccountLocked,
        AuthErrorCode::InvalidCredentials,
        AuthErrorCode::PubkeyNotAuthorized,
        AuthErrorCode::SignatureVerificationFailed,
        AuthErrorCode::ChallengeExpired,
        AuthErrorCode::AuthServiceUnavailable,
        AuthErrorCode::ProtocolError,
        AuthErrorCode::SessionExpired,
        AuthErrorCode::Unknown,
    ] {
        assert_eq!(AuthErrorCode::from_i32(code.as_i32()), Some(code), "code={:?}", code);
    }
    // 既有 HTTP 风格码（≥400）不属于 AuthErrorCode 空间
    for legacy in [400, 401, 403, 404, 408, 500, 501] {
        assert_eq!(AuthErrorCode::from_i32(legacy), None);
    }
}

#[test]
fn payload_error_code_carries_auth_error_code() {
    // Payload::Error.code 保持 i32 类型：归因黑洞补丁复用该字段填 AuthErrorCode 数值
    let env = Envelope::new(0, Payload::Error {
        code: AuthErrorCode::ConnectionLost.as_i32(),
        message: "网络连接异常".to_string(),
    });
    let back = roundtrip(&env);
    match back.payload {
        Payload::Error { code, message } => {
            assert_eq!(AuthErrorCode::from_i32(code), Some(AuthErrorCode::ConnectionLost));
            assert_eq!(message, "网络连接异常");
        }
        _ => panic!("变体不匹配"),
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path quirel-protocol/Cargo.toml`
Expected: 编译失败，`AuthErrorCode` 未定义（`unresolved import quirel_protocol::AuthErrorCode`）。

- [ ] **Step 3: 在 envelope.rs 实现枚举与字段**

3a. 在 `Envelope` 结构体定义之前（约 :18，`use` 语句之后）插入：

```rust
/// 认证/连接错误码（客户端↔Agent 线上协议的组成部分）
///
/// 数值分段（语义一旦发布不可变更，新增只能追加）：
/// - 1-99：客户端本地错误（不经过网络，仅本地分类）
/// - 100-199：网络/传输阶段
/// - 200-299：Agent 认证拒绝
/// - 300-399：会话生命周期（登录后）
/// - 999：兜底
///
/// 双通道使用方式：
/// - `AuthResponse.code`：Option<AuthErrorCode>（serde 序列化为变体名字符串）
/// - `Payload::Error.code`：保持 i32，填 `as_i32()` 数值
///   （与既有 HTTP 风格码 400+ 数值空间不重叠）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u16)]
pub enum AuthErrorCode {
    // 1-99：客户端本地错误
    MissingCredentials = 1,          // 缺少认证凭据
    InvalidKeyFormat = 2,           // 私钥格式不支持
    KeyParseFailed = 3,             // 私钥解析失败（含密码错误）
    CertificateRejected = 4,        // 用户拒绝信任服务器证书

    // 100-199：网络/传输阶段
    DnsFailed = 101,                // 域名解析失败
    ConnectTimeout = 102,           // 连接超时
    TlsHandshakeFailed = 103,       // 安全握手失败
    NetworkUnreachable = 104,       // 网络不可达
    StreamTimeout = 105,            // 认证数据交换超时
    ConnectionLost = 106,           // 连接中断

    // 200-299：Agent 认证拒绝
    RateLimited = 200,              // IP 速率限制
    AccountLocked = 201,            // 账户锁定（15 分钟）
    InvalidCredentials = 202,       // 用户名或密码错误
    PubkeyNotAuthorized = 203,      // 公钥未授权
    SignatureVerificationFailed = 204, // 签名验证失败
    ChallengeExpired = 205,        // 挑战过期
    AuthServiceUnavailable = 206,   // 认证服务不可用
    ProtocolError = 207,           // 协议格式错误

    // 300-399：会话生命周期（登录后）
    SessionExpired = 301,          // 会话超时（24 小时不活动）

    // 999：兜底
    Unknown = 999,
}

impl AuthErrorCode {
    /// 数值形式（用于 Payload::Error.code 的 i32 字段）
    pub fn as_i32(self) -> i32 {
        self as u16 as i32
    }

    /// 从数值解析（未识别返回 None，含既有 HTTP 风格码 ≥400）
    pub fn from_i32(v: i32) -> Option<Self> {
        use AuthErrorCode::*;
        Some(match v {
            1 => MissingCredentials,
            2 => InvalidKeyFormat,
            3 => KeyParseFailed,
            4 => CertificateRejected,
            101 => DnsFailed,
            102 => ConnectTimeout,
            103 => TlsHandshakeFailed,
            104 => NetworkUnreachable,
            105 => StreamTimeout,
            106 => ConnectionLost,
            200 => RateLimited,
            201 => AccountLocked,
            202 => InvalidCredentials,
            203 => PubkeyNotAuthorized,
            204 => SignatureVerificationFailed,
            205 => ChallengeExpired,
            206 => AuthServiceUnavailable,
            207 => ProtocolError,
            301 => SessionExpired,
            999 => Unknown,
            _ => return None,
        })
    }
}
```

3b. 修改 `AuthResponse` 变体（envelope.rs:88-96，注意保留原注释）：

```rust
    /// 认证响应
    #[serde(rename = "auth_response")]
    AuthResponse {
        /// 认证是否成功
        success: bool,
        /// 错误信息（失败时）
        error: Option<String>,
        /// 会话ID（成功时返回）
        session_id: Option<String>,
        /// 结构化错误码（失败时；旧版 Agent 不携带此字段，回退 None）
        #[serde(default)]
        code: Option<AuthErrorCode>,
    },
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --manifest-path quirel-protocol/Cargo.toml`
Expected: 全部 PASS（含既有 golden 用例——证明线上格式无破坏）。

- [ ] **Step 5: 检查点**

提示用户：协议层变更完成，建议自行 `git add quirel-protocol` 提交。

---

### Task 2: agent — send_auth_response 携带 code + 失败点映射

**Files:**
- Modify: `agent/src/server/quic.rs`（send_auth_response ~:1640；失败点 :435/:455/:491/:513/:556/:574/:604/:626/:646/:674/:781/:813/:863/:887/:917）

- [ ] **Step 1: 扩展 send_auth_response 签名**

将 quic.rs:1640-1660 的函数改为（原注释保留）：

```rust
/// 发送认证响应
async fn send_auth_response(
    send: &mut SendStream,
    request_id: u32,
    success: bool,
    error: Option<&str>,
    session_id: Option<&str>,
    code: Option<AuthErrorCode>,
) -> Result<()> {
    let response = Envelope::new(
        request_id,
        Payload::AuthResponse {
            success,
            error: error.map(|s| s.to_string()),
            session_id: session_id.map(|s| s.to_string()),
            code,
        },
    );

    let resp_bytes = response.encode().map_err(|e| anyhow::anyhow!(e))?;
    write_message(send, &resp_bytes).await?;

    Ok(())
}
```

同时在文件头部 `use` 区域确认引入（若已有则跳过）：

```rust
use quirel_protocol::AuthErrorCode;
```

- [ ] **Step 2: 更新全部 16 个调用点**

机械规则：认证成功调用补 `None`；失败调用按映射表补 `Some(AuthErrorCode::…)`。逐点修改（以现有代码中 `send_auth_response(` 为定位锚）：

| 调用点（错误文案） | 追加参数 |
|---|---|
| :435 `"认证流异常关闭"` | `Some(AuthErrorCode::ProtocolError)` |
| :455 `"协议格式错误"` | `Some(AuthErrorCode::ProtocolError)` |
| :487（密码 IP 限速）`"请求过于频繁，请稍后再试"` | `Some(AuthErrorCode::RateLimited)` |
| :509（密码锁定）`"账户暂时锁定，请15分钟后再试"` | `Some(AuthErrorCode::AccountLocked)` |
| :540（密码成功）`None, Some(&session.session_id)` | `None` |
| :556（密码错误）`"用户名或密码错误"` | `Some(AuthErrorCode::InvalidCredentials)` |
| :574（PAM 系统错误）`"认证服务暂时不可用"` | `Some(AuthErrorCode::AuthServiceUnavailable)` |
| :600（公钥 IP 限速）`"请求过于频繁，请稍后再试"` | `Some(AuthErrorCode::RateLimited)` |
| :622（公钥锁定）`"账户暂时锁定，请15分钟后再试"` | `Some(AuthErrorCode::AccountLocked)` |
| :642（检查 authorized_keys 失败）`"认证服务暂时不可用"` | `Some(AuthErrorCode::AuthServiceUnavailable)` |
| :670（公钥未授权）`"公钥未授权"` | `Some(AuthErrorCode::PubkeyNotAuthorized)` |
| :781（公钥不匹配）`"公钥验证失败"` | `Some(AuthErrorCode::SignatureVerificationFailed)` |
| :809（获取用户信息失败）`"认证服务暂时不可用"` | `Some(AuthErrorCode::AuthServiceUnavailable)` |
| :834（公钥成功）`true, None, Some(&session.session_id)` | `None` |
| :863（签名验证 false）`"签名验证失败"` | `Some(AuthErrorCode::SignatureVerificationFailed)` |
| :887（签名验证错误）`"签名验证失败"` | `Some(AuthErrorCode::SignatureVerificationFailed)` |
| :917（挑战验证失败/过期）`"公钥验证失败"` | `Some(AuthErrorCode::ChallengeExpired)` |

示例（:556 密码错误点，其余同型）：

```rust
                    send_auth_response(&mut auth_send, auth_envelope.request_id, false, Some("用户名或密码错误"), None, Some(AuthErrorCode::InvalidCredentials)).await?;
```

- [ ] **Step 3: 编译验证**

Run: `cargo build --manifest-path agent/Cargo.toml`
Expected: 编译零错误（若 Windows 报 Unix 专属测试编译问题，改用 `wsl -e bash -l -c "cd ~/gnome-remote && cargo build --manifest-path agent/Cargo.toml"`，必须带 `-l`）。

- [ ] **Step 4: 检查点**

提示用户：Agent 失败点映射完成，建议自行提交。

---

### Task 3: agent — 归因黑洞补丁 + close code 0x03

**Files:**
- Modify: `agent/src/server/quic.rs`（黑洞点 :709/:715/:734/:932/:939/:947；新增测试 mod）

- [ ] **Step 1: 写失败的单测**

在 quic.rs 文件末尾追加：

```rust
#[cfg(test)]
mod auth_error_tests {
    use quirel_protocol::{AuthErrorCode, Envelope, Payload};

    /// 归因黑洞补丁的错误帧构造规则：
    /// Payload::Error.code 必须能往返解析为 AuthErrorCode（客户端据此归因）
    #[test]
    fn error_frame_carries_auth_error_code() {
        let env = Envelope::new(0, Payload::Error {
            code: AuthErrorCode::ConnectionLost.as_i32(),
            message: "网络连接异常".to_string(),
        });
        let bytes = env.encode().unwrap();
        let back = Envelope::decode(&bytes).unwrap();
        match back.payload {
            Payload::Error { code, .. } => {
                assert_eq!(AuthErrorCode::from_i32(code), Some(AuthErrorCode::ConnectionLost));
            }
            _ => panic!("变体不匹配"),
        }
    }
}
```

Run: `cargo test --manifest-path agent/Cargo.toml auth_error_tests`
Expected: PASS（此测试锁定构造模式，本身即通过——它是后续 6 处补丁的规格锚点）。

- [ ] **Step 2: 实现 send_error_then_close 辅助函数**

在 `send_auth_response` 函数之后插入：

```rust
/// 归因黑洞补丁：先发一帧 Payload::Error 再 close。
/// 此前这些失败点直接 close，客户端只能看到裸 connection lost，
/// 无法区分「Agent 主动通知的失败」与「网络断开」。
/// close code 0x03 = auth-rejected（0x01 idle / 0x02 session 已占用）。
async fn send_error_then_close(
    connection: &quinn::Connection,
    send: &mut SendStream,
    code: AuthErrorCode,
    message: &str,
    close_reason: &'static [u8],
) {
    let env = Envelope::new(
        0,
        Payload::Error { code: code.as_i32(), message: message.to_string() },
    );
    if let Ok(bytes) = env.encode() {
        let _ = write_message(send, &bytes).await;
    }
    connection.close(3u32.into(), close_reason);
}
```

- [ ] **Step 3: 改造 6 个黑洞点**

| 现状代码（定位锚） | 替换为 |
|---|---|
| `:709` `tracing::error!("发送公钥认证挑战失败: {}", e); connection.close(0u32.into(), b"challenge send failed");` | `tracing::error!("发送公钥认证挑战失败: {}", e); send_error_then_close(&connection, &mut auth_send, AuthErrorCode::ConnectionLost, "网络连接异常", b"challenge send failed").await;` |
| `:715` `tracing::error!("编码公钥认证挑战失败: {}", e); connection.close(0u32.into(), b"challenge encode failed");` | `tracing::error!("编码公钥认证挑战失败: {}", e); send_error_then_close(&connection, &mut auth_send, AuthErrorCode::AuthServiceUnavailable, "认证服务暂时不可用", b"challenge encode failed").await;` |
| `:734` `tracing::warn!("公钥认证响应流已关闭: remote={}", remote); connection.close(0u32.into(), b"response stream closed");` | `tracing::warn!("公钥认证响应流已关闭: remote={}", remote); send_error_then_close(&connection, &mut auth_send, AuthErrorCode::StreamTimeout, "认证响应超时", b"response stream closed").await;` |
| `:932`（resp_envelope.payload 的 `other =>` 分支）`connection.close(0u32.into(), b"expected pubkey response");` | `send_error_then_close(&connection, &mut resp_send, AuthErrorCode::ProtocolError, "协议格式错误", b"expected pubkey response").await;` |
| `:939` `tracing::warn!("接受公钥认证响应流失败: {}", e); connection.close(0u32.into(), b"response stream failed");` | `tracing::warn!("接受公钥认证响应流失败: {}", e); send_error_then_close(&connection, &mut auth_send, AuthErrorCode::StreamTimeout, "认证响应超时", b"response stream failed").await;` |
| `:947`（外层 `other =>` 分支）`connection.close(0u32.into(), b"expected auth request");` | `send_error_then_close(&connection, &mut auth_send, AuthErrorCode::ProtocolError, "协议格式错误", b"expected auth request").await;` |

注意 :932 处变量名是 `resp_send`（在 `Ok((mut resp_send, mut resp_recv))` 分支内），其余为 `auth_send`。

- [ ] **Step 4: 编译 + 测试**

Run: `cargo build --manifest-path agent/Cargo.toml && cargo test --manifest-path agent/Cargo.toml`
Expected: 编译零错误，`auth_error_tests` PASS。

- [ ] **Step 5: 检查点**

提示用户：归因黑洞补丁完成（本次事故「读取挑战长度失败」的直接修复），建议自行提交。

---

### Task 4: 客户端 — ConnectError 结构 + 旧文案 fallback 分类器

**Files:**
- Modify: `src-tauri/src/connection.rs`
- Test: `src-tauri/src/connection.rs`（文件末尾 tests mod）

- [ ] **Step 1: 写失败的测试**

在 connection.rs 文件末尾追加：

```rust
#[cfg(test)]
mod connect_error_tests {
    use super::*;

    /// 旧版 Agent（无结构化 code）的中文文案 → 最近似 AuthErrorCode
    #[test]
    fn classify_legacy_error_maps_known_agent_texts() {
        assert_eq!(classify_legacy_error("请求过于频繁，请稍后再试"), AuthErrorCode::RateLimited);
        assert_eq!(classify_legacy_error("账户暂时锁定，请15分钟后再试"), AuthErrorCode::AccountLocked);
        assert_eq!(classify_legacy_error("用户名或密码错误"), AuthErrorCode::InvalidCredentials);
        assert_eq!(classify_legacy_error("公钥未授权"), AuthErrorCode::PubkeyNotAuthorized);
        assert_eq!(classify_legacy_error("签名验证失败"), AuthErrorCode::SignatureVerificationFailed);
        assert_eq!(classify_legacy_error("公钥验证失败"), AuthErrorCode::SignatureVerificationFailed);
        assert_eq!(classify_legacy_error("认证服务暂时不可用"), AuthErrorCode::AuthServiceUnavailable);
        assert_eq!(classify_legacy_error("QUIC 连接超时 (5秒)"), AuthErrorCode::StreamTimeout);
        assert_eq!(classify_legacy_error("QUIC 连接失败: QUIC 握手失败: ..."), AuthErrorCode::ConnectTimeout);
        assert_eq!(classify_legacy_error("任何未识别的文本"), AuthErrorCode::Unknown);
    }

    /// ConnectError 序列化形状（前端 parseConnectError 的解析契约）
    #[test]
    fn connect_error_serializes_code_and_detail() {
        let e = ConnectError::with_detail(AuthErrorCode::AccountLocked, "prod-1");
        assert_eq!(serde_json::to_string(&e).unwrap(), r#"{"code":201,"detail":"prod-1"}"#);
        let e2 = ConnectError::new(AuthErrorCode::ConnectionLost);
        assert_eq!(serde_json::to_string(&e2).unwrap(), r#"{"code":106,"detail":null}"#);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml connect_error_tests`
Expected: 编译失败，`ConnectError`/`classify_legacy_error` 未定义。

- [ ] **Step 3: 实现 ConnectError 与分类器**

在 connection.rs 顶部类型定义区（`ConnectionLostSource` 定义之后）插入：

```rust
/// 连接失败的结构化错误（remote_connect 的错误通道）
///
/// code：分类（必填）；detail：用户可读上下文（如主机名/操作指引），
/// 不含实现细节。完整技术细节只进 tracing 日志（{:?} 记录源错误）。
/// 前端按 code 查映射表得到标题/消息/行动建议/retryable。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectError {
    pub code: AuthErrorCode,
    pub detail: Option<String>,
}

impl ConnectError {
    fn new(code: AuthErrorCode) -> Self {
        Self { code, detail: None }
    }

    fn with_detail(code: AuthErrorCode, detail: impl Into<String>) -> Self {
        Self { code, detail: Some(detail.into()) }
    }
}

/// 旧版 Agent（无结构化 code）的错误文案分类。
/// 覆盖 Agent 侧已知中文文案 + 原有 4 关键词白名单语义；
/// 无法识别 → Unknown。
fn classify_legacy_error(msg: &str) -> AuthErrorCode {
    if msg.contains("请求过于频繁") {
        AuthErrorCode::RateLimited
    } else if msg.contains("锁定") {
        AuthErrorCode::AccountLocked
    } else if msg.contains("用户名或密码错误") {
        AuthErrorCode::InvalidCredentials
    } else if msg.contains("公钥未授权") {
        AuthErrorCode::PubkeyNotAuthorized
    } else if msg.contains("签名验证失败") || msg.contains("公钥验证失败") {
        AuthErrorCode::SignatureVerificationFailed
    } else if msg.contains("认证服务暂时不可用") {
        AuthErrorCode::AuthServiceUnavailable
    } else if msg.contains("超时") || msg.contains("timeout") || msg.contains("timed out") {
        AuthErrorCode::StreamTimeout
    } else if msg.contains("QUIC 连接失败") || msg.contains("认证请求失败") {
        AuthErrorCode::ConnectTimeout
    } else {
        AuthErrorCode::Unknown
    }
}
```

同时确认文件头部 use 引入：`use quirel_protocol::{AuthErrorCode, Envelope, Payload, ...}`（在既有 quirel_protocol 引入中加入 AuthErrorCode）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml connect_error_tests`
Expected: PASS。

- [ ] **Step 5: 检查点**

提示用户：ConnectError 结构就绪，建议自行提交。

---

### Task 5: 客户端 — try_quic_connect 收敛

**Files:**
- Modify: `src-tauri/src/connection.rs`（try_quic_connect :1400-1474；remote_connect :557）

- [ ] **Step 1: 修改 try_quic_connect 签名与错误分支**

签名改为：

```rust
async fn try_quic_connect(host: &str, port: u16) -> Result<(quinn::Connection, f64, String), ConnectError> {
```

各错误分支替换（原 tracing 日志保留，文案中的 `{e}` 改为 `{:?}` 记录完整错误链）：

| 原错误文本（定位锚） | 替换为返回值 |
|---|---|
| `format!("地址解析失败: {}", e)` | `ConnectError::with_detail(AuthErrorCode::DnsFailed, host)` |
| `format!("DNS 解析失败: {}", e)` | `ConnectError::with_detail(AuthErrorCode::DnsFailed, host)` |
| `"DNS 解析无结果".to_string()` | `ConnectError::with_detail(AuthErrorCode::DnsFailed, host)` |
| `format!("创建 Endpoint 失败: {}", e)` | `ConnectError::new(AuthErrorCode::NetworkUnreachable)` |
| `format!("发起连接失败: {}", e)` | `ConnectError::new(AuthErrorCode::NetworkUnreachable)` |
| `"QUIC 连接超时 (5秒)".to_string()` | `ConnectError::new(AuthErrorCode::ConnectTimeout)` |
| `format!("QUIC 握手失败: {}", e)` | `ConnectError::new(AuthErrorCode::TlsHandshakeFailed)` |
| `"未能获取服务器证书指纹".to_string()` | `ConnectError::new(AuthErrorCode::TlsHandshakeFailed)` |

示例（超时分支）：

```rust
    .await
    .map_err(|_| {
        tracing::warn!("[QUIC] 连接超时 (5秒): addr={}", addr);
        ConnectError::new(AuthErrorCode::ConnectTimeout)
    })?
    .map_err(|e| {
        tracing::warn!("[QUIC] 握手失败: addr={}, error={:?}", addr, e);
        ConnectError::new(AuthErrorCode::TlsHandshakeFailed)
    })?;
```

- [ ] **Step 2: 修改 remote_connect 网络阶段与本地凭据分支**

2a. `remote_connect` 返回类型（:156）：

```rust
) -> Result<ConnectionInfo, ConnectError> {
```

2b. :163-167 的 `quic_err_msg` 捕获块删除（不再需要字符串拼接），:556-558 的兜底改为直接透传：

```rust
    // QUIC 失败，返回结构化错误
    Err(quic_result.unwrap_err())
```

2c. 凭据/证书分支：

| 原代码（定位锚） | 替换为 |
|---|---|
| :235 `return Err("用户拒绝信任服务器证书".to_string())` | `return Err(ConnectError::new(AuthErrorCode::CertificateRejected))` |
| :242 `format!("发送证书信任事件失败: {}", e)` | `ConnectError::new(AuthErrorCode::Unknown)`（原 e 只进日志） |
| :248 `"缺少认证凭据".to_string()` | `ConnectError::new(AuthErrorCode::MissingCredentials)` |
| :254 `"密码认证需要提供密码".to_string()` | `ConnectError::new(AuthErrorCode::MissingCredentials)` |
| :284 `"公钥认证需要提供私钥".to_string()` | `ConnectError::new(AuthErrorCode::MissingCredentials)` |

- [ ] **Step 3: 编译验证**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 此时 remote_connect 内其余 `Err(String)` 分支会因类型不匹配报错（Task 6/7 修复）——为使每步可编译，本 Task 先在 :262/:265/:273/:278 等处临时保留 `Err(...)` 会失败；**因此本 Task 与 Task 6/7 需连续执行后再统一编译**。跳过单独编译，直接进入 Task 6（计划如此设计：password/pubkey 两段是同一函数体内的类型迁移）。

- [ ] **Step 4: 检查点（与 Task 6/7 合并）**

---

### Task 6: 客户端 — perform_pubkey_auth 收敛（含 Error 帧 code 读取）

**Files:**
- Modify: `src-tauri/src/connection.rs`（perform_pubkey_auth :1624-1919）

- [ ] **Step 1: 修改签名**

```rust
) -> Result<Option<String>, ConnectError> {
```

- [ ] **Step 2: 错误分支逐段收敛**

映射总表（原 tracing 日志全部保留，`{e}` 改 `{:?}`；原文案作为 detail 保留的仅限用户操作指引）：

| 定位锚（原文案） | code | detail |
|---|---|---|
| :1676 不支持的私钥格式 | InvalidKeyFormat | `Some(原文案)`（含 OpenSSH 指引） |
| :1686 parse_private_key_auto 失败 | KeyParseFailed | `Some(e)`（generate_key_parse_error 产物已是友好指引） |
| :1695 公钥 SSH 格式转换失败 | KeyParseFailed | `None`（e 只进日志） |
| Envelope::encode 失败（:1720 `?`） | ProtocolError | `None`（map_err 后 `?`） |
| :1710 创建 Stream 超时 / 失败 | StreamTimeout / ConnectionLost | `None` |
| :1726-1728 发送长度/数据/flush 失败 | ConnectionLost | `None` |
| :1732 发送公钥请求超时 | StreamTimeout | `None` |
| :1739/:1743 读取挑战长度/数据失败 | ConnectionLost | `None`（关键场景） |
| :1747 接收挑战超时 | StreamTimeout | `None` |
| :1749 Envelope::decode 失败 | ProtocolError | `None` |
| Payload::Error 帧（见 Step 3） | from_i32(code) | `Some(message)` |
| :1761 期望挑战收到其他 | ProtocolError | `None` |
| :1778 密钥数据不是 RSA 格式 | KeyParseFailed | `None` |
| :1806 RSA 密钥构造失败 | KeyParseFailed | `None` |
| :1837 Ed25519 签名失败 | KeyParseFailed | `None` |
| :1842 签名 PEM 编码失败 | KeyParseFailed | `None` |
| :1849 不支持的密钥算法 | InvalidKeyFormat | `None` |
| :1861/:1862 创建响应 Stream 超时/失败 | StreamTimeout / ConnectionLost | `None` |
| :1876-1878 发送签名失败 | ConnectionLost | `None` |
| :1882 发送签名响应超时 | StreamTimeout | `None` |
| :1889/:1893 读取认证结果失败 | ConnectionLost | `None` |
| :1897 接收认证结果超时 | StreamTimeout | `None` |
| :1899 Envelope::decode 失败 | ProtocolError | `None` |
| :1910 Payload::Error 帧（最终结果阶段） | from_i32(code) | `Some(message)` |
| :1916 期望认证结果收到其他 | ProtocolError | `None` |

闭包内 `Ok::<_, String>` 相应改为 `Ok::<_, ConnectError>`（共 4 处：挑战读取、签名发送、结果读取、Stream 创建块）。

- [ ] **Step 3: 挑战接收段（核心改造，替换 :1736-1763）**

```rust
    // ── 第三步：接收挑战 ─────────────────────────────────
    let challenge_data = tokio::time::timeout(timeout_duration, async {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 读取挑战长度失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;

        let mut data = vec![0u8; resp_len];
        recv.read_exact(&mut data).await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 读取挑战数据失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        Ok::<Vec<u8>, ConnectError>(data)
    })
    .await
    .map_err(|_| ConnectError::new(AuthErrorCode::StreamTimeout))??;

    let challenge_envelope = Envelope::decode(&challenge_data)
        .map_err(|e| {
            tracing::warn!("[PubKeyAuth] 挑战解码失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ProtocolError)
        })?;
    let (challenge, challenge_id) = match challenge_envelope.payload {
        Payload::AuthPubKeyChallenge { challenge, challenge_id } => {
            tracing::debug!("[PubKeyAuth] 收到挑战（长度={}字节，id={}）", challenge.len(), challenge_id);
            (challenge, challenge_id)
        }
        Payload::Error { code, message } => {
            // 归因黑洞补丁：Agent 主动通知的失败（区别于裸网络断开）
            tracing::warn!("[PubKeyAuth] Agent 返回错误: code={}, message={}", code, message);
            let err_code = AuthErrorCode::from_i32(code).unwrap_or(AuthErrorCode::Unknown);
            return Err(ConnectError::with_detail(err_code, message));
        }
        other => {
            tracing::error!("[PubKeyAuth] 期望挑战，收到: {:?}", other);
            return Err(ConnectError::new(AuthErrorCode::ProtocolError));
        }
    };
```

- [ ] **Step 4: 最终认证结果段（替换 :1900-1918）**

```rust
    let final_envelope = Envelope::decode(&final_data)
        .map_err(|e| {
            tracing::warn!("[PubKeyAuth] 认证结果解码失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ProtocolError)
        })?;
    match final_envelope.payload {
        Payload::AuthResponse { success, error, session_id, code } => {
            if success {
                tracing::info!("[PubKeyAuth] 公钥认证成功: username={}, session_id={:?}", username, session_id);
                Ok(session_id)
            } else {
                // 优先使用结构化 code；旧版 Agent 无 code 时按文案 fallback 分类
                let err_code = code
                    .unwrap_or_else(|| classify_legacy_error(error.as_deref().unwrap_or("")));
                tracing::error!("[PubKeyAuth] 公钥认证失败: code={:?}, error={:?}", err_code, error);
                Err(ConnectError::with_detail(err_code, error.unwrap_or_else(|| "公钥认证失败".to_string())))
            }
        }
        Payload::Error { code, message } => {
            tracing::error!("[PubKeyAuth] Agent 返回错误: code={}, message={}", code, message);
            let err_code = AuthErrorCode::from_i32(code).unwrap_or(AuthErrorCode::Unknown);
            Err(ConnectError::with_detail(err_code, message))
        }
        other => {
            tracing::error!("[PubKeyAuth] 期望认证结果，收到: {:?}", other);
            Err(ConnectError::new(AuthErrorCode::ProtocolError))
        }
    }
}
```

- [ ] **Step 5: remote_connect 公钥调用点 close reason 真实化（替换 :289-298）**

```rust
                // 执行公钥认证
                let session_id = perform_pubkey_auth(
                    &conn,
                    creds.username.clone(),
                    private_key,
                    creds.passphrase,
                ).await.map_err(|e| {
                    tracing::error!("[Connection] 公钥认证失败: {:?}", e);
                    // close reason 真实化：网络类（100-199）与 Agent 确认拒绝区分，
                    // 避免服务端日志将网络超时误判为认证失败
                    let reason: &'static [u8] = if (100..=199).contains(&e.code.as_i32()) {
                        b"auth network timeout"
                    } else {
                        b"authentication failed"
                    };
                    conn.close(0u32.into(), reason);
                    e
                })?;
```

- [ ] **Step 6: 检查点（与 Task 5/7 合并）**

---

### Task 7: 客户端 — 密码认证收敛 + connection-lost 事件 code

**Files:**
- Modify: `src-tauri/src/connection.rs`（:261-280 密码认证；:546-549 事件 emit）

- [ ] **Step 1: 密码认证段收敛（替换 :260-280）**

```rust
                // 发送认证请求（增加错误处理）
                let resp = send_and_receive_quic(&conn, manager.next_request_id(), auth_payload).await
                    .map_err(|e| {
                        tracing::warn!("[Connection] 认证请求失败: {:?}", e);
                        // 传输层错误：超时类与断连类区分
                        if e.contains("超时") || e.contains("timeout") {
                            ConnectError::new(AuthErrorCode::StreamTimeout)
                        } else {
                            ConnectError::new(AuthErrorCode::ConnectionLost)
                        }
                    })?;

                let envelope = Envelope::decode(&resp)
                    .map_err(|e| {
                        tracing::warn!("[Connection] 解析认证响应失败: {:?}", e);
                        ConnectError::new(AuthErrorCode::ProtocolError)
                    })?;

                // 验证返回类型（增加类型检查）
                match envelope.payload {
                    Payload::AuthResponse { success, error, session_id: _, code } => {
                        if !success {
                            // 优先结构化 code；旧版 Agent 按文案 fallback 分类
                            let err_code = code
                                .unwrap_or_else(|| classify_legacy_error(error.as_deref().unwrap_or("")));
                            // 关闭连接（Agent 确认拒绝，close reason 如实标注）
                            conn.close(0u32.into(), b"authentication failed");
                            return Err(ConnectError::with_detail(err_code, error.unwrap_or_else(|| "认证失败".to_string())));
                        }
                    }
                    other => {
                        conn.close(0u32.into(), b"unexpected response");
                        tracing::warn!("[Connection] 期望 AuthResponse，收到: {:?}", other);
                        return Err(ConnectError::new(AuthErrorCode::ProtocolError));
                    }
                }
```

- [ ] **Step 2: connection-lost 事件携带 code（修改 :531-549 清理块）**

在 `let source_str = ...` 之后、`app_handle.emit` 之前插入归因计算，并扩展 emit payload：

```rust
            // 断连原因分类（close code 0x01 idle / 0x02 session；其余网络断）
            // 计算须在主动 close 之前已定型：close_reason() 为 None 时（本地主动关闭）
            // 不携带 code，前端按通用网络断开展示
            let lost_code = match conn_clone.close_reason() {
                Some(quinn::ConnectionError::ApplicationClosed(close)) => match close.error_code.into_inner() {
                    1 => Some(AuthErrorCode::StreamTimeout),
                    2 => Some(AuthErrorCode::SessionExpired),
                    _ => None,
                },
                Some(quinn::ConnectionError::TimedOut) => Some(AuthErrorCode::StreamTimeout),
                _ => None,
            };
            let _ = app_handle.emit("connection-lost", serde_json::json!({
                "server_id": &server_id_clone,
                "source": source_str,
                "code": lost_code.map(|c| c.as_i32()),
            }));
```

（替换原 :546-549 的 emit 调用；`code` 为 null 时前端视为通用网络断开。）

- [ ] **Step 3: 统一编译 + 测试（Task 5/6/7 验证点）**

Run: `cargo build --manifest-path src-tauri/Cargo.toml && cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 编译零错误；`connect_error_tests` 与既有测试 PASS。

若编译报 `Payload::AuthResponse` 模式不匹配：检查是否遗漏 :269/:1901 两处解构新增 `code` 字段（Task 6 Step 4 与本 Task Step 1 已分别处理）。

- [ ] **Step 4: 检查点**

提示用户：客户端结构化错误迁移完成（Task 5-7），建议自行提交。

---

### Task 8: 前端 — errors.ts 错误码常量 + 映射表 + 解析/格式化

**Files:**
- Create: `src/types/errors.ts`
- Test: `src/types/errors.test.ts`

- [ ] **Step 1: 写失败的测试**

`src/types/errors.test.ts`：

```typescript
import { describe, it, expect } from "vitest";
import {
  AuthErrorCode,
  ERROR_MAP,
  getErrorInfo,
  parseConnectError,
  buildConnectFailureText,
  describeTerminalFailure,
} from "./errors";

describe("ERROR_MAP 全覆盖（每个 code 必有条目）", () => {
  for (const code of Object.values(AuthErrorCode)) {
    it(`code ${code} 有映射条目`, () => {
      const info = getErrorInfo(code);
      expect(info.title.length).toBeGreaterThan(0);
      expect(info.message.length).toBeGreaterThan(0);
      expect(info.action.length).toBeGreaterThan(0);
      expect(typeof info.retryable).toBe("boolean");
    });
  }
});

describe("parseConnectError", () => {
  it("解析结构化 ConnectError JSON", () => {
    expect(parseConnectError({ code: 201, detail: "prod-1" })).toEqual({ code: 201, detail: "prod-1" });
  });
  it("未知数值回退 Unknown", () => {
    expect(parseConnectError({ code: 12345 }).code).toBe(AuthErrorCode.Unknown);
  });
  it("裸字符串 → Unknown + detail", () => {
    expect(parseConnectError("任意旧文本")).toEqual({ code: AuthErrorCode.Unknown, detail: "任意旧文本" });
  });
  it("Error 对象 → Unknown + message", () => {
    expect(parseConnectError(new Error("boom")).detail).toBe("boom");
  });
});

describe("buildConnectFailureText", () => {
  it("包含标题/消息/建议", () => {
    const text = buildConnectFailureText({ code: AuthErrorCode.AccountLocked }, 0);
    expect(text).toContain("账户已临时锁定");
    expect(text).toContain("15 分钟");
    expect(text).toContain("建议：");
  });
  it("其他服务器正常时附加对比提示", () => {
    const text = buildConnectFailureText({ code: AuthErrorCode.ConnectionLost }, 2);
    expect(text).toContain("其他 2 台服务器连接正常");
  });
  it("无其他服务器时不附加对比提示", () => {
    expect(buildConnectFailureText({ code: AuthErrorCode.ConnectionLost }, 0)).not.toContain("台服务器连接正常");
  });
});

describe("describeTerminalFailure", () => {
  it("未连接时给出连接指引", () => {
    expect(describeTerminalFailure("未找到连接")).toContain("先连接");
  });
  it("未知错误保留原文", () => {
    expect(describeTerminalFailure("启动终端失败: xyz")).toBe("启动终端失败: xyz");
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `npm run test:run -- src/types/errors.test.ts`
Expected: FAIL，模块不存在。

- [ ] **Step 3: 实现 errors.ts**

`src/types/errors.ts`（全项目唯一用户文案源；文案遵循「三不暴露」原则：不暴露协议名词/库错误原文/内部机制）：

```typescript
/**
 * 连接错误码（与 quirel-protocol 的 AuthErrorCode #[repr(u16)] 数值对应）
 * 数值语义不可变更：新增只能追加
 */

/* ── 错误码常量 ──────────────────────────────────────── */

export const AuthErrorCode = {
  // 1-99：客户端本地错误
  MissingCredentials: 1,
  InvalidKeyFormat: 2,
  KeyParseFailed: 3,
  CertificateRejected: 4,
  // 100-199：网络/传输阶段
  DnsFailed: 101,
  ConnectTimeout: 102,
  TlsHandshakeFailed: 103,
  NetworkUnreachable: 104,
  StreamTimeout: 105,
  ConnectionLost: 106,
  // 200-299：Agent 认证拒绝
  RateLimited: 200,
  AccountLocked: 201,
  InvalidCredentials: 202,
  PubkeyNotAuthorized: 203,
  SignatureVerificationFailed: 204,
  ChallengeExpired: 205,
  AuthServiceUnavailable: 206,
  ProtocolError: 207,
  // 300-399：会话生命周期
  SessionExpired: 301,
  // 兜底
  Unknown: 999,
} as const;

export type AuthErrorCode = (typeof AuthErrorCode)[keyof typeof AuthErrorCode];

/* ── 映射表条目类型 ──────────────────────────────────── */

export interface ErrorInfo {
  /** 通知标题 */
  title: string;
  /** 用户可读消息（不含实现细节） */
  message: string;
  /** 行动建议 */
  action: string;
  /** true → 自动重连状态机可续期；false → 确定性失败不重试（避免触发服务端锁定） */
  retryable: boolean;
  /** 通知严重度（映射到通知中心 urgency） */
  severity: "critical" | "normal" | "low";
}

/* ── 映射表（全项目唯一用户文案源） ───────────────────── */

export const ERROR_MAP = {
  [AuthErrorCode.MissingCredentials]: { title: "缺少登录信息", message: "未提供所需的登录凭据", action: "请补全服务器登录配置", retryable: false, severity: "normal" },
  [AuthErrorCode.InvalidKeyFormat]: { title: "密钥格式不支持", message: "该密钥文件的格式不受支持", action: "请使用 OpenSSH 格式的密钥文件", retryable: false, severity: "normal" },
  [AuthErrorCode.KeyParseFailed]: { title: "密钥无法读取", message: "密钥文件无法解析，可能已损坏或密码错误", action: "请确认密钥文件与密码", retryable: false, severity: "normal" },
  [AuthErrorCode.CertificateRejected]: { title: "未信任服务器", message: "服务器证书未获信任，连接已取消", action: "如需连接请重新发起并确认证书", retryable: false, severity: "normal" },
  [AuthErrorCode.DnsFailed]: { title: "服务器地址无法解析", message: "找不到该服务器的网络地址", action: "请检查服务器地址配置", retryable: false, severity: "critical" },
  [AuthErrorCode.ConnectTimeout]: { title: "无法连接服务器", message: "连接超时，未能建立连接", action: "请检查网络后重试", retryable: true, severity: "normal" },
  [AuthErrorCode.TlsHandshakeFailed]: { title: "安全连接建立失败", message: "无法与服务器建立安全连接", action: "请检查网络或稍后重试", retryable: true, severity: "normal" },
  [AuthErrorCode.NetworkUnreachable]: { title: "网络不可达", message: "当前网络无法到达该服务器", action: "请检查网络连接", retryable: true, severity: "normal" },
  [AuthErrorCode.StreamTimeout]: { title: "认证超时", message: "认证过程耗时过长", action: "请重新连接", retryable: true, severity: "low" },
  [AuthErrorCode.ConnectionLost]: { title: "网络连接中断", message: "与服务器的连接已中断", action: "请检查网络后重试", retryable: true, severity: "normal" },
  [AuthErrorCode.RateLimited]: { title: "请求过于频繁", message: "短时间内连接请求过多，服务器暂时限制了访问", action: "请稍候片刻再试", retryable: true, severity: "normal" },
  [AuthErrorCode.AccountLocked]: { title: "账户已临时锁定", message: "出于安全考虑，多次认证失败后账户被暂时锁定", action: "请于 15 分钟后重试", retryable: false, severity: "critical" },
  [AuthErrorCode.InvalidCredentials]: { title: "用户名或密码错误", message: "服务器拒绝了当前凭据", action: "请检查后重试", retryable: false, severity: "critical" },
  [AuthErrorCode.PubkeyNotAuthorized]: { title: "密钥未获授权", message: "该密钥未被服务器授权登录", action: "请在服务器上添加公钥授权", retryable: false, severity: "critical" },
  [AuthErrorCode.SignatureVerificationFailed]: { title: "密钥验证失败", message: "密钥与服务器记录不匹配", action: "请确认使用正确的密钥文件", retryable: false, severity: "critical" },
  [AuthErrorCode.ChallengeExpired]: { title: "认证超时", message: "认证流程耗时过长，已失效", action: "重新连接即可", retryable: true, severity: "low" },
  [AuthErrorCode.AuthServiceUnavailable]: { title: "服务暂时不可用", message: "服务器暂时无法处理登录请求", action: "请稍后重试", retryable: true, severity: "normal" },
  [AuthErrorCode.ProtocolError]: { title: "通信异常", message: "与服务器的通信出现异常", action: "请更新客户端后重试", retryable: false, severity: "critical" },
  [AuthErrorCode.SessionExpired]: { title: "会话已超时", message: "长时间未操作，会话已结束", action: "请重新连接", retryable: true, severity: "low" },
  [AuthErrorCode.Unknown]: { title: "连接失败", message: "发生未知错误", action: "请重试", retryable: false, severity: "critical" },
} as const satisfies Record<AuthErrorCode, ErrorInfo>;

/** 查表（未识别 code 回退 Unknown） */
export function getErrorInfo(code: number): ErrorInfo {
  return (ERROR_MAP as Record<number, ErrorInfo>)[code] ?? ERROR_MAP[AuthErrorCode.Unknown];
}

/* ── 解析与格式化 ────────────────────────────────────── */

/** 解析 invoke reject 的错误（结构化 ConnectError JSON 或裸字符串） */
export function parseConnectError(err: unknown): { code: number; detail?: string } {
  if (
    typeof err === "object" && err !== null && "code" in err &&
    typeof (err as { code: unknown }).code === "number"
  ) {
    const e = err as { code: number; detail?: string | null };
    const known = (ERROR_MAP as Record<number, ErrorInfo>)[e.code] !== undefined;
    return { code: known ? e.code : AuthErrorCode.Unknown, detail: e.detail ?? undefined };
  }
  // 裸字符串（防御路径）：保持原文本作 detail，分类 Unknown
  const msg = err instanceof Error ? err.message : String(err);
  return { code: AuthErrorCode.Unknown, detail: msg };
}

/** 判断连接错误是否可重试（自动重连状态机门控） */
export function isRetryableConnectError(err: unknown): boolean {
  return getErrorInfo(parseConnectError(err).code).retryable;
}

/**
 * 构建连接失败的展示文本（标题/消息/上下文/建议）。
 * othersConnected > 0 时附加多服务器对比提示（只读已有状态，零探测）。
 */
export function buildConnectFailureText(err: unknown, othersConnected: number): string {
  const { code, detail } = parseConnectError(err);
  const info = getErrorInfo(code);
  let text = `${info.title}\n${info.message}`;
  if (detail && detail !== info.title) text += `\n${detail}`;
  text += `\n建议：${info.action}`;
  if (othersConnected > 0) {
    text += `\n\nℹ️ 其他 ${othersConnected} 台服务器连接正常，仅此台无法连接\n可能是本机与该服务器之间的网络问题，建议更换网络环境后重试`;
  }
  return text;
}

/** 终端初始化失败的降级文案（含未连接场景指引） */
export function describeTerminalFailure(err: unknown): string {
  const msg = err instanceof Error ? err.message : String(err);
  if (msg.includes("未找到连接") || msg.includes("未找到该服务器的连接")) {
    return "尚未连接到服务器，请先连接后再打开终端";
  }
  return buildConnectFailureText(err, 0);
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `npm run test:run -- src/types/errors.test.ts && npx tsc --noEmit`
Expected: 测试全 PASS，类型检查零错误。

- [ ] **Step 5: 检查点**

提示用户：前端错误码映射表就绪，建议自行提交。

---

### Task 9: 前端 — notificationStore + 通知中心激活

**Files:**
- Create: `src/stores/notificationStore.ts`
- Modify: `src/shell/NotificationCenter.tsx`
- Modify: `src/shell/Desktop.tsx`

- [ ] **Step 1: 创建 notificationStore**

`src/stores/notificationStore.ts`（zustand 内存瞬态 store——通知属于瞬态 UI 状态，不持久化，遵循「存储全部走 Rust」约束）：

```typescript
import { create } from "zustand";

/* ── Types ─────────────────────────────────────────────── */

/** 通知严重度（与错误映射表 severity 对齐） */
export type NotificationUrgency = "low" | "normal" | "critical";

export interface AppNotification {
  id: string;
  /** 标题（如「网络连接中断」） */
  title: string;
  /** 正文（消息 + 可选对比提示） */
  body: string;
  /** 行动建议（如「请检查网络后重试」） */
  action?: string;
  timestamp: number;
  urgency: NotificationUrgency;
  /** 来源（服务器名或模块名） */
  source: string;
  /** 关联服务器（清理用；非服务器通知为空） */
  serverId?: string;
  read: boolean;
}

interface NotificationState {
  notifications: AppNotification[];
  pushNotification: (n: Omit<AppNotification, "id" | "timestamp" | "read">) => void;
  markAsRead: (id: string) => void;
  markAllRead: () => void;
  dismiss: (id: string) => void;
  clearAll: () => void;
}

/* ── 自增 ID（会话内唯一即可） ─────────────────────────── */

let nextId = 0;

/* ── Store ─────────────────────────────────────────────── */

export const useNotificationStore = create<NotificationState>((set) => ({
  notifications: [],
  pushNotification: (n) =>
    set((s) => ({
      notifications: [
        { ...n, id: `notif-${Date.now()}-${nextId++}`, timestamp: Date.now(), read: false },
        ...s.notifications,
      ].slice(0, 50), // 上限 50 条，防膨胀
    })),
  markAsRead: (id) =>
    set((s) => ({
      notifications: s.notifications.map((n) => (n.id === id ? { ...n, read: true } : n)),
    })),
  markAllRead: () =>
    set((s) => ({ notifications: s.notifications.map((n) => ({ ...n, read: true })) })),
  dismiss: (id) =>
    set((s) => ({ notifications: s.notifications.filter((n) => n.id !== id) })),
  clearAll: () => set({ notifications: [] }),
}));
```

- [ ] **Step 2: NotificationCenter 消费真实数据**

修改 `src/shell/NotificationCenter.tsx`：

2a. 头部 import 与类型替换：

```typescript
import { useState, useEffect, useCallback } from "react";
import "./NotificationCenter.css";
import { useNotificationStore } from "../stores/notificationStore";
import type { AppNotification, NotificationUrgency } from "../stores/notificationStore";
```

删除本地 `type Urgency`、`interface Notification` 定义与整个 `generateDemoNotifications()` 函数（保留 `formatTime`/`getUrgencyIcon`/`getUrgencyClass` 工具函数，`Urgency` 类型引用改为 `NotificationUrgency`）。

2b. 组件内 state 替换：

```typescript
export function NotificationCenter({ isOpen, onClose }: NotificationCenterProps) {
  const { notifications, markAsRead, dismiss, clearAll, markAllRead } = useNotificationStore();
  const [filter, setFilter] = useState<NotificationUrgency | "all">("all");
```

（删除 `const [notifications, setNotifications] = useState<Notification[]>(generateDemoNotifications());`）

2c. 通知条目渲染增加行动建议行（在 `.nc-notif-body` 之后）：

```tsx
                  <div className="nc-notif-body">{notif.body}</div>
                  {notif.action && <div className="nc-notif-action">💡 {notif.action}</div>}
                  <div className="nc-notif-time">{formatTime(notif.timestamp)}</div>
```

2d. `markAsRead`/`dismissNotification`/`clearAll`/`markAllRead` 的本地 useCallback 实现删除，直接用 store 解构出的方法（引用处 `dismissNotification(notif.id)` 改 `dismiss(notif.id)`）。

2e. `NotificationCenter.css` 末尾追加行动建议样式：

```css
/* ── 行动建议行 ──────────────────────────────────────── */
.nc-notif-action {
  font-size: 12px;
  color: #99c1f1;
  margin-top: 2px;
  white-space: pre-line;
}
```

- [ ] **Step 3: Desktop 接线（去硬编码）**

修改 `src/shell/Desktop.tsx`：

3a. import 增加：

```typescript
import { useNotificationStore } from "../stores/notificationStore";
```

3b. :143-145 的硬编码替换：

```typescript
  const [notificationOpen, setNotificationOpen] = useState(false);
  const notifications = useNotificationStore((s) => s.notifications);
  const unreadNotifications = notifications.filter((n) => !n.read).length;
  const criticalNotifications = notifications.filter((n) => n.urgency === "critical" && !n.read).length;
```

- [ ] **Step 4: 类型检查 + 全量前端测试**

Run: `npx tsc --noEmit && npm run test:run`
Expected: 零错误。

- [ ] **Step 5: 检查点**

提示用户：通知中心已激活，建议自行提交。

---

### Task 10: 前端 — ServerManager 升级（解析/查表/通知派发/对比提示）

**Files:**
- Modify: `src/context/ServerManager.tsx`

- [ ] **Step 1: import 与类型扩展**

1a. import 增加：

```typescript
import { useNotificationStore } from "../stores/notificationStore";
import { parseConnectError, getErrorInfo, buildConnectFailureText } from "../types/errors";
```

1b. `ConnectionLostPayload` 接口扩展：

```typescript
/**
 * connection-lost 事件负载（Rust 统一清理块 emit）
 * - user_initiated: 用户主动断开，前端不自动重连
 * - heartbeat / quic_closed / send_failed: 网络断开，前端自动重连
 * - code: 断连原因分类（AuthErrorCode 数值；null 为通用网络断开）
 */
interface ConnectionLostPayload {
  server_id: string;
  source: "user_initiated" | "heartbeat" | "quic_closed" | "send_failed";
  code?: number | null;
}
```

1c. 删除原 `isRetryableConnectError` 的 4 关键词字符串实现（:61-70，含注释——其职责由 errors.ts 的查表实现接管，注释迁移过去）。

- [ ] **Step 2: 通知派发辅助函数**

在 Provider 内（`performConnect` 之前）添加：

```typescript
  // ── 连接通知派发（通知中心数据源） ──────────────────────
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  /** 连接失败通知（含多服务器对比提示——只读已有状态，零探测） */
  const notifyConnectFailure = useCallback((server: ServerConfig, err: unknown) => {
    const { code, detail } = parseConnectError(err);
    const info = getErrorInfo(code);
    const others = useServersStore
      .getState()
      .servers.filter((s) => s.id !== server.id && s.status === "connected").length;
    const body =
      `${info.message}${detail && detail !== info.title ? `\n${detail}` : ""}` +
      (others > 0
        ? `\n\nℹ️ 其他 ${others} 台服务器连接正常，仅此台无法连接\n可能是本机与该服务器之间的网络问题，建议更换网络环境后重试`
        : "");
    pushNotification({
      title: info.title,
      body,
      action: info.action,
      urgency: info.severity,
      source: server.name || server.host,
      serverId: server.id,
    });
  }, [pushNotification]);
```

- [ ] **Step 3: connectServer catch 升级**

替换 :298-305：

```typescript
    try {
      await performConnect(server);
    } catch (err) {
      // 结构化解析 → 映射表文案（含对比提示）→ 状态 + 通知双通道
      const others = servers.filter((s) => s.id !== id && s.status === "connected").length;
      const display = buildConnectFailureText(err, others);
      log.error("连接失败:", display);

      setServerStatus(id, "error", display);
      notifyConnectFailure(server, err);
    }
  }, [servers, activeServerId, setServerStatus, setActiveServerId, performConnect, notifyConnectFailure]);
```

- [ ] **Step 4: connection-lost 监听器升级（通知 + code 传递）**

替换 :124-137 的非 user_initiated 分支：

```typescript
        if (source === "user_initiated") {
          // 用户主动断开：不自动重连
          setServerStatus(lostServerId, "disconnected");
          log.warn(`服务器 ${lostServerId} 已主动断开`);
          return;
        }

        // 网络断开：进入自动重连流程（最多 3 次、5 秒间隔）
        // 状态置为 reconnecting，各应用依据 activeServerId 已清空显示离线占位，
        // 重连成功后恢复 activeServerId，应用现有 effect 自动恢复
        setServerStatus(lostServerId, "reconnecting");
        log.warn(`服务器 ${lostServerId} 连接断开（${source}），开始自动重连`);

        // 断连通知（code 携带分类：0x01 idle→认证超时 / 0x02 session→会话超时）
        const lostServer = useServersStore.getState().servers.find((s) => s.id === lostServerId);
        const lostInfo = code != null ? getErrorInfo(code) : null;
        useNotificationStore.getState().pushNotification({
          title: lostInfo?.title ?? "网络连接中断",
          body: lostInfo?.message ?? "与服务器的连接已中断，正在自动重连",
          action: "自动重连中（最多 3 次）",
          urgency: "normal",
          source: lostServer ? lostServer.name || lostServer.host : "连接",
          serverId: lostServerId,
        });

        scheduleReconnectRef.current(lostServerId, 1);
```

监听回调解构处同步改为 `const { server_id: lostServerId, source, code } = event.payload;`。

- [ ] **Step 5: attemptReconnect 升级（查表门控 + 最终失败通知 + 成功通知）**

替换 :329-342 的 try/catch：

```typescript
    log.info(`自动重连第 ${attempt}/${MAX_RECONNECT_ATTEMPTS} 次:`, serverId);
    try {
      await performConnectRef.current(server);
      log.info("自动重连成功:", serverId);
      // 重连成功通知（连接恢复）
      useNotificationStore.getState().pushNotification({
        title: "连接已恢复",
        body: `服务器 ${server.name || server.host} 已重新连接`,
        urgency: "low",
        source: server.name || server.host,
        serverId,
      });
    } catch (err) {
      // 确定性失败（认证被拒/证书被拒等）不重试，避免触发服务端认证锁定
      // 门控从字符串白名单升级为映射表查表（isRetryableConnectError 来自 errors.ts）
      if (!isRetryableConnectError(err) || attempt >= MAX_RECONNECT_ATTEMPTS) {
        const others = useServersStore
          .getState()
          .servers.filter((s) => s.id !== serverId && s.status === "connected").length;
        const display = `自动重连失败: ${buildConnectFailureText(err, others)}`;
        log.warn(display);
        setServerStatus(serverId, "error", display);
        useNotificationStore.getState().pushNotification({
          title: "自动重连失败",
          body: buildConnectFailureText(err, others),
          action: getErrorInfo(parseConnectError(err).code).action,
          urgency: "critical",
          source: server.name || server.host,
          serverId,
        });
        return;
      }
      log.warn(`自动重连失败 (${attempt}/${MAX_RECONNECT_ATTEMPTS})，稍后重试`);
      scheduleReconnectRef.current(serverId, attempt + 1);
    }
```

并在文件头部 import 中加入 `isRetryableConnectError`：

```typescript
import { parseConnectError, getErrorInfo, buildConnectFailureText, isRetryableConnectError } from "../types/errors";
```

- [ ] **Step 6: 类型检查 + 测试**

Run: `npx tsc --noEmit && npm run test:run`
Expected: 零错误、全 PASS。

- [ ] **Step 7: 检查点**

提示用户：ServerManager 升级完成，建议自行提交。

---

### Task 11: 前端 — Settings/Terminal 文案升级

**Files:**
- Modify: `src/apps/Settings.css`（:837-844）
- Modify: `src/apps/Terminal.tsx`（:223-234）

- [ ] **Step 1: Settings 错误多行显示**

`src/apps/Settings.css` :837 的 `.st-conn-error` 规则追加 `white-space: pre-line;`（展示文本含 `\n` 分隔的标题/消息/建议/对比提示）：

```css
.st-conn-error {
  font-size: var(--font-small);
  color: #e01b24;
  background: rgba(224, 27, 36, 0.1);
  padding: 8px 12px;
  border-radius: var(--radius-xs);
  margin-top: 8px;
  white-space: pre-line;
}
```

（Settings.tsx 无需改动——`cardServer.error` 存的已是 `buildConnectFailureText` 的结构化文案。）

- [ ] **Step 2: Terminal 降级提示升级**

`src/apps/Terminal.tsx` 头部 import 增加：

```typescript
import { describeTerminalFailure } from "../types/errors";
```

:227 行替换：

```typescript
          terminal.write(`\x1b[31m[连接失败]\x1b[0m ${describeTerminalFailure(err)}\r\n`);
```

- [ ] **Step 3: 类型检查**

Run: `npx tsc --noEmit`
Expected: 零错误。

- [ ] **Step 4: 检查点**

提示用户：全部 UI 升级完成，建议自行提交。

---

### Task 12: 全链路验证

- [ ] **Step 1: 全部测试套件**

```powershell
cargo test --manifest-path quirel-protocol/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path agent/Cargo.toml
npm run test:run
npx tsc --noEmit
```

Expected: 全部 PASS / 零错误。

- [ ] **Step 2: 客户端整体构建**

```powershell
npm run build
```

Expected: `tsc && vite build` 成功。

- [ ] **Step 3: 手动冒烟（可选，需要真实服务器）**

1. 启动应用连接正常服务器 → 断网 → 观察：通知中心出现「网络连接中断」通知、自动重连 3 次、最终失败时出现含建议的完整通知
2. 用错误密码连接 → 「用户名或密码错误」通知（不自动重试）
3. 其他服务器保持连接时连接失败的服务器 → 通知附带「其他 N 台服务器连接正常」对比提示
4. Settings 连接卡片错误行显示多行结构化文案

- [ ] **Step 4: 最终检查点**

提示用户：实现全部完成。需要部署侧配合：服务器 Agent 需同步更新（旧 Agent 仍可工作——走 fallback 字符串分类，但无归因黑洞修复）。

---

## Self-Review 记录

1. **Spec 覆盖**：§3.1/3.2→Task 1；§3.3→Task 3；§4.1→Task 2；§5.1-5.5→Task 4-7；§6.1/6.2→Task 7 Step 2；§7.1-7.3→Task 8；§7.4→Task 9；§7.5/7.7→Task 10；§7.6→Task 11；§8→各 Task 测试步骤 + Task 12；§9 边界未越界（未动业务命令/主动探测/wss）。
2. **占位符扫描**：无 TBD/TODO；所有代码步骤含完整代码。
3. **类型一致性**：`AuthErrorCode`（Rust 枚举 ↔ TS const 数值）、`ConnectError { code: i32, detail: Option<String> }`（serde 序列化 `{"code":N,"detail":...}` ↔ `parseConnectError`）、`send_auth_response` 6 参签名、`buildConnectFailureText`/`describeTerminalFailure`/`isRetryableConnectError` 在 Task 8 定义、Task 10/11 引用，名称一致。
