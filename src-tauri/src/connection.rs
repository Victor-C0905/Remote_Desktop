use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tokio::sync::mpsc;

// ── 连接状态 ───────────────────────────────────────

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
}

// ── 消息协议 ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub request_id: u32,
    pub payload: Payload,
}

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
    #[serde(rename = "read_dir")]
    ReadDirRequest { path: String },
    #[serde(rename = "read_dir_resp")]
    ReadDirResponse { path: String, entries: Vec<FileEntry> },
    #[serde(rename = "error")]
    Error { code: i32, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: String,
    pub permissions: String,
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

// ── 连接管理器 ───────────────────────────────────────

pub struct ConnectionManager {
    connections: Mutex<HashMap<String, ActiveConnection>>,
    request_counter: Mutex<u32>,
}

struct ActiveConnection {
    info: ConnectionInfo,
    tx: mpsc::Sender<ClientRequest>,
}

enum ClientRequest {
    Send {
        envelope: Envelope,
        response_tx: tokio::sync::oneshot::Sender<Result<Vec<u8>, String>>,
    },
    Disconnect,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            request_counter: Mutex::new(0),
        }
    }

    fn next_request_id(&self) -> u32 {
        let mut counter = self.request_counter.lock().unwrap();
        *counter += 1;
        *counter
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tauri Commands ─────────────────────────────────

#[tauri::command]
pub async fn remote_connect(
    server_id: String,
    host: String,
    port: u16,
    token: Option<String>,
    app: tauri::AppHandle,
) -> Result<ConnectionInfo, String> {
    let manager = app.state::<ConnectionManager>();

    // 安装 CryptoProvider
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 先尝试 QUIC
    let quic_result = try_quic_connect(&host, port).await;
    let quic_err_msg = match &quic_result {
        Err(e) => e.clone(),
        Ok(_) => String::new(),
    };

    if let Ok((conn, rtt)) = quic_result {
        tracing::info!("QUIC 连接成功: {}:{} (RTT={}ms)", host, port, rtt);
        let info = ConnectionInfo {
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
        };

        // 认证
        if let Some(ref tk) = token {
            let auth_result = send_and_receive_quic(&conn, manager.next_request_id(), Payload::AuthRequest { token: tk.clone() }).await;
            if let Ok(resp) = auth_result {
                if let Ok(envelope) = Envelope::decode(&resp) {
                    if let Payload::AuthResponse { success, .. } = envelope.payload {
                        if !success {
                            return Err("认证失败: Token 无效".into());
                        }
                    }
                }
            }
        }

        // 启动消息循环（含心跳和连接状态监听）
        let (tx, mut rx) = mpsc::channel::<ClientRequest>(32);
        {
            let mut conns = manager.connections.lock().unwrap();
            conns.insert(server_id.clone(), ActiveConnection { info: info.clone(), tx: tx.clone() });
        }

        let server_id_clone = server_id.clone();
        let app_handle = app.clone();
        let conn_clone = conn.clone();
        
        tokio::spawn(async move {
            // 心跳任务：每10秒发送 ping 保持连接活跃
            let heartbeat_tx = tx.clone();
            let heartbeat_task = tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
                loop {
                    interval.tick().await;
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let envelope = Envelope::new(0, Payload::Ping { timestamp: now });
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    if heartbeat_tx.send(ClientRequest::Send { envelope, response_tx }).await.is_ok() {
                        // 等待响应，超时则认为连接有问题
                        if response_rx.await.is_err() {
                            tracing::warn!("心跳超时，连接可能已断开");
                            break;
                        }
                    } else {
                        break;
                    }
                }
            });
            
            // 连接状态监听任务
            let status_conn = conn_clone.clone();
            let status_app = app_handle.clone();
            let status_server_id = server_id_clone.clone();
            let status_task = tokio::spawn(async move {
                status_conn.closed().await;
                tracing::warn!("QUIC 连接已关闭: {}", status_server_id);
                let _ = status_app.emit("connection-lost", &status_server_id);
            });
            
            // 主消息循环
            while let Some(req) = rx.recv().await {
                match req {
                    ClientRequest::Send { envelope, response_tx } => {
                        // 检查连接状态
                        if conn_clone.close_reason().is_some() {
                            let _ = response_tx.send(Err("连接已关闭".into()));
                            break;
                        }
                        let result = send_and_receive_quic(&conn_clone, envelope.request_id, envelope.payload).await;
                        let _ = response_tx.send(result);
                    }
                    ClientRequest::Disconnect => break,
                }
            }
            
            // 清理
            heartbeat_task.abort();
            status_task.abort();
            if let Ok(mut conns) = app_handle.state::<ConnectionManager>().connections.lock() {
                conns.remove(&server_id_clone);
            }
            let _ = app_handle.emit("connection-lost", &server_id_clone);
        });

        return Ok(info);
    }

    // QUIC 失败，尝试 WebSocket
    tracing::warn!("QUIC 连接失败 ({})，尝试 WebSocket...", quic_err_msg);
    
    let ws_result = try_ws_connect(&host, port).await;
    let ws_err_msg = match &ws_result {
        Err(e) => e.clone(),
        Ok(_) => String::new(),
    };

    if let Ok(ws_stream) = ws_result {
        tracing::info!("WebSocket 连接成功: {}:{}", host, port);
        let info = ConnectionInfo {
            server_id: server_id.clone(),
            host: host.clone(),
            port,
            transport_type: "websocket".into(),
            status: "connected".into(),
            rtt_ms: -1.0,
            connected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };

        let (tx, mut rx) = mpsc::channel::<ClientRequest>(32);
        {
            let mut conns = manager.connections.lock().unwrap();
            conns.insert(server_id.clone(), ActiveConnection { info: info.clone(), tx });
        }

        let server_id_clone = server_id.clone();
        let app_handle = app.clone();
        tokio::spawn(async move {
            use futures_util::{SinkExt, StreamExt};
            use tokio_tungstenite::tungstenite::Message as WsMessage;

            let (mut write, mut read) = ws_stream.split();

            while let Some(req) = rx.recv().await {
                match req {
                    ClientRequest::Send { envelope, response_tx } => {
                        if let Ok(bytes) = envelope.encode() {
                            if write.send(WsMessage::Binary(bytes)).await.is_err() {
                                let _ = response_tx.send(Err("发送失败".into()));
                                break;
                            }
                            if let Some(Ok(WsMessage::Binary(resp_data))) = read.next().await {
                                let _ = response_tx.send(Ok(resp_data));
                            } else {
                                let _ = response_tx.send(Err("接收失败".into()));
                                break;
                            }
                        }
                    }
                    ClientRequest::Disconnect => break,
                }
            }

            if let Ok(mut conns) = app_handle.state::<ConnectionManager>().connections.lock() {
                conns.remove(&server_id_clone);
            }
            let _ = app_handle.emit("connection-lost", &server_id_clone);
        });

        return Ok(info);
    }

    Err(format!("所有连接方式均失败:\n  QUIC: {}\n  WebSocket: {}", quic_err_msg, ws_err_msg))
}

