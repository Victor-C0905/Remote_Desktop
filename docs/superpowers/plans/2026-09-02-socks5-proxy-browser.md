# SOCKS5 over QUIC 服务器视角浏览器 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用户通过 Quirel 客户端 spawn 系统浏览器，浏览器流量经本地 SOCKS5 监听 → QUIC 隧道 → Agent → 目标网站，实现"以服务器网络视角浏览网页"。

**Architecture:** 代理连接复用现有已认证 QUIC 连接的多路复用流。代理流首帧为 `ProxyOpen` envelope（长度前缀 JSON），Agent 回 `ProxyOpenResponse` 后，流剩余字节做 TCP 盲转（`tokio::io::copy` 双向）。客户端 Rust 端实现 SOCKS5 服务端（仅 CONNECT，远程 DNS）+ 浏览器进程管理。前端 TopBar 加"浏览"入口。

**Tech Stack:** quinn（已有）、tokio TcpStream（已有 tokio/net feature）、quirel-protocol 共享 crate、std::process::Command spawn 浏览器。

**用户规则（必须遵守）:** 保留注释；不执行任何 git 操作（不提交/不回退），每个任务完成只提示用户提交。

---

## 帧协议设计（关键决策）

代理流与现有命令流**共用** `read_message`/`write_message`（4 字节小端长度前缀 + Envelope JSON），**只有首两帧**是 envelope：

```
客户端                                Agent
  │── write_message(ProxyOpen{host,port}) ──▶│  stream 首帧
  │◀─ write_message(ProxyOpenResponse) ──────│  stream 第二帧
  │                                           │
  │◀═══════ 之后原始字节双向透传 ════════════▶│  与 TcpStream copy
```

- 域名由 Agent 端 `TcpStream::connect((host, port))` 解析 → **远程 DNS，无泄漏**
- 浏览器对 QUIC 完全无感知，只认 `--proxy-server=socks5://127.0.0.1:<port>`

---

### Task 1: quirel-protocol 新增 ProxyOpen 协议变体

**Files:**
- Modify: `quirel-protocol/src/envelope.rs`（在 `CurrentUserResponse` 变体附近，约 L199）
- Test: `quirel-protocol/tests/wire_compat.rs`

- [ ] **Step 1: 写失败的 golden 测试**

在 `quirel-protocol/tests/wire_compat.rs` 末尾追加：

```rust
#[test]
fn proxy_open_wire_format() {
    let env = Envelope::new(9, Payload::ProxyOpen { host: "example.com".to_string(), port: 443 });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":9,"payload":{"type":"proxy_open","data":{"host":"example.com","port":443}}}"#
    );
    // 线上往返：编码后必须可解码（代理流首帧的实际路径）
    let env2 = roundtrip(&env);
    match env2.payload {
        Payload::ProxyOpen { host, port } => {
            assert_eq!(host, "example.com");
            assert_eq!(port, 443);
        }
        _ => panic!("变体不匹配"),
    }
}

#[test]
fn proxy_open_response_wire_format() {
    let env = Envelope::new(9, Payload::ProxyOpenResponse { success: true, error: None });
    let json = serde_json::to_string(&env).unwrap();
    assert_eq!(
        json,
        r#"{"request_id":9,"payload":{"type":"proxy_open_resp","data":{"success":true}}}"#
    );
    // 失败场景
    let fail = Envelope::new(9, Payload::ProxyOpenResponse {
        success: false,
        error: Some("connection refused".to_string()),
    });
    let fail2 = roundtrip(&fail);
    match fail2.payload {
        Payload::ProxyOpenResponse { success, error } => {
            assert!(!success);
            assert_eq!(error.as_deref(), Some("connection refused"));
        }
        _ => panic!("变体不匹配"),
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cd quirel-protocol && cargo test --test wire_compat`
Expected: 编译错误 `no variant named ProxyOpen`

- [ ] **Step 3: 实现变体**

`quirel-protocol/src/envelope.rs` 的 `Payload` 枚举中，紧跟 `CurrentUserResponse` 变体后添加：

```rust
    // ===== SOCKS5 代理协议（服务器视角浏览）=====
    // 代理流首帧：客户端告知目标地址（域名由 Agent 端解析，远程 DNS）
    #[serde(rename = "proxy_open")]
    ProxyOpen { host: String, port: u16 },

    // 代理流第二帧：Agent 回报 TCP 连接结果，之后流内字节全部透传
    #[serde(rename = "proxy_open_resp")]
    ProxyOpenResponse {
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        error: Option<String>,
    },
```

