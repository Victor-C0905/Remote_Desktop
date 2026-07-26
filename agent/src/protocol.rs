// agent/src/protocol.rs
use serde::{Deserialize, Serialize};
use crate::diff::FileDiff; // 导入差异类型

/// 文件传输方向
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum TransferDirection {
    /// 上传：本地 → 远程
    Upload,
    /// 下载：远程 → 本地
    Download,
}

/// 订阅类型枚举
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", content = "params")]
pub enum SubscriptionType {
    // 系统监控
    #[serde(rename = "metrics")]
    Metrics {
        interval_secs: Option<u64>, // 可选，默认使用配置值
    },

    // 文件变化监控（后续实现）
    #[serde(rename = "file_changes")]
    FileChanges {
        path: String,
        recursive: Option<bool>,
    },

    // 进程事件（后续实现）
    #[serde(rename = "process_events")]
    ProcessEvents {
        interval_secs: Option<u64>,
    },

    // 应用日志（后续实现）
    #[serde(rename = "app_logs")]
    AppLogs {
        app_name: String,
        level: Option<String>,
    },

    // 服务状态（后续实现）
    #[serde(rename = "service_status")]
    ServiceStatus {
        service: String,
        interval_secs: Option<u64>,
    },
}

impl SubscriptionType {
    /// 获取订阅类型名称
    pub fn type_name(&self) -> String {
        match self {
            SubscriptionType::Metrics { .. } => "metrics",
            SubscriptionType::FileChanges { .. } => "file_changes",
            SubscriptionType::ProcessEvents { .. } => "process_events",
            SubscriptionType::AppLogs { .. } => "app_logs",
            SubscriptionType::ServiceStatus { .. } => "service_status",
        }.to_string()
    }
}

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

    /// 密码认证请求
    #[serde(rename = "auth_password_request")]
    AuthPasswordRequest {
        /// 用户名
        username: String,
        /// 密码
        password: String,
    },

    /// 公钥认证请求（挑战-响应模式）
    #[serde(rename = "auth_pubkey_request")]
    AuthPubKeyRequest {
        /// 用户名
        username: String,
        /// SSH公钥（DER格式，即原始二进制格式）
        public_key: Vec<u8>,
        /// 签名数据（客户端使用私钥对challenge进行签名）
        signature: Vec<u8>,
        /// 服务端生成的挑战数据（用于防止重放攻击）
        challenge: Vec<u8>,
    },

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
    ReadFileResponse {
        path: String,
        content: String, // base64 编码（支持二进制）
        mtime: u64,      // 文件修改时间（Unix timestamp）
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

    #[serde(rename = "terminal_spawn")]
    TerminalSpawnRequest {
        shell: String,
        cols: u16,
        rows: u16,
        working_directory: Option<String>,  // 新增字段
    },

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
        direction: String,           // 传输方向（"upload" 或 "download"）
        path: String,                  // 远程文件路径
        file_size: Option<u64>,        // 文件大小（上传时提供）
        chunk_size: Option<u32>,       // 建议的分块大小（可选，默认 64KB）
        resume_from: Option<u64>,      // 断点续传：从哪个字节开始（可选）
    },

    /// 文件传输接受响应（Agent → 客户端）
    #[serde(rename = "file_transfer_accept")]
    FileTransferAccept {
        session_id: String,       // 传输会话 ID
        file_size: u64,           // 文件总大小
        chunk_size: u32,          // 确认的分块大小（字节）
        mtime: Option<u64>,       // 文件修改时间（下载时提供）
    },

    /// 文件数据块（双向传输）
    #[serde(rename = "file_chunk")]
    FileChunk {
        session_id: String,       // 传输会话 ID
        seq: u32,                 // 块序号（从 1 开始）
        data: Vec<u8>,            // 文件数据（原始字节，serde_json 会自动 base64 编码）
        size: u32,                // 实际数据大小（字节）
    },

    /// 文件传输完成（双向传输）
    #[serde(rename = "file_transfer_complete")]
    FileTransferComplete {
        session_id: String,       // 传输会话 ID
        success: bool,            // 是否成功
        mtime: Option<u64>,       // 文件修改时间（上传成功后返回）
        error: Option<String>,    // 错误信息（失败时）
    },

    /// 文件传输进度（Agent → 客户端，主动推送）
    #[serde(rename = "file_transfer_progress")]
    FileTransferProgress {
        session_id: String,       // 传输会话 ID
        transferred: u64,         // 已传输字节数
        total: u64,               // 总字节数
        speed_bps: u64,           // 传输速度（字节/秒）
        eta_secs: u64,            // 预计剩余时间（秒）
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

    // 新增：通用订阅
    #[serde(rename = "subscribe")]
    Subscribe {
        server_id: String,
        types: Vec<SubscriptionType>, // 支持同时订阅多种类型
    },

    // 新增：通用取消订阅
    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        server_id: String,
        types: Vec<SubscriptionType>, // 支持取消部分订阅
    },

    // 新增：通用事件推送
    #[serde(rename = "event")]
    Event {
        event_type: String,           // "metrics" / "file_changes" / ...
        data: serde_json::Value,      // 事件数据（动态类型）
        timestamp: u64,               // 事件时间戳
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

    // 文件编辑器差异同步（流量优化）
    #[serde(rename = "calc_diff")]
    CalculateDiffRequest {
        path: String,           // 文件路径
        old_content: String,    // 原内容（客户端缓存）
        new_content: String,    // 新内容（用户修改后）
    },

    #[serde(rename = "calc_diff_resp")]
    CalculateDiffResponse {
        path: String,           // 文件路径
        diffs: Vec<FileDiff>,   // 差异列表
        mtime: u64,             // 文件修改时间（Unix timestamp）
    },

    #[serde(rename = "apply_diff")]
    ApplyDiffRequest {
        path: String,           // 文件路径
        base_mtime: u64,        // 基准 mtime（客户端缓存的版本）
        diffs: Vec<FileDiff>,   // 差异列表
    },

    #[serde(rename = "apply_diff_resp")]
    ApplyDiffResponse {
        path: String,           // 文件路径
        success: bool,          // 是否成功
        new_mtime: u64,         // 新的 mtime（写入后）
        error: Option<String>,  // 错误信息（如果失败）
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

impl Payload {
    /// 获取 Payload 变体名称（用于日志记录）
    pub fn type_name(&self) -> &'static str {
        match self {
            Payload::Ping { .. } => "Ping",
            Payload::Pong { .. } => "Pong",
            Payload::AuthRequest { .. } => "AuthRequest",
            Payload::AuthPasswordRequest { .. } => "AuthPasswordRequest",
            Payload::AuthPubKeyRequest { .. } => "AuthPubKeyRequest",
            Payload::AuthResponse { .. } => "AuthResponse",
            Payload::MetricsSubscribeRequest {} => "MetricsSubscribeRequest",
            Payload::MetricsData(_) => "MetricsData",
            Payload::ReadDirRequest { .. } => "ReadDirRequest",
            Payload::ReadDirResponse { .. } => "ReadDirResponse",
            Payload::ReadFileRequest { .. } => "ReadFileRequest",
            Payload::ReadFileResponse { .. } => "ReadFileResponse",
            Payload::WriteFileRequest { .. } => "WriteFileRequest",
            Payload::WriteFileResponse { .. } => "WriteFileResponse",
            Payload::DeleteRequest { .. } => "DeleteRequest",
            Payload::DeleteResponse { .. } => "DeleteResponse",
            Payload::MkdirRequest { .. } => "MkdirRequest",
            Payload::MkdirResponse { .. } => "MkdirResponse",
            Payload::RenameRequest { .. } => "RenameRequest",
            Payload::RenameResponse { .. } => "RenameResponse",
            Payload::CopyRequest { .. } => "CopyRequest",
            Payload::CopyResponse { .. } => "CopyResponse",
            Payload::MoveRequest { .. } => "MoveRequest",
            Payload::MoveResponse { .. } => "MoveResponse",
            Payload::TerminalSpawnRequest { .. } => "TerminalSpawnRequest",
            Payload::TerminalSpawnResponse { .. } => "TerminalSpawnResponse",
            Payload::TerminalResizeRequest { .. } => "TerminalResizeRequest",
            Payload::TerminalResizeResponse => "TerminalResizeResponse",
            Payload::TerminalData { .. } => "TerminalData",
            Payload::GetCurrentUser => "GetCurrentUser",
            Payload::CurrentUserResponse { .. } => "CurrentUserResponse",
            Payload::GetMounts => "GetMounts",
            Payload::MountsResponse { .. } => "MountsResponse",
            Payload::FileTransferRequest { .. } => "FileTransferRequest",
            Payload::FileTransferAccept { .. } => "FileTransferAccept",
            Payload::FileChunk { .. } => "FileChunk",
            Payload::FileTransferComplete { .. } => "FileTransferComplete",
            Payload::FileTransferProgress { .. } => "FileTransferProgress",
            Payload::FileExistsRequest { .. } => "FileExistsRequest",
            Payload::FileExistsResponse { .. } => "FileExistsResponse",
            Payload::CancelFileTransfer { .. } => "CancelFileTransfer",
            Payload::CancelFileTransferResponse { .. } => "CancelFileTransferResponse",
            Payload::Subscribe { .. } => "Subscribe",
            Payload::Unsubscribe { .. } => "Unsubscribe",
            Payload::Event { .. } => "Event",
            Payload::SubscribeAck { .. } => "SubscribeAck",
            Payload::UnsubscribeAck { .. } => "UnsubscribeAck",
            Payload::CalculateDiffRequest { .. } => "CalculateDiffRequest",
            Payload::CalculateDiffResponse { .. } => "CalculateDiffResponse",
            Payload::ApplyDiffRequest { .. } => "ApplyDiffRequest",
            Payload::ApplyDiffResponse { .. } => "ApplyDiffResponse",
            Payload::DisconnectRequest {} => "DisconnectRequest",
            Payload::DisconnectResponse { .. } => "DisconnectResponse",
            Payload::Error { .. } => "Error",
        }
    }
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

/// 挂载点信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountInfo {
    pub mount_point: String,
    pub device: String,
    pub filesystem: String,
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