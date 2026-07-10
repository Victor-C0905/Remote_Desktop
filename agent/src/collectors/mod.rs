// agent/src/collectors/mod.rs
// ✅ 优化: 删除未使用的collectors,仅保留实际使用的metrics模块

pub mod metrics;

pub use metrics::MetricsCollector;