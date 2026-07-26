use chrono::{DateTime, Utc};
use std::fs::{self, File, OpenOptions};
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::sync::Mutex;

/// 审计日志记录器
///
/// 用于记录所有关键操作，包括：
/// - 认证成功/失败
/// - 文件操作（读取、写入、删除）
/// - PTY会话创建
/// - 连接建立/断开
///
/// # 线程安全
///
/// 使用 `Mutex` 保证多线程环境下的安全写入。
pub struct AuditLogger {
    log_file: Mutex<File>,
    enabled: bool,
}

impl AuditLogger {
    /// 创建新的审计日志记录器
    ///
    /// # 参数
    ///
    /// - `log_path`: 日志文件路径（如 `/var/log/gnome-remote/audit.log`）
    ///
    /// # 返回
    ///
    /// 返回 `Result<AuditLogger, std::io::Error>`
    ///
    /// # 错误
    ///
    /// - 无法创建日志目录
    /// - 无法打开日志文件
    ///
    /// # 示例
    ///
    /// ```rust
    /// use agent::audit::AuditLogger;
    ///
    /// let logger = AuditLogger::new("/var/log/gnome-remote/audit.log")?;
    /// ```
    pub fn new(log_path: &str) -> std::io::Result<Self> {
        // 自动创建日志目录
        let path = PathBuf::from(log_path);
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // 以追加模式打开文件（不存在则创建）
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .write(true)
            .open(&path)?;

        Ok(Self {
            log_file: Mutex::new(file),
            enabled: true,
        })
    }

    /// 禁用审计日志
    ///
    /// 禁用审计日志
    #[allow(dead_code)]
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// 启用审计日志
    #[allow(dead_code)]
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// 通用日志记录方法
    ///
    /// # 参数
    ///
    /// - `user`: 用户名（如 "root", "alice"）
    /// - `uid`: 用户ID
    /// - `operation`: 操作类型（如 "auth_success", "file_read"）
    /// - `details`: 操作详情（如 "path=/etc/passwd", "reason=invalid_token"）
    ///
    /// # 日志格式
    ///
    /// ```
    /// timestamp user=X uid=X op=X details=X
    /// ```
    fn log_operation(&self, user: &str, uid: u32, operation: &str, details: &str) {
        if !self.enabled {
            return;
        }

        let timestamp: DateTime<Utc> = Utc::now();
        let log_line = format!(
            "{} user={} uid={} op={} details={}\n",
            timestamp.format("%Y-%m-%d %H:%M:%S%.3f"),
            user,
            uid,
            operation,
            details
        );

        // 加锁并写入
        if let Ok(mut file) = self.log_file.lock() {
            let _ = file.write_all(log_line.as_bytes());
        }
    }

    /// 记录认证成功
    ///
    /// # 参数
    ///
    /// - `user`: 用户名
    /// - `uid`: 用户ID
    /// - `method`: 认证方式（如 "token", "pam", "ssh_key"）
    pub fn log_auth_success(&self, user: &str, uid: u32, method: &str) {
        self.log_operation(user, uid, "auth_success", &format!("method={}", method));
    }

    /// 记录认证失败
    ///
    /// # 参数
    ///
    /// - `user`: 用户名（可能为空或无效）
    /// - `uid`: 用户ID（失败时可能为 0 或无效值）
    /// - `reason`: 失败原因（如 "invalid_token", "user_not_found", "pam_auth_failed"）
    pub fn log_auth_failure(&self, user: &str, uid: u32, reason: &str) {
        self.log_operation(user, uid, "auth_failure", &format!("reason={}", reason));
    }

    /// 记录文件操作
    ///
    /// # 参数
    ///
    /// - `user`: 用户名
    /// - `uid`: 用户ID
    /// - `operation`: 操作类型（如 "read", "write", "delete", "upload", "download"）
    /// - `path`: 文件路径
    /// - `size`: 文件大小（字节），如未知可传 0
    ///
    /// # 示例
    ///
    /// ```rust
    /// logger.log_file_operation("alice", 1000, "read", "/etc/passwd", 1234);
    /// logger.log_file_operation("root", 0, "write", "/etc/nginx/nginx.conf", 5678);
    /// 记录文件操作
    #[allow(dead_code)]
    pub fn log_file_operation(&self, user: &str, uid: u32, operation: &str, path: &str, size: u64) {
        self.log_operation(user, uid, &format!("file_{}", operation), &format!("path={} size={}", path, size));
    }

    /// 记录PTY会话创建
    ///
    /// # 参数
    ///
    /// - `user`: 用户名
    /// - `uid`: 用户ID
    /// - `session_id`: 会话ID
    /// - `shell`: 启动的Shell（如 "/bin/bash"）
    #[allow(dead_code)]
    pub fn log_pty_session(&self, user: &str, uid: u32, session_id: &str, shell: &str) {
        self.log_operation(user, uid, "pty_session_start", &format!("session_id={} shell={}", session_id, shell));
    }

    /// 记录连接事件
    ///
    /// # 参数
    ///
    /// - `user`: 用户名
    /// - `uid`: 用户ID
    /// - `event`: 事件类型（如 "connect", "disconnect", "timeout"）
    /// - `client_ip`: 客户端IP地址/// 记录连接事件
    #[allow(dead_code)]
    pub fn log_connection(&self, user: &str, uid: u32, event: &str, client_ip: &str) {
        self.log_operation(user, uid, &format!("connection_{}", event), &format!("client_ip={}", client_ip));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_audit_logger_creation() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();
        let logger = AuditLogger::new(path).unwrap();
        assert!(logger.enabled);
    }

    #[test]
    fn test_log_auth_success() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();
        let logger = AuditLogger::new(path).unwrap();

        logger.log_auth_success("alice", 1000, "token");

        // 读取日志文件验证
        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("user=alice uid=1000 op=auth_success details=method=token"));
    }

    #[test]
    fn test_log_auth_failure() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();
        let logger = AuditLogger::new(path).unwrap();

        logger.log_auth_failure("alice", 1000, "invalid_token");

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("user=alice uid=1000 op=auth_failure details=reason=invalid_token"));
    }

    #[test]
    fn test_log_file_operation() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();
        let logger = AuditLogger::new(path).unwrap();

        logger.log_file_operation("alice", 1000, "read", "/etc/passwd", 1234);

        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("user=alice uid=1000 op=file_read details=path=/etc/passwd size=1234"));
    }

    #[test]
    fn test_disable_enable() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();
        let mut logger = AuditLogger::new(path).unwrap();

        // 禁用后不记录
        logger.disable();
        logger.log_auth_success("alice", 1000, "token");
        let content = fs::read_to_string(path).unwrap();
        assert!(content.is_empty());

        // 启用后恢复记录
        logger.enable();
        logger.log_auth_success("bob", 2000, "pam");
        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("user=bob uid=2000 op=auth_success"));
    }

    #[test]
    fn test_auto_create_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path().join("nested").join("dir").join("audit.log");
        let path_str = log_path.to_str().unwrap();

        let logger = AuditLogger::new(path_str).unwrap();
        logger.log_auth_success("alice", 1000, "token");

        assert!(log_path.exists());
    }
}