use anyhow::Result;
use clap::Parser;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};
use quireld::auth::CompositeAuthenticator;

// 使用库中的模块
use quireld::{config, cert, event_bus, subscription, audit, server};

#[cfg(unix)]
use quireld::manager::Manager;

#[derive(Parser, Debug)]
#[command(name = "quireld")]
#[command(about = "Quirel Control — 远程 Agent 服务端")]
struct Args {
    /// 配置文件路径
    #[arg(short, long, default_value = "quireld.toml")]
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

    // ========================================================================
    // 隔离子命令参数（posix_spawn 方案：Manager 通过 std::process::Command
    // 拉起自身二进制并以 --isolated-* 子命令模式运行；子进程不启动 tokio runtime）
    // ========================================================================
    /// 启动隔离 writer 子进程（参数: temp_path）
    #[arg(long, hide = true)]
    isolated_writer: Option<String>,

    /// 启动隔离 reader 子进程（参数: path）
    #[arg(long, hide = true)]
    isolated_reader: Option<String>,

    /// 启动隔离 writer_part 子进程（参数: part_path）
    #[arg(long, hide = true)]
    isolated_writer_part: Option<String>,

    /// 启动隔离 merger 子进程（part_paths 通过 --part-paths 传递）
    #[arg(long, hide = true)]
    isolated_merger: bool,

    /// 启动 metadata 查询子进程（参数: path）
    #[arg(long, hide = true)]
    metadata: Option<String>,

    // 子命令通用参数
    #[arg(long, hide = true)]
    uid: Option<u32>,
    #[arg(long, hide = true)]
    gid: Option<u32>,
    /// writer 的 final_path
    #[arg(long, hide = true)]
    final_path: Option<String>,
    /// merger 的 part_paths（逗号分隔）
    #[arg(long, hide = true)]
    part_paths: Option<String>,
}

/// 初始化日志系统
///
/// - **调试模式**：RUST_LOG 环境变量存在时，优先使用；stdout 使用 pretty 格式
/// - **生产模式**：默认紧凑格式输出到 stdout + JSON 结构化写入日志文件（按天轮转）
/// - **日志文件**：`{log_dir}/quireld.YYYY-MM-DD.log`（JSON 格式，便于日志聚合分析）
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
            .unwrap_or_else(|_| "quireld=debug".into())
    } else {
        // 生产模式：使用配置文件中的日志级别
        // 注意:quireld 是二进制 crate,quireld 也是库 crate(manager/worker 等模块在其中)
        tracing_subscriber::EnvFilter::new(format!(
            "quireld={},tokio=info",
            log_level
        ))
    };

    // ── 日志文件层（JSON 结构化，按天轮转） ────────────────────
    if log_dir != "off" {
        let file_appender = rolling::daily(log_dir, "quireld.log");
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
                            "quireld=debug,tokio=info",
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
                            "quireld=debug,tokio=info",
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

    // 检查是否激活了隔离子命令（posix_spawn 方案：子进程不启动 tokio runtime）
    #[cfg(unix)]
    if dispatch_isolated_command(&args).await? {
        return Ok(());
    }

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
#[cfg(unix)]
async fn run_worker_mode(args: &Args) -> Result<()> {
    let ipc_socket_path = args.ipc_socket.clone()
        .ok_or_else(|| anyhow::anyhow!("Worker 模式必须指定 --ipc-socket 参数"))?;

    // 初始化简单的日志（Worker 模式使用简化日志）
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Worker 模式启动，连接到: {}", ipc_socket_path);

    // 初始化 IpcClient 并连接到 Manager
    let ipc_client = quireld::worker::IpcClient::connect(&ipc_socket_path).await?;
    quireld::worker::run(ipc_client).await?;

    tracing::info!("Worker 进程已退出");

    Ok(())
}

/// Worker 模式入口（非 Unix 平台 stub）
#[cfg(not(unix))]
async fn run_worker_mode(_args: &Args) -> Result<()> {
    anyhow::bail!("Worker 模式仅在 Unix 系统上可用")
}

/// Manager 模式入口（原有的主逻辑）
async fn run_manager_mode(args: &Args) -> Result<()> {
    let cfg = config::load(&args.config)?;

    // 上传大小限制：初始化全局热值与配置写回路径（设置页修改时写回此文件）
    quireld::transfer_limit::init(cfg.limits.max_file_transfer_mb, &args.config);

    init_logging(&args, &cfg);

    let (cert, key) = cert::ensure_certificate(&cfg)?;

    // 日志模式标识
    let log_mode = if std::env::var("RUST_LOG").is_ok() { "debug (RUST_LOG)" } else { "production" };
    tracing::info!("Quireld 启动中...");
    tracing::info!("   日志模式: {}", log_mode);
    tracing::info!("   日志级别: {}", cfg.log.level);
    tracing::info!("   日志目录: {}", if cfg.log.dir == "off" { "关闭".to_string() } else { cfg.log.dir.clone() });
    tracing::info!("   QUIC  监听: udp://{}:{}", cfg.server.bind, cfg.server.quic_port);
    tracing::info!("   WS    监听: tcp://{}:{}", cfg.server.bind, cfg.server.ws_port);

    // 创建 EventBus 和 SubscriptionManager
    let event_bus = Arc::new(event_bus::EventBus::new());
    let subscription_manager = Arc::new(subscription::SubscriptionManager::new(cfg.clone(), event_bus.clone()));

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
    // 阶段 2:包装为 Arc<Manager>,以便共享给 quic::run 等并发任务
    #[cfg(unix)]
    let manager = {
        let m = Manager::new(&cfg).await?;
        tracing::info!("Manager 已实例化,正在启动...");
        // start 后包装成 Arc(阶段 2:start/shutdown 已改为 &self)
        Arc::new(m)
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
            authenticator.clone(),
            audit_log.clone(),
            #[cfg(unix)]
            manager.clone(),
        ),
        server::websocket::run(cfg.clone(), cert, key),
    );

    // 阶段 1 新增:QUIC/WS 退出后停止 Manager
    #[cfg(unix)]
    manager.shutdown().await?;

    result?;
    Ok(())
}

