//! 文件流处理模块
//!
//! 提供文件上传和下载的分块流式传输能力，支持进度跟踪和断点续传。

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// 生成随机临时文件路径
///
/// 使用随机 UUID 生成临时文件名，避免：
/// - 可预测的临时文件名被攻击者预创建符号链接劫持（symlink attack）
/// - 不同会话并发传输同一目标文件时临时文件冲突
///
/// 注意：随机命名后无法通过目标路径反查临时文件，断点续传需将临时名存入
/// TransferSession（当前隔离写入模式已禁用断点续传，故此处返回 None）。
///
/// 文件名格式: `gnome_remote_{uuid}.tmp`
pub fn generate_temp_path(path: &Path) -> String {
    let temp_name = format!("gnome_remote_{}.tmp", uuid::Uuid::new_v4());
    path.parent()
        .unwrap_or(Path::new("."))
        .join(temp_name)
        .to_string_lossy()
        .into_owned()
}

/// 查找已存在的临时文件（用于断点续传）
///
/// 随机 UUID 命名后无法通过目标路径确定性反查临时文件。
/// 当前隔离写入模式已禁用断点续传，故始终返回 None；
/// 若未来恢复断点续传，需将临时名持久化到 TransferSession 后按 session_id 查找。
fn find_existing_temp_file(_path: &Path) -> Option<String> {
    None
}

/// 默认分块大小 (256KB)
const DEFAULT_CHUNK_SIZE: u32 = 256 * 1024;

/// 文件流读取器（用于下载：分块读取文件）
#[derive(Debug)]
pub struct FileStreamReader {
    /// 文件读取器（带缓冲）
    file: BufReader<File>,
    /// 文件总大小
    file_size: u64,
    /// 已传输字节数
    transferred: u64,
    /// 分块大小
    chunk_size: u32,
}

impl FileStreamReader {
    /// 创建新的文件读取器
    ///
    /// # 参数
    /// - `path`: 文件路径
    /// - `file_size`: 文件大小（用于进度计算）
    ///
    /// # 安全性说明
    ///
    /// 此方法不验证路径安全性，调用方应确保：
    /// - 路径在操作权限范围内
    /// - 路径不包含路径遍历字符（如 `../`）
    /// - 调用方有足够的文件系统权限
    ///
    /// 实际的路径验证应在 handler 层面完成。
    ///
    /// # 错误
    /// - 文件不存在
    /// - 权限不足
    /// - IO 错误
    pub fn new(path: &str, file_size: u64) -> Result<Self, String> {
        Self::with_resume(path, file_size, 0)
    }

    /// 创建支持断点续传的文件读取器
    ///
    /// # 参数
    /// - `path`: 文件路径
    /// - `file_size`: 文件大小
    /// - `resume_from`: 从哪个字节开始读取（0 = 从头开始）
    ///
    /// # 错误
    /// - 文件不存在
    /// - 权限不足
    /// - IO 错误
    /// - `resume_from` 超出文件大小
    pub fn with_resume(path: &str, file_size: u64, resume_from: u64) -> Result<Self, String> {
        let path = Path::new(path);

        // 检查文件是否存在
        if !path.exists() {
            return Err(format!("文件不存在: {}", path.display()));
        }

        // 检查是否为文件（而非目录）
        if !path.is_file() {
            return Err(format!("路径不是文件: {}", path.display()));
        }

        // 打开文件
        let mut file = File::open(path).map_err(|e| {
            tracing::warn!("[FileStreamReader] 打开文件失败: path={}, error={}", path.display(), e);
            match e.kind() {
                io::ErrorKind::PermissionDenied => {
                    format!("权限不足，无法读取文件: {}", path.display())
                }
                _ => format!("无法打开文件 {}: {}", path.display(), e),
            }
        })?;

        // 如果需要断点续传，跳转到指定位置
        if resume_from > 0 {
            if resume_from > file_size {
                return Err(format!(
                    "断点续传位置超出文件大小: {} > {}",
                    resume_from, file_size
                ));
            }

            file.seek(SeekFrom::Start(resume_from))
                .map_err(|e| {
                    tracing::warn!("[FileStreamReader] seek 失败: error={}", e);
                    format!("跳转到断点位置失败: {}", e)
                })?;

            tracing::info!(
                "断点续传: 从 {} 字节开始读取 (总大小: {})",
                resume_from,
                file_size
            );
        }

        // 创建带缓冲的读取器
        let file = BufReader::new(file);

        Ok(Self {
            file,
            file_size,
            transferred: resume_from,
            chunk_size: DEFAULT_CHUNK_SIZE,
        })
    }

    /// 设置分块大小
    ///
    /// # 参数
    /// - `size`: 分块大小（字节）
    #[allow(dead_code)]
    pub fn set_chunk_size(&mut self, size: u32) {
        self.chunk_size = size;
    }

