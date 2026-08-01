//! Manager 模块（网关层）
//!
//! 负责监听 QUIC 端口，管理客户端连接，持有 PTY master_fd，直接读写 PTY。

pub mod connection;
pub mod session;
pub mod auth;
pub mod pty_registry;
pub mod worker_manager;
pub mod ipc_server;

pub use connection::ConnectionManager;
pub use session::SessionManager;
pub use pty_registry::PtyRegistry;
pub use worker_manager::WorkerManager;
pub use ipc_server::IpcServer;

use std::sync::Arc;
use anyhow::Result;
use crate::config::AgentConfig;

/// Manager 主结构
pub struct Manager {
    /// PTY 注册表（管理所有 master_fd）
    pty_registry: Arc<PtyRegistry>,

    /// Worker 进程管理器
    worker_manager: Arc<WorkerManager>,

    /// 用户会话管理器
    session_manager: Arc<SessionManager>,

    /// IPC Server（接收 Worker 的 FD）
    ipc_server: Arc<IpcServer>,
}

impl Manager {
    /// 创建新的 Manager
    pub async fn new(config: &AgentConfig) -> Result<Self> {
        // TODO: 实现
        unimplemented!()
    }

    /// 启动 Manager
    pub async fn run(&self) -> Result<()> {
        // TODO: 实现
        unimplemented!()
    }
}