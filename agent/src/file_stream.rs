//! 文件流处理模块
//!
//! 提供文件上传和下载的分块流式传输能力，支持进度跟踪和断点续传。

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// 默认分块大小 (64KB)
const DEFAULT_CHUNK_SIZE: u32 = 64 * 1024;

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
    /// - 路径在 Agent 配置的 `allowed_paths` 白名单内
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
                .map_err(|e| format!("跳转到断点位置失败: {}", e))?;

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
    pub fn progress(&self) -> u32 {
        if self.file_size == 0 {
            return 100; // 空文件直接返回完成
        }
        ((self.transferred * 100) / self.file_size) as u32
    }

    /// 获取已传输字节数
    pub fn transferred(&self) -> u64 {
        self.transferred
    }

    /// 获取文件总大小
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
    /// 临时文件使用固定后缀 `.tmp`，在生产环境中建议：
    /// - 使用随机 UUID 生成唯一临时文件名
    /// - 防止临时文件被恶意替换
    ///
    /// 此方法的路径验证由调用方负责（见 FileReader 说明）。
    ///
    /// # 流程
    /// 1. 创建临时文件（.tmp 后缀）
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
    /// - 如果临时文件不存在或大小不匹配，创建新文件并忽略 `resume_from`
    /// - 记录是否成功续传，用于降级处理
    ///
    /// # 错误
    /// - 权限不足
    /// - 磁盘空间不足
    /// - IO 错误
    pub fn with_resume(path: &str, file_size: u64, resume_from: u64) -> Result<Self, String> {
        let final_path = Path::new(path);

        // 创建临时文件路径
        let temp_path = format!("{}.tmp", path);

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
                // 尝试打开现有临时文件
                match OpenOptions::new()
                    .write(true)
                    .open(&temp_path)
                {
                    Ok(mut existing_file) => {
                        // 检查临时文件大小
                        match existing_file.metadata() {
                            Ok(metadata) => {
                                let temp_size = metadata.len();

                                // 如果临时文件大小匹配断点位置，继续写入
                                if temp_size == resume_from {
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
                                            let file = OpenOptions::new()
                                                .create(true)
                                                .write(true)
                                                .truncate(true)
                                                .open(&temp_path)
                                                .map_err(|e| format!("无法创建临时文件: {}", e))?;
                                            (BufWriter::new(file), 0, false)
                                        }
                                    }
                                } else {
                                    // 临时文件大小不匹配，降级为重新传输
                                    tracing::warn!(
                                        "临时文件大小不匹配 ({} != {})，降级为重新传输",
                                        temp_size,
                                        resume_from
                                    );
                                    let file = OpenOptions::new()
                                        .create(true)
                                        .write(true)
                                        .truncate(true)
                                        .open(&temp_path)
                                        .map_err(|e| format!("无法创建临时文件: {}", e))?;
                                    (BufWriter::new(file), 0, false)
                                }
                            }
                            Err(e) => {
                                tracing::warn!("无法获取临时文件元数据，降级为重新传输: {}", e);
                                let file = OpenOptions::new()
                                    .create(true)
                                    .write(true)
                                    .truncate(true)
                                    .open(&temp_path)
                                    .map_err(|e| format!("无法创建临时文件: {}", e))?;
                                (BufWriter::new(file), 0, false)
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("无法打开现有临时文件，降级为重新传输: {}", e);
                        let file = OpenOptions::new()
                            .create(true)
                            .write(true)
                            .truncate(true)
                            .open(&temp_path)
                            .map_err(|e| format!("无法创建临时文件: {}", e))?;
                        (BufWriter::new(file), 0, false)
                    }
                }
            } else {
                // 临时文件不存在，降级为重新传输
                tracing::info!("临时文件不存在，从头开始传输");
                let file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&temp_path)
                    .map_err(|e| format!("无法创建临时文件: {}", e))?;
                (BufWriter::new(file), 0, false)
            }
        } else {
            // 从头开始传输
            let file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temp_path)
                .map_err(|e| match e.kind() {
                    io::ErrorKind::PermissionDenied => {
                        format!("权限不足，无法创建临时文件: {}", temp_path)
                    }
                    io::ErrorKind::StorageFull => {
                        "磁盘空间不足".to_string()
                    }
                    _ => format!("无法创建临时文件 {}: {}", temp_path, e),
                })?;
            (BufWriter::new(file), 0, false)
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
            .map_err(|e| match e.kind() {
                io::ErrorKind::StorageFull => "磁盘空间不足".to_string(),
                _ => format!("写入文件失败: {}", e),
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
            .map_err(|e| format!("刷新缓冲区失败: {}", e))?;

        // 获取底层文件引用并同步到磁盘
        let file = self.file.get_ref();
        file.sync_all()
            .map_err(|e| format!("同步到磁盘失败: {}", e))?;

        // 重命名临时文件为最终文件
        fs::rename(&self.temp_path, &self.final_path).map_err(|e| {
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
    pub fn progress(&self) -> u32 {
        if self.file_size == 0 {
            return 100; // 空文件直接返回完成
        }
        ((self.transferred * 100) / self.file_size) as u32
    }

    /// 获取已传输字节数
    pub fn transferred(&self) -> u64 {
        self.transferred
    }

    /// 获取文件总大小
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// 检查是否已写入全部数据
    pub fn is_complete(&self) -> bool {
        self.transferred == self.file_size
    }

    /// 获取临时文件路径（用于调试和日志）
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

// ===== 启动时临时文件清理 =====

/// 默认临时文件过期时间（1 小时）
const TEMP_FILE_MAX_AGE_SECS: u64 = 3600;

/// 清理过期的临时文件（`.tmp` 后缀）
///
/// 在 Agent 启动时调用，扫描指定目录中超过 `max_age_secs` 的 `.tmp` 文件并删除。
/// 这些文件通常是上次运行中上传中断遗留的。
///
/// # 参数
/// - `scan_dirs`: 需要扫描的目录列表（通常来自 Agent 配置的 `allowed_paths`）
/// - `max_age_secs`: 临时文件最大存活时间（秒），超过此时间的 `.tmp` 文件将被删除
///
/// # 行为
/// - 递归扫描目录（最大深度 3 层，避免扫描过深）
/// - 仅删除 `.tmp` 后缀的文件
/// - 删除失败仅打印警告，不影响 Agent 启动
/// - 扫描目录不存在时静默跳过
pub fn cleanup_stale_temp_files(scan_dirs: &[String], max_age_secs: Option<u64>) {
    let max_age = max_age_secs.unwrap_or(TEMP_FILE_MAX_AGE_SECS);
    let now = std::time::SystemTime::now();
    let mut cleaned_count = 0u32;
    let mut failed_count = 0u32;

    for dir in scan_dirs {
        let dir_path = Path::new(dir);
        if !dir_path.exists() || !dir_path.is_dir() {
            continue;
        }

        // 递归扫描（最大深度 3 层）
        cleanup_stale_temp_files_recursive(dir_path, max_age, now, 0, 3, &mut cleaned_count, &mut failed_count);
    }

    if cleaned_count > 0 || failed_count > 0 {
        tracing::info!(
            "[启动清理] 临时文件清理完成: 删除 {} 个, 失败 {} 个",
            cleaned_count,
            failed_count
        );
    } else {
        tracing::info!("[启动清理] 未发现过期的临时文件");
    }
}

/// 递归扫描并清理过期临时文件
fn cleanup_stale_temp_files_recursive(
    dir: &Path,
    max_age_secs: u64,
    now: std::time::SystemTime,
    current_depth: u32,
    max_depth: u32,
    cleaned_count: &mut u32,
    failed_count: &mut u32,
) {
    if current_depth >= max_depth {
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return, // 权限不足等，静默跳过
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            // 递归扫描子目录
            cleanup_stale_temp_files_recursive(
                &path, max_age_secs, now, current_depth + 1, max_depth, cleaned_count, failed_count,
            );
            continue;
        }

        // 检查是否为 .tmp 文件
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();
        if !file_name.ends_with(".tmp") {
            continue;
        }

        // 检查文件年龄
        match entry.metadata() {
            Ok(metadata) => {
                let age = now
                    .duration_since(metadata.modified().unwrap_or(std::time::UNIX_EPOCH))
                    .unwrap_or_default()
                    .as_secs();

                if age > max_age_secs {
                    match fs::remove_file(&path) {
                        Ok(_) => {
                            tracing::info!(
                                "[启动清理] 已删除过期临时文件: {} (年龄: {}秒)",
                                path.display(),
                                age
                            );
                            *cleaned_count += 1;
                        }
                        Err(e) => {
                            tracing::warn!(
                                "[启动清理] 删除临时文件失败: {}: {}",
                                path.display(),
                                e
                            );
                            *failed_count += 1;
                        }
                    }
                }
            }
            Err(_) => continue, // 无法获取元数据，跳过
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
        let temp_path = format!("{}.tmp", file_path.to_str().unwrap());

        {
            // 创建 writer 并写入部分数据（不调用 finish）
            let mut writer = FileStreamWriter::new(file_path.to_str().unwrap(), 100).unwrap();
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

        {
            let mut writer = FileStreamWriter::new(file_path.to_str().unwrap(), 100).unwrap();
            writer.write_chunk(b"some data").unwrap();
            writer.abort(); // 主动删除临时文件
            // writer 在此被 drop，不应 panic 或重复删除
        }

        // 验证临时文件已被删除
        let temp_path = format!("{}.tmp", file_path.to_str().unwrap());
        assert!(!Path::new(&temp_path).exists());
    }

    #[test]
    fn test_cleanup_stale_temp_files() {
        // 测试启动时清理过期临时文件
        let temp_dir = TempDir::new().unwrap();
        let dir_path = temp_dir.path().to_str().unwrap().to_string();

        // 创建一个 .tmp 文件
        let tmp_file = temp_dir.path().join("stale_file.tmp");
        fs::write(&tmp_file, b"stale data").unwrap();

        // 设置文件修改时间为 2 小时前
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        filetime::set_file_mtime(
            tmp_file.to_str().unwrap(),
            filetime::FileTime::from_system_time(two_hours_ago),
        ).unwrap();

        // 创建一个非 .tmp 文件（不应被删除）
        let normal_file = temp_dir.path().join("normal_file.txt");
        fs::write(&normal_file, b"normal data").unwrap();

        // 清理过期临时文件
        cleanup_stale_temp_files(&[dir_path], Some(3600));

        // 验证 .tmp 文件被删除，普通文件保留
        assert!(!tmp_file.exists(), "过期的 .tmp 文件应被删除");
        assert!(normal_file.exists(), "普通文件不应被删除");
    }
}