    /// 读取下一个数据块
    ///
    /// # 返回
    /// - `Ok(Some(buffer))`: 成功读取数据块
    /// - `Ok(None)`: 文件已读完
    /// - `Err`: 读取错误
    pub fn read_next_chunk(&mut self) -> Result<Option<Vec<u8>>, String> {
        // 计算剩余字节数
        let remaining = self.file_size.saturating_sub(self.transferred);

        // 检查是否已读完
        if remaining == 0 {
            return Ok(None);
        }

        // 计算本次读取大小
        let read_size = std::cmp::min(self.chunk_size as u64, remaining) as usize;

        // 创建 buffer
        let mut buffer = vec![0u8; read_size];

        // 读取数据
        let bytes_read = self
            .file
            .read(&mut buffer)
            .map_err(|e| format!("读取文件失败: {}", e))?;

        // 如果读取字节数为 0，表示文件结束
        if bytes_read == 0 {
            return Ok(None);
        }

        // 调整 buffer 大小（如果读取的字节数小于预期）
        if bytes_read < read_size {
            buffer.truncate(bytes_read);
        }

        // 更新已传输字节数
        self.transferred += bytes_read as u64;

        Ok(Some(buffer))
    }

    /// 获取进度百分比
    ///
    /// # 返回
    /// - 0-100 的百分比数值
    /// - 如果 file_size 为 0，返回 100（避免除零）
    #[allow(dead_code)]
    pub fn progress(&self) -> u32 {
        if self.file_size == 0 {
            return 100; // 空文件直接返回完成
        }
        ((self.transferred * 100) / self.file_size) as u32
    }

    /// 获取已传输字节数
    #[allow(dead_code)]
    pub fn transferred(&self) -> u64 {
        self.transferred
    }

    /// 获取文件总大小
    #[allow(dead_code)]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }
}

/// 文件流写入器（用于上传：分块写入文件）
///
/// # 临时文件清理策略
///
/// - 创建时生成 `.tmp` 后缀的临时文件
/// - `finish()` 成功后重命名为最终文件，标记 `completed = true`
/// - `abort()` 主动删除临时文件
/// - **Drop 保护**：如果 writer 被意外 drop（连接中断、进程异常等）且未完成，
///   自动删除临时文件，防止磁盘空间泄漏
#[derive(Debug)]
pub struct FileStreamWriter {
    /// 文件写入器（带缓冲）
    file: BufWriter<File>,
    /// 文件总大小
    file_size: u64,
    /// 已传输字节数
    transferred: u64,
    /// 临时文件路径
    temp_path: String,
    /// 最终文件路径
    final_path: String,
    /// 是否已完成传输（finish 成功后为 true）
    completed: bool,
}

impl FileStreamWriter {
    /// 创建新的文件写入器（使用临时文件）
    ///
    /// # 参数
    /// - `path`: 最终文件路径
    /// - `file_size`: 文件大小（用于进度计算）
    ///
    /// # 安全性说明
    ///
    /// 临时文件使用随机 UUID 生成唯一文件名（格式: `{原文件名}.{随机hex}.tmp`），
    /// 防止符号链接攻击和临时文件被恶意替换。
    ///
    /// 此方法的路径验证由调用方负责（见 FileReader 说明）。
    ///
    /// # 流程
    /// 1. 创建随机命名的临时文件
    /// 2. 写入数据到临时文件
    /// 3. 完成后重命名为最终文件
    ///
    /// # 错误
    /// - 权限不足
    /// - 磁盘空间不足
    /// - IO 错误
    pub fn new(path: &str, file_size: u64) -> Result<Self, String> {
        Self::with_resume(path, file_size, 0)
    }

