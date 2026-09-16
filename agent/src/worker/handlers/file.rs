//! 文件操作处理器 - 处理文件和目录相关请求
//!
//! 该模块负责：
//! - 读取目录列表（ReadDir，含 owner/group）
//! - 读取文件内容（ReadFile）
//! - 探测文件格式（FileInfo）
//! - 写入文件（WriteFile）
//! - 删除文件/目录（Delete）
//! - 创建目录（Mkdir）
//! - 重命名（Rename）
//! - 复制文件（Copy）
//! - 移动（Move）
//! - 检查文件存在（FileExists）
//! - 应用差异（ApplyDiff）
//! - 修改权限（Chmod）
//! - 修改属主/属组（Chown）
//
// 阶段 3 改造:集成 UserExecutor(fork+setuid)实现用户隔离
// 所有文件操作在目标用户上下文中执行,Linux 文件系统权限自动生效
// 返回格式与 handler.rs 对齐:
// - ReadDir: mtime 为 RFC3339 字符串, permissions 为 9 字符(不含类型前缀)
// - ReadFile: mtime 为 Unix 秒(u64)
// - WriteFile: mtime 为 Unix 秒(u64)

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result as AnyhowResult;
use chrono::{DateTime, Utc};

use crate::auth::{UserSession, UserExecutor};
use crate::diff::{FileDiff, DiffType, apply_diff};
use crate::protocol::generated::{
    ReadDir, DirListing, FileEntry,
    ReadFile, FileContent, FileInfo, FileInfoResult,
    WriteFile, WriteResult,
    Delete, DeleteResult,
    Mkdir, MkdirResult,
    Rename, RenameResult,
    Copy, CopyResult,
    Move, MoveResult,
    FileExists as FileExistsReq, FileExistsResult,
    ApplyDiff as ApplyDiffReq, ApplyDiffResult,
    Chmod, ChmodResult, Chown, ChownResult,
    WorkerResponse, worker_response, Error,
};

/// 从 protobuf 消息的用户上下文字段构造 UserSession
fn build_user_session(uid: u32, gid: u32, username: &str, home_dir: &str) -> UserSession {
    use crate::auth::UserIdentity;
    let identity = UserIdentity::new(
        username.to_string(),
        uid,
        gid,
        home_dir.to_string(),
        "/bin/bash".to_string(), // 默认 shell,文件操作不使用
    );
    UserSession::new(identity)
}

