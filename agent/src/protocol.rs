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