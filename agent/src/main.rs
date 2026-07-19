use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod cert;
mod collectors;
mod config;
mod event_bus;
mod file_stream; // 文件流处理模块（上传下载）
mod handler;
mod protocol;
mod server;
mod subscription;
mod pty;
mod diff; // 差异计算模块（文件编辑器流量优化）
mod transfer_session; // 传输会话管理模块

#[derive(Parser, Debug)]
#[command(name = "gnome-remote-agent")]
#[command(about = "GNOME Remote Control — 远程 Agent 服务端")]
struct Args {
    /// 配置文件路径
    #[arg(short, long, default_value = "agent.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 安装 rustls CryptoProvider (ring)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let args = Args::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agent=info,agent::server=info,agent::handler=info,tokio=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cfg = config::load(&args.config)?;

    let (cert, key) = cert::ensure_certificate(&cfg)?;

    tracing::info!("🚀 GNOME Remote Agent 启动中...");
    tracing::info!("   QUIC  监听: udp://{}:{}", cfg.server.bind, cfg.server.quic_port);
    tracing::info!("   WS    监听: tcp://{}:{}", cfg.server.bind, cfg.server.ws_port);

    if cfg.auth.token.is_empty() {
        tracing::warn!("⚠️  未设置认证 Token，首次启动将自动生成");
    } else {
        let masked: String = cfg.auth.token.chars().take(12).collect();
        tracing::info!("🔑 认证 Token: {}...", masked);
    }

    // 创建 EventBus 和 SubscriptionManager
    let event_bus = Arc::new(event_bus::EventBus::new());
    let subscription_manager = Arc::new(subscription::SubscriptionManager::new(cfg.clone(), event_bus.clone()));

    // 创建 PTY 管理器
    let pty_manager = Arc::new(pty::PtyManager::new());

    let key_clone = key.clone_key();
    tokio::try_join!(
        server::quic::run(cfg.clone(), cert.clone(), key_clone, subscription_manager.clone(), event_bus.clone(), pty_manager.clone()),
        server::websocket::run(cfg.clone(), cert, key),
    )?;

    Ok(())
}