use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::config::AgentConfig;

pub async fn run(cfg: AgentConfig, certs: Vec<CertificateDer<'static>>, key: PrivateKeyDer<'static>) -> Result<()> {
    let addr = format!("{}:{}", cfg.server.bind, cfg.server.ws_port);
    tracing::info!("🟢 WebSocket 服务器监听: {}", addr);

    let tls_config = build_tls_config(certs, key)?;
    let tls_acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls_config));

    let listener = TcpListener::bind(&addr).await?;

    loop {
        let (tcp_stream, remote_addr) = listener.accept().await?;
        let acceptor = tls_acceptor.clone();
        let cfg_clone = cfg.clone();

        tokio::spawn(async move {
            match acceptor.accept(tcp_stream).await {
                Ok(tls_stream) => {
                    tracing::info!("WSS 连接来自: {}", remote_addr);
                    match tokio_tungstenite::accept_async(tls_stream).await {
                        Ok(ws_stream) => {
                            handle_ws_socket(ws_stream, cfg_clone).await;
                        }
                        Err(e) => {
                            tracing::warn!("WebSocket 协议升级失败: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("TLS 握手失败 ({}): {}", remote_addr, e);
                }
            }
        });
    }
}

fn build_tls_config(
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<rustls::ServerConfig> {
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(config)
}

async fn handle_ws_socket(mut ws_stream: tokio_tungstenite::WebSocketStream<tokio_rustls::server::TlsStream<tokio::net::TcpStream>>, cfg: AgentConfig) {
    tracing::info!("WebSocket 会话已建立");

    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(WsMessage::Binary(data)) => {
                match crate::protocol::Envelope::decode(&data) {
                    Ok(envelope) => {
                        let response = crate::handler::handle_envelope(&envelope, &cfg).await;
                        if let Ok(resp_bytes) = response.encode() {
                            if ws_stream.send(WsMessage::Binary(resp_bytes)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("WS 消息解码失败: {}", e);
                    }
                }
            }
            Ok(WsMessage::Close(_)) => {
                tracing::info!("WebSocket 客户端断开");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("WS 接收错误: {}", e);
                break;
            }
        }
    }

    tracing::info!("WebSocket 会话结束");
}