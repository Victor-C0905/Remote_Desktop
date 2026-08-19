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
#[derive(Clone)]
pub struct UserExecutor {
    /// 用户ID (UID)
    uid: u32,
    /// 组ID (GID)
    gid: u32,
    /// 用户家目录
    home_dir: PathBuf,
}

impl UserExecutor {
    /// 从 UserSession 创建新的执行器
    pub fn new(session: &UserSession) -> Self {
        Self {
            uid: session.uid,
            gid: session.gid,
            home_dir: session.home_dir.clone(),
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

    /// 在隔离的子进程中处理文件写入,父进程经 pipe 写 chunk,子进程 read pipe → write file
    ///
    /// # 隔离链
    /// - fork 子进程
    /// - 子进程 setuid/setgid + UserNamespace::new(uid,gid).create_and_switch() (Linux)
    /// - 子进程内 open + write 文件(同步 IO)
    /// - 经 os_pipe 与父进程交换 chunk
    ///
    /// # 返回
    /// - Ok((pipe_writer, child_pid)): 父进程持 pipe_writer 写 chunk,完成后调 wait_isolated_child
    /// - Err: fork 或 pipe 失败
    #[cfg(unix)]
    pub fn spawn_isolated_writer(&self, temp_path: &str, final_path: &str, _file_size: u64) -> Result<(os_pipe::PipeWriter, i32)> {
        use std::io::{Read as StdRead, Write as StdWrite};

        // 1. 创建 os_pipe
        let (mut reader, writer) = os_pipe::pipe()
            .map_err(|e| anyhow::anyhow!("创建管道失败: {}", e))?;

        let uid = self.uid;
        let gid = self.gid;
        let temp_path = temp_path.to_string();
        let final_path = final_path.to_string();

        tracing::debug!("fork 子进程执行隔离写入: uid={}, gid={}", uid, gid);

        // 2. fork 子进程
        let pid = unsafe { libc::fork() };
        match pid {
            -1 => {
                Err(anyhow::anyhow!("fork 失败: {}", std::io::Error::last_os_error()))
            }
            0 => {
                // ===== 子进程 =====
                // 关闭 pipe writer 端（子进程只读 pipe）
                drop(writer);

                // 降权：先 setgid，再 setuid（顺序重要）
                unsafe {
                    if libc::setgid(gid) != 0 {
                        libc::_exit(1);
                    }
                    if libc::setuid(uid) != 0 {
                        libc::_exit(1);
                    }
                }

                // 验证降权成功
                let current_uid = unsafe { libc::getuid() };
                let current_gid = unsafe { libc::getgid() };
                if current_uid != uid || current_gid != gid {
                    unsafe { libc::_exit(1); }
                }

                // Linux: 创建 User Namespace（防御纵深，失败则继续以 setuid 隔离）
                #[cfg(target_os = "linux")]
                {
                    if uid != 0 {
                        if let Err(e) = super::namespace::UserNamespace::new(uid, gid).create_and_switch() {
                            tracing::warn!("User Namespace 创建失败,继续以 setuid 隔离: {}", e);
                        }
                    }
                }

                // 打开临时文件（O_NOFOLLOW 防符号链接劫持，0o600 限属主读写）
                let file = {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        match std::fs::OpenOptions::new()
                            .create(true)
                            .write(true)
                            .truncate(true)
                            .mode(0o600)
                            .custom_flags(libc::O_NOFOLLOW)
                            .open(&temp_path)
                        {
                            Ok(f) => f,
                            Err(_) => {
                                unsafe { libc::_exit(2); }
                            }
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        match std::fs::File::create(&temp_path) {
                            Ok(f) => f,
                            Err(_) => {
                                unsafe { libc::_exit(2); }
                            }
                        }
                    }
                };
                let mut file_writer = std::io::BufWriter::with_capacity(256 * 1024, file);

                // loop: read pipe → write file
                let mut buf = [0u8; 256 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,  // EOF（父进程关闭了 pipe writer）
                        Ok(n) => {
                            if file_writer.write_all(&buf[..n]).is_err() {
                                unsafe { libc::_exit(3); }
                            }
                        }
                        Err(_) => {
                            unsafe { libc::_exit(4); }
                        }
                    }
                }

                // flush + sync + rename
                if file_writer.flush().is_err() {
                    unsafe { libc::_exit(5); }
                }
                if file_writer.get_ref().sync_all().is_err() {
                    unsafe { libc::_exit(6); }
                }
                // 释放文件句柄后再 rename
                drop(file_writer);
                if std::fs::rename(&temp_path, &final_path).is_err() {
                    unsafe { libc::_exit(7); }
                }

                unsafe { libc::_exit(0); }
            }
            child_pid => {
                // ===== 父进程 =====
                // 关闭 pipe reader 端（父进程只写 pipe）
                drop(reader);
                Ok((writer, child_pid))
            }
        }
    }

    /// 在隔离的子进程中处理文件读取,子进程 read file → write pipe,父进程经 pipe 读 chunk
    ///
    /// # 隔离链
    /// - fork 子进程
    /// - 子进程 setuid/setgid + UserNamespace::new(uid,gid).create_and_switch() (Linux)
    /// - 子进程内 open + read 文件(同步 IO),写入 pipe
    /// - 经 os_pipe 与父进程交换 chunk
    ///
    /// # 返回
    /// - Ok((pipe_reader, child_pid)): 父进程持 pipe_reader 读 chunk
    /// - Err: fork 或 pipe 失败
    #[cfg(unix)]
    pub fn spawn_isolated_reader(&self, path: &str) -> Result<(os_pipe::PipeReader, i32)> {
        use std::io::{Read as StdRead, Write as StdWrite};

        // 1. 创建 os_pipe
        let (reader, mut writer) = os_pipe::pipe()
            .map_err(|e| anyhow::anyhow!("创建管道失败: {}", e))?;

        let uid = self.uid;
        let gid = self.gid;
        let path = path.to_string();

        tracing::debug!("fork 子进程执行隔离读取: uid={}, gid={}", uid, gid);

        // 2. fork 子进程
        let pid = unsafe { libc::fork() };
        match pid {
            -1 => {
                Err(anyhow::anyhow!("fork 失败: {}", std::io::Error::last_os_error()))
            }
            0 => {
                // ===== 子进程 =====
                // 关闭 pipe reader 端（子进程只写 pipe）
                drop(reader);

                // 降权：先 setgid，再 setuid（顺序重要）
                unsafe {
                    if libc::setgid(gid) != 0 {
                        libc::_exit(1);
                    }
                    if libc::setuid(uid) != 0 {
                        libc::_exit(1);
                    }
                }

                // 验证降权成功
                let current_uid = unsafe { libc::getuid() };
                let current_gid = unsafe { libc::getgid() };
                if current_uid != uid || current_gid != gid {
                    unsafe { libc::_exit(1); }
                }

                // Linux: 创建 User Namespace（防御纵深，失败则继续以 setuid 隔离）
                #[cfg(target_os = "linux")]
                {
                    if uid != 0 {
                        if let Err(e) = super::namespace::UserNamespace::new(uid, gid).create_and_switch() {
                            tracing::warn!("User Namespace 创建失败,继续以 setuid 隔离: {}", e);
                        }
                    }
                }

                // 打开文件
                let file = match std::fs::File::open(&path) {
                    Ok(f) => f,
                    Err(_) => {
                        unsafe { libc::_exit(2); }
                    }
                };
                let mut file_reader = std::io::BufReader::with_capacity(256 * 1024, file);

                // loop: read file → write pipe
                let mut buf = [0u8; 256 * 1024];
                loop {
                    match file_reader.read(&mut buf) {
                        Ok(0) => break,  // 文件读完
                        Ok(n) => {
                            if writer.write_all(&buf[..n]).is_err() {
                                unsafe { libc::_exit(3); }
                            }
                        }
                        Err(_) => {
                            unsafe { libc::_exit(4); }
                        }
                    }
                }

                // 关闭 pipe writer 让父进程读到 EOF
                drop(writer);
                unsafe { libc::_exit(0); }
            }
            child_pid => {
                // ===== 父进程 =====
                // 关闭 pipe writer 端（父进程只读 pipe）
                drop(writer);
                Ok((reader, child_pid))
            }
        }
    }

    /// 在隔离的子进程中处理文件段写入（多流并行，方案 A: 独立段+合并）
    ///
    /// 与 `spawn_isolated_writer` 的区别：
    /// - 仅接受 `part_path`（段文件路径），无 `final_path`，**不执行 rename**
    /// - 子进程 flush + sync 后直接 _exit(0)，段文件留待后续 `spawn_isolated_merger` 合并
    ///
    /// # 隔离链
    /// - fork 子进程
    /// - 子进程 setuid/setgid + UserNamespace::new(uid,gid).create_and_switch() (Linux)
    /// - 子进程内 open part_path + write（同步 IO）
    /// - 经 os_pipe 与父进程交换 chunk
    ///
    /// # 返回
    /// - Ok((pipe_writer, child_pid)): 父进程持 pipe_writer 写 chunk
    /// - Err: fork 或 pipe 失败
    ///
    /// # 退出码（与 writer 一致，去掉 7=rename）
    /// - 1: 降权失败 / 2: 打开段文件失败 / 3: 写入段文件失败 / 4: 读取 pipe 失败
    /// - 5: flush 失败 / 6: sync_all 失败
    #[cfg(unix)]
    pub fn spawn_isolated_writer_part(&self, part_path: &str) -> Result<(os_pipe::PipeWriter, i32)> {
        use std::io::{Read as StdRead, Write as StdWrite};

        // 1. 创建 os_pipe
        let (mut reader, writer) = os_pipe::pipe()
            .map_err(|e| anyhow::anyhow!("创建管道失败: {}", e))?;

        let uid = self.uid;
        let gid = self.gid;
        let part_path = part_path.to_string();

        tracing::debug!(
            "fork 子进程执行隔离段写入(多流): uid={}, gid={}, part_path={}",
            uid, gid, part_path
        );

        // 2. fork 子进程
        let pid = unsafe { libc::fork() };
        match pid {
            -1 => {
                Err(anyhow::anyhow!("fork 失败: {}", std::io::Error::last_os_error()))
            }
            0 => {
                // ===== 子进程 =====
                // 关闭 pipe writer 端（子进程只读 pipe）
                drop(writer);

                // 降权：先 setgid，再 setuid（顺序重要）
                unsafe {
                    if libc::setgid(gid) != 0 {
                        libc::_exit(1);
                    }
                    if libc::setuid(uid) != 0 {
                        libc::_exit(1);
                    }
                }

                // 验证降权成功
                let current_uid = unsafe { libc::getuid() };
                let current_gid = unsafe { libc::getgid() };
                if current_uid != uid || current_gid != gid {
                    unsafe { libc::_exit(1); }
                }

                // Linux: 创建 User Namespace（防御纵深，失败则继续以 setuid 隔离）
                #[cfg(target_os = "linux")]
                {
                    if uid != 0 {
                        if let Err(e) = super::namespace::UserNamespace::new(uid, gid).create_and_switch() {
                            tracing::warn!("User Namespace 创建失败,继续以 setuid 隔离: {}", e);
                        }
                    }
                }

                // 打开段文件（O_NOFOLLOW 防符号链接劫持，0o600 限属主读写）
                let file = {
                    use std::os::unix::fs::OpenOptionsExt;
                    match std::fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .mode(0o600)
                        .custom_flags(libc::O_NOFOLLOW)
                        .open(&part_path)
                    {
                        Ok(f) => f,
                        Err(_) => {
                            unsafe { libc::_exit(2); }
                        }
                    }
                };
                let mut file_writer = std::io::BufWriter::with_capacity(256 * 1024, file);

                // loop: read pipe → write part file
                let mut buf = [0u8; 256 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,  // EOF（父进程关闭了 pipe writer）
                        Ok(n) => {
                            if file_writer.write_all(&buf[..n]).is_err() {
                                unsafe { libc::_exit(3); }
                            }
                        }
                        Err(_) => {
                            unsafe { libc::_exit(4); }
                        }
                    }
                }

                // flush + sync（不 rename，段文件留待 merger 合并）
                if file_writer.flush().is_err() {
                    unsafe { libc::_exit(5); }
                }
                if file_writer.get_ref().sync_all().is_err() {
                    unsafe { libc::_exit(6); }
                }

                unsafe { libc::_exit(0); }
            }
            child_pid => {
                // ===== 父进程 =====
                // 关闭 pipe reader 端（父进程只写 pipe）
                drop(reader);
                Ok((writer, child_pid))
            }
        }
    }

