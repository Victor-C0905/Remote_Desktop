// Envelope + Payload: 客户端↔Agent 线上协议消息格式的单一真相源
//
// 基准搬运自 agent/src/protocol/serde.rs,并按协议统一审计结论修订:
// - 移除 CalculateDiffRequest/CalculateDiffResponse(calc_diff/calc_diff_resp):
//   双端死代码,Agent 端无处理逻辑
// - 并入 GetPathSuggestionsRequest/PathSuggestionsResponse
//   (get_path_suggestions/path_suggestions_resp):来自客户端 connection.rs,
//   Agent 端此前未定义但客户端在用
//
// ⚠️ 线上格式字节级不变:serde tag/content 与各 rename 字符串、字段顺序
// 均为已部署行为,不得改动。

use serde::{Deserialize, Serialize};

use crate::stats::StatsResponse;
use crate::subscription::SubscriptionType;
use crate::types::{default_frame_mode, one, FileDiff, FileEntry, MetricsSnapshot, MountInfo};

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

    /// 密码认证请求
    #[serde(rename = "auth_password_request")]
    AuthPasswordRequest {
        /// 用户名
        username: String,
        /// 密码
        password: String,
    },

    /// 公钥认证请求（第一步：发送公钥）
    #[serde(rename = "auth_pubkey_request")]
    AuthPubKeyRequest {
        /// 用户名
        username: String,
        /// SSH公钥（DER格式）
        public_key: Vec<u8>,
    },

    /// 公钥认证挑战（Agent返回）
    #[serde(rename = "auth_pubkey_challenge")]
    AuthPubKeyChallenge {
        /// 挑战数据（随机生成，用于防重放）
        challenge: Vec<u8>,
        /// 挑战ID（用于后续验证）
        challenge_id: String,
    },

    /// 公钥认证响应（客户端签名后）
    #[serde(rename = "auth_pubkey_response")]
    AuthPubKeyResponse {
        /// 挑战ID
        challenge_id: String,
        /// 签名数据
        signature: Vec<u8>,
        /// 公钥（用于验证）
        public_key: Vec<u8>,
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

    /// 统计查询请求
    #[serde(rename = "get_stats")]
    GetStats {
        /// 统计类型: "auth" | "connection" | "performance" | "all"
        stats_type: String,
    },

    /// 统计查询响应
    /// (newtype 变体复用 crate::stats::StatsResponse,消除双重定义;
    /// tag/content 下 newtype 与内联 struct 变体序列化字节完全相同,线上格式不变)
    #[serde(rename = "stats_response")]
    StatsResponse(StatsResponse),

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
    CurrentUserResponse { username: String, home_dir: String },

    #[serde(rename = "get_mounts")]
    GetMounts,

    #[serde(rename = "mounts_resp")]
    MountsResponse { mounts: Vec<MountInfo> },

    // ===== 文件传输协议扩展 =====

    /// 文件传输请求（客户端 → Agent）
    #[serde(rename = "file_transfer")]
    FileTransferRequest {
        // 传输方向为裸字符串 "upload"/"download"（线上已锁定；曾有 TransferDirection 枚举，因 direction 字段未使用它而已删除）
        direction: String,           // 传输方向（"upload" 或 "download"）
        path: String,                  // 远程文件路径
        file_size: Option<u64>,        // 文件大小（上传时提供）
        chunk_size: Option<u32>,       // 建议的分块大小（可选，默认 64KB）
        resume_from: Option<u64>,      // 断点续传：从哪个字节开始（可选）
        /// 帧模式: "raw"(裸二进制帧,新客户端) | "json"(旧客户端回退)
        /// 旧客户端不带该字段,反序列化时取 default "json" 保持兼容
        #[serde(default = "default_frame_mode")]
        frame_mode: String,
        /// 多流并行数(大文件加速,方案 A 独立段+合并)
        /// None 或 1 = 单流(默认,向后兼容);>1 = 客户端期望开 N 个 stream 并行
        /// 服务端在 Accept 中回填确认的 stream_count(可能小于请求值)
        #[serde(default)]
        stream_count: Option<u32>,
    },

    /// 文件传输接受响应（Agent → 客户端）
    #[serde(rename = "file_transfer_accept")]
    FileTransferAccept {
        session_id: String,       // 传输会话 ID
        file_size: u64,           // 文件总大小
        chunk_size: u32,          // 确认的分块大小（字节）
        mtime: Option<u64>,       // 文件修改时间（下载时提供）
        /// 帧模式: "raw" | "json"(旧端回退)
        /// 服务端按客户端请求的 frame_mode 回填,旧客户端不带该字段时取 default "json"
        #[serde(default = "default_frame_mode")]
        frame_mode: String,
        /// 确认的多流并行数(方案 A)
        /// 1 = 单流(默认,向后兼容);>1 = 服务端确认开 N 个 stream
        /// 客户端按此值开 N 个 bi-stream,每 stream 各发一段 offset
        #[serde(default = "one")]
        stream_count: u32,
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

    // 新增：路径建议
    #[serde(rename = "get_path_suggestions")]
    GetPathSuggestionsRequest { path: String },
    #[serde(rename = "path_suggestions_resp")]
    PathSuggestionsResponse { suggestions: Vec<String> },

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

    /// 多流加入握手(客户端 → Agent,后续 stream 的第一帧)
    ///
    /// 方案 A(独立段+合并):主 stream 走 FileTransferRequest/Accept 握手,
    /// 后续 N-1 个 stream 各自 open_bi 后发送此 payload 加入同一 session,
    /// 声明本 stream 负责的 offset 段。
    #[serde(rename = "multi_stream_join")]
    MultiStreamJoin {
        session_id: String,       // 关联的传输会话 ID(主 stream Accept 返回的)
        stream_index: u32,        // 本 stream 索引(0..N-1,主 stream 隐式为 0)
        offset_start: u64,         // 本 stream 负责的起始偏移
        offset_end: u64,           // 本 stream 负责的结束偏移(-exclusive)
    },

    /// 多流合并完成(Agent → 客户端,主 stream 收到所有段完成后的结果)
    ///
    /// 服务端在所有 N 个 stream 的段文件写入完成后,合并段文件 → 最终文件,
    /// 然后通过主 stream 发送此 payload 通知客户端最终结果。
    #[serde(rename = "multi_stream_merge_complete")]
    MultiStreamMergeComplete {
        session_id: String,       // 传输会话 ID
        success: bool,            // 合并是否成功
        error: Option<String>,    // 失败原因
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
            Payload::AuthPasswordRequest { .. } => "AuthPasswordRequest",
            Payload::AuthPubKeyRequest { .. } => "AuthPubKeyRequest",
            Payload::AuthPubKeyChallenge { .. } => "AuthPubKeyChallenge",
            Payload::AuthPubKeyResponse { .. } => "AuthPubKeyResponse",
            Payload::AuthResponse { .. } => "AuthResponse",
            Payload::GetStats { .. } => "GetStats",
            Payload::StatsResponse(_) => "StatsResponse",
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
            Payload::GetPathSuggestionsRequest { .. } => "GetPathSuggestionsRequest",
            Payload::PathSuggestionsResponse { .. } => "PathSuggestionsResponse",
            Payload::CancelFileTransfer { .. } => "CancelFileTransfer",
            Payload::CancelFileTransferResponse { .. } => "CancelFileTransferResponse",
            Payload::MultiStreamJoin { .. } => "MultiStreamJoin",
            Payload::MultiStreamMergeComplete { .. } => "MultiStreamMergeComplete",
            Payload::Subscribe { .. } => "Subscribe",
            Payload::Unsubscribe { .. } => "Unsubscribe",
            Payload::Event { .. } => "Event",
            Payload::SubscribeAck { .. } => "SubscribeAck",
            Payload::UnsubscribeAck { .. } => "UnsubscribeAck",
            Payload::ApplyDiffRequest { .. } => "ApplyDiffRequest",
            Payload::ApplyDiffResponse { .. } => "ApplyDiffResponse",
            Payload::DisconnectRequest {} => "DisconnectRequest",
            Payload::DisconnectResponse { .. } => "DisconnectResponse",
            Payload::Error { .. } => "Error",
        }
    }
}
