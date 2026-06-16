use anyhow::Result;
use quinn::{RecvStream, SendStream};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::AgentConfig;
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
) -> Result<()> {
    let addr = format!("{}:{}", cfg.server.bind, cfg.server.quic_port);
    tracing::info!("🔵 QUIC 服务器监听: {}", addr);

    let server_config = build_server_config(certs, key)?;
    let endpoint = quinn::Endpoint::server(server_config, addr.parse()?)?;

    while let Some(incoming) = endpoint.accept().await {
        let cfg_clone = cfg.clone();
        let subscription_manager_clone = subscription_manager.clone();
        let event_bus_clone = event_bus.clone();
        tokio::spawn(async move {
            let conn = incoming.await;
            match conn {
                Ok(connection) => {
                    if let Err(e) = handle_connection(
                        connection,
                        &cfg_clone,
                        subscription_manager_clone,
                        event_bus_clone,
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
) -> Result<()> {
    let remote = connection.remote_address();
    tracing::info!("✅ 新的 QUIC 连接来自: {}", remote);

    while let Ok(stream) = connection.accept_bi().await {
        let cfg_inner = cfg.clone();
        let subscription_manager_inner = subscription_manager.clone();
        let event_bus_inner = event_bus.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_stream(
                stream,
                &cfg_inner,
                subscription_manager_inner,
                event_bus_inner,
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
) -> Result<()> {
    let (mut send, mut recv) = stream;

    // 生成唯一的 Stream ID
    let stream_id = STREAM_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    tracing::info!("Stream ID: {}", stream_id);

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