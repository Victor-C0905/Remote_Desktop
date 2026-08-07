use crate::config::AgentConfig;
use crate::protocol::{Envelope, MetricsSnapshot, MountInfo, Payload};
use crate::transfer_session::{TransferSession, TransferStatus};
use crate::file_stream::{FileStreamReader, FileStreamWriter};
use crate::auth::{UserSession, UserExecutor, StatsManager}; // 新增：用户会话、执行器和统计管理器
use crate::auth::stats::ConnectionStatsSnapshot; // 新增：连接统计快照类型
use std::fs;

use std::time::{SystemTime, UNIX_EPOCH, Instant};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;
use uuid::Uuid;

// 全局传输会话管理器
lazy_static::lazy_static! {
    pub static ref TRANSFER_SESSIONS: Arc<Mutex<HashMap<String, TransferSession>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

#[tracing::instrument(skip(envelope, cfg, session, stats_manager), fields(request_id = envelope.request_id, payload_type = envelope.payload.type_name()))]
pub async fn handle_envelope(envelope: &Envelope, cfg: &AgentConfig, session: &UserSession, stats_manager: Arc<StatsManager>) -> Envelope {
    // 记录请求开始时间
    let start = Instant::now();

    // 处理请求
    let response = match &envelope.payload {
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

        Payload::GetCurrentUser => {
            tracing::info!("获取当前用户请求");
            let username = handle_get_current_user(session);
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

            match handle_file_transfer_request(envelope.request_id, direction, path, *file_size, *chunk_size, *resume_from, cfg, session).await {
                Ok(response) => response,
                Err(e) => {
                    tracing::error!("文件传输请求失败: {}", e);
                    error_response(envelope.request_id, &e)
                }
            }
        }

        // 文件数据块
        Payload::FileChunk { session_id, seq, data, size } => {
            tracing::debug!("文件数据块: session_id={}, seq={}, size={}", session_id, seq, size);
            match handle_file_chunk(envelope.request_id, session_id, *seq, data, cfg, stats_manager.clone()).await {
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

        // 统计查询请求
        Payload::GetStats { stats_type } => {
            tracing::info!("统计查询请求: stats_type={}, uid={}", stats_type, session.uid);

            // 权限检查：只有 root 用户可以查看全局统计
            if session.uid != 0 && stats_type != "connection" {
                tracing::warn!("权限拒绝: 用户 {} 尝试查询 {} 统计", session.username, stats_type);
                return Envelope::new(
                    envelope.request_id,
                    Payload::Error {
                        code: 403,
                        message: "权限不足：只有root用户可以查看此统计".to_string(),
                    },
                );
            }

            // 获取统计数据
            let stats = match stats_type.as_str() {
                "auth" => {
                    let snapshot = stats_manager.get_auth_stats();
                    Payload::StatsResponse {
                        auth: Some(snapshot),
                        connection: ConnectionStatsSnapshot::default(),
                        performance: None,
                    }
                },
                "connection" => {
                    let snapshot = if session.uid == 0 {
                        stats_manager.get_connection_stats()
                    } else {
                        stats_manager.get_connection_stats_for_user(session)
                    };
                    Payload::StatsResponse {
                        auth: None,
                        connection: snapshot,
                        performance: None,
                    }
                },
                "performance" => {
                    let snapshot = stats_manager.get_performance_stats().await;
                    Payload::StatsResponse {
                        auth: None,
                        connection: ConnectionStatsSnapshot::default(),
                        performance: Some(snapshot),
                    }
                },
                "all" => {
                    // 只有 root 用户可以查看所有统计
                    if session.uid != 0 {
                        tracing::warn!("权限拒绝: 用户 {} 尝试查询所有统计", session.username);
                        return Envelope::new(
                            envelope.request_id,
                            Payload::Error {
                                code: 403,
                                message: "权限不足：只有root用户可以查看全局统计".to_string(),
                            },
                        );
                    }

                    let auth = stats_manager.get_auth_stats();
                    let connection = stats_manager.get_connection_stats();
                    let performance = stats_manager.get_performance_stats().await;
                    Payload::StatsResponse {
                        auth: Some(auth),
                        connection,
                        performance: Some(performance),
                    }
                },
                _ => {
                    tracing::warn!("未知的统计类型: {}", stats_type);
                    return Envelope::new(
                        envelope.request_id,
                        Payload::Error {
                            code: 400,
                            message: format!("未知的统计类型: {}", stats_type),
                        },
                    );
                }
            };

            tracing::info!("统计查询成功: stats_type={}", stats_type);
            Envelope::new(envelope.request_id, stats)
        }

        other => {
            tracing::warn!("未处理的消息类型: {:?}", std::mem::discriminant(other));
            error_response(envelope.request_id, "未知的消息类型")
        }
    };

    // 记录响应时间
    let elapsed = start.elapsed();
    let duration_ms = elapsed.as_millis() as u64;
    stats_manager.record_api_response_time(duration_ms).await;

    response
}

/// 获取当前登录用户名
///
/// 返回SSH/PAM认证后的用户名，而不是Agent运行用户
///
/// # 参数
/// - `session`: 用户会话信息
///
/// # 返回
/// 登录用户名（如"vic"）
fn handle_get_current_user(session: &UserSession) -> String {
    session.username.clone()
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
    // 1. 生成 session_id
    let session_id = format!("transfer-{}", Uuid::new_v4());

    // 2. 根据方向处理
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
            let writer = executor.execute_as_user_unchecked(move || {
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
            let _chunk_size_value = chunk_size.unwrap_or(64 * 1024);
            let (file_size, mtime, reader) = executor.execute_as_user_unchecked(move || {
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
/// - `stats_manager`: 统计管理器
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
    stats_manager: Arc<StatsManager>,
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

        // 记录文件传输字节数
        stats_manager.record_file_transfer_bytes(data.len() as u64);

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