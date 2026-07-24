//! SSH公钥认证实现
//!
//! # 功能
//! - 加载用户的 `~/.ssh/authorized_keys` 文件
//! - 解析OpenSSH格式的公钥
//! - 公钥匹配验证
//!
//! # 限制
//! - **当前版本不包含签名验证**
//! - 仅支持公钥匹配，无法防止重放攻击
//! - 签名验证将在后续版本中添加
//!
//! # 后续改进
//! - 使用 `russh-keys` 进行完整签名验证
//! - 支持多种密钥算法（ssh-ed25519, rsa-sha2-*）
//!
//! ## 职责边界（高内聚）
//! - ✅ 处理SSH公钥认证
//! - ✅ 管理 authorized_keys 文件
//! - 🔄 验证公钥签名（部分实现）
//! - ❌ 不处理密码认证（由PAM模块负责）
//!
//! ## 设计说明
//! - 使用简化的base64解析，后续可用russh-keys完整实现
//! - 遵循OpenSSH authorized_keys格式规范

use super::{get_user_info, AuthResult, Authenticator};
use anyhow::Result;
use std::path::PathBuf;

/// SSH 公钥认证器
///
/// 基于 `~/.ssh/authorized_keys` 进行公钥认证，
/// 遵循SSH协议规范。
pub struct SshAuthenticator {
    // 预留字段位置，后续版本可添加配置项
}

impl SshAuthenticator {
    /// 创建新的SSH认证器
    pub fn new() -> Self {
        Self {}
    }

    /// 获取用户的 authorized_keys 文件路径
    ///
    /// # 参数
    /// - `username`: 用户名
    ///
    /// # 返回
    /// - 成功返回 Ok(PathBuf)
    /// - 用户不存在或路径构建失败返回 Err
    fn get_authorized_keys_path(&self, username: &str) -> Result<PathBuf> {
        // 获取用户信息
        let user_info = get_user_info(username)?;

        // 构建路径: ~/.ssh/authorized_keys
        let path = PathBuf::from(&user_info.home_dir)
            .join(".ssh")
            .join("authorized_keys");

        Ok(path)
    }

    /// 加载用户的 authorized_keys 文件
    ///
    /// # 参数
    /// - `username`: 用户名
    ///
    /// # 返回
    /// - 成功返回 Ok(String) 文件内容
    /// - 文件不存在返回 Ok(String::new())
    /// - 读取失败返回 Err
    fn load_authorized_keys(&self, username: &str) -> Result<String> {
        let path = self.get_authorized_keys_path(username)?;

        // 文件不存在返回空字符串（不视为错误）
        if !path.exists() {
            return Ok(String::new());
        }

        // 读取文件内容
        let content = std::fs::read_to_string(&path)?;
        Ok(content)
    }

    /// 解析 authorized_keys 文件内容（OpenSSH格式）
    ///
    /// # 格式说明
    /// 标准格式: key-type base64-key comment
    /// 例如: ssh-rsa AAAAB3NzaC1yc2E... user@host
    ///
    /// # 参数
    /// - `content`: 文件内容
    ///
    /// # 返回
    /// - 成功返回 Ok(Vec<Vec<u8>>) 公钥列表（base64解码后的原始数据）
    fn parse_authorized_keys(&self, content: &str) -> Result<Vec<Vec<u8>>> {
        let mut keys = Vec::new();

        for line in content.lines() {
            let line = line.trim();

            // 跳过空行和注释
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // 提取base64部分
            // 格式: key-type base64-key [comment]
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                // 尝试解码base64
                if let Ok(decoded) = base64_decode(parts[1]) {
                    keys.push(decoded);
                }
                // 解码失败跳过该行（不中断处理）
            }
        }

        Ok(keys)
    }

    /// 验证公钥签名
    ///
    /// # 参数
    /// - `pubkey`: 公钥数据
    /// - `signature`: 签名数据
    /// - `challenge`: 挑战数据（用于签名验证）
    ///
    /// # 返回
    /// - 签名有效返回 Ok(true)
    /// - 签名无效返回 Ok(false)
    ///
    /// # 说明
    /// 当前为简化实现，后续可用russh-keys完整实现
    #[allow(dead_code)]
    fn verify_signature(&self, _pubkey: &[u8], _signature: &[u8], _challenge: &[u8]) -> Result<bool> {
        // TODO: 实现签名验证
        // 当前返回false，表示签名验证未实现
        Ok(false)
    }
}

impl Default for SshAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

