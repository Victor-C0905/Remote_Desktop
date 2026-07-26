use crate::config::AgentConfig;
use crate::protocol::{Envelope, FileEntry, MetricsSnapshot, MountInfo, Payload};
use crate::diff::{apply_diff, FileDiff}; // 只导入 apply_diff
use crate::transfer_session::{TransferSession, TransferStatus};
use crate::file_stream::{FileStreamReader, FileStreamWriter};
use crate::auth::{UserSession, UserExecutor}; // 新增：用户会话和执行器
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;
use uuid::Uuid;

// 全局传输会话管理器
lazy_static::lazy_static! {
    pub static ref TRANSFER_SESSIONS: Arc<Mutex<HashMap<String, TransferSession>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

// ============================================================================
// 权限检查函数
// ============================================================================

/// 检查路径权限
///
/// 验证用户是否有权访问指定路径，确保路径在用户的家目录范围内。
///
/// # 参数
/// - `path`: 要检查的路径
/// - `session`: 用户会话信息
///
/// # 返回
/// - `Ok(true)`: 有权限访问
/// - `Ok(false)`: 无权限访问
///
/// # 安全性
/// - 只允许访问用户家目录及其子目录
/// - 防止路径遍历攻击（如 `../`）
fn check_path_permission(path: &str, session: &UserSession) -> Result<bool, String> {
    let path = PathBuf::from(path);

    // 规范化路径，防止路径遍历攻击
    let canonical_path = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            // 路径不存在时，使用绝对路径检查
            if !path.is_absolute() {
                return Err(format!("路径必须是绝对路径: {}", path.display()));
            }
            path
        }
    };

    // 检查是否在用户家目录范围内
    if !canonical_path.starts_with(&session.home_dir) {
        tracing::warn!(
            "权限拒绝: 用户 {} 尝试访问路径 {:?}，家目录为 {:?}",
            session.username,
            canonical_path,
            session.home_dir
        );
        return Ok(false);
    }

    Ok(true)
}

