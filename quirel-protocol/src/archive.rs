//! 压缩能力探测协议类型（客户端 ↔ Agent）
//!
//! Agent 按登录用户实际执行系统解压工具的样本操作，返回该服务器真实可用的
//! 格式矩阵；客户端据此展示能力并禁用不可用操作。
//!
//! 字段均为显式结构，新增字段必须带 `#[serde(default)]`。

use serde::{Deserialize, Serialize};

/// 服务器压缩能力快照
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveCapabilities {
    /// 探测时间（Unix 毫秒）
    pub probed_at: u64,
    #[serde(default)]
    pub os: ArchiveHostOs,
    #[serde(default)]
    pub tools: Vec<ArchiveTool>,
    #[serde(default)]
    pub formats: Vec<ArchiveFormatCapability>,
}

/// 服务器发行版信息（用于客户端生成安装建议）
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveHostOs {
    /// /etc/os-release 的 ID（如 ubuntu / debian / alpine）
    #[serde(default)]
    pub id: String,
    /// 版本号（VERSION_ID）
    #[serde(default)]
    pub version: String,
    /// 包管理器标识（apt/dnf/apk/pacman/zypper/nix/unknown）
    #[serde(default)]
    pub package_manager: String,
}

/// 探测到的系统工具
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveTool {
    /// 命令名（如 unzip / tar / 7z）
    pub name: String,
    /// 解析到的绝对路径（仅展示与日志，执行仍用命令名以保持白名单语义）
    #[serde(default)]
    pub path: String,
    /// 版本描述（探测失败时为空）
    #[serde(default)]
    pub version: String,
}

/// 解压已存在同名项时的处理策略（由 Agent 映射为各工具参数）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveExtractPolicy {
    /// 覆盖已存在文件
    #[default]
    Overwrite,
    /// 跳过已存在文件
    Skip,
}

/// 压缩包列表操作（Agent 自行构造固定 argv，客户端不传命令参数）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveListRequest {
    /// 压缩包绝对路径
    pub archive_path: String,
    /// 格式标识：zip / tar / tar.gz / tar.bz2 / tar.xz / tar.zst / 7z / rar
    pub format_id: String,
}

/// 压缩包解压操作（entries 为空表示整个压缩包）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveExtractRequest {
    pub archive_path: String,
    /// 目标目录绝对路径（必须已存在）
    pub target_dir: String,
    pub format_id: String,
    /// 精确条目集合；None = 全部
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<String>>,
    #[serde(default)]
    pub policy: ArchiveExtractPolicy,
}

/// 单一格式的能力矩阵
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveFormatCapability {
    /// 格式标识：zip / tar / tar.gz / tar.bz2 / tar.xz / tar.zst / 7z / rar
    pub id: String,
    pub can_list: bool,
    pub can_extract_one: bool,
    pub can_extract_all: bool,
    /// 实际采用的命令名；客户端构造命令时使用（未探测到时为 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// 结构化原因码：tool_missing / probe_failed / decompressor_missing / probe_timeout
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    /// 缺失的工具名（用于安装建议）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_tool: Option<String>,
    /// 技术细节（仅日志/折叠展示，不进用户正文）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}
