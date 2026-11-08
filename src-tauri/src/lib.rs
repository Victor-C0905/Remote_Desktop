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
mod net_frame;
mod proxy;
mod store_maintenance;
mod terminal;
mod transfer;
mod ui_logging;

/* ── 展示版本号 ───────────────────────────────────────────── */

/// 展示用版本号：debug 构建（cargo tauri dev）自动追加 -dev 后缀，与正式构建一眼区分。
/// 仅展示层注入，Cargo.toml 版本与产物命名不变（AGENTS 第 5 节：不为区分构建改版本文件）
#[cfg(debug_assertions)]
pub const APP_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "-dev");
#[cfg(not(debug_assertions))]
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

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


/* ── PTY Management (Unix) ──────────────────────────────── */


/* ── Tauri Commands (File System) ──────────────────────── */

// ── 本地文件命令组（审计 低-9 边界说明）─────────────────────
// read_dir / stat_file / read_file_text / wallpaper_cache_write 接受任意
// 绝对路径（文件管理器本地面板所需）。信任模型：调用方是本进程 renderer，
// CSP（default-src 'self'）限制脚本来源——renderer 被攻破时这些命令是
// 高价值目标，属已知接受风险；若未来 CSP 放宽需先收紧此处。

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

/// 清空 wallpaper.cache.* 旧缓存并返回新缓存文件目标路径（保证"只有一张"）
fn wallpaper_cache_dest(cache_dir: &std::path::Path, ext: &str) -> std::path::PathBuf {
    if let Ok(entries) = fs::read_dir(cache_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("wallpaper.cache.") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    cache_dir.join(format!("wallpaper.cache.{}", ext))
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

    let dest = wallpaper_cache_dest(&cache_dir, &ext);
    fs::copy(src, &dest).map_err(|e| format!("复制壁纸失败: {}", e))?;
    let result = dest.to_string_lossy().to_string();
    tracing::info!("[Wallpaper] 缓存已更新: {}", result);
    Ok(result)
}

/// 旧方案 dataURL 壁纸迁移落盘：前端 zustand persist 曾把导入壁纸整串 base64
/// 存进 settings.bin（单图可达 5MB，任意设置变更触发 persist 都全量重写）。
/// 本命令解码 dataURL 写为缓存文件并返回路径，前端随即改存 cachedPath——
/// 与 wallpaper_cache_write 同语义（清旧缓存、单一文件），来源是内存字节而非源文件。
#[tauri::command]
fn wallpaper_cache_write_data(data_url: String, app: tauri::AppHandle) -> Result<String, String> {
    // data:image/<ext>;base64,<payload>，仅接受与导入一致的位图格式
    let rest = data_url
        .strip_prefix("data:image/")
        .ok_or_else(|| "不是 image dataURL".to_string())?;
    let (ext, payload) = rest
        .split_once(";base64,")
        .ok_or_else(|| "dataURL 缺少 base64 段".to_string())?;
    let ext = match ext {
        "png" => "png",
        "jpg" | "jpeg" => "jpeg",
        "webp" => "webp",
        other => return Err(format!("不支持的图片格式 '.{}'（允许: png/jpg/webp）", other)),
    };

    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| format!("base64 解码失败: {}", e))?;
    const MAX_WALLPAPER_BYTES: usize = 32 * 1_048_576;
    if bytes.len() > MAX_WALLPAPER_BYTES {
        return Err(format!("图片太大 ({}MB)，上限 32MB", bytes.len() / 1_048_576));
    }

    let cache_dir = app
        .path()
        .app_data_dir()
        .map(|p| p.join("wallpaper"))
        .map_err(|e| format!("无法定位应用数据目录: {}", e))?;
    fs::create_dir_all(&cache_dir).map_err(|e| format!("创建缓存目录失败: {}", e))?;

    let dest = wallpaper_cache_dest(&cache_dir, ext);
    fs::write(&dest, &bytes).map_err(|e| format!("写入壁纸缓存失败: {}", e))?;
    tracing::info!("[Wallpaper] dataURL 已迁移落盘: {}", dest.to_string_lossy());
    Ok(dest.to_string_lossy().to_string())
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
async fn prepare_shutdown(app: tauri::AppHandle) -> Result<(), String> {
    tracing::info!("[Frontend] 收到关闭通知");
    // 刷新/关闭的前置隐藏（会话原子性的过渡态补全）：beforeunload 到新页面
    // 收养之间存在空窗——tab webview（独立 HWND，Rust 持有）若保持可见，
    // 会以「脱离的悬浮层」形态残留（实测截图）。此处对全部会话下发隐藏，
    // 新页面收养后由可见性链路（SetVisible 全量版）重新接管。
    {
        use tauri::Manager as _;
        let sessions = app.state::<proxy::ProxySessionManager>();
        sessions.hide_all_webviews();
    }
    Ok(())
}

/// 进程纪元（pid + 静态捕获的启动时刻）：前端 sessionStorage 以此判别
/// 进程是否重启——WebView2 的 sessionStorage 会在进程重启后存活（Chromium
/// 崩溃恢复特性，同 user-data + 同源即保留），纪元不一致即清空上一世代的
/// 会话镜像（窗口快照/连接镜像/挂载代际），保证「关闭项目 = 干净桌面」。
fn process_epoch() -> String {
    static EPOCH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    EPOCH.get_or_init(|| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("{}-{ts}", std::process::id())
    })
    .clone()
}

