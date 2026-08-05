use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};
use gnome_remote_agent::auth::CompositeAuthenticator;

// 使用库中的模块
use gnome_remote_agent::{config, cert, event_bus, subscription, pty, audit, server};

#[cfg(unix)]
use gnome_remote_agent::manager::Manager;

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

    /// Worker 模式标志（由 Manager 自动启动，不应手动指定）
    #[arg(long, hide = true)]
    worker: bool,

    /// IPC Socket 路径（Worker 模式必须指定）
    #[arg(long, hide = true)]
    ipc_socket: Option<String>,
}

/// 初始化日志系统
///
/// - **调试模式**：RUST_LOG 环境变量存在时，优先使用；stdout 使用 pretty 格式
/// - **生产模式**：默认紧凑格式输出到 stdout + JSON 结构化写入日志文件（按天轮转）
/// - **日志文件**：`{log_dir}/agent.YYYY-MM-DD.log`（JSON 格式，便于日志聚合分析）
/// - **时间格式**：北京时间（UTC+8），格式：YYYY-MM-DD HH:MM:SS.mmm
fn init_logging(_args: &Args, cfg: &config::AgentConfig) {
    use tracing_appender::rolling;
    use tracing_subscriber::fmt::time::FormatTime;
    use tracing_subscriber::fmt::format::Writer;

    // ── 北京时间格式化器 ──────────────────────────────────────
    /// 北京时间格式化器（UTC+8）
    struct BeijingTime;

    impl FormatTime for BeijingTime {
        fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
            // 获取当前 UTC 时间，转换为北京时间
            let now = chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap());
            write!(w, "{}", now.format("%Y-%m-%d %H:%M:%S%.3f"))
        }
    }

    // ── 环境判断 ──────────────────────────────────────────────
    // RUST_LOG 存在 → 调试模式（开发者手动设置了过滤规则）
    let is_debug = std::env::var("RUST_LOG").is_ok();

    // 使用配置文件中的日志级别和目录
    let log_level = &cfg.log.level;
    let log_dir = &cfg.log.dir;

    // ── 过滤层 ────────────────────────────────────────────────
    let env_filter = if is_debug {
        // 调试模式：完全尊重 RUST_LOG 环境变量
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "agent=debug".into())
    } else {
        // 生产模式：使用配置文件中的日志级别
        // 注意:agent 是二进制 crate,gnome_remote_agent 是库 crate(manager/worker 等模块在其中)
        tracing_subscriber::EnvFilter::new(format!(
            "agent={},agent::server={},agent::handler={},gnome_remote_agent={},tokio=info",
            log_level, log_level, log_level, log_level
        ))
    };

    // ── 日志文件层（JSON 结构化，按天轮转） ────────────────────
    if log_dir != "off" {
        let file_appender = rolling::daily(log_dir, "agent.log");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

        // 将 guard 泄漏到全局，防止日志文件句柄在 init() 后被关闭
        // （tracing-appender 的 non_blocking guard 需要存活到进程结束）
        std::mem::forget(_guard);

        if is_debug {
            // 调试模式：pretty stdout（北京时间） + JSON 文件
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .pretty()
                        .with_timer(BeijingTime)
                        .with_filter(env_filter),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_ansi(false)
                        .with_writer(non_blocking)
                        .with_filter(tracing_subscriber::EnvFilter::new(
                            "agent=debug,agent::server=debug,agent::handler=debug,gnome_remote_agent=debug,tokio=info",
                        )),
                )
                .init();
        } else {
            // 生产模式：自定义格式 stdout（北京时间，带颜色） + JSON 文件
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_timer(BeijingTime)
                        .with_target(true)
                        .with_thread_ids(false)
                        .with_thread_names(false)
                        .with_ansi(true)
                        .with_filter(env_filter),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .json()
                        .with_ansi(false)
                        .with_writer(non_blocking)
                        .with_filter(tracing_subscriber::EnvFilter::new(
                            "agent=debug,agent::server=debug,agent::handler=debug,gnome_remote_agent=debug,tokio=info",
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
                        .with_timer(BeijingTime)
                        .with_filter(env_filter),
                )
                .init();
        } else {
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_timer(BeijingTime)
                        .with_target(true)
                        .with_thread_ids(false)
                        .with_thread_names(false)
                        .with_ansi(true)
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

    // 判断运行模式
    if args.worker {
        // Worker 模式
        run_worker_mode(&args).await?;
    } else {
        // Manager 模式（默认）
        run_manager_mode(&args).await?;
    }

    Ok(())
}

/// Worker 模式入口
async fn run_worker_mode(args: &Args) -> Result<()> {
    let ipc_socket_path = args.ipc_socket.clone()
        .ok_or_else(|| anyhow::anyhow!("Worker 模式必须指定 --ipc-socket 参数"))?;

    // 初始化简单的日志（Worker 模式使用简化日志）
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Worker 模式启动，连接到: {}", ipc_socket_path);

    // 初始化 IpcClient 并连接到 Manager
    let ipc_client = gnome_remote_agent::worker::IpcClient::connect(&ipc_socket_path).await?;
    gnome_remote_agent::worker::run(ipc_client).await?;

    tracing::info!("Worker 进程已退出");

    Ok(())
}

/// Manager 模式入口（原有的主逻辑）
async fn run_manager_mode(args: &Args) -> Result<()> {
    let cfg = config::load(&args.config)?;

    init_logging(&args, &cfg);

    let (cert, key) = cert::ensure_certificate(&cfg)?;

    // 日志模式标识
    let log_mode = if std::env::var("RUST_LOG").is_ok() { "debug (RUST_LOG)" } else { "production" };
    tracing::info!("GNOME Remote Agent 启动中...");
    tracing::info!("   日志模式: {}", log_mode);
    tracing::info!("   日志级别: {}", cfg.log.level);
    tracing::info!("   日志目录: {}", if cfg.log.dir == "off" { "关闭".to_string() } else { cfg.log.dir.clone() });
    tracing::info!("   QUIC  监听: udp://{}:{}", cfg.server.bind, cfg.server.quic_port);
    tracing::info!("   WS    监听: tcp://{}:{}", cfg.server.bind, cfg.server.ws_port);

    // 创建 EventBus 和 SubscriptionManager
    let event_bus = Arc::new(event_bus::EventBus::new());
    let subscription_manager = Arc::new(subscription::SubscriptionManager::new(cfg.clone(), event_bus.clone()));

    // 创建 PTY 管理器
    let pty_manager = Arc::new(pty::PtyManager::new());

    // 初始化审计日志
    let audit_log = Arc::new(audit::AuditLogger::new(&cfg.audit.log_path)
        .expect("无法创建审计日志文件"));
    tracing::info!("审计日志已启用: {}", cfg.audit.log_path);

    // 初始化认证器
    let authenticator = Arc::new(CompositeAuthenticator::new(
        cfg.auth.ssh.pam_service.clone(),
        cfg.auth.ssh.enable_pubkey,
        cfg.auth.ssh.enable_password,
    ));
    tracing::info!("认证器已初始化 (公钥认证: {}, 密码认证: {})",
        cfg.auth.ssh.enable_pubkey, cfg.auth.ssh.enable_password);

    // 阶段 1 新增:实例化并启动 Manager(IPC + Worker + CrashDetector)
    #[cfg(unix)]
    let mut manager = {
        let m = Manager::new(&cfg).await?;
        tracing::info!("Manager 已实例化,正在启动...");
        m
    };
    #[cfg(unix)]
    manager.start().await?;

    let key_clone = key.clone_key();
    let result = tokio::try_join!(
        server::quic::run(
            cfg.clone(),
            cert.clone(),
            key_clone,
            subscription_manager.clone(),
            event_bus.clone(),
            pty_manager.clone(),
            authenticator.clone(),
            audit_log.clone(),
        ),
        server::websocket::run(cfg.clone(), cert, key),
    );

    // 阶段 1 新增:QUIC/WS 退出后停止 Manager
    #[cfg(unix)]
    manager.shutdown().await?;

    result?;
    Ok(())
}