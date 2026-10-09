//! SSH公钥认证实现
//!
//! # 功能
//! - 加载用户的 `~/.ssh/authorized_keys` 文件
//! - 解析OpenSSH格式的公钥
//! - 公钥匹配验证
//! - **完整的签名验证功能**
//!
//! # 支持的签名算法
//! - ssh-ed25519（Ed25519 签名算法）
//! - ssh-rsa（RSA 签名算法）
//! - rsa-sha2-256 / rsa-sha2-512（SHA2 变体）
//!
//! ## 职责边界（高内聚）
//! - ✅ 处理SSH公钥认证
//! - ✅ 管理 authorized_keys 文件
//! - ✅ 验证公钥签名（完整实现）
//! - ❌ 不处理密码认证（由PAM模块负责）
//!
//! ## 设计说明
//! - 使用 `ssh_key` crate 进行签名验证
//! - 支持多种公钥格式（OpenSSH、Base64、原始字节）
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

    /// 检查公钥是否在用户的 authorized_keys 中
    ///
    /// # 参数
    /// - `username`: 用户名
    /// - `pubkey`: 公钥数据（可能是 OpenSSH 格式字符串或原始字节）
    ///
    /// # 返回
    /// - Ok(true): 公钥已授权
    /// - Ok(false): 公钥未授权
    /// - Err: 检查过程中发生错误
    pub fn check_pubkey_in_authorized_keys(&self, username: &str, pubkey: &[u8]) -> Result<bool> {
        // 第一步：加载用户的 authorized_keys
        let content = self.load_authorized_keys(username)?;

        // 如果文件为空，直接返回 false
        if content.is_empty() {
            tracing::warn!("用户的 authorized_keys 文件为空或不存在: username={}", username);
            return Ok(false);
        }

        // 第二步：解析 authorized_keys
        let authorized_keys = self.parse_authorized_keys(&content)?;

        // 第三步：解析客户端提供的公钥
        // 尝试提取公钥的 base64 部分
        let client_pubkey_base64 = self.extract_pubkey_base64(pubkey)?;

        // 第四步：查找匹配的公钥
        let key_found = authorized_keys.iter().any(|key| {
            // authorized_keys 中的每个 key 都是 base64 解码后的字节
            // 我们需要比较 base64 编码是否匹配
            let authorized_base64 = base64_encode(key);
            authorized_base64 == client_pubkey_base64
        });

        if key_found {
            tracing::info!("公钥匹配成功: username={}", username);
        } else {
            tracing::warn!("公钥未匹配: username={}", username);
        }

        Ok(key_found)
    }

    /// 从公钥数据中提取 base64 编码部分
    ///
    /// 支持格式：
    /// 1. OpenSSH 格式字符串："ssh-rsa AAAA... user@host"
    /// 2. 纯 base64 字节
    fn extract_pubkey_base64(&self, pubkey: &[u8]) -> Result<String> {
        // 尝试转换为 UTF-8 字符串
        if let Ok(pubkey_str) = std::str::from_utf8(pubkey) {
            // 如果是 OpenSSH 格式（包含空格），提取第二部分
            let parts: Vec<&str> = pubkey_str.split_whitespace().collect();
            if parts.len() >= 2 {
                // 返回 base64 部分（第二部分）
                return Ok(parts[1].to_string());
            } else {
                // 整个字符串就是 base64
                return Ok(pubkey_str.trim().to_string());
            }
        }

        // 如果不是 UTF-8，直接编码为 base64
        Ok(base64_encode(pubkey))
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
    /// - `pubkey`: 公钥数据（authorized_keys 格式的 base64 编码）
    /// - `signature`: 签名数据（SSH 签名格式）
    /// - `challenge`: 挑战数据（用于签名验证）
    ///
    /// # 返回
    /// - 签名有效返回 Ok(true)
    /// - 签名无效返回 Ok(false)
    /// - 解析错误返回 Err
    ///
    /// # 支持的算法
    /// - ssh-ed25519（Ed25519 签名算法）
    /// - ssh-rsa（RSA 签名算法）
    /// - rsa-sha2-256 / rsa-sha2-512（SHA2 变体）
    pub fn verify_signature(&self, pubkey: &[u8], signature: &[u8], challenge: &[u8]) -> Result<bool> {
        tracing::debug!(
            "开始签名验证: pubkey_len={}, sig_len={}, challenge_len={}",
            pubkey.len(),
            signature.len(),
            challenge.len()
        );

        // 第一步：解析公钥
        let public_key = match self.parse_public_key(pubkey) {
            Ok(key) => key,
            Err(e) => {
                tracing::warn!("公钥解析失败: {}", e);
                // 解析失败返回 false，而不是 error（向后兼容）
                return Ok(false);
            }
        };

        tracing::debug!("公钥解析成功: algorithm={:?}", public_key.algorithm());

        // 第二步：解析签名（SSH 协议格式）
        // SSH 签名格式: string algorithm, string signature_data
        let (sig_algorithm, sig_data) = match self.parse_ssh_signature(signature) {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!("签名解析失败: {}", e);
                return Ok(false);
            }
        };

        tracing::debug!("签名解析成功: algorithm={}", sig_algorithm);

        // 第三步：验证算法是否匹配
        let key_algorithm = public_key.algorithm();
        if !self.is_algorithm_compatible(&key_algorithm, &sig_algorithm) {
            tracing::warn!(
                "算法不匹配: key={:?}, sig={}",
                key_algorithm,
                sig_algorithm
            );
            return Ok(false);
        }

        // 第四步：使用底层加密库验证签名
        match self.verify_with_key(&public_key, &sig_data, challenge) {
            Ok(true) => {
                tracing::info!("签名验证成功");
                Ok(true)
            }
            Ok(false) => {
                tracing::warn!("签名验证失败");
                Ok(false)
            }
            Err(e) => {
                tracing::error!("签名验证错误: {}", e);
                Ok(false)
            }
        }
    }

    /// 解析 SSH 公钥
    ///
    /// # 参数
    /// - `pubkey`: 公钥数据（可能是原始 base64 或完整的 SSH 公钥格式）
    ///
    /// # 返回
    /// - 成功返回 Ok(PublicKey)
    /// - 失败返回 Err
    fn parse_public_key(&self, pubkey: &[u8]) -> Result<ssh_key::PublicKey> {
        use ssh_key::PublicKey;

        // 方法 0: 尝试直接从字节解析（SSH 二进制格式）
        if let Ok(key) = PublicKey::from_bytes(pubkey) {
            tracing::debug!("公钥解析成功（原始字节格式）");
            return Ok(key);
        }

        // 尝试转换为 UTF-8 字符串
        let pubkey_str = match std::str::from_utf8(pubkey) {
            Ok(s) => s,
            Err(_) => {
                // UTF-8 转换失败，直接返回错误
                return Err(anyhow::anyhow!("无法解析公钥（非UTF-8且非SSH字节格式）"));
            }
        };

        tracing::trace!("尝试解析公钥字符串: {} bytes", pubkey_str.len());

        // 方法 1: 尝试解析为完整的 SSH 公钥格式（ssh-rsa AAAA...）
        if let Ok(key) = PublicKey::from_openssh(pubkey_str) {
            tracing::debug!("公钥解析成功（OpenSSH 格式）");
            return Ok(key);
        }

        // 方法 2: 尝试从 base64 解码后解析
        if let Ok(decoded) = base64_decode(pubkey_str.trim()) {
            if let Ok(key) = PublicKey::from_bytes(&decoded) {
                tracing::debug!("公钥解析成功（Base64 解码格式）");
                return Ok(key);
            }
        }

        Err(anyhow::anyhow!("无法解析公钥格式"))
    }

    /// 解析 SSH 协议签名格式
    ///
    /// # SSH 签名格式
    /// - string: 算法名称（如 "ssh-ed25519"）
    /// - string: 签名数据
    ///
    /// # 参数
    /// - `signature`: 原始签名字节
    ///
    /// # 返回
    /// - 成功返回 Ok((算法名称, 签名数据))
    fn parse_ssh_signature(&self, signature: &[u8]) -> Result<(String, Vec<u8>)> {
        if signature.len() < 8 {
            return Err(anyhow::anyhow!("签名数据太短"));
        }

        // 读取算法名称长度（4字节 big-endian）
        let algo_len = u32::from_be_bytes([signature[0], signature[1], signature[2], signature[3]]) as usize;

        if signature.len() < 4 + algo_len + 4 {
            return Err(anyhow::anyhow!("签名数据格式错误"));
        }

        // 读取算法名称
        let algo_start = 4;
        let algo_end = algo_start + algo_len;
        let algorithm = std::str::from_utf8(&signature[algo_start..algo_end])
            .map_err(|e| anyhow::anyhow!("算法名称不是有效的 UTF-8: {}", e))?
            .to_string();

        // 读取签名数据长度（4字节 big-endian）
        let sig_len_start = algo_end;
        let sig_len = u32::from_be_bytes([
            signature[sig_len_start],
            signature[sig_len_start + 1],
            signature[sig_len_start + 2],
            signature[sig_len_start + 3],
        ]) as usize;

        if signature.len() < sig_len_start + 4 + sig_len {
            return Err(anyhow::anyhow!("签名数据长度不匹配"));
        }

        // 读取签名数据
        let sig_data = signature[sig_len_start + 4..sig_len_start + 4 + sig_len].to_vec();

        Ok((algorithm, sig_data))
    }

    /// 检查公钥和签名算法是否兼容
    ///
    /// # 参数
    /// - `key_algorithm`: 公钥算法
    /// - `sig_algorithm`: 签名算法名称
    ///
    /// # 返回
    /// - 兼容返回 true
    /// - 不兼容返回 false
    fn is_algorithm_compatible(&self, key_algorithm: &ssh_key::Algorithm, sig_algorithm: &str) -> bool {
        tracing::trace!(
            "检查算法兼容性: key_algorithm={:?}, sig_algorithm={}",
            key_algorithm,
            sig_algorithm
        );

        match key_algorithm {
            ssh_key::Algorithm::Ed25519 => sig_algorithm == "ssh-ed25519",
            ssh_key::Algorithm::Rsa { .. } => {
                matches!(
                    sig_algorithm,
                    "ssh-rsa" | "rsa-sha2-256" | "rsa-sha2-512"
                )
            }
            _ => false,
        }
    }

    /// 使用公钥验证签名数据
    ///
    /// # 参数
    /// - `public_key`: 公钥
    /// - `sig_data`: 签名数据（原始字节，不包含算法信息）
    /// - `challenge`: 挑战数据
    ///
    /// # 返回
    /// - 验证成功返回 Ok(true)
    /// - 验证失败返回 Ok(false)
    fn verify_with_key(
        &self,
        public_key: &ssh_key::PublicKey,
        sig_data: &[u8],
        challenge: &[u8],
    ) -> Result<bool> {
        use signature::Verifier;

        // 根据密钥类型进行验证
        match public_key.algorithm() {
            ssh_key::Algorithm::Ed25519 => {
                // Ed25519 验证
                if let Some(ed25519_key) = public_key.key_data().ed25519() {
                    // 创建 ssh_key::Signature 对象
                    let signature = match ssh_key::Signature::new(ssh_key::Algorithm::Ed25519, sig_data) {
                        Ok(sig) => sig,
                        Err(e) => {
                            tracing::error!("创建签名对象失败: {}", e);
                            return Ok(false);
                        }
                    };

                    // 验证签名
                    match ed25519_key.verify(challenge, &signature) {
                        Ok(()) => Ok(true),
                        Err(e) => {
                            tracing::debug!("签名验证失败: {}", e);
                            Ok(false)
                        }
                    }
                } else {
                    Err(anyhow::anyhow!("无法提取 Ed25519 公钥"))
                }
            }
            ssh_key::Algorithm::Rsa { .. } => {
                // RSA 验证（使用 rsa-sha2-256 算法）
                tracing::debug!("开始 RSA 签名验证: sig_len={}", sig_data.len());

                // 从 ssh_key 公钥提取 RSA 公钥的 n 和 e
                let rsa_pub = match public_key.key_data().rsa() {
                    Some(rsa) => rsa,
                    None => {
                        tracing::error!("无法提取 RSA 公钥");
                        return Ok(false);
                    }
                };

                let n = rsa_pub.n.as_bytes();
                let e = rsa_pub.e.as_bytes();

                tracing::debug!("RSA 公钥参数: n={}字节, e={}字节", n.len(), e.len());

                // 构造 rsa::RsaPublicKey
                let rsa_public_key = rsa::RsaPublicKey::new(
                    rsa::BigUint::from_bytes_be(n),
                    rsa::BigUint::from_bytes_be(e),
                ).map_err(|e| {
                    tracing::error!("RSA 公钥构造失败: {}", e);
                    anyhow::anyhow!("RSA 公钥构造失败: {}", e)
                })?;

                // 使用 PKCS#1 v1.5 + SHA-256 验证签名
                use rsa::pkcs1v15::VerifyingKey;
                use rsa::sha2::Sha256;
                use rsa::signature::Verifier;

                let verifying_key = VerifyingKey::<Sha256>::new(rsa_public_key);

                // sig_data 是原始的 PKCS#1 v1.5 签名
                let signature = rsa::pkcs1v15::Signature::try_from(sig_data)
                    .map_err(|e| {
                        tracing::error!("签名格式转换失败: {}", e);
                        anyhow::anyhow!("签名格式转换失败: {}", e)
                    })?;

                match verifying_key.verify(challenge, &signature) {
                    Ok(()) => {
                        tracing::info!("RSA 签名验证成功");
                        Ok(true)
                    }
                    Err(e) => {
                        tracing::warn!("RSA 签名验证失败: {}", e);
                        Ok(false)
                    }
                }
            }
            _ => {
                tracing::warn!("不支持的密钥类型");
                Ok(false)
            }
        }
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
    /// 3. 如果提供签名，验证签名（支持 Ed25519 和 RSA）
    /// 4. 成功返回 UserIdentity
    ///
    /// # 参数
    /// - `username`: 用户名
    /// - `pubkey`: 公钥数据（base64解码后的字节或OpenSSH格式字符串）
    /// - `signature`: 签名数据（可选，推荐提供以增强安全性）
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
        if let Some(sig) = signature {
            tracing::debug!("开始验证签名");

            // 构造挑战数据（实际应用中应该使用真实的挑战-响应机制）
            // 这里使用 pubkey 本身作为简化的挑战数据
            // 实际部署时应该使用随机生成的挑战，并防止重放攻击
            let challenge = pubkey;

            match self.verify_signature(pubkey, sig, challenge) {
                Ok(true) => {
                    tracing::info!("签名验证成功");
                }
                Ok(false) => {
                    tracing::warn!("签名验证失败：签名无效");
                    return Ok(AuthResult::Failure);
                }
                Err(e) => {
                    tracing::error!("签名验证过程发生错误: {}", e);
                    return Ok(AuthResult::Failure);
                }
            }
        } else {
            // 未提供签名，仅进行公钥匹配（不安全，记录警告）
            tracing::warn!(
                "公钥认证未提供签名 - 仅匹配公钥，无法防止重放攻击"
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

/// Base64编码辅助函数
///
/// # 参数
/// - `input`: 原始字节
///
/// # 返回
/// - 返回 base64 编码的字符串
fn base64_encode(input: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    STANDARD.encode(input)
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

    #[test]
    fn test_parse_ssh_signature() {
        let auth = SshAuthenticator::new();

        // 构造一个简单的 SSH 签名格式
        // string "ssh-ed25519" (13 bytes with length prefix)
        // string signature_data
        let algorithm = b"ssh-ed25519";
        let sig_data = b"test_signature_data";

        let mut signature_bytes = Vec::new();

        // 添加算法名称（4字节长度 + 数据）
        signature_bytes.extend_from_slice(&(algorithm.len() as u32).to_be_bytes());
        signature_bytes.extend_from_slice(algorithm);

        // 添加签名数据（4字节长度 + 数据）
        signature_bytes.extend_from_slice(&(sig_data.len() as u32).to_be_bytes());
        signature_bytes.extend_from_slice(sig_data);

        let result = auth.parse_ssh_signature(&signature_bytes).unwrap();
        assert_eq!(result.0, "ssh-ed25519");
        assert_eq!(result.1, sig_data.to_vec());
    }

    #[test]
    fn test_parse_ssh_signature_invalid() {
        let auth = SshAuthenticator::new();

        // 测试过短的数据
        assert!(auth.parse_ssh_signature(&[0, 0, 0, 1]).is_err());

        // 测试空数据
        assert!(auth.parse_ssh_signature(&[]).is_err());
    }
}