#[tauri::command]
fn app_process_epoch() -> String {
    process_epoch()
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
pub(crate) const PROD_IDENTIFIER: &str = "com.quirel.app";
/// debug 构建强制使用的独立 identifier（数据目录/keyring/临时目录与 release 隔离）
pub(crate) const DEV_IDENTIFIER: &str = "com.quirel.app.dev";

/// profile 临时目录/标记后缀：生产为空串，dev 形如 "-dev"（identifier 末段）
pub(crate) fn identifier_suffix(identifier: &str) -> String {
    if identifier == PROD_IDENTIFIER {
        String::new()
    } else {
        identifier
            .rsplit('.')
            .next()
            .map(|s| format!("-{s}"))
            .unwrap_or_else(|| format!("-{identifier}"))
    }
}

/// 按 profile 隔离的临时目录（dev 与 release 不互相清理/复用）
pub(crate) fn profile_temp_path(base: &str, identifier: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{base}{}", identifier_suffix(identifier)))
}

fn data_dir_roaming(identifier: &str) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        std::env::var("APPDATA").ok().map(|d| std::path::Path::new(&d).join(identifier))
    }
    #[cfg(not(windows))]
    {
        std::env::var("XDG_DATA_HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var("HOME").ok().map(|h| std::path::Path::new(&h).join(".local/share")))
            .map(|base| base.join(identifier))
    }
}

fn data_dir_local(identifier: &str) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        std::env::var("LOCALAPPDATA").ok().map(|d| std::path::Path::new(&d).join(identifier))
    }
    #[cfg(not(windows))]
    {
        data_dir_roaming(identifier)
    }
}

fn wipe_flag_path(identifier: &str) -> std::path::PathBuf {
    profile_temp_path("quirel-pending-wipe", identifier)
}