同时在同文件的 `type_name()` 的 match 里补两个分支：

```rust
            Payload::ProxyOpen { .. } => "ProxyOpen",
            Payload::ProxyOpenResponse { .. } => "ProxyOpenResponse",
```

- [ ] **Step 4: 运行确认通过**

Run: `cd quirel-protocol && cargo test`
Expected: 10 passed（原 8 + 新 2）

- [ ] **Step 5: 提示用户提交**

```
git add quirel-protocol/
git commit -m "feat(protocol): 新增 ProxyOpen/ProxyOpenResponse 代理流帧协议"
```
（只提示，不执行）

---

### Task 2: Agent 端代理中继

**Files:**
- Create: `agent/src/server/proxy.rs`
- Modify: `agent/src/server/mod.rs`（导出新模块；若已有 `pub mod quic;` 则在旁边加 `pub mod proxy;`）
- Modify: `agent/src/server/quic.rs`（`handle_stream` 的 payload match 中加 `ProxyOpen` 分支，match 位于约 L1127）

- [ ] **Step 1: 实现 `agent/src/server/proxy.rs`**

```rust
//! SOCKS5 代理流中继
//!
//! 代理流协议：首帧 ProxyOpen（envelope，长度前缀 JSON），Agent 回 ProxyOpenResponse，
//! 之后流内全部字节与目标 TCP 连接双向透传（盲转，不理解内容）。
//! 域名在本端解析（远程 DNS，客户端无 DNS 泄漏）。

use quinn::{RecvStream, SendStream};
use std::sync::Arc;

use crate::auth::session::UserSession;
use quirel_protocol::{Envelope, Payload};

use super::quic::{read_message, write_message};

/// 处理一条代理流：连接目标并双向透传
///
/// 进入此函数前，调用方已完成 ProxyOpen envelope 解析。
/// QUIC 流关闭（连接断开）时 copy 任务自动退出，TCP 连接随之关闭——
/// 与连接级清理天然联动，无需注册额外清理逻辑。
pub async fn handle_proxy_relay(
    mut send: SendStream,
    mut recv: RecvStream,
    request_id: u32,
    host: String,
    port: u16,
    session: &UserSession,
) -> Result<(), anyhow::Error> {
    tracing::info!(
        "[Proxy] 代理流打开: username={}, target={}:{}, uid={}",
        session.username, host, port, session.uid
    );

    // 连接目标（域名在本端解析 = 远程 DNS）
    let tcp = match tokio::net::TcpStream::connect((host.as_str(), port)).await {
        Ok(tcp) => tcp,
        Err(e) => {
            // 回报失败，客户端据此回复 SOCKS5 拒绝
            let resp = Envelope::new(
                request_id,
                Payload::ProxyOpenResponse {
                    success: false,
                    error: Some(format!("连接目标失败: {}", e)),
                },
            );
            write_message(&mut send, &resp.encode()?).await?;
            tracing::warn!("[Proxy] 目标连接失败: {}:{}: {}", host, port, e);
            return Ok(());
        }
    };

    // 回报成功，之后开始透传
    let resp = Envelope::new(request_id, Payload::ProxyOpenResponse { success: true, error: None });
    write_message(&mut send, &resp.encode()?).await?;

    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();
    // 双向透传：任一方向结束即退出（半关闭语义交给浏览器/TCP 自己处理）
    tokio::select! {
        r = tokio::io::copy(&mut recv, &mut tcp_tx) => {
            // 客户端侧结束，通知目标服务器我们发完了
            let _ = tcp_tx.shutdown().await;
            if let Err(e) = r {
                tracing::debug!("[Proxy] 客户端→目标复制结束: {}", e);
            }
        }
        r = tokio::io::copy(&mut tcp_rx, &mut send) => {
            // 目标侧结束，结束 QUIC 发送流
            let _ = send.finish();
            if let Err(e) = r {
                tracing::debug!("[Proxy] 目标→客户端复制结束: {}", e);
            }
        }
    }

    tracing::info!("[Proxy] 代理流关闭: username={}, target={}:{}", session.username, host, port);
    Ok(())
}
```

- [ ] **Step 2: 导出模块**

`agent/src/server/mod.rs` 中，在 `pub mod quic;`（或已有模块声明）旁添加：

```rust
pub mod proxy;
```

注意：若 `read_message`/`write_message` 在 `quic.rs` 中是私有函数（`async fn`），需将其改为 `pub(crate) async fn`（两处：约 L1585 的 `read_message` 和约 L1607 的 `write_message`）。

