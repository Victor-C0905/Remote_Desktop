// agent/src/pty.rs
// PTY 会话管理器（仅 Unix 平台）

#[cfg(unix)]
use anyhow::Result;
#[cfg(unix)]
use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::io::RawFd;
#[cfg(unix)]
use std::sync::Arc;
#[cfg(unix)]
use tokio::sync::Mutex;
#[cfg(unix)]
use tracing::{info, warn};

/// PTY 会话（仅 Unix 平台）
#[cfg(unix)]
pub struct PtySession {
    /// 主端文件描述符（用于读写 PTY 数据）
    master_fd: RawFd,
    /// 子进程 PID
    child_pid: i32,
    /// 终端大小
    cols: u16,
    rows: u16,
}

#[cfg(unix)]
impl PtySession {
    /// 创建新的 PTY 会话
    pub fn spawn(shell: &str, cols: u16, rows: u16, working_directory: Option<&str>) -> Result<Self> {
        use nix::pty::{forkpty, Winsize};
        use std::os::fd::IntoRawFd;

        let shell = if shell.is_empty() {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
        } else {
            shell.to_string()
        };

        // 设置终端大小
        let winsize = Winsize {
            ws_col: cols,
            ws_row: rows,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // 使用 forkpty 创建 PTY
        let result = unsafe { forkpty(Some(&winsize), None)? };

        match result {
            nix::pty::ForkptyResult::Parent { child, master } => {
                // 父进程：保存 master fd（使用 into_raw_fd 转移所有权）
                let master_fd = master.into_raw_fd();

                // 设置非阻塞模式（避免阻塞读取）
                let flags = nix::fcntl::OFlag::from_bits_truncate(
                    nix::fcntl::fcntl(master_fd, nix::fcntl::FcntlArg::F_GETFL)
                        .map_err(|e| anyhow::anyhow!("获取文件标志失败: {}", e))?
                );
                let new_flags = flags | nix::fcntl::OFlag::O_NONBLOCK;
                nix::fcntl::fcntl(master_fd, nix::fcntl::FcntlArg::F_SETFL(new_flags))
                    .map_err(|e| anyhow::anyhow!("设置非阻塞模式失败: {}", e))?;

                info!("PTY 创建成功: master_fd={}, child_pid={}", master_fd, child);

                Ok(Self {
                    master_fd,
                    child_pid: child.as_raw() as i32,
                    cols,
                    rows,
                })
            }
            nix::pty::ForkptyResult::Child => {
                // 子进程：执行 shell
                use std::os::unix::process::CommandExt;
                let mut cmd = std::process::Command::new(&shell);

                // 获取 home 目录（跨平台）
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| "/".to_string());

                if let Some(path) = working_directory {
                    // 验证路径是否存在且是目录
                    if std::path::Path::new(path).is_dir() {
                        cmd.current_dir(path);
                        info!("使用指定工作目录: {}", path);
                    } else {
                        cmd.current_dir(&home);
                        warn!("路径不存在或不是目录，回退到 home: {} -> {}", path, home);
                    }
                } else {
                    cmd.current_dir(&home);
                    info!("使用默认工作目录: {}", home);
                }

                cmd.env("TERM", "xterm-256color")
                    .env("COLORTERM", "truecolor")
                    .env("COLUMNS", cols.to_string())
                    .env("LINES", rows.to_string())
                    // 设置 locale 以支持 Unicode 字符（如 htop 边框）
                    // 注意：只设置 LANG，不设置 LC_ALL（避免覆盖系统默认）
                    // 大多数 Linux 系统都支持 en_US.UTF-8 或 C.UTF-8
                    .env("LANG", "en_US.UTF-8")
                    // 如果系统不支持 en_US.UTF-8，尝试 C.UTF-8（大写）
                    .env("LC_CTYPE", "C.UTF-8");

                // 使用 exec 替换当前进程
                let err = cmd.exec();
                warn!("Shell 执行失败: {}", err);
                std::process::exit(1);
            }
        }
    }

    /// 写入数据到 PTY（键盘输入）
    pub fn write(&self, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        // 使用 libc::write 直接写入文件描述符
        let ret = unsafe { libc::write(self.master_fd, data.as_ptr() as *const libc::c_void, data.len()) };
        if ret < 0 {
            return Err(anyhow::anyhow!("PTY 写入失败: {}", nix::errno::Errno::last()));
        }
        Ok(())
    }

