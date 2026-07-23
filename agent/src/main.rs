use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

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

    /// 日志输出目录（默认: ./logs）；设为 "off" 关闭文件日志
    #[arg(long, default_value = "logs")]
    log_dir: String,

    /// 日志级别（trace/debug/info/warn/error），优先级低于 RUST_LOG 环境变量
    #[arg(long, default_value = "info")]
    log_level: String,
}

/// 初始化日志系统
///
/// - **调试模式**：RUST_LOG 环境变量存在时，优先使用；stdout 使用 pretty 格式
/// - **生产模式**：默认紧凑格式输出到 stdout + JSON 结构化写入日志文件（按天轮转）
/// - **日志文件**：`{log_dir}/agent.YYYY-MM-DD.log`（JSON 格式，便于日志聚合分析）
fn init_logging(args: &Args) {
    use tracing_appender::rolling;

    // ── 环境判断 ──────────────────────────────────────────────
    // RUST_LOG 存在 → 调试模式（开发者手动设置了过滤规则）
    let is_debug = std::env::var("RUST_LOG").is_ok();

    // ── 过滤层 ────────────────────────────────────────────────
    let env_filter = if is_debug {
        // 调试模式：完全尊重 RUST_LOG 环境变量
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "agent=debug".into())
    } else {
        // 生产模式：使用命令行参数或默认 info
        tracing_subscriber::EnvFilter::new(format!(
            "agent={},agent::server={},agent::handler={},tokio=info",
            args.log_level, args.log_level, args.log_level
        ))
    };

    // ── 日志文件层（JSON 结构化，按天轮转） ────────────────────
    if args.log_dir != "off" {
        let file_appender = rolling::daily(&args.log_dir, "agent.log");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

        // 将 guard 泄漏到全局，防止日志文件句柄在 init() 后被关闭
        // （tracing-appender 的 non_blocking guard 需要存活到进程结束）
        std::mem::forget(_guard);

        // 注意：stdout 层必须先注册（此时 subscriber 类型为 Registry），
        // file_layer 后注册（subscriber 类型变为 Layered<...>）
        // 这样 Box<dyn Layer<Registry>> 才能正确匹配
        if is_debug {
            // 调试模式：pretty stdout + JSON 文件
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .pretty()
                        .with_filter(env_filter),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_ansi(false)
                        .with_writer(non_blocking)
                        .with_filter(tracing_subscriber::EnvFilter::new(
                            "agent=debug,agent::server=debug,agent::handler=debug,tokio=info",
                        )),
                )
                .init();
        } else {
            // 生产模式：compact stdout + JSON 文件
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .compact()
                        .with_filter(env_filter),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_ansi(false)
                        .with_writer(non_blocking)
                        .with_filter(tracing_subscriber::EnvFilter::new(
                            "agent=debug,agent::server=debug,agent::handler=debug,tokio=info",
                        )),
                )
                .init();
        }
    } else {
        // 无文件日志模式
        if is_debug {
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .pretty()
                        .with_filter(env_filter),
                )
                .init();
        } else {
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .compact()
                        .with_filter(env_filter),
                )
                .init();
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 安装 rustls CryptoProvider (ring)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let args = Args::parse();

    init_logging(&args);

    let cfg = config::load(&args.config)?;

    // 启动时清理过期的临时文件（上次运行中断遗留的 .tmp 文件）
    if !cfg.security.allowed_paths.is_empty() {
        file_stream::cleanup_stale_temp_files(&cfg.security.allowed_paths, None);
    }

    let (cert, key) = cert::ensure_certificate(&cfg)?;

    // 日志模式标识
    let log_mode = if std::env::var("RUST_LOG").is_ok() { "debug (RUST_LOG)" } else { "production" };
    tracing::info!("GNOME Remote Agent 启动中...");
    tracing::info!("   日志模式: {}", log_mode);
    tracing::info!("   日志目录: {}", if args.log_dir == "off" { "关闭".to_string() } else { args.log_dir.clone() });
    tracing::info!("   QUIC  监听: udp://{}:{}", cfg.server.bind, cfg.server.quic_port);
    tracing::info!("   WS    监听: tcp://{}:{}", cfg.server.bind, cfg.server.ws_port);

    if cfg.auth.token.is_empty() {
        tracing::warn!("未设置认证 Token，首次启动将自动生成");
    } else {
        let masked: String = cfg.auth.token.chars().take(12).collect();
        tracing::info!("认证 Token: {}...", masked);
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