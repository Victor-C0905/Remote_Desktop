//! PTY 工厂 - 创建 Session 进程
//!
//! 该模块负责：
//! - 生成 session_id 和 socket_name
//! - 调用 session_process::create_session 创建 Session 进程
//! - 返回 socket_name 给 Worker（Worker 转发给 Manager）
//!
//! 注意：PtyFactory 不再使用 forkpty，不再通过 SCM_RIGHTS 发送 master_fd。
//! master_fd 由 Session 进程持有，Manager 通过 UnixSocket 与 Session 通信。

use anyhow::{Context, Result};
use uuid::Uuid;

use super::session_process::{
    create_session, SessionCreatedInfo, SessionParams, SessionUserContext,
};
use super::session_protocol::generate_socket_name;

/// 用户上下文信息(用于 PTY 用户隔离)
#[derive(Debug, Clone)]
pub struct UserContext {
    pub uid: u32,
    pub gid: u32,
    pub username: String,
    pub home_dir: String,
}

impl UserContext {
    /// 转换为 Session 进程的 UserContext
    pub fn to_session_context(&self) -> SessionUserContext {
        SessionUserContext {
            uid: self.uid,
            gid: self.gid,
            username: self.username.clone(),
            home_dir: self.home_dir.clone(),
        }
    }
}

/// PTY 工厂
///
/// 负责创建 Session 进程并返回 socket_name。
/// 不再持有 IpcClient，不再通过 SCM_RIGHTS 发送 master_fd。
pub struct PtyFactory;

impl PtyFactory {
    /// 创建 PTY 工厂实例
    pub fn new() -> Self {
        Self
    }

    /// 创建 PTY 会话
    ///
    /// # 参数
    ///
    /// - `shell`: Shell 程序路径（如 `/bin/bash`）
    /// - `cols`: 终端列数
    /// - `rows`: 终端行数
    /// - `cwd`: 工作目录（可选）
    /// - `user_info`: 用户上下文（可选，Some 时启用用户隔离）
    ///
    /// # 返回
    ///
    /// SessionCreatedInfo，包含 session_id、socket_name、session_pid
    pub fn create(
        &self,
        shell: &str,
        cols: u32,
        rows: u32,
        cwd: Option<&str>,
        user_info: Option<&UserContext>,
    ) -> Result<SessionCreatedInfo> {
        // 生成唯一的 session_id 和 socket_name
        let session_id = Uuid::new_v4().to_string();
        let socket_name = generate_socket_name(&session_id);

        tracing::debug!(
            "开始创建 Session 进程: shell={}, cols={}, rows={}, cwd={:?}",
            shell, cols, rows, cwd
        );

        // 构建参数
        let params = SessionParams {
            session_id: session_id.clone(),
            socket_name: socket_name.clone(),
            shell: shell.to_string(),
            cols: cols as u16,
            rows: rows as u16,
            working_directory: cwd.map(|s| s.to_string()),
            user: user_info.map(|u| u.to_session_context()),
        };

        // 创建 Session 进程
        let result = create_session(params).context("创建 Session 进程失败")?;

        tracing::info!(
            "Session 进程已创建: session_id={}, socket_name={}, pid={}",
            result.session_id,
            result.socket_name,
            result.session_pid
        );

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    // 注意：由于 PtyFactory 需要 fork 和 openpty，单元测试需要在集成测试中进行
}
