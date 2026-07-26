//! 认证模块 - SSH兼容的多用户认证系统
//!
//! ## 设计原则
//! - **高内聚**: 每个模块只负责一件事（SSH只处理公钥认证，PAM只处理密码认证）
//! - **低耦合**: 通过 Authenticator trait 定义统一接口，实现可替换
//!
//! ## 模块结构
//! - `mod.rs`: 定义 Authenticator trait 和公共类型
//! - `ssh.rs`: SSH公钥认证实现
//! - `pam.rs`: PAM密码认证实现

use anyhow::Result;
use std::fmt;
use std::sync::Arc;

pub mod executor;
pub mod namespace;
pub mod pam;
pub mod session;
pub mod ssh;
pub mod challenge;

pub use executor::UserExecutor;
#[cfg(target_os = "linux")]
pub use namespace::UserNamespace;
pub use session::UserSession;
pub use challenge::ChallengeManager;

// ============================================================================
// 公共类型定义
// ============================================================================

/// 认证结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthResult {
    /// 认证成功
    Success(UserIdentity),
    /// 认证失败
    Failure,
    /// 部分成功（需要更多认证）
    #[allow(dead_code)]
    Partial,
}

/// 用户身份信息
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserIdentity {
    /// 用户名
    pub username: String,
    /// 用户ID (UID)
    pub uid: u32,
    /// 组ID (GID)
    pub gid: u32,
    /// 用户家目录
    pub home_dir: String,
    /// 用户Shell
    pub shell: String,
}

impl UserIdentity {
    /// 创建新的用户身份
    #[allow(dead_code)]
    pub fn new(username: String, uid: u32, gid: u32, home_dir: String, shell: String) -> Self {
        Self {
            username,
            uid,
            gid,
            home_dir,
            shell,
        }
    }
}

impl fmt::Display for UserIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}(uid={}, gid={}, home={})",
            self.username, self.uid, self.gid, self.home_dir
        )
    }
}

// ============================================================================
// Authenticator Trait - 统一认证接口
// ============================================================================

/// 认证器 Trait - 统一接口，降低耦合
///
/// 实现此 trait 的类型必须支持：
/// - Send + Sync：支持跨线程共享
/// - 公钥认证：SSH风格
/// - 密码认证：PAM风格
pub trait Authenticator: Send + Sync {
    /// 公钥认证（SSH风格）
    ///
    /// # 参数
    /// - `username`: 用户名
    /// - `pubkey`: 公钥数据（SSH公钥格式）
    /// - `signature`: 可选的签名数据（用于验证）
    ///
    /// # 返回
    /// - `Ok(AuthResult::Success(identity))`: 认证成功
    /// - `Ok(AuthResult::Failure)`: 认证失败
    /// - `Err(...)`: 系统错误（如IO错误）
    fn authenticate_pubkey(
        &self,
        username: &str,
        pubkey: &[u8],
        signature: Option<&[u8]>,
    ) -> Result<AuthResult>;

    /// 密码认证（PAM风格）
    ///
    /// # 参数
    /// - `username`: 用户名
    /// - `password`: 密码
    ///
    /// # 返回
    /// - `Ok(AuthResult::Success(identity))`: 认证成功
    /// - `Ok(AuthResult::Failure)`: 认证失败
    /// - `Err(...)`: 系统错误（如IO错误）
    fn authenticate_password(&self, username: &str, password: &str) -> Result<AuthResult>;
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 从 /etc/passwd 读取用户信息
///
/// # 参数
/// - `username`: 要查询的用户名
///
/// # 返回
/// - `Ok(UserIdentity)`: 用户信息
/// - `Err`: 用户不存在或读取失败
///
/// # 安全性
/// - 使用symlink_metadata防止符号链接攻击（检查链接本身，不跟随）
/// - 通过文件描述符检查，防止TOCTOU竞态条件
/// - 验证文件类型、所有者、权限
/// - 错误信息不包含用户名，防止信息泄露
///
/// # 示例
/// ```rust,ignore
/// let identity = get_user_info("root")?;
/// println!("UID: {}", identity.uid);
/// ```
#[cfg(unix)]
pub fn get_user_info(username: &str) -> Result<UserIdentity> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    let passwd_path = Path::new("/etc/passwd");

    // 第一步：检查路径本身是否为符号链接（防止符号链接攻击）
    // 使用symlink_metadata而非metadata，确保检查的是链接本身而非目标文件
    let link_metadata = std::fs::symlink_metadata(passwd_path)?;

    // 显式检查是否为符号链接
    if link_metadata.file_type().is_symlink() {
        return Err(anyhow::anyhow!("Passwd file is a symlink"));
    }

