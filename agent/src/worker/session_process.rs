//! Session 进程实现
//!
//! Session 进程是独立的用户态进程，持有 PTY master_fd 并做 I/O 中继。
//! 通过 abstract UnixSocket 与 Manager 通信。
//!
//! 创建流程:
//! 1. Worker 调用 create_session() → openpty 创建 PTY 对
//! 2. fork → child 是 Session 进程
//! 3. Session 进程 setuid 降权 → fork → 孙进程 execvp(bash)
//! 4. Session 进程 bind abstract socket → listen → accept Manager 连接
//! 5. 单线程 poll 事件循环: 读 master_fd→socket, 读 socket→write master_fd
//!
//! fork 后 tokio runtime 已损坏，本模块全部使用纯同步代码（libc + nix 同步 API）。

use anyhow::{anyhow, Context, Result};
use std::ffi::CString;
use std::os::fd::{AsRawFd, BorrowedFd, IntoRawFd, RawFd};

use nix::libc::{ioctl, winsize, TIOCSCTTY, TIOCSWINSZ};
use nix::pty::openpty;
use nix::sys::socket::{accept, bind, listen, AddressFamily, Backlog, SockFlag, SockType};
use nix::sys::socket::UnixAddr;
use nix::unistd::{close, dup2, execvp, fork, setsid, ForkResult};

use super::session_protocol::{
    msg_type, FRAME_HEADER_SIZE, MAX_FRAME_DATA_SIZE, RECONNECT_TIMEOUT_SECS,
};

/// 用户上下文（用于 Session 进程降权）
#[derive(Debug, Clone)]
pub struct SessionUserContext {
    pub uid: u32,
    pub gid: u32,
    pub username: String,
    pub home_dir: String,
}

/// 创建 Session 的参数
pub struct SessionParams {
    pub session_id: String,
    pub socket_name: String,
    pub shell: String,
    pub cols: u16,
    pub rows: u16,
    pub working_directory: Option<String>,
    pub user: Option<SessionUserContext>,
}

/// 创建 Session 进程的结果（Worker parent 侧）
pub struct SessionCreatedInfo {
    pub session_id: String,
    pub socket_name: String,
    /// Session 进程 PID（不是 bash PID）
    pub session_pid: i32,
}

/// 创建 Session 进程
///
/// Worker 调用此函数，fork 出 Session 进程。
/// Worker parent 立即返回，Session 进程在 child 中运行。
pub fn create_session(params: SessionParams) -> Result<SessionCreatedInfo> {
    // 1. openpty 创建 PTY 对
    let ws = winsize {
        ws_row: params.rows,
        ws_col: params.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let pty = openpty(&ws, None).context("openpty 失败")?;

    let session_id = params.session_id.clone();
    let socket_name = params.socket_name.clone();

    // 2. fork → child 是 Session 进程
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            // Worker parent: pty.master 和 pty.slave 是 OwnedFd，
            // 函数返回时自动 Drop 关闭 fd
            tracing::info!(
                "Session 进程已创建: pid={}, session_id={}",
                child,
                session_id
            );

            Ok(SessionCreatedInfo {
                session_id,
                socket_name,
                session_pid: child.as_raw() as i32,
            })
        }
        Ok(ForkResult::Child) => {
            // Session 进程（child）
            // 消费 pty 的 OwnedFd，防止 Drop 在 fork 后误关 fd
            // into_raw_fd 返回 RawFd 并放弃 OwnedFd 的所有权
            let master_fd = pty.master.into_raw_fd();
            let slave_fd = pty.slave.into_raw_fd();

            // 注意: fork 后 tokio runtime 已损坏，不能用 tracing
            if let Err(e) = run_session_process(params, master_fd, slave_fd) {
                eprintln!("Session 进程错误: {}", e);
            }
            // 必须用 std::process::exit，不能用 return（避免运行析构函数导致死锁）
            std::process::exit(0);
        }
        Err(e) => Err(anyhow!("fork 失败: {}", e)),
    }
}

