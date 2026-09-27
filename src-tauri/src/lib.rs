use serde::Serialize;
use std::fs;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tauri::Manager;  // 导入 Manager trait
use tokio::sync::Mutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

mod connection;
mod proxy;
mod terminal;
mod transfer;
mod ui_logging;

/* ── 日志系统初始化 ───────────────────────────────────────── */

/// 初始化客户端日志系统
///
/// - **调试模式**（`RUST_LOG` 环境变量存在）：pretty 格式输出到 stdout
/// - **生产模式**：紧凑格式 stdout + JSON 结构化写入日志文件（按天轮转）
/// - 日志文件路径：`{app_data_dir}/logs/client.YYYY-MM-DD.log`
/// - **时间格式**：北京时间（UTC+8），格式：YYYY-MM-DD HH:MM:SS.mmm
fn init_logging(app: &tauri::App) {
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

    let is_debug = std::env::var("RUST_LOG").is_ok();

    // 默认过滤规则：客户端模块 info，第三方库 warn
    let default_filter = "quirel_lib=info,quirel_lib::connection=info,quirel_lib::transfer=info,tokio=info";

    let env_filter = if is_debug {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "quirel_lib=debug".into())
    } else {
        tracing_subscriber::EnvFilter::new(default_filter)
    };

    // 日志文件目录：优先使用 Tauri 数据目录，保证跨平台写入合法
    let log_dir = app
        .path()
        .app_data_dir()
        .ok()
        .map(|p| p.join("logs"))
        .unwrap_or_else(|| std::path::PathBuf::from("logs"));

    let _ = fs::create_dir_all(&log_dir);

    let file_appender = rolling::daily(&log_dir, "client.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    // 泄漏 guard 使其存活到进程结束
    std::mem::forget(_guard);

    // stdout 层先注册（subscriber 类型为 Registry），file_layer 后注册
    if is_debug {
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
                        "quirel_lib=debug,quirel_lib::connection=debug,quirel_lib::transfer=debug,tokio=info",
                    )),
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
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_ansi(false)
                    .with_writer(non_blocking)
                    .with_filter(tracing_subscriber::EnvFilter::new(
                        "quirel_lib=debug,quirel_lib::connection=debug,quirel_lib::transfer=debug,tokio=info",
                    )),
            )
            .init();
    }

    // 桥接 rustls / tokio-rustls 的 `log` crate 输出到 tracing
    // （这些 crate 使用 `log` 宏，不桥接的话它们的日志会被丢弃）
    tracing_log::LogTracer::init().ok();
}

/* ── 优雅关闭机制 ───────────────────────────────────────── */

/// 全局关闭标志
pub static SHUTDOWN_FLAG: once_cell::sync::Lazy<Arc<Mutex<bool>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(false)));

/// 注册关闭钩子
pub fn setup_shutdown_hook(app: &tauri::AppHandle) {
    let app_handle = app.clone();

    // Ctrl+C 处理
    ctrlc::set_handler(move || {
        tracing::info!("[Shutdown] 收到关闭信号");
        let app = app_handle.clone();
        tokio::spawn(async move {
            graceful_shutdown(app).await;
        });
    }).expect("无法设置 Ctrl+C 处理器");
}

/// 优雅关闭
async fn graceful_shutdown(app: tauri::AppHandle) {
    tracing::info!("[Shutdown] 开始优雅关闭...");

    // 1. 设置关闭标志
    *SHUTDOWN_FLAG.lock().await = true;

    // 2. 取消所有传输任务
    if let Some(transfer_manager) = app.try_state::<Arc<transfer::TransferManager>>() {
        let tasks = transfer_manager.list_tasks().await;
        for task in tasks {
            if task.status == transfer::TransferStatus::Pending || task.status == transfer::TransferStatus::Active {
                let _ = transfer_manager.cancel_task(&task.id, &app).await;
            }
        }
    }

    // 3. 断开所有连接
    if let Some(connection_manager) = app.try_state::<connection::ConnectionManager>() {
        // 获取所有连接 ID
        let server_ids: Vec<String> = {
            let conns = connection_manager.connections.lock().unwrap();
            conns.keys().cloned().collect()
        };

        // 移除连接
        for server_id in &server_ids {
            tracing::info!("[Shutdown] 断开连接: {}", server_id);
            let mut conns = connection_manager.connections.lock().unwrap();
            conns.remove(server_id);
        }
    }

    tracing::info!("[Shutdown] 优雅关闭完成");
    // 不在这里强制退出，让 Tauri 自己管理退出
    // std::process::exit 会导致 WebView2 无法正确清理
}

