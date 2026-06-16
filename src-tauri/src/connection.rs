use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
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
    #[serde(rename = "mkdir")]
    MkdirRequest { path: String },
    #[serde(rename = "mkdir_resp")]
    MkdirResponse { success: bool, path: String },
    #[serde(rename = "rename")]
    RenameRequest { old_path: String, new_path: String },
    #[serde(rename = "rename_resp")]
    RenameResponse { success: bool, old_path: String, new_path: String },
    #[serde(rename = "copy")]
    CopyRequest { src: String, dst: String },
    #[serde(rename = "copy_resp")]
    CopyResponse { success: bool, src: String, dst: String },
    #[serde(rename = "move")]
    MoveRequest { src: String, dst: String },
    #[serde(rename = "move_resp")]
    MoveResponse { success: bool, src: String, dst: String },
    #[serde(rename = "metrics_subscribe")]
    MetricsSubscribeRequest {},
    #[serde(rename = "metrics_data")]
    MetricsData(MetricsSnapshot),
    #[serde(rename = "terminal_spawn")]
    TerminalSpawnRequest { shell: String, cols: u16, rows: u16 },
    #[serde(rename = "terminal_spawn_resp")]
    TerminalSpawnResponse { session_id: String },
    #[serde(rename = "terminal_data")]
    TerminalData { session_id: String, data: Vec<u8>, is_input: bool },
    #[serde(rename = "get_current_user")]
    GetCurrentUser,
    #[serde(rename = "current_user_resp")]
    CurrentUserResponse { username: String },
    #[serde(rename = "get_mounts")]
    GetMounts,
    #[serde(rename = "mounts_resp")]
    MountsResponse { mounts: Vec<MountInfo> },

    // 新增：通用订阅
    #[serde(rename = "subscribe")]
    Subscribe {
        server_id: String,
        types: Vec<SubscriptionType>,
    },

    // 新增：通用取消订阅
    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        server_id: String,
        types: Vec<SubscriptionType>,
    },

    // 新增：通用事件推送
    #[serde(rename = "event")]
    Event {
        event_type: String,
        data: serde_json::Value,
        timestamp: u64,
    },

    // 新增：订阅确认
    #[serde(rename = "subscribe_ack")]
    SubscribeAck {
        success: bool,
        subscribed_types: Vec<SubscriptionType>,
    },

    // 新增：取消订阅确认
    #[serde(rename = "unsubscribe_ack")]
    UnsubscribeAck {
        success: bool,
    },

    #[serde(rename = "error")]
    Error { code: i32, message: String },
}

// 新增：订阅类型定义
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", content = "params")]
pub enum SubscriptionType {
    #[serde(rename = "metrics")]
    Metrics { interval_secs: Option<u64> },

    #[serde(rename = "file_changes")]
    FileChanges { path: String, recursive: Option<bool> },

    #[serde(rename = "process_events")]
    ProcessEvents { interval_secs: Option<u64> },

    #[serde(rename = "app_logs")]
    AppLogs { app_name: String, level: Option<String> },