- [ ] **Step 3: 在 `handle_stream` 的 match 中挂分支**

`agent/src/server/quic.rs` 的 `handle_stream` 内，找到 `match &envelope.payload {`（约 L1127），在 `Payload::Subscribe` 分支之后、其他命令分支之前添加：

```rust
        // ===== SOCKS5 代理流 =====
        // 首帧为 ProxyOpen：连接目标后流内字节全部透传，不再走 envelope 解析
        Payload::ProxyOpen { host, port } => {
            // 订阅流注册到连接上下文（连接断开时统一清理）
            ctx.register_stream_id(stream_id).await;
            let host = host.clone();
            let port = *port;
            let session_clone = session.clone();
            let req_id = envelope.request_id;
            drop(envelope);
            return crate::server::proxy::handle_proxy_relay(
                send, recv, req_id, host, port, &session_clone,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e));
        }
```

注意：`session` 参数在 `handle_stream` 签名中是 `&UserSession`，`UserSession` 需可 clone（检查 `agent/src/auth/session.rs`，若未 derive Clone 则为它加 `#[derive(Clone)]`）。

- [ ] **Step 4: 编译并跑全部测试**

Run: `cd agent && cargo check && cargo test`
Expected: 编译通过，原 84 个测试全绿（无回归）

- [ ] **Step 5: 提示用户提交**

```
git add agent/
git commit -m "feat(agent): 代理流中继——ProxyOpen 首帧后 TCP 盲转"
```

---

### Task 3: 客户端 SOCKS5 协议实现（纯函数 + TDD）

**Files:**
- Create: `src-tauri/src/proxy/mod.rs`
- Create: `src-tauri/src/proxy/socks5.rs`

- [ ] **Step 1: 写失败的 SOCKS5 解析测试**

`src-tauri/src/proxy/socks5.rs`：

```rust
//! SOCKS5 服务端协议实现（RFC 1928 子集）
//!
//! 只实现：无认证 + CONNECT 命令 + IPv4/域名/IPv6 目标地址。
//! BIND / UDP ASSOCIATE / 用户名密码认证均不支持（浏览器场景用不到，YAGNI）。

use std::io::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// CONNECT 请求解析出的目标地址
#[derive(Debug, Clone, PartialEq)]
pub struct Socks5Target {
    pub host: String,
    pub port: u16,
}

const SOCKS5_VERSION: u8 = 0x05;

/// 读 greeting 并回复"无需认证"。
/// 浏览器发起：[VER=5, NMETHODS, METHODS...]
pub async fn socks5_greeting(stream: &mut TcpStream) -> Result<(), String> {
    let ver = stream.read_u8().await.map_err(err("读协议版本"))?;
    if ver != SOCKS5_VERSION {
        return Err(format!("仅支持 SOCKS5，收到版本 {}", ver));
    }
    let nmethods = stream.read_u8().await.map_err(err("读方法数"))?;
    let mut methods = vec![0u8; nmethods as usize];
    stream.read_exact(&mut methods).await.map_err(err("读方法列表"))?;
    // 回复：无需认证（0x00）
    stream.write_all(&[SOCKS5_VERSION, 0x00]).await.map_err(err("回复 greeting"))?;
    Ok(())
}

/// 读 CONNECT 请求并解析目标地址（域名场景直接透传域名 → 远程 DNS）。
pub async fn socks5_read_connect(stream: &mut TcpStream) -> Result<Socks5Target, String> {
    let ver = stream.read_u8().await.map_err(err("读版本"))?;
    if ver != SOCKS5_VERSION {
        return Err(format!("CONNECT 版本错误: {}", ver));
    }
    let cmd = stream.read_u8().await.map_err(err("读命令"))?;
    if cmd != 0x01 {
        // 仅支持 CONNECT
        socks5_reply(stream, 0x07).await?; // 0x07 command not supported
        return Err(format!("仅支持 CONNECT，收到命令 0x{:02x}", cmd));
    }
    let _rsv = stream.read_u8().await.map_err(err("读保留位"))?;
    let atyp = stream.read_u8().await.map_err(err("读地址类型"))?;

    let host = match atyp {
        0x01 => {
            // IPv4：4 字节，转点分十进制
            let mut b = [0u8; 4];
            stream.read_exact(&mut b).await.map_err(err("读 IPv4"))?;
            format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
        }
        0x03 => {
            // 域名：1 字节长度 + 域名（远程解析的关键路径）
            let len = stream.read_u8().await.map_err(err("读域名长度"))?;
            let mut b = vec![0u8; len as usize];
            stream.read_exact(&mut b).await.map_err(err("读域名"))?;
            String::from_utf8(b).map_err(|_| "域名不是合法 UTF-8".to_string())?
        }
        0x04 => {
            // IPv6：16 字节，转标准格式
            let mut b = [0u8; 16];
            stream.read_exact(&mut b).await.map_err(err("读 IPv6"))?;
            let ip: std::net::Ipv6Addr = b.into();
            ip.to_string()
        }
        other => return Err(format!("未知地址类型 0x{:02x}", other)),
    };

    let port = stream
        .read_u16()
        .await
        .map_err(err("读端口"))?;

    Ok(Socks5Target { host, port })
}

/// 回复 CONNECT 结果。code: 0x00 成功，其余为 RFC 1928 错误码。
pub async fn socks5_reply(stream: &mut TcpStream, code: u8) -> Result<(), String> {
    // 回复格式：VER, REP, RSV, ATYP=IPv4, BND.ADDR=0.0.0.0, BND.PORT=0
    stream
        .write_all(&[SOCKS5_VERSION, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
        .map_err(err("回复 CONNECT 结果"))?;
    Ok(())
}

fn err(ctx: &str) -> impl Fn(Error) -> String + '_ {
    move |e| format!("{}: {}", ctx, e)
}
```

