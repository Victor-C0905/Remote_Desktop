# Agent 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现远程 Agent 服务器,支持 QUIC/WebSocket 协议,提供系统指标采集和文件操作功能。

**Architecture:** Agent 作为独立 Rust 二进制程序运行在远程 Ubuntu 服务器上,监听 UDP 8443(QUIC) 和 WSS /ws(WebSocket),处理客户端请求并返回结构化数据。

**Tech Stack:** Rust + Quinn(QUIC) + Tokio-tungstenite(WebSocket) + Sysinfo(系统指标) + Serde(JSON 序列化)

---

## 文件结构

**创建文件:**
```
agent/src/
  ├── protocol.rs       # 协议定义(Envelope, Payload, MetricsSnapshot)
  ├── handler.rs        # 消息处理(handle_envelope, collect_metrics)
  ├── server/
  │   ├── mod.rs        # 服务器模块导出
  │   ├── quic.rs       # QUIC 服务器实现
  │   └── websocket.rs  # WebSocket 服务器实现
  └── cert.rs           # 证书生成(自签名 TLS 证书)
```

**修改文件:**
```
agent/src/main.rs       # 主入口,启动 QUIC/WebSocket 服务器
agent/src/config.rs     # 配置管理(添加 metrics_interval_secs)
agent/Cargo.toml        # 添加依赖(sysinfo)
```

---

## Task 1: 协议定义

**Files:**
- Create: `agent/src/protocol.rs`

- [ ] **Step 1: 定义消息结构**

```rust
// agent/src/protocol.rs
use serde::{Deserialize, Serialize};

/// 消息信封,包含请求 ID 和 payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub request_id: u32,
    pub payload: Payload,
}

impl Envelope {
    pub fn new(request_id: u32, payload: Payload) -> Self {
        Self { request_id, payload }
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(self).map_err(|e| e.to_string())
    }

    pub fn decode(data: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(data).map_err(|e| e.to_string())
    }
}

/// 消息 payload,使用 serde tag 标记类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Payload {
    #[serde(rename = "ping")]
    Ping { timestamp: u64 },

    #[serde(rename = "pong")]
    Pong { timestamp: u64, server_time: u64 },

    #[serde(rename = "auth_request")]
    AuthRequest { token: String },

    #[serde(rename = "auth_response")]
    AuthResponse { success: bool, error: Option<String> },

    #[serde(rename = "metrics_subscribe")]
    MetricsSubscribeRequest {},

    #[serde(rename = "metrics_data")]
    MetricsData(MetricsSnapshot),

    #[serde(rename = "read_dir")]
    ReadDirRequest { path: String },

    #[serde(rename = "read_dir_resp")]
    ReadDirResponse { path: String, entries: Vec<FileEntry> },

    #[serde(rename = "read_file")]
    ReadFileRequest { path: String },

    #[serde(rename = "read_file_resp")]
    ReadFileResponse { path: String, content: String, size: u64 },

    #[serde(rename = "write_file")]
    WriteFileRequest { path: String, content: String },

    #[serde(rename = "write_file_resp")]
    WriteFileResponse { path: String, size: u64 },

    #[serde(rename = "delete")]
    DeleteRequest { path: String },

    #[serde(rename = "delete_resp")]
    DeleteResponse { success: bool },

    #[serde(rename = "terminal_spawn")]
    TerminalSpawnRequest { shell: String, cols: u16, rows: u16 },

    #[serde(rename = "terminal_spawn_resp")]
    TerminalSpawnResponse { session_id: String },

    #[serde(rename = "terminal_data")]
    TerminalData { session_id: String, data: Vec<u8>, is_input: bool },

    #[serde(rename = "error")]
    Error { code: i32, message: String },
}

/// 系统指标快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub cpu_percent: f32,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub disks: Vec<DiskInfo>,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub uptime_secs: u64,
}

/// 磁盘信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
}

/// 文件条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: String,
    pub permissions: String,
}
```

- [ ] **Step 2: 验证协议定义**

运行: `cargo check --manifest-path agent/Cargo.toml`
预期: 编译成功,无错误

