//! PTY 工厂 - 用于创建 PTY 会话
//!
//! 该模块负责：
//! - 使用 `forkpty()` 创建 PTY 会话
//! - 配置终端大小、环境变量
//! - 将 master_fd 通过 IPC 发送给 Manager

use anyhow::{Result, Context, anyhow};
use std::os::unix::io::AsRawFd;
use nix::pty::{forkpty, ForkptyResult};
use nix::unistd::{execvp, Pid};
use nix::sys::termios::{tcgetattr, tcsetattr, SetArg, OutputFlags, LocalFlags};
use nix::libc::{ioctl, TIOCSWINSZ, winsize};
use uuid::Uuid;

use super::IpcClient;

/// PTY 工厂
///
/// 负责创建 PTY 会话并将文件描述符转移给 Manager。
/// 注意：PtyFactory 不持有 IpcClient，而是在 create 时借用，避免与主循环的 &mut 冲突。
pub struct PtyFactory;

impl PtyFactory {
    /// 创建 PTY 工厂实例
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let factory = PtyFactory::new();
    /// let (session_id, pid) = factory.create(&ipc_client, "/bin/bash", 80, 24, None)?;
    /// ```
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
    ///
    /// # 返回
    ///
    /// 成功返回 `(session_id, pid)`，失败返回错误。
    ///
    /// # 流程
    ///
    /// 1. 调用 `forkpty()` 创建 PTY
    /// 2. 子进程：设置环境变量、切换工作目录、执行 shell
    /// 3. 父进程：设置终端大小、通过 IPC 发送 master_fd 给 Manager
    ///
    /// # 注意
    ///
    /// master_fd 会自动通过 IPC 发送给 Manager，不在返回值中。
    ///
    /// # 示例
    ///
    /// ```rust,ignore
    /// let factory = PtyFactory::new();
    /// let (session_id, pid) = factory.create(&ipc_client, "/bin/bash", 80, 24, Some("/home/user"))?;
    /// ```
    pub fn create(
        &self,
        ipc_client: &IpcClient,
        shell: &str,
        cols: u32,
        rows: u32,
        cwd: Option<&str>,
    ) -> Result<(String, i32)> {
        // 生成唯一的 session_id
        let session_id = Uuid::new_v4().to_string();

        tracing::debug!(
            "开始创建 PTY 会话: shell={}, cols={}, rows={}, cwd={:?}",
            shell, cols, rows, cwd
        );

        // 转换为 u16（终端大小限制）
        let cols = cols as u16;
        let rows = rows as u16;

        // 1. 调用 forkpty 创建 PTY
        // nix 0.29 将 forkpty 标记为 unsafe（因为它涉及 fork 语义）
        let result = unsafe { forkpty(None, None) }
            .context("Failed to fork PTY")?;

        match result {
            ForkptyResult::Parent { master, child } => {
                // 父进程逻辑
                tracing::info!("PTY 子进程已启动: pid={}", child);

                let master_fd = master.as_raw_fd();

                // 2. 设置终端大小
                self.set_window_size(master_fd, cols, rows)
                    .context("Failed to set window size")?;

                // 3. 通过 IPC 发送 master_fd 给 Manager
                // send_fd 是同步方法，签名为 &self，无需 &mut
                ipc_client.send_fd(master_fd)
                    .context("Failed to send master_fd to Manager")?;

                // 关闭 master_fd，避免资源泄漏
                // 发送 FD 后，父进程不再需要持有该文件描述符
                nix::unistd::close(master_fd)
                    .context("Failed to close master_fd")?;

                tracing::info!(
                    "PTY 会话已创建: session_id={}, pid={}",
                    session_id, child
                );

                Ok((session_id, child.as_raw() as i32))
            }
            ForkptyResult::Child => {
                // 子进程逻辑
                // 注意：子进程中的错误处理要特别小心，不能使用 tracing 等可能已经初始化的库

                // 设置工作目录
                if let Some(dir) = cwd {
                    if let Err(e) = std::env::set_current_dir(dir) {
                        eprintln!("Failed to set working directory: {}", e);
                        std::process::exit(1);
                    }
                }

                // 设置环境变量（TERM、SHELL 等）
                std::env::set_var("TERM", "xterm-256color");
                std::env::set_var("SHELL", shell);

                // 配置终端属性（确保回显等设置正确）
                if let Ok(fd) = nix::unistd::dup2(0, 0) {
                    // 尝试设置终端属性，失败也继续
                    let _ = configure_terminal(fd);
                }

                // 执行 shell
                let shell_path = which::which(shell);
                match shell_path {
                    Ok(path) => {
                        // execvp 要求 args 为 &[&CStr]
                        // nix 0.29: execvp<S: AsRef<CStr>>(filename: S, args: &[S])
                        let path_cstr = std::ffi::CString::new(path.to_string_lossy().as_ref())
                            .context("Invalid shell path")?;
                        let arg0_cstr = std::ffi::CString::new(shell.to_string())
                            .context("Invalid shell arg")?;
                        let args = [arg0_cstr.as_ref()];
                        // execvp 成功时不会返回
                        let _ = execvp(&path_cstr, &args);
                        // 如果执行到这里，说明 execvp 失败了
                        eprintln!("Failed to exec shell: {}", shell);
                        std::process::exit(1);
                    }
                    Err(e) => {
                        eprintln!("Shell not found: {} ({})", shell, e);
                        std::process::exit(1);
                    }
                }
            }
        }
    }