/* ── Shared Types ────────────────────────────────────────── */

#[derive(Serialize, Clone)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: String,       // ISO 8601
    pub permissions: String, // "rwxr-xr-x"
}

#[derive(Serialize)]
pub struct ReadDirResponse {
    pub path: String,
    pub entries: Vec<FileEntry>,
}

/* ── Platform-specific mode reading ─────────────────────── */

#[cfg(unix)]
fn get_permissions(metadata: &fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    format_mode(metadata.mode())
}

#[cfg(windows)]
fn get_permissions(_metadata: &fs::Metadata) -> String {
    let readonly = _metadata.permissions().readonly();
    if readonly { "r--r--r--".to_string() } else { "rw-rw-rw-".to_string() }
}

#[allow(dead_code)]
fn format_mode(mode: u32) -> String {
    let bits = [
        (0o400, 'r'), (0o200, 'w'), (0o100, 'x'),
        (0o040, 'r'), (0o020, 'w'), (0o010, 'x'),
        (0o004, 'r'), (0o002, 'w'), (0o001, 'x'),
    ];
    bits.iter().map(|(mask, ch)| if mode & mask != 0 { *ch } else { '-' }).collect()
}

/* ── PTY Management (Windows ConPTY) ────────────────────── */

#[cfg(windows)]
mod pty {
    use base64::Engine;
    use serde::Serialize;
    use std::collections::HashMap;
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::sync::Mutex;
    use tauri::Manager;

    #[derive(Serialize)]
    pub struct SpawnResult {
        pub pty_id: String,
    }

    /// Windows ConPTY using the `conpty` crate or raw Win32 API.
    /// For now, we use a simplified approach: spawn a hidden console process
    /// and pipe its I/O.
    pub struct PtySession {
        child: std::process::Child,
    }

    impl PtySession {
        pub fn new(cols: u16, rows: u16) -> Result<Self, String> {
            // Use cmd.exe as the shell on Windows
            let child = std::process::Command::new("cmd.exe")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .env("TERM", "xterm-256color")
                .env("COLUMNS", cols.to_string())
                .env("LINES", rows.to_string())
                .spawn()
                .map_err(|e| format!("启动终端失败: {}", e))?;

            Ok(Self { child })
        }

        pub fn write(&mut self, data: &[u8]) -> Result<(), String> {
            use std::io::Write;
            let stdin = self.child.stdin.as_mut().ok_or("stdin 不可用")?;
            stdin.write_all(data).map_err(|e| format!("写入失败: {}", e))?;
            stdin.flush().map_err(|e| format!("flush 失败: {}", e))?;
            Ok(())
        }

        pub fn read(&mut self, _timeout_ms: u64) -> Result<String, String> {
            let mut buf = [0u8; 4096];
            // Non-blocking: try to read available data
            // On Windows, we use a simple approach with small reads
            match self.child.stdout.as_mut() {
                Some(stdout) => {
                    // Set non-blocking would need platform-specific code
                    // For simplicity, just do a small read
                    match stdout.read(&mut buf) {
                        Ok(0) => Ok(String::new()),
                        Ok(n) => {
                            Ok(base64::engine::general_purpose::STANDARD.encode(&buf[..n]))
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            Ok(String::new())
                        }
                        Err(e) => Err(format!("读取失败: {}", e)),
                    }
                }
                None => Err("stdout 不可用".to_string()),
            }
        }

        pub fn resize(&mut self, _cols: u16, _rows: u16) -> Result<(), String> {
            // ConPTY resize would need Win32 API calls
            // For now, just update env vars (limited effect)
            Ok(())
        }
    }

    /// Global PTY sessions store
    pub struct PtyManager {
        sessions: Mutex<HashMap<String, PtySession>>,
    }