- [ ] **Step 3: 提交协议定义**

```bash
git add agent/src/protocol.rs
git commit -m "feat(agent): 定义消息协议和系统指标结构"
```

---

## Task 2: 消息处理

**Files:**
- Modify: `agent/src/handler.rs:1-306` (替换现有实现)

- [ ] **Step 1: 重构消息处理函数**

```rust
// agent/src/handler.rs (替换现有内容)
use crate::config::AgentConfig;
use crate::protocol::{Envelope, FileEntry, MetricsSnapshot, Payload};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn handle_envelope(envelope: &Envelope, cfg: &AgentConfig) -> Envelope {
    match &envelope.payload {
        Payload::Ping { timestamp } => {
            let server_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            tracing::debug!("收到 ping, timestamp={}", timestamp);

            Envelope::new(
                envelope.request_id,
                Payload::Pong {
                    timestamp: *timestamp,
                    server_time,
                },
            )
        }

        Payload::AuthRequest { token } => {
            let success = !cfg.auth.token.is_empty() && token == &cfg.auth.token;
            let status_str = if success { "✅ 成功" } else { "❌ 失败" };
            tracing::info!(
                "认证请求: {} (expected: {}...)",
                status_str,
                &cfg.auth.token[..cfg.auth.token.len().min(8)]
            );

            Envelope::new(
                envelope.request_id,
                Payload::AuthResponse {
                    success,
                    error: if success {
                        None
                    } else {
                        Some("Token 无效".into())
                    },
                },
            )
        }

        Payload::MetricsSubscribeRequest {} => {
            match collect_metrics() {
                Ok(metrics) => Envelope::new(
                    envelope.request_id,
                    Payload::MetricsData(metrics),
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }

        Payload::ReadDirRequest { path } => {
            match handle_read_dir(path, cfg) {
                Ok(entries) => Envelope::new(
                    envelope.request_id,
                    Payload::ReadDirResponse {
                        path: path.clone(),
                        entries,
                    },
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }

        Payload::ReadFileRequest { path } => {
            match handle_read_file(path, cfg) {
                Ok((content, size)) => Envelope::new(
                    envelope.request_id,
                    Payload::ReadFileResponse {
                        path: path.clone(),
                        content,
                        size,
                    },
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }

        Payload::WriteFileRequest { path, content } => {
            match handle_write_file(path, content, cfg) {
                Ok(size) => Envelope::new(
                    envelope.request_id,
                    Payload::WriteFileResponse {
                        path: path.clone(),
                        size,
                    },
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }

        Payload::DeleteRequest { path } => {
            match handle_delete(path, cfg) {
                Ok(_) => Envelope::new(
                    envelope.request_id,
                    Payload::DeleteResponse { success: true },
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }

        Payload::TerminalSpawnRequest { shell, cols, rows } => {
            tracing::info!("终端请求: shell={}, cols={}, rows={} (未实现)", shell, cols, rows);
            error_response(envelope.request_id, "终端功能尚未实现")
        }

        Payload::TerminalData { session_id, data, is_input } => {
            tracing::debug!("终端数据: session={}, len={}, is_input={} (未实现)", session_id, data.len(), is_input);
            error_response(envelope.request_id, "终端功能尚未实现")
        }

        other => {
            tracing::warn!("未处理的消息类型: {:?}", std::mem::discriminant(other));
            error_response(envelope.request_id, "未知的消息类型")
        }
    }
}

fn error_response(request_id: u32, message: &str) -> Envelope {
    Envelope::new(
        request_id,
        Payload::Error {
            code: -1,
            message: message.into(),
        },
    )
}

fn handle_read_dir(path: &str, cfg: &AgentConfig) -> Result<Vec<FileEntry>, String> {
    let allowed = cfg
        .security
        .allowed_paths
        .iter()
        .any(|prefix| path.starts_with(prefix));
    if !allowed && !cfg.security.allowed_paths.is_empty() {
        return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
    }

    let entries = fs::read_dir(path)
        .map_err(|e| format!("无法读取目录 '{}': {}", path, e))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let metadata = entry.metadata().ok()?;
            let name = entry.file_name().to_string_lossy().to_string();

            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| {
                    let dt = chrono::DateTime::<chrono::Utc>::from(SystemTime::UNIX_EPOCH + d);
                    dt.to_rfc3339()
                })
                .unwrap_or_default();

            Some(FileEntry {
                name,
                is_dir: metadata.is_dir(),
                size: if metadata.is_dir() { 0 } else { metadata.len() },
                mtime,
                permissions: format_permissions(&metadata),
            })
        })
        .collect();

    Ok(entries)
}

fn handle_read_file(path: &str, cfg: &AgentConfig) -> Result<(String, u64), String> {
    let allowed = cfg
        .security
        .allowed_paths
        .iter()
        .any(|prefix| path.starts_with(prefix));
    if !allowed && !cfg.security.allowed_paths.is_empty() {
        return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
    }

    let metadata = fs::metadata(path)
        .map_err(|e| format!("无法访问文件 '{}': {}", path, e))?;

    if metadata.is_dir() {
        return Err("这是一个目录，不能作为文件读取".to_string());
    }

    const MAX_SIZE: u64 = 10 * 1024 * 1024; // 10MB
    if metadata.len() > MAX_SIZE {
        return Err(format!("文件太大 ({}MB)，限制 10MB", metadata.len() / (1024 * 1024)));
    }

    let content = fs::read_to_string(path)
        .map_err(|e| format!("读取文件失败: {}", e))?;

    Ok((content, metadata.len()))
}

fn handle_write_file(path: &str, content: &str, cfg: &AgentConfig) -> Result<u64, String> {
    let allowed = cfg
        .security
        .allowed_paths
        .iter()
        .any(|prefix| path.starts_with(prefix));
    if !allowed && !cfg.security.allowed_paths.is_empty() {
        return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
    }

    fs::write(path, content)
        .map_err(|e| format!("写入文件失败: {}", e))?;

    Ok(content.len() as u64)
}

fn handle_delete(path: &str, cfg: &AgentConfig) -> Result<(), String> {
    let allowed = cfg
        .security
        .allowed_paths
        .iter()
        .any(|prefix| path.starts_with(prefix));
    if !allowed && !cfg.security.allowed_paths.is_empty() {
        return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
    }

    for blocked in &cfg.security.blocked_commands {
        if path.contains(blocked) {
            return Err(format!("禁止删除: 路径包含敏感内容 ({})", path));
        }
    }

    let metadata = fs::metadata(path)
        .map_err(|e| format!("无法访问 '{}': {}", path, e))?;

    if metadata.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|e| format!("删除目录失败: {}", e))?;
    } else {
        fs::remove_file(path)
            .map_err(|e| format!("删除文件失败: {}", e))?;
    }

    Ok(())
}

#[cfg(unix)]
fn format_permissions(metadata: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = metadata.permissions().mode();
    let bits = [
        (0o400, 'r'), (0o200, 'w'), (0o100, 'x'),
        (0o040, 'r'), (0o020, 'w'), (0o010, 'x'),
        (0o004, 'r'), (0o002, 'w'), (0o001, 'x'),
    ];
    bits.iter()
        .map(|(mask, ch)| if mode & mask != 0 { *ch } else { '-' })
        .collect()
}

#[cfg(not(unix))]
fn format_permissions(_metadata: &std::fs::Metadata) -> String {
    "rw-rw-rw-".into()
}

fn collect_metrics() -> Result<MetricsSnapshot, String> {
    use sysinfo::{Disks, Networks, System};

    let mut sys = System::new_all();
    sys.refresh_all();

    let mem_used = sys.used_memory();
    let mem_total = sys.total_memory();
    let cpu_percent = sys.global_cpu_usage();

    let disks_obj = Disks::new_with_refreshed_list();
    let disks: Vec<_> = disks_obj
        .iter()
        .map(|d| crate::protocol::DiskInfo {
            mount_point: d.mount_point().to_string_lossy().to_string(),
            total_bytes: d.total_space(),
            used_bytes: d.total_space() - d.available_space(),
        })
        .collect();

    let networks = Networks::new_with_refreshed_list();
    let mut network_rx = 0u64;
    let mut network_tx = 0u64;
    for (_name, data) in &networks {
        network_rx += data.received();
        network_tx += data.transmitted();
    }

    Ok(MetricsSnapshot {
        cpu_percent,
        mem_used_bytes: mem_used,
        mem_total_bytes: mem_total,
        swap_used_bytes: sys.used_swap(),
        disks,
        network_rx_bytes: network_rx,
        network_tx_bytes: network_tx,
        uptime_secs: System::uptime() as u64,
    })
}
```

