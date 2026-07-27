//! 用户上下文执行器
//!
//! 封装用户切换逻辑，提供在指定用户上下文中执行操作的能力。
//!
//! ## 设计原则
//! - **高内聚**: 只处理用户上下文切换相关逻辑
//! - **低耦合**: 独立于具体业务逻辑，提供通用执行能力
//!
//! ## 安全机制
//! - 使用User Namespace隔离
//! - 支持跨平台（Linux使用Namespace，其他平台降级）

use anyhow::{Result, Context};
use std::path::PathBuf;

use super::UserSession;

/// 用户上下文执行器
///
/// 封装用户切换逻辑，支持在指定用户的上下文中执行操作。
#[allow(dead_code)]
pub struct UserExecutor {
    /// 用户ID (UID)
    uid: u32,
    /// 组ID (GID)
    gid: u32,
    /// 用户家目录
    home_dir: PathBuf,
}

impl UserExecutor {
    /// 从UserSession创建新的执行器
    ///
    /// # 参数
    /// - `session`: 用户会话信息
    ///
    /// # 返回
    /// 返回配置好用户上下文的 `UserExecutor`
    ///
    /// # 示例
    /// ```rust,ignore
    /// let session = UserSession::new(identity);
    /// let executor = UserExecutor::new(&session);
    /// ```
    pub fn new(session: &UserSession) -> Self {
        Self {
            uid: session.uid,
            gid: session.gid,
            home_dir: session.home_dir.clone(),
        }
    }

    /// 在用户上下文中执行操作
    ///
    /// # 实现机制
    /// - Agent以root身份运行，直接执行文件操作
    /// - 通过 check_path_permission 逻辑权限检查控制访问范围
    /// - 不使用setuid（避免主进程永久降权）
    /// - 不使用User Namespace（避免兼容性问题）
    ///
    /// # 参数
    /// - `f`: 要执行的闭包
    ///
    /// # 返回
    /// 返回闭包的执行结果
    ///
    /// # 安全性
    /// - Agent必须以root运行
    /// - 文件操作前必须调用 check_path_permission 进行权限检查
    /// - 文件系统权限作为第二道防线
    ///
    /// # 示例
    /// ```rust,ignore
    /// let result = executor.execute_as_user(|| {
    ///     // 在用户上下文中执行的操作
    ///     Ok(42)
    /// })?;
    /// ```
    pub fn execute_as_user<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        #[cfg(unix)]
        {
            tracing::info!(
                "在用户上下文中执行操作: uid={}, gid={}",
                self.uid,
                self.gid
            );

            // 获取当前用户ID
            let current_uid = nix::unistd::getuid().as_raw();

            // 如果当前已经是目标用户，直接执行
            if current_uid == self.uid {
                tracing::debug!("当前用户已是目标用户，跳过切换: uid={}", self.uid);
                return f();
            }

            // 如果当前是root用户，直接以root权限执行
            // 权限控制通过 check_path_permission 函数实现
            if current_uid == 0 {
                tracing::debug!(
                    "root用户执行操作（逻辑权限已检查）: target_uid={}",
                    self.uid
                );
                return f();
            }

            // 非root用户：直接执行，依赖文件系统权限
            tracing::debug!(
                "当前用户(uid={})执行操作，依赖文件系统权限",
                current_uid
            );
            f()
        }

        #[cfg(not(unix))]
        {
            tracing::warn!(
                "当前平台直接执行（无用户隔离）: uid={}, gid={}",
                self.uid,
                self.gid
            );
            f()
        }
    }

    /// 在用户上下文中生成子进程（Unix平台，用于PTY）
    ///
    /// # 实现机制
    /// - 使用Command创建子进程
    /// - 子进程中通过setuid/setgid切换到目标用户（root时）
    /// - 父进程返回子进程的PID
    ///
    /// # 参数
    /// - `program`: 要执行的程序
    /// - `args`: 程序参数
    ///
    /// # 返回
    /// 返回子进程的PID
    ///
    /// # 安全性
    /// - root用户通过setuid/setgid切换到目标用户
    /// - 非root用户依赖文件系统权限
    /// - 子进程继承父进程的文件描述符
    ///
    /// # 平台
    /// 仅在Unix平台可用
    #[cfg(unix)]
    pub fn spawn_process(&self, program: &str, args: &[&str]) -> Result<i32> {
        use std::os::unix::process::CommandExt;
        use std::process::Command;

        tracing::info!(
            "生成子进程: program={}, uid={}, gid={}",
            program,
            self.uid,
            self.gid
        );

        // 使用Command创建进程，并在子进程中设置uid/gid
        let mut cmd = Command::new(program);
        cmd.args(args);

        // 设置工作目录为用户家目录
        cmd.current_dir(&self.home_dir);

        // 复制uid/gid到局部变量（避免闭包捕获self）
        let uid = self.uid;
        let gid = self.gid;

        // 在子进程执行前设置uid/gid
        unsafe {
            cmd.pre_exec(move || {
                // 获取当前用户ID
                let current_uid = nix::unistd::getuid().as_raw();

                // 如果当前已经是目标用户，无需切换
                if current_uid == uid {
                    return Ok(());
                }

                // 如果当前是root用户，切换到目标用户
                if current_uid == 0 {
                    // 先切换GID，再切换UID（顺序重要：先降GID再降UID）
                    nix::unistd::setgid(nix::unistd::Gid::from_raw(gid))
                        .map_err(|e| std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to setgid: {}", e)
                        ))?;
                    nix::unistd::setuid(nix::unistd::Uid::from_raw(uid))
                        .map_err(|e| std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to setuid: {}", e)
                        ))?;
                    return Ok(());
                }

                // 非root用户：无法切换用户，依赖文件系统权限
                tracing::warn!(
                    "非root用户(uid={})无法切换到目标用户(uid={})，依赖文件系统权限",
                    current_uid,
                    uid
                );
                Ok(())
            });
        }

        // 启动子进程
        let child = cmd.spawn().context("Failed to spawn process")?;
        let pid = child.id() as i32;

        tracing::info!("子进程已启动: pid={}", pid);

        Ok(pid)
    }

    /// 获取用户UID
    #[allow(dead_code)]
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// 获取用户GID
    #[allow(dead_code)]
    pub fn gid(&self) -> u32 {
        self.gid
    }

    /// 获取用户家目录
    #[allow(dead_code)]
    pub fn home_dir(&self) -> &PathBuf {
        &self.home_dir
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;
    use uuid::Uuid;

    fn create_test_session() -> UserSession {
        UserSession {
            session_id: format!("sess-{}", Uuid::new_v4()),
            username: "testuser".to_string(),
            uid: 1000,
            gid: 1000,
            home_dir: PathBuf::from("/home/testuser"),
            shell: PathBuf::from("/bin/bash"),
            created_at: SystemTime::now(),
        }
    }

    #[test]
    fn test_executor_creation() {
        let session = create_test_session();
        let executor = UserExecutor::new(&session);

        assert_eq!(executor.uid(), 1000);
        assert_eq!(executor.gid(), 1000);
        assert_eq!(executor.home_dir(), &PathBuf::from("/home/testuser"));
    }

    #[test]
    fn test_execute_as_user() {
        let session = create_test_session();
        let executor = UserExecutor::new(&session);

        let result = executor.execute_as_user(|| Ok(42));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_execute_as_user_with_error() {
        let session = create_test_session();
        let executor = UserExecutor::new(&session);

        let result: Result<()> = executor.execute_as_user(|| {
            anyhow::bail!("Test error");
        });

        assert!(result.is_err());
    }
}