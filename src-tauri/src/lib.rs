use serde::Serialize;
use std::fs;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tauri::{Emitter, Manager};  // 导入 Emitter/Manager trait
use tokio::sync::Mutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

pub mod errors;
mod connection;
mod credentials;
mod fullscreen;
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

/// 壁纸缓存：把用户选中的图片复制为应用数据目录下的单一缓存文件
/// （wallpaper/wallpaper.cache.<ext>），返回缓存绝对路径。
/// 前端用 convertFileSrc() 经 asset 协议直接加载——避免 base64 dataURL
/// 整串过 IPC + CSS 二次解码的慢路径（见 wallpaperStore.ts）。
#[tauri::command]
fn wallpaper_cache_write(source_path: String, app: tauri::AppHandle) -> Result<String, String> {
    use std::path::Path;

    const ALLOWED_EXT: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
    const MAX_WALLPAPER_BYTES: u64 = 32 * 1_048_576;

    let src = Path::new(&source_path);
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !ALLOWED_EXT.contains(&ext.as_str()) {
        return Err(format!("不支持的图片格式 '.{}'（允许: png/jpg/jpeg/webp）", ext));
    }

    let metadata = fs::metadata(src).map_err(|e| format!("无法访问 '{}': {}", source_path, e))?;
    if metadata.len() > MAX_WALLPAPER_BYTES {
        return Err(format!("图片太大 ({}MB)，上限 32MB", metadata.len() / 1_048_576));
    }

    let cache_dir = app
        .path()
        .app_data_dir()
        .map(|p| p.join("wallpaper"))
        .map_err(|e| format!("无法定位应用数据目录: {}", e))?;
    fs::create_dir_all(&cache_dir).map_err(|e| format!("创建缓存目录失败: {}", e))?;

    // 清掉旧缓存（无论扩展名是否变化，保证"只有一张"）
    if let Ok(entries) = fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("wallpaper.cache.") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    let dest = cache_dir.join(format!("wallpaper.cache.{}", ext));
    fs::copy(src, &dest).map_err(|e| format!("复制壁纸失败: {}", e))?;
    let result = dest.to_string_lossy().to_string();
    tracing::info!("[Wallpaper] 缓存已更新: {}", result);
    Ok(result)
}

/// 查询当前磁盘上的壁纸缓存路径（wallpaper/wallpaper.cache.*）。
/// 供设置页显示"使用我的图片"恢复入口——切换预设后图片仍在磁盘，
/// 只是缺少回到 cached 状态的 UI 通道。
#[tauri::command]
fn wallpaper_cache_current(app: tauri::AppHandle) -> Option<String> {
    let cache_dir = app
        .path()
        .app_data_dir()
        .map(|p| p.join("wallpaper"))
        .ok()?;
    let entries = fs::read_dir(&cache_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("wallpaper.cache.") {
            return Some(entry.path().to_string_lossy().to_string());
        }
    }
    None
}

/// 读取壁纸缓存文件并返回 base64。前端转 blob URL 渲染——
/// 不依赖 asset 协议（其在部分环境下拦截器未注册，ERR_CONNECTION_REFUSED），
/// 本机 IPC 一次性传几 MB 约几十毫秒，仅在启动/导入时调用。
#[tauri::command]
fn wallpaper_read_cached(app: tauri::AppHandle) -> Result<String, String> {
    use base64::Engine as _;
    let cache_dir = app
        .path()
        .app_data_dir()
        .map(|p| p.join("wallpaper"))
        .map_err(|e| format!("无法定位应用数据目录: {}", e))?;
    let entries = fs::read_dir(&cache_dir).map_err(|e| format!("读取缓存目录失败: {}", e))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("wallpaper.cache.") {
            let data = fs::read(entry.path()).map_err(|e| format!("读取壁纸缓存失败: {}", e))?;
            return Ok(base64::engine::general_purpose::STANDARD.encode(data));
        }
    }
    Err("没有壁纸缓存".to_string())
}