/// 加载用户登录环境（对标 GNOME GDM/Xsession 的会话环境加载）
///
/// 通过以目标用户身份执行 `<shell> -l -c 'env'` 获取完整登录环境。
/// 当前进程已通过 setuid 降权为目标用户，子进程会继承该身份。
///
/// 获取的环境变量包括：
/// - PATH（用户自定义的，包含 ~/.local/bin 等）
/// - HOSTNAME、LANG、LC_*
/// - 用户在 ~/.bash_profile / ~/.profile 中设置的自定义变量
///
/// 后续 fork 的 bash 进程会继承这些环境变量，与非登录 shell 的 ~/.bashrc 读取配合，
/// 完全复现 GNOME 桌面终端的环境（登录时加载会话环境，终端继承）。
fn load_login_environment(shell: &str, user: &SessionUserContext) -> Result<Vec<(String, String)>> {
    use std::process::Command;

    let output = Command::new(shell)
        .arg("-l")
        .arg("-c")
        .arg("env")
        .env("HOME", &user.home_dir)
        .env("USER", &user.username)
        .env("LOGNAME", &user.username)
        .stdin(std::process::Stdio::null())  // 防止 .bash_profile 中的 read 命令阻塞
        .output()
        .context("执行登录 shell 获取环境失败")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("登录 shell 执行失败: {}", stderr));
    }

    // 解析 env 输出（KEY=VALUE 格式，每行一个）
    let env_str = String::from_utf8_lossy(&output.stdout);
    let vars: Vec<(String, String)> = env_str
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '=');
            let key = parts.next()?.to_string();
            let value = parts.next()?.to_string();
            if key.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .collect();

    Ok(vars)
}

/// Session 进程主逻辑
///
/// 步骤:
/// 1. 忽略 SIGPIPE（写已关闭的 socket 不应杀进程）
/// 2. setuid 降权
/// 3. 加载用户登录环境（对标 GNOME GDM/Xsession 的会话环境加载）
/// 4. bind abstract socket
/// 5. fork 孙进程 execvp(bash)
/// 6. close slave（只需 master）
/// 7. listen + session_io_loop
fn run_session_process(params: SessionParams, master_fd: RawFd, slave_fd: RawFd) -> Result<()> {
    // 1. 忽略 SIGPIPE（write 到已关闭的 socket 不应杀进程）
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    // 2. 降权（必须在 fork 孙进程前完成，确保孙进程继承降权后的身份）
    if let Some(ref user) = params.user {
        // 先 setgid 再 setuid
        let ret = unsafe { libc::setgid(user.gid as libc::gid_t) };
        if ret != 0 {
            eprintln!("setgid 失败: {}", std::io::Error::last_os_error());
            std::process::exit(1);
        }
        let ret = unsafe { libc::setuid(user.uid as libc::uid_t) };
        if ret != 0 {
            eprintln!("setuid 失败: {}", std::io::Error::last_os_error());
            std::process::exit(1);
        }
    }

    // 3. 加载用户登录环境（对标 GNOME GDM/Xsession 的会话环境加载）
    //    通过以目标用户身份执行 `<shell> -l -c 'env'` 获取完整登录环境，
    //    包括 PATH、HOSTNAME、LANG、用户在 ~/.bash_profile 中设置的自定义变量等。
    //    后续 fork 的 bash 进程会继承这些环境变量，与非登录 shell 的 ~/.bashrc 读取配合，
    //    完全复现 GNOME 桌面终端的环境（登录时加载会话环境，终端继承）。
    if let Some(ref user) = params.user {
        match load_login_environment(&params.shell, user) {
            Ok(env_vars) => {
                for (key, value) in &env_vars {
                    std::env::set_var(key, value);
                }
                eprintln!("已加载 {} 个登录环境变量", env_vars.len());
            }
            Err(e) => {
                eprintln!("加载登录环境失败(回退到最小环境): {}", e);
                // 回退:设置最小环境变量
                std::env::set_var("HOME", &user.home_dir);
                std::env::set_var("USER", &user.username);
                std::env::set_var("LOGNAME", &user.username);
                std::env::set_var("SHELL", &params.shell);
            }
        }
    }
    // TERM 不在登录环境中，需要显式设置
    std::env::set_var("TERM", "xterm-256color");

    // 设置工作目录
    let cwd = if let Some(ref cwd) = params.working_directory {
        cwd.clone()
    } else if let Some(ref user) = params.user {
        user.home_dir.clone()
    } else {
        "/".to_string()
    };
    if let Err(e) = std::env::set_current_dir(&cwd) {
        eprintln!("set_current_dir 失败: {}, 使用 /", e);
        let _ = std::env::set_current_dir("/");
    }

    // 4. bind abstract socket
    let sock_fd = create_abstract_socket(&params.socket_name)?;

    // 5. fork 孙进程 execvp(bash)
    let shell_cstr = CString::new(params.shell.as_str())
        .map_err(|e| anyhow!("shell 路径含 null: {}", e))?;

    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            // Session 进程: 关闭 slave（只需 master）
            let _ = close(slave_fd);

            let bash_pid = child.as_raw() as i32;

            eprintln!(
                "Session 进程就绪: pid={}, session_id={}, bash_pid={}",
                std::process::id(),
                params.session_id,
                bash_pid
            );

            // 6. listen + I/O 循环
            let borrowed = unsafe { BorrowedFd::borrow_raw(sock_fd) };
            listen(&borrowed, Backlog::new(1).context("Backlog 无效")?)
                .context("listen 失败")?;

            session_io_loop(master_fd, sock_fd, bash_pid, &params.session_id)?;

            // 清理
            let _ = close(master_fd);
            let _ = close(sock_fd);

            Ok(())
        }
        Ok(ForkResult::Child) => {
            // 孙进程: execvp(bash)
            // 关闭 master（孙进程不需要）
            let _ = close(master_fd);
            // 关闭 socket fd（孙进程不需要）
            let _ = close(sock_fd);

            // setsid 创建新会话
            let _ = setsid();

            // slave 成为控制终端
            unsafe {
                ioctl(slave_fd, TIOCSCTTY, 0);
            }

            // dup2 slave 到 0/1/2
            dup2(slave_fd, 0).context("dup2 stdin 失败")?;
            dup2(slave_fd, 1).context("dup2 stdout 失败")?;
            dup2(slave_fd, 2).context("dup2 stderr 失败")?;

            // 关闭原始 slave fd
            if slave_fd > 2 {
                let _ = close(slave_fd);
            }

            // execvp shell
            let args = [shell_cstr.as_c_str()];
            let _ = execvp(&shell_cstr, &args);

            // execvp 失败才会到这里
            eprintln!("execvp 失败: {}", std::io::Error::last_os_error());
            std::process::exit(127);
        }
        Err(e) => {
            eprintln!("fork 孙进程失败: {}", e);
            std::process::exit(1);
        }
    }
}

