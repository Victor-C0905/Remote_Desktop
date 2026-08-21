# 版本系统实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Agent 和 Client 引入标准 SemVer 版本号系统,支持 `agent --version`、Client 关于页动态显示版本、连接后显示 Agent 版本,并用 `release.ps1` 辅助修复 bug/新增功能时同步修改版本号。

**Architecture:** Agent 与 Client 各自以 Cargo.toml 的 `version` 字段为唯一真相源;Agent 通过 clap `#[command(version)]` 暴露 `--version`,协议 `AuthResponse` 新增 `agent_version` 字段透传给 Client;Client 的 `src-tauri/build.rs` 读 Cargo.toml 同步到 tauri.conf.json/package.json 并生成前端 `version.ts`;前端关于页改用动态常量,连接后从 `ConnectionInfo.agentVersion` 读取并缓存到 serversStore。

**Tech Stack:** Rust(clap 4 / serde / build.rs)、TypeScript(React / Zustand / Tauri 2)、PowerShell(发布脚本)

**关联 spec:** [docs/superpowers/specs/2026-08-21-version-system-design.md](file:///e:/MyWork/gnome-remote/docs/superpowers/specs/2026-08-21-version-system-design.md)

**Git 规则:** 本计划所有 commit step 仅打印建议命令,由用户手动执行 git;实现过程不自动 `git add`/`git commit`/`git push`。

---

## 文件结构

### 修改的现有文件
- `agent/src/protocol/serde.rs` — AuthResponse 增加 agent_version 字段(Agent 侧协议)
- `agent/src/server/quic.rs` — send_auth_response 填入 agent_version
- `agent/src/main.rs` — clap 加 version 属性 + 启动日志
- `src-tauri/src/connection.rs` — Payload::AuthResponse 加 agent_version + ConnectionInfo 加 agentVersion + 两处解析提取
- `src-tauri/build.rs` — 重写:同步 tauri.conf.json/package.json + 生成 version.ts
- `src/types/server.ts` — ServerConfig 加 agentVersion 字段
- `src/context/ServerManager.tsx` — 连接成功后缓存 agentVersion 到 store
- `src/apps/Settings.tsx` — 关于页改用 CLIENT_VERSION + 显示 activeServer.agentVersion

### 新增文件
- `src/config/version.ts` — 由 build.rs 生成,导出 CLIENT_VERSION
- `scripts/release.ps1` — 版本号修改 + 触发同步 + 打印 git 提示
- `scripts/sync-version.ps1` — 触发 build.rs 重新生成派生文件

---

## Task 1: Agent 协议层 AuthResponse 增加 agent_version

**Files:**
- Modify: `agent/src/protocol/serde.rs:142-151`(AuthResponse 定义)
- Test: `agent/tests/auth_test.rs`(已有测试文件,加序列化测试)

- [ ] **Step 1: 写失败的序列化测试**

在 `agent/tests/auth_test.rs` 末尾追加:

```rust
#[test]
fn test_auth_response_serializes_agent_version() {
    use gnome_remote_agent::protocol::Payload;

    // 成功响应带 agent_version
    let payload = Payload::AuthResponse {
        success: true,
        error: None,
        session_id: Some("sess-123".into()),
        agent_version: Some("0.2.0".into()),
    };
    let env = gnome_remote_agent::protocol::Envelope::new(1, payload);
    let json = env.encode().unwrap();
    let json_str = String::from_utf8(json).unwrap();
    assert!(json_str.contains(r#""agent_version":"0.2.0""#), "agent_version 应被序列化: {}", json_str);

    // 旧客户端兼容:None 时字段被 skip,不出现在 JSON
    let payload_old = Payload::AuthResponse {
        success: false,
        error: Some("fail".into()),
        session_id: None,
        agent_version: None,
    };
    let env_old = gnome_remote_agent::protocol::Envelope::new(2, payload_old);
    let json_old = String::from_utf8(env_old.encode().unwrap()).unwrap();
    assert!(!json_old.contains("agent_version"), "None 时不应序列化 agent_version: {}", json_old);
}

#[test]
fn test_auth_response_deserializes_without_agent_version() {
    // 旧 Agent 发的响应(无 agent_version 字段)能被新 Client 解析,字段为 None
    let json = r#"{"request_id":3,"payload":{"type":"auth_response","data":{"success":true,"error":null,"session_id":"sess-x"}}}"#;
    let env: gnome_remote_agent::protocol::Envelope = serde_json::from_str(json).unwrap();
    if let gnome_remote_agent::protocol::Payload::AuthResponse { agent_version, .. } = env.payload {
        assert_eq!(agent_version, None, "缺失字段应反序列化为 None");
    } else {
        panic!("期望 AuthResponse");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run(在 WSL,`agent/` 目录): `cargo test --test auth_test test_auth_response_serializes_agent_version -- --nocapture`
Expected: 编译失败,`no field 'agent_version' in Payload::AuthResponse` 之类错误。

- [ ] **Step 3: 修改 AuthResponse 增加 agent_version 字段**

修改 `agent/src/protocol/serde.rs` 的 AuthResponse(L142-151):

```rust
    /// 认证响应
    #[serde(rename = "auth_response")]
    AuthResponse {
        /// 认证是否成功
        success: bool,
        /// 错误信息(失败时)
        error: Option<String>,
        /// 会话ID(成功时返回)
        session_id: Option<String>,
        /// Agent 版本号(认证成功时返回,如 "0.2.0");旧 Agent 无此字段,反序列化为 None
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_version: Option<String>,
    },
```

- [ ] **Step 4: 运行测试确认通过**

Run(WSL,`agent/`): `cargo test --test auth_test test_auth_response_ -- --nocapture`
Expected: 两个测试 PASS。

- [ ] **Step 5: 提交(用户手动执行)**

```
git add agent/src/protocol/serde.rs agent/tests/auth_test.rs
git commit -m "feat(agent): AuthResponse 增加 agent_version 字段透传版本号"
```

---

## Task 2: Agent send_auth_response 填入 agent_version

**Files:**
- Modify: `agent/src/server/quic.rs:1617-1638`(send_auth_response 函数)
- Modify: `agent/src/server/quic.rs:517`、`L789`(两处调用点)

- [ ] **Step 1: 修改 send_auth_response 函数签名与构造**

修改 `agent/src/server/quic.rs` 的 `send_auth_response`(L1617-1638):

```rust
/// 发送认证响应
async fn send_auth_response(
    send: &mut SendStream,
    request_id: u32,
    success: bool,
    error: Option<&str>,
    session_id: Option<&str>,
    agent_version: Option<&str>,
) -> Result<()> {
    let response = Envelope::new(
        request_id,
        Payload::AuthResponse {
            success,
            error: error.map(|s| s.to_string()),
            session_id: session_id.map(|s| s.to_string()),
            agent_version: agent_version.map(|s| s.to_string()),
        },
    );

    let resp_bytes = response.encode().map_err(|e| anyhow::anyhow!(e))?;
    write_message(send, &resp_bytes).await?;

    Ok(())
}
```

- [ ] **Step 2: 修改密码认证成功调用点**

在 `agent/src/server/quic.rs` 约 L517 附近(密码认证成功,原调用 `send_auth_response(..., true, ..., session.session_id)`),在末尾追加 `agent_version` 参数。定位原有调用(搜索 `send_auth_response` 在 L517 区域),改为:

```rust
send_auth_response(
    send,
    request_id,
    true,
    None,
    Some(&session.session_id),
    Some(env!("CARGO_PKG_VERSION")),
).await?;
```

- [ ] **Step 3: 修改公钥认证成功调用点**

在 `agent/src/server/quic.rs` 约 L789 附近(公钥认证成功),同样追加第 6 参数 `Some(env!("CARGO_PKG_VERSION"))`:

```rust
send_auth_response(
    send,
    request_id,
    true,
    None,
    Some(&session.session_id),
    Some(env!("CARGO_PKG_VERSION")),
).await?;
```

- [ ] **Step 4: 修改认证失败调用点(若有)**

搜索 `send_auth_response` 的所有调用(WSL,`agent/`): `grep -n "send_auth_response(" src/server/quic.rs`,对每个调用追加第 6 参数——失败响应传 `None`:

```rust
send_auth_response(send, request_id, false, Some("认证失败"), None, None).await?;
```

- [ ] **Step 5: 编译验证**

Run(WSL,`agent/`): `cargo build --bin agent`
Expected: 编译通过(可能有 warning,无 error)。若报"参数数量不匹配",回到 Step 2-4 检查所有调用点已加第 6 参数。

- [ ] **Step 6: 提交(用户手动执行)**

```
git add agent/src/server/quic.rs
git commit -m "feat(agent): send_auth_response 填入 agent_version=CARGO_PKG_VERSION"
```

---

## Task 3: Agent 命令行 --version 与启动日志

**Files:**
- Modify: `agent/src/main.rs:13-15`(clap 定义) 及启动日志位置

- [ ] **Step 1: clap 增加 version 属性**

修改 `agent/src/main.rs` L13-15:

```rust
#[derive(Parser, Debug)]
#[command(name = "gnome-remote-agent")]
#[command(version, about = "GNOME Remote Control — 远程 Agent 服务端")]
struct Args {
```

(仅在原有 `#[command(about = ...)]` 行改为 `#[command(version, about = ...)]`,加 `version`)

- [ ] **Step 2: 启动日志增加版本打印**

在 `agent/src/main.rs` 的 `init_logging` 调用之后、进入 worker/manager 业务逻辑之前(搜索 `init_logging` 调用处,在其后),插入:

```rust
tracing::info!(version = env!("CARGO_PKG_VERSION"), "gnome-remote-agent starting");
```

- [ ] **Step 3: 验证 --version 输出**

Run(WSL,`agent/`): `cargo run --bin agent -- --version`
Expected: 输出 `gnome-remote-agent 0.1.0`(当前 Cargo.toml 版本)。

- [ ] **Step 4: 验证 -V 简写**

Run: `cargo run --bin agent -- -V`
Expected: 同样输出 `gnome-remote-agent 0.1.0`。

- [ ] **Step 5: 验证启动日志**

Run: `cargo run --bin agent -- --config agent.toml`(若需配置,或用 `--help` 查可用参数,然后短时启动观察首行日志)
Expected: 首条 info 日志含 `version=0.1.0`。Ctrl+C 退出。

- [ ] **Step 6: 提交(用户手动执行)**

```
git add agent/src/main.rs
git commit -m "feat(agent): clap 暴露 --version 并在启动日志打印版本"
```

---

## Task 4: Client Rust 协议层同步 agent_version

**Files:**
- Modify: `src-tauri/src/connection.rs:16-29`(ConnectionInfo) `:67-83`(Payload::AuthResponse) `:612`(info 构造) `:710`(密码解析) `:730-742`(公钥调用) `:2014`(公钥解析)

- [ ] **Step 1: ConnectionInfo 增加 agentVersion 字段**

修改 `src-tauri/src/connection.rs` 的 `ConnectionInfo`(L16-29):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub server_id: String,
    pub host: String,
    pub port: u16,
    #[serde(rename = "transport")]
    pub transport_type: String,
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "rttMs")]
    pub rtt_ms: f64,
    #[serde(rename = "connectedAt")]
    pub connected_at: u64,
    /// Agent 版本号(认证成功后从 AuthResponse 提取;旧 Agent 无此字段为 None)
    #[serde(rename = "agentVersion", skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
}
```

- [ ] **Step 2: info 构造处初始化 agent_version 为 None**

修改 `src-tauri/src/connection.rs` L612 的 `ConnectionInfo { ... }` 构造,末尾加字段:

```rust
        let mut info = ConnectionInfo {
            server_id: server_id.clone(),
            host: host.clone(),
            port,
            transport_type: "quic".into(),
            status: "connected".into(),
            rtt_ms: rtt,
            connected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            agent_version: None,
        };
