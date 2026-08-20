use crate::config::AgentConfig;
use crate::protocol::{Envelope, MetricsSnapshot, MountInfo, Payload};
use crate::transfer_session::{TransferSession, TransferStatus};
use crate::file_stream::{PipeFileStreamReader, PipeFileStreamWriter, generate_temp_path};
use crate::auth::{UserSession, UserExecutor, StatsManager}; // 新增：用户会话、执行器和统计管理器
use crate::auth::stats::ConnectionStatsSnapshot; // 新增：连接统计快照类型
use crate::audit::AuditLogger; // 新增：审计日志记录器
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

#[tracing::instrument(skip(envelope, cfg, session, stats_manager, audit_log), fields(request_id = envelope.request_id, payload_type = envelope.payload.type_name()))]
pub async fn handle_envelope(envelope: &Envelope, cfg: &AgentConfig, session: &UserSession, stats_manager: Arc<StatsManager>, audit_log: Arc<AuditLogger>) -> Envelope {
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
        Payload::FileTransferRequest { direction, path, file_size, chunk_size, resume_from, frame_mode, stream_count } => {
            tracing::info!("文件传输请求: direction={:?}, path={}, resume_from={:?}, frame_mode={}, stream_count={:?}", direction, path, resume_from, frame_mode, stream_count);

            match handle_file_transfer_request(envelope.request_id, direction, path, *file_size, *chunk_size, *resume_from, cfg, session, audit_log.clone(), frame_mode, *stream_count).await {
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
    audit_log: Arc<AuditLogger>,
    frame_mode: &str,
    stream_count: Option<u32>,
) -> Result<Envelope, String> {
    // 1. 生成 session_id
    let session_id = format!("transfer-{}", Uuid::new_v4());

    // 路径安全校验：防目录穿越、限制用户家目录、防符号链接攻击
    let safe_path = crate::auth::validate_path(path, session.home_dir.as_path(), session.uid)
        .map_err(|e| e.to_string())?;
    let path = safe_path.as_str();

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

            // 默认分块大小 256KB
            let chunk_size = chunk_size.unwrap_or(256 * 1024);

            // 协商 stream_count（None/0/1 视为单流，向后兼容）
            let stream_count = stream_count.unwrap_or(1).max(1);

            // 使用 UserExecutor 在隔离子进程中执行文件写入（方案 ii'）
            let executor = UserExecutor::new(session);

            // 断点续传暂不支持隔离写入模式
            if resume_from.is_some() {
                tracing::warn!("断点续传暂不支持隔离写入模式,将从头开始传输");
            }

            if stream_count > 1 {
                // ===== 多流并行模式（方案 A: 独立段+合并） =====
                //
                // 主 stream (index=0) 在此创建 part writer（spawn_isolated_writer_part，无 rename）。
                // 其余 N-1 个 stream 通过 MultiStreamJoin 握手后各自创建 part writer（见 handle_multi_stream_join）。
                // 全部 N 个 stream 完成 → spawn_isolated_merger 合并段文件 → 最终文件。
                //
                // part_offsets 由服务端计算（等分 file_size），客户端按 Accept 中的 stream_count
                // 自行计算对应 offset 段并在 MultiStreamJoin 中声明，服务端校验匹配。

                // 计算 part_offsets：等分 file_size 为 N 段
                let part_offsets = compute_part_offsets(file_size, stream_count);

                // 主 stream (index=0) 的段文件路径
                let part_path = generate_temp_path(std::path::Path::new(path));

                // fork 子进程:子进程 setuid + namespace 后打开段文件,父进程经 pipe 写 chunk（无 rename）
                let (pipe_writer, child_pid) = executor
                    .spawn_isolated_writer_part(&part_path)
                    .map_err(|e| e.to_string())?;
                let writer = PipeFileStreamWriter::new(
                    pipe_writer, child_pid, executor.clone(), file_size, part_path.clone(),
                );

                // 创建多流传输会话
                let mut transfer_session = TransferSession::new_multi_stream(
                    session_id.clone(),
                    direction.to_string(),
                    path.to_string(),
                    file_size,
                    chunk_size,
                    stream_count,
                    part_offsets.clone(),
                );
                transfer_session.writer = Some(writer);
                transfer_session.part_paths[0] = part_path;

                // 保存到全局会话管理器
                let mut sessions = TRANSFER_SESSIONS.lock().await;
                sessions.insert(session_id.clone(), transfer_session);

                tracing::info!(
                    "创建多流上传会话: session_id={}, path={}, size={}, streams={}",
                    session_id, path, file_size, stream_count
                );

                // 审计日志：记录上传开始
                audit_log.log_file_operation(&session.username, session.uid, "upload_start", path, file_size);

                // 返回接受响应（stream_count 确认 N，客户端据此开 N 个 bi-stream）
                Ok(Envelope::new(
                    request_id,
                    Payload::FileTransferAccept {
                        session_id,
                        file_size,
                        chunk_size,
                        mtime: None,
                        frame_mode: frame_mode.to_string(),
                        stream_count,
                    },
                ))
            } else {
                // ===== 单流模式（向后兼容，spawn_isolated_writer 含 rename） =====

                // 生成临时文件路径
                let temp_path = generate_temp_path(std::path::Path::new(path));

                // fork 子进程:子进程 setuid + namespace 后打开临时文件,父进程经 pipe 写 chunk
                let (pipe_writer, child_pid) = executor
                    .spawn_isolated_writer(&temp_path, path, file_size)
                    .map_err(|e| e.to_string())?;
                let writer = PipeFileStreamWriter::new(pipe_writer, child_pid, executor.clone(), file_size, temp_path);

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

                // 审计日志：记录上传开始
                audit_log.log_file_operation(&session.username, session.uid, "upload_start", path, file_size);

                // 返回接受响应
                Ok(Envelope::new(
                    request_id,
                    Payload::FileTransferAccept {
                        session_id,
                        file_size,
                        chunk_size,
                        mtime: None,
                        frame_mode: frame_mode.to_string(),
                        stream_count: 1,
                    },
                ))
            }
        }

        "download" => {
            // 下载：Agent 发送文件到客户端
            // 使用 UserExecutor 在隔离子进程中执行文件读取（方案 ii'）
            let executor = UserExecutor::new(session);

            // 断点续传暂不支持隔离读取模式
            if resume_from.is_some() {
                tracing::warn!("断点续传暂不支持隔离读取模式,将从头开始传输");
            }

            // 获取文件元数据（以目标用户身份,验证读取权限）
            // 注意：原为 execute_as_user + fs::metadata 闭包（fork+pipe 回收结果），
            // 现改用 posix_spawn 方案的 get_metadata_async 子进程，避免 fork 拷贝 runtime 状态
            let (file_size, mtime) = executor.get_metadata_async(path)
                .await
                .map_err(|e| e.to_string())?;

            // fork 子进程:子进程 setuid + namespace 后打开文件,父进程经 pipe 读 chunk
            let (pipe_reader, child_pid) = executor
                .spawn_isolated_reader(path)
                .map_err(|e| e.to_string())?;
            let reader = PipeFileStreamReader::new(pipe_reader, child_pid, executor.clone(), file_size);

            // 默认分块大小 256KB(与上传一致,减少帧数与 syscall 开销)
            let chunk_size = chunk_size.unwrap_or(256 * 1024);

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

            // 审计日志：记录下载开始
            audit_log.log_file_operation(&session.username, session.uid, "download_start", path, file_size);

            // 返回接受响应
            Ok(Envelope::new(
                request_id,
                Payload::FileTransferAccept {
                    session_id,
                    file_size,
                    chunk_size,
                    mtime: Some(mtime),
                    frame_mode: frame_mode.to_string(),
                    stream_count: 1,  // 下载暂不多流(下载读盘已快,瓶颈在上行)
                },
            ))
        }

        _ => {
            Err(format!("无效的传输方向: {}", direction))
        }
    }
}