    /// 从 PTY 读取数据（终端输出）
    pub fn read(&self) -> Result<Vec<u8>> {
        let mut buf = [0u8; 4096];
        // 使用 libc::read 直接读取文件描述符（非阻塞模式）
        let ret = unsafe { libc::read(self.master_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        match ret {
            n if n > 0 => Ok(buf[..n as usize].to_vec()),
            0 => Ok(Vec::new()),
            _ if nix::errno::Errno::last() == nix::errno::Errno::EAGAIN ||
                 nix::errno::Errno::last() == nix::errno::Errno::EWOULDBLOCK => {
                // 非阻塞模式下无数据可读，返回空数据
                Ok(Vec::new())
            }
            _ if nix::errno::Errno::last() == nix::errno::Errno::EIO => {
                // 子进程已退出
                Ok(Vec::new())
            }
            _ => Err(anyhow::anyhow!("PTY 读取失败: {}", nix::errno::Errno::last())),
        }
    }

    /// 调整终端大小
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        use nix::pty::Winsize;

        let winsize = Winsize {
            ws_col: cols,
            ws_row: rows,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // 使用 libc::ioctl 直接调用
        let ret = unsafe {
            libc::ioctl(self.master_fd, libc::TIOCSWINSZ as libc::c_ulong, &winsize)
        };

        if ret < 0 {
            return Err(anyhow::anyhow!("PTY resize 失败: {}", nix::errno::Errno::last()));
        }

        info!("PTY resize: cols={}, rows={}", cols, rows);
        Ok(())
    }

    /// 检查子进程是否存活
    pub fn is_alive(&self) -> bool {
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
        use nix::unistd::Pid;
        match waitpid(Pid::from_raw(self.child_pid), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) => true,
            Ok(_) => false,
            Err(_) => false,
        }
    }

    /// 获取子进程 PID
    pub fn child_pid(&self) -> i32 {
        self.child_pid
    }
}

#[cfg(unix)]
impl Drop for PtySession {
    fn drop(&mut self) {
        use nix::unistd::close;
        // 关闭 master fd
        let _ = close(self.master_fd);
        info!("PTY 会话关闭: pid={}", self.child_pid);
    }
}

#[cfg(unix)]
impl Clone for PtySession {
    fn clone(&self) -> Self {
        Self {
            master_fd: self.master_fd,
            child_pid: self.child_pid,
            cols: self.cols,
            rows: self.rows,
        }
    }
}

/// PTY 会话管理器（仅 Unix 平台）
#[cfg(unix)]
pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<String, PtySession>>>,
}

#[cfg(unix)]
impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 创建新的 PTY 会话
    pub async fn spawn(&self, shell: &str, cols: u16, rows: u16, working_directory: Option<&str>) -> Result<String> {
        let session = PtySession::spawn(shell, cols, rows, working_directory)?;
        let session_id = format!("pty-{}", uuid::Uuid::new_v4());

        let mut sessions = self.sessions.lock().await;
        sessions.insert(session_id.clone(), session);

        info!("PTY 会话创建: id={}, shell={}, cwd={:?}",
            session_id, shell, working_directory);
        Ok(session_id)
    }

    /// 写入数据到 PTY
    pub async fn write(&self, session_id: &str, data: &[u8]) -> Result<()> {
        let sessions = self.sessions.lock().await;
        let session = sessions.get(session_id)
            .ok_or_else(|| anyhow::anyhow!("PTY 会话不存在: {}", session_id))?;
        session.write(data)
    }

    /// 从 PTY 读取数据
    pub async fn read(&self, session_id: &str) -> Result<Vec<u8>> {
        let sessions = self.sessions.lock().await;
        let session = sessions.get(session_id)
            .ok_or_else(|| anyhow::anyhow!("PTY 会话不存在: {}", session_id))?;
        session.read()
    }

    /// 调整终端大小
    pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<()> {
        let sessions = self.sessions.lock().await;
        let session = sessions.get(session_id)
            .ok_or_else(|| anyhow::anyhow!("PTY 会话不存在: {}", session_id))?;
        session.resize(cols, rows)
    }

    /// 移除 PTY 会话
    pub async fn remove(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(session_id);
        info!("PTY 会话移除: id={}", session_id);
        Ok(())
    }

    /// 获取会话管理器的 Arc 引用
    pub fn inner(&self) -> Arc<Mutex<HashMap<String, PtySession>>> {
        self.sessions.clone()
    }
}

#[cfg(unix)]
impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

// 非 Unix 平台的 stub 实现
#[cfg(not(unix))]
pub struct PtyManager;

#[cfg(not(unix))]
impl PtyManager {
    pub fn new() -> Self {
        Self
    }
}