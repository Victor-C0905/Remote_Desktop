// agent/src/protocol/serde.rs
// 线上协议定义已统一至 quirel-protocol crate（单一真相源）
// 此文件仅保留 re-export，维持既有 `crate::protocol::serde::Payload` 等引用路径不变
pub use quirel_protocol::{
    Envelope, Payload, SubscriptionType,
    MetricsSnapshot, DiskInfo, MountInfo, FileEntry,
    FileDiff, DiffType,
    AuthStatsSnapshot, ConnectionStatsSnapshot, PerformanceStatsSnapshot,
    ResponseTimePercentiles, StatsResponse,
    AuthErrorCode,
};