    /// 在隔离的子进程中合并段文件为最终文件（多流并行合并阶段）
    ///
    /// # 流程
    /// 1. fork 子进程，setuid/setgid + UserNamespace
    /// 2. 子进程 open final_path（create/truncate，O_NOFOLLOW，0o600）
    /// 3. 按 part_paths 顺序逐个 open 段文件 → read → write 到 final_path
    /// 4. fsync final_path
    /// 5. 删除所有段文件（清理）
    /// 6. _exit(0)
    ///
    /// # 返回
    /// - Ok(child_pid): 父进程调 wait_isolated_child 等待合并完成
    /// - Err: fork 失败
    ///
    /// # 退出码（独立段，避免与 writer/reader 混淆）
    /// - 1: 降权失败 / 10: 打开最终文件失败 / 11: 打开段文件失败
    /// - 12: 读取段文件失败 / 13: 写入最终文件失败 / 14: sync_all 失败 / 15: 删除段文件失败
    #[cfg(unix)]
    pub fn spawn_isolated_merger(&self, part_paths: Vec<String>, final_path: &str) -> Result<i32> {
        use std::io::{Read as StdRead, Write as StdWrite};

        let uid = self.uid;
        let gid = self.gid;
        let final_path = final_path.to_string();

        tracing::debug!(
            "fork 子进程执行隔离段合并(多流): uid={}, gid={}, parts={}, final={}",
            uid, gid, part_paths.len(), final_path
        );

        let pid = unsafe { libc::fork() };
        match pid {
            -1 => {
                Err(anyhow::anyhow!("fork 失败: {}", std::io::Error::last_os_error()))
            }
            0 => {
                // ===== 子进程 =====
                // 降权：先 setgid，再 setuid（顺序重要）
                unsafe {
                    if libc::setgid(gid) != 0 {
                        libc::_exit(1);
                    }
                    if libc::setuid(uid) != 0 {
                        libc::_exit(1);
                    }
                }

                // 验证降权成功
                let current_uid = unsafe { libc::getuid() };
                let current_gid = unsafe { libc::getgid() };
                if current_uid != uid || current_gid != gid {
                    unsafe { libc::_exit(1); }
                }

                // Linux: 创建 User Namespace（防御纵深）
                #[cfg(target_os = "linux")]
                {
                    if uid != 0 {
                        if let Err(e) = super::namespace::UserNamespace::new(uid, gid).create_and_switch() {
                            tracing::warn!("User Namespace 创建失败,继续以 setuid 隔离: {}", e);
                        }
                    }
                }

                // 打开最终文件（create/truncate，O_NOFOLLOW，0o600）
                let final_file = {
                    use std::os::unix::fs::OpenOptionsExt;
                    match std::fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .mode(0o600)
                        .custom_flags(libc::O_NOFOLLOW)
                        .open(&final_path)
                    {
                        Ok(f) => f,
                        Err(_) => {
                            unsafe { libc::_exit(10); }
                        }
                    }
                };
                let mut final_writer = std::io::BufWriter::with_capacity(256 * 1024, final_file);

                // 按 part_paths 顺序合并段文件
                let mut buf = [0u8; 256 * 1024];
                for part_path in &part_paths {
                    let mut part_file = match std::fs::File::open(part_path) {
                        Ok(f) => f,
                        Err(_) => {
                            unsafe { libc::_exit(11); }
                        }
                    };
                    loop {
                        match part_file.read(&mut buf) {
                            Ok(0) => break,  // 段文件读完
                            Ok(n) => {
                                if final_writer.write_all(&buf[..n]).is_err() {
                                    unsafe { libc::_exit(13); }
                                }
                            }
                            Err(_) => {
                                unsafe { libc::_exit(12); }
                            }
                        }
                    }
                    // 显式关闭段文件（drop 即可，但此处清晰）
                    drop(part_file);
                }

                // flush + sync 最终文件
                if final_writer.flush().is_err() {
                    unsafe { libc::_exit(13); }
                }
                if final_writer.get_ref().sync_all().is_err() {
                    unsafe { libc::_exit(14); }
                }
                drop(final_writer);

                // 删除所有段文件（清理）
                for part_path in &part_paths {
                    if std::fs::remove_file(part_path).is_err() {
                        // 段文件删除失败不致命（最终文件已完整），记录但继续
                        tracing::warn!("[merger] 删除段文件失败: {}", part_path);
                    }
                }
                // 即使部分段文件删除失败也视为合并成功（最终文件已落盘）
                unsafe { libc::_exit(0); }
            }
            child_pid => {
                // ===== 父进程 =====
                Ok(child_pid)
            }
        }
    }

