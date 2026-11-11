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

/// 允许远程执行的命令白名单（安全关键：在执行点强制）
/// 语法：命令名（argv[0]）；仅接受裸命令名，任何路径形式一律拒绝
const ALLOWED_COMMANDS: &[&str] = &["unzip", "tar", "7z", "7zz", "unrar"];

/// 白名单校验：仅接受白名单内的裸命令名。
///
/// 路径形式（含 `/` 或 `\`）一律拒绝——若按 basename 匹配放行绝对路径，
/// 客户端可先上传同名二进制（如 ~/tmp/unzip）再以路径调用，绕过白名单
/// 以登录用户身份执行任意代码。裸命令名的 PATH 查找由 worker 进程自身
/// 环境完成，客户端无法影响。
fn ensure_command_allowed(command: &str) -> Result<(), String> {
    if command.is_empty() {
        return Err("命令不能为空".to_string());
    }
    if command.contains('/') || command.contains('\\') {
        return Err(format!(
            "命令必须为白名单内的裸命令名，不接受路径形式: {}",
            command
        ));
    }
    if !ALLOWED_COMMANDS.contains(&command) {
        return Err(format!("Command not allowed: {}", command));
    }
    Ok(())
}

/// timeout_secs 为 0 时的默认超时（秒）
const DEFAULT_TIMEOUT_SECS: u64 = 300;

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
/// - 命令白名单强制（ALLOWED_COMMANDS，执行点校验，客户端无法越权）
/// - 通过 UserExecutor 在子进程中降权执行
/// - 超时 kill（timeout_secs，0 = 默认 300s）
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

    // 白名单校验（执行点强制；仅裸命令名，路径形式可被同名二进制绕过）
    if let Err(message) = ensure_command_allowed(&req.command) {
        tracing::warn!("命令不在白名单，拒绝执行: {}", req.command);
        return WorkerResponse {
            payload: Some(worker_response::Payload::Error(Error {
                code: 403,
                message,
            })),
            ..Default::default()
        };
    }

    // 构建用户会话
    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    let executor = UserExecutor::new(&session);

    // 准备命令参数（移入闭包）
    let command = req.command.clone();
    let args = req.args.clone();
    let working_directory = req.working_directory.clone();

    // 超时 + 子进程 PID 追踪：超时 kill 命令进程，避免死循环命令占住通道
    let timeout_secs = if req.timeout_secs > 0 { req.timeout_secs as u64 } else { DEFAULT_TIMEOUT_SECS };
    let child_pid = std::sync::Arc::new(std::sync::Mutex::new(None::<u32>));
    let pid_slot = child_pid.clone();

    // spawn_blocking：execute_as_user 是同步阻塞（fork+wait），放入阻塞线程池
    let exec_handle = tokio::task::spawn_blocking(move || {
        executor.execute_as_user(move || {
            let mut cmd = std::process::Command::new(&command);

            if !args.is_empty() {
                cmd.args(&args);
            }

            if !working_directory.is_empty() {
                cmd.current_dir(&working_directory);
            }

            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
            // 远程命令无交互输入：stdin 必须置空，否则 unzip 等会继承 agent 的
            // 终端等待密码输入，直到 300s 超时才失败（实测加密 zip 解压挂起）
            cmd.stdin(std::process::Stdio::null());

            // 先 spawn 再记录 PID（供超时 kill），随后等待输出
            let child = cmd.spawn()?;
            *pid_slot.lock().unwrap() = Some(child.id());
            let output = child.wait_with_output()?;

            Ok(CommandResult {
                stdout: output.stdout,
                stderr: output.stderr,
                exit_code: output.status.code().unwrap_or(-1),
            })
        })
    });

    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        exec_handle,
    ).await {
        Ok(Ok(inner)) => inner,          // spawn_blocking 正常完成
        Ok(Err(e)) => {                  // spawn_blocking panic / JoinError
            tracing::error!("命令执行线程异常: command={}, error={}", req.command, e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 500,
                    message: format!("Command execution thread failed: {}", e),
                })),
                ..Default::default()
            };
        }
        Err(_) => {                      // 超时：kill 子进程后返回错误
            if let Some(pid) = *child_pid.lock().unwrap() {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid as i32),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
            tracing::warn!("命令执行超时（{}s），已 kill: command={}", timeout_secs, req.command);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 408,
                    message: format!("Command timed out after {}s", timeout_secs),
                })),
                ..Default::default()
            };
        }
    };

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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_accepts_bare_names() {
        assert!(ensure_command_allowed("unzip").is_ok());
        assert!(ensure_command_allowed("tar").is_ok());
        assert!(ensure_command_allowed("7z").is_ok());
        assert!(ensure_command_allowed("unrar").is_ok());
    }

    #[test]
    fn whitelist_rejects_path_forms() {
        // 同名二进制绕过：任何路径形式都不放行，即使 basename 在白名单内
        assert!(ensure_command_allowed("/home/u/evil/unzip").is_err());
        assert!(ensure_command_allowed("/usr/bin/unzip").is_err());
        assert!(ensure_command_allowed("./unzip").is_err());
        assert!(ensure_command_allowed("dir\\unzip").is_err());
        assert!(ensure_command_allowed("../tar").is_err());
    }

    #[test]
    fn whitelist_rejects_unknown_and_empty() {
        assert!(ensure_command_allowed("rm").is_err());
        assert!(ensure_command_allowed("sh").is_err());
        assert!(ensure_command_allowed("").is_err());
    }
}
