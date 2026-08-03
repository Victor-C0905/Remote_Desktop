//! 文件操作处理器 - 处理文件和目录相关请求
//!
//! 该模块负责：
//! - 读取目录列表（ReadDir）
//! - 读取文件内容（ReadFile）
//! - 写入文件（WriteFile）

use anyhow::{Result, Context};
use std::fs;
use std::path::Path;
use chrono::{DateTime, Local, TimeZone};

use crate::protocol::generated::{
    ReadDir, DirListing, FileEntry,
    ReadFile, FileContent,
    WriteFile, WriteResult,
    WorkerResponse, worker_response, Error,
};

/// 处理 ReadDir 请求
///
/// # 参数
///
/// - `req`: ReadDir 请求参数，包含路径
///
/// # 返回
///
/// 返回 `WorkerResponse`，包含 `DirListing` 或 `Error`。
///
/// # 示例
///
/// ```rust,ignore
/// let response = handle_read_dir(req).await;
/// match response.payload {
///     Some(worker_response::Payload::DirListing(listing)) => {
///         for entry in listing.entries {
///             println!("{}: {} bytes", entry.name, entry.size);
///         }
///     }
///     _ => { /* 错误处理 */ }
/// }
/// ```
#[tracing::instrument(fields(path = %req.path))]
pub async fn handle_read_dir(req: ReadDir) -> WorkerResponse {
    tracing::info!("处理 ReadDir 请求: path={}", req.path);

    let path = Path::new(&req.path);

    // 检查路径是否存在
    if !path.exists() {
        tracing::warn!("路径不存在: {}", req.path);
        return WorkerResponse {
            payload: Some(worker_response::Payload::Error(Error {
                code: 404,
                message: format!("Path not found: {}", req.path),
            })),
            ..Default::default()
        };
    }

    // 检查是否为目录
    if !path.is_dir() {
        tracing::warn!("路径不是目录: {}", req.path);
        return WorkerResponse {
            payload: Some(worker_response::Payload::Error(Error {
                code: 400,
                message: format!("Path is not a directory: {}", req.path),
            })),
            ..Default::default()
        };
    }

    // 读取目录
    match fs::read_dir(path) {
        Ok(entries) => {
            let mut file_entries = Vec::new();

            for entry_result in entries {
                match entry_result {
                    Ok(entry) => {
                        let path = entry.path();
                        let metadata = match entry.metadata() {
                            Ok(m) => m,
                            Err(e) => {
                                tracing::warn!("读取元数据失败: path={}, error={}", path.display(), e);
                                continue;
                            }
                        };

                        let name = entry.file_name().to_string_lossy().to_string();
                        let is_dir = metadata.is_dir();
                        let size = metadata.len();

                        // 格式化修改时间
                        let mtime = metadata.modified()
                            .map(|t| format_system_time(t))
                            .unwrap_or_else(|_| "Unknown".to_string());

                        // 格式化权限
                        let permissions = format_permissions(&metadata);

                        file_entries.push(FileEntry {
                            name,
                            is_dir,
                            size,
                            mtime,
                            permissions,
                        });
                    }
                    Err(e) => {
                        tracing::warn!("读取目录条目失败: error={}", e);
                        continue;
                    }
                }
            }

            tracing::debug!("读取目录完成: path={}, count={}", req.path, file_entries.len());

            WorkerResponse {
                payload: Some(worker_response::Payload::DirListing(DirListing {
                    path: req.path,
                    entries: file_entries,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("读取目录失败: path={}, error={}", req.path, e);

            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 500,
                    message: format!("Failed to read directory: {}", e),
                })),
                ..Default::default()
            }
        }
    }
}

/// 处理 ReadFile 请求
///
/// # 参数
///
/// - `req`: ReadFile 请求参数，包含文件路径
///
/// # 返回
///
/// 返回 `WorkerResponse`，包含 `FileContent` 或 `Error`。
///
/// # 限制
///
/// 当前实现读取整个文件内容，不适用于超大文件。
/// 未来可考虑流式传输或分块读取。
///
/// # 示例
///
/// ```rust,ignore
/// let response = handle_read_file(req).await;
/// match response.payload {
///     Some(worker_response::Payload::FileContent(content)) => {
///         println!("文件大小: {} 字节", content.content.len());
///     }
///     _ => { /* 错误处理 */ }
/// }
/// ```
#[tracing::instrument(fields(path = %req.path))]
pub async fn handle_read_file(req: ReadFile) -> WorkerResponse {
    tracing::info!("处理 ReadFile 请求: path={}", req.path);

    let path = Path::new(&req.path);

    // 检查文件是否存在
    if !path.exists() {
        tracing::warn!("文件不存在: {}", req.path);
        return WorkerResponse {
            payload: Some(worker_response::Payload::Error(Error {
                code: 404,
                message: format!("File not found: {}", req.path),
            })),
            ..Default::default()
        };
    }

    // 检查是否为文件
    if !path.is_file() {
        tracing::warn!("路径不是文件: {}", req.path);
        return WorkerResponse {
            payload: Some(worker_response::Payload::Error(Error {
                code: 400,
                message: format!("Path is not a file: {}", req.path),
            })),
            ..Default::default()
        };
    }

    // 读取文件元数据
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("读取文件元数据失败: path={}, error={}", req.path, e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 500,
                    message: format!("Failed to read file metadata: {}", e),
                })),
                ..Default::default()
            };
        }
    };

    // 读取文件内容
    match fs::read(path) {
        Ok(content) => {
            let mtime = metadata.modified()
                .map(|t| format_system_time_millis(t))
                .unwrap_or(0);

            tracing::debug!("读取文件完成: path={}, size={}", req.path, content.len());

            WorkerResponse {
                payload: Some(worker_response::Payload::FileContent(FileContent {
                    path: req.path,
                    content,
                    mtime,
                    size: metadata.len(),
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("读取文件失败: path={}, error={}", req.path, e);

            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 500,
                    message: format!("Failed to read file: {}", e),
                })),
                ..Default::default()
            }
        }
    }
}