    // 第二步：打开文件（原子操作的起点）
    // 注意：在检查和打开之间存在TOCTOU窗口，但已通过第一步的符号链接检查降低风险
    let file = File::open(passwd_path)?;

    // 第三步：通过文件描述符获取元数据（防止TOCTOU）
    // 使用file.metadata()确保检查的是已打开的文件，而非可能被替换的路径
    let metadata = file.metadata()?;

    // 检查是否为常规文件（通过文件描述符）
    if !metadata.file_type().is_file() {
        return Err(anyhow::anyhow!("Invalid passwd file type"));
    }

    // 检查文件所有者（必须为root，uid=0）
    let uid = metadata.uid();
    if uid != 0 {
        return Err(anyhow::anyhow!("Passwd file has invalid owner"));
    }

    // 检查文件权限（不超过644）
    let mode = metadata.mode();
    if (mode & 0o777) > 0o644 {
        return Err(anyhow::anyhow!("Passwd file has insecure permissions"));
    }

    // 第四步：读取文件内容
    let reader = BufReader::new(file);

    // 解析 passwd 格式: username:x:uid:gid:comment:home:shell
    for line in reader.lines() {
        let line = line?;
        let parts: Vec<&str> = line.split(':').collect();

        if parts.len() >= 7 && parts[0] == username {
            return Ok(UserIdentity {
                username: parts[0].to_string(),
                uid: parts[2].parse()?,
                gid: parts[3].parse()?,
                home_dir: parts[5].to_string(),
                shell: parts[6].to_string(),
            });
        }
    }

    anyhow::bail!("User not found")
}

/// Windows 平台的占位实现（暂不支持）
#[cfg(windows)]
pub fn get_user_info(_username: &str) -> Result<UserIdentity> {
    anyhow::bail!("Windows平台暂不支持 get_user_info")
}

// ============================================================================
// 组合认证器 - 支持多种认证方式
// ============================================================================

/// 组合认证器（支持多种认证方式）
///
/// 通过委托模式组合 SSH 和 PAM 认证器，实现高内聚低耦合。
/// - 公钥认证委托给 SshAuthenticator
/// - 密码认证委托给 PamAuthenticator
pub struct CompositeAuthenticator {
    ssh_auth: Arc<ssh::SshAuthenticator>,
    pam_auth: Arc<pam::PamAuthenticator>,
    enable_pubkey: bool,
    enable_password: bool,
}

impl CompositeAuthenticator {
    /// 创建新的组合认证器
    ///
    /// # 参数
    /// - `pam_service`: PAM 服务名称
    /// - `enable_pubkey`: 是否启用公钥认证
    /// - `enable_password`: 是否启用密码认证
    ///
    /// # 示例
    /// ```rust,ignore
    /// let auth = CompositeAuthenticator::new(
    ///     "gnome-remote".to_string(),
    ///     true,
    ///     true,
    /// );
    /// ```
    pub fn new(
        pam_service: String,
        enable_pubkey: bool,
        enable_password: bool,
    ) -> Self {
        Self {
            ssh_auth: Arc::new(ssh::SshAuthenticator::new()),
            pam_auth: Arc::new(pam::PamAuthenticator::new(pam_service)),
            enable_pubkey,
            enable_password,
        }
    }
}

impl Authenticator for CompositeAuthenticator {
    fn authenticate_pubkey(
        &self,
        username: &str,
        pubkey: &[u8],
        signature: Option<&[u8]>,
    ) -> Result<AuthResult> {
        if !self.enable_pubkey {
            return Ok(AuthResult::Failure);
        }
        self.ssh_auth.authenticate_pubkey(username, pubkey, signature)
    }

    fn authenticate_password(&self, username: &str, password: &str) -> Result<AuthResult> {
        if !self.enable_password {
            return Ok(AuthResult::Failure);
        }
        self.pam_auth.authenticate_password(username, password)
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_identity_display() {
        let identity = UserIdentity::new(
            "testuser".to_string(),
            1000,
            1000,
            "/home/testuser".to_string(),
            "/bin/bash".to_string(),
        );

        assert_eq!(
            format!("{}", identity),
            "testuser(uid=1000, gid=1000, home=/home/testuser)"
        );
    }

    #[test]
    fn test_auth_result_equality() {
        let success1 = AuthResult::Success(UserIdentity::new(
            "user".to_string(),
            1000,
            1000,
            "/home/user".to_string(),
            "/bin/bash".to_string(),
        ));

        let success2 = AuthResult::Success(UserIdentity::new(
            "user".to_string(),
            1000,
            1000,
            "/home/user".to_string(),
            "/bin/bash".to_string(),
        ));

        assert_eq!(success1, success2);
        assert_eq!(AuthResult::Failure, AuthResult::Failure);
    }
}