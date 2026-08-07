//! PTY 注册表
//!
//! 管理所有活动的 PTY master_fd，并提供直接的 PTY 读写功能。
//! 仅在 Unix 系统上可用。

use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use anyhow::Result;
use tracing::{info, debug};

/// PTY 会话信息
#[derive(Debug, Clone)]
pub struct PtySession {
    /// 会话 ID
    pub session_id: String,

    /// PTY master 文件描述符
    pub master_fd: RawFd,

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
        sessions.insert(session.session_id.clone(), session);
        Ok(())
    }

    /// 注销 PTY 会话
    pub async fn unregister(&self, session_id: &str) -> Result<Option<PtySession>> {
        let mut sessions = self.sessions.write().await;
        Ok(sessions.remove(session_id))
    }

    /// 获取 PTY 会话
    pub async fn get(&self, session_id: &str) -> Option<PtySession> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// 获取 PTY master_fd
    ///
    /// # 参数
    /// - `session_id`: PTY 会话 ID
    ///
    /// # 返回
    /// 返回 master_fd 或错误
    pub async fn get_fd(&self, session_id: &str) -> Result<RawFd> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id)
            .map(|s| s.master_fd)
            .ok_or_else(|| anyhow::anyhow!("Session {} not found", session_id))
    }

    /// 获取所有活动会话的 ID
    pub async fn list_sessions(&self) -> Vec<String> {
        let sessions = self.sessions.read().await;
        sessions.keys().cloned().collect()
    }

    /// 写入数据到 PTY
    ///
    /// # 参数
    /// - `session_id`: PTY 会话 ID
    /// - `data`: 要写入的数据
    ///
    /// # 返回
    /// 成功返回 Ok(())
    pub async fn write(&self, session_id: &str, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let sessions = self.sessions.read().await;
        let session = sessions.get(session_id)
            .ok_or_else(|| anyhow::anyhow!("PTY 会话不存在: {}", session_id))?;

        // 使用 libc::write 直接写入文件描述符
        let ret = unsafe { libc::write(session.master_fd, data.as_ptr() as *const libc::c_void, data.len()) };
        if ret < 0 {
            return Err(anyhow::anyhow!("PTY 写入失败: {}", nix::errno::Errno::last()));
        }

        debug!("PTY 写入成功: session_id={}, len={}", session_id, data.len());
        Ok(())
    }

    /// 从 PTY 读取数据（终端输出）
    ///
    /// # 参数
    /// - `session_id`: PTY 会话 ID
    ///
    /// # 返回
    /// 返回读取到的数据
    pub async fn read(&self, session_id: &str) -> Result<Vec<u8>> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(session_id)
            .ok_or_else(|| anyhow::anyhow!("PTY 会话不存在: {}", session_id))?;

        let mut buf = [0u8; 4096];
        // 使用 libc::read 直接读取文件描述符（非阻塞模式）
        let ret = unsafe { libc::read(session.master_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };

        match ret {
            n if n > 0 => {
                debug!("PTY 读取成功: session_id={}, len={}", session_id, n);
                Ok(buf[..n as usize].to_vec())
            },
            0 => Ok(Vec::new()),
            _ if nix::errno::Errno::last() == nix::errno::Errno::EAGAIN ||
                 nix::errno::Errno::last() == nix::errno::Errno::EWOULDBLOCK => {
                // 非阻塞模式下无数据可读，返回空数据
                Ok(Vec::new())
            }
            _ if nix::errno::Errno::last() == nix::errno::Errno::EIO => {
                // 子进程已退出
                Ok(Vec::new())
            }
            _ => Err(anyhow::anyhow!("PTY 读取失败: {}", nix::errno::Errno::last())),
        }
    }

    /// 调整终端大小
    ///
    /// # 参数
    /// - `session_id`: PTY 会话 ID
    /// - `cols`: 列数
    /// - `rows`: 行数
    pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<()> {
        use nix::pty::Winsize;

        let sessions = self.sessions.read().await;
        let session = sessions.get(session_id)
            .ok_or_else(|| anyhow::anyhow!("PTY 会话不存在: {}", session_id))?;

        let winsize = Winsize {
            ws_col: cols,
            ws_row: rows,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // 使用 libc::ioctl 直接调用
        let ret = unsafe {
            libc::ioctl(session.master_fd, libc::TIOCSWINSZ as libc::c_ulong, &winsize)
        };

        if ret < 0 {
            return Err(anyhow::anyhow!("PTY resize 失败: {}", nix::errno::Errno::last()));
        }

        info!("PTY resize: session_id={}, cols={}, rows={}", session_id, cols, rows);
        Ok(())
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
        let session = PtySession {
            session_id: "test-session".to_string(),
            master_fd: 10,
            user_info: UserInfo {
                username: "test".to_string(),
                uid: 1000,
                gid: 1000,
            },
            created_at: SystemTime::now(),
        };

        registry.register(session.clone()).await.unwrap();
        let retrieved = registry.get("test-session").await;
        assert!(retrieved.is_some());
    }
}