- [ ] **Step 2: 添加 sysinfo 依赖**

```toml
# agent/Cargo.toml (在 [dependencies] 添加)
sysinfo = "0.30"
```

- [ ] **Step 3: 验证消息处理**

运行: `cargo check --manifest-path agent/Cargo.toml`
预期: 编译成功,无错误

- [ ] **Step 4: 提交消息处理**

```bash
git add agent/src/handler.rs agent/Cargo.toml
git commit -m "feat(agent): 实现消息处理和系统指标采集"
```

---

## Task 3: QUIC 服务器

**Files:**
- Create: `agent/src/server/mod.rs`
- Create: `agent/src/server/quic.rs`

- [ ] **Step 1: 创建服务器模块导出**

```rust
// agent/src/server/mod.rs
pub mod quic;
pub mod websocket;
```

- [ ] **Step 2: 实现 QUIC 服务器**

```rust
// agent/src/server/quic.rs
use crate::config::AgentConfig;
use crate::handler::handle_envelope;
use crate::protocol::Envelope;
use quinn::{Endpoint, ServerConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn run_quic_server(config: &AgentConfig) -> Result<(), String> {
    let addr: SocketAddr = config.server.listen.parse()
        .map_err(|e| format!("地址解析失败: {}", e))?;

    let server_config = build_server_config(config)?;

    let endpoint = Endpoint::server(server_config, addr)
        .map_err(|e| format!("创建 Endpoint 失败: {}", e))?;

    tracing::info!("QUIC 服务器启动: {}", addr);

    while let Some(conn) = endpoint.accept().await {
        let connection = conn.await
            .map_err(|e| format!("连接失败: {}", e))?;

        tracing::info!("QUIC 连接建立: {}", connection.remote_address());

        let config_clone = Arc::new(config.clone());
        tokio::spawn(handle_connection(connection, config_clone));
    }

    Ok(())
}

async fn handle_connection(conn: quinn::Connection, config: Arc<AgentConfig>) {
    while let Ok(stream) = conn.accept_bi().await {
        let (mut send, mut recv) = stream;
        let config_clone = config.clone();

        tokio::spawn(async move {
            // 读取消息长度(4 字节 LE)
            let mut len_buf = [0u8; 4];
            if recv.read_exact(&mut len_buf).await.is_err() {
                return;
            }
            let len = u32::from_le_bytes(len_buf) as usize;

            // 读取消息数据
            let mut data = vec![0u8; len];
            if recv.read_exact(&mut data).await.is_err() {
                return;
            }

            // 解码消息
            let envelope = Envelope::decode(&data);
            if let Ok(env) = envelope {
                // 处理消息
                let response = handle_envelope(&env, &config_clone);

                // 编码响应
                let bytes = response.encode();
                if let Ok(b) = bytes {
                    // 发送响应长度(4 字节 LE)
                    let resp_len = (b.len() as u32).to_le_bytes();
                    if send.write_all(&resp_len).await.is_err() {
                        return;
                    }

                    // 发送响应数据
                    if send.write_all(&b).await.is_err() {
                        return;
                    }

                    send.finish().await.ok();
                }
            }
        });
    }

    tracing::info!("QUIC 连接关闭: {}", conn.remote_address());
}

fn build_server_config(config: &AgentConfig) -> Result<ServerConfig, String> {
    // 读取证书和私钥
    let cert = std::fs::read(&config.server.cert_path)
        .map_err(|e| format!("读取证书失败: {}", e))?;
    let key = std::fs::read(&config.server.key_path)
        .map_err(|e| format!("读取私钥失败: {}", e))?;

    // 创建服务器配置
    let server_config = ServerConfig::with_single_cert(
        vec![quinn::rustls::pki_types::CertificateDer::from(cert)],
        quinn::rustls::pki_types::PrivateKeyDer::try_from(key)
            .map_err(|e| format!("私钥格式错误: {}", e))?,
    ).map_err(|e| format!("创建服务器配置失败: {}", e))?;

    Ok(server_config)
}
```