    /// 创建支持断点续传的文件写入器
    ///
    /// # 参数
    /// - `path`: 最终文件路径
    /// - `file_size`: 文件大小
    /// - `resume_from`: 从哪个字节开始写入（0 = 从头开始）
    ///
    /// # 断点续传逻辑
    /// - 如果 `resume_from` > 0，尝试打开已存在的临时文件并跳转到指定位置
    /// - 验证临时文件完整性：
    ///   1. 文件大小是否与 `resume_from` 一致
    ///   2. 确保已有数据已落盘（flush + sync）
    /// - 如果临时文件不存在、大小不匹配或数据不完整，创建新文件并忽略 `resume_from`
    /// - 记录是否成功续传，用于降级处理
    ///
    /// # 错误
    /// - 权限不足
    /// - 磁盘空间不足
    /// - IO 错误
    pub fn with_resume(path: &str, file_size: u64, resume_from: u64) -> Result<Self, String> {
        let final_path = Path::new(path);

        // 确定临时文件路径：
        // - 断点续传时查找已存在的临时文件（格式: `{原文件名}.*.tmp`）
        // - 新传输时生成随机临时文件路径（格式: `{原文件名}.{随机hex}.tmp`）
        let temp_path = if resume_from > 0 {
            find_existing_temp_file(final_path).unwrap_or_else(|| generate_temp_path(final_path))
        } else {
            generate_temp_path(final_path)
        };

        // 检查目标目录是否存在
        let parent = final_path.parent();
        if let Some(dir) = parent {
            if !dir.exists() {
                return Err(format!("目标目录不存在: {}", dir.display()));
            }
        }

        // 尝试断点续传
        let (file, actual_resume_from, is_resuming) = if resume_from > 0 {
            // 检查临时文件是否存在
            if Path::new(&temp_path).exists() {
                // 验证临时文件完整性并尝试续传
                match Self::verify_temp_file_integrity(&temp_path, resume_from) {
                    Ok(mut existing_file) => {
                        // 跳转到断点位置
                        match existing_file.seek(SeekFrom::Start(resume_from)) {
                            Ok(_) => {
                                tracing::info!(
                                    "断点续传: 从 {} 字节继续写入临时文件 (总大小: {})",
                                    resume_from,
                                    file_size
                                );
                                (BufWriter::new(existing_file), resume_from, true)
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "跳转到断点位置失败，降级为重新传输: {}",
                                    e
                                );
                                // 降级：创建新文件
                                let (file, _, _) = Self::create_fresh_temp_file(&temp_path)?;
                                (file, 0, false)
                            }
                        }
                    }
                    Err(reason) => {
                        tracing::warn!("临时文件完整性校验失败: {}，降级为重新传输", reason);
                        let (file, _, _) = Self::create_fresh_temp_file(&temp_path)?;
                        (file, 0, false)
                    }
                }
            } else {
                // 临时文件不存在，降级为重新传输
                tracing::info!("临时文件不存在，从头开始传输");
                Self::create_fresh_temp_file(&temp_path)?
            }
        } else {
            // 从头开始传输
            Self::create_fresh_temp_file(&temp_path)?
        };

        // 如果成功续传，记录日志
        if is_resuming {
            tracing::info!(
                "断点续传成功: {} -> {} (已传输: {} 字节)",
                temp_path,
                path,
                actual_resume_from
            );
        }