```

(注意:`let info` 改为 `let mut info`,因后续要写入 agent_version)

- [ ] **Step 3: Payload::AuthResponse 增加 agent_version 字段**

修改 `src-tauri/src/connection.rs` 的 `Payload::AuthResponse`(L75-83):

```rust
    /// 认证响应
    #[serde(rename = "auth_response")]
    AuthResponse {
        /// 认证是否成功
        success: bool,
        /// 错误信息(失败时)
        error: Option<String>,
        /// 会话ID(成功时返回)
        session_id: Option<String>,
        /// Agent 版本号(认证成功时返回)
        #[serde(default)]
        agent_version: Option<String>,
    },
```

(`#[serde(default)]` 保证旧 Agent 无此字段时反序列化为 None)

- [ ] **Step 4: 密码认证解析处提取 agent_version**

修改 `src-tauri/src/connection.rs` L710 的解析:

```rust
                match envelope.payload {
                    Payload::AuthResponse { success, error, session_id: _, agent_version } => {
                        if !success {
                            conn.close(0u32.into(), b"authentication failed");
                            return Err(error.unwrap_or_else(|| "认证失败".to_string()));
                        }
                        info.agent_version = agent_version;
                    }
                    other => {
                        conn.close(0u32.into(), b"unexpected response");
                        return Err(format!("期望 AuthResponse,收到: {:?}", other));
                    }
                }
```