/// 创建 abstract UnixSocket 并 bind
///
/// 返回 RawFd（调用方负责关闭）
fn create_abstract_socket(socket_name: &str) -> Result<RawFd> {
    let fd = nix::sys::socket::socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::empty(),
        None,
    )
    .context("socket 创建失败")?;

    // UnixAddr::new_abstract 自动添加 \0 前缀
    let addr = UnixAddr::new_abstract(socket_name.as_bytes())
        .map_err(|e| anyhow!("abstract socket 地址创建失败: {}", e))?;

    bind(fd.as_raw_fd(), &addr).context("bind 失败")?;

    // 消费 OwnedFd，返回 RawFd（调用方管理生命周期）
    Ok(fd.into_raw_fd())
}

/// Session I/O 中继主循环（单线程 poll 事件循环）
///
/// 监听:
/// - master_fd 可读 → 读终端输出 → 发送 PtyOutput 帧给 Manager
/// - conn_fd 可读 → 读 Manager 消息 → 写入 master_fd 或处理 Resize/Close
/// - bash 退出 → 发送 EOF → 退出
///
/// Manager 断开时:
/// - 关闭当前 conn_fd
/// - 重新 accept（30秒超时，超时则 kill bash 并退出）
/// - bash 在重连窗口期内退出 → 正常退出
fn session_io_loop(master_fd: RawFd, sock_fd: RawFd, bash_pid: i32, session_id: &str) -> Result<()> {
    let mut first_connection = true;

    loop {
        // === Accept Manager 连接 ===
        let conn_fd = if first_connection {
            first_connection = false;
            accept(sock_fd).context("首次 accept 失败")?
        } else {
            // 重连阶段: poll 等待新连接或 bash 退出，30秒超时
            match wait_for_reconnect(sock_fd, bash_pid) {
                ReconnectResult::Connected(fd) => fd,
                ReconnectResult::BashExited => return Ok(()),
                ReconnectResult::Timeout => {
                    // 超时 → kill bash 并退出
                    eprintln!(
                        "Session 重连超时 ({}秒), 终止 bash: pid={}",
                        RECONNECT_TIMEOUT_SECS, bash_pid
                    );
                    unsafe {
                        libc::kill(bash_pid, libc::SIGHUP);
                    }
                    let mut status: i32 = 0;
                    unsafe {
                        libc::waitpid(bash_pid, &mut status, 0);
                    }
                    return Ok(());
                }
            }
        };

        // 发送 Hello 验证
        if send_frame(conn_fd, msg_type::HELLO, session_id.as_bytes()).is_err() {
            let _ = close(conn_fd);
            continue;
        }

        eprintln!(
            "Session I/O 循环启动: session_id={}, bash_pid={}",
            session_id, bash_pid
        );

        // === 事件循环 ===
        let mut bash_exited = false;
        let mut socket_disconnected = false;

        while !bash_exited && !socket_disconnected {
            let mut pfds = [
                libc::pollfd {
                    fd: master_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: conn_fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];

            // poll 等待事件（1秒超时用于周期性检查 bash 退出）
            let ret = unsafe { libc::poll(pfds.as_mut_ptr(), 2, 1000) };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return Err(anyhow!("poll 失败: {}", err));
            }

            // 周期性检查 bash 是否退出（waitpid WNOHANG）
            if ret == 0 {
                // poll 超时，检查 bash
                let mut status: i32 = 0;
                let wait_ret = unsafe { libc::waitpid(bash_pid, &mut status, libc::WNOHANG) };
                if wait_ret == bash_pid {
                    let _ = send_frame(conn_fd, msg_type::EOF, &[]);
                    bash_exited = true;
                    break;
                }
                continue;
            }

            // === 处理 master_fd 可读（终端输出）===
            if pfds[0].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
                let mut buf = [0u8; 4096];
                let n = unsafe {
                    libc::read(master_fd, buf.as_mut_ptr() as *mut _, buf.len())
                };
                if n > 0 {
                    // 终端输出 → 发给 Manager
                    if send_frame(conn_fd, msg_type::PTY_OUTPUT, &buf[..n as usize]).is_err() {
                        socket_disconnected = true;
                    }
                } else if n == 0 {
                    // EOF (bash 退出导致 slave 关闭)
                    let _ = send_frame(conn_fd, msg_type::EOF, &[]);
                    bash_exited = true;
                } else {
                    let err = std::io::Error::last_os_error();
                    if err.raw_os_error() != Some(libc::EINTR) {
                        // 读错误，可能是 bash 退出
                        let _ = send_frame(conn_fd, msg_type::EOF, &[]);
                        bash_exited = true;
                    }
                }
            }

            // === 处理 conn_fd 可读（Manager 消息）===
            if !bash_exited && pfds[1].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
                match recv_frame(conn_fd) {
                    Ok((t, data)) => match t {
                        t if t == msg_type::PTY_INPUT => {
                            // 键盘输入 → 写入 master_fd
                            if write_all_sync(master_fd, &data).is_err() {
                                // master_fd 写失败，bash 可能已退出
                            }
                        }
                        t if t == msg_type::RESIZE => {
                            // 窗口大小调整
                            if data.len() >= 8 {
                                let cols = u32::from_be_bytes([
                                    data[0], data[1], data[2], data[3],
                                ]);
                                let rows = u32::from_be_bytes([
                                    data[4], data[5], data[6], data[7],
                                ]);
                                let ws = winsize {
                                    ws_row: rows as u16,
                                    ws_col: cols as u16,
                                    ws_xpixel: 0,
                                    ws_ypixel: 0,
                                };
                                unsafe {
                                    ioctl(master_fd, TIOCSWINSZ, &ws);
                                }
                            }
                        }
                        t if t == msg_type::CLOSE => {
                            // Manager 要求关闭
                            socket_disconnected = true;
                        }
                        _ => {
                            // 其他消息（如 Hello 回显），忽略
                        }
                    },
                    Err(_) => {
                        // 读失败 → Manager 断开
                        socket_disconnected = true;
                    }
                }
            }

            // === 检查错误事件 ===
            if pfds[0].revents & libc::POLLERR != 0 {
                let _ = send_frame(conn_fd, msg_type::EOF, &[]);
                bash_exited = true;
            }
            if pfds[1].revents & libc::POLLERR != 0 {
                socket_disconnected = true;
            }
        }

        // 关闭当前 Manager 连接
        let _ = close(conn_fd);

        if bash_exited {
            // bash 已退出，回收子进程
            let mut status: i32 = 0;
            unsafe {
                libc::waitpid(bash_pid, &mut status, 0);
            }
            eprintln!(
                "Session 进程退出: session_id={}, bash_pid={}, status={}",
                session_id, bash_pid, status
            );
            return Ok(());
        }

        // socket 断开，进入重连阶段（下一轮循环）
        eprintln!(
            "Manager 断开，等待重连: session_id={}, bash_pid={}",
            session_id, bash_pid
        );
    }
}