/// 处理 ReadDir 请求
///
/// 在目标用户上下文中读取目录列表(通过 fork+setuid 实现用户隔离)。
/// 返回的 mtime 为 RFC3339 格式,permissions 为 9 字符(如 "rwxr-xr-x")。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_read_dir(req: ReadDir) -> WorkerResponse {
    tracing::info!("处理 ReadDir 请求: path={}, uid={}", req.path, req.uid);

    // 构造用户会话
    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：防目录穿越、限制用户家目录、防符号链接攻击
    let safe_path = match crate::auth::validate_path(&req.path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    // 序列化中间结果类型:(name, is_dir, size, mtime_rfc3339, permissions, owner, group)
    type DirEntry = (String, bool, u64, String, String, String, String);

    let path = safe_path.as_str().to_string();
    let result: AnyhowResult<Vec<DirEntry>> = executor.execute_as_user(move || {
        let p = Path::new(&path);

        if !p.exists() {
            anyhow::bail!("Path not found: {}", path);
        }
        if !p.is_dir() {
            anyhow::bail!("Path is not a directory: {}", path);
        }

        // 构建 uid/gid → 名称映射(手写解析 /etc/passwd 与 /etc/group,
        // 不引入新依赖;解析失败回退空表,条目属主显示数字字符串)
        let uid_names = build_uid_name_map();
        let gid_names = build_gid_name_map();

        let entries = fs::read_dir(p)
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("Permission denied") {
                    anyhow::anyhow!("权限不足: 无法访问目录 '{}' (需要相应的 Linux 用户权限)", path)
                } else {
                    anyhow::anyhow!("无法读取目录 '{}': {}", path, e)
                }
            })?
            .filter_map(|entry_result| {
                let entry = entry_result.ok()?;
                let metadata = entry.metadata().ok()?;
                let name = entry.file_name().to_string_lossy().to_string();

                let mtime = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| {
                        let dt = DateTime::<Utc>::from(SystemTime::UNIX_EPOCH + d);
                        dt.to_rfc3339()
                    })
                    .unwrap_or_default();

                // 属主/属组:MetadataExt 的 uid()/gid() 查映射,解析失败回退数字字符串
                #[cfg(unix)]
                let (owner, group) = {
                    use std::os::unix::fs::MetadataExt;
                    let uid = metadata.uid();
                    let gid = metadata.gid();
                    (
                        uid_names.get(&uid).cloned().unwrap_or_else(|| uid.to_string()),
                        gid_names.get(&gid).cloned().unwrap_or_else(|| gid.to_string()),
                    )
                };
                #[cfg(not(unix))]
                let (owner, group) = (String::new(), String::new());

                Some((
                    name,
                    metadata.is_dir(),
                    if metadata.is_dir() { 0 } else { metadata.len() },
                    mtime,
                    format_permissions(&metadata),
                    owner,
                    group,
                ))
            })
            .collect();

        Ok(entries)
    });

    match result {
        Ok(entries) => {
            tracing::info!("读取目录完成: path={}, count={}", req.path, entries.len());

            let file_entries = entries
                .into_iter()
                .map(|(name, is_dir, size, mtime, permissions, owner, group)| FileEntry {
                    name,
                    is_dir,
                    size,
                    mtime,
                    permissions,
                    owner,
                    group,
                })
                .collect();

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

            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}

/// 处理 ReadFile 请求
///
/// 在目标用户上下文中读取文件内容(通过 fork+setuid 实现用户隔离)。
/// 返回 content 为原始字节,mtime 为 Unix 秒。
/// 限制:文件大小不超过 10MB。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_read_file(req: ReadFile) -> WorkerResponse {
    tracing::info!("处理 ReadFile 请求: path={}, uid={}", req.path, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：防目录穿越、限制用户家目录、防符号链接攻击
    let safe_path = match crate::auth::validate_path(&req.path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    // 序列化中间结果类型:(content_bytes, mtime_secs, size)
    type FileData = (Vec<u8>, u64, u64);

    let path = safe_path.as_str().to_string();
    let result: AnyhowResult<FileData> = executor.execute_as_user(move || {
        let p = Path::new(&path);

        let metadata = fs::metadata(p).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("Permission denied") {
                anyhow::anyhow!("权限不足: 无法访问文件 '{}' (需要相应的 Linux 用户权限)", path)
            } else {
                anyhow::anyhow!("无法访问文件 '{}': {}", path, e)
            }
        })?;

        if metadata.is_dir() {
            anyhow::bail!("这是一个目录，不能作为文件读取");
        }

        // 10MB 大小限制
        const MAX_SIZE: u64 = 10 * 1024 * 1024;
        if metadata.len() > MAX_SIZE {
            anyhow::bail!("文件太大 ({}MB)，限制 10MB", metadata.len() / (1024 * 1024));
        }

        let content = fs::read(p).map_err(|e| {
            anyhow::anyhow!("读取文件失败: {}", e)
        })?;

        let mtime = metadata
            .modified()
            .map_err(|e| anyhow::anyhow!("无法获取修改时间: {}", e))?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("时间转换失败: {}", e))?
            .as_secs();

        Ok((content, mtime, metadata.len()))
    });

    match result {
        Ok((content, mtime, size)) => {
            tracing::debug!("读取文件完成: path={}, size={}", req.path, content.len());

            WorkerResponse {
                payload: Some(worker_response::Payload::FileContent(FileContent {
                    path: req.path,
                    content,
                    mtime,
                    size,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("读取文件失败: path={}, error={}", req.path, e);

            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}

/// is_text 启发式判定（参考 git 的 binary 检测思路）
///
/// 规则（检查前 8000 字节）：
/// 1. UTF-8/UTF-16 BOM → 文本
/// 2. 含 NUL(0x00) → 二进制
/// 3. 控制字符（除 \t \n \r）占比 >= 5% → 二进制
/// 4. 其余 → 文本（空文件视为文本）
fn detect_is_text(bytes: &[u8]) -> bool {
    // BOM 检测：UTF-8 (EF BB BF)、UTF-16LE (FF FE)、UTF-16BE (FE FF)
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF])
        || bytes.starts_with(&[0xFF, 0xFE])
        || bytes.starts_with(&[0xFE, 0xFF])
    {
        return true;
    }

    let check_len = bytes.len().min(8000);
    if check_len == 0 {
        return true; // 空文件视为文本
    }

    let mut control_count = 0usize;
    for &b in &bytes[..check_len] {
        if b == 0 {
            return false; // NUL 字节 → 二进制
        }
        // 非法控制字符（除 Tab/LF/CR）
        if b < 32 && b != b'\t' && b != b'\n' && b != b'\r' {
            control_count += 1;
        }
    }

    // 控制字符占比 < 5% 视为文本
    control_count * 100 / check_len < 5
}

/// 处理 FileInfo 请求：读取文件元数据 + 头部 512 字节
///
/// 在目标用户上下文中执行（fork+setuid 用户隔离），
/// 返回格式路由所需的全部信息（size/is_text/extension/magic_bytes）。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_file_info(req: FileInfo) -> WorkerResponse {
    tracing::info!("处理 FileInfo 请求: path={}, uid={}", req.path, req.uid);

    // 构造用户会话
    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：防目录穿越、限制用户家目录、防符号链接攻击
    let safe_path = match crate::auth::validate_path(&req.path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    // 序列化中间结果：(size, is_dir, extension, magic_bytes)
    let path = safe_path.as_str().to_string();
    let result: AnyhowResult<(u64, bool, String, Vec<u8>)> = executor.execute_as_user(move || {
        let p = Path::new(&path);

        let metadata = fs::metadata(p)
            .map_err(|e| anyhow::anyhow!("无法访问文件 '{}': {}", path, e))?;

        if !metadata.is_file() {
            // 目录或特殊文件（FIFO/socket/device）：返回目录标记，无头部字节
            return Ok((metadata.len(), metadata.is_dir(), String::new(), Vec::new()));
        }

        // 扩展名（小写、不含点）
        let extension = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        // 读取头部 512 字节（所有常见 magic number 都在前 16 字节内，512 留足余量）
        let mut file = fs::File::open(p)
            .map_err(|e| anyhow::anyhow!("打开文件失败 '{}': {}", path, e))?;
        let mut head = vec![0u8; 512];
        use std::io::Read;
        let n = file.read(&mut head)
            .map_err(|e| anyhow::anyhow!("读取文件头失败 '{}': {}", path, e))?;
        head.truncate(n);

        Ok((metadata.len(), false, extension, head))
    });

    match result {
        Ok((size, is_dir, extension, head)) => {
            let is_text = if is_dir { false } else { detect_is_text(&head) };
            tracing::debug!("探测完成: path={}, size={}, is_dir={}, is_text={}, ext={}", req.path, size, is_dir, is_text, extension);

            WorkerResponse {
                payload: Some(worker_response::Payload::FileInfoResult(FileInfoResult {
                    path: req.path,
                    size,
                    is_dir,
                    is_text,
                    extension,
                    magic_bytes: head,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("FileInfo 处理失败: path={}, error={}", req.path, e);

            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}

/// 处理 WriteFile 请求
///
/// 在目标用户上下文中写入文件(通过 fork+setuid 实现用户隔离)。
/// 返回 mtime 为 Unix 秒。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_write_file(req: WriteFile) -> WorkerResponse {
    tracing::info!("处理 WriteFile 请求: path={}, size={}, uid={}", req.path, req.content.len(), req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：防目录穿越、限制用户家目录、防符号链接攻击
    let safe_path = match crate::auth::validate_path(&req.path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    // 序列化中间结果类型:(mtime_secs, size)
    type WriteOutcome = (u64, u64);

    let path = safe_path.as_str().to_string();
    let content = req.content.clone();
    let result: AnyhowResult<WriteOutcome> = executor.execute_as_user(move || {
        let p = Path::new(&path);

        // 如果目录不存在,自动创建
        if let Some(parent) = p.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| {
                    anyhow::anyhow!("创建目录失败: {}", e)
                })?;
            }
        }

        fs::write(p, &content).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("Permission denied") {
                anyhow::anyhow!("权限不足: 无法写入文件 '{}' (需要相应的 Linux 用户权限)", path)
            } else {
                anyhow::anyhow!("写入文件失败: {}", e)
            }
        })?;

        // 获取写入后的 mtime
        let metadata = fs::metadata(p).map_err(|e| {
            anyhow::anyhow!("无法获取文件信息: {}", e)
        })?;

        let mtime = metadata
            .modified()
            .map_err(|e| anyhow::anyhow!("无法获取修改时间: {}", e))?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("时间转换失败: {}", e))?
            .as_secs();

        Ok((mtime, metadata.len()))
    });

    match result {
        Ok((mtime, size)) => {
            tracing::info!("文件写入成功: path={}, size={}", req.path, size);

            WorkerResponse {
                payload: Some(worker_response::Payload::WriteResult(WriteResult {
                    path: req.path,
                    mtime,
                    size,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("写入文件失败: path={}, error={}", req.path, e);

            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}

/// 处理 Delete 请求
///
/// 在目标用户上下文中删除文件或目录。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_delete(req: Delete) -> WorkerResponse {
    tracing::info!("处理 Delete 请求: path={}, uid={}", req.path, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：防目录穿越、限制用户家目录、防符号链接攻击
    let safe_path = match crate::auth::validate_path(&req.path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    let path = safe_path.as_str().to_string();
    let result: AnyhowResult<()> = executor.execute_as_user(move || {
        let p = Path::new(&path);
        let metadata = fs::metadata(p).map_err(|e| {
            anyhow::anyhow!("无法访问 '{}': {}", path, e)
        })?;

        if metadata.is_dir() {
            fs::remove_dir_all(p).map_err(|e| anyhow::anyhow!("删除目录失败: {}", e))?;
        } else {
            fs::remove_file(p).map_err(|e| anyhow::anyhow!("删除文件失败: {}", e))?;
        }
        Ok(())
    });

    match result {
        Ok(()) => {
            tracing::info!("删除成功: path={}", req.path);
            WorkerResponse {
                payload: Some(worker_response::Payload::DeleteResult(DeleteResult { success: true })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("删除失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}

/// 处理 Mkdir 请求
///
/// 在目标用户上下文中创建目录(含父目录)。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_mkdir(req: Mkdir) -> WorkerResponse {
    tracing::info!("处理 Mkdir 请求: path={}, uid={}", req.path, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：防目录穿越、限制用户家目录、防符号链接攻击
    let safe_path = match crate::auth::validate_path(&req.path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    let path = safe_path.as_str().to_string();
    let result: AnyhowResult<String> = executor.execute_as_user(move || {
        fs::create_dir_all(&path).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("Permission denied") {
                anyhow::anyhow!("权限不足: 无法创建目录 '{}' (需要相应的 Linux 用户权限)", path)
            } else {
                anyhow::anyhow!("无法创建目录 '{}': {}", path, e)
            }
        })?;
        Ok(path)
    });

    match result {
        Ok(path) => {
            tracing::info!("创建目录成功: path={}", path);
            WorkerResponse {
                payload: Some(worker_response::Payload::MkdirResult(MkdirResult { path })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("创建目录失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}

/// 处理 Rename 请求
///
/// 在目标用户上下文中重命名文件或目录。
#[tracing::instrument(fields(old_path = %req.old_path, new_path = %req.new_path, uid = req.uid))]
pub async fn handle_rename(req: Rename) -> WorkerResponse {
    tracing::info!("处理 Rename 请求: {} -> {}, uid={}", req.old_path, req.new_path, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：验证 old_path 和 new_path（防目录穿越、限制家目录、防符号链接）
    let safe_old = match crate::auth::validate_path(&req.old_path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.old_path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let safe_new = match crate::auth::validate_path(&req.new_path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.new_path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    let old_path = safe_old.as_str().to_string();
    let new_path = safe_new.as_str().to_string();
    let result: AnyhowResult<(String, String)> = executor.execute_as_user(move || {
        fs::rename(&old_path, &new_path).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("Permission denied") {
                anyhow::anyhow!("权限不足: 无法重命名 '{}' (需要相应的 Linux 用户权限)", old_path)
            } else {
                anyhow::anyhow!("无法重命名 '{}': {}", old_path, e)
            }
        })?;
        Ok((old_path, new_path))
    });

    match result {
        Ok((old, new)) => {
            tracing::info!("重命名成功: {} -> {}", old, new);
            WorkerResponse {
                payload: Some(worker_response::Payload::RenameResult(RenameResult {
                    old_path: old,
                    new_path: new,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("重命名失败: {} -> {}, error={}", req.old_path, req.new_path, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}

/// 处理 Copy 请求
///
/// 在目标用户上下文中复制文件(不支持目录)。
#[tracing::instrument(fields(src = %req.src, dst = %req.dst, uid = req.uid))]
pub async fn handle_copy(req: Copy) -> WorkerResponse {
    tracing::info!("处理 Copy 请求: {} -> {}, uid={}", req.src, req.dst, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：验证 src 和 dst（防目录穿越、限制家目录、防符号链接）
    let safe_src = match crate::auth::validate_path(&req.src, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.src, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let safe_dst = match crate::auth::validate_path(&req.dst, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.dst, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    let src = safe_src.as_str().to_string();
    let dst = safe_dst.as_str().to_string();
    let result: AnyhowResult<(String, String)> = executor.execute_as_user(move || {
        let metadata = fs::metadata(&src).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("Permission denied") {
                anyhow::anyhow!("权限不足: 无法访问源文件 '{}' (需要相应的 Linux 用户权限)", src)
            } else if msg.contains("No such file") {
                anyhow::anyhow!("源文件 '{}' 不存在", src)
            } else {
                anyhow::anyhow!("无法访问源文件 '{}': {}", src, e)
            }
        })?;

        if metadata.is_dir() {
            anyhow::bail!("不支持复制目录 '{}' (请使用移动功能)", src);
        }

        fs::copy(&src, &dst).map_err(|e| {
            let msg = e.to_string();
            if msg.contains("Permission denied") {
                anyhow::anyhow!("权限不足: 无法复制 '{}' (需要相应的 Linux 用户权限)", src)
            } else {
                anyhow::anyhow!("无法复制 '{}': {}", src, e)
            }
        })?;
        Ok((src, dst))
    });

    match result {
        Ok((s, d)) => {
            tracing::info!("复制成功: {} -> {}", s, d);
            WorkerResponse {
                payload: Some(worker_response::Payload::CopyResult(CopyResult { src: s, dst: d })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("复制失败: {} -> {}, error={}", req.src, req.dst, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}

/// 处理 Move 请求
///
/// Move 等同于 Rename,在目标用户上下文中执行。
#[tracing::instrument(fields(src = %req.src, dst = %req.dst, uid = req.uid))]
pub async fn handle_move(req: Move) -> WorkerResponse {
    tracing::info!("处理 Move 请求: {} -> {}, uid={}", req.src, req.dst, req.uid);

    // 路径安全校验：验证 src 和 dst（Move 委托给 Rename，在此提前校验）
    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    if let Err(e) = crate::auth::validate_path(&req.src, session.home_dir.as_path(), session.uid) {
        tracing::error!("路径校验失败: path={}, error={}", req.src, e);
        let (code, message) = error_to_code_message(&e);
        return WorkerResponse {
            payload: Some(worker_response::Payload::Error(Error { code, message })),
            ..Default::default()
        };
    }
    if let Err(e) = crate::auth::validate_path(&req.dst, session.home_dir.as_path(), session.uid) {
        tracing::error!("路径校验失败: path={}, error={}", req.dst, e);
        let (code, message) = error_to_code_message(&e);
        return WorkerResponse {
            payload: Some(worker_response::Payload::Error(Error { code, message })),
            ..Default::default()
        };
    }

    // Move 本质上是 Rename
    let rename_req = Rename {
        old_path: req.src,
        new_path: req.dst,
        uid: req.uid,
        gid: req.gid,
        username: req.username,
        home_dir: req.home_dir,
    };

    match handle_rename(rename_req).await {
        resp => match resp.payload {
            Some(worker_response::Payload::RenameResult(r)) => WorkerResponse {
                payload: Some(worker_response::Payload::MoveResult(MoveResult {
                    src: r.old_path,
                    dst: r.new_path,
                })),
                ..Default::default()
            },
            other => WorkerResponse { payload: other, ..Default::default() },
        }
    }
}

/// 处理 FileExists 请求
///
/// 在目标用户上下文中检查文件是否存在,返回大小和修改时间。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_file_exists(req: FileExistsReq) -> WorkerResponse {
    tracing::info!("处理 FileExists 请求: path={}, uid={}", req.path, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：防目录穿越、限制用户家目录、防符号链接攻击
    let safe_path = match crate::auth::validate_path(&req.path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    let path = safe_path.as_str().to_string();
    let result: AnyhowResult<Option<(u64, u64)>> = executor.execute_as_user(move || {
        match fs::metadata(&path) {
            Ok(meta) => {
                let size = meta.len();
                let mtime = meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                Ok(Some((size, mtime)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(anyhow::anyhow!("无法访问文件: {}", e)),
        }
    });

    match result {
        Ok(Some((size, mtime))) => {
            tracing::debug!("文件存在: path={}, size={}", req.path, size);
            WorkerResponse {
                payload: Some(worker_response::Payload::FileExistsResult(FileExistsResult {
                    exists: true,
                    size,
                    mtime,
                })),
                ..Default::default()
            }
        }
        Ok(None) => {
            tracing::debug!("文件不存在: path={}", req.path);
            WorkerResponse {
                payload: Some(worker_response::Payload::FileExistsResult(FileExistsResult {
                    exists: false,
                    size: 0,
                    mtime: 0,
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("检查文件存在失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}

/// 处理 Chmod 请求
///
/// 在目标用户上下文中修改文件/目录权限(通过 fork+setuid 实现用户隔离)。
/// mode 为八进制数值(如 0o755 = 493);recursive=true 时递归应用于目录下所有内容
/// (递归遍历跳过符号链接,与 chmod -R 行为一致)。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_chmod(req: Chmod) -> WorkerResponse {
    tracing::info!("处理 Chmod 请求: path={}, mode={:o}, recursive={}, uid={}", req.path, req.mode, req.recursive, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：防目录穿越、限制用户家目录、防符号链接攻击
    let safe_path = match crate::auth::validate_path(&req.path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    let path = safe_path.as_str().to_string();
    let mode = req.mode;
    let recursive = req.recursive;
    let result: AnyhowResult<()> = executor.execute_as_user(move || {
        apply_chmod(&path, mode, recursive)
    });

    match result {
        Ok(()) => {
            tracing::info!("修改权限成功: path={}, mode={:o}", req.path, req.mode);
            WorkerResponse {
                payload: Some(worker_response::Payload::ChmodResult(ChmodResult { success: true })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("修改权限失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}

/// 处理 Chown 请求
///
/// 在目标用户上下文中修改文件/目录属主/属组(通过 fork+setuid 实现用户隔离)。
/// owner/group 为用户名/组名,先解析为 uid/gid(不存在时返回确定性错误),
/// 再用 std::os::unix::fs::chown 执行(Rust 1.73+ 标准库)。
/// 语义:把 owner 改成其他用户仅登录用户为 root 时成功,否则 Linux 返回 EPERM(自然发生);
/// recursive=true 时递归应用于目录下所有内容(遍历跳过符号链接)。
#[tracing::instrument(fields(path = %req.path, uid = req.uid))]
pub async fn handle_chown(req: Chown) -> WorkerResponse {
    tracing::info!("处理 Chown 请求: path={}, owner={}, group={}, recursive={}, uid={}",
        req.path, req.owner, req.group, req.recursive, req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：防目录穿越、限制用户家目录、防符号链接攻击
    let safe_path = match crate::auth::validate_path(&req.path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };

    // 名称 → uid/gid 解析(在降权执行前完成,不存在时直接返回确定性错误)
    let target_uid = match resolve_uid_by_name(&req.owner) {
        Some(uid) => uid,
        None => {
            let msg = format!("chown 失败: 用户 '{}' 不存在", req.owner);
            tracing::error!("{}", msg);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code: 404, message: msg })),
                ..Default::default()
            };
        }
    };
    let target_gid = match resolve_gid_by_name(&req.group) {
        Some(gid) => gid,
        None => {
            let msg = format!("chown 失败: 组 '{}' 不存在", req.group);
            tracing::error!("{}", msg);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code: 404, message: msg })),
                ..Default::default()
            };
        }
    };

    let executor = UserExecutor::new(&session);

    let path = safe_path.as_str().to_string();
    let recursive = req.recursive;
    let result: AnyhowResult<()> = executor.execute_as_user(move || {
        apply_chown(&path, target_uid, target_gid, recursive)
    });

    match result {
        Ok(()) => {
            tracing::info!("修改属主成功: path={}, owner={}, group={}", req.path, req.owner, req.group);
            WorkerResponse {
                payload: Some(worker_response::Payload::ChownResult(ChownResult { success: true })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("修改属主失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            }
        }
    }
}

/// 处理 ApplyDiff 请求
///
/// 在目标用户上下文中应用文件差异(mtime 版本校验 + 应用差异 + 写入)。
#[tracing::instrument(fields(path = %req.path, base_mtime = req.base_mtime, uid = req.uid))]
pub async fn handle_apply_diff(req: ApplyDiffReq) -> WorkerResponse {
    tracing::info!("处理 ApplyDiff 请求: path={}, base_mtime={}, diffs={}, uid={}",
        req.path, req.base_mtime, req.diffs.len(), req.uid);

    let session = build_user_session(req.uid, req.gid, &req.username, &req.home_dir);
    // 路径安全校验：防目录穿越、限制用户家目录、防符号链接攻击
    let safe_path = match crate::auth::validate_path(&req.path, session.home_dir.as_path(), session.uid) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("路径校验失败: path={}, error={}", req.path, e);
            let (code, message) = error_to_code_message(&e);
            return WorkerResponse {
                payload: Some(worker_response::Payload::Error(Error { code, message })),
                ..Default::default()
            };
        }
    };
    let executor = UserExecutor::new(&session);

    // 转换 protobuf FileDiff 为内部 FileDiff
    let diffs: Vec<FileDiff> = req.diffs.iter().map(|d| FileDiff {
        diff_type: match d.diff_type {
            0 => DiffType::Insert,
            1 => DiffType::Delete,
            2 => DiffType::Replace,
            _ => DiffType::Replace,
        },
        line_number: d.line_number as usize,
        old_content: if d.old_content.is_empty() { None } else { Some(d.old_content.clone()) },
        new_content: if d.new_content.is_empty() { None } else { Some(d.new_content.clone()) },
    }).collect();

    let path = safe_path.as_str().to_string();
    let base_mtime = req.base_mtime;
    let result: AnyhowResult<u64> = executor.execute_as_user(move || {
        // 获取文件当前 mtime
        let metadata = fs::metadata(&path)
            .map_err(|e| anyhow::anyhow!("无法获取文件信息 '{}': {}", path, e))?;
        let current_mtime = metadata
            .modified()
            .map_err(|e| anyhow::anyhow!("无法获取修改时间: {}", e))?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("时间转换失败: {}", e))?
            .as_secs();

        // 检查版本冲突
        if current_mtime != base_mtime {
            anyhow::bail!(
                "文件版本冲突: 期望 mtime={}, 实际 mtime={}",
                base_mtime, current_mtime
            );
        }

        // 读取原文件内容（按字节读取，避免非 UTF-8 文件 read_to_string 失败）
        let old_bytes = fs::read(&path)
            .map_err(|e| anyhow::anyhow!("读取文件失败: {}", e))?;
        let old_content = String::from_utf8(old_bytes)
            .map_err(|e| anyhow::anyhow!("文件不是有效的 UTF-8 文本，无法应用差异: {}", e))?;

        // 应用差异
        let new_content = apply_diff(&old_content, &diffs);

        // 写入文件
        fs::write(&path, &new_content)
            .map_err(|e| anyhow::anyhow!("写入文件失败: {}", e))?;

        // 获取新的 mtime
        let new_metadata = fs::metadata(&path)
            .map_err(|e| anyhow::anyhow!("无法获取新文件信息: {}", e))?;
        let new_mtime = new_metadata
            .modified()
            .map_err(|e| anyhow::anyhow!("无法获取新修改时间: {}", e))?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("时间转换失败: {}", e))?
            .as_secs();

        Ok(new_mtime)
    });

    match result {
        Ok(new_mtime) => {
            tracing::info!("应用差异成功: path={}, new_mtime={}", req.path, new_mtime);
            WorkerResponse {
                payload: Some(worker_response::Payload::ApplyDiffResult(ApplyDiffResult {
                    path: req.path,
                    success: true,
                    new_mtime,
                    error: String::new(),
                })),
                ..Default::default()
            }
        }
        Err(e) => {
            tracing::error!("应用差异失败: path={}, error={}", req.path, e);
            WorkerResponse {
                payload: Some(worker_response::Payload::ApplyDiffResult(ApplyDiffResult {
                    path: req.path,
                    success: false,
                    new_mtime: 0,
                    error: e.to_string(),
                })),
                ..Default::default()
            }
        }
    }
}

/// 从 anyhow::Error 提取错误码和消息
///
/// 约定:
/// - "权限不足" → 403
/// - "不存在"/"not found" → 404
/// - "不是目录"/"不是文件" → 400
/// - 其他 → 500
fn error_to_code_message(e: &anyhow::Error) -> (u32, String) {
    let msg = e.to_string();
    if msg.contains("权限不足") || msg.contains("Permission denied") {
        (403, msg)
    } else if msg.contains("不存在") || msg.contains("not found") {
        (404, msg)
    } else if msg.contains("不是目录") || msg.contains("不是文件") || msg.contains("not a directory") || msg.contains("not a file") {
        (400, msg)
    } else {
        (500, msg)
    }
}

/// 格式化文件权限(与 handler.rs 对齐:9 字符,不含类型前缀)
#[cfg(unix)]
fn format_permissions(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    let mode = metadata.permissions().mode();
    let bits = [
        (0o400, 'r'), (0o200, 'w'), (0o100, 'x'),
        (0o040, 'r'), (0o020, 'w'), (0o010, 'x'),
        (0o004, 'r'), (0o002, 'w'), (0o001, 'x'),
    ];
    bits.iter()
        .map(|(mask, ch)| if mode & mask != 0 { *ch } else { '-' })
        .collect()
}

/// 格式化文件权限(非 Unix 平台)
#[cfg(not(unix))]
fn format_permissions(_metadata: &fs::Metadata) -> String {
    "rw-rw-rw-".into()
}

// ===== 属主/属组名称解析(手写解析 /etc/passwd 与 /etc/group,不引入新依赖) =====

/// 解析 /etc/passwd,返回 (用户名, uid) 列表
///
/// 格式: `name:x:uid:gid:gecos:home:shell`,取第 1、3 冒号分隔字段。
/// 解析失败(文件缺失/行格式异常)跳过该行,由调用方回退数字字符串。
fn parse_passwd_entries() -> Vec<(String, u32)> {
    parse_colon_separated("/etc/passwd")
}

/// 解析 /etc/group,返回 (组名, gid) 列表
///
/// 格式: `name:x:gid:members`,取第 1、3 冒号分隔字段。
fn parse_group_entries() -> Vec<(String, u32)> {
    parse_colon_separated("/etc/group")
}

/// /etc/passwd 与 /etc/group 的通用解析:
/// 每行按冒号分隔,第 0 段为名称、第 2 段为数字 ID
fn parse_colon_separated(file: &str) -> Vec<(String, u32)> {
    let content = match fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("读取 {} 失败,名称解析回退数字字符串: {}", file, e);
            return Vec::new();
        }
    };

    content
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let _x = fields.next()?; // 密码占位段(通常为 x)
            let id = fields.next()?.parse::<u32>().ok()?;
            Some((name.to_string(), id))
        })
        .collect()
}

/// 构建 uid → 用户名映射(目录列表显示属主用)
fn build_uid_name_map() -> HashMap<u32, String> {
    parse_passwd_entries().into_iter().map(|(name, uid)| (uid, name)).collect()
}

/// 构建 gid → 组名映射(目录列表显示属组用)
fn build_gid_name_map() -> HashMap<u32, String> {
    parse_group_entries().into_iter().map(|(name, gid)| (gid, name)).collect()
}

/// 用户名 → uid 解析(chown 用)
///
/// 解析失败(用户不存在)返回 None,由调用方返回确定性错误
pub fn resolve_uid_by_name(username: &str) -> Option<u32> {
    parse_passwd_entries()
        .into_iter()
        .find(|(name, _)| name == username)
        .map(|(_, uid)| uid)
}

/// 组名 → gid 解析(chown 用)
///
/// 解析失败(组不存在)返回 None,由调用方返回确定性错误
pub fn resolve_gid_by_name(group: &str) -> Option<u32> {
    parse_group_entries()
        .into_iter()
        .find(|(name, _)| name == group)
        .map(|(_, gid)| gid)
}

// ===== chmod/chown 执行辅助 =====

/// 对单个路径应用 chmod(友好的权限错误信息,交由 error_to_code_message 映射 403)
#[cfg(unix)]
fn chmod_one(path: &str, mode: u32) -> AnyhowResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("Permission denied") || msg.contains("Operation not permitted") {
            anyhow::anyhow!("权限不足: 无法修改 '{}' 的权限 (需要相应的 Linux 用户权限)", path)
        } else {
            anyhow::anyhow!("无法修改 '{}' 的权限: {}", path, e)
        }
    })
}

/// 对单个路径应用 chown(友好的权限错误信息)
///
/// 把 owner 改成其他用户仅 root 可成功,非 root 时 Linux 返回 EPERM(原生语义)
#[cfg(unix)]
fn chown_one(path: &str, uid: u32, gid: u32) -> AnyhowResult<()> {
    std::os::unix::fs::chown(path, Some(uid), Some(gid)).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("Permission denied") || msg.contains("Operation not permitted") {
            anyhow::anyhow!("权限不足: 无法修改 '{}' 的属主 (仅 root 可将文件转让给其他用户)", path)
        } else {
            anyhow::anyhow!("无法修改 '{}' 的属主: {}", path, e)
        }
    })
}

/// chmod 递归遍历目录内容(后序:先处理子内容再处理目录自身,
/// 避免先移除读权限导致无法遍历;遍历中跳过符号链接,与 chmod -R 行为一致)
#[cfg(unix)]
fn chmod_dir_contents(dir: &str, mode: u32) -> AnyhowResult<()> {
    for entry in fs::read_dir(dir).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("Permission denied") {
            anyhow::anyhow!("权限不足: 无法访问目录 '{}' (需要相应的 Linux 用户权限)", dir)
        } else {
            anyhow::anyhow!("无法读取目录 '{}': {}", dir, e)
        }
    })? {
        let entry = entry.map_err(|e| anyhow::anyhow!("读取目录项失败: {}", e))?;
        let file_type = entry.file_type().map_err(|e| anyhow::anyhow!("无法获取文件类型: {}", e))?;
        if file_type.is_symlink() {
            continue; // 递归遍历跳过符号链接(不修改链接目标)
        }
        let child = entry.path().to_string_lossy().to_string();
        if file_type.is_dir() {
            chmod_dir_contents(&child, mode)?; // 后序:先处理孙内容
            chmod_one(&child, mode)?;
        } else {
            chmod_one(&child, mode)?;
        }
    }
    Ok(())
}

/// chown 递归遍历目录内容(后序,跳过符号链接,与 chmod_dir_contents 同构)
#[cfg(unix)]
fn chown_dir_contents(dir: &str, uid: u32, gid: u32) -> AnyhowResult<()> {
    for entry in fs::read_dir(dir).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("Permission denied") {
            anyhow::anyhow!("权限不足: 无法访问目录 '{}' (需要相应的 Linux 用户权限)", dir)
        } else {
            anyhow::anyhow!("无法读取目录 '{}': {}", dir, e)
        }
    })? {
        let entry = entry.map_err(|e| anyhow::anyhow!("读取目录项失败: {}", e))?;
        let file_type = entry.file_type().map_err(|e| anyhow::anyhow!("无法获取文件类型: {}", e))?;
        if file_type.is_symlink() {
            continue; // 递归遍历跳过符号链接(不修改链接目标)
        }
        let child = entry.path().to_string_lossy().to_string();
        if file_type.is_dir() {
            chown_dir_contents(&child, uid, gid)?; // 后序:先处理孙内容
            chown_one(&child, uid, gid)?;
        } else {
            chown_one(&child, uid, gid)?;
        }
    }
    Ok(())
}

/// chmod 执行体:顶层路径先做存在性预检(友好 404),目录递归时后序应用
#[cfg(unix)]
fn apply_chmod(path: &str, mode: u32, recursive: bool) -> AnyhowResult<()> {
    let metadata = fs::metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("路径 '{}' 不存在", path)
        } else {
            anyhow::anyhow!("无法访问 '{}': {}", path, e)
        }
    })?;

    if metadata.is_dir() && recursive {
        chmod_dir_contents(path, mode)?; // 先处理目录内容
    }
    chmod_one(path, mode)?; // 最后处理顶层自身(递归时;非递归则仅此一步)
    Ok(())
}

/// chown 执行体:顶层路径先做存在性预检(友好 404),目录递归时后序应用
#[cfg(unix)]
fn apply_chown(path: &str, uid: u32, gid: u32, recursive: bool) -> AnyhowResult<()> {
    let metadata = fs::metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("路径 '{}' 不存在", path)
        } else {
            anyhow::anyhow!("无法访问 '{}': {}", path, e)
        }
    })?;

    if metadata.is_dir() && recursive {
        chown_dir_contents(path, uid, gid)?; // 先处理目录内容
    }
    chown_one(path, uid, gid)?; // 最后处理顶层自身(递归时;非递归则仅此一步)
    Ok(())
}

/// 非 Unix 平台:chmod 不支持(agent 仅部署于 Linux,此分支仅为编译兜底)
#[cfg(not(unix))]
fn apply_chmod(_path: &str, _mode: u32, _recursive: bool) -> AnyhowResult<()> {
    anyhow::bail!("chmod 仅支持 Unix 平台")
}

/// 非 Unix 平台:chown 不支持(agent 仅部署于 Linux,此分支仅为编译兜底)
#[cfg(not(unix))]
fn apply_chown(_path: &str, _uid: u32, _gid: u32, _recursive: bool) -> AnyhowResult<()> {
    anyhow::bail!("chown 仅支持 Unix 平台")
}