// ============================================================================
// 隔离子命令分发（posix_spawn 方案）
// ============================================================================
//
// Manager 侧通过 std::process::Command 拉起自身二进制并以 --isolated-* 子命令
// 模式运行；子进程不启动 tokio runtime，仅做同步 I/O，与原 fork 子进程行为一致。
// 降权逻辑（setgid/setuid）在 dispatch_isolated_command 中统一处理。

/// 子命令分发入口：返回 true 表示已处理子命令，main 应直接退出
#[cfg(unix)]
async fn dispatch_isolated_command(args: &Args) -> Result<bool> {
    // 互斥检查：最多一个子命令被激活
    let active: Vec<&str> = [
        args.isolated_writer.as_ref().map(|_| "writer"),
        args.isolated_reader.as_ref().map(|_| "reader"),
        args.isolated_writer_part.as_ref().map(|_| "writer_part"),
        args.isolated_merger.then(|| "merger"),
        args.metadata.as_ref().map(|_| "metadata"),
    ].into_iter().flatten().collect();

    match active.len() {
        0 => Ok(false),
        1 => {
            let uid = args.uid.unwrap_or(0);
            let gid = args.gid.unwrap_or(0);
            // 降权（setgid 先于 setuid，顺序重要）
            unsafe {
                if libc::setgid(gid) != 0 {
                    std::process::exit(1);
                }
                if libc::setuid(uid) != 0 {
                    std::process::exit(1);
                }
            }
            // 验证降权成功
            if unsafe { libc::getuid() } != uid || unsafe { libc::getgid() } != gid {
                std::process::exit(1);
            }

            // 按激活的子命令分发
            if let Some(temp_path) = &args.isolated_writer {
                run_isolated_writer(temp_path, args.final_path.as_ref().unwrap())?;
            } else if let Some(path) = &args.isolated_reader {
                run_isolated_reader(path)?;
            } else if let Some(part_path) = &args.isolated_writer_part {
                run_isolated_writer_part(part_path)?;
            } else if args.isolated_merger {
                let parts: Vec<String> = args.part_paths.as_ref().unwrap()
                    .split(',').map(String::from).collect();
                run_isolated_merger(&parts, args.final_path.as_ref().unwrap())?;
            } else if let Some(path) = &args.metadata {
                run_metadata(path)?;
            }
            Ok(true)
        }
        _ => Err(anyhow::anyhow!("只能指定一个隔离子命令")),
    }
}

/// 子命令分发入口（非 Unix 平台 stub）
#[cfg(not(unix))]
async fn dispatch_isolated_command(_args: &Args) -> Result<bool> {
    Ok(false)
}

// ============================================================================
// 5 个隔离子命令实现（同步 I/O，不启动 tokio runtime）
// ============================================================================

/// 隔离写入子进程：从 stdin 读 → 写文件 → flush+sync+rename
/// 退出码：1=降权失败(已在 dispatch 中处理) / 2=打开文件失败 / 3=写入失败
/// / 4=读取 stdin 失败 / 5=flush 失败 / 6=sync_all 失败 / 7=rename 失败
#[cfg(unix)]
fn run_isolated_writer(temp_path: &str, final_path: &str) -> Result<()> {
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;

    // 打开临时文件（O_NOFOLLOW 防符号链接劫持，0o600 限属主读写）
    let file = match std::fs::OpenOptions::new()
        .create(true).write(true).truncate(true)
        .mode(0o600).custom_flags(libc::O_NOFOLLOW)
        .open(temp_path)
    {
        Ok(f) => f,
        Err(_) => std::process::exit(2),
    };
    let mut buf_writer = std::io::BufWriter::with_capacity(256 * 1024, file);
    let mut stdin = std::io::stdin();
    let mut buf = [0u8; 256 * 1024];
    loop {
        match stdin.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if buf_writer.write_all(&buf[..n]).is_err() {
                    std::process::exit(3);
                }
            }
            Err(_) => std::process::exit(4),
        }
    }
    if buf_writer.flush().is_err() {
        std::process::exit(5);
    }
    if buf_writer.get_ref().sync_all().is_err() {
        std::process::exit(6);
    }
    drop(buf_writer);
    if std::fs::rename(temp_path, final_path).is_err() {
        std::process::exit(7);
    }
    std::process::exit(0);
}