`src-tauri/src/proxy/mod.rs`（先建骨架，Task 4 扩充）：

```rust
//! SOCKS5 over QUIC 代理会话管理

pub mod socks5;
```

- [ ] **Step 2: 写字节流级测试**

在 `src-tauri/src/proxy/socks5.rs` 末尾追加（异步测试需要 `#[tokio::test]`）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    /// 用内存管道模拟 TcpStream 双向字节流
    async fn pipe() -> (TcpStream, TcpStream) {
        // 用真实 loopback 建一对连接（tokio 的 TcpStream 没有内存 mock）
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (_server_side_raw, mut client_side) = listener.accept().await.unwrap();
        // client_side 模拟"浏览器侧"；返回 (浏览器侧, 代理侧)
        let _ = &mut client_side;
        // 交换：client 是我们写入请求的一端
        Ok::<(), ()>(()).ok();
        unreachable!() // 占位，见下方真实实现
    }

    #[tokio::test]
    async fn greeting_and_connect_domain() {
        // 一对真实 TCP 连接：a 端模拟浏览器写请求，b 端被测函数读取
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut a = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (mut b, _) = listener.accept().await.unwrap();

        // 浏览器发 greeting：支持无认证
        a.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        socks5_greeting(&mut b).await.unwrap();
        let mut buf = [0u8; 2];
        tokio::io::AsyncReadExt::read_exact(&mut a, &mut buf).await.unwrap();
        assert_eq!(buf, [0x05, 0x00]);

        // 浏览器发 CONNECT example.com:443（域名类型）
        a.write_all(&[0x05, 0x01, 0x00, 0x03, 11])
            .await.unwrap();
        a.write_all(b"example.com").await.unwrap();
        a.write_all(&443u16.to_be_bytes()).await.unwrap();
        let target = socks5_read_connect(&mut b).await.unwrap();
        assert_eq!(
            target,
            Socks5Target { host: "example.com".to_string(), port: 443 }
        );
    }

    #[tokio::test]
    async fn connect_ipv4() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut a = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (mut b, _) = listener.accept().await.unwrap();

        a.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        socks5_greeting(&mut b).await.unwrap();
        let _ = tokio::io::AsyncReadExt::read(&mut a, &mut [0u8; 2]).await;

        // CONNECT 10.0.0.1:80
        a.write_all(&[0x05, 0x01, 0x00, 0x01, 10, 0, 0, 1])
            .await.unwrap();
        a.write_all(&80u16.to_be_bytes()).await.unwrap();
        let target = socks5_read_connect(&mut b).await.unwrap();
        assert_eq!(
            target,
            Socks5Target { host: "10.0.0.1".to_string(), port: 80 }
        );
    }

    #[tokio::test]
    async fn reject_non_connect_command() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut a = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (mut b, _) = listener.accept().await.unwrap();

        a.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        socks5_greeting(&mut b).await.unwrap();
        let _ = tokio::io::AsyncReadExt::read(&mut a, &mut [0u8; 2]).await;

        // BIND 命令（0x02）应被拒绝并回复 0x07
        a.write_all(&[0x05, 0x02, 0x00, 0x01, 0, 0, 0, 0]).await.unwrap();
        a.write_all(&0u16.to_be_bytes()).await.unwrap();
        assert!(socks5_read_connect(&mut b).await.is_err());
        let mut reply = [0u8; 10];
        tokio::io::AsyncReadExt::read_exact(&mut a, &mut reply).await.unwrap();
        assert_eq!(reply[1], 0x07);
    }
}
```

（注意：上面 `pipe()` 辅助函数实际未被使用——实现时直接删除它，三个测试各自建立 loopback 连接对。）

- [ ] **Step 3: 运行测试**

Run: `cd src-tauri && cargo test socks5`
Expected: 3 passed

- [ ] **Step 4: 提示用户提交**

```
git add src-tauri/src/proxy/
git commit -m "feat(client): SOCKS5 协议解析（无认证 + CONNECT + 远程 DNS 透传）"
```

---

### Task 4: 客户端代理会话管理 + 浏览器 spawn + Tauri 命令

**Files:**
- Modify: `src-tauri/src/proxy/mod.rs`（扩充）
- Create: `src-tauri/src/proxy/browser.rs`
- Modify: `src-tauri/src/lib.rs`（约 L548 `.manage(...)` 处与约 L570 `invoke_handler` 列表）

- [ ] **Step 1: 浏览器探测与 spawn**

`src-tauri/src/proxy/browser.rs`：

```rust
//! 系统浏览器探测与 spawn
//!
//! 通过 --proxy-server 启动参数让浏览器流量走本地 SOCKS5；
//! --user-data-dir 强制独立 profile（否则单实例机制会忽略代理参数）。