#[tauri::command]
pub async fn remote_disconnect(server_id: String, app: tauri::AppHandle) -> Result<(), String> {
    let manager = app.state::<ConnectionManager>();
    
    // 先获取 tx，释放 lock 后再发送
    let tx = {
        let mut conns = manager.connections.lock().unwrap();
        conns.remove(&server_id).map(|c| c.tx)
    };
    
    if let Some(tx) = tx {
        let _ = tx.send(ClientRequest::Disconnect).await;
        Ok(())
    } else {
        Err("未找到该服务器的连接".into())
    }
}

#[tauri::command]
pub async fn remote_ping(server_id: String, app: tauri::AppHandle) -> Result<PingResult, String> {
    let manager = app.state::<ConnectionManager>();
    
    let (tx, request_id) = {
        let conns = manager.connections.lock().unwrap();
        let conn = conns.get(&server_id).ok_or("未找到连接")?;
        (conn.tx.clone(), manager.next_request_id())
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let envelope = Envelope::new(request_id, Payload::Ping { timestamp: now });
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    
    tx.send(ClientRequest::Send { envelope, response_tx }).await.map_err(|_| "发送请求失败")?;
    let data = response_rx.await.map_err(|_| "等待响应超时")??;

    let resp = Envelope::decode(&data)?;
    if let Payload::Pong { timestamp, server_time } = resp.payload {
        let latency = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64 - timestamp;
        Ok(PingResult { ok: true, latency_ms: latency as f64, server_time })
    } else {
        Err("收到意外的响应类型".into())
    }
}

#[derive(Debug, Serialize)]
pub struct PingResult {
    pub ok: bool,
    pub latency_ms: f64,
    pub server_time: u64,
}

#[tauri::command]
pub async fn remote_send(server_id: String, payload: Payload, app: tauri::AppHandle) -> Result<Envelope, String> {
    let manager = app.state::<ConnectionManager>();
    
    let (tx, request_id) = {
        let conns = manager.connections.lock().unwrap();
        let conn = conns.get(&server_id).ok_or("未找到连接")?;
        (conn.tx.clone(), manager.next_request_id())
    };

    let envelope = Envelope::new(request_id, payload);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    
    tx.send(ClientRequest::Send { envelope, response_tx }).await.map_err(|_| "发送请求失败")?;
    let data = response_rx.await.map_err(|_| "等待响应超时")??;
    Envelope::decode(&data)
}

#[tauri::command]
pub async fn remote_read_dir(server_id: String, path: String, app: tauri::AppHandle) -> Result<RemoteReadDirResponse, String> {
    let resp = remote_send(server_id, Payload::ReadDirRequest { path }, app).await?;
    match resp.payload {
        Payload::ReadDirResponse { path, entries } => Ok(RemoteReadDirResponse { path, entries }),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[derive(Debug, Serialize)]
pub struct RemoteReadDirResponse {
    pub path: String,
    pub entries: Vec<FileEntry>,
}

// ── QUIC 客户端 ───────────────────────────────────────

async fn try_quic_connect(host: &str, port: u16) -> Result<(quinn::Connection, f64), String> {
    // 安装 CryptoProvider
    let _ = rustls::crypto::ring::default_provider().install_default();

    let addr = format!("{}:{}", host, port).parse().map_err(|e| format!("地址解析失败: {}", e))?;

    // 创建客户端配置（跳过证书验证）
    let client_config = build_quic_client_config()?;

    // 创建 Endpoint
    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .map_err(|e| format!("创建 Endpoint 失败: {}", e))?;
    
    endpoint.set_default_client_config(client_config);

    let start = std::time::Instant::now();
    let conn = endpoint
        .connect(addr, "gnome-remote")
        .map_err(|e| format!("发起连接失败: {}", e))?
        .await
        .map_err(|e| format!("QUIC 握手失败: {}", e))?;

    Ok((conn, start.elapsed().as_secs_f64() * 1000.0))
}

fn build_quic_client_config() -> Result<quinn::ClientConfig, String> {
    // 跳过证书验证（开发阶段）
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(SkipCertVerification))
        .with_no_client_auth();

    Ok(quinn::ClientConfig::new(std::sync::Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
            .map_err(|e| format!("QUIC TLS 配置错误: {}", e))?
    )))
}

#[derive(Debug)]
struct SkipCertVerification;

impl rustls::client::danger::ServerCertVerifier for SkipCertVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

async fn send_and_receive_quic(conn: &quinn::Connection, request_id: u32, payload: Payload) -> Result<Vec<u8>, String> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| format!("打开 Stream 失败: {}", e))?;

    let bytes = Envelope::new(request_id, payload).encode()?;
    let len = (bytes.len() as u32).to_le_bytes();
    
    send.write_all(&len).await.map_err(|e| format!("发送长度失败: {}", e))?;
    send.write_all(&bytes).await.map_err(|e| format!("发送数据失败: {}", e))?;
    send.finish().map_err(|e| format!("关闭写入流失败: {}", e))?;

    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await.map_err(|e| format!("读取响应长度失败: {}", e))?;
    let resp_len = u32::from_le_bytes(len_buf) as usize;
    
    let mut data = vec![0u8; resp_len];
    recv.read_exact(&mut data).await.map_err(|e| format!("读取响应数据失败: {}", e))?;

    Ok(data)
}

// ── WebSocket 客户端 ───────────────────────────────────

async fn try_ws_connect(host: &str, port: u16) -> Result<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, String> {
    let url = format!("wss://{}:{}/ws", host, port);
    let (stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("WebSocket 连接失败: {}", e))?;
    Ok(stream)
}