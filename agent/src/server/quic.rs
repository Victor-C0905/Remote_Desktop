use anyhow::Result;
use quinn::{RecvStream, SendStream};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use crate::config::AgentConfig;

pub async fn run(cfg: AgentConfig, certs: Vec<CertificateDer<'static>>, key: PrivateKeyDer<'static>) -> Result<()> {
    let addr = format!("{}:{}", cfg.server.bind, cfg.server.quic_port);
    tracing::info!("🔵 QUIC 服务器监听: {}", addr);

    let server_config = build_server_config(certs, key)?;
    let endpoint = quinn::Endpoint::server(server_config, addr.parse()?)?;

    while let Some(incoming) = endpoint.accept().await {
        let cfg_clone = cfg.clone();
        tokio::spawn(async move {
            let conn = incoming.await;
            match conn {
                Ok(connection) => {
                    if let Err(e) = handle_connection(connection, &cfg_clone).await {
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

async fn handle_connection(connection: quinn::Connection, cfg: &AgentConfig) -> Result<()> {
    let remote = connection.remote_address();
    tracing::info!("✅ 新的 QUIC 连接来自: {}", remote);

    while let Ok(stream) = connection.accept_bi().await {
        let cfg_inner = cfg.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_stream(stream, &cfg_inner).await {
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
) -> Result<()> {
    let (mut send, mut recv) = stream;

    loop {
        let data = match read_message(&mut recv).await {
            Ok(Some(d)) => d,
            Ok(None) => break,
            Err(e) => {
                tracing::warn!("读取消息失败: {}", e);
                break;
            }
        };

        match crate::protocol::Envelope::decode(&data) {
            Ok(envelope) => {
                let response = crate::handler::handle_envelope(&envelope, cfg);
                match response.encode() {
                    Ok(resp_bytes) => {
                        if let Err(e) = write_message(&mut send, &resp_bytes).await {
                            tracing::warn!("发送响应失败: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("编码响应失败: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("解码消息失败: {}", e);
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