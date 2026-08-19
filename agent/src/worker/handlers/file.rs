//! 文件操作处理器 - 处理文件和目录相关请求
//!
//! 该模块负责：
//! - 读取目录列表（ReadDir）
//! - 读取文件内容（ReadFile）
//! - 写入文件（WriteFile）
//! - 删除文件/目录（Delete）
//! - 创建目录（Mkdir）
//! - 重命名（Rename）
//! - 复制文件（Copy）
//! - 移动（Move）
//! - 检查文件存在（FileExists）
//! - 应用差异（ApplyDiff）
//
// 阶段 3 改造:集成 UserExecutor(fork+setuid)实现用户隔离
// 所有文件操作在目标用户上下文中执行,Linux 文件系统权限自动生效
// 返回格式与 handler.rs 对齐:
// - ReadDir: mtime 为 RFC3339 字符串, permissions 为 9 字符(不含类型前缀)
// - ReadFile: mtime 为 Unix 秒(u64)
// - WriteFile: mtime 为 Unix 秒(u64)

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result as AnyhowResult;
use chrono::{DateTime, Utc};

use crate::auth::{UserSession, UserExecutor};
use crate::diff::{FileDiff, DiffType, apply_diff};
use crate::protocol::generated::{
    ReadDir, DirListing, FileEntry,
    ReadFile, FileContent,
    WriteFile, WriteResult,
    Delete, DeleteResult,
    Mkdir, MkdirResult,
    Rename, RenameResult,
    Copy, CopyResult,
    Move, MoveResult,
    FileExists as FileExistsReq, FileExistsResult,
    ApplyDiff as ApplyDiffReq, ApplyDiffResult,
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

    // 序列化中间结果类型:(name, is_dir, size, mtime_rfc3339, permissions)
    type DirEntry = (String, bool, u64, String, String);

    let path = safe_path.as_str().to_string();
    let result: AnyhowResult<Vec<DirEntry>> = executor.execute_as_user(move || {
        let p = Path::new(&path);

        if !p.exists() {
            anyhow::bail!("Path not found: {}", path);
        }
        if !p.is_dir() {
            anyhow::bail!("Path is not a directory: {}", path);
        }

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

                Some((
                    name,
                    metadata.is_dir(),
                    if metadata.is_dir() { 0 } else { metadata.len() },
                    mtime,
                    format_permissions(&metadata),
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
                .map(|(name, is_dir, size, mtime, permissions)| FileEntry {
                    name,
                    is_dir,
                    size,
                    mtime,
                    permissions,
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

        // 读取原文件内容
        let old_content = fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("读取文件失败: {}", e))?;

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