    /// 等待隔离子进程结束,翻译退出码为描述性错误
    ///
    /// # 返回
    /// - Ok(()): 子进程正常退出(exit code 0)
    /// - Err: 子进程被信号终止、非零退出(翻译为描述性错误)或已被回收(ECHILD)
    ///
    /// # 退出码翻译
    /// 与 spawn_isolated_writer/writer_part/reader/merger 中的 _exit(N) 对应:
    /// - 1: 降权失败(setgid/setuid)
    /// - 2: 打开文件失败(writer/writer_part/reader)
    /// - 3: 写入失败(writer=文件, reader=pipe)
    /// - 4: 读取失败(writer=pipe, reader=文件)
    /// - 5: flush 失败(writer/writer_part)
    /// - 6: sync_all 失败(writer/writer_part)
    /// - 7: rename 失败(writer)
    /// - 10: merger 打开最终文件失败
    /// - 11: merger 打开段文件失败
    /// - 12: merger 读取段文件失败
    /// - 13: merger 写入最终文件失败
    /// - 14: merger sync_all 失败
    /// - 15: merger 删除段文件失败(不致命,但记录)
    #[cfg(unix)]
    pub fn wait_isolated_child(&self, child_pid: i32) -> Result<()> {
        let mut status = 0i32;
        let ret = unsafe { libc::waitpid(child_pid, &mut status, 0) };
        if ret == -1 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ECHILD) {
                return Err(anyhow::anyhow!(
                    "隔离子进程(pid={})已被回收,无法确认退出状态(可能数据丢失)",
                    child_pid
                ));
            }
            return Err(anyhow::anyhow!("waitpid 失败: pid={}, err={}", child_pid, err));
        }

        // 检查子进程退出状态
        if libc::WIFSIGNALED(status) {
            let sig = libc::WTERMSIG(status);
            tracing::warn!("隔离子进程被信号终止: pid={}, signal={}",
                child_pid, sig);
            return Err(anyhow::anyhow!(
                "隔离子进程被信号终止: pid={}, signal={}", child_pid, sig
            ));
        }

        if libc::WIFEXITED(status) {
            let code = libc::WEXITSTATUS(status);
            if code != 0 {
                // 翻译子进程 exit code(与 spawn_isolated_* 中的 _exit(N) 对应)
                let msg = match code {
                    1 => "降权失败(setgid/setuid)",
                    2 => "打开文件失败",
                    3 => "写入失败(文件/pipe)",
                    4 => "读取失败(pipe/文件)",
                    5 => "flush 失败",
                    6 => "sync_all 失败",
                    7 => "rename 失败",
                    10 => "merger 打开最终文件失败",
                    11 => "merger 打开段文件失败",
                    12 => "merger 读取段文件失败",
                    13 => "merger 写入最终文件失败",
                    14 => "merger sync_all 失败",
                    15 => "merger 删除段文件失败",
                    _ => "未知错误",
                };
                tracing::warn!("隔离子进程异常退出: pid={}, code={}, 原因={}",
                    child_pid, code, msg);
                return Err(anyhow::anyhow!(
                    "隔离子进程异常退出: pid={}, code={}, 原因={}",
                    child_pid, code, msg
                ));
            }
        }

        Ok(())
    }

    /// 在隔离的子进程中处理文件写入（非 Unix 平台占位）
    #[cfg(not(unix))]
    pub fn spawn_isolated_writer(&self, _temp_path: &str, _final_path: &str, _file_size: u64) -> Result<(os_pipe::PipeWriter, i32)> {
        anyhow::bail!("用户隔离写入仅支持 Unix 平台")
    }

    /// 在隔离的子进程中处理文件读取（非 Unix 平台占位）
    #[cfg(not(unix))]
    pub fn spawn_isolated_reader(&self, _path: &str) -> Result<(os_pipe::PipeReader, i32)> {
        anyhow::bail!("用户隔离读取仅支持 Unix 平台")
    }

    /// 在隔离的子进程中处理文件段写入（非 Unix 平台占位）
    #[cfg(not(unix))]
    pub fn spawn_isolated_writer_part(&self, _part_path: &str) -> Result<(os_pipe::PipeWriter, i32)> {
        anyhow::bail!("用户隔离段写入仅支持 Unix 平台")
    }

    /// 在隔离的子进程中合并段文件（非 Unix 平台占位）
    #[cfg(not(unix))]
    pub fn spawn_isolated_merger(&self, _part_paths: Vec<String>, _final_path: &str) -> Result<i32> {
        anyhow::bail!("用户隔离段合并仅支持 Unix 平台")
    }

    /// 等待隔离子进程结束（非 Unix 平台占位）
    #[cfg(not(unix))]
    pub fn wait_isolated_child(&self, _child_pid: i32) -> Result<()> {
        anyhow::bail!("用户隔离仅支持 Unix 平台")
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

    /// 测试 spawn_isolated_writer 往返:写入数据 → 等待子进程 → 验证文件内容
    #[cfg(unix)]
    #[test]
    fn test_spawn_isolated_writer_round_trip() {
        use std::io::{Read, Write};
        use tempfile::tempdir;

        let tmp = tempdir().expect("创建 tempdir 失败");
        let final_path = tmp.path().join("test_upload.bin");
        let temp_path = format!("{}.tmp", final_path.to_string_lossy());

        // 创建测试 session(目标 uid = 当前 uid,避免降权复杂度)
        let current_uid = nix::unistd::getuid().as_raw();
        let current_gid = nix::unistd::getgid().as_raw();
        let identity = crate::auth::UserIdentity::new(
            "testuser".to_string(),
            current_uid,
            current_gid,
            "/tmp".to_string(),
            "/bin/bash".to_string(),
        );
        let session = crate::auth::UserSession::new(identity);
        let executor = UserExecutor::new(&session);

        let data = vec![0xAA; 1024 * 100]; // 100KB
        let (mut pipe_writer, child_pid) = executor
            .spawn_isolated_writer(&temp_path, &final_path.to_string_lossy(), data.len() as u64)
            .expect("spawn_isolated_writer 失败");

        pipe_writer.write_all(&data).expect("写入 pipe 失败");
        drop(pipe_writer); // EOF signal

        executor.wait_isolated_child(child_pid).expect("wait 失败");

        // 验证文件内容
        let mut file = std::fs::File::open(&final_path).expect("打开 final 失败");
        let mut read_data = Vec::new();
        file.read_to_end(&mut read_data).expect("读取失败");
        assert_eq!(read_data, data);
    }

    /// 测试 spawn_isolated_reader 往返:写入测试文件 → 读取 pipe → 验证内容
    #[cfg(unix)]
    #[test]
    fn test_spawn_isolated_reader_round_trip() {
        use std::io::Read;
        use tempfile::tempdir;

        let tmp = tempdir().expect("创建 tempdir 失败");
        let path = tmp.path().join("test_download.bin");
        let data = vec![0xBB; 1024 * 100];
        std::fs::write(&path, &data).expect("写入测试文件失败");

        let current_uid = nix::unistd::getuid().as_raw();
        let current_gid = nix::unistd::getgid().as_raw();
        let identity = crate::auth::UserIdentity::new(
            "testuser".to_string(),
            current_uid,
            current_gid,
            "/tmp".to_string(),
            "/bin/bash".to_string(),
        );
        let session = crate::auth::UserSession::new(identity);
        let executor = UserExecutor::new(&session);

        let (mut pipe_reader, child_pid) = executor
            .spawn_isolated_reader(&path.to_string_lossy())
            .expect("spawn_isolated_reader 失败");

        let mut read_data = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match pipe_reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => read_data.extend_from_slice(&buf[..n]),
                Err(e) => panic!("读取 pipe 失败: {}", e),
            }
        }
        drop(pipe_reader);
        executor.wait_isolated_child(child_pid).expect("wait 失败");
        assert_eq!(read_data, data);
    }

    /// 测试 wait_isolated_child 处理 ECHILD:子进程已被回收时应返回错误
    #[cfg(unix)]
    #[test]
    fn test_wait_isolated_child_echild() {
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let final_path = tmp.path().join("final.bin");
        let temp_path = format!("{}.tmp", final_path.to_string_lossy());

        let current_uid = nix::unistd::getuid().as_raw();
        let current_gid = nix::unistd::getgid().as_raw();
        let identity = crate::auth::UserIdentity::new(
            "testuser".to_string(),
            current_uid,
            current_gid,
            "/tmp".to_string(),
            "/bin/bash".to_string(),
        );
        let session = crate::auth::UserSession::new(identity);
        let executor = UserExecutor::new(&session);

        // spawn 一个会立即 EOF 的 writer(不写数据,直接 drop pipe_writer)
        let (_pipe_writer, child_pid) = executor
            .spawn_isolated_writer(&temp_path, &final_path.to_string_lossy(), 0)
            .unwrap();
        drop(_pipe_writer); // child reads EOF, exits 0

        // 主动 waitpid 抢先回收(阻塞等待子进程退出)
        let mut status = 0i32;
        unsafe {
            libc::waitpid(child_pid, &mut status, 0);
        }

        // 现在调用 wait_isolated_child 应返回 ECHILD Err
        let result = executor.wait_isolated_child(child_pid);
        assert!(result.is_err(), "ECHILD 应返回 Err");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("已被回收"),
            "错误信息应提及已被回收,实际: {}", err_msg
        );
    }
}