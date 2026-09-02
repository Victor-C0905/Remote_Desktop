// quirel-protocol: 客户端↔Agent 线上协议单一真相源
//
// 本 crate 定义的类型同时被两端引用：
// - src-tauri (Tauri 客户端)
// - agent (远程 Agent 服务端)
//
// ⚠️ 线上兼容性规则：
// - 不得改变任何 serde rename 字符串（线上 tag）
// - 不得移除字段；新增字段必须带 #[serde(default)]
// - tests/wire_compat.rs 的 golden 用例锁死字节级格式

pub mod envelope;
pub mod stats;
pub mod subscription;
pub mod types;

pub use envelope::{Envelope, Payload};
pub use stats::{
    AuthStatsSnapshot, ConnectionStatsSnapshot, PerformanceStatsSnapshot,
    ResponseTimePercentiles, StatsResponse,
};
pub use subscription::SubscriptionType;
pub use types::{
    DiffType, DiskInfo, FileDiff, FileEntry, MetricsSnapshot, MountInfo,
};
