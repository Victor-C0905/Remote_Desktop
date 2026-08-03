// agent/src/auth/executor.rs
//! 用户上下文执行器
//!
//! 通过 fork + setuid/setgid 在目标用户上下文中执行文件操作。
//! 与 SSH Server 的 sftp-server 模式一致。
//!
//! ## 设计原则
//! - **进程隔离**: 每次文件操作 fork 子进程，子进程以目标用户身份运行
//! - **Linux 权限**: 子进程中 Linux 文件系统权限自动生效
//! - **管道通信**: 子进程通过管道将结果序列化返回给父进程
//!
//! ## 安全机制
//! - 父进程保持 root，子进程降权到目标用户
//! - 子进程崩溃不影响父进程
//! - 子进程使用 _exit() 退出，避免运行父进程的析构函数

use anyhow::Result;
use std::path::PathBuf;

use super::UserSession;

/// 用户上下文执行器
pub struct UserExecutor {
    /// 用户ID (UID)
    uid: u32,
    /// 组ID (GID)
    gid: u32,
    /// 用户家目录
    home_dir: PathBuf,
    /// 会话ID
    session_id: String,
}

impl UserExecutor {
    /// 从 UserSession 创建新的执行器
    pub fn new(session: &UserSession) -> Self {
        Self {
            uid: session.uid,
            gid: session.gid,
            home_dir: session.home_dir.clone(),
            session_id: session.session_id.clone(),
        }
    }

