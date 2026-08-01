//! PTY 注册表
//!
//! 管理所有活动的 PTY master_fd。

use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use anyhow::Result;

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

    /// 获取所有活动会话的 ID
    pub async fn list_sessions(&self) -> Vec<String> {
        let sessions = self.sessions.read().await;
        sessions.keys().cloned().collect()
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