#[tauri::command]
async fn prepare_shutdown(_app: tauri::AppHandle) -> Result<(), String> {
    tracing::info!("[Frontend] 收到关闭通知");
    // 前端会在 beforeunload 时调用此命令
    // 可以在这里执行一些快速清理操作
    Ok(())
}

/* ── App Entry ──────────────────────────────────────────── */

/// debug 构建专属：探测 vite dev server（devUrl localhost:1420）是否存活。
/// vite `host: false` 在 Node 17+ 可能只绑 IPv6 ::1（netstat 实证），双栈都探；
/// 任一可达即放行。不可达时弹原生 MessageBox（GUI 进程 eprintln 不可见）后退出，
/// 避免透明窗口静默失败。release 构建打包了前端资源，不经过此路径。
#[cfg(all(debug_assertions, target_os = "windows"))]
fn ensure_dev_server_or_exit() {
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;

    let addrs: [SocketAddr; 2] = [
        "127.0.0.1:1420".parse().expect("合法 IPv4 SocketAddr"),
        "[::1]:1420".parse().expect("合法 IPv6 SocketAddr"),
    ];
    let reachable = addrs.iter().any(|a| {
        TcpStream::connect_timeout(a, Duration::from_millis(800)).is_ok()
    });
    if reachable {
        return;
    }

    eprintln!("[App] dev server (localhost:1420) 未运行，debug 客户端无法加载前端页面");

    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND,
    };
    // PCWSTR 要求 NUL 结尾的 UTF-16；encode_utf16().chain([0]) 生成临时 Vec，
    // 指针在 MessageBoxW 调用期间有效（Vec 绑定为 let，非内联临时值）
    let title: Vec<u16> = "Quirel 开发模式\0".encode_utf16().collect();
    let text: Vec<u16> = concat!(
        "开发服务器未运行（http://localhost:1420）。\n\n",
        "debug 版客户端的前端页面来自 Vite 开发服务器，单独运行 exe 只会得到透明空窗。\n\n",
        "请在项目根目录执行：\nnpm run tauri dev\0",
    )
    .encode_utf16()
    .collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONWARNING | MB_SETFOREGROUND,
        );
    }
    std::process::exit(1);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// 应用标识（数据目录名）：early_maintenance 在无 AppHandle 阶段手工定位目录
const APP_IDENTIFIER: &str = "com.quirel.app";

fn data_dir_roaming() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        std::env::var("APPDATA").ok().map(|d| std::path::Path::new(&d).join(APP_IDENTIFIER))
    }
    #[cfg(not(windows))]
    {
        std::env::var("XDG_DATA_HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var("HOME").ok().map(|h| std::path::Path::new(&h).join(".local/share")))
            .map(|base| base.join(APP_IDENTIFIER))
    }
}

fn data_dir_local() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        std::env::var("LOCALAPPDATA").ok().map(|d| std::path::Path::new(&d).join(APP_IDENTIFIER))
    }
    #[cfg(not(windows))]
    {
        data_dir_roaming()
    }
}

fn wipe_flag_path() -> std::path::PathBuf {
    std::env::temp_dir().join("quirel-pending-wipe")
}