- [ ] **Step 5: perform_pubkey_auth 改返回 (session_id, agent_version)**

修改 `src-tauri/src/connection.rs` L2014 的公钥认证解析(在 `perform_pubkey_auth` 函数内):

```rust
        Payload::AuthResponse { success, error, session_id, agent_version } => {
            if success {
                tracing::info!("[PubKeyAuth] 公钥认证成功: username={}, session_id={:?}", username, session_id);
                Ok((session_id, agent_version))
            } else {
                tracing::error!("[PubKeyAuth] 公钥认证失败: {:?}", error);
                Err(error.unwrap_or_else(|| "公钥认证失败".to_string()))
            }
        }
```

同时修改该函数签名返回类型由 `Result<Option<String>, String>`(session_id)改为 `Result<(Option<String>, Option<String>), String>`(session_id, agent_version)。在函数定义处(搜索 `async fn perform_pubkey_auth`)修改返回类型。

- [ ] **Step 6: 公钥认证调用处解构并赋值**

修改 `src-tauri/src/connection.rs` L730-742 的调用:

```rust
            AuthMethod::PubKey => {
                let private_key = creds.private_key.ok_or_else(|| "公钥认证需要提供私钥".to_string())?;
                tracing::info!("[Connection] 开始公钥认证: username={}", creds.username);

                let (session_id, agent_version) = perform_pubkey_auth(
                    &conn,
                    creds.username.clone(),
                    private_key,
                    creds.passphrase,
                ).await.map_err(|e| {
                    tracing::error!("[Connection] 公钥认证失败: {}", e);
                    conn.close(0u32.into(), b"authentication failed");
                    e
                })?;

                info.agent_version = agent_version;
                tracing::info!("[Connection] 公钥认证成功: username={}, session_id={:?}", creds.username, session_id);
            }
```