    impl PtyManager {
        pub fn new() -> Self {
            Self {
                sessions: Mutex::new(HashMap::new()),
            }
        }
    }

    // Tauri commands for PTY

    #[tauri::command]
    pub fn spawn_terminal(shell: String, cols: u16, rows: u16, app: tauri::AppHandle) -> Result<SpawnResult, String> {
        tracing::info!("[PTY] 创建终端: shell={}", shell);
        let _ = shell; // Use default shell (cmd.exe on Windows)
        let session = PtySession::new(cols, rows)?;
        let id = format!("pty-{}", uuid::Uuid::new_v4());

        let manager = app.state::<PtyManager>();
        manager.sessions.lock().unwrap().insert(id.clone(), session);

        Ok(SpawnResult { pty_id: id })
    }

    #[tauri::command]
    pub fn terminal_write(pty_id: String, data: String, app: tauri::AppHandle) -> Result<(), String> {
        tracing::debug!("[PTY] 写入数据: pty_id={}, bytes={}", pty_id, data.len());
        let manager = app.state::<PtyManager>();
        let mut sessions = manager.sessions.lock().unwrap();
        let session = sessions.get_mut(&pty_id).ok_or("PTY 会话不存在")?;

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&data)
            .map_err(|e| format!("Base64 解码失败: {}", e))?;

        session.write(&bytes)
    }

    #[tauri::command]
    pub fn terminal_read(pty_id: String, _timeout_ms: u64, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
        tracing::debug!("[PTY] 读取数据: pty_id={}", pty_id);
        let manager = app.state::<PtyManager>();
        let mut sessions = manager.sessions.lock().unwrap();
        let session = sessions.get_mut(&pty_id).ok_or("PTY 会话不存在")?;

        let data = session.read(_timeout_ms)?;
        Ok(serde_json::json!({ "data": data }))
    }

    #[tauri::command]
    pub fn terminal_resize(pty_id: String, cols: u16, rows: u16, app: tauri::AppHandle) -> Result<(), String> {
        tracing::debug!("[PTY] 调整大小: pty_id={}, cols={}, rows={}", pty_id, cols, rows);
        let manager = app.state::<PtyManager>();
        let mut sessions = manager.sessions.lock().unwrap();
        let session = sessions.get_mut(&pty_id).ok_or("PTY 会话不存在")?;
        session.resize(cols, rows)
    }
}

/* ── PTY Management (Unix) ──────────────────────────────── */

#[cfg(unix)]
mod pty {
    use base64::Engine;
    use serde::Serialize;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tauri::Manager;

    #[derive(Serialize)]
    pub struct SpawnResult {
        pub pty_id: String,
    }

    // Unix PTY implementation using nix::pty
    // For MVP, use a simple spawned process with pipes
    pub struct PtySession {
        child: std::process::Child,
    }

    impl PtySession {
        pub fn new(cols: u16, rows: u16) -> Result<Self, String> {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
            let child = std::process::Command::new(&shell)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .env("TERM", "xterm-256color")
                .env("COLUMNS", cols.to_string())
                .env("LINES", rows.to_string())
                .spawn()
                .map_err(|e| format!("启动终端失败: {}", e))?;

            Ok(Self { child })
        }

        pub fn write(&mut self, data: &[u8]) -> Result<(), String> {
            use std::io::Write;
            let stdin = self.child.stdin.as_mut().ok_or("stdin 不可用")?;
            stdin.write_all(data).map_err(|e| format!("写入失败: {}", e))?;
            stdin.flush().map_err(|e| format!("flush 失败: {}", e))
        }

        pub fn read(&mut self, _timeout_ms: u64) -> Result<String, String> {
            use std::io::Read;
            let mut buf = [0u8; 4096];
            match self.child.stdout.as_mut() {
                Some(stdout) => match stdout.read(&mut buf) {
                    Ok(0) => Ok(String::new()),
                    Ok(n) => {
                        Ok(base64::engine::general_purpose::STANDARD.encode(&buf[..n]))
                    }
                    Err(e) => Err(format!("读取失败: {}", e)),
                },
                None => Err("stdout 不可用".to_string()),
            }
        }

