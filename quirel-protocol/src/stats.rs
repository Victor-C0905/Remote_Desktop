// 统计快照:get_stats 查询与 stats_response 推送使用的统计类型
//
// 搬运自 src-tauri/src/connection.rs(客户端版本)。与 agent/src/auth/stats.rs
// 中的快照结构字段完全一致(ConnectionStatsSnapshot/ResponseTimePercentiles
// 的 Default derive 并入自 agent 侧原有需求:全字段 u64 可安全 derive,
// 不影响 serde 线上格式)。

use serde::{Deserialize, Serialize};

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
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
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
    /// 认证统计（仅root可见）
    pub auth: Option<AuthStatsSnapshot>,
    /// 连接统计（root看全局，普通用户看个人）
    pub connection: ConnectionStatsSnapshot,
    /// 性能指标（仅root可见）
    pub performance: Option<PerformanceStatsSnapshot>,
}
