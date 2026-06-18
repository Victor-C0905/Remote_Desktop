// agent/src/protocol.rs
use serde::{Deserialize, Serialize};

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
    TerminalSpawnRequest { shell: String, cols: u16, rows: u16 },

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