        pub fn resize(&mut self, _cols: u16, _rows: u16) -> Result<(), String> {
            // True PTY resize would use ioctl(TIOCSWINSZ)
            Ok(())
        }
    }

    pub struct PtyManager {
        sessions: Mutex<HashMap<String, PtySession>>,
    }

    impl PtyManager {
        pub fn new() -> Self {
            Self { sessions: Mutex::new(HashMap::new()) }
        }
    }

    #[tauri::command]
    pub fn spawn_terminal(shell: String, cols: u16, rows: u16, app: tauri::AppHandle) -> Result<SpawnResult, String> {
        tracing::info!("[PTY] 创建终端: shell={}", shell);
        let _ = shell;
        let session = PtySession::new(cols, rows)?;
        let id = format!("pty-{}", uuid::Uuid::new_v4());
        let manager = app.state::<PtyManager>();
        manager.sessions.lock().unwrap().insert(id.clone(), session);
        Ok(SpawnResult { pty_id: id })
    }

    #[tauri::command]
    pub fn terminal_write(pty_id: String, data: String, app: tauri::AppHandle) -> Result<(), String> {
        tracing::debug!("[PTY] 写入数据: pty_id={}, bytes={}", pty_id, data.len());
        let manager = app.state::<PtyManager>();
        let mut sessions = manager.sessions.lock().unwrap();
        let session = sessions.get_mut(&pty_id).ok_or("PTY 会话不存在")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&data).map_err(|e| format!("Base64 解码失败: {}", e))?;
        session.write(&bytes)
    }

    #[tauri::command]
    pub fn terminal_read(pty_id: String, _timeout_ms: u64, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
        tracing::debug!("[PTY] 读取数据: pty_id={}", pty_id);
        let manager = app.state::<PtyManager>();
        let mut sessions = manager.sessions.lock().unwrap();
        let session = sessions.get_mut(&pty_id).ok_or("PTY 会话不存在")?;
        let data = session.read(_timeout_ms)?;
        Ok(serde_json::json!({ "data": data }))
    }

    #[tauri::command]
    pub fn terminal_resize(pty_id: String, cols: u16, rows: u16, app: tauri::AppHandle) -> Result<(), String> {
        tracing::debug!("[PTY] 调整大小: pty_id={}, cols={}, rows={}", pty_id, cols, rows);
        let manager = app.state::<PtyManager>();
        let mut sessions = manager.sessions.lock().unwrap();
        let session = sessions.get_mut(&pty_id).ok_or("PTY 会话不存在")?;
        session.resize(cols, rows)
    }
}

/* ── Tauri Commands (File System) ──────────────────────── */

#[tauri::command]
fn read_dir(path: String) -> Result<ReadDirResponse, String> {
    tracing::debug!("[ReadDir] path={}", path);
    let entries = fs::read_dir(&path)
        .map_err(|e| format!("无法读取目录 '{}': {}", path, e))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let metadata = entry.metadata().ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            let permissions = get_permissions(&metadata);
            let mtime = metadata
                .modified().ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| {
                    chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default()
                })
                .unwrap_or_default();

            Some(FileEntry {
                name, is_dir: metadata.is_dir(),
                size: if metadata.is_dir() { 0 } else { metadata.len() },
                mtime, permissions,
            })
        })
        .collect();

    Ok(ReadDirResponse { path, entries })
}

#[tauri::command]
fn stat_file(path: String) -> Result<FileEntry, String> {
    tracing::debug!("[StatFile] path={}", path);
    let metadata = fs::metadata(&path).map_err(|e| format!("无法访问 '{}': {}", path, e))?;
    let name = std::path::Path::new(&path)
        .file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
    let permissions = get_permissions(&metadata);
    let mtime = metadata.modified().ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| {
            chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                .map(|dt| dt.to_rfc3339()).unwrap_or_default()
        })
        .unwrap_or_default();

    Ok(FileEntry { name, is_dir: metadata.is_dir(), size: if metadata.is_dir() { 0 } else { metadata.len() }, mtime, permissions })
}