use std::path::PathBuf;
use std::process::{Child, Command};

/// 探测顺序：Edge（Windows 必装）→ Chrome
const BROWSER_CANDIDATES: &[&str] = &[
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
];

/// 探测可用的浏览器路径，返回 (路径, 名称)
pub fn detect_browser() -> Option<(PathBuf, &'static str)> {
    for (path, name) in BROWSER_CANDIDATES
        .iter()
        .zip(["edge", "chrome", "chrome", "chrome"])
    {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some((p, name));
        }
    }
    None
}

/// spawn 浏览器：所有流量走本地 SOCKS5 监听
pub fn spawn_browser(browser_path: &PathBuf, socks_port: u16, profile_dir: &PathBuf) -> Result<Child, String> {
    Command::new(browser_path)
        .arg(format!("--proxy-server=socks5://127.0.0.1:{}", socks_port))
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("about:blank")
        .spawn()
        .map_err(|e| format!("启动浏览器失败: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_browser_returns_existing() {
        // Windows 开发环境必有 Edge 或 Chrome；找不到是环境异常
        let result = detect_browser();
        assert!(result.is_some(), "未探测到任何浏览器");
        let (path, _) = result.unwrap();
        assert!(path.exists());
    }
}
```

- [ ] **Step 2: 会话管理 + 命令（扩充 `src-tauri/src/proxy/mod.rs`）**

```rust
//! SOCKS5 over QUIC 代理会话管理
//!
//! 生命周期：start_proxy_session 起本地 SOCKS5 监听 + spawn 浏览器；
//! QUIC 连接断开 → 所有代理流关闭 → 浏览器整体断网（设计使然，与登录态绑定）。

pub mod browser;
pub mod socks5;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::connection::ConnectionManager;
use quirel_protocol::{Envelope, Payload};

/// 返回给前端的会话信息
#[derive(serde::Serialize)]
pub struct ProxySessionInfo {
    pub port: u16,
    pub browser: String,
}

struct ProxySession {
    port: u16,
    /// 监听任务句柄（abort 即关闭监听）
    listener_task: tokio::task::JoinHandle<()>,
    /// 浏览器子进程（stop 时 kill）
    browser_child: Arc<Mutex<Option<Child>>>,
}

/// 代理会话管理器（Tauri managed state）
pub struct ProxySessionManager {
    sessions: Mutex<HashMap<String, ProxySession>>,
}

impl ProxySessionManager {
    pub fn new() -> Self {
        Self { sessions: Mutex::new(HashMap::new()) }
    }
}

impl Default for ProxySessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// QUIC 流上的长度前缀帧读写（与 Agent 端 read_message/write_message 同构）
async fn write_frame(send: &mut quinn::SendStream, data: &[u8]) -> Result<(), String> {
    send.write_all(&(data.len() as u32).to_le_bytes())
        .await
        .map_err(|e| format!("写帧头失败: {}", e))?;
    send.write_all(data).await.map_err(|e| format!("写帧失败: {}", e))
}

async fn read_frame(recv: &mut quinn::RecvStream) -> Result<Option<Vec<u8>>, String> {
    let mut len_buf = [0u8; 4];
    match recv.read_exact(&mut len_buf).await {
        Ok(()) => {}
        Err(quinn::ReadExactError::FinishedEarly(_)) => return Ok(None),
        Err(e) => return Err(format!("读帧头失败: {}", e)),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut data = vec![0u8; len];
    recv.read_exact(&mut data).await.map_err(|e| format!("读帧失败: {}", e))?;
    Ok(Some(data))
}

/// 单条 SOCKS5 连接的处理：握手 → 开 QUIC 代理流 → 双向透传
async fn handle_socks5_conn(
    mut tcp: TcpStream,
    quic_conn: Arc<quinn::Connection>,
) -> Result<(), String> {
    // 1. SOCKS5 握手（greeting + CONNECT）
    socks5::socks5_greeting(&mut tcp).await?;
    let target = socks5::socks5_read_connect(&mut tcp).await?;

    // 2. 开 QUIC 双向流，发 ProxyOpen 首帧
    let (mut quic_tx, mut quic_rx) = quic_conn
        .open_bi()
        .await
        .map_err(|e| format!("开代理流失败: {}", e))?;
    let open = Envelope::new(
        0, // 代理流内 request_id 无匹配语义，固定 0
        Payload::ProxyOpen { host: target.host, port: target.port },
    );
    write_frame(&mut quic_tx, &open.encode()?).await?;

    // 3. 读 ProxyOpenResponse
    let resp_data = read_frame(&mut quic_rx).await?.ok_or("代理流提前关闭")?;
    let resp = Envelope::decode(&resp_data)?;
    match resp.payload {
        Payload::ProxyOpenResponse { success: true, .. } => {}
        Payload::ProxyOpenResponse { success: false, error } => {
            // Agent 连不上目标 → 回 SOCKS5 拒绝（0x01 general failure）
            socks5::socks5_reply(&mut tcp, 0x01).await?;
            return Err(error.unwrap_or_else(|| "目标连接失败".into()));
        }
        other => {
            socks5::socks5_reply(&mut tcp, 0x01).await?;
            return Err(format!("意外响应: {}", other.type_name()));
        }
    }

    // 4. 回 SOCKS5 成功，之后双向透传
    socks5::socks5_reply(&mut tcp, 0x00).await?;
    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();
    // 双向透传：join! 两方向独立跑完（与 Agent 端一致，半关闭互不截断）
    let a = async {
        let r = tokio::io::copy(&mut tcp_rx, &mut quic_tx).await;
        let _ = quic_tx.finish();
        if let Err(e) = r {
            tracing::debug!("[Proxy] 浏览器→Agent 透传结束: {}", e);
        }
    };
    let b = async {
        let r = tokio::io::copy(&mut quic_rx, &mut tcp_tx).await;
        let _ = tcp_tx.shutdown().await;
        if let Err(e) = r {
            tracing::debug!("[Proxy] Agent→浏览器 透传结束: {}", e);
        }
    };
    tokio::join!(a, b);
    Ok(())
}

/// 启动代理会话：SOCKS5 监听 + 浏览器
#[tauri::command]
#[tracing::instrument(skip(app))]
pub async fn proxy_start_session(
    server_id: String,
    app: AppHandle,
) -> Result<ProxySessionInfo, String> {
    let manager: State<ConnectionManager> = app.state();
    let quic_conn = {
        let conns = manager.connections.lock().unwrap();
        conns
            .get(&server_id)
            .and_then(|c| c.quic_conn.clone())
            .ok_or("服务器未连接或连接不可用")?
    };

    // 会话已存在：只 spawn 新浏览器窗口（同 profile），不重建监听
    {
        let sessions = app.state::<ProxySessionManager>();
        let existing = sessions.sessions.lock().unwrap();
        if let Some(sess) = existing.get(&server_id) {
            let port = sess.port;
            drop(existing);
            if let Some((path, _)) = browser::detect_browser() {
                let profile = browser_profile_dir(&app)?;
                let _ = browser::spawn_browser(&path, port, &profile)?;
            }
            return Ok(ProxySessionInfo { port, browser: "existing".into() });
        }
    }

    // 探测浏览器
    let (browser_path, browser_name) =
        browser::detect_browser().ok_or("未找到 Edge/Chrome，无法启动浏览")?;

    // 绑定本地随机端口
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("SOCKS5 监听绑定失败: {}", e))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    // 监听循环
    let quic_for_accept = quic_conn.clone();
    let listener_task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((tcp, _addr)) => {
                    let quic = quic_for_accept.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_socks5_conn(tcp, quic).await {
                            tracing::debug!("[Proxy] SOCKS5 连接结束: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("[Proxy] SOCKS5 监听退出: {}", e);
                    break;
                }
            }
        }
    });

    // spawn 浏览器
    let profile = browser_profile_dir(&app)?;
    let child = browser::spawn_browser(&browser_path, port, &profile)?;

    // 注册会话
    let sessions: State<ProxySessionManager> = app.state();
    sessions.sessions.lock().unwrap().insert(
        server_id.clone(),
        ProxySession {
            port,
            listener_task,
            browser_child: Arc::new(Mutex::new(Some(child))),
        },
    );

    tracing::info!(
        "[Proxy] 会话已启动: server_id={}, socks_port={}, browser={}",
        server_id, port, browser_name
    );
    Ok(ProxySessionInfo { port, browser: browser_name.to_string() })
}