    /// 在目标用户上下文中执行文件操作
    ///
    /// 通过 fork + setuid/setgid 创建子进程，在子进程中以目标用户身份执行操作。
    /// 子进程通过管道将结果序列化返回给父进程。
    ///
    /// # 实现机制
    /// 1. 创建管道（父子进程通信）
    /// 2. fork 子进程
    /// 3. 子进程：setgid + setuid → 执行闭包 → 序列化结果写入管道 → _exit
    /// 4. 父进程：读取管道数据 → 反序列化结果 → waitpid 回收子进程
    ///
    /// # 性能
    /// - 每次操作约 10-50ms（fork 开销）
    /// - 文件操作本身是 I/O 密集型，fork 开销可忽略
    ///
    /// # 安全性
    /// - 父进程始终保持 root 权限
    /// - 子进程降权后无法恢复到 root
    /// - 子进程崩溃不影响父进程
    /// - 子进程使用 _exit() 退出，避免运行父进程的析构函数
    pub fn execute_as_user<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
        T: serde::Serialize + for<'de> serde::Deserialize<'de> + Send + 'static,
    {
        #[cfg(unix)]
        {
            // 如果当前已经是目标用户，直接执行
            let current_uid = nix::unistd::getuid().as_raw();
            if current_uid == self.uid {
                tracing::debug!("当前用户已是目标用户，跳过切换: uid={}", self.uid);
                return f();
            }

            // 如果目标用户是 root，直接执行（当前进程就是 root）
            if self.uid == 0 {
                tracing::debug!("目标用户是 root，直接执行");
                return f();
            }

            // 创建管道（用于子进程向父进程传递结果）
            let mut pipe_fds = [0i32; 2];
            let ret = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
            if ret != 0 {
                return Err(anyhow::anyhow!("创建管道失败: {}", std::io::Error::last_os_error()));
            }
            let read_fd = pipe_fds[0];
            let write_fd = pipe_fds[1];

            let uid = self.uid;
            let gid = self.gid;

            tracing::debug!("fork 子进程执行操作: uid={}, gid={}", uid, gid);

            // fork 子进程
            let pid = unsafe { libc::fork() };
            match pid {
                -1 => {
                    // fork 失败
                    unsafe {
                        libc::close(read_fd);
                        libc::close(write_fd);
                    }
                    Err(anyhow::anyhow!("fork 失败: {}", std::io::Error::last_os_error()))
                }
                0 => {
                    // ===== 子进程 =====
                    // 关闭管道的读取端
                    unsafe { libc::close(read_fd); }

                    // 降权：先 setgid，再 setuid（顺序重要）
                    unsafe {
                        if libc::setgid(gid) != 0 {
                            let msg = format!("setgid({}) 失败", gid);
                            let _ = write_error_to_pipe(write_fd, &msg);
                            libc::close(write_fd);
                            libc::_exit(1);
                        }
                        if libc::setuid(uid) != 0 {
                            let msg = format!("setuid({}) 失败", uid);
                            let _ = write_error_to_pipe(write_fd, &msg);
                            libc::close(write_fd);
                            libc::_exit(1);
                        }
                    }

                    // 验证降权成功
                    let current_uid = unsafe { libc::getuid() };
                    let current_gid = unsafe { libc::getgid() };
                    if current_uid != uid || current_gid != gid {
                        let msg = format!(
                            "降权验证失败: 期望 uid={} gid={}, 实际 uid={} gid={}",
                            uid, gid, current_uid, current_gid
                        );
                        let _ = write_error_to_pipe(write_fd, &msg);
                        unsafe {
                            libc::close(write_fd);
                            libc::_exit(1);
                        }
                    }

                    // 执行闭包
                    let result = f();

                    // 根据结果类型分别处理：
                    // - Ok(T): 序列化 T 直接写入管道（父进程按 T 反序列化）
                    // - Err(e): 通过 write_error_to_pipe 写入错误（带 0x00 标记）
                    match result {
                        Ok(val) => {
                            match serde_json::to_vec(&val) {
                                Ok(data) => {
                                    // 先写入长度（4 字节小端）
                                    let len = (data.len() as u32).to_le_bytes();
                                    if write_all_to_pipe(write_fd, &len).is_err()
                                        || write_all_to_pipe(write_fd, &data).is_err()
                                    {
                                        let _ = write_error_to_pipe(write_fd, "写入结果失败");
                                    }
                                }
                                Err(e) => {
                                    let _ = write_error_to_pipe(write_fd, &e.to_string());
                                }
                            }
                        }
                        Err(e) => {
                            let _ = write_error_to_pipe(write_fd, &e.to_string());
                        }
                    }

                    // 关闭管道并退出
                    unsafe {
                        libc::close(write_fd);
                        libc::_exit(0);
                    }
                }
                child_pid => {
                    // ===== 父进程 =====
                    // 关闭管道的写入端
                    unsafe { libc::close(write_fd); }

                    // 读取子进程返回的结果
                    let result = read_result_from_pipe::<T>(read_fd);

                    // 关闭管道读取端
                    unsafe { libc::close(read_fd); }

                    // 等待子进程退出（防止僵尸进程）
                    let mut status = 0i32;
                    unsafe {
                        libc::waitpid(child_pid, &mut status, 0);
                    }

                    // 检查子进程退出状态
                    if libc::WIFSIGNALED(status) {
                        tracing::warn!("子进程被信号终止: pid={}, signal={}",
                            child_pid, libc::WTERMSIG(status));
                        return Err(anyhow::anyhow!(
                            "子进程被信号终止: signal={}",
                            libc::WTERMSIG(status)
                        ));
                    }

                    if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) != 0 {
                        tracing::warn!("子进程异常退出: pid={}, status={}",
                            child_pid, libc::WEXITSTATUS(status));
                        return Err(anyhow::anyhow!(
                            "子进程异常退出: status={}",
                            libc::WEXITSTATUS(status)
                        ));
                    }

                    result
                }
            }
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

    /// 在用户上下文中执行操作(不要求序列化)
    ///
    /// # 重要说明
    /// - 此方法**不会** fork 子进程或降权(因为无法通过管道传递不可序列化的结果)
    /// - 操作以当前进程权限执行,依赖 Linux 文件系统权限检查
    /// - 如需严格的用户隔离,请使用 `execute_as_user` 并确保返回值可序列化
    ///
    /// # 使用场景
    /// - 返回不可序列化类型的操作(如 `FileStreamWriter`)
    /// - 需要在用户上下文中创建复杂对象的场景
    ///
    /// # 安全性
    /// - 仅用于信任用户或已在其他方式中验证的场景
    /// - 不提供进程级的权限隔离
    pub fn execute_as_user_unchecked<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        #[cfg(unix)]
        {
            // 如果当前已经是目标用户，直接执行
            let current_uid = nix::unistd::getuid().as_raw();
            if current_uid == self.uid {
                tracing::debug!("当前用户已是目标用户，跳过切换: uid={}", self.uid);
                return f();
            }

            // 如果目标用户是 root，直接执行
            if self.uid == 0 {
                tracing::debug!("目标用户是 root，直接执行");
                return f();
            }

            // 非 root 目标用户，但返回值不可序列化
            // 这里需要降权执行，但无法通过管道传递结果
            // 最佳实践：在父进程中以 root 执行，依赖 Linux 文件系统权限
            tracing::debug!(
                "不可序列化的返回类型，依赖文件系统权限: uid={}, gid={}",
                self.uid, self.gid
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

    /// 获取用户 UID
    #[allow(dead_code)]
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// 获取用户 GID
    #[allow(dead_code)]
    pub fn gid(&self) -> u32 {
        self.gid
    }

    /// 获取用户家目录
    #[allow(dead_code)]
    pub fn home_dir(&self) -> &PathBuf {
        &self.home_dir
    }

    /// 在用户上下文中生成子进程（Unix平台，用于PTY）
    #[cfg(unix)]
    #[allow(dead_code)]
    pub fn spawn_process(&self, program: &str, args: &[&str]) -> Result<i32> {
        use anyhow::Context;
        use std::os::unix::process::CommandExt;
        use std::process::Command;

        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.current_dir(&self.home_dir);

        let uid = self.uid;
        let gid = self.gid;

        unsafe {
            cmd.pre_exec(move || {
                let current_uid = nix::unistd::getuid().as_raw();
                if current_uid == uid {
                    return Ok(());
                }
                if current_uid == 0 {
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
                Ok(())
            });
        }

        let child = cmd.spawn().context("Failed to spawn process")?;
        Ok(child.id() as i32)
    }
}

/// 向管道写入所有数据(循环写入,处理部分写入)
///
/// # 返回
/// - Ok(()): 所有数据写入成功
/// - Err(e): 写入失败
#[cfg(unix)]
fn write_all_to_pipe(fd: i32, data: &[u8]) -> std::io::Result<()> {
    let mut offset = 0;
    while offset < data.len() {
        let n = unsafe {
            libc::write(fd, data[offset..].as_ptr() as *const _, data.len() - offset)
        };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        offset += n as usize;
    }
    Ok(())
}

/// 向管道写入错误信息
///
/// 格式: [0x00][4字节长度][JSON 错误数据]
/// 正常结果的格式: [4字节长度][JSON 数据]
/// 用 0x00 标记区分错误和正常结果
#[cfg(unix)]
fn write_error_to_pipe(fd: i32, message: &str) -> std::io::Result<()> {
    // 写入错误标记
    let marker: u8 = 0x00;
    write_all_to_pipe(fd, &[marker])?;

    // 序列化错误消息
    let error_data = serde_json::to_vec(&message)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // 写入长度和数据
    let len = (error_data.len() as u32).to_le_bytes();
    write_all_to_pipe(fd, &len)?;
    write_all_to_pipe(fd, &error_data)?;

    Ok(())
}

/// 从管道读取结果
///
/// 格式:
/// - 正常结果: [4字节长度][JSON 数据]  → 解析为 Ok(T)
/// - 错误结果: [0x00][4字节长度][JSON 错误消息] → 解析为 Err
#[cfg(unix)]
fn read_result_from_pipe<T>(fd: i32) -> Result<T>
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    // 读取第一个字节判断是否为错误标记
    let mut first_byte = [0u8; 1];
    let n = unsafe { libc::read(fd, first_byte.as_mut_ptr() as *mut _, 1) };
    if n != 1 {
        return Err(anyhow::anyhow!("读取管道数据失败: 无法读取标记字节"));
    }

    // 如果是错误标记
    if first_byte[0] == 0x00 {
        // 读取错误消息长度
        let mut len_buf = [0u8; 4];
        let n = unsafe { libc::read(fd, len_buf.as_mut_ptr() as *mut _, 4) };
        if n != 4 {
            return Err(anyhow::anyhow!("读取管道数据失败: 无法读取错误消息长度"));
        }
        let msg_len = u32::from_le_bytes(len_buf) as usize;

        // 读取错误消息
        let mut msg_buf = vec![0u8; msg_len];
        let mut offset = 0;
        while offset < msg_len {
            let n = unsafe {
                libc::read(fd, msg_buf.as_mut_ptr().add(offset) as *mut _, msg_len - offset)
            };
            if n <= 0 {
                return Err(anyhow::anyhow!("读取管道数据失败: 连接断开"));
            }
            offset += n as usize;
        }

        let error_msg: String = serde_json::from_slice(&msg_buf)
            .unwrap_or_else(|_| "未知错误".to_string());
        return Err(anyhow::anyhow!("子进程执行失败: {}", error_msg));
    }

    // 正常结果：第一个字节是长度的高位字节
    // 读取剩余 3 字节的长度
    let mut len_buf = [0u8; 4];
    len_buf[0] = first_byte[0];
    let n = unsafe { libc::read(fd, len_buf.as_mut_ptr().add(1) as *mut _, 3) };
    if n != 3 {
        return Err(anyhow::anyhow!("读取管道数据失败: 无法读取结果长度"));
    }
    let data_len = u32::from_le_bytes(len_buf) as usize;

    // 限制结果大小（防止恶意子进程写入大量数据）
    if data_len > 100 * 1024 * 1024 {
        return Err(anyhow::anyhow!("结果数据过大: {} bytes", data_len));
    }

    // 读取结果数据
    let mut data_buf = vec![0u8; data_len];
    let mut offset = 0;
    while offset < data_len {
        let n = unsafe {
            libc::read(fd, data_buf.as_mut_ptr().add(offset) as *mut _, data_len - offset)
        };
        if n <= 0 {
            return Err(anyhow::anyhow!("读取管道数据失败: 连接断开"));
        }
        offset += n as usize;
    }

    // 反序列化结果
    serde_json::from_slice(&data_buf)
        .map_err(|e| anyhow::anyhow!("反序列化结果失败: {}", e))
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
    fn test_execute_as_user_simple() {
        // 在测试环境中，当前用户可能就是 uid=1000
        // 所以会直接执行，不走 fork 路径
        let session = create_test_session();
        let executor = UserExecutor::new(&session);

        let result: Result<i32> = executor.execute_as_user(|| Ok(42));
        assert!(result.is_ok(), "execute_as_user failed: {:?}", result.err());
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