/// 计算多流并行的各段 offset 范围（等分 file_size 为 N 段）
///
/// 返回 `Vec<(offset_start, offset_end)>`，长度 == stream_count。
/// 最后一段负责剩余全部字节（处理 file_size 不能被 N 整除的情况）。
///
/// 例：file_size=1024, stream_count=4 → [(0,256),(256,512),(512,768),(768,1024)]
///     file_size=1000, stream_count=4 → [(0,250),(250,500),(500,750),(750,1000)]
fn compute_part_offsets(file_size: u64, stream_count: u32) -> Vec<(u64, u64)> {
    let n = stream_count as u64;
    let part = file_size / n;
    let mut offsets = Vec::with_capacity(stream_count as usize);
    let mut cur = 0u64;
    for i in 0..stream_count as u64 {
        let start = cur;
        // 最后一段负责剩余全部字节（处理整除余数）
        let end = if i == n - 1 {
            file_size
        } else {
            cur + part
        };
        offsets.push((start, end));
        cur = end;
    }
    offsets
}

/// 处理多流加入握手（非主 stream 的第一个控制帧，Payload::MultiStreamJoin）
///
/// 在 TransferSession 中为指定 stream_index 创建 part writer（spawn_isolated_writer_part，
/// 无 rename），存入 `part_writers`，并将 part_path 存入 `part_paths[stream_index]`。
///
/// 调用方（quic.rs `handle_stream` 的 `Payload::MultiStreamJoin` 分支）随后从 session
/// 取走 writer（`part_writers.remove(&stream_index)`），进入 part-upload 循环
/// （接收 chunk → write → finish → mark_stream_completed）。
///
/// 注意：writer 不从此函数返回（`PipeFileStreamWriter` 持唯一 pipe FD，不可 Clone），
/// 而是存入 session 后由调用方 take，与主 stream 的 `session.writer.take()` 模式一致。
///
/// # 参数
/// - `session_id`: 关联的传输会话 ID（主 stream Accept 返回的）
/// - `stream_index`: 本 stream 索引（1..N-1，主 stream 隐式为 0 不经此函数）
/// - `offset_start` / `offset_end`: 客户端声明的本 stream offset 段（-exclusive）
/// - `session`: 用户会话（用于创建 UserExecutor）
///
/// # 返回
/// - `Ok((file_size, chunk_size, frame_mode))`: 调用方持此信息 + take writer 接收 chunk
/// - `Err(String)`: 会话不存在、非多流会话、stream_index 越界、offset 不匹配、spawn 失败
pub async fn handle_multi_stream_join(
    session_id: &str,
    stream_index: u32,
    offset_start: u64,
    offset_end: u64,
    session: &UserSession,
) -> Result<(u64, u32, String), String> {
    let mut sessions = TRANSFER_SESSIONS.lock().await;
    let transfer_session = sessions.get_mut(session_id)
        .ok_or_else(|| format!("多流加入失败: 会话不存在: {}", session_id))?;

    // 校验为多流会话
    if !transfer_session.is_multi_stream() {
        return Err(format!(
            "多流加入失败: 会话非多流模式 (stream_count={}): {}",
            transfer_session.stream_count, session_id
        ));
    }

    // 校验 stream_index 范围（1..N-1，主 stream 为 0 不经此函数）
    if stream_index == 0 || stream_index >= transfer_session.stream_count {
        return Err(format!(
            "多流加入失败: stream_index={} 越界 (有效范围 1..{}): {}",
            stream_index, transfer_session.stream_count - 1, session_id
        ));
    }

    // 校验 offset 段匹配服务端计算的 part_offsets
    let expected = transfer_session.part_offsets.get(stream_index as usize)
        .ok_or_else(|| format!("多流加入失败: part_offsets[{}] 不存在", stream_index))?;
    if offset_start != expected.0 || offset_end != expected.1 {
        return Err(format!(
            "多流加入失败: offset 段不匹配 (客户端 [{},{}) vs 服务端 [{},{})): {}",
            offset_start, offset_end, expected.0, expected.1, session_id
        ));
    }

    // 防重复加入（同一 stream_index 已有 writer）
    if transfer_session.part_writers.contains_key(&stream_index) {
        return Err(format!(
            "多流加入失败: stream_index={} 已加入 (重复): {}", stream_index, session_id
        ));
    }

    // 生成段文件路径
    let part_path = generate_temp_path(std::path::Path::new(&transfer_session.path));

    // 提取会话参数（释放锁前 clone 出来）
    let file_size = transfer_session.file_size;
    let chunk_size = transfer_session.chunk_size;
    // 多流仅支持 raw 帧模式（JSON 模式旧客户端不走多流）
    let frame_mode = "raw".to_string();

    // 创建 executor + fork 子进程写段文件（无 rename）
    let executor = UserExecutor::new(session);
    let (pipe_writer, child_pid) = executor
        .spawn_isolated_writer_part(&part_path)
        .map_err(|e| format!("spawn_isolated_writer_part 失败: {}", e))?;
    let writer = PipeFileStreamWriter::new(
        pipe_writer, child_pid, executor, file_size, part_path.clone(),
    );

    // 注册到 session（调用方后续 take 出来用）
    transfer_session.part_paths[stream_index as usize] = part_path.clone();
    transfer_session.part_writers.insert(stream_index, writer);

    tracing::info!(
        "多流加入成功: session_id={}, stream_index={}, offset=[{},{}), part_path={}",
        session_id, stream_index, offset_start, offset_end, part_path
    );

    Ok((file_size, chunk_size, frame_mode))
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
    // 1. 从 TRANSFER_SESSIONS 取出 writer,立即释放锁(避免在 sync pipe 写期间持有 tokio Mutex)
    let mut writer = {
        let mut sessions = TRANSFER_SESSIONS.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("传输会话不存在: {}", session_id))?;
        // 检查会话状态是否为 Active
        if session.status != TransferStatus::Active {
            return Err(format!("传输会话状态异常: {:?}", session.status));
        }
        session.writer.take()
            .ok_or_else(|| "传输会话没有 writer".to_string())?
    };

    // 2. spawn_blocking 写 pipe,不阻塞 tokio worker(pipe 缓冲区满时子进程读慢会阻塞)
    let data_owned = data.to_vec();
    let (writer, write_result) = tokio::task::spawn_blocking(move || {
        let result = writer.write_chunk(&data_owned);
        (writer, result)
    })
    .await
    .map_err(|e| format!("写入任务 panic: {}", e))?;

    // 3. 重新拿锁放回 writer + 更新 transferred
    let transferred = writer.transferred();
    {
        let mut sessions = TRANSFER_SESSIONS.lock().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.transferred = transferred;
            session.writer = Some(writer);
        }
        // 若 session 已不存在(被并发清理),writer drop 时自动 abort(子进程 SIGTERM + 临时文件清理)
    }

    // 4. 检查写入结果(失败则返回错误,不记录统计)
    write_result?;

    // 记录文件传输字节数(仅在写入成功后)
    stats_manager.record_file_transfer_bytes(data.len() as u64);

    tracing::debug!(
        "写入数据块: session_id={}, seq={}, size={}, transferred={}",
        session_id,
        seq,
        data.len(),
        transferred
    );

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