- [ ] **Step 7: 编译验证**

Run(Windows,项目根): `cd src-tauri; cargo build`
Expected: 编译通过。若 `perform_pubkey_auth` 还有其他调用点(搜索 `perform_pubkey_auth(`),同步修改解构。

- [ ] **Step 8: 提交(用户手动执行)**

```
git add src-tauri/src/connection.rs
git commit -m "feat(client): ConnectionInfo 透传 agent_version 到前端"
```

---

## Task 5: Client build.rs 同步版本号并生成 version.ts

**Files:**
- Modify: `src-tauri/build.rs`(当前仅 `fn main() { tauri_build::build() }`)
- Create: `src/config/version.ts`(初始占位)

- [ ] **Step 1: 写初始 version.ts 占位文件**

创建 `src/config/version.ts`:

```typescript
// 本文件由 src-tauri/build.rs 自动生成,请勿手工编辑
// 真相源: src-tauri/Cargo.toml 的 version 字段
// 此为初始占位,首次 cargo build 后由 build.rs 覆盖为实际版本
export const CLIENT_VERSION = "0.1.0";
```

- [ ] **Step 2: 重写 build.rs 实现同步逻辑**

替换 `src-tauri/build.rs` 全部内容:

```rust
use std::fs;
use std::path::PathBuf;

fn main() {
    tauri_build::build();

    // 真相源:本 crate 的 Cargo.toml version 字段
    let pkg_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());

    // Cargo.toml 变更时重跑本脚本
    println!("cargo:rerun-if-changed=Cargo.toml");

    // 1) 同步 tauri.conf.json 的 version 字段
    sync_tauri_conf_version(&pkg_version);

    // 2) 同步 ../package.json 的 version 字段
    sync_package_json_version(&pkg_version);

    // 3) 生成前端 src/config/version.ts
    generate_version_ts(&pkg_version);
}

/// 读取并更新 tauri.conf.json 的 version 字段
fn sync_tauri_conf_version(version: &str) {
    let path = PathBuf::from("tauri.conf.json");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            println!("cargo:warning=无法读取 tauri.conf.json: {}", e);
            return;
        }
    };

    // 用 serde_json 解析并改写(保留 $schema 等字段)
    let mut json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            println!("cargo:warning=tauri.conf.json 解析失败: {}", e);
            return;
        }
    };

    if let Some(obj) = json.as_object_mut() {
        let current = obj.get("version").and_then(|v| v.as_str()).unwrap_or("");
        if current == version {
            return; // 无需改写
        }
        obj.insert("version".to_string(), serde_json::Value::String(version.to_string()));
        let pretty = serde_json::to_string_pretty(&json).unwrap_or_else(|_| content.clone());
        if let Err(e) = fs::write(&path, pretty) {
            println!("cargo:warning=写入 tauri.conf.json 失败: {}", e);
        }
    }
}

/// 读取并更新 ../package.json 的 version 字段
fn sync_package_json_version(version: &str) {
    let path = PathBuf::from("../package.json");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            println!("cargo:warning=无法读取 package.json: {}", e);
            return;
        }
    };

    let mut json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            println!("cargo:warning=package.json 解析失败: {}", e);
            return;
        }
    };

    if let Some(obj) = json.as_object_mut() {
        let current = obj.get("version").and_then(|v| v.as_str()).unwrap_or("");
        if current == version {
            return;
        }
        obj.insert("version".to_string(), serde_json::Value::String(version.to_string()));
        let pretty = serde_json::to_string_pretty(&json).unwrap_or_else(|_| content.clone());
        if let Err(e) = fs::write(&path, pretty) {
            println!("cargo:warning=写入 package.json 失败: {}", e);
        }
    }
}

/// 生成 ../src/config/version.ts
fn generate_version_ts(version: &str) {
    let path = PathBuf::from("../src/config/version.ts");
    let content = format!(
        "// 本文件由 src-tauri/build.rs 自动生成,请勿手工编辑\n\
         // 真相源: src-tauri/Cargo.toml 的 version 字段\n\
         export const CLIENT_VERSION = \"{}\";\n",
        version
    );

    // 若内容未变则不写(避免触发不必要的文件变更)
    if fs::read_to_string(&path).ok().as_deref() == Some(content.as_str()) {
        return;
    }

    if let Err(e) = fs::create_dir_all(path.parent().unwrap_or(PathBuf::from("../src/config")))
        .and_then(|_| fs::write(&path, content))
    {
        println!("cargo:warning=生成 version.ts 失败: {}", e);
    }
}
```

