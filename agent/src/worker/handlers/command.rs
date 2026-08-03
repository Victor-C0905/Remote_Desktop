//! 命令执行处理器 - 处理命令执行请求
//!
//! 该模块负责：
//! - 执行系统命令（ExecuteCommand）
//! - 收集命令输出（stdout/stderr）
//! - 返回退出码

use anyhow::{Result, Context};
use tokio::process::Command;
use std::path::Path;

use crate::protocol::generated::{
    ExecuteCommand, CommandOutput,
    WorkerResponse, worker_response, Error,
};

/// 处理 ExecuteCommand 请求
///
/// # 参数
///
/// - `req`: ExecuteCommand 请求参数
///
/// # 返回
///
/// 返回 `WorkerResponse`，包含 `CommandOutput` 或 `Error`。
///
/// # 安全性
///
/// - 不检查命令是否在黑名单中（由 Agent 配置文件控制）
/// - 不限制执行时间（无 timeout）
///
/// # 示例
///
/// ```rust,ignore
/// let req = ExecuteCommand {
///     command: "ls".to_string(),
///     args: vec!["-l".to_string(), "/tmp".to_string()],
///     working_directory: "/home/user".to_string(),
/// };
/// let response = handle_execute_command(req).await;
/// match response.payload {
///     Some(worker_response::Payload::CommandOutput(output)) => {
///         println!("退出码: {}", output.exit_code);
///         println!("输出: {}", String::from_utf8_lossy(&output.stdout));
///     }
///     _ => { /* 错误处理 */ }
/// }
/// ```
#[tracing::instrument(fields(command = %req.command, args = ?req.args))]
pub async fn handle_execute_command(req: ExecuteCommand) -> WorkerResponse {
    tracing::info!(
        "处理 ExecuteCommand 请求: command={}, args={:?}, cwd={}",
        req.command, req.args, req.working_directory
    );

    // 构建命令
    let mut cmd = Command::new(&req.command);

    // 设置参数
    if !req.args.is_empty() {
        cmd.args(&req.args);
    }

    // 设置工作目录
    if !req.working_directory.is_empty() {
        let cwd = Path::new(&req.working_directory);
        if cwd.exists() && cwd.is_dir() {
            cmd.current_dir(cwd);
        } else {
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

    // 设置输出捕获
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // 执行命令
    match cmd.output().await {
        Ok(output) => {
            let exit_code = output.status.code().unwrap_or(-1);

            tracing::info!(
                "命令执行完成: command={}, exit_code={}",
                req.command, exit_code
            );

            // 记录输出（调试级别）
            if !output.stdout.is_empty() {
                tracing::debug!(
                    "命令 stdout: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            if !output.stderr.is_empty() {
                tracing::debug!(
                    "命令 stderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            WorkerResponse {
                payload: Some(worker_response::Payload::CommandOutput(CommandOutput {
                    stdout: output.stdout,
                    stderr: output.stderr,
                    exit_code,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("命令执行失败: command={}, error={}", req.command, e);

            // 判断错误类型
            let (code, message) = if e.kind() == std::io::ErrorKind::NotFound {
                (404, format!("Command not found: {}", req.command))
            } else {
                (500, format!("Failed to execute command: {}", e))
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