/// 启动早期维护（webview 创建前执行，文件无锁）：
/// 1) 删除数据目录中 WebView 升级残留的 *.bak 死数据（实测曾滞留 547MB）
/// 2) 检测 pending-wipe 标记 → 等待旧进程退出 → 清空全部本地数据目录
/// `identifier` 来自 tauri 配置：dev 构建（com.quirel.app.dev）与 release 完全隔离
fn early_maintenance(identifier: &str) {
    // 1) .bak 残留清理
    if let Some(local) = data_dir_local(identifier) {
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
    let flag = wipe_flag_path(identifier);
    if flag.exists() {
        tracing::info!("[Wipe] 检测到完全清除标记，等待旧进程释放文件锁…");
        let targets: Vec<std::path::PathBuf> = [
            data_dir_roaming(identifier),
            data_dir_local(identifier),
            Some(profile_temp_path("quirel-view", identifier)),
            Some(profile_temp_path("quirel-test-browser-settings", identifier)),
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

// ── 主窗口窗口控制命令（TopBar 窗口控制菜单专用）──────────────
// 无边框模式下系统标题栏不再存在，最小化/最大化/关闭由 TopBar 的
// ToolbarMenu 菜单发起，经这三个命令作用于主窗口。图标态随
// app-maximize-changed 事件同步（Resized 段探测，与最小化闸门同路径）。

// 实验 G：主窗口暴露区填充色（COLORREF 0x00BBGGRR）。最大化/缩放时窗口
// 背衬面新增区域在 WebView2 新帧栅格化完成前是坏帧——「不擦+NULL 刷」
// 方案（实验 F+G 三件套）下显示为纯黑，高刷屏上被 DWM 采成 2~5 个连续
// 黑帧（60Hz 常被动画掩蔽，165Hz 必然可见；实测时长 13~55ms 与刷新率
// 无关）。改为以当前壁纸平均色填充：坏帧从「闪黑」变成与壁纸几乎不可
// 分的色差。初值 = 默认壁纸 dusk 平均色，前端经 set_window_background_color
// 随壁纸同步。no_erase_proc 主窗口分支消费。
static WINDOW_BG_COLORREF: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0x8A57_A3); // dusk rgb(163,87,138)

/// 同步主窗口背景色为当前壁纸平均色（前端壁纸变更/加载时调用）。
/// 两层同时生效：主窗口 WM_ERASEBKGND 暴露区填充（WINDOW_BG_COLORREF）+
/// WebView2 DefaultBackgroundColor（子窗口已覆盖但新帧未栅格化的区域）。
/// 同时联动浏览器页面 webview（webview2-com 直建，独立于主 UI webview）：
/// 创建时经 create_controller 取当前色，壁纸运行时变更经此命令对存活
/// webview 补发——否则其默认白底在最大化/缩放时闪白（坏帧同源）。
#[tauri::command]
fn set_window_background_color(
    app: tauri::AppHandle,
    r: u8,
    g: u8,
    b: u8,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    WINDOW_BG_COLORREF.store(
        r as u32 | (g as u32) << 8 | (b as u32) << 16,
        Ordering::Relaxed,
    );
    if let Some(w) = app.get_webview_window("main") {
        w.set_background_color(Some(tauri::utils::config::Color(r, g, b, 255)))
            .map_err(|e| format!("设置窗口背景色失败: {}", e))?;
    }
    crate::proxy::sync_browser_background_colors(&app);
    Ok(())
}

/// 最小化主窗口
#[tauri::command]
fn window_minimize(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        w.minimize().map_err(|e| format!("最小化失败: {}", e))?;
    }
    Ok(())
}

/// 最大化/还原切换（按当前状态取反，供菜单单项使用）
#[tauri::command]
fn window_toggle_maximize(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_maximized().unwrap_or(false) {
            w.unmaximize().map_err(|e| format!("还原失败: {}", e))?;
        } else {
            w.maximize().map_err(|e| format!("最大化失败: {}", e))?;
        }
    }
    Ok(())
}

/// 关闭主窗口（最终退出经 RunEvent::ExitRequested 的 cleanup_all_sessions）
#[tauri::command]
fn window_close(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        w.close().map_err(|e| format!("关闭失败: {}", e))?;
    }
    Ok(())
}

