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

// ============================================================================
// ForkGuard：串行化所有 spawn_isolated_* 调用
// ============================================================================
//
// 在 posix_spawn 方案下，子进程是独立的新程序，不继承父进程 runtime 状态，
// ForkGuard 仅作为防止并发 spawn 带来 fd 资源竞争的额外保险。
//
// 注意：std::sync::Mutex 在所有平台都可用，故此处不做 cfg 门控。
use std::sync::Mutex;

/// ForkGuard 全局锁：串行化所有 spawn_isolated_* 调用
///
/// 在 posix_spawn 方案下，子进程是独立新程序不继承 runtime 状态，
/// ForkGuard 仅作为防止并发 spawn 带来 fd 资源竞争的额外保险
static FORK_GUARD: Mutex<()> = Mutex::new(());

/// 获取 ForkGuard 锁（短暂持有，spawn 完成即释放）
fn with_fork_guard() -> std::sync::MutexGuard<'static, ()> {
    FORK_GUARD.lock().unwrap_or_else(|e| e.into_inner())
}

/// 隔离/辅助子进程的可执行文件路径
///
/// 生产环境为当前进程（quireld 自身）。`cargo test` 的 current_exe 是测试
/// harness，无法解析 `--isolated-*` 参数；集成测试通过 `QUIRELD_EXECUTABLE`
/// 指向真实的 quireld 二进制。
#[cfg(unix)]
fn isolated_executable() -> Result<PathBuf> {
    use anyhow::Context;
    match std::env::var_os("QUIRELD_EXECUTABLE") {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        _ => std::env::current_exe().context("Failed to get current exe path"),
    }
}

/// 解析 /etc/group 内容，返回 username 所在组的 gid 列表。
///
/// 行格式 `group_name:password:gid:members`，members 逗号分隔、精确匹配
/// 用户名；字段不足或 gid 非数字的畸形行跳过。调用方需自行并入主 gid。
/// （仅 fork 降权路径使用，该路径 cfg(unix)；非 unix 平台无死代码告警）
#[cfg_attr(not(unix), allow(dead_code))]
fn parse_supplementary_group_ids(content: &str, username: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    for line in content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 4 {
            continue;
        }
        let gid = match fields[2].trim().parse::<u32>() {
            Ok(g) => g,
            Err(_) => continue,
        };
        if fields[3].split(',').any(|m| m.trim() == username) {
            ids.push(gid);
        }
    }
    ids
}

/// 解析目标用户的完整组列表（主 gid + /etc/group 附加组），供全部降权路径共用。
///
/// 读取失败按空表处理（仅保留主 gid，拒绝默认继承），fail-closed。
/// 调用方必须在进程仍为 root 时调用 libc::setgroups 显式替换附加组
/// （setuid 后无权再改；仅 setgid/setuid 不清除附加组——2026-11-20 审计 A-01）。
/// （fork/隔离子命令降权路径使用，均 cfg(unix)；非 unix 平台无死代码告警）
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn resolve_supplementary_groups(username: &str, primary_gid: u32) -> Vec<u32> {
    let supplementary = std::fs::read_to_string("/etc/group")
        .map(|content| parse_supplementary_group_ids(&content, username))
        .unwrap_or_else(|e| {
            tracing::warn!("读取 /etc/group 失败，降权进程仅保留主 gid: {}", e);
            Vec::new()
        });
    combine_group_ids(primary_gid, supplementary)
}

/// 并入主 gid 并去重（setgroups 只关心集合，去重防 /etc/group 畸形重复行）
#[cfg_attr(not(unix), allow(dead_code))]
fn combine_group_ids(primary_gid: u32, supplementary: Vec<u32>) -> Vec<u32> {
    let mut groups = vec![primary_gid];
    groups.extend(supplementary);
    groups.sort_unstable();
    groups.dedup();
    groups
}

