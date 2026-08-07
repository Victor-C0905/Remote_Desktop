//! 命令执行处理器 - 处理命令执行请求
//!
//! 该模块负责：
//! - 执行系统命令（ExecuteCommand）
//! - 收集命令输出（stdout/stderr）
//! - 返回退出码
//!
//! # 用户隔离
//!
//! 通过 `UserExecutor` 在 fork 的子进程中以目标用户身份执行命令,
//! 确保命令在正确的用户上下文中运行（UID/GID/家目录）。

use std::path::Path;

use crate::auth::UserExecutor;
use crate::protocol::generated::{
    ExecuteCommand, CommandOutput,
    WorkerResponse, worker_response, Error,
};

/// 构建用户会话（复用 file.rs 中的模式）
fn build_user_session(uid: u32, gid: u32, username: &str, home_dir: &str) -> crate::auth::UserSession {
    use crate::auth::UserIdentity;
    let identity = UserIdentity::new(
        username.to_string(),
        uid,
        gid,
        home_dir.to_string(),
        "/bin/bash".to_string(), // 默认 shell,命令执行不使用
    );
    crate::auth::UserSession::new(identity)
}

/// 命令执行结果（用于 UserExecutor 序列化传递）
#[derive(serde::Serialize, serde::Deserialize)]
struct CommandResult {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: i32,
}

/// 处理 ExecuteCommand 请求
///
/// 在目标用户上下文中执行命令（通过 fork + setuid/setgid）。
///
/// # 参数
///
/// - `req`: ExecuteCommand 请求参数（包含 uid/gid/username/home_dir 用户上下文）
///
/// # 返回
///
/// 返回 `WorkerResponse`，包含 `CommandOutput` 或 `Error`。
///
/// # 安全性
///
/// - 通过 UserExecutor 在子进程中降权执行
/// - 不检查命令是否在黑名单中（由 Agent 配置文件控制）
/// - 不限制执行时间（无 timeout）
#[tracing::instrument(fields(command = %req.command, args = ?req.args, uid = req.uid))]
pub async fn handle_execute_command(req: ExecuteCommand) -> WorkerResponse {
    tracing::info!(
        "处理 ExecuteCommand 请求: command={}, args={:?}, cwd={}, uid={}",
        req.command, req.args, req.working_directory, req.uid
    );

    // 验证工作目录
    if !req.working_directory.is_empty() {
        let cwd = Path::new(&req.working_directory);
        if !cwd.exists() || !cwd.is_dir() {
            tracing::warn!("工作目录不存在或不是目录: {}", req.working_directory);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 400,
                    message: format!("Working directory not found: {}", req.working_directory),
                })),
                ..Default::default()
            };
        }
    }

    // 构建用户会话
    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    let executor = UserExecutor::new(&session);

    // 准备命令参数（移入闭包）
    let command = req.command.clone();
    let args = req.args.clone();
    let working_directory = req.working_directory.clone();

    // 在目标用户上下文中执行命令
    let result = executor.execute_as_user(move || {
        let mut cmd = std::process::Command::new(&command);

        if !args.is_empty() {
            cmd.args(&args);
        }

        if !working_directory.is_empty() {
            cmd.current_dir(&working_directory);
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let output = cmd.output()?;

        Ok(CommandResult {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code().unwrap_or(-1),
        })
    });

    match result {
        Ok(result) => {
            tracing::info!(
                "命令执行完成: command={}, exit_code={}",
                req.command, result.exit_code
            );

            // 记录输出（调试级别）
            if !result.stdout.is_empty() {
                tracing::debug!(
                    "命令 stdout: {}",
                    String::from_utf8_lossy(&result.stdout)
                );
            }
            if !result.stderr.is_empty() {
                tracing::debug!(
                    "命令 stderr: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
            }

            WorkerResponse {
                payload: Some(worker_response::Payload::CommandOutput(CommandOutput {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("命令执行失败: command={}, error={}", req.command, e);

            // 判断错误类型
            let message = e.to_string();
            let code = if message.contains("No such file or directory") {
                404
            } else {
                500
            };

            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code,
                    message,
                })),
                ..Default::default()
            }
        }
    }
}