impl Authenticator for SshAuthenticator {
    /// SSH公钥认证
    ///
    /// # 实现流程
    /// 1. 加载用户的 authorized_keys
    /// 2. 解析并查找匹配的公钥
    /// 3. 如果提供签名，验证签名
    /// 4. 成功返回 UserIdentity
    ///
    /// # 限制
    /// - **当前版本不验证签名**
    /// - 仅支持公钥匹配，无法防止重放攻击
    /// - 签名验证将在后续版本中添加（使用russh-keys）
    ///
    /// # 参数
    /// - `username`: 用户名
    /// - `pubkey`: 公钥数据（base64解码后的字节）
    /// - `signature`: 签名数据（当前忽略，向后兼容）
    fn authenticate_pubkey(
        &self,
        username: &str,
        pubkey: &[u8],
        signature: Option<&[u8]>,
    ) -> Result<AuthResult> {
        // 第一步：加载用户的 authorized_keys
        let content = match self.load_authorized_keys(username) {
            Ok(content) => content,
            Err(e) => {
                // 错误信息不包含用户名，防止信息泄露
                tracing::warn!("Failed to load authorized_keys: {}", e);
                return Ok(AuthResult::Failure);
            }
        };

        // 第二步：解析 authorized_keys
        let authorized_keys = match self.parse_authorized_keys(&content) {
            Ok(keys) => keys,
            Err(e) => {
                tracing::warn!("Failed to parse authorized_keys: {}", e);
                return Ok(AuthResult::Failure);
            }
        };

        // 第三步：查找匹配的公钥
        // 使用简化的字节比较，后续可用russh-keys完整实现
        let key_found = authorized_keys.iter().any(|key| key == pubkey);

        if !key_found {
            // 公钥未匹配
            return Ok(AuthResult::Failure);
        }

        // 第四步：如果提供签名，验证签名
        if let Some(_sig) = signature {
            // 当前签名验证未实现，为了向后兼容，忽略签名参数
            tracing::debug!(
                "Signature verification not implemented yet - public key auth only"
            );
        }

        // 第五步：认证成功，返回用户身份
        let user_info = match get_user_info(username) {
            Ok(info) => info,
            Err(e) => {
                tracing::warn!("Failed to get user info: {}", e);
                return Ok(AuthResult::Failure);
            }
        };

        Ok(AuthResult::Success(user_info))
    }

    /// 密码认证（不支持）
    ///
    /// # 设计说明
    /// SSH模块遵循高内聚原则，只处理公钥认证。
    /// 密码认证由PAM模块负责，避免职责混淆。
    fn authenticate_password(&self, _username: &str, _password: &str) -> Result<AuthResult> {
        // SSH模块不支持密码认证
        Ok(AuthResult::Failure)
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

/// Base64解码辅助函数
///
/// # 参数
/// - `input`: base64编码的字符串
///
/// # 返回
/// - 成功返回 Ok(Vec<u8>)
/// - 失败返回 Err
fn base64_decode(input: &str) -> Result<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD
        .decode(input)
        .map_err(|e| anyhow::anyhow!("Base64 decode error: {}", e))
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_authenticator_new() {
        let auth = SshAuthenticator::new();
        // 空实现测试，确保能创建实例
        assert!(matches!(
            auth.authenticate_pubkey("test", &[], None).unwrap(),
            AuthResult::Failure
        ));
    }

    #[test]
    fn test_ssh_password_not_supported() {
        let auth = SshAuthenticator::new();
        // SSH不应该支持密码认证
        assert!(matches!(
            auth.authenticate_password("test", "password").unwrap(),
            AuthResult::Failure
        ));
    }

    #[test]
    fn test_load_authorized_keys() {
        let auth = SshAuthenticator::new();

        // 测试用户不存在情况
        // 注意：在Windows上，获取不存在用户的信息会失败
        let result = auth.load_authorized_keys("nonexistent_user_12345");
        // 应该返回错误或空内容
        // 当前实现：如果用户不存在，get_user_info 会返回错误
        assert!(result.is_err() || result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_authorized_keys() {
        let auth = SshAuthenticator::new();

        // 测试正常格式（使用有效的base64字符串）
        let content = "ssh-rsa SGVsbG8gV29ybGQ= user@host\nssh-ed25519 VGVzdEtleQ== user@host2";
        let keys = auth.parse_authorized_keys(content).unwrap();
        assert_eq!(keys.len(), 2);

        // 测试包含注释和空行
        let content_with_comments = "# Comment\nssh-rsa SGVsbG8gV29ybGQ= user@host\n\n# Another comment";
        let keys = auth.parse_authorized_keys(content_with_comments).unwrap();
        assert_eq!(keys.len(), 1);

        // 测试空文件
        let empty_content = "";
        let keys = auth.parse_authorized_keys(empty_content).unwrap();
        assert_eq!(keys.len(), 0);

        // 测试只有注释的文件
        let comments_only = "# Only comments\n# No keys";
        let keys = auth.parse_authorized_keys(comments_only).unwrap();
        assert_eq!(keys.len(), 0);

        // 测试无效的base64（应该跳过该行）
        let invalid_content = "ssh-rsa InvalidBase64!!! user@host\nssh-rsa SGVsbG8gV29ybGQ= user@host2";
        let keys = auth.parse_authorized_keys(invalid_content).unwrap();
        assert_eq!(keys.len(), 1); // 只解析成功一行
    }

    #[test]
    fn test_base64_decode() {
        // 测试正常的base64解码
        let valid_b64 = "SGVsbG8gV29ybGQ=";
        let decoded = base64_decode(valid_b64).unwrap();
        assert_eq!(decoded, b"Hello World");

        // 测试无效的base64
        let invalid_b64 = "Invalid!Base64@";
        assert!(base64_decode(invalid_b64).is_err());
    }
}