- [ ] **Step 3: 确认 src-tauri/Cargo.toml 已含 serde_json 依赖**

检查 `src-tauri/Cargo.toml` 的 `[dependencies]` 是否含 `serde_json`(实际上已有 `serde_json = "1"`)。若没有则添加,但当前确认已存在。

- [ ] **Step 4: 运行 build 触发同步并验证**

Run(项目根): `cd src-tauri; cargo build`
Expected: 编译通过,无 warning 关于 build.rs 失败。

- [ ] **Step 5: 验证 version.ts 已重新生成**

Read `src/config/version.ts`,确认内容为 `export const CLIENT_VERSION = "0.1.0";`(与 Cargo.toml 一致)。

- [ ] **Step 6: 验证版本号修改触发同步**

测试改版本号验证同步链路:把 `src-tauri/Cargo.toml` 的 `version = "0.1.0"` 改为 `version = "0.1.1"`,Run: `cd src-tauri; cargo build`,然后检查:
- `src-tauri/tauri.conf.json` 的 `"version"` 变为 `"0.1.1"`
- `package.json` 的 `"version"` 变为 `"0.1.1"`
- `src/config/version.ts` 的 `CLIENT_VERSION` 变为 `"0.1.1"`

验证后把 Cargo.toml 改回 `0.1.0`(或保留 0.1.1,看用户意图;此处为测试,建议改回)。

