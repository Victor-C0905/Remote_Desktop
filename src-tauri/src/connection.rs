use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::mpsc;

/// frame_mode 默认值: 旧服务端不带该字段时回退到 "json" 帧模式
fn default_frame_mode() -> String {
    "json".to_string()
}

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
        /// 帧模式: "raw"(裸二进制帧) | "json"(旧端回退)
        #[serde(default = "default_frame_mode")]
        frame_mode: String,
        /// 多流并行数(大文件加速,方案 A 独立段+合并)
        /// None 或 1 = 单流(默认);>1 = 客户端期望开 N 个 stream 并行
        #[serde(default)]
        stream_count: Option<u32>,
    },

    /// 文件传输接受响应（Agent → 客户端）
    #[serde(rename = "file_transfer_accept")]
    FileTransferAccept {
        session_id: String,       // 传输会话 ID
        file_size: u64,           // 文件总大小
        chunk_size: u32,          // 确认的分块大小
        mtime: Option<u64>,       // 文件修改时间
        /// 帧模式: "raw" | "json"(旧端回退)
        #[serde(default = "default_frame_mode")]
        frame_mode: String,
        /// 确认的多流并行数(1=单流,>1=服务端确认多流)
        #[serde(default)]
        stream_count: u32,
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

    /// 多流加入握手(客户端 → Agent,后续 stream 的第一帧,方案 A)
    #[serde(rename = "multi_stream_join")]
    MultiStreamJoin {
        session_id: String,       // 关联的传输会话 ID
        stream_index: u32,        // 本 stream 索引(0..N-1)
        offset_start: u64,         // 本 stream 负责的起始偏移
        offset_end: u64,           // 本 stream 负责的结束偏移(-exclusive)
    },

    /// 多流合并完成(Agent → 客户端,所有段写入完成后)
    #[serde(rename = "multi_stream_merge_complete")]
    MultiStreamMergeComplete {
        session_id: String,       // 传输会话 ID
        success: bool,            // 合并是否成功
        error: Option<String>,    // 失败原因
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

    /// 统计查询请求
    #[serde(rename = "get_stats")]
    GetStats {
        stats_type: String,
    },

    /// 统计查询响应
    #[serde(rename = "stats_response")]
    StatsResponse {
        auth: Option<AuthStatsSnapshot>,
        connection: ConnectionStatsSnapshot,
        performance: Option<PerformanceStatsSnapshot>,
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

// ── 统计信息快照 ───────────────────────────────────────

/// 认证统计快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthStatsSnapshot {
    pub total_attempts: u64,
    pub successful: u64,
    pub failed: u64,
    pub locked: u64,
    pub rate_limited: u64,
    pub session_timeout: u64,
    pub password_attempts: u64,
    pub pubkey_attempts: u64,
}

/// 连接统计快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatsSnapshot {
    pub active_connections: u64,
    pub total_connections: u64,
    pub normal_disconnects: u64,
    pub timeout_disconnects: u64,
    pub error_disconnects: u64,
}

/// 性能统计快照
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceStatsSnapshot {
    pub response_times: ResponseTimePercentiles,
    pub total_bytes_transferred: u64,
    pub total_terminal_bytes: u64,
}

/// 响应时间百分位数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseTimePercentiles {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub min: u64,
    pub max: u64,
    pub count: u64,
}

/// 统计响应（用于 get_stats 命令返回）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsResponse {
    pub auth: Option<AuthStatsSnapshot>,
    pub connection: ConnectionStatsSnapshot,
    pub performance: Option<PerformanceStatsSnapshot>,
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

/// 连接丢失的触发源（用于日志区分）
#[derive(Debug, Clone)]
pub enum ConnectionLostSource {
    /// 心跳 Ping 超时
    Heartbeat,
    /// QUIC 连接关闭（idle_timeout / 对端 close）
    QuicClosed,
    /// 业务请求发送失败
    SendFailed,
}

impl std::fmt::Display for ConnectionLostSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionLostSource::Heartbeat => write!(f, "心跳超时"),
            ConnectionLostSource::QuicClosed => write!(f, "QUIC 连接关闭"),
            ConnectionLostSource::SendFailed => write!(f, "发送失败"),
        }
    }
}

enum ClientRequest {
    Send {
        envelope: Envelope,
        response_tx: tokio::sync::oneshot::Sender<Result<Vec<u8>, String>>,
    },
    Disconnect,
    /// 被动检测到连接丢失（由心跳/status/发送失败触发）
    /// 仅用于通知主循环退出，不携带响应通道
    ConnectionLost {
        source: ConnectionLostSource,
    },
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
    cert_fingerprint: Option<String>,
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

