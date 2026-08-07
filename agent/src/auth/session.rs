//! 用户会话管理
//!
//! 提供用户会话的创建和管理功能

use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

use super::UserIdentity;

/// 用户会话
///
/// 表示一个已认证用户的会话信息，包含会话ID、用户身份和时间戳。
#[derive(Debug, Clone)]
pub struct UserSession {
    /// 会话唯一标识符
    pub session_id: String,
    /// 用户名
    pub username: String,
    /// 用户ID (UID)
    pub uid: u32,
    /// 组ID (GID)
    pub gid: u32,
    /// 用户家目录
    pub home_dir: PathBuf,
    /// 用户Shell
    pub shell: PathBuf,
    /// 会话创建时间
    #[allow(dead_code)]
    pub created_at: SystemTime,
}

impl UserSession {
    /// 从用户身份创建新会话
    ///
    /// # 参数
    /// - `identity`: 用户身份信息
    ///
    /// # 返回
    /// 返回带有唯一会话ID的 `UserSession`
    ///
    /// # 示例
    /// ```rust,ignore
    /// let identity = UserIdentity::new(
    ///     "testuser".to_string(),
    ///     1000,
    ///     1000,
    ///     "/home/testuser".to_string(),
    ///     "/bin/bash".to_string(),
    /// );
    /// let session = UserSession::new(identity);
    /// ```
    pub fn new(identity: UserIdentity) -> Self {
        Self {
            session_id: format!("sess-{}", Uuid::new_v4()),
            username: identity.username,
            uid: identity.uid,
            gid: identity.gid,
            home_dir: PathBuf::from(identity.home_dir),
            shell: PathBuf::from(identity.shell),
            created_at: SystemTime::now(),
        }
    }

    /// 获取会话持续时间
    #[allow(dead_code)]
    pub fn duration(&self) -> Duration {
        self.created_at
            .elapsed()
            .unwrap_or(Duration::from_secs(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let identity = UserIdentity::new(
            "testuser".to_string(),
            1000,
            1000,
            "/home/testuser".to_string(),
            "/bin/bash".to_string(),
        );

        let session = UserSession::new(identity);

        // 验证会话ID格式
        assert!(session.session_id.starts_with("sess-"));

        // 验证用户信息
        assert_eq!(session.username, "testuser");
        assert_eq!(session.uid, 1000);
        assert_eq!(session.gid, 1000);

        // 验证路径转换
        assert_eq!(session.home_dir, PathBuf::from("/home/testuser"));
        assert_eq!(session.shell, PathBuf::from("/bin/bash"));
    }
}