        Ok(Self {
            file,
            file_size,
            transferred: actual_resume_from,
            temp_path,
            final_path: path.to_string(),
            completed: false,
        })
    }

    /// 验证临时文件完整性（用于断点续传）
    ///
    /// # 验证步骤
    /// 1. 打开现有临时文件（读写模式）
    /// 2. 获取文件大小并验证与 `expected_size` 匹配
    /// 3. flush 确保之前写入的数据完全提交到内核缓冲区
    /// 4. sync_all 确保数据和元数据落盘
    /// 5. 再次读取大小确认一致性（排除并发修改）
    ///
    /// # 返回
    /// - `Ok(File)`: 验证通过，返回可用于继续写入的文件句柄
    /// - `Err(String)`: 验证失败的原因
    fn verify_temp_file_integrity(temp_path: &str, expected_size: u64) -> Result<File, String> {
        // 打开现有临时文件（读写模式，不截断）
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(temp_path)
            .map_err(|e| format!("无法打开临时文件 {}: {}", temp_path, e))?;

        // 获取文件大小
        let metadata = file.metadata()
            .map_err(|e| format!("无法获取临时文件元数据: {}", e))?;
        let temp_size = metadata.len();

        // 验证文件大小是否匹配断点位置
        if temp_size != expected_size {
            return Err(format!(
                "临时文件大小不匹配: 实际 {} 字节, 期望 {} 字节",
                temp_size, expected_size
            ));
        }

        // flush + sync 确保之前写入的数据完全落盘
        // 这样即使之前进程异常退出导致部分数据还在内核缓冲区，
        // sync 也会将其写入磁盘，避免续传后数据不一致
        file.sync_all()
            .map_err(|e| format!("同步临时文件到磁盘失败: {}", e))?;

        // 再次读取大小确认一致性（排除 sync 期间的并发修改）
        let post_sync_metadata = file.metadata()
            .map_err(|e| format!("同步后重新获取文件元数据失败: {}", e))?;
        if post_sync_metadata.len() != expected_size {
            return Err(format!(
                "同步后临时文件大小发生变化: {} -> {}",
                temp_size, post_sync_metadata.len()
            ));
        }

        tracing::info!(
            "临时文件完整性校验通过: {} (大小: {} 字节)",
            temp_path,
            expected_size
        );

        Ok(file)
    }

    /// 创建全新的临时文件（截断模式）
    ///
    /// 用于：
    /// - 从头开始传输（resume_from == 0）
    /// - 断点续传降级（临时文件不存在或完整性校验失败）
    fn create_fresh_temp_file(temp_path: &str) -> Result<(BufWriter<File>, u64, bool), String> {
        let file = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                // O_NOFOLLOW: 不跟随符号链接，防御 symlink attack
                // mode 0o600: 仅属主可读写，避免其他用户窥探/篡改临时文件
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(temp_path)
            }
            #[cfg(not(unix))]
            {
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(temp_path)
            }
        }
        .map_err(|e| {
            tracing::warn!("[FileStreamWriter] 创建临时文件失败: error={}", e);
            match e.kind() {
                io::ErrorKind::PermissionDenied => {
                    format!("权限不足，无法创建临时文件: {}", temp_path)
                }
                io::ErrorKind::StorageFull => {
                    "磁盘空间不足".to_string()
                }
                _ => format!("无法创建临时文件 {}: {}", temp_path, e),
            }
        })?;
        Ok((BufWriter::new(file), 0, false))
    }

    /// 写入数据块
    ///
    /// # 参数
    /// - `data`: 数据块
    ///
    /// # 错误
    /// - 磁盘空间不足
    /// - IO 错误
    pub fn write_chunk(&mut self, data: &[u8]) -> Result<(), String> {
        // 检查是否会超出预期文件大小
        let new_transferred = self.transferred + data.len() as u64;
        if new_transferred > self.file_size {
            return Err(format!(
                "写入数据超出预期大小: 已写入 {} + 本次 {} > 总大小 {}",
                self.transferred,
                data.len(),
                self.file_size
            ));
        }

        // 写入数据
        self.file
            .write_all(data)
            .map_err(|e| {
                tracing::warn!("[FileStreamWriter] 写入失败: offset={}, error={}", self.transferred, e);
                match e.kind() {
                    io::ErrorKind::StorageFull => "磁盘空间不足".to_string(),
                    _ => format!("写入文件失败: {}", e),
                }
            })?;

        // 更新已传输字节数
        self.transferred = new_transferred;

        Ok(())
    }

    /// 完成写入（重命名临时文件为最终文件）
    ///
    /// # 流程
    /// 1. 刷新缓冲区
    /// 2. 同步到磁盘
    /// 3. 重命名临时文件
    pub fn finish(&mut self) -> Result<(), String> {
        // 刷新缓冲区
        self.file
            .flush()
            .map_err(|e| {
                tracing::warn!("[FileStreamWriter] flush/sync 失败: error={}", e);
                format!("刷新缓冲区失败: {}", e)
            })?;

        // 获取底层文件引用并同步到磁盘
        let file = self.file.get_ref();
        file.sync_all()
            .map_err(|e| {
                tracing::warn!("[FileStreamWriter] flush/sync 失败: error={}", e);
                format!("同步到磁盘失败: {}", e)
            })?;

        // 重命名临时文件为最终文件
        fs::rename(&self.temp_path, &self.final_path).map_err(|e| {
            tracing::warn!("[FileStreamWriter] 重命名失败: temp={}, final={}, error={}", self.temp_path, self.final_path, e);
            format!(
                "重命名文件失败: {} -> {}: {}",
                self.temp_path, self.final_path, e
            )
        })?;

        // 标记为已完成，防止 Drop 时删除最终文件
        self.completed = true;

        tracing::info!(
            "文件上传完成: {} ({}/{} 字节)",
            self.final_path,
            self.transferred,
            self.file_size
        );

        Ok(())
    }

    /// 取消写入（删除临时文件）
    ///
    /// # 说明
    /// 删除临时文件，忽略所有错误（因为取消操作不应失败）
    pub fn abort(&mut self) {
        // 标记为已完成，防止 Drop 时重复删除
        self.completed = true;
        if let Err(e) = fs::remove_file(&self.temp_path) {
            tracing::warn!("删除临时文件失败 {}: {}", self.temp_path, e);
        } else {
            tracing::info!("取消上传，已删除临时文件: {}", self.temp_path);
        }
    }

    /// 获取进度百分比
    ///
    /// # 返回
    /// - 0-100 的百分比数值
    /// - 如果 file_size 为 0，返回 100（避免除零）
    #[allow(dead_code)]
    pub fn progress(&self) -> u32 {
        if self.file_size == 0 {
            return 100; // 空文件直接返回完成
        }
        ((self.transferred * 100) / self.file_size) as u32
    }

    /// 获取已传输字节数
    #[allow(dead_code)]
    pub fn transferred(&self) -> u64 {
        self.transferred
    }

    /// 获取文件总大小
    #[allow(dead_code)]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// 检查是否已写入全部数据
    #[allow(dead_code)]
    pub fn is_complete(&self) -> bool {
        self.transferred == self.file_size
    }

    /// 获取临时文件路径（用于调试和日志）
    #[allow(dead_code)]
    pub fn temp_path(&self) -> &str {
        &self.temp_path
    }
}