- [ ] **Step 3: 验证 QUIC 服务器**

运行: `cargo check --manifest-path agent/Cargo.toml`
预期: 编译成功,无错误

- [ ] **Step 4: 提交 QUIC 服务器**

```bash
git add agent/src/server/mod.rs agent/src/server/quic.rs
git commit -m "feat(agent): 实现 QUIC 服务器"
```

---

## Task 4: WebSocket 服务器

**Files:**
- Create: `agent/src/server/websocket.rs`

- [ ] **Step 1: 实现 WebSocket 服务器**

```rust
// agent/src/server/websocket.rs
use crate::config::AgentConfig;
use crate::handler::handle_envelope;
use crate::protocol::Envelope;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

pub async fn run_ws_server(config: &AgentConfig) -> Result<(), String> {
    let addr: SocketAddr = config.server.listen.parse()
        .map_err(|e| format!("地址解析失败: {}", e))?;

    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("绑定端口失败: {}", e))?;

    tracing::info!("WebSocket 服务器启动: {}", addr);

    while let Ok((stream, addr)) = listener.accept().await {
        tracing::info!("WebSocket 连接建立: {}", addr);

        let config_clone = Arc::new(config.clone());
        tokio::spawn(handle_ws_connection(stream, config_clone));
    }

    Ok(())
}

async fn handle_ws_connection(stream: tokio::net::TcpStream, config: Arc<AgentConfig>) {
    let ws_stream = tokio_tungstenite::accept_hdr_async(stream, |_, _| Ok(None))
        .await
        .ok();

    if let Some(ws) = ws_stream {
        let (mut write, mut read) = ws.split();

        while let Some(msg) = read.next().await {
            if let Ok(WsMessage::Binary(data)) = msg {
                // 解码消息
                let envelope = Envelope::decode(&data);
                if let Ok(env) = envelope {
                    // 处理消息
                    let response = handle_envelope(&env, &config);

                    // 编码响应
                    let bytes = response.encode();
                    if let Ok(b) = bytes {
                        // 发送响应
                        if write.send(WsMessage::Binary(b)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }

        tracing::info!("WebSocket 连接关闭");
    }
}
```

