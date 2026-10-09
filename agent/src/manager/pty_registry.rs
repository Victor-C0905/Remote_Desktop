//! PTY 注册表
//!
//! 管理所有活动的 Session 连接。
//! 替代旧的 master_fd 直接读写模式。
//! PtySession 不再持有 master_fd，而是持有 SessionConnection（UnixSocket 连接）。
//!
//! # 架构变更
//!
//! 旧架构：Manager 持有 master_fd，直接 read/write/ioctl
//! 新架构：Manager 持有 SessionConnection，通过 UnixSocket 帧协议与 Session 进程通信
//!        Session 进程持有 master_fd 并做 I/O 中继

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use anyhow::Result;
use tracing::{info, debug};

use crate::manager::session_connection::SessionConnection;

/// PTY 会话信息
#[derive(Debug, Clone)]
pub struct PtySession {
    /// 会话 ID
    pub session_id: String,

    /// Session 连接（替代 master_fd）
    ///
    /// 使用 Arc 共享，允许多个任务（pty_output 读循环、quic.rs 输入写入）并发访问。
    /// SessionConnection 内部已用 into_split 拆分读写半部，读写互不阻塞。
    pub connection: Arc<SessionConnection>,

    /// 用户信息
    pub user_info: UserInfo,

    /// 创建时间
    pub created_at: SystemTime,
}

/// 用户信息（简化版）
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
}

impl UserInfo {
    /// 创建新的用户信息
    pub fn new(username: String, uid: u32, gid: u32) -> Self {
        Self { username, uid, gid }
    }
}

/// PTY 注册表
pub struct PtyRegistry {
    /// session_id -> PtySession
    sessions: Arc<RwLock<HashMap<String, PtySession>>>,
}

impl PtyRegistry {
    /// 创建新的 PTY 注册表
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册 PTY 会话
    pub async fn register(&self, session: PtySession) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        info!("注册 PTY 会话: session_id={}", session.session_id);
        sessions.insert(session.session_id.clone(), session);
        Ok(())
    }

    /// 注销 PTY 会话（发送 Close 给 Session 进程）
    ///
    /// # 流程
    ///
    /// 1. 从注册表移除会话
    /// 2. 发送 Close 帧给 Session 进程（通知其 kill bash 并退出）
    ///
    /// # 注意
    ///
    /// 先移除再发 Close，确保即使 Close 发送失败也不会残留注册表记录。
    pub async fn unregister(&self, session_id: &str) -> Result<Option<PtySession>> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.remove(session_id) {
            info!("注销 PTY 会话: session_id={}", session_id);
            // 发送 Close 给 Session 进程（忽略错误：进程可能已退出）
            let _ = session.connection.send_close().await;
            Ok(Some(session))
        } else {
            Ok(None)
        }
    }

    /// 获取 PTY 会话
    pub async fn get(&self, session_id: &str) -> Option<PtySession> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// 获取所有活动会话的 ID
    pub async fn list_sessions(&self) -> Vec<String> {
        let sessions = self.sessions.read().await;
        sessions.keys().cloned().collect()
    }

    /// 写入数据到 PTY（发送键盘输入给 Session 进程）
    ///
    /// # 参数
    ///
    /// - `session_id`: PTY 会话 ID
    /// - `data`: 键盘输入字节流
    pub async fn write(&self, session_id: &str, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        // 短暂持锁：clone Arc 后立即释放，避免阻塞 unregister
        let connection = {
            let sessions = self.sessions.read().await;
            sessions.get(session_id)
                .map(|s| Arc::clone(&s.connection))
                .ok_or_else(|| anyhow::anyhow!("PTY 会话不存在: {}", session_id))?
        };

        connection.send_input(data).await?;
        debug!("PTY 写入成功: session_id={}, len={}", session_id, data.len());
        Ok(())
    }

    /// 接收 PTY 输出（从 Session 进程读取一帧）
    ///
    /// 由 pty_output 任务循环调用。
    ///
    /// # 返回
    ///
    /// - `(msg_type, data)`: 消息类型（PTY_OUTPUT=0x02 / EOF=0x04）和数据
    pub async fn read(&self, session_id: &str) -> Result<(u8, Vec<u8>)> {
        // 短暂持锁：clone Arc 后立即释放
        let connection = {
            let sessions = self.sessions.read().await;
            sessions.get(session_id)
                .map(|s| Arc::clone(&s.connection))
                .ok_or_else(|| anyhow::anyhow!("PTY 会话不存在: {}", session_id))?
        };

        let result = connection.recv_frame().await?;
        debug!("PTY 读取成功: session_id={}, msg_type=0x{:02x}, len={}",
            session_id, result.0, result.1.len());
        Ok(result)
    }

    /// 调整终端大小（发送 Resize 帧给 Session 进程）
    ///
    /// # 参数
    ///
    /// - `session_id`: PTY 会话 ID
    /// - `cols`: 列数
    /// - `rows`: 行数
    pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<()> {
        // 短暂持锁：clone Arc 后立即释放
        let connection = {
            let sessions = self.sessions.read().await;
            sessions.get(session_id)
                .map(|s| Arc::clone(&s.connection))
                .ok_or_else(|| anyhow::anyhow!("PTY 会话不存在: {}", session_id))?
        };

        connection.send_resize(cols, rows).await?;
        info!("PTY resize: session_id={}, cols={}, rows={}", session_id, cols, rows);
        Ok(())
    }

    /// 获取 Session 连接的共享句柄
    ///
    /// 用于需要长期持有连接的场景（如 pty_output 任务自己的读循环）。
    pub async fn get_connection(&self, session_id: &str) -> Option<Arc<SessionConnection>> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).map(|s| Arc::clone(&s.connection))
    }
}

impl Default for PtyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_and_get() {
        let registry = PtyRegistry::new();

        // 注册一个会话（使用不存在的连接，仅测试注册表逻辑）
        // 注意：SessionConnection 无法在测试中轻松构造（需要真实 socket），
        // 这里只测试 list_sessions 和 get 的 None 路径
        assert!(registry.get("nonexistent").await.is_none());
        assert!(registry.list_sessions().await.is_empty());
    }

    #[tokio::test]
    async fn test_unregister_nonexistent() {
        let registry = PtyRegistry::new();
        let result = registry.unregister("nonexistent").await.unwrap();
        assert!(result.is_none());
    }
}