/// 启动早期维护（webview 创建前执行，文件无锁）：
/// 1) 删除数据目录中 WebView 升级残留的 *.bak 死数据（实测曾滞留 547MB）
/// 2) 检测 pending-wipe 标记 → 等待旧进程退出 → 清空全部本地数据目录
fn early_maintenance() {
    // 1) .bak 残留清理
    if let Some(local) = data_dir_local() {
        if let Ok(entries) = fs::read_dir(&local) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".bak") {
                    let _ = fs::remove_dir_all(e.path());
                    tracing::info!("[Maintenance] 已清理 WebView 残留: {}", name);
                }
            }
        }
    }

    // 2) pending-wipe（用户在应用内触发的完全清除，延迟到重启后执行——
    //    运行中的 webview/浏览器数据目录被自身锁定，进程外删除才干净）
    let flag = wipe_flag_path();
    if flag.exists() {
        tracing::info!("[Wipe] 检测到完全清除标记，等待旧进程释放文件锁…");
        let targets: Vec<std::path::PathBuf> = [
            data_dir_roaming(),
            data_dir_local(),
            Some(std::env::temp_dir().join("quirel-view")),
            Some(std::env::temp_dir().join("quirel-test-browser-settings")),
        ]
        .into_iter()
        .flatten()
        .collect();

        for attempt in 1..=6 {
            let mut all_ok = true;
            for t in &targets {
                if t.exists() && fs::remove_dir_all(t).is_err() {
                    all_ok = false;
                }
            }
            if all_ok {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(700));
            tracing::info!("[Wipe] 文件仍被占用，重试 {}…", attempt);
        }
        let _ = fs::remove_file(&flag);
        tracing::info!("[Wipe] 本地数据已完全清除（凭据库由前端先行删除）");
    }
}

/// 前端 boot 完成（hydration 门控 + 首帧渲染）后显示主窗口。
/// 与创建时的 visible(false) 配对，消除启动期的白屏/默认壁纸闪现。
#[tauri::command]
fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        w.show().map_err(|e| format!("显示窗口失败: {}", e))?;
    }
    Ok(())
}

/// 应用内「清除所有本地数据」：写标记 → 拉起新进程（其启动时执行清除）→ 退出当前进程。
/// 注意系统凭据库（Credential Manager）不随数据目录删除——前端须先按 serverId
/// 逐个调用 credential_delete_all。
#[tauri::command]
fn wipe_all_data(app: tauri::AppHandle) -> Result<(), String> {
    use std::io::Write;
    let flag = wipe_flag_path();
    let mut f = fs::File::create(&flag).map_err(|e| format!("无法创建清除标记: {}", e))?;
    f.write_all(b"pending").map_err(|e| format!("无法写入清除标记: {}", e))?;

    // 拉起新实例执行清除，随后退出自身
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .spawn()
        .map_err(|e| format!("无法重启应用: {}", e))?;
    app.exit(0);
    Ok(())
}