/// 停止代理会话：关监听 + kill 浏览器
#[tauri::command]
#[tracing::instrument(skip(app))]
pub async fn proxy_stop_session(server_id: String, app: AppHandle) -> Result<(), String> {
    let sessions: State<ProxySessionManager> = app.state();
    let session = sessions.sessions.lock().unwrap().remove(&server_id);
    if let Some(sess) = session {
        sess.listener_task.abort();
        if let Some(mut child) = sess.browser_child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        tracing::info!("[Proxy] 会话已停止: server_id={}, port={}", server_id, sess.port);
    }
    Ok(())
}

/// 浏览器独立 profile 目录（app cache 下）
fn browser_profile_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("获取缓存目录失败: {}", e))?;
    let profile = dir.join("browser-profile");
    std::fs::create_dir_all(&profile).map_err(|e| format!("创建 profile 目录失败: {}", e))?;
    Ok(profile)
}
```

- [ ] **Step 3: lib.rs 注册**

`src-tauri/src/lib.rs` 中，`mod` 声明区添加：

```rust
mod proxy;
```

`.manage(connection::ConnectionManager::new())`（约 L548）之后添加：

```rust
        .manage(proxy::ProxySessionManager::new())
```

`invoke_handler` 列表（约 L570）末尾添加：

```rust
            proxy::proxy_start_session,
            proxy::proxy_stop_session,