/// 隔离读取子进程：读文件 → 写 stdout
/// 退出码：2=打开文件失败 / 3=写入 stdout 失败 / 4=读取文件失败
#[cfg(unix)]
fn run_isolated_reader(path: &str) -> Result<()> {
    use std::io::{Read, Write};
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => std::process::exit(2),
    };
    let mut buf_reader = std::io::BufReader::with_capacity(256 * 1024, file);
    let mut stdout = std::io::stdout();
    let mut buf = [0u8; 256 * 1024];
    loop {
        match buf_reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if stdout.write_all(&buf[..n]).is_err() {
                    std::process::exit(3);
                }
            }
            Err(_) => std::process::exit(4),
        }
    }
    std::process::exit(0);
}

/// 隔离段写入子进程：从 stdin 读 → 写段文件 → flush+sync（不 rename）
/// 退出码：2=打开段文件失败 / 3=写入段文件失败 / 4=读取 stdin 失败
/// / 5=flush 失败 / 6=sync_all 失败
#[cfg(unix)]
fn run_isolated_writer_part(part_path: &str) -> Result<()> {
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;
    let file = match std::fs::OpenOptions::new()
        .create(true).write(true).truncate(true)
        .mode(0o600).custom_flags(libc::O_NOFOLLOW)
        .open(part_path)
    {
        Ok(f) => f,
        Err(_) => std::process::exit(2),
    };
    let mut buf_writer = std::io::BufWriter::with_capacity(256 * 1024, file);
    let mut stdin = std::io::stdin();
    let mut buf = [0u8; 256 * 1024];
    loop {
        match stdin.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if buf_writer.write_all(&buf[..n]).is_err() {
                    std::process::exit(3);
                }
            }
            Err(_) => std::process::exit(4),
        }
    }
    if buf_writer.flush().is_err() {
        std::process::exit(5);
    }
    if buf_writer.get_ref().sync_all().is_err() {
        std::process::exit(6);
    }
    std::process::exit(0);
}

/// 隔离合并子进程：按 part_paths 顺序合并段文件为最终文件
/// 退出码：10=打开最终文件失败 / 11=打开段文件失败 / 12=读取段文件失败
/// / 13=写入最终文件失败 / 14=sync_all 失败
#[cfg(unix)]
fn run_isolated_merger(part_paths: &[String], final_path: &str) -> Result<()> {
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;
    let final_file = match std::fs::OpenOptions::new()
        .create(true).write(true).truncate(true)
        .mode(0o600).custom_flags(libc::O_NOFOLLOW)
        .open(final_path)
    {
        Ok(f) => f,
        Err(_) => std::process::exit(10),
    };
    let mut final_writer = std::io::BufWriter::with_capacity(256 * 1024, final_file);
    let mut buf = [0u8; 256 * 1024];
    for part_path in part_paths {
        let mut part_file = match std::fs::File::open(part_path) {
            Ok(f) => f,
            Err(_) => std::process::exit(11),
        };
        loop {
            match part_file.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if final_writer.write_all(&buf[..n]).is_err() {
                        std::process::exit(13);
                    }
                }
                Err(_) => std::process::exit(12),
            }
        }
    }
    if final_writer.flush().is_err() {
        std::process::exit(13);
    }
    if final_writer.get_ref().sync_all().is_err() {
        std::process::exit(14);
    }
    drop(final_writer);
    // 删除所有段文件（清理）
    for part_path in part_paths {
        if std::fs::remove_file(part_path).is_err() {
            // 段文件删除失败不致命（最终文件已完整），记录但继续
            eprintln!("[merger] 删除段文件失败: {}", part_path);
        }
    }
    std::process::exit(0);
}

/// 元数据查询子进程：获取文件 size 和 mtime，输出 JSON 到 stdout
/// 退出码：2=获取元数据失败
#[cfg(unix)]
fn run_metadata(path: &str) -> Result<()> {
    use std::io::Write;
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => std::process::exit(2),
    };
    if metadata.is_dir() {
        eprintln!("路径是目录，不能下载");
        std::process::exit(2);
    }
    let size = metadata.len();
    let mtime = metadata.modified()
        .map_err(|_| std::process::exit(2))
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 输出 JSON: {"size": N, "mtime": N}
    let json = format!("{{\"size\":{},\"mtime\":{}}}", size, mtime);
    let _ = std::io::stdout().write_all(json.as_bytes());
    std::process::exit(0);
}