/// 处理 WriteFile 请求
///
/// # 参数
///
/// - `req`: WriteFile 请求参数，包含文件路径和内容
///
/// # 返回
///
/// 返回 `WorkerResponse`，包含 `WriteResult` 或 `Error`。
///
/// # 安全性
///
/// - 不检查路径权限（由 Agent 配置文件控制）
/// - 不验证文件类型
///
/// # 示例
///
/// ```rust,ignore
/// let response = handle_write_file(req).await;
/// match response.payload {
///     Some(worker_response::Payload::WriteResult(result)) => {
///         println!("写入成功: {} 字节", result.size);
///     }
///     _ => { /* 错误处理 */ }
/// }
/// ```
#[tracing::instrument(fields(path = %req.path))]
pub async fn handle_write_file(req: WriteFile) -> WorkerResponse {
    tracing::info!("处理 WriteFile 请求: path={}, size={}", req.path, req.content.len());

    let path = Path::new(&req.path);

    // 写入文件（如果目录不存在，自动创建）
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            if let Err(e) = fs::create_dir_all(parent) {
                tracing::error!("创建目录失败: path={}, error={}", parent.display(), e);
                return WorkerResponse {
                    payload: Some(worker_response::Payload::Error(Error {
                        code: 500,
                        message: format!("Failed to create parent directory: {}", e),
                    })),
                    ..Default::default()
                };
            }
        }
    }

    match fs::write(path, &req.content) {
        Ok(_) => {
            // 读取写入后的元数据
            let metadata = match fs::metadata(path) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("读取写入后的文件元数据失败: path={}, error={}", req.path, e);
                    // 写入成功但读取元数据失败，返回部分结果
                    return WorkerResponse {
                        payload: Some(worker_response::Payload::WriteResult(WriteResult {
                            path: req.path,
                            mtime: 0,
                            size: req.content.len() as u64,
                        })),
                        ..Default::default()
                    };
                }
            };

            let mtime = metadata.modified()
                .map(|t| format_system_time_millis(t))
                .unwrap_or(0);

            tracing::info!("文件写入成功: path={}, size={}", req.path, metadata.len());

            WorkerResponse {
                payload: Some(worker_response::Payload::WriteResult(WriteResult {
                    path: req.path,
                    mtime,
                    size: metadata.len(),
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("写入文件失败: path={}, error={}", req.path, e);

            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error {
                    code: 500,
                    message: format!("Failed to write file: {}", e),
                })),
                ..Default::default()
            }
        }
    }
}

/// 格式化系统时间为字符串
///
/// # 参数
///
/// - `time`: 系统时间
///
/// # 返回
///
/// 格式化后的时间字符串（YYYY-MM-DD HH:MM:SS）。
fn format_system_time(time: std::time::SystemTime) -> String {
    let datetime: DateTime<Local> = time.into();
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 格式化系统时间为 Unix 时间戳（毫秒）
///
/// # 参数
///
/// - `time`: 系统时间
///
/// # 返回
///
/// Unix 时间戳（毫秒）。
fn format_system_time_millis(time: std::time::SystemTime) -> u64 {
    time.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 格式化文件权限
///
/// # 参数
///
/// - `metadata`: 文件元数据
///
/// # 返回
///
/// 权限字符串（如 "-rwxr-xr-x"）。
#[cfg(unix)]
fn format_permissions(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;

    let mode = metadata.permissions().mode();
    let perms = mode & 0o777;

    let mut result = String::new();

    // 文件类型
    result.push(if metadata.is_dir() { 'd' } else { '-' });

    // 用户权限
    result.push(if perms & 0o400 != 0 { 'r' } else { '-' });
    result.push(if perms & 0o200 != 0 { 'w' } else { '-' });
    result.push(if perms & 0o100 != 0 { 'x' } else { '-' });

    // 组权限
    result.push(if perms & 0o040 != 0 { 'r' } else { '-' });
    result.push(if perms & 0o020 != 0 { 'w' } else { '-' });
    result.push(if perms & 0o010 != 0 { 'x' } else { '-' });

    // 其他权限
    result.push(if perms & 0o004 != 0 { 'r' } else { '-' });
    result.push(if perms & 0o002 != 0 { 'w' } else { '-' });
    result.push(if perms & 0o001 != 0 { 'x' } else { '-' });

    result
}

/// 格式化文件权限（非 Unix 平台）
///
/// # 参数
///
/// - `metadata`: 文件元数据
///
/// # 返回
///
/// 简化的权限字符串。
#[cfg(not(unix))]
fn format_permissions(metadata: &fs::Metadata) -> String {
    let mut result = String::new();

    // 文件类型
    result.push(if metadata.is_dir() { 'd' } else { '-' });

    // 简化权限（只读/读写）
    let readonly = metadata.permissions().readonly();
    result.push(if readonly { 'r' } else { 'w' });
    result.push('-');
    result.push('-');

    result
}