```

- [ ] **Step 4: 编译 + 测试**

Run: `cd src-tauri && cargo check && cargo test proxy`
Expected: 编译通过，browser::tests::detect_browser_returns_existing 及 socks5 测试全绿

- [ ] **Step 5: 提示用户提交**

```
git add src-tauri/src/
git commit -m "feat(client): SOCKS5 会话管理 + 系统浏览器 spawn（proxy_start/stop_session）"
```

---

### Task 5: 前端"浏览"入口

**Files:**
- Modify: `src/shell/TopBar/TopBar.tsx`（已连接指示器区域，约 L54-L63）

- [ ] **Step 1: 添加浏览按钮**

在 TopBar.tsx 顶部导入区添加：

```typescript
import { invoke } from "@tauri-apps/api/core";
```

组件内（`const { activeServer } = useServerManager();` 之后）添加状态与处理函数：

```typescript
  const [proxyRunning, setProxyRunning] = useState(false);
  const [proxyBusy, setProxyBusy] = useState(false);

  const toggleProxy = async () => {
    if (!activeServer || proxyBusy) return;
    setProxyBusy(true);
    try {
      if (proxyRunning) {
        await invoke("proxy_stop_session", { serverId: activeServer.id });
        setProxyRunning(false);
      } else {
        const info = await invoke<{ port: number; browser: string }>(
          "proxy_start_session", { serverId: activeServer.id });
        setProxyRunning(true);
        console.info(`[Proxy] 浏览会话已启动: ${info.browser}, SOCKS5 端口 ${info.port}`);
      }
    } catch (e) {
      console.error("[Proxy] 浏览会话操作失败:", e);
      alert(`浏览会话失败: ${e}`);
    } finally {
      setProxyBusy(false);
    }
  };