    /// 设置终端窗口大小
    ///
    /// # 参数
    ///
    /// - `fd`: PTY master 文件描述符
    /// - `cols`: 终端列数
    /// - `rows`: 终端行数
    ///
    /// # 返回
    ///
    /// 成功返回 `Ok(())`，失败返回错误。
    fn set_window_size(&self, fd: std::os::unix::io::RawFd, cols: u16, rows: u16) -> Result<()> {
        let ws = winsize {
            ws_col: cols,
            ws_row: rows,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // 使用 unsafe 调用 ioctl（nix 没有提供安全的 TIOCSWINSZ 封装）
        let result = unsafe {
            ioctl(fd, TIOCSWINSZ, &ws)
        };

        if result < 0 {
            return Err(anyhow!("ioctl(TIOCSWINSZ) failed"));
        }

        tracing::debug!("终端窗口大小已设置: cols={}, rows={}", cols, rows);

        Ok(())
    }
}

/// 配置终端属性
///
/// 确保终端回显、规范模式等设置正确。
///
/// # 参数
///
/// - `fd`: 终端文件描述符
///
/// # 返回
///
/// 成功返回 `Ok(())`，失败返回错误。
fn configure_terminal(fd: std::os::unix::io::RawFd) -> Result<()> {
    use std::os::unix::io::BorrowedFd;

    // nix 0.29 的 tcgetattr/tcsetattr 接受 AsFd
    let fd_borrowed = unsafe { BorrowedFd::borrow_raw(fd) };

    // 获取当前终端属性
    let mut termios = tcgetattr(fd_borrowed)
        .context("Failed to get terminal attributes")?;

    // 启用回显、规范模式、信号字符等
    let local_flags = LocalFlags::ECHO
        | LocalFlags::ECHOE
        | LocalFlags::ECHOK
        | LocalFlags::ICANON
        | LocalFlags::ISIG;

    termios.local_flags.insert(local_flags);

    // 禁用输出处理（让 shell 自己处理）
    termios.output_flags.remove(OutputFlags::OPOST);

    // 设置终端属性
    tcsetattr(fd_borrowed, SetArg::TCSANOW, &termios)
        .context("Failed to set terminal attributes")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // 注意：由于 PtyFactory 需要实际的 IPC 连接，单元测试需要在集成测试中进行
    // 这里只测试辅助函数

    #[test]
    fn test_configure_terminal_invalid_fd() {
        // 使用无效的 fd，应该返回错误
        let result = configure_terminal(999);
        assert!(result.is_err());
    }
}