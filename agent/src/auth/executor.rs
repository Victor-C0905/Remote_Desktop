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

use anyhow::{Context, Result};
use std::path::PathBuf;

use super::{UserNamespace, UserSession};

/// 用户上下文执行器
///
/// 封装用户切换逻辑，支持在指定用户的上下文中执行操作。
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
    /// - Linux平台: 使用User Namespace隔离
    /// - 其他平台: 记录警告日志，直接执行
    ///
    /// # 参数
    /// - `f`: 要执行的闭包
    ///
    /// # 返回
    /// 返回闭包的执行结果
    ///
    /// # 安全性
    /// - Linux平台通过User Namespace提供隔离
    /// - 非Linux平台无隔离，仅用于开发/测试
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
        #[cfg(target_os = "linux")]
        {
            tracing::info!(
                "在用户上下文中执行操作: uid={}, gid={}",
                self.uid,
                self.gid
            );

            // 创建User Namespace并切换
            let ns = UserNamespace::new(self.uid, self.gid);
            ns.create_and_switch()
                .context("Failed to create and switch user namespace")?;

            // 执行用户操作
            f()
        }

        #[cfg(not(target_os = "linux"))]
        {
            tracing::warn!(
                "User Namespace仅支持Linux平台，当前平台直接执行（无隔离）: uid={}, gid={}",
                self.uid,
                self.gid
            );
            f()
        }
    }

    /// 在用户上下文中生成子进程（Unix平台，用于PTY）
    ///
    /// # 实现机制
    /// - 使用fork创建子进程
    /// - 子进程中切换到用户上下文
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
    /// - 使用User Namespace隔离
    /// - 子进程继承父进程的文件描述符
    ///
    /// # 平台
    /// 仅在Unix平台可用
    #[cfg(unix)]
    pub fn spawn_process(&self, program: &str, args: &[&str]) -> Result<i32> {
        use nix::unistd::{fork, ForkResult};
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

        // 在子进程执行前设置uid/gid
        unsafe {
            cmd.pre_exec(move || {
                #[cfg(target_os = "linux")]
                {
                    // 创建User Namespace
                    let ns = UserNamespace::new(self.uid, self.gid);
                    ns.create_and_switch()
                        .expect("Failed to switch to user namespace");
                }

                #[cfg(not(target_os = "linux"))]
                {
                    tracing::warn!("User Namespace not supported, skipping user switch");
                }

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
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// 获取用户GID
    pub fn gid(&self) -> u32 {
        self.gid
    }

    /// 获取用户家目录
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