/// Drop 保护：当 FileStreamWriter 被意外 drop 时自动清理临时文件
///
/// # 触发场景
/// - QUIC 连接中断，上传 stream 被关闭
/// - 进程 panic 导致栈展开
/// - TransferSession 被从 HashMap 中移除（超时清理等）
///
/// # 安全性
/// - `finish()` 和 `abort()` 会设置 `completed = true`，Drop 不会重复清理
/// - 删除失败仅打印警告，不 panic（Drop 中不应 panic）
impl Drop for FileStreamWriter {
    fn drop(&mut self) {
        if !self.completed && self.transferred < self.file_size {
            tracing::warn!(
                "[FileStreamWriter] 检测到未完成的文件传输，清理临时文件: {} (已传输: {}/{})",
                self.temp_path,
                self.transferred,
                self.file_size
            );
            if let Err(e) = fs::remove_file(&self.temp_path) {
                // 临时文件可能已被 abort() 删除，忽略 NotFound 错误
                if e.kind() != io::ErrorKind::NotFound {
                    tracing::warn!(
                        "[FileStreamWriter] 清理临时文件失败 {}: {}",
                        self.temp_path,
                        e
                    );
                }
            } else {
                tracing::info!(
                    "[FileStreamWriter] 已清理临时文件: {}",
                    self.temp_path
                );
            }
        }
    }
}

// ============================================================================
// 基于 pipe 的文件流（方案 ii' 隔离读写）
// ============================================================================

/// 默认分块大小 (64KB) — 与 FileStreamReader 一致
const PIPE_CHUNK_SIZE: usize = 64 * 1024;

/// 基于 pipe 的文件流写入器(方案 ii' 隔离写入)
///
/// 父进程持 pipe_writer 写 chunk,子进程(隔离)read pipe → write file
///
/// # 工作流程
/// 1. `spawn_isolated_writer` fork 子进程,子进程 setuid + namespace 后打开临时文件
/// 2. 父进程通过 `write_chunk` 向 pipe 写入数据,子进程从 pipe 读取并写入文件
/// 3. `finish()`: 父进程关闭 pipe(EOF),子进程 flush + sync + rename 后退出,父进程 waitpid
/// 4. `abort()`: 父进程关闭 pipe + kill 子进程 + 清理临时文件
pub struct PipeFileStreamWriter {
    /// pipe 写入端(Option 以便 finish/abort 时 take + drop)
    pipe_writer: Option<os_pipe::PipeWriter>,
    /// 子进程 PID
    child_pid: i32,
    /// 用户执行器(用于 wait_isolated_child)
    executor: crate::auth::UserExecutor,
    /// 临时文件路径(用于 abort 时清理)
    temp_path: String,
    /// 已传输字节数
    transferred: u64,
    /// 文件总大小
    file_size: u64,
    /// 是否已完成(finish 或 abort 后为 true,防止 Drop 重复清理)
    finished: bool,
}

impl PipeFileStreamWriter {
    /// 创建新的 pipe 文件流写入器
    ///
    /// # 参数
    /// - `pipe_writer`: os_pipe 写入端
    /// - `child_pid`: 隔离子进程 PID
    /// - `executor`: 用户执行器(用于 wait_isolated_child)
    /// - `file_size`: 文件总大小
    /// - `temp_path`: 临时文件路径(用于 abort 清理)
    pub fn new(
        pipe_writer: os_pipe::PipeWriter,
        child_pid: i32,
        executor: crate::auth::UserExecutor,
        file_size: u64,
        temp_path: String,
    ) -> Self {
        Self {
            pipe_writer: Some(pipe_writer),
            child_pid,
            executor,
            temp_path,
            transferred: 0,
            file_size,
            finished: false,
        }
    }

    /// 写入数据块(同步 IO,将数据写入 pipe,子进程从 pipe 读取并写入文件)
    ///
    /// # 参数
    /// - `data`: 数据块
    ///
    /// # 错误
    /// - 管道已关闭
    /// - 写入超出文件大小
    /// - IO 错误(管道断裂等)
    pub fn write_chunk(&mut self, data: &[u8]) -> Result<(), String> {
        use std::io::Write;
        let writer = self.pipe_writer.as_mut().ok_or("管道已关闭")?;

        // 检查是否会超出预期文件大小
        let new_transferred = self.transferred + data.len() as u64;
        if new_transferred > self.file_size {
            return Err(format!(
                "写入数据超出预期大小: 已写入 {} + 本次 {} > 总大小 {}",
                self.transferred, data.len(), self.file_size
            ));
        }

        // 写入 pipe
        writer.write_all(data).map_err(|e| {
            tracing::warn!("[PipeFileStreamWriter] 写入 pipe 失败: offset={}, error={}", self.transferred, e);
            format!("写入 pipe 失败: {}", e)
        })?;

        self.transferred = new_transferred;
        Ok(())
    }

