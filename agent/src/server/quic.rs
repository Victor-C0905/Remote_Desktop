use anyhow::Result;
use quinn::{RecvStream, SendStream};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

// Unix平台特有的导入(终端功能)
#[cfg(unix)]
use tokio::io::AsyncReadExt;
#[cfg(unix)]
use tokio::time::{sleep, Duration};

use crate::config::AgentConfig;
use crate::pty::PtyManager;
use crate::subscription::SubscriptionManager;
use crate::event_bus::EventBus;
use crate::protocol::{Envelope, Payload};

// 全局 Stream ID 计数器
static STREAM_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub async fn run(
    cfg: AgentConfig,
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    subscription_manager: Arc<SubscriptionManager>,
    event_bus: Arc<EventBus>,
    pty_manager: Arc<PtyManager>,
) -> Result<()> {
    let addr = format!("{}:{}", cfg.server.bind, cfg.server.quic_port);
    tracing::info!("🔵 QUIC 服务器监听: {}", addr);

    let server_config = build_server_config(certs, key)?;
    let endpoint = quinn::Endpoint::server(server_config, addr.parse()?)?;

    while let Some(incoming) = endpoint.accept().await {
        let cfg_clone = cfg.clone();
        let subscription_manager_clone = subscription_manager.clone();
        let event_bus_clone = event_bus.clone();
        let pty_manager_clone = pty_manager.clone();
        tokio::spawn(async move {
            let conn = incoming.await;
            match conn {
                Ok(connection) => {
                    if let Err(e) = handle_connection(
                        connection,
                        &cfg_clone,
                        subscription_manager_clone,
                        event_bus_clone,
                        pty_manager_clone,
                    ).await {
                        tracing::warn!("QUIC 连接错误: {}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("QUIC 连接握手失败: {}", e);
                }
            }
        });
    }

    Ok(())
}

fn build_server_config(
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<quinn::ServerConfig> {
    let mut quic_config = quinn::ServerConfig::with_single_cert(certs, key)?;
    
    // 配置传输参数，禁用空闲超时
    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(None); // 禁用空闲超时，连接不会因无活动而关闭
    transport.keep_alive_interval(Some(std::time::Duration::from_secs(5))); // 每5秒发送保持活跃包
    
    quic_config.transport_config(std::sync::Arc::new(transport));
    Ok(quic_config)
}

async fn handle_connection(
    connection: quinn::Connection,
    cfg: &AgentConfig,
    subscription_manager: Arc<SubscriptionManager>,
    event_bus: Arc<EventBus>,
    pty_manager: Arc<PtyManager>,
) -> Result<()> {
    let remote = connection.remote_address();
    tracing::info!("✅ 新的 QUIC 连接来自: {}", remote);

    while let Ok(stream) = connection.accept_bi().await {
        let cfg_inner = cfg.clone();
        let subscription_manager_inner = subscription_manager.clone();
        let event_bus_inner = event_bus.clone();
        let pty_manager_inner = pty_manager.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_stream(
                stream,
                &cfg_inner,
                subscription_manager_inner,
                event_bus_inner,
                pty_manager_inner,
            ).await {
                tracing::warn!("QUIC Stream 处理错误: {}", e);
            }
        });
    }

    tracing::info!("连接关闭: {}", remote);
    Ok(())
}

async fn handle_stream(
    stream: (SendStream, RecvStream),
    cfg: &AgentConfig,
    subscription_manager: Arc<SubscriptionManager>,
    event_bus: Arc<EventBus>,
    #[cfg(unix)] pty_manager: Arc<PtyManager>,
    #[cfg(not(unix))] _pty_manager: Arc<PtyManager>,
) -> Result<()> {
    let (mut send, mut recv) = stream;

    // 生成唯一的 Stream ID
    let stream_id = STREAM_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    tracing::debug!("Stream ID: {}", stream_id);

    // 读取第一条消息（订阅请求）
    let data = read_message(&mut recv).await?;
    if data.is_none() {
        tracing::info!("Stream 关闭: stream_id={}", stream_id);
        return Ok(());
    }

    let data = data.unwrap();
    let envelope = Envelope::decode(&data).map_err(|e| anyhow::anyhow!(e))?;

    // 处理订阅请求
    match &envelope.payload {
        Payload::Subscribe { server_id, types } => {
            tracing::info!("订阅请求: server_id={}, types={}", server_id, types.len());

            // 添加订阅
            let subscribed_types = subscription_manager.subscribe(stream_id, types.clone()).await?;

            // 发送确认响应
            let response = Envelope::new(
                envelope.request_id,
                Payload::SubscribeAck {
                    success: true,
                    subscribed_types,
                },
            );
            match response.encode() {
                Ok(resp_bytes) => {
                    if let Err(e) = write_message(&mut send, &resp_bytes).await {
                        tracing::warn!("发送响应失败: {}", e);
                        return Ok(());
                    }
                }
                Err(e) => {
                    tracing::warn!("编码响应失败: {}", e);
                    return Ok(());
                }
            }

            // 订阅 EventBus（用于事件推送）
            let mut event_rx = event_bus.subscribe();

            tracing::info!("开始事件推送: stream_id={}", stream_id);

            // 进入事件推送循环
            loop {
                match event_rx.recv().await {
                    Ok(event) => {
                        // 检查是否有订阅者
                        if subscription_manager.has_subscribers(&event.event_type).await {
                            // 发送事件
                            let event_envelope = Envelope::new(
                                0, // 事件推送不需要 request_id
                                Payload::Event {
                                    event_type: event.event_type.clone(),
                                    data: event.data.clone(),
                                    timestamp: event.timestamp,
                                },
                            );

                            match event_envelope.encode() {
                                Ok(resp_bytes) => {
                                    if let Err(e) = write_message(&mut send, &resp_bytes).await {
                                        tracing::warn!("事件推送失败: {}", e);
                                        break;
                                    }
                                    tracing::debug!("事件推送成功: event_type={}", event.event_type);
                                }
                                Err(e) => {
                                    tracing::warn!("编码事件失败: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("EventBus 接收失败: {}", e);
                        break;
                    }
                }
            }

            // 移除订阅
            subscription_manager.remove_all(stream_id).await?;
            tracing::info!("停止事件推送: stream_id={}", stream_id);
        }

        #[cfg(unix)]
        Payload::TerminalSpawnRequest { shell, cols, rows } => {
            tracing::info!("终端创建请求: shell={}, cols={}, rows={}", shell, cols, rows);

            // 创建 PTY 会话
            let session_id = pty_manager.spawn(&shell, *cols, *rows).await?;

            // 发送响应
            let response = Envelope::new(
                envelope.request_id,
                Payload::TerminalSpawnResponse { session_id: session_id.clone() },
            );
            match response.encode() {
                Ok(resp_bytes) => {
                    if let Err(e) = write_message(&mut send, &resp_bytes).await {
                        tracing::warn!("发送终端响应失败: {}", e);
                        return Ok(());
                    }
                }
                Err(e) => {
                    tracing::warn!("编码终端响应失败: {}", e);
                    return Ok(());
                }
            }

            tracing::info!("终端会话创建成功: session_id={}", session_id);

            // 进入终端双向数据隧道循环
            handle_terminal_stream(
                session_id.clone(),
                send,
                recv,
                pty_manager,
            ).await?;

            tracing::info!("终端 Stream 结束: session_id={}", session_id);
        }

        #[cfg(not(unix))]
        Payload::TerminalSpawnRequest { .. } => {
            tracing::warn!("终端功能仅支持 Unix 平台");
            let response = Envelope::new(
                envelope.request_id,
                Payload::Error { code: -1, message: "终端功能仅支持 Unix 平台".to_string() },
            );
            if let Ok(resp_bytes) = response.encode() {
                write_message(&mut send, &resp_bytes).await?;
            }
        }

        #[cfg(unix)]
        Payload::TerminalData { session_id, data, is_input } => {
            // 单条终端数据消息（用于非持久连接）
            if *is_input {
                pty_manager.write(&session_id, &data).await?;
            } else {
                // 输出数据不应该从客户端发送
                tracing::warn!("收到意外的终端输出数据: session_id={}", session_id);
            }

            // 发送空响应
            let response = Envelope::new(envelope.request_id, Payload::TerminalSpawnResponse { session_id: session_id.clone() });
            if let Ok(resp_bytes) = response.encode() {
                write_message(&mut send, &resp_bytes).await?;
            }
        }

        #[cfg(not(unix))]
        Payload::TerminalData { .. } => {
            tracing::warn!("终端功能仅支持 Unix 平台");
            let response = Envelope::new(
                envelope.request_id,
                Payload::Error { code: -1, message: "终端功能仅支持 Unix 平台".to_string() },
            );
            if let Ok(resp_bytes) = response.encode() {
                write_message(&mut send, &resp_bytes).await?;
            }
        }

        #[cfg(unix)]
        Payload::TerminalResizeRequest { session_id, cols, rows } => {
            // 调整远程 PTY 大小
            match pty_manager.resize(&session_id, *cols, *rows).await {
                Ok(()) => {
                    tracing::info!("PTY resize 成功: session_id={}, {}x{}", session_id, cols, rows);
                    let response = Envelope::new(envelope.request_id, Payload::TerminalResizeResponse);
                    if let Ok(resp_bytes) = response.encode() {
                        write_message(&mut send, &resp_bytes).await?;
                    }
                }
                Err(e) => {
                    tracing::warn!("PTY resize 失败: {}", e);
                    let response = Envelope::new(
                        envelope.request_id,
                        Payload::Error { code: -1, message: format!("resize 失败: {}", e) },
                    );
                    if let Ok(resp_bytes) = response.encode() {
                        write_message(&mut send, &resp_bytes).await?;
                    }
                }
            }
        }

        _ => {
            // 其他请求使用同步 handler
            let response = crate::handler::handle_envelope(&envelope, cfg);
            match response.encode() {
                Ok(resp_bytes) => {
                    if let Err(e) = write_message(&mut send, &resp_bytes).await {
                        tracing::warn!("发送响应失败: {}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("编码响应失败: {}", e);
                }
            }
        }
    }

    Ok(())
}

async fn read_message(recv: &mut RecvStream) -> Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match recv.read_exact(&mut len_buf).await {
        Ok(()) => {
            let len = u32::from_le_bytes(len_buf) as usize;
            if len == 0 || len > 10 * 1024 * 1024 {
                anyhow::bail!("无效的消息长度: {}", len);
            }
            let mut data = vec![0u8; len];
            recv.read_exact(&mut data).await?;
            Ok(Some(data))
        }
        Err(quinn::ReadExactError::FinishedEarly(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

async fn write_message(send: &mut SendStream, data: &[u8]) -> Result<()> {
    let len = (data.len() as u32).to_le_bytes();
    send.write_all(&len).await?;
    send.write_all(data).await?;
    Ok(())
}

/// 处理终端持久 Stream（双向数据隧道）
#[cfg(unix)]
async fn handle_terminal_stream(
    session_id: String,
    mut send: SendStream,
    mut recv: RecvStream,
    pty_manager: Arc<PtyManager>,
) -> Result<()> {
    tracing::info!("终端双向隧道启动: session_id={}", session_id);

    // ── PTY 输出读取任务（带批量发送优化）────────
    // 策略：累积数据直到达到 BATCH_SIZE 或超过 BATCH_INTERVAL_MS
    const BATCH_SIZE: usize = 1024;        // 每批最大字节数
    const BATCH_INTERVAL_MS: u64 = 30;     // 最大等待时间 (ms)

    let session_id_clone = session_id.clone();
    let pty_manager_clone = pty_manager.clone();
    let pty_read_task = tokio::spawn(async move {
        let mut batch_buffer = Vec::with_capacity(BATCH_SIZE);
        let mut last_send_time = std::time::Instant::now();

        loop {
            // 从 PTY 读取输出
            match pty_manager_clone.read(&session_id_clone).await {
                Ok(data) if !data.is_empty() => {
                    batch_buffer.extend_from_slice(&data);

                    // 判断是否需要发送批次
                    let should_flush = batch_buffer.len() >= BATCH_SIZE
                        || last_send_time.elapsed().as_millis() >= BATCH_INTERVAL_MS as u128;

                    if should_flush && !batch_buffer.is_empty() {
                        // 发送批量数据
                        let len = (batch_buffer.len() as u32).to_le_bytes();
                        if let Err(e) = send.write_all(&len).await {
                            tracing::warn!("发送终端数据长度失败: {}", e);
                            break;
                        }
                        if let Err(e) = send.write_all(&batch_buffer).await {
                            tracing::warn!("发送终端数据失败: {}", e);
                            break;
                        }
                        tracing::debug!(
                            "PTY 批量输出发送: batch_len={}, chunks=1",
                            batch_buffer.len()
                        );
                        batch_buffer.clear();
                        last_send_time = std::time::Instant::now();
                    }
                }
                Ok(_) => {
                    // 无数据时检查是否有积压数据需要刷新
                    if !batch_buffer.is_empty()
                        && last_send_time.elapsed().as_millis() >= BATCH_INTERVAL_MS as u128
                    {
                        let len = (batch_buffer.len() as u32).to_le_bytes();
                        if let Err(e) = send.write_all(&len).await {
                            tracing::warn!("发送终端数据长度失败(空闲刷新): {}", e);
                            break;
                        }
                        if let Err(e) = send.write_all(&batch_buffer).await {
                            tracing::warn!("发送终端数据失败(空闲刷新): {}", e);
                            break;
                        }
                        tracing::debug!(
                            "PTY 空闲刷新: batch_len={}",
                            batch_buffer.len()
                        );
                        batch_buffer.clear();
                        last_send_time = std::time::Instant::now();
                    }
                    // 无数据，短暂等待（降低轮询频率减少 CPU 占用）
                    sleep(Duration::from_millis(10)).await;
                }
                Err(e) => {
                    tracing::warn!("PTY 读取失败: {}", e);
                    break;
                }
            }
        }

        // 发送剩余数据
        if !batch_buffer.is_empty() {
            let len = (batch_buffer.len() as u32).to_le_bytes();
            let _ = send.write_all(&len).await;
            let _ = send.write_all(&batch_buffer).await;
        }

        tracing::info!("PTY 读取任务结束: session_id={}", session_id_clone);
    });

    // 创建客户端输入读取任务
    let session_id_clone = session_id.clone();
    let pty_manager_clone = pty_manager.clone();
    let client_read_task = tokio::spawn(async move {
        loop {
            // 从客户端读取输入
            let mut len_buf = [0u8; 4];
            match recv.read_exact(&mut len_buf).await {
                Ok(_) => {
                    let len = u32::from_le_bytes(len_buf) as usize;
                    if len == 0 || len > 1024 * 1024 {
                        tracing::warn!("无效的终端数据长度: {}", len);
                        break;
                    }
                    let mut data = vec![0u8; len];
                    match recv.read_exact(&mut data).await {
                        Ok(_) => {
                            // 写入 PTY
                            if let Err(e) = pty_manager_clone.write(&session_id_clone, &data).await {
                                tracing::warn!("写入 PTY 失败: {}", e);
                                break;
                            }
                            tracing::debug!("客户端输入写入 PTY: len={}", len);
                        }
                        Err(e) => {
                            tracing::warn!("读取客户端数据失败: {}", e);
                            break;
                        }
                    }
                }
                Err(quinn::ReadExactError::FinishedEarly(_)) => {
                    tracing::info!("客户端关闭发送端: session_id={}", session_id_clone);
                    break;
                }
                Err(e) => {
                    tracing::warn!("读取客户端长度失败: {}", e);
                    break;
                }
            }
        }
        tracing::info!("客户端读取任务结束: session_id={}", session_id_clone);
    });

    // 等待任一任务结束
    tokio::select! {
        _ = pty_read_task => {
            tracing::info!("PTY 读取任务先结束: session_id={}", session_id);
        }
        _ = client_read_task => {
            tracing::info!("客户端读取任务先结束: session_id={}", session_id);
        }
    }

    // 清理 PTY 会话
    pty_manager.remove(&session_id).await?;

    Ok(())
}
// ✅ 优化: 删除Windows平台的stub实现,因为终端功能仅支持Unix
// #[cfg(not(unix))] 的 handle_terminal_stream 已删除