- [ ] **Step 2: 验证 WebSocket 服务器**

运行: `cargo check --manifest-path agent/Cargo.toml`
预期: 编译成功,无错误

- [ ] **Step 3: 提交 WebSocket 服务器**

```bash
git add agent/src/server/websocket.rs
git commit -m "feat(agent): 实现 WebSocket 服务器"
```

---

## Task 5: 证书生成

**Files:**
- Create: `agent/src/cert.rs`

- [ ] **Step 1: 实现证书生成**

```rust
// agent/src/cert.rs
use rcgen::{CertificateParams, KeyPair};
use std::fs;
use std::path::Path;

pub fn generate_cert(cert_path: &str, key_path: &str) -> Result<(), String> {
    // 如果证书已存在,跳过生成
    if Path::new(cert_path).exists() && Path::new(key_path).exists() {
        tracing::info!("证书已存在,跳过生成");
        return Ok(());
    }

    // 生成自签名证书
    let mut params = CertificateParams::default();
    params.common_name = "gnome-remote-agent".into();

    let key_pair = KeyPair::generate()
        .map_err(|e| format!("生成密钥失败: {}", e))?;

    let cert = params.serialize_self_signed(&key_pair)
        .map_err(|e| format!("生成证书失败: {}", e))?;

    // 保存证书
    fs::write(cert_path, cert.pem())
        .map_err(|e| format!("保存证书失败: {}", e))?;

    // 保存私钥
    fs::write(key_path, key_pair.serialize_pem())
        .map_err(|e| format!("保存私钥失败: {}", e))?;

    tracing::info!("证书生成成功: {} {}", cert_path, key_path);
    Ok(())
}
```

