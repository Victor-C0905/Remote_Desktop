// agent/src/collectors/mod.rs

pub mod app_logs;
pub mod file_changes;
pub mod metrics;
pub mod process_events;
pub mod service_status;

pub use metrics::MetricsCollector;