/// 应用内「清除所有本地数据」：写标记 → 拉起新进程（其启动时执行清除）→ 退出当前进程。
/// 注意系统凭据库（Credential Manager）不随数据目录删除——前端须先按 serverId
/// 逐个调用 credential_delete_all。
#[tauri::command]
fn wipe_all_data(app: tauri::AppHandle) -> Result<(), String> {
    use std::io::Write;
    let flag = wipe_flag_path(&app.config().identifier.clone());
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
    // 先取配置：debug 构建在下方运行时改为 com.quirel.app.dev，
    // 保证数据目录/keyring/临时目录与 release 完全隔离
    let mut context = tauri::generate_context!();

    // debug 构建强制切换独立 identifier：无论用 `npm run tauri dev`、IDE 启动
    // 还是直接跑 exe，都不会与 release 共用 servers.bin/keyring/临时目录。
    #[cfg(debug_assertions)]
    {
        let config = context.config_mut();
        if config.identifier == PROD_IDENTIFIER {
            config.identifier = DEV_IDENTIFIER.to_string();
        }
    }

    let identifier = context.config().identifier.clone();

    // 启动早期维护（任何窗口/webview 创建前，文件未被锁定）：
    // 1) WebView 升级残留的 *.bak 死数据自动清理
    // 2) pending-wipe：上一次「清除所有本地数据」的延迟执行（见 wipe_all_data）
    early_maintenance(&identifier);

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
                // 最大化状态翻转同步：TopBar 窗口控制菜单的「最大化/还原」
                // 图标与文案依赖此事件（与最小化闸门同路径、同去重模式）
                static LAST_MAXIMIZED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                let maximized = window.is_maximized().unwrap_or(false);
                if LAST_MAXIMIZED.swap(maximized, Ordering::Relaxed) != maximized {
                    let _ = window.emit("app-maximize-changed", maximized);
                }
                // 注意：浏览器 webview 的窗口 resize 跟随不走此事件——
                // 前端 ResizeObserver 直测上报是唯一真源（曾试过 runtime 端
                // 「客户区−边距」外推直通，因浏览器卡片是有 CSS 过渡动画的
                // 自有几何浮动窗口、边距不恒定而回退，见 docs/PROGRESS.md）
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
            // 启动自愈（审计 低-13）：还原上次崩溃可能残留的隐藏任务栏
            fullscreen::restore_taskbars_on_startup();
            // webview 加载前先备份全部 store：前端 persist 的首次写回不会覆盖原始数据
            store_maintenance::backup_all_on_startup(app.handle());

            // 主窗口改由代码创建（tauri.conf.json 的 windows 已置空）：
            // - visible(false)：页面/首帧就绪前不显示，消除启动闪现（A3）
            // - no_redirection_bitmap（tauri 2.12 新 API）：绕开透明窗口的
            //   重定向位图合成路径，根治 resize 白闪
            // 浏览器附加参数（预计算为 String：CDP 端口运行期生成，
            // 不能内联在 if-else 里借用临时值，审计 低-7）
            #[cfg(target_os = "windows")]
            let browser_args: String = if cfg!(debug_assertions) {
                format!(
                    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IntensiveWakeUpThrottling --remote-debugging-port={} --disable-background-timer-throttling --disable-backgrounding-occluded-windows",
                    9222 + (std::process::id() % 1000) as u32
                )
            } else {
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IntensiveWakeUpThrottling --disable-background-timer-throttling --disable-backgrounding-occluded-windows".to_string()
            };

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
            // 实验 E（定稿）：无边框 + 非透明——TopBar 即标题栏（自绘窗口控制
            // 菜单），壁纸铺满 webview 一体到底；不透明窗口走标准 DWM 合成
            // 路径，无 resize 透明闪。resize 命中区由下方 NCHITTEST 子类提供，
            // 窗口圆角由 DWMWA_WINDOW_CORNER_PREFERENCE 提供（含 DWM 阴影）。
            .decorations(false)
            .transparent(false)
            .visible(false)
            // additional_browser_args 是替换语义 → 必须显式补回 wry 默认的
            // disable-features（与 webview_host.rs 同规则）；debug 构建追加
            // CDP 远程调试口，供诊断「DOM 与可见像素脱节」类渲染问题。
            //
            // ⚠️ 必须禁用后台/遮挡定时器节流：§5.1 的 1s 前端心跳
            // （browser_heartbeat）活在壳页面里，而浏览器 tab webview 是盖在
            // 壳页面上的独立 HWND（且独立 user-data-dir = 独立 Chromium 进程），
            // Chromium 把壳页面判为被遮挡 → IntensiveWakeUpThrottling 生效，
            // setInterval 强节流到 60.000s/次（相位锁定）→ 心跳恒超 3s 阈值 →
            // 执行器隐藏 webview → 遮挡解除 → 心跳恢复 → re-assert 可见 →
            // 再被遮挡……自持振荡（webview 每分钟可见 3s、隐藏 57s）。
            // 2026-11-01 探针实锤：心跳 seq0-3 后蒸发，恢复间隔恒 60.0s。
            // 注意：IWT 不受 --disable-background-timer-throttling 管辖，必须
            // 以 feature 名单独禁用；wry 的 background_throttling 策略仅在
            // macOS (wkwebview) 实现，WebView2 后端无消费点。仅壳页面需要；
            // tab webview（远程页）保持节流以省资源。
            .additional_browser_args(&browser_args);

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
            // 非 Windows 分支（跨平台预留，当前产品仅面向 Windows/NSIS）。
            // 注意：与 AGENTS.md §6「主窗口必须 transparent(false)」的字面
            // 要求不一致——非 Windows 无防闪三件套体系，若未来启用非
            // Windows 构建，须先按 docs/window-flash-rules.md 重建（审计 低-6）
            .transparent(true)
            .visible(false);

            let main_window = builder.build().map_err(|e| {
                tracing::error!("[Window] 主窗口创建失败: {}", e);
                e
            })?;

            // 关闭浏览器类行为入口（F5/Ctrl+R 加速键 + 默认右键菜单）：壳页面
            // 刷新入口收编为 RootErrorBoundary 的「重置界面」受控重载，前端
            // 另有键盘拦截兜底（main.tsx，仅 release 构建）；应用自身的自定义
            // 右键菜单是 DOM 渲染，不受此设置影响。加速键成员在 Settings3
            // 子接口；失败仅日志，不阻塞启动。
            #[cfg(target_os = "windows")]
            {
                use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
                use windows::core::Interface as _;
                let _ = main_window.with_webview(|wv| {
                    let controller = wv.controller();
                    unsafe {
                        if let Err(e) = controller
                            .CoreWebView2()
                            .and_then(|core| core.Settings())
                            .and_then(|s| {
                                s.SetAreDefaultContextMenusEnabled(false)?;
                                s.cast::<ICoreWebView2Settings3>()
                                    .and_then(|s3| s3.SetAreBrowserAcceleratorKeysEnabled(false))
                            })
                        {
                            tracing::warn!("[Window] 主窗口关闭浏览器加速键/默认右键菜单失败: err={e}");
                        }
                    }
                });
            }

            // 实验 F+G：resize 防闪三件套（Win32 教科书组合）。
            // 1) 主窗口 subclass 拦 WM_ERASEBKGND：实验 F 原样「声称已擦不擦」
            //    时，最大化放大的背衬面新增区域是坏帧且无刷子可擦 → 纯黑
            //    （探针实测高刷屏 2~5 帧，见 debug-maximize-black-flash.md）。
            //    改为以壁纸平均色填充暴露区（data=1 区分），坏帧不再是黑。
            // 2) 窗口类背景刷置 NULL（系统层不再有刷子可擦）
            // 3) 对全部子窗口（WebView2 宿主链）装同样拦截——但维持「不擦」：
            //    WebView2 子窗口背景由 Chromium 自管，填充会在内容上叠色
            #[cfg(target_os = "windows")]
            {
                use windows::core::BOOL;
                use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
                use windows::Win32::Graphics::Gdi::{
                    CreateSolidBrush, DeleteObject, FillRect, HDC,
                };
                use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
                use windows::Win32::UI::WindowsAndMessaging::{
                    EnumChildWindows, GetClientRect, SetClassLongPtrW, GCLP_HBRBACKGROUND,
                };

                const SUBCLASS_ID: usize = 0x5152_4c;

                unsafe extern "system" fn no_erase_proc(
                    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
                    _id: usize, data: usize,
                ) -> LRESULT {
                    const WM_ERASEBKGND: u32 = 0x0014;
                    if msg == WM_ERASEBKGND {
                        // 主窗口（data=1）：壁纸平均色填充暴露区。FillRect 受
                        // DC 裁剪区约束只真正画无效像素，稳态 resize 零开销；
                        // 填充色与最终内容近似，视觉上无擦除闪。
                        if data == 1 {
                            let hdc = HDC(wp.0 as *mut _);
                            let mut rc = RECT::default();
                            if unsafe { GetClientRect(hwnd, &mut rc) }.is_ok() {
                                let brush = unsafe { CreateSolidBrush(COLORREF(
                                    WINDOW_BG_COLORREF
                                        .load(std::sync::atomic::Ordering::Relaxed),
                                )) };
                                unsafe {
                                    FillRect(hdc, &rc, brush);
                                    let _ = DeleteObject(brush.into());
                                }
                            }
                        }
                        return LRESULT(1);
                    }
                    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
                }

                unsafe extern "system" fn enum_child_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
                    unsafe {
                        // 子窗口的 subclass 安装结果无恢复手段，BOOL 显式忽略
                        let _ = SetWindowSubclass(hwnd, Some(no_erase_proc), SUBCLASS_ID, 0);
                    }
                    BOOL(1) // 继续枚举
                }

                let hwnd = main_window.hwnd().map_err(|e| format!("获取 HWND 失败: {}", e))?;
                unsafe {
                    // 1) 主窗口拦截（data=1：填充暴露区）
                    if !SetWindowSubclass(hwnd, Some(no_erase_proc), SUBCLASS_ID, 1).as_bool() {
                        tracing::error!("[Window] 主窗口 subclass 安装失败");
                    }
                    // 2) 类背景刷置 NULL
                    SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, 0);
                    // 3) 子窗口链（WebView2 宿主）全量拦截（data=0：不擦）
                    let _ = EnumChildWindows(Some(hwnd), Some(enum_child_proc), LPARAM(0));
                }
                // WebView2 层兜底：子窗口已覆盖但新帧未栅格化的区域以
                // DefaultBackgroundColor 呈现，同步为默认壁纸色（前端随壁纸更新）
                let _ = main_window.set_background_color(Some(tauri::utils::config::Color(
                    0xa3, 0x57, 0x8a, 255,
                )));
                tracing::info!("[Window] 防闪三件套已安装（主窗口填色+类刷+子窗口链拦截）");
            }

            // 实验 E 配套：无边框窗口的 resize 命中区与系统圆角。
            // 1) WM_NCHITTEST 子类：decorations(false) 后 tao 不再提供 NC 手柄，
            //    在 HTCLIENT 区把边缘 8 逻辑 px（含四角）改判为边框命中，其余
            //    放行；与 no_erase_proc 用不同 SUBCLASS_ID 共存（comctl 子类链）。
            // 2) DWMWA_WINDOW_CORNER_PREFERENCE = ROUND：Win11 系统圆角，
            //    DWM 阴影随附，无需自绘。
            #[cfg(target_os = "windows")]
            {
                use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
                use windows::Win32::Graphics::Dwm::{
                    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWM_WINDOW_CORNER_PREFERENCE,
                };
                use windows::Win32::UI::HiDpi::GetDpiForWindow;
                use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
                use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

                const SUBCLASS_ID_RESIZE: usize = 0x5152_4d;

                // Win32 边框命中码（HTLEFT..HTBOTTOMRIGHT）
                const HTLEFT: i32 = 10;
                const HTRIGHT: i32 = 11;
                const HTTOP: i32 = 12;
                const HTTOPLEFT: i32 = 13;
                const HTTOPRIGHT: i32 = 14;
                const HTBOTTOM: i32 = 15;
                const HTBOTTOMLEFT: i32 = 16;
                const HTBOTTOMRIGHT: i32 = 17;
                const HTCLIENT: i32 = 1;

                unsafe extern "system" fn resize_hit_proc(
                    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
                    _id: usize, _data: usize,
                ) -> LRESULT {
                    const WM_NCHITTEST: u32 = 0x0084;
                    if msg == WM_NCHITTEST {
                        let hit = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
                        if hit.0 == HTCLIENT as isize {
                            // lParam：屏幕物理坐标（低/高 16 位，有符号）
                            let pt = POINT {
                                x: (lp.0 as i16) as i32,
                                y: ((lp.0 >> 16) as i16) as i32,
                            };
                            let mut rc = RECT::default();
                            if unsafe { GetWindowRect(hwnd, &mut rc) }.is_ok() {
                                // 8 逻辑 px → 物理 px（随 DPI 缩放，最低保 4）
                                let dpi = unsafe { GetDpiForWindow(hwnd) };
                                let border = ((dpi as i32 * 8 / 96).max(4)) as i32;
                                let x = pt.x - rc.left;
                                let y = pt.y - rc.top;
                                let w = rc.right - rc.left;
                                let h = rc.bottom - rc.top;
                                let (l, r) = (x < border, x >= w - border);
                                let (t, b) = (y < border, y >= h - border);
                                let code = match (l, r, t, b) {
                                    (true, _, true, _) => HTTOPLEFT,
                                    (_, true, true, _) => HTTOPRIGHT,
                                    (true, _, _, true) => HTBOTTOMLEFT,
                                    (_, true, _, true) => HTBOTTOMRIGHT,
                                    (true, _, _, _) => HTLEFT,
                                    (_, true, _, _) => HTRIGHT,
                                    (_, _, true, _) => HTTOP,
                                    (_, _, _, true) => HTBOTTOM,
                                    _ => HTCLIENT,
                                };
                                if code != HTCLIENT {
                                    return LRESULT(code as isize);
                                }
                            }
                        }
                        return hit;
                    }
                    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
                }

                let hwnd = main_window.hwnd().map_err(|e| format!("获取 HWND 失败: {}", e))?;
                unsafe {
                    if !SetWindowSubclass(hwnd, Some(resize_hit_proc), SUBCLASS_ID_RESIZE, 0).as_bool() {
                        tracing::error!("[Window] resize 命中区 subclass 安装失败");
                    }
                    // Win11 系统圆角（失败仅降级为直角，不阻塞启动）
                    let pref: DWM_WINDOW_CORNER_PREFERENCE = DWM_WINDOW_CORNER_PREFERENCE(2); // DWMWCP_ROUND
                    let hr = DwmSetWindowAttribute(
                        hwnd,
                        DWMWA_WINDOW_CORNER_PREFERENCE,
                        &pref as *const _ as *const std::ffi::c_void,
                        std::mem::size_of::<i32>() as u32,
                    );
                    if hr.is_err() {
                        tracing::warn!("[Window] DWM 圆角设置失败（Win10 及以下将显示直角）");
                    }
                }
                tracing::info!("[Window] 无边框配套已安装（resize 命中区 + DWM 圆角）");
            }

            // 初始化日志系统（必须在所有其他操作之前）
            init_logging(app);
            // 启动横幅：版本 + 构建模式 + git 构建信息 + 系统信息，用于从日志快速识别客户端版本
            tracing::info!(
                "[App] Quirel 客户端启动: version={} build={} git={} os={}",
                APP_VERSION,
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
            wallpaper_cache_write_data,
            wallpaper_cache_current,
            wallpaper_read_cached,
            credentials::credential_set,
            credentials::credential_get,
            credentials::credential_delete,
            credentials::credential_delete_all,
            credentials::credential_probe,
            store_maintenance::store_backup,      // 写前备份（Store 保护层）
            store_maintenance::store_quarantine,  // 备份并移除损坏/高版本 store 后重启
            store_maintenance::app_restart,
            store_maintenance::app_exit,
            wipe_all_data,
            show_main_window,
            window_minimize,
            window_toggle_maximize,
            window_close,
            set_window_background_color,
            prepare_shutdown,
            app_process_epoch,
            fullscreen::toggle_fullscreen,
            fullscreen::is_fullscreen,
            fullscreen::start_os_drag,
            ui_logging::log_write,
            ui_logging::get_app_version,
            ui_logging::get_build_info,
            connection::remote_connect,
            connection::remote_disconnect,
            connection::remote_ping,
            connection::server_connected_ids,
            connection::remote_send,
            connection::remote_read_dir,
            connection::remote_search, // 文件管理器搜索（宿主侧受限遍历，复用 ReadDir 权限模型）
            connection::remote_get_current_user,
            connection::remote_get_mounts,
            connection::remote_get_path_suggestions,
            connection::remote_get_metrics,
            connection::remote_read_file,
            connection::remote_file_info, // 文件格式探测（格式路由依据）
            connection::remote_read_file_binary, // 二进制读取（图片/PDF/hex 查看器）
            connection::remote_archive_list, // 压缩包列表（Agent 构造固定命令）
            connection::remote_archive_extract, // 压缩包解压（Agent 构造固定命令）
            connection::remote_archive_capabilities, // 压缩能力探测（解压工具可用性）
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
            proxy::browser_window::browser_close_all_windows,
            proxy::browser_window::browser_navigate,
            proxy::browser_window::browser_back,
            proxy::browser_window::browser_forward,
            proxy::browser_window::browser_reload,
            proxy::browser_window::browser_create_tab,
            proxy::browser_window::browser_close_tab,
            proxy::browser_window::browser_focus_tab,
            proxy::browser_window::browser_reorder_tab,
            proxy::browser_window::browser_refresh_tabs,
            proxy::browser_window::browser_heartbeat,
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
        .build(context)
        .expect("error while building tauri application")
        .run(|app, event| {
            // App 退出时清理全部代理会话（SOCKS5 监听 + 浏览器子进程），
            // 否则浏览器作为独立系统进程会残留并指向已死的代理端口
            if let tauri::RunEvent::Exit = event {
                proxy::cleanup_all_sessions(app);
            }
        });
}