/// 重连阶段的结果
enum ReconnectResult {
    /// 新 Manager 已连接
    Connected(RawFd),
    /// bash 在等待期间退出
    BashExited,
    /// 超时
    Timeout,
}

/// 等待 Manager 重连或 bash 退出
///
/// 每 1 秒检查一次 bash 是否退出，最多等待 RECONNECT_TIMEOUT_SECS 秒。
fn wait_for_reconnect(sock_fd: RawFd, bash_pid: i32) -> ReconnectResult {
    let start = std::time::Instant::now();
    let timeout_ms = (RECONNECT_TIMEOUT_SECS as u64) * 1000;

    loop {
        // 检查 bash 是否已退出
        let mut status: i32 = 0;
        let wait_ret = unsafe { libc::waitpid(bash_pid, &mut status, libc::WNOHANG) };
        if wait_ret == bash_pid {
            return ReconnectResult::BashExited;
        }

        // 检查超时
        if start.elapsed().as_millis() as u64 >= timeout_ms {
            return ReconnectResult::Timeout;
        }

        // poll 等待新连接（1秒超时）
        let mut pfd = libc::pollfd {
            fd: sock_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ret = unsafe { libc::poll(&mut pfd, 1, 1000) };
        if ret > 0 {
            // 有新连接
            match accept(sock_fd) {
                Ok(fd) => return ReconnectResult::Connected(fd),
                Err(_) => {
                    // accept 失败，继续等待
                    continue;
                }
            }
        }
        // ret == 0 (超时) 或 ret < 0 (EINTR) → 继续循环
    }
}

// === 帧协议辅助函数 ===

/// 发送一帧数据
///
/// 帧格式: [1字节类型][4字节长度(big-endian)][数据]
fn send_frame(fd: RawFd, msg_type: u8, data: &[u8]) -> Result<()> {
    let len = data.len() as u32;
    let mut header = [0u8; FRAME_HEADER_SIZE];
    header[0] = msg_type;
    header[1..5].copy_from_slice(&len.to_be_bytes());

    // 写 header
    write_all_sync(fd, &header)?;
    // 写 data
    if !data.is_empty() {
        write_all_sync(fd, data)?;
    }
    Ok(())
}

/// 接收一帧数据
fn recv_frame(fd: RawFd) -> Result<(u8, Vec<u8>)> {
    // 读 header
    let header = read_all_sync(fd, FRAME_HEADER_SIZE)?;
    let msg_type = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;

    if len > MAX_FRAME_DATA_SIZE {
        return Err(anyhow!("帧数据过大: {} > {}", len, MAX_FRAME_DATA_SIZE));
    }

    // 读 data
    let data = if len > 0 {
        read_all_sync(fd, len)?
    } else {
        Vec::new()
    };

    Ok((msg_type, data))
}

/// 同步写入所有数据（处理部分写入和 EINTR）
fn write_all_sync(fd: RawFd, mut data: &[u8]) -> Result<()> {
    while !data.is_empty() {
        let n = unsafe { libc::write(fd, data.as_ptr() as *const _, data.len()) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(anyhow!("write 失败: {}", err));
        }
        data = &data[n as usize..];
    }
    Ok(())
}

/// 同步读取确切字节数（处理部分读取和 EINTR）
fn read_all_sync(fd: RawFd, total: usize) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; total];
    let mut read = 0;
    while read < total {
        let n = unsafe {
            libc::read(fd, buf[read..].as_mut_ptr() as *mut _, total - read)
        };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(anyhow!("read 失败: {}", err));
        }
        if n == 0 {
            return Err(anyhow!("EOF: 读取到 {} 字节, 期望 {}", read, total));
        }
        read += n as usize;
    }
    Ok(buf)
}