    /// 完成写入(关闭 pipe + 等待子进程 flush + sync + rename)
    ///
    /// # 流程
    /// 1. 关闭 pipe writer(让子进程读到 EOF)
    /// 2. 等待子进程退出(子进程会 flush + sync_all + rename temp→final)
    /// 3. 检查子进程退出状态
    pub fn finish(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }

        // 关闭 pipe writer,让子进程读到 EOF
        self.pipe_writer.take();

        // 等待子进程完成(flush + sync + rename)
        // 注意:不在此处设置 finished=true,等 waitpid 成功后再标记
        match self.executor.wait_isolated_child(self.child_pid) {
            Ok(()) => {
                self.finished = true;
                tracing::info!(
                    "文件上传完成(隔离): {} ({}/{} 字节)",
                    self.temp_path, self.transferred, self.file_size
                );
                Ok(())
            }
            Err(e) => {
                // 失败时清理临时文件,防止残留
                let _ = fs::remove_file(&self.temp_path);
                self.finished = true; // 标记完成以避免 Drop 二次操作
                tracing::warn!(
                    "[PipeFileStreamWriter] finish 失败,已清理临时文件: {}, 错误: {}",
                    self.temp_path, e
                );
                Err(format!("等待隔离子进程失败: {}", e))
            }
        }
    }

    /// 取消写入(关闭 pipe + kill 子进程 + 清理临时文件)
    ///
    /// # 说明
    /// 关闭 pipe 和 kill 子进程,并尝试删除临时文件(忽略错误)
    pub fn abort(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;

        // 关闭 pipe
        self.pipe_writer.take();

        // kill 子进程并回收(防止僵尸进程)
        #[cfg(unix)]
        {
            unsafe {
                libc::kill(self.child_pid, libc::SIGTERM);
            }
            let mut status = 0i32;
            unsafe {
                libc::waitpid(self.child_pid, &mut status, 0);
            }
        }

        // 清理临时文件(父进程以 root 运行,可以删除任何文件)
        if let Err(e) = fs::remove_file(&self.temp_path) {
            if e.kind() != io::ErrorKind::NotFound {
                tracing::warn!("[PipeFileStreamWriter] 清理临时文件失败 {}: {}", self.temp_path, e);
            }
        } else {
            tracing::info!("[PipeFileStreamWriter] 已清理临时文件: {}", self.temp_path);
        }
    }

    /// 获取已传输字节数
    pub fn transferred(&self) -> u64 {
        self.transferred
    }

    /// 获取文件总大小
    #[allow(dead_code)]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }
}

/// Drop 保护:未完成的 writer 被 drop 时自动 abort(kill 子进程 + 清理临时文件)
impl Drop for PipeFileStreamWriter {
    fn drop(&mut self) {
        if !self.finished {
            tracing::warn!(
                "[PipeFileStreamWriter] 检测到未完成的文件传输,自动清理: {} (已传输: {}/{})",
                self.temp_path, self.transferred, self.file_size
            );
            self.abort();
        }
    }
}

/// 基于 pipe 的文件流读取器(方案 ii' 隔离读取)
///
/// 子进程(隔离)read file → write pipe,父进程经 pipe 读 chunk
///
/// # 工作流程
/// 1. `spawn_isolated_reader` fork 子进程,子进程 setuid + namespace 后打开文件
/// 2. 子进程循环 read file → write pipe
/// 3. 父进程通过 `read_next_chunk` 从 pipe 读取数据
/// 4. Drop 时自动 kill 子进程(如果尚未完成)
pub struct PipeFileStreamReader {
    /// pipe 读取端(Option 以便 finish/abort 时 take + drop)
    pipe_reader: Option<os_pipe::PipeReader>,
    /// 子进程 PID
    child_pid: i32,
    /// 用户执行器(用于 wait_isolated_child)
    executor: crate::auth::UserExecutor,
    /// 已传输字节数
    transferred: u64,
    /// 文件总大小
    file_size: u64,
    /// 是否已完成
    finished: bool,
}

impl PipeFileStreamReader {
    /// 创建新的 pipe 文件流读取器
    ///
    /// # 参数
    /// - `pipe_reader`: os_pipe 读取端
    /// - `child_pid`: 隔离子进程 PID
    /// - `executor`: 用户执行器(用于 wait_isolated_child)
    /// - `file_size`: 文件总大小
    pub fn new(
        pipe_reader: os_pipe::PipeReader,
        child_pid: i32,
        executor: crate::auth::UserExecutor,
        file_size: u64,
    ) -> Self {
        Self {
            pipe_reader: Some(pipe_reader),
            child_pid,
            executor,
            transferred: 0,
            file_size,
            finished: false,
        }
    }