    if let Ok((conn, rtt, server_cert_fingerprint)) = quic_result {
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

        // ── 证书钉扎校验（SSH known_hosts 模式） ──────────
        // 1. 已知指纹且匹配 → 跳过确认
        // 2. 首次连接（无已知指纹）→ 展示指纹，用户确认后存储
        // 3. 指纹不匹配 → 警告用户（可能服务器重装或 MITM），由用户决定
        let need_confirm = match &cert_fingerprint {
            None => true,                                   // 首次连接
            Some(known) if known == &server_cert_fingerprint => false,  // 指纹匹配
            Some(_) => true,                                // 指纹不匹配
        };

        if need_confirm {
            let is_first = cert_fingerprint.is_none();
            let fingerprint_display = PinningCertVerifier::format_fingerprint(&server_cert_fingerprint);

            let title = if is_first {
                "首次连接 - 确认服务器证书"
            } else {
                "⚠️ 服务器证书已变更"
            };
            let message = if is_first {
                format!(
                    "这是首次连接到该服务器。\n\n服务器证书指纹 (SHA-256):\n{}\n\n请确认您信任此服务器。",
                    fingerprint_display
                )
            } else {
                format!(
                    "⚠️ 警告：服务器证书与之前记录的不一致！\n可能是服务器重装或存在中间人攻击风险。\n\n新证书指纹 (SHA-256):\n{}\n\n是否信任新证书？",
                    fingerprint_display
                )
            };

            // 使用原生对话框（在独立线程上阻塞，避免阻塞 tokio 运行时）
            let app_clone = app.clone();
            let title_clone = title.to_string();
            let message_clone = message.clone();
            let kind = if is_first {
                tauri_plugin_dialog::MessageDialogKind::Info
            } else {
                tauri_plugin_dialog::MessageDialogKind::Warning
            };
            let accepted = tokio::task::spawn_blocking(move || {
                app_clone.dialog()
                    .message(message_clone)
                    .title(title_clone)
                    .kind(kind)
                    .buttons(tauri_plugin_dialog::MessageDialogButtons::YesNo)
                    .blocking_show()
            }).await.unwrap_or(false);

            if !accepted {
                conn.close(0u32.into(), b"cert rejected");
                return Err("用户拒绝信任服务器证书".to_string());
            }

            // 通知前端存储证书指纹
            app.emit("cert-trusted", serde_json::json!({
                "server_id": &server_id,
                "fingerprint": &server_cert_fingerprint,
            })).map_err(|e| format!("发送证书信任事件失败: {}", e))?;

            tracing::info!("[TLS] 证书已信任并存储: server_id={}", server_id);
        }

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
            // 独立通道设计：
            //   - 心跳直接用 conn.clone() 调用 send_and_receive_quic
            //   - 不走主循环队列，避免被耗时业务请求阻塞
            //   - Quinn 支持同一连接上多个并发 stream，互不干扰
            //
            // 只通知不清理：
            //   - 检测到断连后只发 ConnectionLost 给主循环
            //   - 不执行 cleanup / emit，由主循环统一清理
            let heartbeat_tx = tx.clone();
            let heartbeat_server_id = server_id_clone.clone();
            let heartbeat_conn = conn_clone.clone();
            let heartbeat_task = tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
                loop {
                    interval.tick().await;

                    // 快速退出：连接已关闭
                    if heartbeat_conn.close_reason().is_some() {
                        break;
                    }

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let envelope = Envelope::new(0, Payload::Ping { timestamp: now });

                    // 独立通道：直接调用 send_and_receive_quic，不走主循环队列
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        send_and_receive_quic(&heartbeat_conn, 0, envelope.payload),
                    ).await {
                        Ok(Ok(_)) => { /* Pong 正常收到，连接存活 */ }
                        Ok(Err(e)) => {
                            tracing::warn!("心跳发送失败，连接可能已断开: {}", e);
                            let _ = heartbeat_tx.send(ClientRequest::ConnectionLost {
                                source: ConnectionLostSource::Heartbeat,
                            }).await;
                            break;
                        }
                        Err(_) => {
                            tracing::warn!("Ping 超时 (5s)，连接无响应: {}", heartbeat_server_id);
                            let _ = heartbeat_tx.send(ClientRequest::ConnectionLost {
                                source: ConnectionLostSource::Heartbeat,
                            }).await;
                            break;
                        }
                    }
                }
                // 心跳任务结束：不执行 cleanup / emit，由主循环统一清理
            });
            
            // 连接状态监听任务
            // 只通知主循环，不执行 cleanup / emit
            // 竞态安全：心跳和 status 可能同时检测到断连，都发 ConnectionLost。
            // 主循环处理第一个后 break，第二个消息随 channel drop 自动丢弃。
            let status_conn = conn_clone.clone();
            let status_tx = tx.clone();
            let status_server_id = server_id_clone.clone();
            let status_task = tokio::spawn(async move {
                status_conn.closed().await;
                tracing::warn!("QUIC 连接已关闭: {}", status_server_id);
                let _ = status_tx.send(ClientRequest::ConnectionLost {
                    source: ConnectionLostSource::QuicClosed,
                }).await;
            });
            
            // 主循环需要 tx clone 用于 SendFailed 时通知自己
            let main_tx = tx.clone();

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
                        if result.is_err() {
                            let _ = response_tx.send(Err("连接已断开".into()));
                            // 通过 ConnectionLost 通知自己退出，清理逻辑只有一个入口
                            let _ = main_tx.send(ClientRequest::ConnectionLost {
                                source: ConnectionLostSource::SendFailed,
                            }).await;
                            continue;
                        }
                        let _ = response_tx.send(result);
                    }
                    ClientRequest::Disconnect => break,
                    ClientRequest::ConnectionLost { source } => {
                        tracing::info!("连接丢失（{}）: {}", source, server_id_clone);
                        break;
                    }
                }
            }

            // ════════════════════════════════════════════════════════════
            // 统一清理块（唯一清理入口）
            // 退出原因：Disconnect / ConnectionLost / 主循环自然结束
            // 所有清理集中在此处，保证只执行一次
            // ════════════════════════════════════════════════════════════

            // 1. 停止检测任务（防止心跳/status 在清理后还发 ConnectionLost）
            heartbeat_task.abort();
            status_task.abort();

            // 2. 显式关闭 QUIC 连接（幂等：已关闭则无操作）
            //    主动断开：确保连接立即关闭，释放资源
            //    被动断开：close_reason() 已有值，close() 是无操作
            if conn_clone.close_reason().is_none() {
                conn_clone.close(0u32.into(), b"client closing");
                tracing::info!("QUIC 连接已主动关闭: {}", server_id_clone);
            }

            // 3. 清理传输任务（取消该连接所有未完成的传输）
            let tm = app_handle.state::<std::sync::Arc<crate::transfer::TransferManager>>();
            tm.cleanup_by_connection(&server_id_clone).await;

            // 4. 从 ConnectionManager 移除连接条目
            //    同时 abort subscription_task，避免被动断开时的残留读取日志
            if let Ok(mut conns) = app_handle.state::<ConnectionManager>().connections.lock() {
                if let Some(active_conn) = conns.remove(&server_id_clone) {
                    if let Some(task) = active_conn.subscription_task {
                        task.abort();
                    }
                }
            }

            // 5. 通知前端（只 emit 一次）
            let _ = app_handle.emit("connection-lost", &server_id_clone);

            tracing::info!("连接清理完成: {}", server_id_clone);
        });

        return Ok(info);
    }

    // QUIC 失败，返回错误
    Err(format!("QUIC 连接失败: {}", quic_err_msg))
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_disconnect(server_id: String, app: tauri::AppHandle) -> Result<(), String> {
    tracing::info!("[Connection] 主动断开: {}", server_id);

    // 1. 获取连接句柄（quic_conn + tx）
    let manager = app.state::<ConnectionManager>();
    let conn_info = {
        let conns = manager.connections.lock().unwrap();
        conns.get(&server_id).map(|c| (c.quic_conn.clone(), c.tx.clone()))
    };

    let (quic_conn, tx) = match conn_info {
        Some(v) => v,
        None => return Err("未找到该服务器的连接".into()),
    };

    // 2. 发送 DisconnectRequest（fire-and-forget，不等待响应）
    //    Agent 收到后清理传输会话等资源；收不到则靠 conn.closed() 兜底
    let request_id = manager.next_request_id();
    let envelope = Envelope::new(request_id, Payload::DisconnectRequest {});
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let _ = tx.try_send(ClientRequest::Send { envelope, response_tx });
    // 不等待 response_rx —— 立即关闭连接

    // 3. 立即关闭 QUIC 连接
    //    主循环会在下次 send 时检测到 close_reason，或 status_task 的
    //    conn.closed() 触发，任一都会通过统一清理块完成清理
    if let Some(conn) = quic_conn {
        conn.close(0u32.into(), b"client disconnect");
    }

    // 4. 发送 Disconnect 通知主循环退出（触发统一清理块）
    let _ = tx.send(ClientRequest::Disconnect).await;

    tracing::info!("[Connection] 主动断开完成: {}", server_id);
    Ok(())
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
        Payload::ReadFileResponse { path, content, mtime, size } => {
            // Agent 返回 base64 编码的原始字节（支持二进制文件）
            // 客户端文本编辑器需要 UTF-8 文本，这里解码 base64 并尝试 UTF-8 转换
            use base64::Engine;
            let content_bytes = base64::engine::general_purpose::STANDARD
                .decode(content.as_bytes())
                .map_err(|e| format!("base64 解码失败: {}", e))?;
            let content_text = String::from_utf8(content_bytes)
                .map_err(|e| format!("文件不是有效的 UTF-8 文本（可能为二进制或非 UTF-8 编码）: {}", e))?;
            Ok(RemoteReadFileResponse { path, content: content_text, mtime, size })
        }
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
    // Agent 协议要求 content 为 base64 编码（支持二进制文件）
    // 前端传入的是 UTF-8 文本，这里编码为 base64
    use base64::Engine;
    let content_b64 = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
    let resp = remote_send(server_id, Payload::WriteFileRequest { path, content: content_b64 }, app).await?;
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

#[tauri::command]
pub async fn get_stats(server_id: String, stats_type: String, app: tauri::AppHandle) -> Result<StatsResponse, String> {
    tracing::debug!("[Connection] get_stats: server_id={}, stats_type={}", server_id, stats_type);

    let resp = remote_send(server_id, Payload::GetStats { stats_type }, app).await?;

    match resp.payload {
        Payload::StatsResponse { auth, connection, performance } => {
            Ok(StatsResponse {
                auth,
                connection,
                performance,
            })
        }
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

async fn try_quic_connect(host: &str, port: u16) -> Result<(quinn::Connection, f64, String), String> {
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

    // 创建客户端配置（证书钉扎：提取指纹供后续校验）
    let observed_fingerprint = Arc::new(Mutex::new(None));
    let client_config = build_quic_client_config(observed_fingerprint.clone())?;

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
            .connect(addr, "quirel")
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

    // 提取握手过程中观测到的服务器证书指纹
    let fingerprint = observed_fingerprint
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "未能获取服务器证书指纹".to_string())?;

    Ok((conn, start.elapsed().as_secs_f64() * 1000.0, fingerprint))
}

fn build_quic_client_config(
    observed_fingerprint: Arc<Mutex<Option<String>>>,
) -> Result<quinn::ClientConfig, String> {
    // 证书钉扎：提取服务器证书指纹供后续校验（SSH known_hosts 模式）
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(PinningCertVerifier {
            observed_fingerprint,
        }))
        .with_no_client_auth();

    // 传输层配置：Bbr 拥塞控制 + 大窗口 + keep_alive + idle_timeout
    // idle_timeout: 30 秒无数据收发 → QUIC 自动关闭连接
    // 配合心跳(2s)使用：心跳每 2s 发一次，若连续超时说明连接异常
    // 30s 空闲超时给予网络波动足够的恢复时间，同时不会让僵死连接长期占用资源
    let mut transport = quinn::TransportConfig::default();
    if let Ok(timeout) = quinn::IdleTimeout::try_from(std::time::Duration::from_secs(30)) {
        transport.max_idle_timeout(Some(timeout));
    }
    transport.keep_alive_interval(Some(std::time::Duration::from_secs(5))); // 每5秒发送保持活跃包
    // 流控窗口：8MB per-stream / 64MB connection-wide，与服务端对称
    transport.stream_receive_window(quinn::VarInt::from_u32(8 * 1024 * 1024));  // 8MB
    transport.receive_window(quinn::VarInt::from_u32(64 * 1024 * 1024));         // 64MB
    // Bbr 拥塞控制：相比默认 Cubic 更适合高带宽高延迟链路
    transport.congestion_controller_factory(std::sync::Arc::new(quinn::congestion::BbrConfig::default()));

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

/// 证书钉扎验证器（SSH known_hosts 模式）
///
/// 替代无脑跳过证书验证，实现证书指纹提取：
/// - TLS 握手时提取服务器证书的 SHA-256 指纹
/// - 总是允许握手通过（真正的指纹校验在 remote_connect 中做）
/// - remote_connect 根据已知指纹决定是否需要用户确认
///
/// 安全模型（与 SSH known_hosts 一致）：
/// 1. 首次连接：展示指纹给用户确认，确认后存储
/// 2. 后续连接：自动校验指纹是否匹配
/// 3. 指纹不匹配：警告用户（可能服务器重装或 MITM），由用户决定
#[derive(Debug)]
struct PinningCertVerifier {
    /// 回传本次握手获取的服务器证书指纹（SHA-256，纯十六进制小写）
    observed_fingerprint: Arc<Mutex<Option<String>>>,
}

impl PinningCertVerifier {
    /// 计算证书 DER 数据的 SHA-256 指纹（纯十六进制小写）
    fn compute_fingerprint(cert_der: &[u8]) -> String {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(cert_der);
        let hash = hasher.finalize();
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// 格式化指纹用于显示（每两位用冒号分隔，便于阅读）
    fn format_fingerprint(hex: &str) -> String {
        hex.as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap_or("??"))
            .collect::<Vec<_>>()
            .join(":")
    }
}

impl rustls::client::danger::ServerCertVerifier for PinningCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let fingerprint = Self::compute_fingerprint(end_entity.as_ref());
        tracing::debug!("[TLS] 服务器证书指纹: {}", Self::format_fingerprint(&fingerprint));

        // 回传指纹供 remote_connect 校验
        *self.observed_fingerprint.lock().unwrap() = Some(fingerprint);

        // 允许握手通过（指纹校验在 remote_connect 中做，以便获取实际指纹展示给用户）
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
    use tokio::io::AsyncWriteExt;

    tracing::info!("[PubKeyAuth] 开始公钥认证流程: username={}", username);

    // ── 第一步：解析私钥 ───────────────────────────────
    tracing::info!("[PubKeyAuth] 私钥原始长度={}字节", private_key.len());
    tracing::info!("[PubKeyAuth] 私钥前150字符: {}", &private_key.chars().take(150).collect::<String>());

    tracing::debug!("[PubKeyAuth] 解析私钥（长度={}字节）", private_key.len());

    // 清理私钥内容：
    // 1. 去除BOM标记（\u{FEFF}）
    // 2. 统一换行符（将 \r\n 转为 \n）
    // 3. 去除首尾空白
    let private_key_cleaned = private_key
        .strip_prefix('\u{FEFF}')
        .unwrap_or(&private_key)
        .replace("\r\n", "\n")
        .trim()
        .to_string();

    tracing::debug!("[PubKeyAuth] 私钥清理后长度={}字节", private_key_cleaned.len());

    // 调试：输出私钥的前150个字符
    let preview_len = std::cmp::min(150, private_key_cleaned.len());
    tracing::debug!("[PubKeyAuth] 私钥预览（前{}字符）: {:?}", preview_len, &private_key_cleaned[..preview_len]);

    // 检查是否包含不可见字符
    let has_null = private_key_cleaned.contains('\u{0000}');
    let has_non_printable = private_key_cleaned.chars().any(|c| {
        c.is_control() && c != '\n' && c != '\r' && c != '\t'
    });

    if has_null {
        tracing::error!("[PubKeyAuth] 私钥包含 NUL 字节（\\0），这通常是二进制数据被错误读取导致的");
    }
    if has_non_printable {
        tracing::error!("[PubKeyAuth] 私钥包含不可打印的控制字符");
    }

    // 检查私钥格式
    if !private_key_cleaned.starts_with("-----BEGIN") {
        tracing::error!(
            "[PubKeyAuth] 不支持的私钥格式（应以 -----BEGIN 开头），当前开头: {}",
            &private_key_cleaned[..std::cmp::min(50, private_key_cleaned.len())]
        );
        return Err("不支持的私钥格式。请使用OpenSSH格式的私钥文件（通常以 -----BEGIN OPENSSH PRIVATE KEY----- 开头）".to_string());
    }

    // ── 第二步：自动检测私钥格式并解析 ─────────────────────
    // 自动检测私钥格式并解析
    tracing::debug!("[PubKeyAuth] 解析私钥（长度={}字节）", private_key_cleaned.len());

    let key = parse_private_key_auto(&private_key_cleaned, passphrase.as_deref())
        .map_err(|e| {
            tracing::error!("[PubKeyAuth] 私钥解析失败: {}", e);
            e
        })?;

    tracing::info!("[PubKeyAuth] 私钥解析成功: 算法={}", key.algorithm());

    // 提取公钥并转换为 SSH 格式字符串（如 "ssh-rsa AAAA..."）
    tracing::info!("[PubKeyAuth] 尝试提取公钥...");
    let public_key_str = key.public_key().to_openssh().map_err(|e| {
        tracing::error!("[PubKeyAuth] 公钥 SSH 格式转换失败: {}", e);
        format!("公钥 SSH 格式转换失败: {}", e)
    })?;

    tracing::info!("[PubKeyAuth] 公钥 SSH 格式: {}", &public_key_str[..std::cmp::min(50, public_key_str.len())]);

    let public_key = public_key_str.into_bytes();

    tracing::info!("[PubKeyAuth] 公钥提取成功（长度={}字节）", public_key.len());

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
    tracing::info!("[PubKeyAuth] 使用私钥签名挑战...");
    tracing::info!("[PubKeyAuth] 挑战长度: {} 字节", challenge.len());

    // 使用 SSH 标准签名格式
    // 根据密钥类型选择合适的签名方式
    let signature_bytes = match key.algorithm() {
        ssh_key::Algorithm::Rsa { hash } => {
            tracing::info!("[PubKeyAuth] 使用 RSA 签名，hash={:?}", hash);

            // 获取 RSA 私钥的原始数据
            let rsa_keypair = match key.key_data() {
                ssh_key::private::KeypairData::Rsa(rsa) => rsa,
                _ => return Err("密钥数据不是 RSA 格式".to_string()),
            };

            // 构造 RSA 私钥用于签名
            use rsa::pkcs1v15::SigningKey;
            use rsa::sha2::Sha256;
            use rsa::signature::{Signer, SignatureEncoding};

            // 从 ssh_key 的 RSA keypair 构造 rsa::RsaPrivateKey
            let n = rsa_keypair.public.n.as_bytes();
            let e = rsa_keypair.public.e.as_bytes();
            let d = rsa_keypair.private.d.as_bytes();
            let p = rsa_keypair.private.p.as_bytes();
            let q = rsa_keypair.private.q.as_bytes();

            tracing::info!("[PubKeyAuth] RSA 密钥参数: n={}字节, e={}字节, d={}字节", n.len(), e.len(), d.len());

            // 构造 RSA 私钥
            let rsa_priv = rsa::RsaPrivateKey::from_components(
                rsa::BigUint::from_bytes_be(n),
                rsa::BigUint::from_bytes_be(e),
                rsa::BigUint::from_bytes_be(d),
                vec![
                    rsa::BigUint::from_bytes_be(p),
                    rsa::BigUint::from_bytes_be(q),
                ],
            ).map_err(|e| {
                tracing::error!("[PubKeyAuth] RSA 密钥构造失败: {}", e);
                format!("RSA 密钥构造失败: {}", e)
            })?;

            // 使用 PKCS#1 v1.5 签名（SSH 标准使用的方式）
            let signing_key = SigningKey::<Sha256>::new(rsa_priv);
            let signature = signing_key.sign(&challenge);

            tracing::info!("[PubKeyAuth] RSA 签名成功（长度={}字节）", signature.to_bytes().len());

            // 包装成 SSH 协议格式：
            // string  算法名（如 "rsa-sha2-256"）
            // string  签名数据
            let sig_bytes = signature.to_bytes();
            let algo_name = b"rsa-sha2-256";
            let mut ssh_signature = Vec::new();
            // 算法名（4字节大端长度 + 数据）
            ssh_signature.extend_from_slice(&(algo_name.len() as u32).to_be_bytes());
            ssh_signature.extend_from_slice(algo_name);
            // 签名数据（4字节大端长度 + 数据）
            ssh_signature.extend_from_slice(&(sig_bytes.len() as u32).to_be_bytes());
            ssh_signature.extend_from_slice(&sig_bytes);

            tracing::info!("[PubKeyAuth] SSH格式签名（长度={}字节）", ssh_signature.len());
            ssh_signature
        }
        ssh_key::Algorithm::Ed25519 => {
            tracing::info!("[PubKeyAuth] 使用 Ed25519 签名");

            // 使用 ssh_key 的标准签名
            let sshsig = key.sign("quirel", ssh_key::HashAlg::default(), &challenge).map_err(|e| {
                tracing::error!("[PubKeyAuth] Ed25519 签名失败: {}", e);
                format!("Ed25519 签名失败: {}", e)
            })?;

            let sig_pem = sshsig.to_pem(ssh_key::LineEnding::default()).map_err(|e| {
                tracing::error!("[PubKeyAuth] 签名 PEM 编码失败: {}", e);
                format!("签名 PEM 编码失败: {}", e)
            })?;

            sig_pem.into_bytes()
        }
        other => {
            tracing::error!("[PubKeyAuth] 不支持的密钥算法: {:?}", other);
            return Err(format!("不支持的密钥算法: {:?}", other));
        }
    };

    tracing::info!("[PubKeyAuth] 签名成功（长度={}字节）", signature_bytes.len());

    // ── 第五步：发送签名响应 ─────────────────────────────
    // 注意：服务器端期望在一个新的 Stream 上接收响应
    tracing::info!("[PubKeyAuth] 创建新的 QUIC Stream 发送签名响应...");

    let (mut resp_send, mut resp_recv) = tokio::time::timeout(timeout_duration, conn.open_bi())
        .await
        .map_err(|_| "创建响应 Stream 超时".to_string())?
        .map_err(|e| format!("创建响应 Stream 失败: {}", e))?;

    let response_envelope = Envelope::new(request_id, Payload::AuthPubKeyResponse {
        challenge_id: challenge_id.clone(),
        signature: signature_bytes,
        public_key: public_key.clone(),
    });

    let bytes = response_envelope.encode()?;
    let len = (bytes.len() as u32).to_le_bytes();

    tracing::info!("[PubKeyAuth] 发送签名响应...");

    tokio::time::timeout(timeout_duration, async {
        resp_send.write_all(&len).await.map_err(|e| format!("发送签名长度失败: {}", e))?;
        resp_send.write_all(&bytes).await.map_err(|e| format!("发送签名数据失败: {}", e))?;
        resp_send.flush().await.map_err(|e| format!("刷新发送缓冲区失败: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "发送签名响应超时".to_string())??;

    tracing::info!("[PubKeyAuth] 签名响应已发送，等待最终认证结果...");

    // ── 第六步：接收最终认证结果 ───────────────────────
    let final_data = tokio::time::timeout(timeout_duration, async {
        let mut len_buf = [0u8; 4];
        resp_recv.read_exact(&mut len_buf).await.map_err(|e| format!("读取认证结果长度失败: {}", e))?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;

        let mut data = vec![0u8; resp_len];
        resp_recv.read_exact(&mut data).await.map_err(|e| format!("读取认证结果数据失败: {}", e))?;
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

// ── PEM 格式私钥解析辅助函数 ─────────────────────────────────

/// 解析 PEM 格式的 RSA 私钥（阿里云等云服务商常用）
fn parse_pem_rsa(pem: &str, passphrase: Option<&str>) -> Result<ssh_key::PrivateKey, String> {
    use rsa::RsaPrivateKey;
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;
    use ssh_key::private::{RsaKeypair, KeypairData};

    tracing::debug!("[PEM] 开始解析 PEM RSA 私钥");

    // 检查是否有非空密码（空字符串视为无密码）
    if passphrase.filter(|s| !s.is_empty()).is_some() {
        return Err("加密的PEM私钥暂不支持。请使用ssh-keygen解密：\nssh-keygen -p -f <私钥文件>\n或转换为OpenSSH格式：\nssh-keygen -i -f <PEM私钥> > id_rsa".to_string());
    }

    // 尝试解析未加密的私钥（支持PKCS#1和PKCS#8）
    let rsa_key = if pem.contains("-----BEGIN PRIVATE KEY-----") {
        // PKCS#8 格式
        tracing::debug!("[PEM] 检测到 PKCS#8 格式");
        RsaPrivateKey::from_pkcs8_pem(pem)
            .map_err(|e| {
                tracing::error!("[PEM] PKCS#8 RSA 解析失败: {}", e);
                format!("PKCS#8 RSA 解析失败: {}", e)
            })?
    } else {
        // PKCS#1 格式
        tracing::debug!("[PEM] 检测到 PKCS#1 格式");
        RsaPrivateKey::from_pkcs1_pem(pem)
            .map_err(|e| {
                tracing::error!("[PEM] PKCS#1 RSA 解析失败: {}", e);
                format!("PKCS#1 RSA 解析失败: {}", e)
            })?
    };

    // 直接从 rsa::RsaPrivateKey 转换为 ssh_key 格式
    let rsa_keypair = RsaKeypair::try_from(&rsa_key)
        .map_err(|e| {
            tracing::error!("[PEM] 转换为 SSH 密钥失败: {}", e);
            format!("转换为 SSH 密钥失败: {}", e)
        })?;

    // 构造 ssh_key::PrivateKey
    let private_key = ssh_key::PrivateKey::new(
        KeypairData::Rsa(rsa_keypair),
        ""  // comment
    ).map_err(|e| {
        tracing::error!("[PEM] 构造 SSH 私钥失败: {}", e);
        format!("构造 SSH 私钥失败: {}", e)
    })?;

    Ok(private_key)
}

// ── 私钥自动格式检测 ─────────────────────────────────────

/// 自动检测私钥格式并解析
///
/// 支持的格式（按优先级）：
/// 1. OpenSSH 格式（现代标准）
/// 2. PEM PKCS#8 格式（通用PEM）
/// 3. PEM PKCS#1 RSA 格式（阿里云等云服务商）
fn parse_private_key_auto(key_data: &str, passphrase: Option<&str>) -> Result<ssh_key::PrivateKey, String> {
    tracing::debug!("[KeyParser] 开始自动检测私钥格式");

    // 关键调试信息（INFO级别，确保显示）
    tracing::info!("[KeyParser] 私钥长度: {} 字节", key_data.len());
    tracing::info!("[KeyParser] 私钥前100字符: {}", &key_data.chars().take(100).collect::<String>());
    tracing::info!("[KeyParser] 密码字段: {:?}", passphrase);

    // 1. 尝试 OpenSSH 格式（优先级最高）
    if key_data.contains("-----BEGIN OPENSSH PRIVATE KEY-----") {
        tracing::debug!("[KeyParser] 尝试解析 OpenSSH 格式");
        match parse_openssh_key(key_data, passphrase) {
            Ok(key) => {
                tracing::info!("[KeyParser] ✅ 成功解析 OpenSSH 格式密钥");
                return Ok(key);
            }
            Err(e) => {
                tracing::debug!("[KeyParser] ❌ OpenSSH 格式解析失败: {}", e);
            }
        }
    }

    // 2. 尝试 PEM 格式
    if key_data.contains("-----BEGIN") {
        tracing::debug!("[KeyParser] 尝试解析 PEM 格式");

        // 检查是否加密
        if key_data.contains("ENCRYPTED") {
            tracing::warn!("[KeyParser] 检测到加密私钥");
            if passphrase.is_none() {
                return Err("私钥已加密，请在'私钥密码'字段输入密码。".to_string());
            }
        }

        match parse_pem_rsa(key_data, passphrase) {
            Ok(key) => {
                tracing::info!("[KeyParser] ✅ 成功解析 PEM 格式密钥");
                return Ok(key);
            }
            Err(e) => {
                tracing::debug!("[KeyParser] ❌ PEM 格式解析失败: {}", e);
            }
        }
    }

    // 3. 所有格式都失败，返回友好的错误提示
    Err(generate_key_parse_error(key_data))
}

/// 解析 OpenSSH 格式私钥
fn parse_openssh_key(key_data: &str, passphrase: Option<&str>) -> Result<ssh_key::PrivateKey, String> {
    if let Some(pwd) = passphrase {
        ssh_key::PrivateKey::from_openssh(key_data)
            .map_err(|e| format!("OpenSSH格式解析失败: {}", e))?
            .decrypt(pwd.as_bytes())
            .map_err(|e| format!("私钥解密失败（密码错误）: {}", e))
    } else {
        ssh_key::PrivateKey::from_openssh(key_data)
            .map_err(|e| format!("OpenSSH格式解析失败: {}（如果私钥有密码保护，请输入密码）", e))
    }
}

/// 生成友好的密钥解析错误提示
fn generate_key_parse_error(key_data: &str) -> String {
    let mut error_msg = String::from("无法解析私钥文件。\n\n");

    // 分析可能的问题
    if key_data.contains("PuTTY") {
        error_msg.push_str("检测到 PuTTY 格式（.ppk）。请使用以下方法转换：\n");
        error_msg.push_str("1. 打开 PuTTYgen\n");
        error_msg.push_str("2. 加载您的 .ppk 文件\n");
        error_msg.push_str("3. 点击 'Conversions' -> 'Export OpenSSH key'\n");
        error_msg.push_str("4. 保存为新的 OpenSSH 格式文件\n\n");
    } else if !key_data.contains("-----BEGIN") {
        error_msg.push_str("未检测到有效的私钥格式。请检查：\n");
        error_msg.push_str("1. 文件是否完整（包含 -----BEGIN 和 -----END 标记）\n");
        error_msg.push_str("2. 是否是私钥文件（公钥文件无法用于认证）\n\n");
    } else {
        error_msg.push_str("支持的格式：\n");
        error_msg.push_str("✅ OpenSSH 格式（推荐）\n");
        error_msg.push_str("✅ PEM 格式 - PKCS#1 RSA（阿里云、AWS等云服务商）\n");
        error_msg.push_str("✅ PEM 格式 - PKCS#8（通用PEM格式）\n\n");

        error_msg.push_str("不支持的格式：\n");
        error_msg.push_str("❌ PuTTY 格式（.ppk）- 需要先转换为 OpenSSH 格式\n");
        error_msg.push_str("❌ SSH.com 格式 - 需要先转换\n\n");

        error_msg.push_str("如果私钥有密码保护，请在'私钥密码'字段输入密码。");
    }

    error_msg
}