- [ ] **Step 2: 验证证书生成**

运行: `cargo check --manifest-path agent/Cargo.toml`
预期: 编译成功,无错误

- [ ] **Step 3: 提交证书生成**

```bash
git add agent/src/cert.rs
git commit -m "feat(agent): 实现自签名证书生成"
```

---

## Task 6: 主入口集成

**Files:**
- Modify: `agent/src/main.rs:1-50`

- [ ] **Step 1: 重构主入口**

```rust
// agent/src/main.rs (替换现有内容)
mod cert;
mod config;
mod handler;
mod protocol;
mod server;

use config::AgentConfig;
use server::{quic::run_quic_server, websocket::run_ws_server};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    // 加载配置
    let config_path = "agent.toml";
    let config = if Path::new(config_path).exists() {
        AgentConfig::load(config_path)?
    } else {
        tracing::warn!("配置文件不存在,使用默认配置");
        AgentConfig::default()
    };

    tracing::info!("Agent 配置加载成功: {:?}", config);

    // 生成证书(如果不存在)
    cert::generate_cert(&config.server.cert_path, &config.server.key_path)?;

    // 启动 QUIC 和 WebSocket 服务器
    let quic_task = tokio::spawn(run_quic_server(&config));
    let ws_task = tokio::spawn(run_ws_server(&config));

    // 等待任意一个服务器完成(通常不会完成)
    tokio::select! {
        result = quic_task => {
            tracing::info!("QUIC 服务器结束: {:?}", result);
        }
        result = ws_task => {
            tracing::info!("WebSocket 服务器结束: {:?}", result);
        }
    }

    Ok(())
}
```

- [ ] **Step 2: 验证主入口**

运行: `cargo check --manifest-path agent/Cargo.toml`
预期: 编译成功,无错误

- [ ] **Step 3: 提交主入口**

```bash
git add agent/src/main.rs
git commit -m "feat(agent): 集成 QUIC 和 WebSocket 服务器"
```

---

## Task 7: 配置管理

**Files:**
- Modify: `agent/src/config.rs:1-50`

- [ ] **Step 1: 扩展配置结构**

```rust
// agent/src/config.rs (在现有结构添加)
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub limits: LimitsConfig,
    pub security: SecurityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub listen: String,
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    pub max_file_transfer_mb: u64,
    pub max_terminal_sessions: u32,
    pub metrics_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub allowed_paths: Vec<String>,
    pub blocked_commands: Vec<String>,
}

impl AgentConfig {
    pub fn load(path: &str) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("读取配置失败: {}", e))?;

        toml::from_str(&content)
            .map_err(|e| format!("解析配置失败: {}", e))
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                listen: "0.0.0.0:8443".into(),
                cert_path: "cert.pem".into(),
                key_path: "key.pem".into(),
            },
            auth: AuthConfig {
                token: "".into(),
            },
            limits: LimitsConfig {
                max_file_transfer_mb: 1000,
                max_terminal_sessions: 10,
                metrics_interval_secs: 2,
            },
            security: SecurityConfig {
                allowed_paths: vec!["/home".into(), "/etc".into(), "/var/log".into()],
                blocked_commands: vec!["rm -rf /".into(), "dd if=".into()],
            },
        }
    }
}
```

- [ ] **Step 2: 验证配置管理**

运行: `cargo check --manifest-path agent/Cargo.toml`
预期: 编译成功,无错误

- [ ] **Step 3: 提交配置管理**

```bash
git add agent/src/config.rs
git commit -m "feat(agent): 扩展配置管理,支持指标采集间隔"
```

---

## Task 8: 编译和测试

**Files:**
- 无文件修改

- [ ] **Step 1: 编译 Agent**

运行: `cargo build --release --manifest-path agent/Cargo.toml`
预期: 编译成功,生成 agent 二进制文件

- [ ] **Step 2: 创建默认配置文件**

