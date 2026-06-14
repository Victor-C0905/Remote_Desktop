use serde::Serialize;
use std::fs;
use std::time::UNIX_EPOCH;

mod connection;

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

/* ── App Entry ──────────────────────────────────────────── */

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(pty::PtyManager::new())
        .manage(connection::ConnectionManager::new())
        .invoke_handler(tauri::generate_handler![
            read_dir, stat_file, read_file_text,
            pty::spawn_terminal,
            pty::terminal_write,
            pty::terminal_read,
            pty::terminal_resize,
            connection::remote_connect,
            connection::remote_disconnect,
            connection::remote_ping,
            connection::remote_send,
            connection::remote_read_dir,
            connection::remote_get_metrics,
            connection::remote_read_file,
            connection::remote_write_file,
            connection::remote_delete,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
