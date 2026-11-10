// agent/src/collectors/mod.rs
// ✅ 优化: 删除未使用的collectors,仅保留实际使用的metrics模块

pub mod metrics;
pub mod processes;
// 目录监听依赖 worker 的文件元数据口径（worker 仅在 Unix 编译）
#[cfg(unix)]
pub mod file_watcher;

pub use metrics::MetricsCollector;
#[cfg(unix)]
pub use file_watcher::FileWatcher;