pub fn run() {
    // 启动早期维护（任何窗口/webview 创建前，文件未被锁定）：
    // 1) WebView 升级残留的 *.bak 死数据自动清理
    // 2) pending-wipe：上一次「清除所有本地数据」的延迟执行（见 wipe_all_data）
    early_maintenance();

    // debug 启动守卫：debug 版前端页面来自 devUrl（vite localhost:1420），
    // dev 会话结束后单独运行 exe 时页面加载失败——transparent 窗口完全透明、
    // 零报错，表象即「窗口打不开」（2026-10-23 复发实证）。创建任何窗口前
    // 先探测 dev server，不可达则弹原生对话框明示原因并退出，杜绝透明空窗。
    #[cfg(all(debug_assertions, target_os = "windows"))]
    ensure_dev_server_or_exit();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(pty::PtyManager::new())
        .manage(fullscreen::FullscreenState::default())
        .manage(connection::ConnectionManager::new())
        .manage(proxy::ProxySessionManager::new())
        .manage(Arc::new(terminal::TerminalStreamManager::new()))
        .on_window_event(|window, event| {
            // Tier1 最小化闸门：透明窗口破坏 WebView2 遮挡检测，最小化后页面
            // 仍被判定"可见"而持续渲染（终端 GPU 31% 归因结论之一）。在
            // Resized 事件时探测真实最小化状态，仅状态翻转时通知前端，前端
            // 据此缓冲终端输出/暂停动画/关闭毛玻璃滤镜。
            if matches!(event, tauri::WindowEvent::Resized(_)) {
                // 进程级去重：Resized 在拖拽尺寸时高频触发，只在最小化状态
                // 翻转的边界各发一次（恢复时同样经 Resized 事件探测到）
                static LAST_MINIMIZED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                use std::sync::atomic::Ordering;
                let minimized = window.is_minimized().unwrap_or(false);
                if LAST_MINIMIZED.swap(minimized, Ordering::Relaxed) != minimized {
                    let _ = window.emit("app-minimize-changed", minimized);
                }
            }
            // 全屏物理锁死：任何来源的窗口移动/变形（系统 NC 通道绕过前端
            // 与命令层）在事件回调里立即拉回锁定 rect。用 FullscreenState
            // 的 locked 基准（进入全屏末尾保存），避免进入过程的中间态被误拦。
            use tauri::Manager as _;
            let fs_state = window.app_handle().state::<fullscreen::FullscreenState>();
            let locked = *fs_state.locked.lock().unwrap();
            if let Some((lx, ly, lw, lh)) = locked {
                match event {
                    tauri::WindowEvent::Moved(pos) => {
                        if pos.x != lx || pos.y != ly {
                            let _ = window.set_position(tauri::PhysicalPosition::new(lx, ly));
                            tracing::warn!(
                                "[Fullscreen] 拦截全屏移动: ({},{}) → ({},{})",
                                pos.x, pos.y, lx, ly
                            );
                        }
                    }
                    tauri::WindowEvent::Resized(size) => {
                        if size.width != lw || size.height != lh {
                            let _ = window.set_size(tauri::PhysicalSize::new(lw, lh));
                            tracing::warn!(
                                "[Fullscreen] 拦截全屏变形: {}x{} → {}x{}",
                                size.width, size.height, lw, lh
                            );
                        }
                    }
                    _ => {}
                }
            }
        })
        .setup(|app| {
            // 主窗口改由代码创建（tauri.conf.json 的 windows 已置空）：
            // - visible(false)：页面/首帧就绪前不显示，消除启动闪现（A3）
            // - no_redirection_bitmap（tauri 2.12 新 API）：绕开透明窗口的
            //   重定向位图合成路径，根治 resize 白闪
            #[cfg(target_os = "windows")]
            let builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::default(),
            )
            .title("Quirel")
            .inner_size(1024.0, 768.0)
            .min_inner_size(800.0, 600.0)
            .resizable(true)
            .fullscreen(false)
            // 实验 D：有边框 + 非透明（标准系统窗口合成路径，DWM 直接管理），
            // 验证「透明/无边框是 resize 透明闪的根源」。确认后实验 E：
            // decorations(false)+transparent(false)——主窗口是全屏桌面（壁纸
            // 铺满 webview），不透明理论上零视觉损失。
            .decorations(true)
            .transparent(false)
            .visible(false);

            #[cfg(not(target_os = "windows"))]
            let builder = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::default(),
            )
            .title("Quirel")
            .inner_size(1024.0, 768.0)
            .min_inner_size(800.0, 600.0)
            .resizable(true)
            .fullscreen(false)
            .decorations(false)
            .transparent(true)
            .visible(false);

            let main_window = builder.build().map_err(|e| {
                tracing::error!("[Window] 主窗口创建失败: {}", e);
                e
            })?;

            // 实验 F+G：resize 防闪三件套（Win32 教科书组合）。
            // 1) 主窗口 subclass 拦 WM_ERASEBKGND（不擦背景，DWM 拉伸旧内容）
            // 2) 窗口类背景刷置 NULL（系统层不再有刷子可擦）
            // 3) 对全部子窗口（WebView2 宿主链）装同样拦截——残余黑闪来自
            //    WebView2 子窗口自己的擦除
            #[cfg(target_os = "windows")]
            {
                use windows::core::BOOL;
                use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
                use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
                use windows::Win32::UI::WindowsAndMessaging::{
                    EnumChildWindows, SetClassLongPtrW, GCLP_HBRBACKGROUND,
                };

                const SUBCLASS_ID: usize = 0x5152_4c;

                unsafe extern "system" fn no_erase_proc(
                    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
                    _id: usize, _data: usize,
                ) -> LRESULT {
                    const WM_ERASEBKGND: u32 = 0x0014;
                    if msg == WM_ERASEBKGND {
                        return LRESULT(1); // 声称已擦（实际不擦）
                    }
                    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
                }

                unsafe extern "system" fn enum_child_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
                    unsafe {
                        SetWindowSubclass(hwnd, Some(no_erase_proc), SUBCLASS_ID, 0);
                    }
                    BOOL(1) // 继续枚举
                }

                let hwnd = main_window.hwnd().map_err(|e| format!("获取 HWND 失败: {}", e))?;
                unsafe {
                    // 1) 主窗口拦截
                    if !SetWindowSubclass(hwnd, Some(no_erase_proc), SUBCLASS_ID, 0).as_bool() {
                        tracing::error!("[Window] 主窗口 subclass 安装失败");
                    }
                    // 2) 类背景刷置 NULL
                    SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, 0);
                    // 3) 子窗口链（WebView2 宿主）全量拦截
                    let _ = EnumChildWindows(Some(hwnd), Some(enum_child_proc), LPARAM(0));
                }
                tracing::info!("[Window] 防闪三件套已安装（主窗口+类刷+子窗口链）");
            }

            // 初始化日志系统（必须在所有其他操作之前）
            init_logging(app);
            // 启动横幅：版本 + 构建模式 + git 构建信息 + 系统信息，用于从日志快速识别客户端版本
            tracing::info!(
                "[App] Quirel 客户端启动: version={} build={} git={} os={}",
                env!("CARGO_PKG_VERSION"),
                if cfg!(debug_assertions) { "debug" } else { "release" },
                env!("QUIREL_BUILD_INFO"),
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
            wallpaper_cache_write,
            wallpaper_cache_current,
            wallpaper_read_cached,
            credentials::credential_set,
            credentials::credential_get,
            credentials::credential_delete,
            credentials::credential_delete_all,
            credentials::credential_probe,
            wipe_all_data,
            show_main_window,
            prepare_shutdown,
            fullscreen::toggle_fullscreen,
            fullscreen::is_fullscreen,
            fullscreen::start_os_drag,
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
            connection::get_processes,          // 进程列表查询（SystemMonitor 进程页签）
            connection::get_agent_capabilities, // Agent 能力查询（前端门控新协议命令）
            connection::get_transfer_limit,     // 查询上传大小限制（设置页）
            connection::set_transfer_limit,     // 修改上传大小限制（仅 root）
            connection::subscribe,
            connection::unsubscribe,
            proxy::proxy_start_session,
            proxy::browser_open,
            proxy::proxy_stop_session,
            proxy::proxy_session_status,
            proxy::browser_window::browser_close_window,
            proxy::browser_window::browser_navigate,
            proxy::browser_window::browser_back,
            proxy::browser_window::browser_forward,
            proxy::browser_window::browser_reload,
            proxy::browser_window::browser_create_tab,
            proxy::browser_window::browser_close_tab,
            proxy::browser_window::browser_focus_tab,
            proxy::browser_window::browser_reorder_tab,
            proxy::browser_window::browser_set_bounds,
            proxy::browser_window::browser_set_visible,
            proxy::browser_window::browser_get_settings,
            proxy::browser_window::browser_set_settings,
            proxy::suggest::browser_suggest, // 地址栏联想词（经 SOCKS5 隧道取搜索引擎补全）
            proxy::browser::proxy_list_browsers, // 枚举本机浏览器（设置页"系统浏览器"下拉）
            proxy::browser::browser_open_in_system, // 系统浏览器打开当前页（挂同一隧道）
            terminal::remote_spawn_terminal,
            terminal::remote_terminal_write,
            terminal::remote_terminal_close,
            terminal::remote_terminal_resize,
            transfer::transfer_file,
            transfer::local_path_is_file,
            transfer::delete_transfer_temp, // 回收下载残留的临时文件（用户移除任务记录时）
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
