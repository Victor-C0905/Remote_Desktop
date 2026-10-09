use anyhow::{Result, anyhow};
use futures_util::{SinkExt, StreamExt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::config::AgentConfig;

pub async fn run(cfg: AgentConfig, certs: Vec<CertificateDer<'static>>, key: PrivateKeyDer<'static>) -> Result<()> {
    let addr = format!("{}:{}", cfg.server.bind, cfg.server.ws_port);
    tracing::info!("🟢 WebSocket 服务器监听: {}", addr);

    let tls_config = build_tls_config(certs, key)?;
    let tls_acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls_config));

    // 手动创建 TCP socket 并设置 SO_REUSEADDR，解决 systemctl restart 时端口释放延迟问题
    // SO_REUSEADDR 对 TCP 只能复用 TIME_WAIT 端口，不能复用旧进程仍在监听的端口
    // 因此还需重试机制（最多 5 次，每次 1 秒），等待旧进程退出释放端口
    let listener = {
        let sock_addr: std::net::SocketAddr = addr.parse()?;
        let mut retries = 0;
        const MAX_RETRIES: u32 = 5;
        loop {
            let socket = socket2::Socket::new(
                socket2::Domain::for_address(sock_addr),
                socket2::Type::STREAM,
                None,
            )?;
            socket.set_reuse_address(true)?;
            socket.set_nonblocking(true)?;

            match socket.bind(&sock_addr.into()) {
                Ok(()) => {
                    socket.listen(1024)?;
                    tracing::info!("🟢 WebSocket socket 已绑定: {}", addr);
                    break tokio::net::TcpListener::from_std(socket.into())?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && retries < MAX_RETRIES => {
                    retries += 1;
                    tracing::warn!(
                        "WebSocket 端口 {} 被占用，等待重试 ({}/{}): {}",
                        addr, retries, MAX_RETRIES, e
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                Err(e) => {
                    return Err(anyhow!(
                        "WebSocket bind {} 失败 (重试 {} 次后放弃): {}",
                        addr, retries, e
                    ));
                }
            }
        }
    };

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

async fn handle_ws_socket(mut ws_stream: tokio_tungstenite::WebSocketStream<tokio_rustls::server::TlsStream<tokio::net::TcpStream>>, _cfg: AgentConfig) {
    tracing::info!("WebSocket 会话已建立");

    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(WsMessage::Binary(data)) => {
                match crate::protocol::Envelope::decode(&data) {
                    Ok(envelope) => {
                        // TODO: Phase 3集成认证流程时恢复,需要传入 session 参数
                        // let response = crate::handler::handle_envelope(&envelope, &cfg, &session).await;
                        let response = crate::protocol::Envelope::new(
                            envelope.request_id,
                            crate::protocol::Payload::Error {
                                code: -1,
                                message: "认证功能尚未实现".to_string(),
                            },
                        );
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