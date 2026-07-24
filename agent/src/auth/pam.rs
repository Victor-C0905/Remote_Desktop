//! PAM (Pluggable Authentication Modules) 密码认证模块
//!
//! ## 职责边界（高内聚）
//! - ✅ 处理PAM密码认证
//! - ✅ 集成系统用户管理
//! - ✅ 支持PAM会话管理
//! - ❌ 不处理公钥认证（由SSH模块负责）
//!
//! ## 平台支持
//! - Unix: 使用pam库进行系统认证
//! - Windows: 占位实现，不支持

use super::{AuthResult, Authenticator};
use anyhow::Result;

#[cfg(unix)]
use super::{get_user_info, UserIdentity};
#[cfg(unix)]
use tracing::{debug, warn};

#[cfg(not(unix))]
use tracing::warn;

// ============================================================================
// Unix 平台实现
// ============================================================================

#[cfg(unix)]
use pam::{Authenticator as PamAuthLib, PasswordConv};

/// PAM 认证器
///
/// 集成Linux系统的PAM框架进行密码认证，
/// 支持各种PAM模块（如pam_unix, pam_ldap等）。
#[cfg(unix)]
pub struct PamAuthenticator {
    /// PAM服务名（如 "sshd", "login"）
    pam_service: String,
}

#[cfg(unix)]
impl PamAuthenticator {
    /// 创建新的PAM认证器
    ///
    /// # 参数
    /// - `pam_service`: PAM服务名，通常使用 "sshd" 或 "login"
    ///
    /// # 示例
    /// ```rust,ignore
    /// let auth = PamAuthenticator::new("sshd".to_string());
    /// ```
    pub fn new(pam_service: String) -> Self {
        Self { pam_service }
    }

    /// 执行PAM密码认证
    ///
    /// # 实现细节
    /// 1. 创建PAM认证会话
    /// 2. 设置用户名和密码
    /// 3. 调用PAM认证
    /// 4. 获取用户信息
    ///
    /// # 错误处理
    /// - 不记录密码等敏感信息
    /// - 错误信息不包含用户名（防止信息泄露）
    fn do_pam_authenticate(&self, username: &str, password: &str) -> Result<AuthResult> {
        debug!("Attempting PAM authentication for service: {}", self.pam_service);

        // 创建PAM认证会话
        let mut auth = match PamAuthLib::with_password(&self.pam_service) {
            Ok(a) => a,
            Err(e) => {
                warn!("Failed to create PAM authenticator: {}", e);
                return Ok(AuthResult::Failure);
            }
        };

        // 设置用户名
        if let Err(e) = auth.get_username().map(|u| u.set_username(username)) {
            warn!("Failed to set username in PAM: {}", e);
            return Ok(AuthResult::Failure);
        };

        // 设置密码
        if let Err(e) = auth.get_password().map(|p| p.set_password(password)) {
            warn!("Failed to set password in PAM: {}", e);
            return Ok(AuthResult::Failure);
        };

        // 执行认证
        match auth.authenticate() {
            Ok(_) => {
                debug!("PAM authentication successful");
                
                // 获取用户信息
                match get_user_info(username) {
                    Ok(identity) => Ok(AuthResult::Success(identity)),
                    Err(e) => {
                        warn!("Failed to get user info after successful auth: {}", e);
                        Ok(AuthResult::Failure)
                    }
                }
            }
            Err(e) => {
                // 认证失败（可能是密码错误、用户不存在等）
                // 不记录具体错误信息，防止信息泄露
                debug!("PAM authentication failed");
                Ok(AuthResult::Failure)
            }
        }
    }
}

#[cfg(unix)]
impl Default for PamAuthenticator {
    fn default() -> Self {
        Self::new("sshd".to_string())
    }
}

#[cfg(unix)]
impl Authenticator for PamAuthenticator {
    /// 公钥认证（不支持）
    ///
    /// # 设计说明
    /// PAM模块遵循高内聚原则，只处理密码认证。
    /// 公钥认证由SSH模块负责，避免职责混淆。
    fn authenticate_pubkey(
        &self,
        _username: &str,
        _pubkey: &[u8],
        _signature: Option<&[u8]>,
    ) -> Result<AuthResult> {
        // PAM模块不支持公钥认证
        Ok(AuthResult::Failure)
    }

    /// 密码认证（PAM风格）
    ///
    /// # 实现说明
    /// 使用Linux PAM框架进行系统级密码认证
    fn authenticate_password(&self, username: &str, password: &str) -> Result<AuthResult> {
        self.do_pam_authenticate(username, password)
    }
}

// ============================================================================
// Windows 平台占位实现
// ============================================================================

#[cfg(not(unix))]
pub struct PamAuthenticator {
    // Windows平台不支持PAM，使用空结构体
}

#[cfg(not(unix))]
impl PamAuthenticator {
    /// 创建新的PAM认证器（Windows不支持）
    pub fn new(_pam_service: String) -> Self {
        Self {}
    }
}

#[cfg(not(unix))]
impl Default for PamAuthenticator {
    fn default() -> Self {
        Self::new(String::new())
    }
}

#[cfg(not(unix))]
impl Authenticator for PamAuthenticator {
    /// 公钥认证（Windows不支持）
    fn authenticate_pubkey(
        &self,
        _username: &str,
        _pubkey: &[u8],
        _signature: Option<&[u8]>,
    ) -> Result<AuthResult> {
        Ok(AuthResult::Failure)
    }

    /// 密码认证（Windows不支持）
    fn authenticate_password(&self, _username: &str, _password: &str) -> Result<AuthResult> {
        warn!("PAM authentication not supported on Windows platform");
        Ok(AuthResult::Failure)
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pam_authenticator_new() {
        let auth = PamAuthenticator::new("sshd".to_string());
        // 确保能创建实例
        assert!(matches!(
            auth.authenticate_password("test", "password").unwrap(),
            AuthResult::Failure
        ));
    }

    #[test]
    fn test_pam_pubkey_not_supported() {
        let auth = PamAuthenticator::new("sshd".to_string());
        // PAM不应该支持公钥认证
        assert!(matches!(
            auth.authenticate_pubkey("test", &[], None).unwrap(),
            AuthResult::Failure
        ));
    }

    #[test]
    fn test_pam_default_service() {
        let auth = PamAuthenticator::default();
        // 默认使用sshd服务
        assert!(matches!(
            auth.authenticate_password("test", "password").unwrap(),
            AuthResult::Failure
        ));
    }
}