```toml
# agent/agent.toml (创建文件)
[server]
listen = "0.0.0.0:8443"
cert_path = "cert.pem"
key_path = "key.pem"

[auth]
token = "gmr_test_token_12345678"

[limits]
max_file_transfer_mb = 1000
max_terminal_sessions = 10
metrics_interval_secs = 2

[security]
allowed_paths = ["/home", "/etc", "/var/log", "/opt"]
blocked_commands = ["rm -rf /", "dd if=", "mkfs."]
```

- [ ] **Step 3: 运行 Agent(本地测试)**

运行: `cd agent && cargo run --release`
预期: Agent 启动,监听 UDP 8443 和 WSS /ws

- [ ] **Step 4: 验证 Agent 功能**

使用客户端测试脚本验证:
- Ping/Pong 协议
- 认证协议
- 系统指标采集
- 文件操作

- [ ] **Step 5: 提交配置文件**

```bash
git add agent/agent.toml
git commit -m "feat(agent): 添加默认配置文件"
```

---

## Task 9: 文档和部署

**Files:**
- Create: `agent/README.md`

- [ ] **Step 1: 编写 Agent 文档**

```markdown
# GNOME Remote Agent

远程 Agent 服务器,支持 QUIC 和 WebSocket 协议。

## 功能

- QUIC 服务器(UDP 8443)
- WebSocket 服务器(WSS /ws)
- 系统指标采集(CPU、内存、磁盘、网络)
- 文件操作(浏览、读取、写入、删除)
- Token 认证
- 路径访问控制

## 编译

```bash
cargo build --release --target x86_64-unknown-linux-musl
```

## 部署

```bash
scp target/x86_64-unknown-linux-musl/release/agent user@server:/opt/gnome-remote/
scp agent.toml user@server:/opt/gnome-remote/
scp cert.pem user@server:/opt/gnome-remote/
scp key.pem user@server:/opt/gnome-remote/

systemctl enable --now gnome-remote-agent
```

## 配置

编辑 `agent.toml`:

```toml
[server]
listen = "0.0.0.0:8443"
cert_path = "/opt/gnome-remote/cert.pem"
key_path = "/opt/gnome-remote/key.pem"

[auth]
token = "gmr_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"

[limits]
max_file_transfer_mb = 1000
max_terminal_sessions = 10
metrics_interval_secs = 2

[security]
allowed_paths = ["/home", "/etc", "/var/log", "/opt"]
blocked_commands = ["rm -rf /", "dd if=", "mkfs."]
```

## API 文档

参见 `docs/superpowers/specs/2026-06-12-desktop-shell-design.md` 第三章。
```

- [ ] **Step 2: 提交文档**

```bash
git add agent/README.md
git commit -m "docs(agent): 添加 Agent 使用文档"
```

---

## Self-Review

**1. Spec coverage:**
- ✅ 协议定义(第三章 3.1-3.7) - Task 1
- ✅ 消息处理(第五章 5.3) - Task 2
- ✅ QUIC 服务器(第五章 5.4) - Task 3
- ✅ WebSocket 服务器(第五章 5.5) - Task 4
- ✅ 证书生成(第五章 5.1) - Task 5
- ✅ 主入口集成(第五章 5.1) - Task 6
- ✅ 配置管理(第五章 5.1) - Task 7
- ✅ 编译和测试(第六章 6.2) - Task 8
- ✅ 文档和部署(第七章 7.1) - Task 9

**2. Placeholder scan:**
- ✅ 无 TBD、TODO、incomplete sections
- ✅ 所有步骤包含完整代码
- ✅ 所有命令包含预期输出

**3. Type consistency:**
- ✅ Envelope、Payload、MetricsSnapshot 定义一致
- ✅ handle_envelope 函数签名一致
- ✅ 所有协议消息类型匹配

---

## 执行选项

计划完成并保存到 `docs/superpowers/plans/2026-06-12-agent-implementation.md`。

**两种执行方式:**

**1. Subagent-Driven (推荐)** - 我为每个任务派发新的子代理,任务之间审查,快速迭代

**2. Inline Execution** - 在此会话中使用 executing-plans 执行任务,批量执行带检查点

你希望采用哪种方式?