/// 完整组列表的命令行形态（隔离子命令经 `--supp-gids` 由 Manager 侧解析后传入）
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn supplementary_gids_csv(username: &str, primary_gid: u32) -> String {
    resolve_supplementary_groups(username, primary_gid)
        .iter()
        .map(|g| g.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// 用户上下文执行器
#[derive(Clone)]
pub struct UserExecutor {
    /// 用户名（解析 /etc/group 附加组用，仅 unix fork 路径读取）
    #[cfg_attr(not(unix), allow(dead_code))]
    username: String,
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
            username: session.username.clone(),
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

            // 附加组：fork 前在父进程中解析目标用户的组列表。子进程 setgroups
            // 显式替换（而不是继承父进程 = root 的附加组）；读取失败按空表
            // 处理（仅保留主 gid，拒绝默认继承），fail-closed。
            let groups = resolve_supplementary_groups(&self.username, gid);

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

                    // 附加组：必须仍在 root 时 setgroups（setuid 后无权再改）
                    let gids: Vec<libc::gid_t> = groups;
                    if unsafe { libc::setgroups(gids.len(), gids.as_ptr()) } != 0 {
                        let msg = format!(
                            "setgroups({:?}) 失败: {}",
                            gids,
                            std::io::Error::last_os_error()
                        );
                        let _ = write_error_to_pipe(write_fd, &msg);
                        unsafe { libc::close(write_fd); }
                        unsafe { libc::_exit(1); }
                    }

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
                    // - Ok(T): 序列化 T 写成功帧（QROK 魔数 + 长度 + JSON，父进程按 T 反序列化）
                    // - Err(e): 通过 write_error_to_pipe 写错误帧（QERR 魔数）
                    match result {
                        Ok(val) => {
                            match serde_json::to_vec(&val) {
                                Ok(data) => {
                                    // 先写入魔数与长度（4 字节小端）
                                    let len = (data.len() as u32).to_le_bytes();
                                    if write_all_to_pipe(write_fd, &FRAME_MAGIC_OK).is_err()
                                        || write_all_to_pipe(write_fd, &len).is_err()
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

    /// 在阻塞线程池中执行用户上下文操作（async 友好包装）
    ///
    /// `execute_as_user` 全程同步阻塞（fork + waitpid + 管道阻塞读，最长可达
    /// 整个文件操作时长）。直接在 async 上下文调用会占住 tokio runtime worker
    /// 线程，并发文件操作会饿死线程池（心跳/PTY 转发全部排队）。本包装将其
    /// 移入 `spawn_blocking`，调用方以 `.await` 等待；Ok/Err 原样透传。
    pub async fn execute_as_user_blocking<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send + 'static,
        T: serde::Serialize + for<'de> serde::Deserialize<'de> + Send + 'static,
    {
        let executor = self.clone();
        tokio::task::spawn_blocking(move || executor.execute_as_user(f))
            .await
            .map_err(|e| anyhow::anyhow!("隔离执行线程异常: {}", e))?
    }

    /// 获取用户 UID
    #[allow(dead_code)]
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// 在隔离的子进程中处理文件写入,父进程经 pipe 写 chunk,子进程 read pipe → write file
    ///
    /// # 隔离链（posix_spawn 方案）
    /// - std::process::Command 自动用 posix_spawn 启动独立子进程（Rust 1.65+）
    /// - 子进程通过命令行参数接收 uid/gid/路径，自行 setgid/setuid 降权
    /// - 子进程内 open + write 文件(同步 IO)
    /// - 父进程经 std::process::ChildStdin 与子进程交换 chunk
    ///
    /// # 返回
    /// - Ok((child_stdin, child_pid)): 父进程持 child_stdin 写 chunk,完成后调 wait_isolated_child
    /// - Err: spawn 或 pipe 失败
    #[cfg(unix)]
    pub fn spawn_isolated_writer(&self, temp_path: &str, final_path: &str, _file_size: u64) -> Result<(std::process::ChildStdin, i32)> {
        use anyhow::Context;
        let _guard = with_fork_guard();
        let current_exe = isolated_executable()?;
        let mut cmd = std::process::Command::new(current_exe);
        cmd.arg("--isolated-writer").arg(temp_path)
           .arg("--final-path").arg(final_path)
           .arg("--uid").arg(self.uid.to_string())
           .arg("--gid").arg(self.gid.to_string())
           .arg("--supp-gids").arg(supplementary_gids_csv(&self.username, self.gid))
           .stdin(std::process::Stdio::piped())
           .stdout(std::process::Stdio::null())
           .stderr(std::process::Stdio::null());
        let mut child = cmd.spawn()
            .context("Failed to spawn isolated writer")?;
        let pid = child.id() as i32;
        let stdin = child.stdin.take().unwrap();
        Ok((stdin, pid))
    }

    /// 在隔离的子进程中处理文件读取,子进程 read file → write pipe,父进程经 pipe 读 chunk
    ///
    /// # 隔离链（posix_spawn 方案）
    /// - std::process::Command 自动用 posix_spawn 启动独立子进程（Rust 1.65+）
    /// - 子进程通过命令行参数接收 uid/gid/路径，自行 setgid/setuid 降权
    /// - 子进程内 open + read 文件(同步 IO),写入 stdout
    /// - 父进程经 std::process::ChildStdout 与子进程交换 chunk
    ///
    /// # 返回
    /// - Ok((child_stdout, child_pid)): 父进程持 child_stdout 读 chunk
    /// - Err: spawn 或 pipe 失败
    #[cfg(unix)]
    pub fn spawn_isolated_reader(&self, path: &str) -> Result<(std::process::ChildStdout, i32)> {
        use anyhow::Context;
        let _guard = with_fork_guard();
        let current_exe = isolated_executable()?;
        let mut cmd = std::process::Command::new(current_exe);
        cmd.arg("--isolated-reader").arg(path)
           .arg("--uid").arg(self.uid.to_string())
           .arg("--gid").arg(self.gid.to_string())
           .arg("--supp-gids").arg(supplementary_gids_csv(&self.username, self.gid))
           .stdin(std::process::Stdio::null())
           .stdout(std::process::Stdio::piped())
           .stderr(std::process::Stdio::null());
        let mut child = cmd.spawn()
            .context("Failed to spawn isolated reader")?;
        let pid = child.id() as i32;
        let stdout = child.stdout.take().unwrap();
        Ok((stdout, pid))
    }

    /// 在隔离的子进程中处理文件段写入（多流并行，方案 A: 独立段+合并）
    ///
    /// 与 `spawn_isolated_writer` 的区别：
    /// - 仅接受 `part_path`（段文件路径），无 `final_path`，**不执行 rename**
    /// - 子进程 flush + sync 后直接 _exit(0)，段文件留待后续 `spawn_isolated_merger` 合并
    ///
    /// # 隔离链（posix_spawn 方案）
    /// - std::process::Command 自动用 posix_spawn 启动独立子进程（Rust 1.65+）
    /// - 子进程通过命令行参数接收 uid/gid/路径，自行 setgid/setuid 降权
    /// - 子进程内 open part_path + write（同步 IO）
    /// - 父进程经 std::process::ChildStdin 与子进程交换 chunk
    ///
    /// # 返回
    /// - Ok((child_stdin, child_pid)): 父进程持 child_stdin 写 chunk
    /// - Err: spawn 或 pipe 失败
    ///
    /// # 退出码（与 writer 一致，去掉 7=rename）
    /// - 1: 降权失败 / 2: 打开段文件失败 / 3: 写入段文件失败 / 4: 读取 pipe 失败
    /// - 5: flush 失败 / 6: sync_all 失败
    #[cfg(unix)]
    pub fn spawn_isolated_writer_part(&self, part_path: &str) -> Result<(std::process::ChildStdin, i32)> {
        use anyhow::Context;
        let _guard = with_fork_guard();
        let current_exe = isolated_executable()?;
        let mut cmd = std::process::Command::new(current_exe);
        cmd.arg("--isolated-writer-part").arg(part_path)
           .arg("--uid").arg(self.uid.to_string())
           .arg("--gid").arg(self.gid.to_string())
           .arg("--supp-gids").arg(supplementary_gids_csv(&self.username, self.gid))
           .stdin(std::process::Stdio::piped())
           .stdout(std::process::Stdio::null())
           .stderr(std::process::Stdio::null());
        let mut child = cmd.spawn()
            .context("Failed to spawn isolated writer_part")?;
        let pid = child.id() as i32;
        let stdin = child.stdin.take().unwrap();
        Ok((stdin, pid))
    }

    /// 在隔离的子进程中合并段文件为最终文件（多流并行合并阶段）
    ///
    /// # 流程（posix_spawn 方案）
    /// 1. std::process::Command 启动独立 merger 子进程，传入 part_paths/final_path/uid/gid
    /// 2. 子进程 setuid/setgid 降权
    /// 3. 子进程 open final_path（create/truncate，O_NOFOLLOW，0o600）
    /// 4. 按 part_paths 顺序逐个 open 段文件 → read → write 到 final_path
    /// 5. fsync final_path
    /// 6. 删除所有段文件（清理）
    /// 7. _exit(0)
    ///
    /// # 返回
    /// - Ok(child_pid): 父进程调 wait_isolated_child 等待合并完成
    /// - Err: spawn 失败
    ///
    /// # 退出码（独立段，避免与 writer/reader 混淆）
    /// - 1: 降权失败 / 10: 打开最终文件失败 / 11: 打开段文件失败
    /// - 12: 读取段文件失败 / 13: 写入最终文件失败 / 14: sync_all 失败 / 15: 删除段文件失败
    #[cfg(unix)]
    pub fn spawn_isolated_merger(&self, part_paths: Vec<String>, final_path: &str) -> Result<i32> {
        use anyhow::Context;
        let _guard = with_fork_guard();
        let current_exe = isolated_executable()?;
        let part_paths_str = part_paths.join(",");
        let mut cmd = std::process::Command::new(current_exe);
        cmd.arg("--isolated-merger")
           .arg("--part-paths").arg(&part_paths_str)
           .arg("--final-path").arg(final_path)
           .arg("--uid").arg(self.uid.to_string())
           .arg("--gid").arg(self.gid.to_string())
           .arg("--supp-gids").arg(supplementary_gids_csv(&self.username, self.gid))
           .stdin(std::process::Stdio::null())
           .stdout(std::process::Stdio::null())
           .stderr(std::process::Stdio::null());
        let child = cmd.spawn()
            .context("Failed to spawn isolated merger")?;
        Ok(child.id() as i32)
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
    /// - 8: 路径不是常规文件(reader, FIFO/socket/device 等)
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
                    8 => "路径不是常规文件(FIFO/socket/device 等)",
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

    /// 获取文件元数据（size, mtime），在隔离子进程中执行
    ///
    /// 替代原 handler.rs 中的 execute_as_user + fs::metadata 闭包，
    /// 通过启动 --metadata 子进程获取元数据（同样以目标用户身份降权）。
    ///
    /// # 返回
    /// - Ok((size, mtime)): 文件大小（字节）+ 修改时间（UNIX 秒）
    /// - Err: spawn 失败 / 子进程非零退出 / JSON 解析失败
    #[cfg(unix)]
    pub async fn get_metadata_async(&self, path: &str) -> Result<(u64, u64)> {
        let path = path.to_string();
        let uid = self.uid;
        let gid = self.gid;
        let username = self.username.clone();
        tokio::task::spawn_blocking(move || -> Result<(u64, u64)> {
            use anyhow::Context;
            let _guard = with_fork_guard();
            let current_exe = isolated_executable()?;
            // 附加组解析留在此 spawn_blocking 内（/etc/group 读取不占 async worker）
            let supp_gids = supplementary_gids_csv(&username, gid);
            let output = std::process::Command::new(current_exe)
                .arg("--metadata").arg(&path)
                .arg("--uid").arg(uid.to_string())
                .arg("--gid").arg(gid.to_string())
                .arg("--supp-gids").arg(&supp_gids)
                .output()
                .context("Failed to spawn metadata command")?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let msg = if stderr.contains("目录") {
                    "路径是目录，不能下载".to_string()
                } else if stderr.contains("No such file") || stderr.is_empty() {
                    format!("文件 '{}' 不存在", path)
                } else {
                    format!("获取元数据失败: {}", stderr)
                };
                return Err(anyhow::anyhow!(msg));
            }
            // 解析 stdout JSON: {"size": N, "mtime": N}
            #[derive(serde::Deserialize)]
            struct MetadataResult { size: u64, mtime: u64 }
            let meta: MetadataResult = serde_json::from_slice(&output.stdout)
                .context("Failed to parse metadata JSON")?;
            Ok((meta.size, meta.mtime))
        }).await?
    }

    /// 在隔离的子进程中处理文件写入（非 Unix 平台占位）
    #[cfg(not(unix))]
    pub fn spawn_isolated_writer(&self, _temp_path: &str, _final_path: &str, _file_size: u64) -> Result<(std::process::ChildStdin, i32)> {
        anyhow::bail!("用户隔离写入仅支持 Unix 平台")
    }

    /// 在隔离的子进程中处理文件读取（非 Unix 平台占位）
    #[cfg(not(unix))]
    pub fn spawn_isolated_reader(&self, _path: &str) -> Result<(std::process::ChildStdout, i32)> {
        anyhow::bail!("用户隔离读取仅支持 Unix 平台")
    }

    /// 在隔离的子进程中处理文件段写入（非 Unix 平台占位）
    #[cfg(not(unix))]
    pub fn spawn_isolated_writer_part(&self, _part_path: &str) -> Result<(std::process::ChildStdin, i32)> {
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

    /// 获取文件元数据（非 Unix 平台占位）
    #[cfg(not(unix))]
    pub async fn get_metadata_async(&self, _path: &str) -> Result<(u64, u64)> {
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

// ============================================================================
// 管道帧协议（同一二进制内的父子进程，无跨版本兼容约束）
// ============================================================================
//
// 帧格式：
// - 成功帧: [QROK][4字节 LE 长度][JSON 数据] → 父进程解析为 Ok(T)
// - 错误帧: [QERR][4字节 LE 长度][JSON 错误消息] → 父进程解析为 Err
//
// 魔数保证帧类型判定无歧义。旧协议用单字节 0x00 当错误标记，而成功帧首字节
// 是 LE 长度的**低位**字节——data.len() % 256 == 0 时恰为 0x00，成功帧被误判
// 为错误帧，并把后续 JSON 首字节 '{'(0x7b) 当长度高位算出 ~2GB 的分配
// （2026-11-20 审计 A-05）。

/// 成功帧魔数
#[cfg(unix)]
const FRAME_MAGIC_OK: [u8; 4] = *b"QROK";
/// 错误帧魔数
#[cfg(unix)]
const FRAME_MAGIC_ERR: [u8; 4] = *b"QERR";
/// 结果/错误数据统一上限（防止异常子进程按不可信长度放大分配）
#[cfg(unix)]
const FRAME_DATA_LIMIT: usize = 100 * 1024 * 1024;

/// 向管道写入错误信息
///
/// 格式: [QERR][4字节长度][JSON 错误数据]
#[cfg(unix)]
fn write_error_to_pipe(fd: i32, message: &str) -> std::io::Result<()> {
    // 写入错误帧魔数
    write_all_to_pipe(fd, &FRAME_MAGIC_ERR)?;

    // 序列化错误消息
    let error_data = serde_json::to_vec(&message)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // 写入长度和数据
    let len = (error_data.len() as u32).to_le_bytes();
    write_all_to_pipe(fd, &len)?;
    write_all_to_pipe(fd, &error_data)?;

    Ok(())
}

/// 从管道精确读取 n 字节（循环补齐短读）
#[cfg(unix)]
fn read_exact_from_pipe(fd: i32, buf: &mut [u8]) -> Result<()> {
    let mut offset = 0;
    while offset < buf.len() {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().add(offset) as *mut _, buf.len() - offset) };
        if n <= 0 {
            return Err(anyhow::anyhow!("读取管道数据失败: 连接断开"));
        }
        offset += n as usize;
    }
    Ok(())
}

/// 从管道读取 4 字节 LE 长度并校验上限（成功帧与错误帧共用，防止按
/// 不可信长度盲目分配内存）
#[cfg(unix)]
fn read_len_from_pipe(fd: i32) -> Result<usize> {
    let mut len_buf = [0u8; 4];
    read_exact_from_pipe(fd, &mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > FRAME_DATA_LIMIT {
        return Err(anyhow::anyhow!("结果数据过大: {} bytes", len));
    }
    Ok(len)
}

/// 从管道读取结果
///
/// 格式:
/// - 正常结果: [QROK][4字节长度][JSON 数据] → 解析为 Ok(T)
/// - 错误结果: [QERR][4字节长度][JSON 错误消息] → 解析为 Err
#[cfg(unix)]
fn read_result_from_pipe<T>(fd: i32) -> Result<T>
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    // 读取 4 字节魔数判断帧类型
    let mut magic = [0u8; 4];
    read_exact_from_pipe(fd, &mut magic)?;

    if magic == FRAME_MAGIC_ERR {
        // 错误帧：长度与正常分支共用同一上限
        let msg_len = read_len_from_pipe(fd)?;
        let mut msg_buf = vec![0u8; msg_len];
        read_exact_from_pipe(fd, &mut msg_buf)?;

        let error_msg: String = serde_json::from_slice(&msg_buf)
            .unwrap_or_else(|_| "未知错误".to_string());
        return Err(anyhow::anyhow!("子进程执行失败: {}", error_msg));
    }
    if magic != FRAME_MAGIC_OK {
        return Err(anyhow::anyhow!(
            "管道数据帧魔数非法（协议不同步或数据损坏）"
        ));
    }

    // 正常结果：读取数据长度
    let data_len = read_len_from_pipe(fd)?;

    // 读取结果数据
    let mut data_buf = vec![0u8; data_len];
    read_exact_from_pipe(fd, &mut data_buf)?;

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
    fn parse_group_ids_extracts_groups_containing_user() {
        let content = "root:x:0:\n\
adm:x:4:syslog,testuser\n\
disk:x:6:\n\
docker:x:998:testuser,alice\n\
users:x:100:testuser\n";
        let mut ids = parse_supplementary_group_ids(content, "testuser");
        ids.sort();
        assert_eq!(ids, vec![4, 100, 998]);
    }

    #[test]
    fn parse_group_ids_requires_exact_member_match() {
        let content = "g1:x:1:testuser2\n\
g2:x:2:testuser\n";
        assert_eq!(parse_supplementary_group_ids(content, "testuser"), vec![2]);
    }

    #[test]
    fn parse_group_ids_tolerates_malformed_lines() {
        assert!(parse_supplementary_group_ids("root:x:0:\n", "testuser").is_empty());
        assert!(parse_supplementary_group_ids("", "testuser").is_empty());
        // 字段不足 / gid 非数字的畸形行应跳过而非 panic
        assert_eq!(
            parse_supplementary_group_ids("broken\nnox:x:abc:testuser\n", "testuser"),
            Vec::<u32>::new()
        );
    }

    /// A-01 回归：组列表并入主 gid + 去重（setgroups 前的最终集合）
    #[test]
    fn combine_group_ids_keeps_primary_and_dedups() {
        assert_eq!(combine_group_ids(1000, vec![27, 44, 27]), vec![27, 44, 1000]);
        assert_eq!(combine_group_ids(1000, vec![]), vec![1000]);
        // /etc/group 把主 gid 也列为附加组时不产生重复
        assert_eq!(combine_group_ids(1000, vec![1000]), vec![1000]);
    }

    /// A-01 回归：未知用户（或 /etc/group 缺失）→ 仅保留主 gid，fail-closed
    #[cfg(unix)]
    #[test]
    fn resolve_supplementary_groups_unknown_user_keeps_primary_only() {
        assert_eq!(
            resolve_supplementary_groups("no_such_user_xyz", 424242),
            vec![424242]
        );
    }

    /// A-01 回归：CSV 形态可被隔离子命令侧逐段解析回数字列表
    #[cfg(unix)]
    #[test]
    fn supplementary_gids_csv_roundtrip_parses_back() {
        let csv = supplementary_gids_csv("no_such_user_xyz", 424242);
        assert_eq!(csv, "424242");
        let parsed: Vec<u32> = csv.split(',').filter_map(|p| p.parse::<u32>().ok()).collect();
        assert_eq!(parsed, vec![424242]);
    }

    // ---- A-05 管道帧协议回归（QROK/QERR 魔数帧） ----

    /// 打开一对匿名管道供帧测试使用
    #[cfg(unix)]
    fn open_test_pipe() -> (i32, i32) {
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe 创建失败");
        (fds[0], fds[1])
    }

    /// A-05 复现：成功载荷 JSON 长度恰为 256 倍数 → LE 长度低位字节为 0x00。
    /// 旧协议以单字节 0x00 当错误标记，会把它误判为错误帧并按后续字节算出
    /// ~2GB 分配；新协议以 QROK 魔数判定，正确解析为成功帧。
    #[cfg(unix)]
    #[test]
    fn read_result_from_pipe_success_frame_with_len_multiple_of_256() {
        let payload = "a".repeat(254);
        let data = serde_json::to_vec(&payload).unwrap();
        assert_eq!(data.len() % 256, 0, "fixture 必须命中 256 倍数边界");

        let (read_fd, write_fd) = open_test_pipe();
        // 手写成功帧（与子进程写入侧同构）
        let mut frame = FRAME_MAGIC_OK.to_vec();
        frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
        frame.extend_from_slice(&data);
        write_all_to_pipe(write_fd, &frame).unwrap();
        unsafe { libc::close(write_fd); }

        let result: Result<String> = read_result_from_pipe(read_fd);
        unsafe { libc::close(read_fd); }
        assert_eq!(result.unwrap(), payload);
    }

    /// A-05 回归：错误帧按 QERR 魔数解析，消息内容透传
    #[cfg(unix)]
    #[test]
    fn read_result_from_pipe_error_frame_parses_message() {
        let (read_fd, write_fd) = open_test_pipe();
        write_error_to_pipe(write_fd, "测试错误消息").unwrap();
        unsafe { libc::close(write_fd); }

        let result: Result<String> = read_result_from_pipe(read_fd);
        unsafe { libc::close(read_fd); }
        let err = result.unwrap_err().to_string();
        assert!(err.contains("测试错误消息"), "应包含错误消息: {err}");
    }

    /// A-05 回归：魔数非法 → 明确报错，绝不按不可信长度盲目分配
    #[cfg(unix)]
    #[test]
    fn read_result_from_pipe_rejects_bad_magic() {
        let (read_fd, write_fd) = open_test_pipe();
        // 伪装成巨大长度但魔数错误的帧
        write_all_to_pipe(write_fd, &[0x01, 0x02, 0x03, 0x04]).unwrap();
        write_all_to_pipe(write_fd, &u32::MAX.to_le_bytes()).unwrap();
        unsafe { libc::close(write_fd); }

        let result: Result<String> = read_result_from_pipe(read_fd);
        unsafe { libc::close(read_fd); }
        let err = result.unwrap_err().to_string();
        assert!(err.contains("魔数非法"), "应报魔数非法: {err}");
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

    /// execute_as_user_blocking：async 上下文可等待，Ok/Err 均原样透传
    /// （实现走 spawn_blocking，把同步阻塞的 fork+waitpid 移出 runtime worker）
    #[tokio::test]
    async fn test_execute_as_user_blocking_returns_result() {
        let session = create_test_session();
        let executor = UserExecutor::new(&session);

        let ok: Result<i32> = executor.execute_as_user_blocking(|| Ok(42)).await;
        assert!(ok.is_ok(), "blocking 执行失败: {:?}", ok.err());
        assert_eq!(ok.unwrap(), 42);

        let err: Result<()> = executor.execute_as_user_blocking(|| {
            anyhow::bail!("Test error");
        })
        .await;
        assert!(err.is_err());
    }

    /// A-03 回归：reader 拒绝非常规文件的退出码 8 被翻译为可读错误
    #[cfg(unix)]
    #[test]
    fn wait_isolated_child_translates_exit_code_8() {
        let session = create_test_session();
        let executor = UserExecutor::new(&session);
        // spawn 一个以退出码 8 结束的子进程（模拟 reader 拒绝 FIFO/socket/device）
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("exit 8")
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let result = executor.wait_isolated_child(pid);
        assert!(result.is_err(), "非零退出码应返回 Err");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("常规文件"), "应翻译退出码 8: {msg}");
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