- [ ] **Step 7: 提交(用户手动执行)**

```
git add src-tauri/build.rs src/config/version.ts src-tauri/tauri.conf.json package.json
git commit -m "feat(client): build.rs 同步版本号到 tauri.conf.json/package.json/version.ts"
```

---

## Task 6: 前端类型与状态缓存 agentVersion

**Files:**
- Modify: `src/types/server.ts:42-65`(ServerConfig)
- Modify: `src/context/ServerManager.tsx:201-213`(连接成功后缓存)
- Modify: `src/stores/serversStore.ts`(updateServer 已支持,无需改)

- [ ] **Step 1: ServerConfig 增加 agentVersion 字段**

修改 `src/types/server.ts` 的 `ServerConfig` 接口(L42-65),在末尾(certFingerprint 后)加:

```typescript
  /** 服务器证书指纹(SHA-256,证书钉扎,SSH known_hosts 模式) */
  certFingerprint?: string;
  /** 已连接 Agent 的版本号(连接成功后由 ConnectionInfo 填入,未连接或旧 Agent 为 undefined) */
  agentVersion?: string;
}
```

- [ ] **Step 2: 前端 ConnectionInfo 类型镜像(若存在)**

搜索前端是否已有 `ConnectionInfo` 类型定义: `grep -rn "ConnectionInfo" src/`。若在 `src/types/server.ts` 或其他文件有镜像,补充 `agentVersion?: string` 字段。若没有,跳过本步(前端 `invoke<ConnectionInfo>` 可能用内联类型,Task 7 会处理)。

- [ ] **Step 3: ServerManager 连接成功后缓存 agentVersion**

修改 `src/context/ServerManager.tsx` L210-213 的连接成功处理:

```typescript
      log.info("连接成功:", info);

      setServerStatus(id, "connected", undefined, info.rttMs >= 0 ? info.rttMs : undefined);
      // 缓存 Agent 版本号到 store,供关于页显示
      updateServer(id, { agentVersion: info.agentVersion ?? undefined });
      setActiveServerId(id);
```

(`updateServer` 来自 serversStore,组件顶部应已解构;若未解构,在 useServersStore 解构处加 `updateServer`)

- [ ] **Step 4: 验证类型检查通过**

Run(项目根): `npx tsc --noEmit`
Expected: 无类型错误。若报 `info.agentVersion` 不存在,检查 Task 4 的 ConnectionInfo 改动是否生效;若前端有独立 ConnectionInfo 类型未更新,补 `agentVersion?: string`。

- [ ] **Step 5: 提交(用户手动执行)**

```
git add src/types/server.ts src/context/ServerManager.tsx
git commit -m "feat(client): ServerConfig 缓存 agentVersion 供关于页显示"
```

---

## Task 7: Settings 关于页动态显示版本号

**Files:**
- Modify: `src/apps/Settings.tsx:1025-1041`(关于页) 及组件顶部 activeServer 来源

- [ ] **Step 1: 确认 activeServer 在 Settings 组件的来源**

Read `src/apps/Settings.tsx` 开头部分,确认 `activeServer` 变量如何获取(应从 `useServersStore` 取 `servers.find(s => s.id === activeServerId)`)。记录其定义行号,用于 Step 3 引用 `activeServer?.agentVersion`。

- [ ] **Step 2: 关于页改用 CLIENT_VERSION 显示客户端版本**

修改 `src/apps/Settings.tsx` 关于页(L1025-1041)。先在文件顶部 import 区加:

```typescript
import { CLIENT_VERSION } from '../config/version';
```

然后把 L1032 的硬编码版本:

```typescript
              <div className="st-about-version">版本 0.1.0</div>
```

改为:

```typescript
              <div className="st-about-version">版本 {CLIENT_VERSION}</div>
```

- [ ] **Step 3: 关于页新增 Agent 版本显示区块**

在 L1032 的 `st-about-version` 之后、`st-about-desc` 之前插入 Agent 版本显示:

```typescript
              <div className="st-about-version">版本 {CLIENT_VERSION}</div>
              <div className="st-about-agent-version">
                {activeServer?.agentVersion
                  ? `Agent: v${activeServer.agentVersion}`
                  : 'Agent: 未连接'}
              </div>
              <div className="st-about-desc">
                基于 GNOME 设计系统的远程 Linux 服务器控制客户端
              </div>
```

- [ ] **Step 4: 加样式(可选,若 st-about-agent-version 需独立样式)**

若 `src/apps/Settings.css` 有 `.st-about-version` 样式,可复用或新增 `.st-about-agent-version` 同样式。若视觉上与 version 一致即可,跳过新样式(用相同 className 也行)。

- [ ] **Step 5: 验证前端构建**

Run(项目根): `npm run build`
Expected: `tsc && vite build` 全部通过,无错误。若报 `activeServer` 未定义,回到 Step 1 确认 activeServer 变量在组件作用域可见(可能需要在组件内从 store 取)。

- [ ] **Step 6: 手动 UI 验证**

Run: `npm run tauri dev`
打开 Settings → 关于页,确认:
- 客户端版本显示 `版本 0.1.0`(动态,非硬编码;改 Cargo.toml 重建后会变)
- Agent: 未连接(未连服务器时)

连接一台 Agent 后,重新打开关于页,确认显示 `Agent: v0.1.0`(或 Agent 实际版本)。

- [ ] **Step 7: 提交(用户手动执行)**

```
git add src/apps/Settings.tsx src/apps/Settings.css
git commit -m "feat(client): 关于页动态显示客户端版本与已连接 Agent 版本"
```

---

## Task 8: 发布脚本 release.ps1 与 sync-version.ps1

**Files:**
- Create: `scripts/release.ps1`
- Create: `scripts/sync-version.ps1`

- [ ] **Step 1: 创建 release.ps1**

创建 `scripts/release.ps1`:

```powershell
# release.ps1 - 同步修改版本号并触发派生文件同步
# 用法:
#   .\scripts\release.ps1 -AgentVersion 0.2.1            # 升 Agent PATCH(修复 bug)
#   .\scripts\release.ps1 -ClientVersion 0.3.0          # 升 Client MINOR(新增功能)
#   .\scripts\release.ps1 -AgentVersion 0.2.1 -ClientVersion 0.3.0
# 规则: PATCH+1=修复 bug, MINOR+1=新增功能, MAJOR+1=协议不兼容
# 注意: 本脚本不执行任何 git 操作,仅修改文件 + 打印建议命令,由用户手动执行 git

param(
    [string]$AgentVersion,
    [string]$ClientVersion
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot

if (-not $AgentVersion -and -not $ClientVersion) {
    Write-Error "至少指定 -AgentVersion 或 -ClientVersion 之一"
    exit 1
}

function Update-CargoVersion($tomlPath, $newVersion) {
    if (-not (Test-Path $tomlPath)) { Write-Error "找不到: $tomlPath"; exit 1 }
    $content = Get-Content $tomlPath -Raw
    $updated = $content -replace '^(version\s*=\s*)"[^"]*"', "`$1`"$newVersion`""
    if ($updated -eq $content) { Write-Error "$tomlPath 未匹配到 version 行"; exit 1 }
    Set-Content -Path $tomlPath -Value $updated -NoNewline
    Write-Host "  $($tomlPath.Replace("$repoRoot\", ""))  -> $newVersion"
}

Write-Host "已更新基础版本号:"
if ($AgentVersion) {
    Update-CargoVersion "$repoRoot\agent\Cargo.toml" $AgentVersion
}
if ($ClientVersion) {
    Update-CargoVersion "$repoRoot\src-tauri\Cargo.toml" $ClientVersion
}

# 触发 build.rs 同步派生文件(tauri.conf.json / package.json / version.ts)
if ($ClientVersion) {
    Write-Host "`n触发 Client build.rs 同步派生文件..."
    Push-Location "$repoRoot\src-tauri"
    try { cargo build 2>&1 | Out-Host }
    finally { Pop-Location }
    Write-Host "已同步: tauri.conf.json / package.json / src/config/version.ts"
}

Write-Host @"