    /// 读取下一个数据块(同步 IO,从 pipe 读取子进程写入的文件数据)
    ///
    /// # 返回
    /// - `Ok(Some(buffer))`: 成功读取数据块
    /// - `Ok(None)`: 文件已读完(pipe EOF,子进程已关闭写入端)
    /// - `Err`: 读取错误
    pub fn read_next_chunk(&mut self) -> Result<Option<Vec<u8>>, String> {
        use std::io::Read;
        let reader = self.pipe_reader.as_mut().ok_or("管道已关闭")?;

        let mut buf = vec![0u8; PIPE_CHUNK_SIZE];
        match reader.read(&mut buf) {
            Ok(0) => {
                // EOF — 子进程已关闭 pipe writer
                Ok(None)
            }
            Ok(n) => {
                buf.truncate(n);
                self.transferred += n as u64;
                Ok(Some(buf))
            }
            Err(e) => {
                tracing::warn!("[PipeFileStreamReader] 读取 pipe 失败: error={}", e);
                Err(format!("读取 pipe 失败: {}", e))
            }
        }
    }

    /// 完成读取(关闭 pipe + 等待子进程退出)
    ///
    /// 通常在读取完所有数据后调用,确保子进程被正确回收
    pub fn finish(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }

        // 关闭 pipe reader
        self.pipe_reader.take();

        // 等待子进程退出
        // 注意:不在此处设置 finished=true,等 waitpid 成功后再标记
        match self.executor.wait_isolated_child(self.child_pid) {
            Ok(()) => {
                self.finished = true;
                Ok(())
            }
            Err(e) => {
                // reader 无临时文件,仅标记完成以避免 Drop 二次操作
                self.finished = true;
                tracing::warn!(
                    "[PipeFileStreamReader] finish 失败: pid={}, 错误: {}",
                    self.child_pid, e
                );
                Err(format!("等待隔离子进程失败: {}", e))
            }
        }
    }

    /// 中断读取(关闭 pipe + kill 子进程)
    pub fn abort(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;

        // 关闭 pipe
        self.pipe_reader.take();

        // kill 子进程并回收
        #[cfg(unix)]
        {
            unsafe {
                libc::kill(self.child_pid, libc::SIGTERM);
            }
            let mut status = 0i32;
            unsafe {
                libc::waitpid(self.child_pid, &mut status, 0);
            }
        }
    }

    /// 获取已传输字节数
    #[allow(dead_code)]
    pub fn transferred(&self) -> u64 {
        self.transferred
    }

    /// 获取文件总大小
    #[allow(dead_code)]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }
}

/// Drop 保护:未完成的 reader 被 drop 时自动 abort(kill 子进程)
impl Drop for PipeFileStreamReader {
    fn drop(&mut self) {
        if !self.finished {
            tracing::warn!(
                "[PipeFileStreamReader] 检测到未完成的文件读取,自动清理子进程: pid={}",
                self.child_pid
            );
            self.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_file_stream_reader() {
        // 创建临时目录
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_read.txt");

        // 创建测试文件
        let test_data = b"Hello, World! This is a test file for reading.";
        fs::write(&file_path, test_data).unwrap();

        // 创建读取器
        let mut reader = FileStreamReader::new(file_path.to_str().unwrap(), test_data.len() as u64).unwrap();

        // 设置小块大小以测试多次读取
        reader.set_chunk_size(10);

        // 读取所有数据
        let mut all_data = Vec::new();
        while let Some(chunk) = reader.read_next_chunk().unwrap() {
            all_data.extend(chunk);
        }

        // 验证数据
        assert_eq!(all_data.as_slice(), test_data);
        assert_eq!(reader.transferred(), test_data.len() as u64);
        assert_eq!(reader.progress(), 100);
    }

    #[test]
    fn test_file_stream_writer() {
        // 创建临时目录
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_write.txt");

        // 测试数据
        let test_data = b"Hello, World! This is a test file for writing.";

        // 创建写入器
        let mut writer =
            FileStreamWriter::new(file_path.to_str().unwrap(), test_data.len() as u64).unwrap();

        // 分块写入
        writer.write_chunk(&test_data[0..10]).unwrap();
        writer.write_chunk(&test_data[10..20]).unwrap();
        writer.write_chunk(&test_data[20..]).unwrap();

        // 完成写入
        writer.finish().unwrap();

        // 验证文件
        let read_data = fs::read(&file_path).unwrap();
        assert_eq!(read_data.as_slice(), test_data);
    }

    #[test]
    fn test_file_not_found() {
        let result = FileStreamReader::new("/nonexistent/path.txt", 100);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("文件不存在"));
    }

    #[test]
    fn test_progress_with_zero_size() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("empty.txt");
        fs::write(&file_path, b"").unwrap();

        let reader = FileStreamReader::new(file_path.to_str().unwrap(), 0).unwrap();
        assert_eq!(reader.progress(), 100);
    }

