// 基础类型:线上协议 Payload 直接引用的数据结构
//
// 搬运自 agent/src/protocol/serde.rs(default_frame_mode/
// MetricsSnapshot/DiskInfo/MountInfo/FileEntry)与 src-tauri/src/connection.rs
// (FileDiff/DiffType,采用客户端版本:客户端是 FileDiff 的主要生产者,
// skip_serializing_if 保证 None 字段不占用线上流量,Agent 端 Option 字段
// 缺省即回退 None,两端行为一致)。

use serde::{Deserialize, Serialize};

/// frame_mode 默认值: 旧客户端不带该字段时回退到 "json" 帧模式
pub fn default_frame_mode() -> String {
    "json".to_string()
}

/// u32 默认值 1: FileTransferAccept.stream_count 等字段的缺省回退,
/// 缺字段时语义为"单流",而非 `#[serde(default)]` 解码出的 0
pub fn one() -> u32 {
    1
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
