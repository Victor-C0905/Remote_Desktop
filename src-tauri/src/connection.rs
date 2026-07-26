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

// ── 认证凭据 ───────────────────────────────────────

/// 认证方式（与前端 AuthMethod 枚举对应）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Password,
    PubKey,
}

/// 认证凭据结构体（与前端 AuthCredentials 接口对应）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// 认证方式
    pub method: AuthMethod,
    /// 用户名
    pub username: String,
    /// 密码（密码认证时使用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// SSH私钥内容（公钥认证时使用）
    #[serde(skip_serializing_if = "Option::is_none", rename = "private_key")]
    pub private_key: Option<String>,
    /// 私钥密码（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
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
    /// 认证响应
    #[serde(rename = "auth_response")]
    AuthResponse {
        /// 认证是否成功
        success: bool,
        /// 错误信息（失败时）
        error: Option<String>,
        /// 会话ID（成功时返回）
        session_id: Option<String>,
    },
    /// 密码认证请求
    #[serde(rename = "auth_password_request")]
    AuthPasswordRequest {
        username: String,
        password: String,
    },
    /// 公钥认证请求（第一步：发送公钥）
    #[serde(rename = "auth_pubkey_request")]
    AuthPubKeyRequest {
        username: String,
        public_key: Vec<u8>,
    },
    /// 公钥认证挑战（Agent返回）
    #[serde(rename = "auth_pubkey_challenge")]
    AuthPubKeyChallenge {
        challenge: Vec<u8>,
        challenge_id: String,
    },
    /// 公钥认证响应（客户端签名后）
    #[serde(rename = "auth_pubkey_response")]
    AuthPubKeyResponse {
        challenge_id: String,
        signature: Vec<u8>,
        public_key: Vec<u8>,
    },
    #[serde(rename = "read_dir")]
    ReadDirRequest { path: String },
    #[serde(rename = "read_dir_resp")]
    ReadDirResponse { path: String, entries: Vec<FileEntry> },
    #[serde(rename = "read_file")]
    ReadFileRequest { path: String },
    #[serde(rename = "read_file_resp")]
    ReadFileResponse {
        path: String,
        content: String,
        mtime: u64, // 文件修改时间（Unix timestamp）
        size: u64,
    },
    #[serde(rename = "write_file")]
    WriteFileRequest { path: String, content: String },
    #[serde(rename = "write_file_resp")]
    WriteFileResponse {
        path: String,
        mtime: u64, // 文件修改时间（写入后的新 mtime）
        size: u64,
    },
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
    TerminalSpawnRequest { shell: String, cols: u16, rows: u16, working_directory: Option<String> },
    #[serde(rename = "terminal_spawn_resp")]
    TerminalSpawnResponse { session_id: String },
    #[serde(rename = "terminal_resize")]
    TerminalResizeRequest { session_id: String, cols: u16, rows: u16 },
    #[serde(rename = "terminal_resize_resp")]
    TerminalResizeResponse,
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

    // ===== 文件传输协议扩展 =====

    /// 文件传输请求（客户端 → Agent）
    #[serde(rename = "file_transfer")]
    FileTransferRequest {
        direction: String,        // "upload" 或 "download"
        path: String,             // 远程文件路径
        file_size: Option<u64>,   // 文件大小（上传时提供）
        chunk_size: Option<u32>,  // 建议的分块大小（可选）
        resume_from: Option<u64>, // 断点续传：从哪个字节开始（可选）
    },

    /// 文件传输接受响应（Agent → 客户端）
    #[serde(rename = "file_transfer_accept")]
    FileTransferAccept {
        session_id: String,       // 传输会话 ID
        file_size: u64,           // 文件总大小
        chunk_size: u32,          // 确认的分块大小
        mtime: Option<u64>,       // 文件修改时间
    },

    /// 文件数据块（双向传输）
    #[serde(rename = "file_chunk")]
    FileChunk {
        session_id: String,       // 传输会话 ID
        seq: u32,                 // 块序号
        data: Vec<u8>,            // 文件数据（原始字节）
        size: u32,                // 实际数据大小
    },

    /// 文件传输完成（双向传输）
    #[serde(rename = "file_transfer_complete")]
    FileTransferComplete {
        session_id: String,       // 传输会话 ID
        success: bool,            // 是否成功
        mtime: Option<u64>,       // 文件修改时间
        error: Option<String>,    // 错误信息
    },

    /// 文件传输进度（Agent → 客户端）
    #[serde(rename = "file_transfer_progress")]
    FileTransferProgress {
        session_id: String,       // 传输会话 ID
        transferred: u64,         // 已传输字节数
        total: u64,               // 总字节数
        speed_bps: u64,           // 传输速度
        eta_secs: u64,            // 预计剩余时间
    },

    /// 取消文件传输（客户端 → Agent）
    #[serde(rename = "cancel_file_transfer")]
    CancelFileTransfer {
        session_id: String,       // 传输会话 ID
        reason: String,           // 取消原因
    },

    /// 取消文件传输响应（Agent → 客户端）
    #[serde(rename = "cancel_file_transfer_resp")]
    CancelFileTransferResponse {
        session_id: String,       // 传输会话 ID
        success: bool,            // 是否成功取消
    },

    /// 检查文件是否存在（客户端 → Agent）
    #[serde(rename = "file_exists")]
    FileExistsRequest {
        path: String,             // 文件路径
    },

    /// 文件存在响应（Agent → 客户端）
    #[serde(rename = "file_exists_resp")]
    FileExistsResponse {
        exists: bool,             // 是否存在
        size: Option<u64>,        // 文件大小（存在时）
        mtime: Option<u64>,       // 修改时间（存在时）
    },

    // 新增：路径建议
    #[serde(rename = "get_path_suggestions")]
    GetPathSuggestionsRequest { path: String },
    #[serde(rename = "path_suggestions_resp")]
    PathSuggestionsResponse { suggestions: Vec<String> },

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

    // 新增：差异同步（文件编辑器流量优化）
    #[serde(rename = "apply_diff")]
    ApplyDiffRequest {
        path: String,
        base_mtime: u64,
        diffs: Vec<FileDiff>,
    },

    #[serde(rename = "apply_diff_resp")]
    ApplyDiffResponse {
        path: String,
        success: bool,
        new_mtime: u64,
        error: Option<String>,
    },

    /// 断开连接请求（客户端 → Agent）
    /// 客户端主动断开前发送，通知 Agent 清理关联资源（传输会话等）
    #[serde(rename = "disconnect")]
    DisconnectRequest {},

    /// 断开连接响应（Agent → 客户端）
    #[serde(rename = "disconnect_resp")]
    DisconnectResponse { success: bool },

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

