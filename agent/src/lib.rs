//! Quireld 库
//!
//! 提供远程 Agent 服务端功能。

pub mod auth;
pub mod audit;
pub mod cert;
pub mod collectors;
pub mod config;
pub mod event_bus;
pub mod file_stream;
pub mod handler;
pub mod manager;
pub mod protocol;
pub mod server;
pub mod subscription;
pub mod diff;
pub mod transfer_session;

// 仅在 Unix 系统上编译 worker 模块
#[cfg(unix)]
pub mod worker;