请手动执行以下命令完成提交:
  git add agent/Cargo.toml src-tauri/Cargo.toml src-tauri/tauri.conf.json package.json src/config/version.ts
  git commit -m "release: $($(if ($AgentVersion) { "agent v$AgentVersion" }), $(if ($ClientVersion) { "client v$ClientVersion" }) -join ', ')"
"@
```

- [ ] **Step 2: 创建 sync-version.ps1**

创建 `scripts/sync-version.ps1`:

```powershell
# sync-version.ps1 - 强制重新生成 Client 派生文件(version.ts / tauri.conf.json / package.json)
# 用法: .\scripts\sync-version.ps1
# 场景: 拉取新 commit 后 Cargo.toml 变了但 version.ts 未更新时运行

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot

Write-Host "触发 Client build.rs 重新生成派生文件..."
Push-Location "$repoRoot\src-tauri"
try { cargo build 2>&1 | Out-Host }
finally { Pop-Location }
Write-Host "已同步: src-tauri/tauri.conf.json / package.json / src/config/version.ts"
```

- [ ] **Step 3: 验证 release.ps1 修改 Agent 版本号**

Run(项目根,PowerShell): `.\scripts\release.ps1 -AgentVersion 0.1.1`
Expected:
- 输出 `agent/Cargo.toml -> 0.1.1`
- 打印 git 提交建议命令(未执行 git)
- 检查 `agent/Cargo.toml` 的 version 已变 `0.1.1`

验证后改回:`.\scripts\release.ps1 -AgentVersion 0.1.0`

- [ ] **Step 4: 验证 release.ps1 修改 Client 版本号并触发同步**

Run: `.\scripts\release.ps1 -ClientVersion 0.1.1`
Expected:
- 输出 `src-tauri/Cargo.toml -> 0.1.1`
- 运行 `cargo build` 触发 build.rs
- 输出同步 tauri.conf.json / package.json / version.ts
- 检查 `src-tauri/tauri.conf.json`、`package.json`、`src/config/version.ts` 均为 `0.1.1`

验证后改回:`.\scripts\release.ps1 -ClientVersion 0.1.0`

- [ ] **Step 5: 提交(用户手动执行)**

```
git add scripts/release.ps1 scripts/sync-version.ps1
git commit -m "chore: 添加版本号同步与发布脚本 release.ps1 / sync-version.ps1"
```

---

## Self-Review 结果

**1. Spec coverage:**
- 目标1(版本号修改纪律):Task 8 release.ps1 + spec 4.2.2 规范 ✓
- 目标2(单一真相源):Task 5 build.rs 同步 ✓
- 目标3(Agent 暴露):Task 1+2+3 ✓
- 目标4(Client 暴露):Task 5+7 ✓
- 目标5(Agent 透传):Task 1+2(Agent 侧)+ Task 4+6+7(Client 侧) ✓
- 目标6(独立版本号):release.ps1 支持分别指定 -AgentVersion/-ClientVersion ✓
- 非目标(git hash/build_info/sd_notify):未出现在任何 Task ✓

**2. Placeholder 扫描:** 无 TBD/TODO;每个 step 有具体代码或命令。Task 7 Step 1 "记录行号"是实现时定位,但 activeServer 来源明确(从 store 取),不算 placeholder。Task 6 Step 2 "若存在则补充"有条件分支但给出了具体字段。

**3. Type consistency:**
- `agent_version: Option<String>`(Rust)→ `agentVersion?: string`(TS,camelCase via serde rename)在 Task 4(ConnectionInfo rename)、Task 6(ServerConfig)、Task 7(读取 activeServer.agentVersion)一致 ✓
- `CLIENT_VERSION` 在 Task 5(生成)与 Task 7(import)一致 ✓
- `send_auth_response` 签名在 Task 2 所有调用点都传第 6 参数 ✓
- `perform_pubkey_auth` 返回 `(session_id, agent_version)` 元组,Task 4 Step 5 定义、Step 6 解构一致 ✓

**4. 补充确认:** Task 4 Step 3 的 `#[serde(default)]` 与 Task 1 Step 3 的 `#[serde(skip_serializing_if = "Option::is_none")]` 不冲突——前者控制反序列化缺失字段时默认 None,后者控制序列化 None 时省略字段。两者配合实现向后兼容。