#[tracing::instrument(skip(envelope, cfg, session), fields(request_id = envelope.request_id, payload_type = envelope.payload.type_name()))]
pub async fn handle_envelope(envelope: &Envelope, cfg: &AgentConfig, session: &UserSession) -> Envelope {
    match &envelope.payload {
        Payload::Ping { timestamp } => {
            let server_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            tracing::debug!("收到 ping, timestamp={}", timestamp);

            Envelope::new(
                envelope.request_id,
                Payload::Pong {
                    timestamp: *timestamp,
                    server_time,
                },
            )
        }

        Payload::AuthRequest { token } => {
            let success = !cfg.auth.token.is_empty() && token == &cfg.auth.token;
            let status_str = if success { "✅ 成功" } else { "❌ 失败" };
            tracing::info!(
                "认证请求: {} (expected: {}...)",
                status_str,
                &cfg.auth.token[..cfg.auth.token.len().min(8)]
            );

            Envelope::new(
                envelope.request_id,
                Payload::AuthResponse {
                    success,
                    error: if success {
                        None
                    } else {
                        Some("Token 无效".into())
                    },
                    session_id: None,
                },
            )
        }

        Payload::ReadDirRequest { path } => {
            tracing::info!("读取目录请求: {}", path);

            // 权限检查：验证用户是否有权访问该路径
            match check_path_permission(path, session) {
                Ok(true) => {
                    match handle_read_dir(path, cfg, session) {
                        Ok(entries) => {
                            tracing::info!("读取目录成功: {} ({} 个文件)", path, entries.len());
                            Envelope::new(
                                envelope.request_id,
                                Payload::ReadDirResponse {
                                    path: path.clone(),
                                    entries,
                                },
                            )
                        }
                        Err(e) => {
                            tracing::error!("读取目录失败: {} - {}", path, e);
                            error_response(envelope.request_id, &e)
                        }
                    }
                }
                Ok(false) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, path);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", path))
                }
                Err(e) => {
                    tracing::error!("权限检查失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        Payload::ReadFileRequest { path } => {
            tracing::info!("读取文件请求: {}", path);

            // 权限检查：验证用户是否有权访问该路径
            match check_path_permission(path, session) {
                Ok(true) => {
                    match handle_read_file(path, cfg, session) {
                        Ok((content, mtime, size)) => Envelope::new(
                            envelope.request_id,
                            Payload::ReadFileResponse {
                                path: path.clone(),
                                content,
                                mtime,
                                size,
                            },
                        ),
                        Err(e) => error_response(envelope.request_id, &e),
                    }
                }
                Ok(false) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, path);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", path))
                }
                Err(e) => {
                    tracing::error!("权限检查失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        Payload::WriteFileRequest { path, content } => {
            tracing::info!("写入文件请求: {}", path);

            // 权限检查：验证用户是否有权访问该路径
            match check_path_permission(path, session) {
                Ok(true) => {
                    match handle_write_file(path, content, cfg, session) {
                        Ok((mtime, size)) => Envelope::new(
                            envelope.request_id,
                            Payload::WriteFileResponse {
                                path: path.clone(),
                                mtime,
                                size,
                            },
                        ),
                        Err(e) => error_response(envelope.request_id, &e),
                    }
                }
                Ok(false) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, path);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", path))
                }
                Err(e) => {
                    tracing::error!("权限检查失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        Payload::DeleteRequest { path } => {
            tracing::info!("删除请求: {}", path);

            // 权限检查：验证用户是否有权访问该路径
            match check_path_permission(path, session) {
                Ok(true) => {
                    match handle_delete(path, cfg, session) {
                        Ok(_) => Envelope::new(
                            envelope.request_id,
                            Payload::DeleteResponse { success: true },
                        ),
                        Err(e) => error_response(envelope.request_id, &e),
                    }
                }
                Ok(false) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, path);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", path))
                }
                Err(e) => {
                    tracing::error!("权限检查失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        Payload::MkdirRequest { path } => {
            tracing::info!("创建目录请求: {}", path);

            // 权限检查：验证用户是否有权访问该路径
            match check_path_permission(path, session) {
                Ok(true) => {
                    match handle_mkdir(path, cfg, session) {
                        Ok(created_path) => {
                            tracing::info!("创建目录成功: {}", created_path);
                            Envelope::new(
                                envelope.request_id,
                                Payload::MkdirResponse {
                                    success: true,
                                    path: created_path,
                                },
                            )
                        }
                        Err(e) => {
                            tracing::error!("创建目录失败: {} - {}", path, e);
                            error_response(envelope.request_id, &e)
                        }
                    }
                }
                Ok(false) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, path);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", path))
                }
                Err(e) => {
                    tracing::error!("权限检查失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        Payload::RenameRequest { old_path, new_path } => {
            tracing::info!("重命名请求: {} -> {}", old_path, new_path);

            // 权限检查：验证用户是否有权访问源路径和目标路径
            let old_permitted = check_path_permission(old_path, session);
            let new_permitted = check_path_permission(new_path, session);

            match (old_permitted, new_permitted) {
                (Ok(true), Ok(true)) => {
                    match handle_rename(old_path, new_path, cfg, session) {
                        Ok((old, new)) => {
                            tracing::info!("重命名成功: {} -> {}", old, new);
                            Envelope::new(
                                envelope.request_id,
                                Payload::RenameResponse {
                                    success: true,
                                    old_path: old,
                                    new_path: new,
                                },
                            )
                        }
                        Err(e) => {
                            tracing::error!("重命名失败: {} - {}", old_path, e);
                            error_response(envelope.request_id, &e)
                        }
                    }
                }
                (Ok(false), _) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, old_path);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", old_path))
                }
                (_, Ok(false)) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, new_path);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", new_path))
                }
                (Err(e), _) | (_, Err(e)) => {
                    tracing::error!("权限检查失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        Payload::CopyRequest { src, dst } => {
            tracing::info!("复制请求: {} -> {}", src, dst);

            // 权限检查：验证用户是否有权访问源路径和目标路径
            let src_permitted = check_path_permission(src, session);
            let dst_permitted = check_path_permission(dst, session);

            match (src_permitted, dst_permitted) {
                (Ok(true), Ok(true)) => {
                    match handle_copy(src, dst, cfg, session) {
                        Ok((s, d)) => {
                            tracing::info!("复制成功: {} -> {}", s, d);
                            Envelope::new(
                                envelope.request_id,
                                Payload::CopyResponse {
                                    success: true,
                                    src: s,
                                    dst: d,
                                },
                            )
                        }
                        Err(e) => {
                            tracing::error!("复制失败: {} - {}", src, e);
                            error_response(envelope.request_id, &e)
                        }
                    }
                }
                (Ok(false), _) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, src);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", src))
                }
                (_, Ok(false)) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, dst);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", dst))
                }
                (Err(e), _) | (_, Err(e)) => {
                    tracing::error!("权限检查失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        Payload::MoveRequest { src, dst } => {
            tracing::info!("移动请求: {} -> {}", src, dst);

            // 权限检查：验证用户是否有权访问源路径和目标路径
            let src_permitted = check_path_permission(src, session);
            let dst_permitted = check_path_permission(dst, session);

            match (src_permitted, dst_permitted) {
                (Ok(true), Ok(true)) => {
                    match handle_move(src, dst, cfg, session) {
                        Ok((s, d)) => {
                            tracing::info!("移动成功: {} -> {}", s, d);
                            Envelope::new(
                                envelope.request_id,
                                Payload::MoveResponse {
                                    success: true,
                                    src: s,
                                    dst: d,
                                },
                            )
                        }
                        Err(e) => {
                            tracing::error!("移动失败: {} - {}", src, e);
                            error_response(envelope.request_id, &e)
                        }
                    }
                }
                (Ok(false), _) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, src);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", src))
                }
                (_, Ok(false)) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, dst);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", dst))
                }
                (Err(e), _) | (_, Err(e)) => {
                    tracing::error!("权限检查失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        Payload::MetricsSubscribeRequest {} => {
            match collect_metrics() {
                Ok(metrics) => Envelope::new(
                    envelope.request_id,
                    Payload::MetricsData(metrics),
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }

        Payload::TerminalSpawnRequest { shell, cols, rows, .. } => {
            tracing::info!("终端请求: shell={}, cols={}, rows={}", shell, cols, rows);
            // 终端创建需要在 QUIC Stream 异步处理（持久双向隧道）
            error_response(envelope.request_id, "终端创建需要在 QUIC Stream 异步处理")
        }

        Payload::TerminalData { session_id, data, is_input } => {
            tracing::debug!("终端数据: session={}, len={}, is_input={}", session_id, data.len(), is_input);
            // 终端数据需要在持久 Stream 中处理
            error_response(envelope.request_id, "终端数据需要在持久 Stream 中处理")
        }

        // 文件编辑器差异同步（Agent 只负责应用差异）
        Payload::ApplyDiffRequest { path, base_mtime, diffs } => {
            tracing::info!("应用差异请求: {} (base_mtime={})", path, base_mtime);

            // 权限检查：验证用户是否有权访问该路径
            match check_path_permission(path, session) {
                Ok(true) => {
                    match handle_apply_diff(path, *base_mtime, diffs, cfg, session) {
                        Ok(new_mtime) => {
                            tracing::info!("应用差异成功: {} (new_mtime={})", path, new_mtime);
                            Envelope::new(
                                envelope.request_id,
                                Payload::ApplyDiffResponse {
                                    path: path.clone(),
                                    success: true,
                                    new_mtime,
                                    error: None,
                                },
                            )
                        }
                        Err(e) => {
                            tracing::error!("应用差异失败: {} - {}", path, e);
                            Envelope::new(
                                envelope.request_id,
                                Payload::ApplyDiffResponse {
                                    path: path.clone(),
                                    success: false,
                                    new_mtime: 0,
                                    error: Some(e),
                                },
                            )
                        }
                    }
                }
                Ok(false) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, path);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", path))
                }
                Err(e) => {
                    tracing::error!("权限检查失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        Payload::GetCurrentUser => {
            tracing::info!("获取当前用户请求");
            let username = handle_get_current_user();
            tracing::info!("当前用户: {}", username);
            Envelope::new(
                envelope.request_id,
                Payload::CurrentUserResponse { username },
            )
        }

        Payload::GetMounts => {
            tracing::info!("获取挂载点列表请求");
            let mounts = handle_get_mounts();
            tracing::info!("挂载点数量: {}", mounts.len());
            Envelope::new(
                envelope.request_id,
                Payload::MountsResponse { mounts },
            )
        }

        // 新增：订阅处理（暂时返回错误响应，后续在 QUIC Server 中完善）
        Payload::Subscribe { server_id, types } => {
            tracing::info!("订阅请求: server_id={}, types={}", server_id, types.len());
            // 注意：订阅处理需要 SubscriptionManager，需要在 QUIC Server 中异步处理
            // 这里暂时返回错误响应
            error_response(envelope.request_id, "订阅功能需要在 QUIC Server 中异步处理")
        }

        // 新增：取消订阅处理（暂时返回错误响应，后续在 QUIC Server 中完善）
        Payload::Unsubscribe { server_id, types } => {
            tracing::info!("取消订阅请求: server_id={}, types={}", server_id, types.len());
            // 注意：取消订阅处理需要 SubscriptionManager，需要在 QUIC Server 中异步处理
            // 这里暂时返回错误响应
            error_response(envelope.request_id, "取消订阅功能需要在 QUIC Server 中异步处理")
        }

        // ===== 文件传输处理 =====

        // 文件传输请求
        Payload::FileTransferRequest { direction, path, file_size, chunk_size, resume_from } => {
            tracing::info!("文件传输请求: direction={:?}, path={}, resume_from={:?}", direction, path, resume_from);

            // 权限检查：验证用户是否有权访问该路径
            match check_path_permission(path, session) {
                Ok(true) => {
                    match handle_file_transfer_request(envelope.request_id, direction, path, *file_size, *chunk_size, *resume_from, cfg, session).await {
                        Ok(response) => response,
                        Err(e) => {
                            tracing::error!("文件传输请求失败: {}", e);
                            error_response(envelope.request_id, &e)
                        }
                    }
                }
                Ok(false) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, path);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", path))
                }
                Err(e) => {
                    tracing::error!("权限检查失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        // 文件数据块
        Payload::FileChunk { session_id, seq, data, size } => {
            tracing::debug!("文件数据块: session_id={}, seq={}, size={}", session_id, seq, size);
            match handle_file_chunk(envelope.request_id, session_id, *seq, data, cfg).await {
                Ok(response) => response,
                Err(e) => {
                    tracing::error!("处理数据块失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        // 文件传输完成
        Payload::FileTransferComplete { session_id, success, mtime: _, error: _ } => {
            tracing::info!("文件传输完成: session_id={}, success={}", session_id, success);
            match handle_file_transfer_complete(envelope.request_id, session_id, *success, cfg).await {
                Ok(response) => response,
                Err(e) => {
                    tracing::error!("处理传输完成失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        // 检查文件是否存在
        Payload::FileExistsRequest { path } => {
            tracing::info!("检查文件是否存在: {}", path);

            // 权限检查：验证用户是否有权访问该路径
            match check_path_permission(path, session) {
                Ok(true) => {
                    match handle_file_exists(path, cfg, session) {
                        Ok(response) => response,
                        Err(e) => {
                            tracing::error!("检查文件失败: {}", e);
                            error_response(envelope.request_id, &e)
                        }
                    }
                }
                Ok(false) => {
                    tracing::warn!("权限拒绝: 用户 {} 无权访问路径 {}", session.username, path);
                    error_response(envelope.request_id, &format!("权限不足: 无法访问路径 '{}'", path))
                }
                Err(e) => {
                    tracing::error!("权限检查失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        // 取消文件传输
        Payload::CancelFileTransfer { session_id, reason } => {
            tracing::info!("取消文件传输: session_id={}, reason={}", session_id, reason);
            match handle_cancel_file_transfer(envelope.request_id, session_id, cfg).await {
                Ok(response) => response,
                Err(e) => {
                    tracing::error!("取消传输失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        other => {
            tracing::warn!("未处理的消息类型: {:?}", std::mem::discriminant(other));
            error_response(envelope.request_id, "未知的消息类型")
        }
    }
}

fn handle_get_current_user() -> String {
    whoami::username()
}

fn handle_get_mounts() -> Vec<MountInfo> {
    use sysinfo::Disks;

    let disks = Disks::new_with_refreshed_list();
    disks
        .iter()
        .map(|disk| MountInfo {
            mount_point: disk.mount_point().to_string_lossy().to_string(),
            device: disk.name().to_string_lossy().to_string(),
            filesystem: disk.file_system().to_string_lossy().to_string(),
            total_bytes: disk.total_space(),
            used_bytes: disk.total_space() - disk.available_space(),
        })
        .collect()
}

fn handle_read_dir(path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<Vec<FileEntry>, String> {
    // 如果 allowed_paths 不为空，则检查白名单
    // 如果 allowed_paths 为空，则不限制，依赖 Linux 文件系统权限
    if !cfg.security.allowed_paths.is_empty() {
        let allowed = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| path.starts_with(prefix));
        if !allowed {
            return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
        }
    }

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    let path = path.to_string(); // 克隆为 String,以便移动到闭包
    executor.execute_as_user(move || {
        // 尝试读取目录，依赖 Linux 文件系统权限
        let entries = fs::read_dir(&path)
            .map_err(|e| {
                let error_msg = e.to_string();
                if error_msg.contains("Permission denied") {
                    anyhow::anyhow!("权限不足: 无法访问目录 '{}' (需要相应的 Linux 用户权限)", path)
                } else {
                    anyhow::anyhow!("无法读取目录 '{}': {}", path, e)
                }
            })?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let metadata = entry.metadata().ok()?;
                let name = entry.file_name().to_string_lossy().to_string();

                let mtime = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| {
                        let dt = chrono::DateTime::<chrono::Utc>::from(SystemTime::UNIX_EPOCH + d);
                        dt.to_rfc3339()
                    })
                    .unwrap_or_default();

                Some(FileEntry {
                    name,
                    is_dir: metadata.is_dir(),
                    size: if metadata.is_dir() { 0 } else { metadata.len() },
                    mtime,
                    permissions: format_permissions(&metadata),
                })
            })
            .collect();

        Ok(entries)
    }).map_err(|e| e.to_string())
}

fn handle_read_file(path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<(String, u64, u64), String> {
    tracing::debug!("[handle_read_file] path={}", path);
    // 如果 allowed_paths 不为空，则检查白名单
    // 如果 allowed_paths 为空，则不限制，依赖 Linux 文件系统权限
    if !cfg.security.allowed_paths.is_empty() {
        let allowed = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| path.starts_with(prefix));
        if !allowed {
            return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
        }
    }

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    let path = path.to_string(); // 克隆为 String,以便移动到闭包
    executor.execute_as_user(move || {
        let metadata = fs::metadata(&path)
            .map_err(|e| {
                let error_msg = e.to_string();
                tracing::warn!("[handle_read_file] 访问文件失败: path={}, error={}", path, e);
                if error_msg.contains("Permission denied") {
                    anyhow::anyhow!("权限不足: 无法访问文件 '{}' (需要相应的 Linux 用户权限)", path)
                } else {
                    anyhow::anyhow!("无法访问文件 '{}': {}", path, e)
                }
            })?;

        if metadata.is_dir() {
            anyhow::bail!("这是一个目录，不能作为文件读取");
        }

        const MAX_SIZE: u64 = 10 * 1024 * 1024; // 10MB
        if metadata.len() > MAX_SIZE {
            anyhow::bail!("文件太大 ({}MB)，限制 10MB", metadata.len() / (1024 * 1024));
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| {
                tracing::warn!("[handle_read_file] 读取文件失败: path={}, error={}", path, e);
                anyhow::anyhow!("读取文件失败: {}", e)
            })?;

        // 获取文件修改时间（mtime）
        let mtime = metadata
            .modified()
            .map_err(|e| {
                tracing::warn!("[handle_read_file] 获取修改时间失败: path={}, error={}", path, e);
                anyhow::anyhow!("无法获取修改时间: {}", e)
            })?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| {
                tracing::warn!("[handle_read_file] 时间转换失败: path={}, error={}", path, e);
                anyhow::anyhow!("时间转换失败: {}", e)
            })?
            .as_secs();

        Ok((content, mtime, metadata.len()))
    }).map_err(|e| e.to_string())
}

fn handle_write_file(path: &str, content: &str, cfg: &AgentConfig, session: &UserSession) -> Result<(u64, u64), String> {
    tracing::info!("[handle_write_file] path={}", path);
    // 如果 allowed_paths 不为空，则检查白名单
    // 如果 allowed_paths 为空，则不限制，依赖 Linux 文件系统权限
    if !cfg.security.allowed_paths.is_empty() {
        let allowed = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| path.starts_with(prefix));
        if !allowed {
            return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
        }
    }

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    let path = path.to_string(); // 克隆为 String,以便移动到闭包
    let content = content.to_string(); // 移动到闭包外部
    executor.execute_as_user(move || {
        fs::write(&path, &content)
            .map_err(|e| {
                let error_msg = e.to_string();
                tracing::warn!("[handle_write_file] 写入失败: path={}, error={}", path, e);
                if error_msg.contains("Permission denied") {
                    anyhow::anyhow!("权限不足: 无法写入文件 '{}' (需要相应的 Linux 用户权限)", path)
                } else {
                    anyhow::anyhow!("写入文件失败: {}", e)
                }
            })?;

        // 获取写入后的 mtime
        let metadata = fs::metadata(&path)
            .map_err(|e| {
                tracing::warn!("[handle_write_file] 获取文件信息失败: path={}, error={}", path, e);
                anyhow::anyhow!("无法获取文件信息: {}", e)
            })?;
        let mtime = metadata
            .modified()
            .map_err(|e| {
                tracing::warn!("[handle_write_file] 获取修改时间失败: path={}, error={}", path, e);
                anyhow::anyhow!("无法获取修改时间: {}", e)
            })?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| {
                tracing::warn!("[handle_write_file] 时间转换失败: path={}, error={}", path, e);
                anyhow::anyhow!("时间转换失败: {}", e)
            })?
            .as_secs();

        Ok((mtime, content.len() as u64))
    }).map_err(|e| e.to_string())
}

/// 应用差异并写入文件（用于流量优化，无状态化）
///
/// # 参数
/// - `path`: 文件路径
/// - `base_mtime`: 基准 mtime（客户端缓存的版本）
/// - `diffs`: 差异列表（客户端计算）
/// - `cfg`: Agent 配置
/// - `session`: 用户会话信息
///
/// # 返回
/// - `new_mtime`: 新的 mtime（写入后）
#[tracing::instrument(skip(diffs, cfg, session), fields(path = %path, base_mtime = base_mtime, diffs_count = diffs.len()))]
fn handle_apply_diff(
    path: &str,
    base_mtime: u64,
    diffs: &[FileDiff],
    cfg: &AgentConfig,
    session: &UserSession,
) -> Result<u64, String> {
    tracing::debug!("[handle_apply_diff] 开始处理: path={}, base_mtime={}, diffs_count={}",
        path, base_mtime, diffs.len());

    // 检查路径权限
    if !cfg.security.allowed_paths.is_empty() {
        let allowed = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| path.starts_with(prefix));
        if !allowed {
            return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
        }
    }

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    let path = path.to_string(); // 克隆为 String,以便移动到闭包
    let diffs = diffs.to_vec(); // 移动到闭包外部
    executor.execute_as_user(move || {
        // 获取文件当前 mtime
        let metadata = fs::metadata(&path)
            .map_err(|e| anyhow::anyhow!("无法获取文件信息 '{}': {}", path, e))?;
        let current_mtime = metadata
            .modified()
            .map_err(|e| anyhow::anyhow!("无法获取修改时间: {}", e))?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("时间转换失败: {}", e))?
            .as_secs();

        tracing::debug!("[handle_apply_diff] Mtime 检查: expected={}, actual={}, match={}",
            base_mtime, current_mtime, base_mtime == current_mtime);

        // 检查版本冲突（mtime 验证）
        if current_mtime != base_mtime {
            tracing::warn!("[handle_apply_diff] Mtime 不匹配，返回错误");
            anyhow::bail!(
                "文件版本冲突: 期望 mtime={}, 实际 mtime={}",
                base_mtime, current_mtime
            );
        }

        tracing::debug!("[handle_apply_diff] Mtime 匹配，继续应用差异");

        // 读取原文件内容
        let old_content = fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("读取文件失败: {}", e))?;

        tracing::debug!("[handle_apply_diff] 读取文件: {} bytes, {} lines",
            old_content.len(), old_content.lines().count());

        // 应用差异
        let new_content = apply_diff(&old_content, &diffs);

        tracing::debug!("[handle_apply_diff] 应用差异后: {} bytes, {} lines",
            new_content.len(), new_content.lines().count());

        // 写入文件
        fs::write(&path, &new_content)
            .map_err(|e| anyhow::anyhow!("写入文件失败: {}", e))?;

        tracing::info!("[handle_apply_diff] 文件写入成功");

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
    }).map_err(|e| e.to_string())
}

fn handle_delete(path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<(), String> {
    tracing::info!("[handle_delete] path={}", path);
    // 如果 allowed_paths 不为空，则检查白名单
    // 如果 allowed_paths 为空，则不限制，依赖 Linux 文件系统权限
    if !cfg.security.allowed_paths.is_empty() {
        let allowed = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| path.starts_with(prefix));
        if !allowed {
            return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
        }
    }

    // 检查是否在禁止删除的路径
    for blocked in &cfg.security.blocked_commands {
        if path.contains(blocked) {
            return Err(format!("禁止删除: 路径包含敏感内容 ({})", path));
        }
    }

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    let path = path.to_string(); // 克隆为 String,以便移动到闭包
    executor.execute_as_user(move || {
        let metadata = fs::metadata(&path)
            .map_err(|e| {
                tracing::warn!("[handle_delete] 访问失败: path={}, error={}", path, e);
                anyhow::anyhow!("无法访问 '{}': {}", path, e)
            })?;

        if metadata.is_dir() {
            fs::remove_dir_all(&path)
                .map_err(|e| {
                    tracing::warn!("[handle_delete] 删除目录失败: path={}, error={}", path, e);
                    anyhow::anyhow!("删除目录失败: {}", e)
                })?;
        } else {
            fs::remove_file(&path)
                .map_err(|e| {
                    tracing::warn!("[handle_delete] 删除文件失败: path={}, error={}", path, e);
                    anyhow::anyhow!("删除文件失败: {}", e)
                })?;
        }

        Ok(())
    }).map_err(|e| e.to_string())
}

fn handle_mkdir(path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<String, String> {
    // 如果 allowed_paths 不为空，则检查白名单
    // 如果 allowed_paths 为空，则不限制，依赖 Linux 文件系统权限
    if !cfg.security.allowed_paths.is_empty() {
        let allowed = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| path.starts_with(prefix));
        if !allowed {
            return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
        }
    }

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    let path = path.to_string(); // 克隆为 String,以便移动到闭包
    executor.execute_as_user(move || {
        fs::create_dir_all(&path)
            .map_err(|e| {
                let error_msg = e.to_string();
                if error_msg.contains("Permission denied") {
                    anyhow::anyhow!("权限不足: 无法创建目录 '{}' (需要相应的 Linux 用户权限)", path)
                } else {
                    anyhow::anyhow!("无法创建目录 '{}': {}", path, e)
                }
            })?;

        Ok(path.clone())
    }).map_err(|e| e.to_string())
}

fn handle_rename(old_path: &str, new_path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<(String, String), String> {
    // 如果 allowed_paths 不为空，则检查白名单
    // 如果 allowed_paths 为空，则不限制，依赖 Linux 文件系统权限
    if !cfg.security.allowed_paths.is_empty() {
        let allowed_old = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| old_path.starts_with(prefix));
        let allowed_new = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| new_path.starts_with(prefix));
        if !allowed_old || !allowed_new {
            return Err(format!("访问被拒绝: 不在允许的路径列表中"));
        }
    }

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    let old_path = old_path.to_string(); // 克隆为 String,以便移动到闭包
    let new_path = new_path.to_string(); // 克隆为 String,以便移动到闭包
    executor.execute_as_user(move || {
        fs::rename(&old_path, &new_path)
            .map_err(|e| {
                let error_msg = e.to_string();
                if error_msg.contains("Permission denied") {
                    anyhow::anyhow!("权限不足: 无法重命名 '{}' (需要相应的 Linux 用户权限)", old_path)
                } else {
                    anyhow::anyhow!("无法重命名 '{}': {}", old_path, e)
                }
            })?;

        Ok((old_path.clone(), new_path.clone()))
    }).map_err(|e| e.to_string())
}

fn handle_copy(src: &str, dst: &str, cfg: &AgentConfig, session: &UserSession) -> Result<(String, String), String> {
    tracing::info!("[handle_copy] src={}, dst={}", src, dst);
    // 如果 allowed_paths 不为空，则检查白名单
    // 如果 allowed_paths 为空，则不限制，依赖 Linux 文件系统权限
    if !cfg.security.allowed_paths.is_empty() {
        let allowed_src = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| src.starts_with(prefix));
        let allowed_dst = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| dst.starts_with(prefix));
        if !allowed_src || !allowed_dst {
            return Err(format!("访问被拒绝: 不在允许的路径列表中"));
        }
    }

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    let src = src.to_string(); // 克隆为 String,以便移动到闭包
    let dst = dst.to_string(); // 克隆为 String,以便移动到闭包
    executor.execute_as_user(move || {
        // 检查源文件是否存在
        let metadata = fs::metadata(&src)
            .map_err(|e| {
                let error_msg = e.to_string();
                tracing::warn!("[handle_copy] 访问源文件失败: src={}, error={}", src, e);
                if error_msg.contains("Permission denied") {
                    anyhow::anyhow!("权限不足: 无法访问源文件 '{}' (需要相应的 Linux 用户权限)", src)
                } else if error_msg.contains("No such file") {
                    anyhow::anyhow!("源文件 '{}' 不存在", src)
                } else {
                    anyhow::anyhow!("无法访问源文件 '{}': {}", src, e)
                }
            })?;

        // 只支持文件复制，不支持目录复制
        if metadata.is_dir() {
            anyhow::bail!("不支持复制目录 '{}' (请使用移动功能)", src);
        }

        fs::copy(&src, &dst)
            .map_err(|e| {
                let error_msg = e.to_string();
                tracing::warn!("[handle_copy] 复制失败: src={}, dst={}, error={}", src, dst, e);
                if error_msg.contains("Permission denied") {
                    anyhow::anyhow!("权限不足: 无法复制 '{}' (需要相应的 Linux 用户权限)", src)
                } else {
                    anyhow::anyhow!("无法复制 '{}': {}", src, e)
                }
            })?;

        Ok((src.clone(), dst.clone()))
    }).map_err(|e| e.to_string())
}

fn handle_move(src: &str, dst: &str, cfg: &AgentConfig, session: &UserSession) -> Result<(String, String), String> {
    // move 本质上是 rename
    handle_rename(src, dst, cfg, session)
}

#[cfg(unix)]
fn format_permissions(metadata: &std::fs::Metadata) -> String {
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

#[cfg(not(unix))]
fn format_permissions(_metadata: &std::fs::Metadata) -> String {
    "rw-rw-rw-".into()
}

fn collect_metrics() -> Result<MetricsSnapshot, String> {
    use sysinfo::{Disks, Networks, System};

    let mut sys = System::new_all();
    sys.refresh_all();

    let mem_used = sys.used_memory();
    let mem_total = sys.total_memory();
    let cpu_percent = sys.global_cpu_usage();

    let disks_obj = Disks::new_with_refreshed_list();
    let disks: Vec<_> = disks_obj
        .iter()
        .map(|d| crate::protocol::DiskInfo {
            mount_point: d.mount_point().to_string_lossy().to_string(),
            total_bytes: d.total_space(),
            used_bytes: d.total_space() - d.available_space(),
        })
        .collect();

    let networks = Networks::new_with_refreshed_list();
    let mut network_rx = 0u64;
    let mut network_tx = 0u64;
    for (_name, data) in &networks {
        network_rx += data.received();
        network_tx += data.transmitted();
    }

    Ok(MetricsSnapshot {
        cpu_percent,
        mem_used_bytes: mem_used,
        mem_total_bytes: mem_total,
        swap_used_bytes: sys.used_swap(),
        disks,
        network_rx_bytes: network_rx,
        network_tx_bytes: network_tx,
        uptime_secs: System::uptime() as u64,
    })
}

fn error_response(request_id: u32, message: &str) -> Envelope {
    Envelope::new(
        request_id,
        Payload::Error {
            code: -1,
            message: message.into(),
        },
    )
}

// ===== 文件传输处理函数 =====

/// 处理文件传输请求
///
/// # 参数
/// - `request_id`: 请求 ID
/// - `direction`: 传输方向（"upload" 或 "download"）
/// - `path`: 文件路径
/// - `file_size`: 文件大小（上传时提供）
/// - `chunk_size`: 分块大小（可选）
/// - `cfg`: Agent 配置
/// - `session`: 用户会话信息
///
/// # 返回
/// - `Ok(Envelope)`: FileTransferAccept 响应
/// - `Err(String)`: 错误信息
pub async fn handle_file_transfer_request(
    request_id: u32,
    direction: &str,
    path: &str,
    file_size: Option<u64>,
    chunk_size: Option<u32>,
    resume_from: Option<u64>,
    cfg: &AgentConfig,
    session: &UserSession,
) -> Result<Envelope, String> {
    // 1. 检查路径权限（allowed_paths）
    if !cfg.security.allowed_paths.is_empty() {
        let allowed = cfg
            .security
            .allowed_paths
            .iter()
            .any(|prefix| path.starts_with(prefix));
        if !allowed {
            return Err(format!("访问被拒绝: 不在允许的路径列表中 ({})", path));
        }
    }

    // 2. 生成 session_id
    let session_id = format!("transfer-{}", Uuid::new_v4());

    // 3. 根据方向处理
    match direction {
        "upload" => {
            // 上传：客户端上传文件到 Agent
            // 需要 file_size 参数
            let file_size = file_size.ok_or("上传文件必须提供 file_size 参数".to_string())?;

            // 检查文件大小限制
            let max_size = cfg.limits.max_file_transfer_mb * 1024 * 1024;
            if file_size > max_size {
                return Err(format!(
                    "文件大小超过限制: {}MB > {}MB",
                    file_size / (1024 * 1024),
                    cfg.limits.max_file_transfer_mb
                ));
            }

            // 默认分块大小 64KB
            let chunk_size = chunk_size.unwrap_or(64 * 1024);

            // 使用 UserExecutor 在用户上下文中执行操作
            let executor = UserExecutor::new(session);
            let path_str = path.to_string();
            let session_id_clone = session_id.clone();
            let writer = executor.execute_as_user(move || {
                // 尝试断点续传
                let writer = if let Some(resume_from) = resume_from {
                    // 尝试从断点续传
                    match FileStreamWriter::with_resume(&path_str, file_size, resume_from) {
                        Ok(writer) => {
                            tracing::info!(
                                "上传断点续传: session_id={}, path={}, resume_from={}",
                                session_id_clone,
                                path_str,
                                resume_from
                            );
                            writer
                        }
                        Err(e) => {
                            // 断点续传失败，降级为重新传输
                            tracing::warn!(
                                "断点续传失败，降级为重新传输: {}",
                                e
                            );
                            FileStreamWriter::new(&path_str, file_size).map_err(|e| anyhow::anyhow!("{}", e))?
                        }
                    }
                } else {
                    // 从头开始传输
                    FileStreamWriter::new(&path_str, file_size).map_err(|e| anyhow::anyhow!("{}", e))?
                };
                Ok(writer)
            }).map_err(|e| e.to_string())?;

            // 创建传输会话
            let mut transfer_session = TransferSession::new(
                session_id.clone(),
                direction.to_string(),
                path.to_string(),
                file_size,
                chunk_size,
            );
            transfer_session.writer = Some(writer);

            // 保存到全局会话管理器
            let mut sessions = TRANSFER_SESSIONS.lock().await;
            sessions.insert(session_id.clone(), transfer_session);

            tracing::info!("创建上传会话: session_id={}, path={}, size={}", session_id, path, file_size);

            // 返回接受响应
            Ok(Envelope::new(
                request_id,
                Payload::FileTransferAccept {
                    session_id,
                    file_size,
                    chunk_size,
                    mtime: None,
                },
            ))
        }

        "download" => {
            // 下载：Agent 发送文件到客户端
            // 使用 UserExecutor 在用户上下文中执行操作
            let executor = UserExecutor::new(session);
            let path_str = path.to_string();
            let session_id_clone = session_id.clone();
            let chunk_size_value = chunk_size.unwrap_or(64 * 1024);
            let (file_size, mtime, reader) = executor.execute_as_user(move || {
                // 获取文件元数据
                let metadata = fs::metadata(&path_str)
                    .map_err(|e| {
                        let error_msg = e.to_string();
                        if error_msg.contains("Permission denied") {
                            anyhow::anyhow!("权限不足: 无法访问文件 '{}' (需要相应的 Linux 用户权限)", path_str)
                        } else if error_msg.contains("No such file") {
                            anyhow::anyhow!("文件 '{}' 不存在", path_str)
                        } else {
                            anyhow::anyhow!("无法访问文件 '{}': {}", path_str, e)
                        }
                    })?;

                if metadata.is_dir() {
                    anyhow::bail!("路径是目录，不能下载");
                }

                let file_size = metadata.len();
                let mtime = metadata
                    .modified()
                    .map_err(|e| anyhow::anyhow!("无法获取修改时间: {}", e))?
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| anyhow::anyhow!("时间转换失败: {}", e))?
                    .as_secs();

                // 尝试断点续传
                let reader = if let Some(resume_from) = resume_from {
                    // 尝试从断点续传
                    match FileStreamReader::with_resume(&path_str, file_size, resume_from) {
                        Ok(reader) => {
                            tracing::info!(
                                "下载断点续传: session_id={}, path={}, resume_from={}",
                                session_id_clone,
                                path_str,
                                resume_from
                            );
                            reader
                        }
                        Err(e) => {
                            // 断点续传失败，降级为重新传输
                            tracing::warn!(
                                "断点续传失败，降级为重新传输: {}",
                                e
                            );
                            FileStreamReader::new(&path_str, file_size).map_err(|e| anyhow::anyhow!("{}", e))?
                        }
                    }
                } else {
                    // 从头开始传输
                    FileStreamReader::new(&path_str, file_size).map_err(|e| anyhow::anyhow!("{}", e))?
                };

                Ok((file_size, mtime, reader))
            }).map_err(|e| e.to_string())?;

            // 默认分块大小 64KB
            let chunk_size = chunk_size.unwrap_or(64 * 1024);

            // 创建传输会话
            let mut transfer_session = TransferSession::new(
                session_id.clone(),
                direction.to_string(),
                path.to_string(),
                file_size,
                chunk_size,
            );
            transfer_session.reader = Some(reader);

            // 保存到全局会话管理器
            let mut sessions = TRANSFER_SESSIONS.lock().await;
            sessions.insert(session_id.clone(), transfer_session);

            tracing::info!("创建下载会话: session_id={}, path={}, size={}", session_id, path, file_size);

            // 返回接受响应
            Ok(Envelope::new(
                request_id,
                Payload::FileTransferAccept {
                    session_id,
                    file_size,
                    chunk_size,
                    mtime: Some(mtime),
                },
            ))
        }

        _ => {
            Err(format!("无效的传输方向: {}", direction))
        }
    }
}

/// 处理文件数据块（上传时使用）
///
/// # 参数
/// - `request_id`: 请求 ID
/// - `session_id`: 传输会话 ID
/// - `seq`: 块序号（从 1 开始）
/// - `data`: 数据块
/// - `cfg`: Agent 配置
///
/// # 返回
/// - `Ok(Envelope)`: FileChunk 响应（确认）
/// - `Err(String)`: 错误信息
async fn handle_file_chunk(
    request_id: u32,
    session_id: &str,
    seq: u32,
    data: &[u8],
    _cfg: &AgentConfig,
) -> Result<Envelope, String> {
    // 1. 从 TRANSFER_SESSIONS 获取会话
    let mut sessions = TRANSFER_SESSIONS.lock().await;

    let session = sessions
        .get_mut(session_id)
        .ok_or_else(|| format!("传输会话不存在: {}", session_id))?;

    // 2. 检查会话状态是否为 Active
    if session.status != TransferStatus::Active {
        return Err(format!("传输会话状态异常: {:?}", session.status));
    }

    // 3. 写入数据块到文件
    if let Some(writer) = session.writer.as_mut() {
        writer.write_chunk(data)?;

        // 4. 更新 transferred 字段
        session.transferred = writer.transferred();

        tracing::debug!(
            "写入数据块: session_id={}, seq={}, size={}, transferred={}/{}",
            session_id,
            seq,
            data.len(),
            session.transferred,
            session.file_size
        );
    } else {
        return Err("传输会话没有 writer".to_string());
    }

    // 5. 返回 FileChunk 响应（确认）
    Ok(Envelope::new(
        request_id,
        Payload::FileChunk {
            session_id: session_id.to_string(),
            seq,
            data: vec![], // 确认响应不需要数据
            size: data.len() as u32,
        },
    ))
}

/// 处理文件传输完成
///
/// # 参数
/// - `request_id`: 请求 ID
/// - `session_id`: 传输会话 ID
/// - `success`: 是否成功
/// - `cfg`: Agent 配置
///
/// # 返回
/// - `Ok(Envelope)`: FileTransferComplete 响应
/// - `Err(String)`: 错误信息
async fn handle_file_transfer_complete(
    request_id: u32,
    session_id: &str,
    success: bool,
    _cfg: &AgentConfig,
) -> Result<Envelope, String> {
    // 1. 从 TRANSFER_SESSIONS 获取会话
    let mut sessions = TRANSFER_SESSIONS.lock().await;

    let mut session = sessions
        .remove(session_id)
        .ok_or_else(|| format!("传输会话不存在: {}", session_id))?;

    // 2. 根据成功状态处理
    let mtime = if success {
        // 成功：finish() 写入，获取 mtime
        match session.direction.as_str() {
            "upload" => {
                // 上传完成：写入文件
                let mut writer = session.writer.take().ok_or("传输会话没有 writer")?;
                writer.finish()?;

                // 获取文件修改时间
                let metadata = fs::metadata(&session.path)
                    .map_err(|e| format!("无法获取文件信息: {}", e))?;
                let mtime = metadata
                    .modified()
                    .map_err(|e| format!("无法获取修改时间: {}", e))?
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| format!("时间转换失败: {}", e))?
                    .as_secs();

                tracing::info!("文件上传完成: path={}, size={}", session.path, session.file_size);
                Some(mtime)
            }
            "download" => {
                // 下载完成：不需要特殊处理
                tracing::info!("文件下载完成: path={}", session.path);
                None
            }
            _ => None,
        }
    } else {
        // 失败：abort() 清理临时文件
        match session.direction.as_str() {
            "upload" => {
                if let Some(mut writer) = session.writer.take() {
                    writer.abort();
                    tracing::info!("文件上传取消: path={}", session.path);
                }
            }
            "download" => {
                // 下载取消：不需要清理
                tracing::info!("文件下载取消: path={}", session.path);
            }
            _ => {}
        }
        None
    };

    // 3. 返回 FileTransferComplete
    Ok(Envelope::new(
        request_id,
        Payload::FileTransferComplete {
            session_id: session_id.to_string(),
            success,
            mtime,
            error: None,
        },
    ))
}

/// 处理取消文件传输
///
/// # 参数
/// - `request_id`: 请求 ID
/// - `session_id`: 传输会话 ID
/// - `cfg`: Agent 配置
///
/// # 返回
/// - `Ok(Envelope)`: CancelFileTransferResponse 响应
/// - `Err(String)`: 错误信息
async fn handle_cancel_file_transfer(
    request_id: u32,
    session_id: &str,
    _cfg: &AgentConfig,
) -> Result<Envelope, String> {
    // 1. 从 TRANSFER_SESSIONS 移除会话
    let mut sessions = TRANSFER_SESSIONS.lock().await;

    let mut session = sessions
        .remove(session_id)
        .ok_or_else(|| format!("传输会话不存在: {}", session_id))?;

    // 2. 如果有 writer，调用 abort() 清理临时文件
    //    使用 take() 将 writer 从 session 中取出，避免 Drop 时重复清理
    if let Some(mut writer) = session.writer.take() {
        writer.abort();
        tracing::info!("取消上传，已删除临时文件: path={}", session.path);
    }

    // 3. 返回 CancelFileTransferResponse
    Ok(Envelope::new(
        request_id,
        Payload::CancelFileTransferResponse {
            session_id: session_id.to_string(),
            success: true,
        },
    ))
}

/// 检查文件是否存在
fn handle_file_exists(path: &str, cfg: &AgentConfig, session: &UserSession) -> Result<Envelope, String> {
    tracing::debug!("[handle_file_exists] path={}", path);
    // 检查路径权限
    if !cfg.security.allowed_paths.is_empty() {
        let allowed = cfg.security.allowed_paths
            .iter()
            .any(|prefix| path.starts_with(prefix));
        if !allowed {
            return Err(format!("访问被拒绝: {}", path));
        }
    }

    // 使用 UserExecutor 在用户上下文中执行操作
    let executor = UserExecutor::new(session);
    let path = path.to_string(); // 克隆为 String,以便移动到闭包
    executor.execute_as_user(move || {
        // 检查文件是否存在
        let metadata = std::fs::metadata(&path);

        match metadata {
            Ok(meta) => {
                // 文件存在，返回大小和修改时间
                let size = meta.len();
                let mtime = meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());

                Ok(Envelope::new(
                    uuid::Uuid::new_v4().as_u128() as u32,
                    Payload::FileExistsResponse {
                        exists: true,
                        size: Some(size),
                        mtime,
                    },
                ))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // 文件不存在
                Ok(Envelope::new(
                    uuid::Uuid::new_v4().as_u128() as u32,
                    Payload::FileExistsResponse {
                        exists: false,
                        size: None,
                        mtime: None,
                    },
                ))
            }
            Err(e) => {
                // 其他错误（权限问题等）
                Err(anyhow::anyhow!("无法访问文件: {}", e))
            }
        }
    }).map_err(|e| e.to_string())
}