```

在已连接状态显示（`{activeServer?.status === "connected" ? "已连接" : ...}` 所在容器）旁添加按钮，使用与相邻元素一致的样式模式：

```tsx
        {activeServer?.status === "connected" && (
          <button
            type="button"
            onClick={toggleProxy}
            disabled={proxyBusy}
            title={proxyRunning ? "停止浏览会话（关闭浏览器与代理）" : "通过服务器网络浏览网页"}
            style={{
              marginLeft: 8,
              padding: "2px 10px",
              fontSize: 12,
              borderRadius: 6,
              cursor: proxyBusy ? "default" : "pointer",
              border: "1px solid rgba(255,255,255,0.25)",
              background: proxyRunning ? "rgba(87,117,144,0.55)" : "transparent",
              color: "inherit",
            }}
          >
            {proxyRunning ? "浏览中 ● 停止" : "🌐 浏览"}
          </button>
        )}
```

- [ ] **Step 2: 类型检查**

Run: `cd <项目根> && npx tsc --noEmit`
Expected: 无错误

- [ ] **Step 3: 提示用户提交**

```
git add src/shell/TopBar/
git commit -m "feat(frontend): TopBar 浏览入口（服务器视角网页浏览）"
```

---

### Task 6: 断连联动清理

**Files:**
- Modify: `src-tauri/src/connection.rs`（连接丢失/断开处理位置——`ConnectionLost` 分支或 `remote_disconnect` 清理处）
- Modify: `src-tauri/src/proxy/mod.rs`（新增 `pub fn cleanup_session`）

- [ ] **Step 1: proxy/mod.rs 添加同步清理函数**

```rust
/// 连接断开时的同步清理（由 ConnectionLost 处理调用）：
/// abort 监听任务并 kill 浏览器，代理流本身随 QUIC 连接死亡自动关闭。
pub fn cleanup_session(app: &AppHandle, server_id: &str) {
    let sessions: State<ProxySessionManager> = app.state();
    let session = sessions.sessions.lock().unwrap().remove(server_id);
    if let Some(sess) = session {
        sess.listener_task.abort();
        if let Some(mut child) = sess.browser_child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        tracing::info!("[Proxy] 连接断开，代理会话已清理: server_id={}, port={}", server_id, sess.port);
    }
}
```

注意：`ProxySession` 结构体的 `listener_task` 是 `tokio::task::JoinHandle`，`abort()` 可在同步上下文调用。

- [ ] **Step 2: 在连接丢失处挂接**

在 `src-tauri/src/connection.rs` 中找到处理 `ConnectionLost` / `remote_disconnect` 的位置（搜索 `ConnectionLost {` 的 match 分支及 `remote_disconnect` 函数内清理 `connections` 的代码处），在移除 `ActiveConnection` 的同时添加：

```rust
        // 代理会话随连接死亡：abort 监听并关闭浏览器
        crate::proxy::cleanup_session(&app, &server_id);
```

（若该处上下文中变量名不是 `app`/`server_id`，按实际命名调整——语义是"连接移除时清理该 server_id 的代理会话"。）

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo check && cargo test`
Expected: 通过，无回归

- [ ] **Step 4: 手动冒烟清单（提示用户执行）**

1. `npm run tauri dev` → 连接服务器
2. TopBar 点"🌐 浏览" → 应 spawn Edge/Chrome 新实例
3. 浏览器访问 `https://ifconfig.me` 或 `https://ip.cn` → 应显示**服务器 IP**（非本机 IP）
4. 断开服务器连接 → 浏览器应整体断网（页面报错）
5. 重新连接 → 再点浏览 → 恢复

- [ ] **Step 5: 提示用户提交**

```
git add src-tauri/src/
git commit -m "feat(client): 断连时自动清理代理会话"
```

---

## Self-Review 结论

- **协议变体命名一致性**：Task 1 定义 `ProxyOpen{host,port}` / `ProxyOpenResponse{success,error}`，Task 2/4 均按此签名使用 ✓
- **帧协议一致性**：Agent 端用现有 `read_message`/`write_message`，客户端 Task 4 的 `write_frame`/`read_frame` 同构（4 字节小端长度前缀）✓
- **错误路径**：Agent 连不上目标 → ProxyOpenResponse{false} → SOCKS5 reply 0x01；QUIC 断开 → Task 6 联动清理 ✓
- **YAGNI 检查**：不支持 BIND/UDP ASSOCIATE/认证（浏览器不需要）；不做 Agent 出站限制配置（后续需求）；不做 MASQUE ✓
- **已知限制（记录在案）**：`proxy_start_session` 中已有会话时重复 spawn 浏览器，`ProxySessionInfo.browser` 返回 "existing"；监听端口随会话固定。