#[tauri::command]
fn read_file_text(path: String) -> Result<String, String> {
    tracing::debug!("[ReadFileText] path={}", path);
    const MAX_SIZE: u64 = 1_048_576;
    let metadata = fs::metadata(&path).map_err(|e| format!("无法访问 '{}': {}", path, e))?;
    if metadata.is_dir() { return Err("这是一个目录，不能作为文本打开".to_string()); }
    if metadata.len() > MAX_SIZE { return Err(format!("文件太大 ({}KB)，预览限制 1MB", metadata.len() / 1024)); }
    fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {}", e))
}

#[tauri::command]
async fn prepare_shutdown(_app: tauri::AppHandle) -> Result<(), String> {
    tracing::info!("[Frontend] 收到关闭通知");
    // 前端会在 beforeunload 时调用此命令
    // 可以在这里执行一些快速清理操作
    Ok(())
}

/* ── App Entry ──────────────────────────────────────────── */

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(pty::PtyManager::new())
        .manage(connection::ConnectionManager::new())
        .manage(proxy::ProxySessionManager::new())
        .manage(Arc::new(terminal::TerminalStreamManager::new()))
        .setup(|app| {
            // 初始化日志系统（必须在所有其他操作之前）
            init_logging(app);
            // 启动横幅：版本 + 构建模式 + 系统信息，用于从日志快速识别客户端版本
            tracing::info!(
                "[App] Quirel 客户端启动: version={} build={} os={}",
                env!("CARGO_PKG_VERSION"),
                if cfg!(debug_assertions) { "debug" } else { "release" },
                std::env::consts::OS,
            );

            // 注册关闭钩子
            setup_shutdown_hook(&app.handle());

            // 创建 TransferManager（需要 AppHandle）
            let transfer_manager = Arc::new(transfer::TransferManager::new(app.handle().clone()));
            
            // 启动定期清理任务（使用 tauri::async_runtime::spawn 在正确的异步上下文中启动）
            let manager_clone = transfer_manager.clone();
            tauri::async_runtime::spawn(async move {
                manager_clone.start_cleanup_task().await;
            });
            
            app.manage(transfer_manager);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            read_dir, stat_file, read_file_text,
            prepare_shutdown,
            ui_logging::log_write,
            ui_logging::get_app_version,
            pty::spawn_terminal,
            pty::terminal_write,
            pty::terminal_read,
            pty::terminal_resize,
            connection::remote_connect,
            connection::remote_disconnect,
            connection::remote_ping,
            connection::remote_send,
            connection::remote_read_dir,
            connection::remote_get_current_user,
            connection::remote_get_mounts,
            connection::remote_get_path_suggestions,
            connection::remote_get_metrics,
            connection::remote_read_file,
            connection::remote_file_info, // 文件格式探测（格式路由依据）
            connection::remote_read_file_binary, // 二进制读取（图片/PDF/hex 查看器）
            connection::remote_execute_command, // 白名单命令执行（解压）
            connection::remote_open_locally, // 下载到本地用系统应用打开（HTML）
            connection::remote_write_file,
            connection::remote_apply_diff, // 差异同步保存
            connection::remote_delete,
            connection::remote_mkdir,
            connection::remote_rename,
            connection::remote_copy,
            connection::remote_move,
            connection::remote_chmod, // 修改文件权限（属性对话框）
            connection::remote_chown, // 修改文件所有者（属性对话框）
            connection::get_stats,
            connection::subscribe,
            connection::unsubscribe,
            proxy::proxy_start_session,
            proxy::proxy_stop_session,
            proxy::proxy_session_status,
            proxy::proxy_list_browsers,
            terminal::remote_spawn_terminal,
            terminal::remote_terminal_write,
            terminal::remote_terminal_close,
            terminal::remote_terminal_resize,
            transfer::transfer_file,
            transfer::local_path_is_file,
            transfer::pause_transfer,
            transfer::resume_transfer,
            transfer::retry_transfer,
            transfer::cancel_transfer,
            transfer::get_active_transfer_count,
            transfer::check_file_exists,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // App 退出时清理全部代理会话（SOCKS5 监听 + 浏览器子进程），
            // 否则浏览器作为独立系统进程会残留并指向已死的代理端口
            if let tauri::RunEvent::Exit = event {
                proxy::cleanup_all_sessions(app);
            }
        });
}
