use crate::config::AgentConfig;
use crate::protocol::{Envelope, FileEntry, MetricsSnapshot, MountInfo, Payload};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn handle_envelope(envelope: &Envelope, cfg: &AgentConfig) -> Envelope {
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
                },
            )
        }

        Payload::ReadDirRequest { path } => {
            tracing::info!("读取目录请求: {}", path);
            match handle_read_dir(path, cfg) {
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

        Payload::ReadFileRequest { path } => {
            match handle_read_file(path, cfg) {
                Ok((content, size)) => Envelope::new(
                    envelope.request_id,
                    Payload::ReadFileResponse {
                        path: path.clone(),
                        content,
                        size,
                    },
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }

        Payload::WriteFileRequest { path, content } => {
            match handle_write_file(path, content, cfg) {
                Ok(size) => Envelope::new(
                    envelope.request_id,
                    Payload::WriteFileResponse {
                        path: path.clone(),
                        size,
                    },
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }

        Payload::DeleteRequest { path } => {
            match handle_delete(path, cfg) {
                Ok(_) => Envelope::new(
                    envelope.request_id,
                    Payload::DeleteResponse { success: true },
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }

        Payload::MkdirRequest { path } => {
            tracing::info!("创建目录请求: {}", path);
            match handle_mkdir(path, cfg) {
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

        Payload::RenameRequest { old_path, new_path } => {
            tracing::info!("重命名请求: {} -> {}", old_path, new_path);
            match handle_rename(old_path, new_path, cfg) {
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

        Payload::CopyRequest { src, dst } => {
            tracing::info!("复制请求: {} -> {}", src, dst);
            match handle_copy(src, dst, cfg) {
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

        Payload::MoveRequest { src, dst } => {
            tracing::info!("移动请求: {} -> {}", src, dst);
            match handle_move(src, dst, cfg) {
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

        Payload::MetricsSubscribeRequest {} => {
            match collect_metrics() {
                Ok(metrics) => Envelope::new(
                    envelope.request_id,
                    Payload::MetricsData(metrics),
                ),
                Err(e) => error_response(envelope.request_id, &e),
            }
        }

        Payload::TerminalSpawnRequest { shell, cols, rows } => {
            tracing::info!("终端请求: shell={}, cols={}, rows={} (未实现)", shell, cols, rows);
            error_response(envelope.request_id, "终端功能尚未实现")
        }

        Payload::TerminalData { session_id, data, is_input } => {
            tracing::debug!("终端数据: session={}, len={}, is_input={} (未实现)", session_id, data.len(), is_input);
            error_response(envelope.request_id, "终端功能尚未实现")
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

fn handle_read_dir(path: &str, cfg: &AgentConfig) -> Result<Vec<FileEntry>, String> {
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

    // 尝试读取目录，依赖 Linux 文件系统权限
    let entries = fs::read_dir(path)
        .map_err(|e| {
            let error_msg = e.to_string();
            if error_msg.contains("Permission denied") {
                format!("权限不足: 无法访问目录 '{}' (需要相应的 Linux 用户权限)", path)
            } else {
                format!("无法读取目录 '{}': {}", path, e)
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
}

fn handle_read_file(path: &str, cfg: &AgentConfig) -> Result<(String, u64), String> {
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

    let metadata = fs::metadata(path)
        .map_err(|e| {
            let error_msg = e.to_string();
            if error_msg.contains("Permission denied") {
                format!("权限不足: 无法访问文件 '{}' (需要相应的 Linux 用户权限)", path)
            } else {
                format!("无法访问文件 '{}': {}", path, e)
            }
        })?;
    
    if metadata.is_dir() {
        return Err("这是一个目录，不能作为文件读取".to_string());
    }

    const MAX_SIZE: u64 = 10 * 1024 * 1024; // 10MB
    if metadata.len() > MAX_SIZE {
        return Err(format!("文件太大 ({}MB)，限制 10MB", metadata.len() / (1024 * 1024)));
    }

    let content = fs::read_to_string(path)
        .map_err(|e| format!("读取文件失败: {}", e))?;

    Ok((content, metadata.len()))
}

fn handle_write_file(path: &str, content: &str, cfg: &AgentConfig) -> Result<u64, String> {
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

    fs::write(path, content)
        .map_err(|e| {
            let error_msg = e.to_string();
            if error_msg.contains("Permission denied") {
                format!("权限不足: 无法写入文件 '{}' (需要相应的 Linux 用户权限)", path)
            } else {
                format!("写入文件失败: {}", e)
            }
        })?;

    Ok(content.len() as u64)
}

fn handle_delete(path: &str, cfg: &AgentConfig) -> Result<(), String> {
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

    let metadata = fs::metadata(path)
        .map_err(|e| format!("无法访问 '{}': {}", path, e))?;

    if metadata.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|e| format!("删除目录失败: {}", e))?;
    } else {
        fs::remove_file(path)
            .map_err(|e| format!("删除文件失败: {}", e))?;
    }

    Ok(())
}

fn handle_mkdir(path: &str, cfg: &AgentConfig) -> Result<String, String> {
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

    fs::create_dir_all(path)
        .map_err(|e| {
            let error_msg = e.to_string();
            if error_msg.contains("Permission denied") {
                format!("权限不足: 无法创建目录 '{}' (需要相应的 Linux 用户权限)", path)
            } else {
                format!("无法创建目录 '{}': {}", path, e)
            }
        })?;

    Ok(path.to_string())
}

fn handle_rename(old_path: &str, new_path: &str, cfg: &AgentConfig) -> Result<(String, String), String> {
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

    fs::rename(old_path, new_path)
        .map_err(|e| {
            let error_msg = e.to_string();
            if error_msg.contains("Permission denied") {
                format!("权限不足: 无法重命名 '{}' (需要相应的 Linux 用户权限)", old_path)
            } else {
                format!("无法重命名 '{}': {}", old_path, e)
            }
        })?;

    Ok((old_path.to_string(), new_path.to_string()))
}

fn handle_copy(src: &str, dst: &str, cfg: &AgentConfig) -> Result<(String, String), String> {
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

    // 检查源文件是否存在
    let metadata = fs::metadata(src)
        .map_err(|e| {
            let error_msg = e.to_string();
            if error_msg.contains("Permission denied") {
                format!("权限不足: 无法访问源文件 '{}' (需要相应的 Linux 用户权限)", src)
            } else if error_msg.contains("No such file") {
                format!("源文件 '{}' 不存在", src)
            } else {
                format!("无法访问源文件 '{}': {}", src, e)
            }
        })?;

    // 只支持文件复制，不支持目录复制
    if metadata.is_dir() {
        return Err(format!("不支持复制目录 '{}' (请使用移动功能)", src));
    }

    fs::copy(src, dst)
        .map_err(|e| {
            let error_msg = e.to_string();
            if error_msg.contains("Permission denied") {
                format!("权限不足: 无法复制 '{}' (需要相应的 Linux 用户权限)", src)
            } else {
                format!("无法复制 '{}': {}", src, e)
            }
        })?;

    Ok((src.to_string(), dst.to_string()))
}

fn handle_move(src: &str, dst: &str, cfg: &AgentConfig) -> Result<(String, String), String> {
    // move 本质上是 rename
    handle_rename(src, dst, cfg)
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