    #[test]
    fn test_writer_drop_cleans_up_temp_file() {
        // 测试 Drop impl：未完成的 writer 被 drop 时应清理临时文件
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("drop_test.txt");

        let temp_path;
        {
            // 创建 writer 并写入部分数据（不调用 finish）
            let mut writer = FileStreamWriter::new(file_path.to_str().unwrap(), 100).unwrap();
            temp_path = writer.temp_path().to_string();
            writer.write_chunk(b"partial data").unwrap();
            // writer 在此被 drop，应自动清理临时文件
        }

        // 验证临时文件已被删除
        assert!(!Path::new(&temp_path).exists(), "临时文件应在 writer drop 后被删除");
        // 最终文件也不应存在（未完成）
        assert!(!file_path.exists(), "最终文件不应存在（传输未完成）");
    }

    #[test]
    fn test_writer_finish_prevents_drop_cleanup() {
        // 测试 finish() 后 Drop 不会删除最终文件
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("finish_test.txt");
        let test_data = b"complete data";

        {
            let mut writer =
                FileStreamWriter::new(file_path.to_str().unwrap(), test_data.len() as u64).unwrap();
            writer.write_chunk(test_data).unwrap();
            writer.finish().unwrap();
            // writer 在此被 drop，但已完成，不应删除文件
        }

        // 验证最终文件存在且内容正确
        assert!(file_path.exists(), "最终文件应在 finish 后存在");
        let read_data = fs::read(&file_path).unwrap();
        assert_eq!(read_data.as_slice(), test_data);
    }

    #[test]
    fn test_writer_abort_then_drop_no_double_delete() {
        // 测试 abort() 后 Drop 不会重复删除
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("abort_test.txt");

        let temp_path;
        {
            let mut writer = FileStreamWriter::new(file_path.to_str().unwrap(), 100).unwrap();
            temp_path = writer.temp_path().to_string();
            writer.write_chunk(b"some data").unwrap();
            writer.abort(); // 主动删除临时文件
            // writer 在此被 drop，不应 panic 或重复删除
        }

        // 验证临时文件已被删除
        assert!(!Path::new(&temp_path).exists());
    }

    /// 测试 PipeFileStreamWriter::abort 清理临时文件
    #[cfg(unix)]
    #[test]
    fn test_pipe_writer_abort_cleans_temp() {
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let final_path = tmp.path().join("aborted.bin");
        let temp_path = generate_temp_path(&final_path);

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
        let executor = crate::auth::UserExecutor::new(&session);

        let (pipe_writer, child_pid) = executor
            .spawn_isolated_writer(&temp_path, &final_path.to_string_lossy(), 1024)
            .unwrap();
        let mut writer = PipeFileStreamWriter::new(
            pipe_writer,
            child_pid,
            executor,
            1024,
            temp_path.clone(),
        );
        // 不调 finish,直接 abort
        writer.abort();

        // 验证临时文件已被清理
        assert!(
            !std::path::Path::new(&temp_path).exists(),
            "abort 后临时文件应被清理"
        );
    }

    /// 测试 PipeFileStreamWriter 未 finish 直接 drop 时自动清理
    #[cfg(unix)]
    #[test]
    fn test_pipe_writer_drop_when_unfinished() {
        use tempfile::tempdir;

        let tmp = tempdir().unwrap();
        let final_path = tmp.path().join("dropped.bin");
        let temp_path = generate_temp_path(&final_path);

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
        let executor = crate::auth::UserExecutor::new(&session);

        let child_pid = {
            let (pipe_writer, child_pid) = executor
                .spawn_isolated_writer(&temp_path, &final_path.to_string_lossy(), 1024)
                .unwrap();
            let _writer = PipeFileStreamWriter::new(
                pipe_writer,
                child_pid,
                executor,
                1024,
                temp_path.clone(),
            );
            // _writer drops here → Drop → abort → kill + waitpid + remove temp
            child_pid
        };

        // 验证子进程已被回收(Drop 调用 abort → waitpid)
        // Drop 的 abort 调 waitpid(0) 阻塞回收,这里再 wait 应 ECHILD
        let mut status = 0i32;
        let ret = unsafe { libc::waitpid(child_pid, &mut status, libc::WNOHANG) };
        // 若 Drop 已 waitpid,我们调 WNOHANG 应返回 -1(ECHILD)或 0(还在)
        assert!(
            ret == -1 || ret == 0,
            "Drop 后子进程应已被回收或不可达"
        );

        // 验证临时文件已清理
        assert!(
            !std::path::Path::new(&temp_path).exists(),
            "Drop 后临时文件应被清理"
        );
    }
}