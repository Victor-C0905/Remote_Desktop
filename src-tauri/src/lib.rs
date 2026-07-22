use serde::Serialize;
use std::fs;
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tauri::Manager;  // 导入 Manager trait
use tokio::sync::Mutex;

mod connection;
mod terminal;
mod transfer;

/* ── 优雅关闭机制 ───────────────────────────────────────── */

/// 全局关闭标志
pub static SHUTDOWN_FLAG: once_cell::sync::Lazy<Arc<Mutex<bool>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(false)));

/// 注册关闭钩子
pub fn setup_shutdown_hook(app: &tauri::AppHandle) {
    let app_handle = app.clone();

    // Ctrl+C 处理
    ctrlc::set_handler(move || {
        eprintln!("[Shutdown] 收到关闭信号");
        let app = app_handle.clone();
        tokio::spawn(async move {
            graceful_shutdown(app).await;
        });
    }).expect("无法设置 Ctrl+C 处理器");
}

/// 优雅关闭
async fn graceful_shutdown(app: tauri::AppHandle) {
    eprintln!("[Shutdown] 开始优雅关闭...");

    // 1. 设置关闭标志
    *SHUTDOWN_FLAG.lock().await = true;

    // 2. 取消所有传输任务
    if let Some(transfer_manager) = app.try_state::<Arc<transfer::TransferManager>>() {
        let tasks = transfer_manager.list_tasks().await;
        for task in tasks {
            if task.status == "pending" || task.status == "transferring" || task.status == "active" {
                let _ = transfer_manager.cancel_task(&task.id).await;
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
            eprintln!("[Shutdown] 断开连接: {}", server_id);
            let mut conns = connection_manager.connections.lock().unwrap();
            conns.remove(server_id);
        }
    }

    eprintln!("[Shutdown] 优雅关闭完成");
    std::process::exit(0);
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
        let _ = shell; // Use default shell (cmd.exe on Windows)
        let session = PtySession::new(cols, rows)?;
        let id = format!("pty-{}", uuid::Uuid::new_v4());

        let manager = app.state::<PtyManager>();
        manager.sessions.lock().unwrap().insert(id.clone(), session);

        Ok(SpawnResult { pty_id: id })
    }

    #[tauri::command]
    pub fn terminal_write(pty_id: String, data: String, app: tauri::AppHandle) -> Result<(), String> {
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
        let manager = app.state::<PtyManager>();
        let mut sessions = manager.sessions.lock().unwrap();
        let session = sessions.get_mut(&pty_id).ok_or("PTY 会话不存在")?;

        let data = session.read(_timeout_ms)?;
        Ok(serde_json::json!({ "data": data }))
    }

    #[tauri::command]
    pub fn terminal_resize(pty_id: String, cols: u16, rows: u16, app: tauri::AppHandle) -> Result<(), String> {
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
        let _ = shell;
        let session = PtySession::new(cols, rows)?;
        let id = format!("pty-{}", uuid::Uuid::new_v4());
        let manager = app.state::<PtyManager>();
        manager.sessions.lock().unwrap().insert(id.clone(), session);
        Ok(SpawnResult { pty_id: id })
    }

    #[tauri::command]
    pub fn terminal_write(pty_id: String, data: String, app: tauri::AppHandle) -> Result<(), String> {
        let manager = app.state::<PtyManager>();
        let mut sessions = manager.sessions.lock().unwrap();
        let session = sessions.get_mut(&pty_id).ok_or("PTY 会话不存在")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&data).map_err(|e| format!("Base64 解码失败: {}", e))?;
        session.write(&bytes)
    }

    #[tauri::command]
    pub fn terminal_read(pty_id: String, _timeout_ms: u64, app: tauri::AppHandle) -> Result<serde_json::Value, String> {
        let manager = app.state::<PtyManager>();
        let mut sessions = manager.sessions.lock().unwrap();
        let session = sessions.get_mut(&pty_id).ok_or("PTY 会话不存在")?;
        let data = session.read(_timeout_ms)?;
        Ok(serde_json::json!({ "data": data }))
    }

    #[tauri::command]
    pub fn terminal_resize(pty_id: String, cols: u16, rows: u16, app: tauri::AppHandle) -> Result<(), String> {
        let manager = app.state::<PtyManager>();
        let mut sessions = manager.sessions.lock().unwrap();
        let session = sessions.get_mut(&pty_id).ok_or("PTY 会话不存在")?;
        session.resize(cols, rows)
    }
}

/* ── Tauri Commands (File System) ──────────────────────── */

#[tauri::command]
fn read_dir(path: String) -> Result<ReadDirResponse, String> {
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
    const MAX_SIZE: u64 = 1_048_576;
    let metadata = fs::metadata(&path).map_err(|e| format!("无法访问 '{}': {}", path, e))?;
    if metadata.is_dir() { return Err("这是一个目录，不能作为文本打开".to_string()); }
    if metadata.len() > MAX_SIZE { return Err(format!("文件太大 ({}KB)，预览限制 1MB", metadata.len() / 1024)); }
    fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {}", e))
}

#[tauri::command]
async fn prepare_shutdown(app: tauri::AppHandle) -> Result<(), String> {
    eprintln!("[Frontend] 收到关闭通知");
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
        .manage(Arc::new(terminal::TerminalStreamManager::new()))
        .setup(|app| {
            // 注册关闭钩子
            setup_shutdown_hook(&app.handle());

            // 创建 TransferManager（需要 AppHandle）
            let transfer_manager = Arc::new(transfer::TransferManager::new(app.handle().clone()));
            app.manage(transfer_manager);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            read_dir, stat_file, read_file_text,
            prepare_shutdown,
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
            connection::remote_write_file,
            connection::remote_apply_diff, // 差异同步保存
            connection::remote_delete,
            connection::remote_mkdir,
            connection::remote_rename,
            connection::remote_copy,
            connection::remote_move,
            connection::subscribe,
            connection::unsubscribe,
            terminal::remote_spawn_terminal,
            terminal::remote_terminal_write,
            terminal::remote_terminal_close,
            terminal::remote_terminal_resize,
            transfer::transfer_file,
            transfer::pause_transfer,
            transfer::resume_transfer,
            transfer::retry_transfer,
            transfer::cancel_transfer,
            transfer::check_file_exists,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