    #[serde(rename = "service_status")]
    ServiceStatus { service: String, interval_secs: Option<u64> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: String,
    pub permissions: String,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountInfo {
    pub mount_point: String,
    pub device: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
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
    // QUIC Connection（用于创建持久 Stream）
    quic_conn: Option<Arc<quinn::Connection>>,
    // 持久 Stream 监听任务
    subscription_task: Option<tokio::task::JoinHandle<()>>,
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
            conns.insert(server_id.clone(), ActiveConnection {
                info: info.clone(),
                tx: tx.clone(),
                quic_conn: Some(Arc::new(conn.clone())),
                subscription_task: None,
            });
        }

        let server_id_clone = server_id.clone();
        let app_handle = app.clone();
        let conn_clone = conn.clone();
        
        tokio::spawn(async move {
            // ── 心跳任务（Watchdog）：快速检测连接断开 ────────
            // 业界标准（TeamViewer/AnyDesk 级别）：
            //   - 间隔 2s：每 2 秒发一次 Ping 探测连接活性
            //   - 超时 3s：等待 Pong 响应的最长时间
            //   - 最坏延迟：2s(间隔) + 3s(超时) = **5s**
            //   - 最佳延迟：idle_timeout(5s) 或 conn.closed() 瞬间触发
            //
            // 双重保障机制：
            //   路径 A: 心跳 Ping/Pong → 应用层检测 (2+3=5s)
            //   路径 B: QUIC idle_timeout(5s) → 传输层自动关闭 → conn.closed() (5s)
            //   哪条路径先触发，哪条先通知前端
            let heartbeat_tx = tx.clone();
            let heartbeat_app = app_handle.clone();
            let heartbeat_server_id = server_id_clone.clone();
            let heartbeat_task = tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let envelope = Envelope::new(0, Payload::Ping { timestamp: now });
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    if heartbeat_tx.send(ClientRequest::Send { envelope, response_tx }).await.is_ok() {
                        // 显式超时 3s：等待 Pong 响应，超时即判定连接异常
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(3),
                            response_rx,
                        ).await {
                            Ok(Ok(_)) => { /* Pong 正常收到，连接存活 */ }
                            Ok(Err(e)) => {
                                tracing::warn!("心跳发送失败，连接可能已断开: {}", e);
                                break;
                            }
                            Err(_) => {
                                // 超时未收到 Pong → 连接已死（对端无响应）
                                tracing::warn!("Ping 超时 (3s)，连接无响应: {}", heartbeat_server_id);
                                break;
                            }
                        }
                    } else {
                        tracing::warn!("心跳通道已关闭: {}", heartbeat_server_id);
                        break;
                    }
                }
                // 心跳失败 → 立即通知前端断连（不等主循环清理）
                tracing::info!("Watchdog 检测到连接丢失，通知前端: {}", heartbeat_server_id);
                let _ = heartbeat_app.emit("connection-lost", &heartbeat_server_id);
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

    // QUIC 失败，返回错误
    Err(format!("QUIC 连接失败: {}", quic_err_msg))
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

#[tauri::command]
pub async fn remote_get_current_user(server_id: String, app: tauri::AppHandle) -> Result<String, String> {
    let resp = remote_send(server_id, Payload::GetCurrentUser, app).await?;
    match resp.payload {
        Payload::CurrentUserResponse { username } => Ok(username),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_get_mounts(server_id: String, app: tauri::AppHandle) -> Result<Vec<MountInfo>, String> {
    let resp = remote_send(server_id, Payload::GetMounts, app).await?;
    match resp.payload {
        Payload::MountsResponse { mounts } => Ok(mounts),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_get_metrics(server_id: String, app: tauri::AppHandle) -> Result<MetricsSnapshot, String> {
    let resp = remote_send(server_id, Payload::MetricsSubscribeRequest {}, app).await?;
    match resp.payload {
        Payload::MetricsData(metrics) => Ok(metrics),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_read_file(server_id: String, path: String, app: tauri::AppHandle) -> Result<RemoteReadFileResponse, String> {
    let resp = remote_send(server_id, Payload::ReadFileRequest { path }, app).await?;
    match resp.payload {
        Payload::ReadFileResponse { path, content, size } => Ok(RemoteReadFileResponse { path, content, size }),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[derive(Debug, Serialize)]
pub struct RemoteReadFileResponse {
    pub path: String,
    pub content: String,
    pub size: u64,
}

#[tauri::command]
pub async fn remote_write_file(server_id: String, path: String, content: String, app: tauri::AppHandle) -> Result<RemoteWriteFileResponse, String> {
    let resp = remote_send(server_id, Payload::WriteFileRequest { path, content }, app).await?;
    match resp.payload {
        Payload::WriteFileResponse { path, size } => Ok(RemoteWriteFileResponse { path, size }),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[derive(Debug, Serialize)]
pub struct RemoteWriteFileResponse {
    pub path: String,
    pub size: u64,
}

#[tauri::command]
pub async fn remote_delete(server_id: String, path: String, app: tauri::AppHandle) -> Result<bool, String> {
    let resp = remote_send(server_id, Payload::DeleteRequest { path }, app).await?;
    match resp.payload {
        Payload::DeleteResponse { success } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_mkdir(server_id: String, path: String, app: tauri::AppHandle) -> Result<bool, String> {
    println!("[Connection] remote_mkdir: server_id={}, path={}", server_id, path);

    let resp = remote_send(server_id, Payload::MkdirRequest { path }, app).await?;

    match resp.payload {
        Payload::MkdirResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_rename(
    server_id: String,
    old_path: String,
    new_path: String,
    app: tauri::AppHandle
) -> Result<bool, String> {
    println!("[Connection] remote_rename: old={}, new={}", old_path, new_path);

    let resp = remote_send(server_id, Payload::RenameRequest { old_path, new_path }, app).await?;

    match resp.payload {
        Payload::RenameResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_copy(
    server_id: String,
    src: String,
    dst: String,
    app: tauri::AppHandle
) -> Result<bool, String> {
    println!("[Connection] remote_copy: src={}, dst={}", src, dst);

    let resp = remote_send(server_id, Payload::CopyRequest { src, dst }, app).await?;

    match resp.payload {
        Payload::CopyResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_move(
    server_id: String,
    src: String,
    dst: String,
    app: tauri::AppHandle
) -> Result<bool, String> {
    println!("[Connection] remote_move: src={}, dst={}", src, dst);

    let resp = remote_send(server_id, Payload::MoveRequest { src, dst }, app).await?;

    match resp.payload {
        Payload::MoveResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

// ── 订阅相关 Tauri Commands ─────────────────────────────────

#[tauri::command]
pub async fn subscribe(
    server_id: String,
    types: Vec<SubscriptionType>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let manager = app.state::<ConnectionManager>();

    // 使用锁保护整个订阅流程，避免竞态条件
    let quic_conn = {
        let mut conns = manager.connections.lock().unwrap();
        if let Some(active_conn) = conns.get_mut(&server_id) {
            // 检查是否已有订阅任务
            if active_conn.subscription_task.is_some() {
                tracing::info!("已有订阅任务: server_id={}", server_id);
                return Ok(()); // 已订阅，直接返回
            }

            // 获取 QUIC Connection
            active_conn.quic_conn.clone()
        } else {
            return Err("未找到连接".into());
        }
    };

    // 创建持久 Stream（用于订阅请求和事件监听）
    if let Some(conn) = quic_conn {
        let app_handle = app.clone();
        let server_id_clone = server_id.clone();
        let types_clone = types.clone();

        // 创建持久 Stream
        let stream = conn.open_bi().await
            .map_err(|e| format!("创建 Stream 失败: {}", e))?;

        let (mut send, mut recv) = stream;

        // 发送 Subscribe payload
        let request_id = 1; // 使用固定的 request_id
        let subscribe_payload = Envelope::new(request_id, Payload::Subscribe {
            server_id: server_id_clone.clone(),
            types: types_clone.clone(),
        });

        if let Ok(data) = subscribe_payload.encode() {
            use tokio::io::AsyncWriteExt;
            // 发送消息长度
            let len = (data.len() as u32).to_le_bytes();
            if send.write_all(&len).await.is_err() {
                return Err("发送订阅请求失败".into());
            }
            // 发送消息数据
            if send.write_all(&data).await.is_err() {
                return Err("发送订阅请求失败".into());
            }
            send.flush().await.ok();
        }

        // 等待 SubscribeAck 响应
        let mut len_buf = [0u8; 4];
        use tokio::io::AsyncReadExt;
        if recv.read_exact(&mut len_buf).await.is_err() {
            return Err("等待响应超时".into());
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut data_buf = vec![0u8; len];
        if recv.read_exact(&mut data_buf).await.is_err() {
            return Err("读取响应失败".into());
        }

        if let Ok(resp_envelope) = Envelope::decode(&data_buf) {
            if let Payload::SubscribeAck { success, .. } = resp_envelope.payload {
                if !success {
                    return Err("订阅失败".into());
                }
            } else {
                return Err("意外响应".into());
            }
        } else {
            return Err("解析响应失败".into());
        }

        // 启动监听任务
        let task = tokio::spawn(async move {
            tracing::info!("订阅成功: server_id={}", server_id_clone);

            // 监听 Event payload
            loop {
                // 读取消息长度
                let mut len_buf = [0u8; 4];
                match recv.read_exact(&mut len_buf).await {
                    Ok(_) => {
                        let len = u32::from_le_bytes(len_buf) as usize;
                        let mut data_buf = vec![0u8; len];
                        match recv.read_exact(&mut data_buf).await {
                            Ok(_) => {
                                // 解析 Event payload
                                if let Ok(event_envelope) = Envelope::decode(&data_buf) {
                                    if let Payload::Event { event_type, data, timestamp } = event_envelope.payload {
                                        // 转发到前端 Tauri Event
                                        let event_data = serde_json::json!({
                                            "server_id": server_id_clone,
                                            "event_type": event_type,
                                            "data": data,
                                            "timestamp": timestamp,
                                        });

                                        app_handle.emit("subscription_event", event_data).ok();
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("读取事件数据失败: {}", e);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("读取事件长度失败: {}", e);
                        break;
                    }
                }
            }
        });

        // 保存监听任务（使用锁保护）
        {
            let mut conns = manager.connections.lock().unwrap();
            if let Some(active_conn) = conns.get_mut(&server_id) {
                active_conn.subscription_task = Some(task);
            }
        }

        tracing::info!("订阅请求已发送并确认: server_id={}, types={}", server_id, types.len());
        Ok(())
    } else {
        Err("未找到 QUIC Connection".into())
    }
}

#[tauri::command]
pub async fn unsubscribe(
    server_id: String,
    types: Vec<SubscriptionType>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let manager = app.state::<ConnectionManager>();

    // 停止持久 Stream 监听任务
    let task_opt = {
        let mut conns = manager.connections.lock().unwrap();
        if let Some(active_conn) = conns.get_mut(&server_id) {
            active_conn.subscription_task.take()
        } else {
            None
        }
    };

    if let Some(task) = task_opt {
        // 使用 tokio::select! 等待任务完成或超时
        use tokio::time::{sleep, Duration};
        tokio::select! {
            _ = task => {
                tracing::info!("订阅监听任务已正常停止: server_id={}", server_id);
            }
            _ = sleep(Duration::from_secs(2)) => {
                // 超时，强制 abort
                tracing::warn!("订阅监听任务超时，强制停止: server_id={}", server_id);
            }
        }
    } else {
        tracing::info!("未找到订阅监听任务: server_id={}", server_id);
    }

    // 发送 Unsubscribe payload
    let resp = remote_send(server_id.clone(), Payload::Unsubscribe {
        server_id: server_id.clone(),
        types,
    }, app).await?;

    match resp.payload {
        Payload::UnsubscribeAck { success } => {
            if success {
                tracing::info!("取消订阅成功: server_id={}", server_id);
                Ok(())
            } else {
                Err("取消订阅失败".into())
            }
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

// ── QUIC 客户端 ───────────────────────────────────────

async fn try_quic_connect(host: &str, port: u16) -> Result<(quinn::Connection, f64), String> {
    // 安装 CryptoProvider
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 解析域名或 IP 地址
    let addr = if host.contains(':') || host.parse::<std::net::IpAddr>().is_ok() {
        // 已经是 IP 地址格式
        format!("{}:{}", host, port).parse().map_err(|e| format!("地址解析失败: {}", e))?
    } else {
        // 需要解析域名，优先使用 IPv4
        let addr_str = format!("{}:{}", host, port);
        let resolved_addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host(&addr_str)
            .await
            .map_err(|e| format!("DNS 解析失败: {}", e))?
            .collect();

        // 优先选择 IPv4 地址
        resolved_addrs
            .iter()
            .find(|addr| addr.is_ipv4())
            .copied()
            .or_else(|| resolved_addrs.first().copied())
            .ok_or_else(|| "DNS 解析无结果".to_string())?
    };

    // 创建客户端配置（跳过证书验证）
    let client_config = build_quic_client_config()?;

    // 创建 Endpoint
    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .map_err(|e| format!("创建 Endpoint 失败: {}", e))?;
    
    endpoint.set_default_client_config(client_config);

    let start = std::time::Instant::now();
    
    // 添加超时机制（5 秒）
    let conn = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        endpoint
            .connect(addr, "gnome-remote")
            .map_err(|e| format!("发起连接失败: {}", e))?
    )
    .await
    .map_err(|_| "QUIC 连接超时 (5秒)".to_string())?
    .map_err(|e| format!("QUIC 握手失败: {}", e))?;

    Ok((conn, start.elapsed().as_secs_f64() * 1000.0))
}

fn build_quic_client_config() -> Result<quinn::ClientConfig, String> {
    // 跳过证书验证（开发阶段）
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(SkipCertVerification))
        .with_no_client_auth();

    // 传输层配置：设置 idle_timeout 以快速检测死连接
    // idle_timeout: 5 秒无数据收发 → QUIC 自动关闭连接
    // 配合心跳(2s)使用：心跳每 2s 发一次，若 5s 内无响应说明连接已死
    let mut transport = quinn::TransportConfig::default();
    if let Ok(timeout) = quinn::IdleTimeout::try_from(std::time::Duration::from_secs(5)) {
        transport.max_idle_timeout(Some(timeout));
    }

    let mut client = quinn::ClientConfig::new(std::sync::Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
            .map_err(|e| format!("QUIC TLS 配置错误: {}", e))?
    ));
    client.transport_config(std::sync::Arc::new(transport));

    Ok(client)
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