// 新增：文件差异类型（用于流量优化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    /// 差异类型
    pub diff_type: DiffType,
    /// 行号（从 1 开始）
    pub line_number: usize,
    /// 原内容（replace/delete 时存在）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_content: Option<String>,
    /// 新内容（replace/insert 时存在）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_content: Option<String>,
}

/// 差异类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiffType {
    /// 插入新行
    Insert,
    /// 删除行
    Delete,
    /// 替换行
    Replace,
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
    pub connections: Mutex<HashMap<String, ActiveConnection>>,
    request_counter: Mutex<u32>,
}

pub struct ActiveConnection {
    #[allow(dead_code)]
    info: ConnectionInfo,
    tx: mpsc::Sender<ClientRequest>,
    // QUIC Connection（用于创建持久 Stream）
    pub quic_conn: Option<Arc<quinn::Connection>>,
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

    pub fn next_request_id(&self) -> u32 {
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
#[tracing::instrument(skip(credentials, app), fields(server_id = %server_id, host = %host, port = port))]
pub async fn remote_connect(
    server_id: String,
    host: String,
    port: u16,
    credentials: Option<Credentials>,
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
        let creds = credentials.ok_or_else(|| "缺少认证凭据".to_string())?;

        // 根据 method 执行不同的认证流程
        match creds.method {
            AuthMethod::Password => {
                // 密码认证流程（单步）
                let password = creds.password.ok_or_else(|| "密码认证需要提供密码".to_string())?;
                let auth_payload = Payload::AuthPasswordRequest {
                    username: creds.username.clone(),
                    password,
                };

                // 发送认证请求（增加错误处理）
                let resp = send_and_receive_quic(&conn, manager.next_request_id(), auth_payload).await
                    .map_err(|e| format!("认证请求失败: {}", e))?;

                let envelope = Envelope::decode(&resp)
                    .map_err(|e| format!("解析认证响应失败: {}", e))?;

                // 验证返回类型（增加类型检查）
                match envelope.payload {
                    Payload::AuthResponse { success, error, session_id: _ } => {
                        if !success {
                            // 关闭连接
                            conn.close(0u32.into(), b"authentication failed");
                            return Err(error.unwrap_or_else(|| "认证失败".to_string()));
                        }
                    }
                    other => {
                        conn.close(0u32.into(), b"unexpected response");
                        return Err(format!("期望 AuthResponse，收到: {:?}", other));
                    }
                }
            }
            AuthMethod::PubKey => {
                // 公钥认证流程（多步挑战-响应）
                let private_key = creds.private_key.ok_or_else(|| "公钥认证需要提供私钥".to_string())?;

                tracing::info!("[Connection] 开始公钥认证: username={}", creds.username);

                // 执行公钥认证
                let session_id = perform_pubkey_auth(
                    &conn,
                    creds.username.clone(),
                    private_key,
                    creds.passphrase,
                ).await.map_err(|e| {
                    tracing::error!("[Connection] 公钥认证失败: {}", e);
                    conn.close(0u32.into(), b"authentication failed");
                    e
                })?;

                tracing::info!("[Connection] 公钥认证成功: username={}, session_id={:?}", creds.username, session_id);
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
            //   - 间隔 5s：每 5 秒发一次 Ping 探测连接活性
            //   - 超时 5s：等待 Pong 响应的最长时间
            //   - 最坏延迟：5s(间隔) + 5s(超时) = **10s**
            //   - 最佳延迟：conn.closed() 瞬间触发
            //
            // 双重保障机制：
            //   路径 A: 心跳 Ping/Pong → 应用层检测 (5+5=10s)
            //   路径 B: QUIC idle_timeout(30s) → 传输层自动关闭 → conn.closed()
            //   哪条路径先触发，哪条先通知前端
            let heartbeat_tx = tx.clone();
            let heartbeat_app = app_handle.clone();
            let heartbeat_server_id = server_id_clone.clone();
            let heartbeat_task = tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let envelope = Envelope::new(0, Payload::Ping { timestamp: now });
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    if heartbeat_tx.send(ClientRequest::Send { envelope, response_tx }).await.is_ok() {
                        // 显式超时 5s：等待 Pong 响应，超时即判定连接异常
                        match tokio::time::timeout(
                            std::time::Duration::from_secs(5),
                            response_rx,
                        ).await {
                            Ok(Ok(_)) => { /* Pong 正常收到，连接存活 */ }
                            Ok(Err(e)) => {
                                tracing::warn!("心跳发送失败，连接可能已断开: {}", e);
                                break;
                            }
                            Err(_) => {
                                // 超时未收到 Pong → 连接已死（对端无响应）
                                tracing::warn!("Ping 超时 (5s)，连接无响应: {}", heartbeat_server_id);
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
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_disconnect(server_id: String, app: tauri::AppHandle) -> Result<(), String> {
    tracing::info!("[Connection] 断开连接: {}", server_id);

    // 1. 清理传输任务
    let transfer_manager = app.state::<std::sync::Arc<crate::transfer::TransferManager>>();
    transfer_manager.cleanup_by_connection(&server_id).await;

    // 2. 发送断开通知给 Agent（让 Agent 清理传输会话等资源）
    // 注意：必须在移除连接之前发送，因为发送需要通过 tx 通道
    let manager = app.state::<ConnectionManager>();
    let disconnect_result = {
        let conns = manager.connections.lock().unwrap();
        if let Some(conn) = conns.get(&server_id) {
            let tx = conn.tx.clone();
            // 通过消息循环发送 DisconnectRequest 给 Agent
            let request_id = manager.next_request_id();
            let envelope = Envelope::new(request_id, Payload::DisconnectRequest {});
            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            let send_result = tx.try_send(ClientRequest::Send { envelope, response_tx });
            Some((send_result, response_rx))
        } else {
            None
        }
    };

    // 等待 Agent 的响应（带超时，避免因网络问题无限等待）
    if let Some((send_result, response_rx)) = disconnect_result {
        if send_result.is_ok() {
            match tokio::time::timeout(std::time::Duration::from_secs(3), response_rx).await {
                Ok(Ok(Ok(_))) => {
                    tracing::info!("[Connection] Agent 已确认断开请求: {}", server_id);
                }
                Ok(Ok(Err(e))) => {
                    tracing::warn!("[Connection] Agent 断开请求发送失败（非致命）: {}", e);
                }
                Ok(Err(_)) => {
                    tracing::warn!("[Connection] Agent 断开请求响应通道关闭（非致命）");
                }
                Err(_) => {
                    tracing::warn!("[Connection] Agent 断开请求超时（3s），继续断开");
                }
            }
        } else {
            tracing::warn!("[Connection] 发送断开请求失败（消息队列已满，非致命）");
        }
    }

    // 3. 移除连接并通知消息循环退出
    let tx = {
        let mut conns = manager.connections.lock().unwrap();
        conns.remove(&server_id).map(|c| c.tx)
    };

    if let Some(tx) = tx {
        let _ = tx.send(ClientRequest::Disconnect).await;
        tracing::info!("[Connection] 连接已断开: {}", server_id);
        Ok(())
    } else {
        Err("未找到该服务器的连接".into())
    }
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_ping(server_id: String, app: tauri::AppHandle) -> Result<PingResult, String> {
    tracing::debug!("[Ping] server_id={}", server_id);
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
#[tracing::instrument(skip(payload, app), fields(server_id = %server_id))]
pub async fn remote_send(server_id: String, payload: Payload, app: tauri::AppHandle) -> Result<Envelope, String> {
    let manager = app.state::<ConnectionManager>();
    
    let (tx, request_id) = {
        let conns = manager.connections.lock().unwrap();
        let conn = conns.get(&server_id).ok_or_else(|| {
            tracing::warn!("[RemoteSend] 未找到连接: server_id={}", server_id);
            "未找到连接".to_string()
        })?;
        (conn.tx.clone(), manager.next_request_id())
    };

    let envelope = Envelope::new(request_id, payload);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    
    tx.send(ClientRequest::Send { envelope, response_tx }).await.map_err(|e| {
        tracing::warn!("[RemoteSend] 发送请求失败: server_id={}, error={}", server_id, e);
        "发送请求失败".to_string()
    })?;
    // 外层超时保护：如果内部 send_and_receive_quic 的超时未能触发，此层兜底
    let data = tokio::time::timeout(
        std::time::Duration::from_secs(STREAM_TIMEOUT_SECS + 5), // 比 Stream 超时多 5 秒作为缓冲
        response_rx,
    )
    .await
    .map_err(|_| {
        tracing::warn!("[RemoteSend] 等待响应超时: server_id={}", server_id);
        "等待响应超时".to_string()
    })?
    .map_err(|_| {
        tracing::warn!("[RemoteSend] 等待响应通道关闭: server_id={}", server_id);
        "等待响应超时".to_string()
    })??;
    Envelope::decode(&data)
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_read_dir(server_id: String, path: String, app: tauri::AppHandle) -> Result<RemoteReadDirResponse, String> {
    tracing::debug!("[ReadDir] server_id={}, path={}", server_id, path);
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
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_get_current_user(server_id: String, app: tauri::AppHandle) -> Result<String, String> {
    tracing::debug!("[GetCurrentUser] server_id={}", server_id);
    let resp = remote_send(server_id, Payload::GetCurrentUser, app).await?;
    match resp.payload {
        Payload::CurrentUserResponse { username } => Ok(username),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_get_mounts(server_id: String, app: tauri::AppHandle) -> Result<Vec<MountInfo>, String> {
    tracing::debug!("[GetMounts] server_id={}", server_id);
    let resp = remote_send(server_id, Payload::GetMounts, app).await?;
    match resp.payload {
        Payload::MountsResponse { mounts } => Ok(mounts),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_get_path_suggestions(server_id: String, path: String, app: tauri::AppHandle) -> Result<Vec<String>, String> {
    tracing::debug!("[GetPathSuggestions] server_id={}, path={}", server_id, path);
    let resp = remote_send(server_id, Payload::GetPathSuggestionsRequest { path }, app).await?;
    match resp.payload {
        Payload::PathSuggestionsResponse { suggestions } => Ok(suggestions),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_get_metrics(server_id: String, app: tauri::AppHandle) -> Result<MetricsSnapshot, String> {
    tracing::debug!("[GetMetrics] server_id={}", server_id);
    let resp = remote_send(server_id, Payload::MetricsSubscribeRequest {}, app).await?;
    match resp.payload {
        Payload::MetricsData(metrics) => Ok(metrics),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_read_file(server_id: String, path: String, app: tauri::AppHandle) -> Result<RemoteReadFileResponse, String> {
    tracing::debug!("[ReadFile] server_id={}, path={}", server_id, path);
    let resp = remote_send(server_id, Payload::ReadFileRequest { path }, app).await?;
    match resp.payload {
        Payload::ReadFileResponse { path, content, mtime, size } => Ok(RemoteReadFileResponse { path, content, mtime, size }),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[derive(Debug, Serialize)]
pub struct RemoteReadFileResponse {
    pub path: String,
    pub content: String,
    pub mtime: u64, // 文件修改时间（Unix timestamp）
    pub size: u64,
}

#[tauri::command]
#[tracing::instrument(skip(content, app), fields(server_id = %server_id, path = %path))]
pub async fn remote_write_file(server_id: String, path: String, content: String, app: tauri::AppHandle) -> Result<RemoteWriteFileResponse, String> {
    tracing::info!("[WriteFile] server_id={}, path={}", server_id, path);
    let resp = remote_send(server_id, Payload::WriteFileRequest { path, content }, app).await?;
    match resp.payload {
        Payload::WriteFileResponse { path, mtime, size } => Ok(RemoteWriteFileResponse { path, mtime, size }),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[derive(Debug, Serialize)]
pub struct RemoteWriteFileResponse {
    pub path: String,
    pub mtime: u64, // 文件修改时间（写入后的新 mtime）
    pub size: u64,
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_delete(server_id: String, path: String, app: tauri::AppHandle) -> Result<bool, String> {
    tracing::info!("[DeleteFile] server_id={}, path={}", server_id, path);
    let resp = remote_send(server_id, Payload::DeleteRequest { path }, app).await?;
    match resp.payload {
        Payload::DeleteResponse { success } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_mkdir(server_id: String, path: String, app: tauri::AppHandle) -> Result<bool, String> {
    tracing::debug!("[Connection] remote_mkdir: server_id={}, path={}", server_id, path);

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
    tracing::debug!("[Connection] remote_rename: old={}, new={}", old_path, new_path);

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
    tracing::debug!("[Connection] remote_copy: src={}, dst={}", src, dst);

    let resp = remote_send(server_id, Payload::CopyRequest { src, dst }, app).await?;

    match resp.payload {
        Payload::CopyResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

/// 应用差异到远程文件（流量优化）
///
/// # 参数
/// - `server_id`: 服务器 ID
/// - `path`: 文件路径
/// - `base_mtime`: 基准 mtime（客户端缓存的版本）
/// - `diffs`: 差异列表（客户端计算）
///
/// # 返回
/// 应用差异后的响应（新的 mtime）
#[tauri::command]
pub async fn remote_apply_diff(
    server_id: String,
    path: String,
    base_mtime: u64,
    diffs: Vec<FileDiff>, // 使用本地定义的 FileDiff
    app: tauri::AppHandle
) -> Result<RemoteApplyDiffResponse, String> {
    tracing::debug!("[Connection] remote_apply_diff: server_id={}, path={}, base_mtime={}, diffs={}", 
        server_id, path, base_mtime, diffs.len());

    // 发送差异到 Agent
    let resp = remote_send(
        server_id,
        Payload::ApplyDiffRequest {
            path,
            base_mtime,
            diffs,
        },
        app
    ).await?;

    // 处理响应
    match resp.payload {
        Payload::ApplyDiffResponse { path, success, new_mtime, error } => {
            Ok(RemoteApplyDiffResponse {
                path,
                success,
                new_mtime,
                error,
            })
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

/// 应用差异响应结构体
#[derive(Debug, serde::Serialize)]
pub struct RemoteApplyDiffResponse {
    pub path: String,
    pub success: bool,
    pub new_mtime: u64,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn remote_move(
    server_id: String,
    src: String,
    dst: String,
    app: tauri::AppHandle
) -> Result<bool, String> {
    tracing::debug!("[Connection] remote_move: src={}, dst={}", src, dst);

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
        let stream = tokio::time::timeout(
            std::time::Duration::from_secs(STREAM_TIMEOUT_SECS),
            conn.open_bi(),
        )
        .await
        .map_err(|_| "创建 Stream 超时".to_string())?
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
            // 发送操作添加超时
            let send_result = tokio::time::timeout(
                std::time::Duration::from_secs(STREAM_TIMEOUT_SECS),
                async {
                    send.write_all(&len).await.map_err(|e| format!("发送订阅请求长度失败: {}", e))?;
                    send.write_all(&data).await.map_err(|e| format!("发送订阅请求数据失败: {}", e))?;
                    send.flush().await.ok();
                    Ok::<(), String>(())
                },
            )
            .await;
            match send_result {
                Ok(Ok(())) => {}
                _ => return Err("发送订阅请求超时".into()),
            }
        }

        // 等待 SubscribeAck 响应（添加超时）
        let ack_result = tokio::time::timeout(
            std::time::Duration::from_secs(STREAM_TIMEOUT_SECS),
            async {
                let mut len_buf = [0u8; 4];
                recv.read_exact(&mut len_buf).await.map_err(|_| "读取响应长度失败".to_string())?;
                let len = u32::from_le_bytes(len_buf) as usize;
                let mut data_buf = vec![0u8; len];
                recv.read_exact(&mut data_buf).await.map_err(|_| "读取响应数据失败".to_string())?;
                Ok::<Vec<u8>, String>(data_buf)
            },
        )
        .await;
        let data_buf = match ack_result {
            Ok(Ok(buf)) => buf,
            _ => return Err("等待订阅响应超时".into()),
        };

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
            // 每条消息读取超时 60 秒，防止僵死 Stream
            let per_msg_timeout = std::time::Duration::from_secs(60);
            loop {
                // 读取消息长度（带超时）
                let mut len_buf = [0u8; 4];
                let read_len_result = tokio::time::timeout(per_msg_timeout, recv.read_exact(&mut len_buf)).await;
                match read_len_result {
                    Ok(Ok(_)) => {
                        let len = u32::from_le_bytes(len_buf) as usize;
                        let mut data_buf = vec![0u8; len];
                        // 读取消息数据（带超时）
                        let read_data_result = tokio::time::timeout(per_msg_timeout, recv.read_exact(&mut data_buf)).await;
                        match read_data_result {
                            Ok(Ok(_)) => {
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
                            Ok(Err(e)) => {
                                tracing::warn!("读取事件数据失败: {}", e);
                                break;
                            }
                            Err(_) => {
                                tracing::warn!("读取事件数据超时 ({}s)", per_msg_timeout.as_secs());
                                break;
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("读取事件长度失败: {}", e);
                        break;
                    }
                    Err(_) => {
                        tracing::warn!("读取事件长度超时 ({}s)", per_msg_timeout.as_secs());
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
        format!("{}:{}", host, port).parse().map_err(|e| {
            tracing::warn!("[QUIC] 地址解析失败: host={}:{}, error={}", host, port, e);
            format!("地址解析失败: {}", e)
        })?
    } else {
        // 需要解析域名，优先使用 IPv4
        let addr_str = format!("{}:{}", host, port);
        let resolved_addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host(&addr_str)
            .await
            .map_err(|e| {
                tracing::warn!("[QUIC] DNS 解析失败: host={}, error={}", host, e);
                format!("DNS 解析失败: {}", e)
            })?
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
        .map_err(|e| {
            tracing::warn!("[QUIC] 创建 Endpoint 失败: {}", e);
            format!("创建 Endpoint 失败: {}", e)
        })?;
    
    endpoint.set_default_client_config(client_config);

    let start = std::time::Instant::now();
    
    // 添加超时机制（5 秒）
    let conn = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        endpoint
            .connect(addr, "gnome-remote")
            .map_err(|e| {
                tracing::warn!("[QUIC] 发起连接失败: addr={}, error={}", addr, e);
                format!("发起连接失败: {}", e)
            })?
    )
    .await
    .map_err(|_| {
        tracing::warn!("[QUIC] 连接超时 (5秒): addr={}", addr);
        "QUIC 连接超时 (5秒)".to_string()
    })?
    .map_err(|e| {
        tracing::warn!("[QUIC] 握手失败: addr={}, error={}", addr, e);
        format!("QUIC 握手失败: {}", e)
    })?;

    Ok((conn, start.elapsed().as_secs_f64() * 1000.0))
}

fn build_quic_client_config() -> Result<quinn::ClientConfig, String> {
    // 跳过证书验证（开发阶段）
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(SkipCertVerification))
        .with_no_client_auth();

    // 传输层配置：设置 idle_timeout 以检测死连接
    // idle_timeout: 30 秒无数据收发 → QUIC 自动关闭连接
    // 配合心跳(2s)使用：心跳每 2s 发一次，若连续超时说明连接异常
    // 30s 空闲超时给予网络波动足够的恢复时间，同时不会让僵死连接长期占用资源
    let mut transport = quinn::TransportConfig::default();
    if let Ok(timeout) = quinn::IdleTimeout::try_from(std::time::Duration::from_secs(30)) {
        transport.max_idle_timeout(Some(timeout));
    }
    // 流控制窗口：1MB，防止慢速接收端导致发送端阻塞
    transport.stream_receive_window(quinn::VarInt::from_u32(1024 * 1024)); // 1MB per-stream
    transport.receive_window(quinn::VarInt::from_u32(1024 * 1024));         // 1MB connection-wide

    let mut client = quinn::ClientConfig::new(std::sync::Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
            .map_err(|e| {
                tracing::warn!("[QUIC] TLS 配置错误: {}", e);
                format!("QUIC TLS 配置错误: {}", e)
            })?
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

/// Stream 操作默认超时时间（30秒）
/// 适用于常规请求/响应操作（目录列表、文件读写元数据等）
const STREAM_TIMEOUT_SECS: u64 = 30;

/// 执行公钥认证流程（挑战-响应）
///
/// # 参数
/// - `conn`: QUIC 连接
/// - `username`: 用户名
/// - `private_key`: OpenSSH 格式的私钥内容
/// - `passphrase`: 私钥密码（可选）
///
/// # 返回
/// 成功返回 AuthResponse 的 session_id，失败返回错误信息
async fn perform_pubkey_auth(
    conn: &quinn::Connection,
    username: String,
    private_key: String,
    passphrase: Option<String>,
) -> Result<Option<String>, String> {
    use ssh_key::PrivateKey;
    use tokio::io::AsyncWriteExt;

    tracing::info!("[PubKeyAuth] 开始公钥认证流程: username={}", username);

    // ── 第一步：解析私钥 ───────────────────────────────
    tracing::debug!("[PubKeyAuth] 解析私钥（长度={}字节）", private_key.len());

    let key = if let Some(pwd) = passphrase {
        // 带密码的私钥
        PrivateKey::from_openssh(&private_key)
            .map_err(|e| {
                tracing::error!("[PubKeyAuth] 私钥解析失败: {}", e);
                format!("私钥解析失败: {}", e)
            })?
            .decrypt(pwd.as_bytes())
            .map_err(|e| {
                tracing::error!("[PubKeyAuth] 私钥解密失败（密码错误）: {}", e);
                format!("私钥解密失败（密码错误）: {}", e)
            })?
    } else {
        // 无密码的私钥
        PrivateKey::from_openssh(&private_key).map_err(|e| {
            tracing::error!("[PubKeyAuth] 私钥解析失败: {}", e);
            format!("私钥解析失败: {}", e)
        })?
    };

    tracing::debug!("[PubKeyAuth] 私钥解析成功: 算法={}", key.algorithm());

    // 提取公钥并转换为字节数组
    let public_key = key.public_key().to_bytes().map_err(|e| {
        tracing::error!("[PubKeyAuth] 公钥转换失败: {}", e);
        format!("公钥转换失败: {}", e)
    })?;

    tracing::debug!("[PubKeyAuth] 公钥提取成功（长度={}字节）", public_key.len());

    // ── 第二步：创建 Stream 并发送公钥请求 ───────────────
    tracing::debug!("[PubKeyAuth] 创建 QUIC Stream...");

    let timeout_duration = std::time::Duration::from_secs(STREAM_TIMEOUT_SECS);
    let (mut send, mut recv) = tokio::time::timeout(timeout_duration, conn.open_bi())
        .await
        .map_err(|_| "创建 Stream 超时".to_string())?
        .map_err(|e| format!("创建 Stream 失败: {}", e))?;

    // 发送 AuthPubKeyRequest
    let request_id = 0; // 使用固定的 request_id（认证请求）
    let envelope = Envelope::new(request_id, Payload::AuthPubKeyRequest {
        username: username.clone(),
        public_key: public_key.clone(),
    });

    let bytes = envelope.encode()?;
    let len = (bytes.len() as u32).to_le_bytes();

    tracing::debug!("[PubKeyAuth] 发送公钥请求（{}字节）...", bytes.len());

    tokio::time::timeout(timeout_duration, async {
        send.write_all(&len).await.map_err(|e| format!("发送长度失败: {}", e))?;
        send.write_all(&bytes).await.map_err(|e| format!("发送数据失败: {}", e))?;
        send.flush().await.map_err(|e| format!("刷新发送缓冲区失败: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "发送公钥请求超时".to_string())??;

    tracing::debug!("[PubKeyAuth] 公钥请求已发送，等待挑战...");

    // ── 第三步：接收挑战 ─────────────────────────────────
    let challenge_data = tokio::time::timeout(timeout_duration, async {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await.map_err(|e| format!("读取挑战长度失败: {}", e))?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;

        let mut data = vec![0u8; resp_len];
        recv.read_exact(&mut data).await.map_err(|e| format!("读取挑战数据失败: {}", e))?;
        Ok::<Vec<u8>, String>(data)
    })
    .await
    .map_err(|_| "接收挑战超时".to_string())??;

    let challenge_envelope = Envelope::decode(&challenge_data)?;
    let (challenge, challenge_id) = match challenge_envelope.payload {
        Payload::AuthPubKeyChallenge { challenge, challenge_id } => {
            tracing::debug!("[PubKeyAuth] 收到挑战（长度={}字节，id={}）", challenge.len(), challenge_id);
            (challenge, challenge_id)
        }
        Payload::Error { message, .. } => {
            tracing::error!("[PubKeyAuth] Agent 返回错误: {}", message);
            return Err(format!("Agent 错误: {}", message));
        }
        other => {
            tracing::error!("[PubKeyAuth] 期望挑战，收到: {:?}", other);
            return Err(format!("期望 AuthPubKeyChallenge，收到: {:?}", other));
        }
    };

    // ── 第四步：签名挑战 ─────────────────────────────────
    tracing::debug!("[PubKeyAuth] 使用私钥签名挑战...");

    // 使用 SSH 签名格式（namespace + hash_alg + msg）
    // 注意：Agent 端目前简化实现，不验证签名内容，但为了未来兼容性使用正确格式
    let sshsig = key.sign("gnome-remote", ssh_key::HashAlg::default(), &challenge).map_err(|e| {
        tracing::error!("[PubKeyAuth] 签名失败: {}", e);
        format!("签名失败: {}", e)
    })?;

    // 将 SshSig 编码为 PEM 格式，然后转换为字节数组
    // Agent 端可以根据需要解析 PEM 或直接使用
    let signature_pem = sshsig.to_pem(ssh_key::LineEnding::default()).map_err(|e| {
        tracing::error!("[PubKeyAuth] 签名 PEM 编码失败: {}", e);
        format!("签名 PEM 编码失败: {}", e)
    })?;

    let signature_bytes = signature_pem.into_bytes();

    tracing::debug!("[PubKeyAuth] 签名成功（长度={}字节）", signature_bytes.len());

    // ── 第五步：发送签名响应 ─────────────────────────────
    let response_envelope = Envelope::new(request_id, Payload::AuthPubKeyResponse {
        challenge_id: challenge_id.clone(),
        signature: signature_bytes,
        public_key: public_key.clone(),
    });

    let bytes = response_envelope.encode()?;
    let len = (bytes.len() as u32).to_le_bytes();

    tracing::debug!("[PubKeyAuth] 发送签名响应...");

    tokio::time::timeout(timeout_duration, async {
        send.write_all(&len).await.map_err(|e| format!("发送签名长度失败: {}", e))?;
        send.write_all(&bytes).await.map_err(|e| format!("发送签名数据失败: {}", e))?;
        send.flush().await.map_err(|e| format!("刷新发送缓冲区失败: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "发送签名响应超时".to_string())??;

    tracing::debug!("[PubKeyAuth] 签名响应已发送，等待最终认证结果...");

    // ── 第六步：接收最终认证结果 ───────────────────────
    let final_data = tokio::time::timeout(timeout_duration, async {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await.map_err(|e| format!("读取认证结果长度失败: {}", e))?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;

        let mut data = vec![0u8; resp_len];
        recv.read_exact(&mut data).await.map_err(|e| format!("读取认证结果数据失败: {}", e))?;
        Ok::<Vec<u8>, String>(data)
    })
    .await
    .map_err(|_| "接收认证结果超时".to_string())??;

    let final_envelope = Envelope::decode(&final_data)?;
    match final_envelope.payload {
        Payload::AuthResponse { success, error, session_id } => {
            if success {
                tracing::info!("[PubKeyAuth] 公钥认证成功: username={}, session_id={:?}", username, session_id);
                Ok(session_id)
            } else {
                tracing::error!("[PubKeyAuth] 公钥认证失败: {:?}", error);
                Err(error.unwrap_or_else(|| "公钥认证失败".to_string()))
            }
        }
        Payload::Error { message, .. } => {
            tracing::error!("[PubKeyAuth] Agent 返回错误: {}", message);
            Err(format!("Agent 错误: {}", message))
        }
        other => {
            tracing::error!("[PubKeyAuth] 期望认证结果，收到: {:?}", other);
            Err(format!("期望 AuthResponse，收到: {:?}", other))
        }
    }
}

async fn send_and_receive_quic(conn: &quinn::Connection, request_id: u32, payload: Payload) -> Result<Vec<u8>, String> {
    let timeout_duration = std::time::Duration::from_secs(STREAM_TIMEOUT_SECS);

    // Stream 打开也需要超时保护，避免在连接异常时无限等待
    let (mut send, mut recv) = tokio::time::timeout(timeout_duration, conn.open_bi())
        .await
        .map_err(|_| {
            tracing::warn!("[QUIC] 打开 Stream 超时: request_id={}", request_id);
            "打开 Stream 超时".to_string()
        })?
        .map_err(|e| {
            tracing::warn!("[QUIC] 打开 Stream 失败: request_id={}, error={}", request_id, e);
            format!("打开 Stream 失败: {}", e)
        })?;

    let bytes = Envelope::new(request_id, payload).encode()?;
    let len = (bytes.len() as u32).to_le_bytes();

    // 发送操作也需要超时
    tokio::time::timeout(timeout_duration, async {
        send.write_all(&len).await.map_err(|e| format!("发送长度失败: {}", e))?;
        send.write_all(&bytes).await.map_err(|e| format!("发送数据失败: {}", e))?;
        send.finish().map_err(|e| format!("关闭写入流失败: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| {
        tracing::warn!("[QUIC] 发送数据超时: request_id={}", request_id);
        "发送数据超时".to_string()
    })??;

    // 接收操作也需要超时
    let data = tokio::time::timeout(timeout_duration, async {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await.map_err(|e| format!("读取响应长度失败: {}", e))?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;

        let mut data = vec![0u8; resp_len];
        recv.read_exact(&mut data).await.map_err(|e| format!("读取响应数据失败: {}", e))?;
        Ok::<Vec<u8>, String>(data)
    })
    .await
    .map_err(|_| {
        tracing::warn!("[QUIC] 接收响应超时: request_id={}", request_id);
        "接收响应超时".to_string()
    })??;

    Ok(data)
}