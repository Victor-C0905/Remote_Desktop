//! 用户会话管理
//!
//! 提供用户会话的创建和管理功能，包括会话超时机制

use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

use super::UserIdentity;

/// 默认会话最大空闲时间（24小时）
const DEFAULT_MAX_IDLE_TIME: Duration = Duration::from_secs(24 * 60 * 60);

/// 用户会话
///
/// 表示一个已认证用户的会话信息，包含会话ID、用户身份和时间戳。
///
/// # 会话超时机制
/// - 每个会话有最大空闲时间（默认24小时）
/// - 每次活动会更新 `last_activity` 时间戳
/// - 超过最大空闲时间未活动，会话将过期
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
    #[allow(dead_code)]
    pub shell: PathBuf,
    /// 会话创建时间
    #[allow(dead_code)]
    pub created_at: SystemTime,
    /// 最后活动时间（用于超时检查）
    pub last_activity: SystemTime,
    /// 最大空闲时间（超时阈值）
    pub max_idle_time: Duration,
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
        let now = SystemTime::now();
        Self {
            session_id: format!("sess-{}", Uuid::new_v4()),
            username: identity.username,
            uid: identity.uid,
            gid: identity.gid,
            home_dir: PathBuf::from(identity.home_dir),
            shell: PathBuf::from(identity.shell),
            created_at: now,
            last_activity: now,
            max_idle_time: DEFAULT_MAX_IDLE_TIME,
        }
    }

    /// 创建带有自定义超时时间的会话
    ///
    /// # 参数
    /// - `identity`: 用户身份信息
    /// - `max_idle_time_secs`: 最大空闲时间（秒）
    pub fn with_timeout(identity: UserIdentity, max_idle_time_secs: u64) -> Self {
        let mut session = Self::new(identity);
        session.max_idle_time = Duration::from_secs(max_idle_time_secs);
        session
    }

    /// 检查会话是否已过期
    ///
    /// # 返回
    /// - `true`: 会话已过期
    /// - `false`: 会话仍然有效
    pub fn is_expired(&self) -> bool {
        match self.last_activity.elapsed() {
            Ok(elapsed) => elapsed > self.max_idle_time,
            Err(_) => true, // 时间错误，视为过期
        }
    }

    /// 获取剩余有效时间
    ///
    /// # 返回
    /// - `Some(duration)`: 剩余有效时间
    /// - `None`: 会话已过期
    pub fn remaining_time(&self) -> Option<Duration> {
        match self.last_activity.elapsed() {
            Ok(elapsed) => {
                if elapsed > self.max_idle_time {
                    None
                } else {
                    Some(self.max_idle_time - elapsed)
                }
            }
            Err(_) => None,
        }
    }

    /// 更新最后活动时间（心跳）
    ///
    /// 每次用户操作时调用，防止会话超时
    pub fn touch(&mut self) {
        self.last_activity = SystemTime::now();
    }

    /// 获取会话持续时间
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