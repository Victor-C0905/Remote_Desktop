// src-tauri/src/terminal.rs
// 远程终端 Tauri commands（符合 GNOME Terminal 标准）

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, Mutex};

use crate::connection::{ConnectionManager, Envelope, Payload};

/// 终端 Stream 初始连接超时（30秒）
/// 仅用于 Stream 创建和初始请求/响应，不影响后续数据传输
const TERMINAL_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteTerminalSession {
    pub session_id: String,
    pub server_id: String,
    pub cols: u16,
    pub rows: u16,
}

/// 终端 Stream 管理器（保存持久 Stream 的发送端）
#[derive(Clone)]
pub struct TerminalStreamManager {
    /// 活跃的终端会话（session_id -> 输入发送通道）
    sessions: Arc<Mutex<HashMap<String, mpsc::Sender<Vec<u8>>>>>,
}

impl TerminalStreamManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 获取内部的 Arc（用于克隆）
    #[allow(dead_code)]
    pub fn inner(&self) -> Arc<Mutex<HashMap<String, mpsc::Sender<Vec<u8>>>>> {
        self.sessions.clone()
    }

    /// 添加终端会话
    pub async fn add_session(&self, session_id: String, input_tx: mpsc::Sender<Vec<u8>>) {
        let mut sessions = self.sessions.lock().await;
        sessions.insert(session_id, input_tx);
    }

    /// 获取终端会话的输入通道
    pub async fn get_input_tx(&self, session_id: &str) -> Option<mpsc::Sender<Vec<u8>>> {
        let sessions = self.sessions.lock().await;
        sessions.get(session_id).cloned()
    }

    /// 移除终端会话
    pub async fn remove_session(&self, session_id: &str) {
        let mut sessions = self.sessions.lock().await;
        sessions.remove(session_id);
    }
}

impl Default for TerminalStreamManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tauri Commands ─────────────────────────────────

/// 创建远程终端会话（符合 GNOME Terminal 标准）
#[tauri::command]
pub async fn remote_spawn_terminal(
    server_id: String,
    shell: String,
    cols: u16,
    rows: u16,
    working_directory: Option<String>,
    app: tauri::AppHandle,
) -> Result<RemoteTerminalSession, String> {
    let manager = app.state::<ConnectionManager>();
    // 使用 Arc::clone 避免生命周期问题
    let terminal_manager = Arc::clone(&app.state::<Arc<TerminalStreamManager>>());

    // 获取 QUIC Connection
    let quic_conn = {
        let conns = manager.connections.lock().unwrap();
        let conn = conns.get(&server_id).ok_or("未找到连接")?;
        conn.quic_conn.clone()
    };

    let conn = quic_conn.ok_or("QUIC Connection 不可用")?;

    // 创建持久 Stream（双向隧道，添加超时）
    let stream = tokio::time::timeout(TERMINAL_CONNECT_TIMEOUT, conn.open_bi())
        .await
        .map_err(|_| "创建终端 Stream 超时".to_string())?
        .map_err(|e| format!("创建 Stream 失败: {}", e))?;

    let (mut send, mut recv) = stream;

    // 发送 TerminalSpawnRequest（添加超时）
    let request_id = manager.next_request_id();
    let envelope = Envelope::new(request_id, Payload::TerminalSpawnRequest {
        shell: shell.clone(),
        cols,
        rows,
        working_directory,
    });

    let data = envelope.encode()?;
    let len = (data.len() as u32).to_le_bytes();
    use tokio::io::AsyncWriteExt;
    tokio::time::timeout(TERMINAL_CONNECT_TIMEOUT, async {
        send.write_all(&len).await.map_err(|e| format!("发送长度失败: {}", e))?;
        send.write_all(&data).await.map_err(|e| format!("发送数据失败: {}", e))?;
        send.flush().await.map_err(|e| format!("flush 失败: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "发送终端创建请求超时".to_string())??;

    // 读取响应（添加超时）
    let resp_data = tokio::time::timeout(TERMINAL_CONNECT_TIMEOUT, async {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await.map_err(|e| format!("读取响应长度失败: {}", e))?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;
        let mut resp_data = vec![0u8; resp_len];
        recv.read_exact(&mut resp_data).await.map_err(|e| format!("读取响应数据失败: {}", e))?;
        Ok::<Vec<u8>, String>(resp_data)
    })
    .await
    .map_err(|_| "等待终端创建响应超时".to_string())??;

    let resp_envelope = Envelope::decode(&resp_data)?;
    let session_id = match resp_envelope.payload {
        Payload::TerminalSpawnResponse { session_id } => session_id,
        Payload::Error { message, .. } => return Err(message),
        _ => return Err("意外响应".into()),
    };

    tracing::info!("远程终端创建成功: server_id={}, session_id={}", server_id, session_id);

    // 创建输入通道（用于后续写入）
    let (input_tx, mut input_rx) = mpsc::channel::<Vec<u8>>(256);

    // 保存到管理器
    terminal_manager.add_session(session_id.clone(), input_tx).await;

    // 启动双向数据隧道任务
    let session_id_clone = session_id.clone();
    let server_id_clone = server_id.clone();
    let app_handle = app.clone();

    tokio::spawn(async move {
        // 客户端输入写入任务（从通道读取，写入 Stream）
        let session_id_input = session_id_clone.clone();
        let input_task = tokio::spawn(async move {
            while let Some(data) = input_rx.recv().await {
                // 直接写入原始字节到 Stream（不封装为 Envelope）
                let len = (data.len() as u32).to_le_bytes();
                if let Err(e) = send.write_all(&len).await {
                    tracing::warn!("写入终端输入长度失败: {}", e);
                    break;
                }
                if let Err(e) = send.write_all(&data).await {
                    tracing::warn!("写入终端输入失败: {}", e);
                    break;
                }
                if let Err(e) = send.flush().await {
                    tracing::warn!("flush 终端输入失败: {}", e);
                    break;
                }
                tracing::debug!("终端输入写入: len={}", data.len());
            }
            tracing::info!("输入写入任务结束: session_id={}", session_id_input);
        });

        // PTY 输出读取循环
        let session_id_output = session_id_clone.clone();
        loop {
            let mut len_buf = [0u8; 4];
            match recv.read_exact(&mut len_buf).await {
                Ok(_) => {
                    let len = u32::from_le_bytes(len_buf) as usize;
                    if len == 0 || len > 1024 * 1024 {
                        tracing::warn!("无效的终端数据长度: {}", len);
                        break;
                    }
                    let mut data = vec![0u8; len];
                    match recv.read_exact(&mut data).await {
                        Ok(_) => {
                            // 发送事件到前端
                            let _ = app_handle.emit("terminal-output", serde_json::json!({
                                "session_id": session_id_output,
                                "data": data,
                            }));
                        }
                        Err(e) => {
                            tracing::warn!("读取终端输出失败: {}", e);
                            break;
                        }
                    }
                }
                Err(quinn::ReadExactError::FinishedEarly(_)) => {
                    tracing::info!("终端 Stream 关闭");
                    break;
                }
                Err(e) => {
                    tracing::warn!("读取终端输出长度失败: {}", e);
                    break;
                }
            }
        }

        // 等待输入任务结束
        input_task.abort();

        // 清理会话
        terminal_manager.remove_session(&session_id_clone).await;

        // 通知前端终端断连
        let _ = app_handle.emit("terminal-disconnected", serde_json::json!({
            "server_id": server_id_clone,
            "session_id": session_id_clone,
        }));

        tracing::info!("终端 Stream 任务结束: session_id={}", session_id_clone);
    });

    Ok(RemoteTerminalSession {
        session_id: session_id.clone(),
        server_id,
        cols,
        rows,
    })
}

/// 写入数据到远程终端（使用持久 Stream）
#[tauri::command]
pub async fn remote_terminal_write(
    session_id: String,
    data: Vec<u8>,
    _server_id: String,  // 不再需要 server_id，使用持久 Stream
    app: tauri::AppHandle,
) -> Result<(), String> {
    let terminal_manager = app.state::<Arc<TerminalStreamManager>>();

    // 获取输入通道
    let input_tx = terminal_manager.get_input_tx(&session_id).await
        .ok_or("终端会话不存在或已关闭")?;

    // 发送数据到通道
    input_tx.send(data).await
        .map_err(|e| format!("发送终端输入失败: {}", e))?;

    Ok(())
}

/// 关闭远程终端会话
#[tauri::command]
pub async fn remote_terminal_close(
    session_id: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let terminal_manager = app.state::<Arc<TerminalStreamManager>>();

    // 移除会话（会关闭通道，导致 Stream 任务结束）
    terminal_manager.remove_session(&session_id).await;

    tracing::info!("远程终端关闭请求: session_id={}", session_id);
    Ok(())
}

/// 调整远程终端大小（同步到远程 PTY）
#[tauri::command]
pub async fn remote_terminal_resize(
    session_id: String,
    cols: u16,
    rows: u16,
    _server_id: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let manager = app.state::<ConnectionManager>();

    // 获取 QUIC Connection（用于发送 resize 请求）
    let quic_conn = {
        let conns = manager.connections.lock().unwrap();
        // 找到包含该 session 的连接
        // 简化处理：使用第一个可用连接
        conns.values().next().and_then(|c| c.quic_conn.clone())
    };

    let conn = quic_conn.ok_or("QUIC Connection 不可用")?;

    // 创建新的 Stream 发送 resize 请求（添加超时）
    let stream = tokio::time::timeout(TERMINAL_CONNECT_TIMEOUT, conn.open_bi())
        .await
        .map_err(|_| "创建 resize Stream 超时".to_string())?
        .map_err(|e| format!("创建 Stream 失败: {}", e))?;

    let (mut send, mut recv) = stream;

    // 发送 TerminalResizeRequest（添加超时）
    let request_id = manager.next_request_id();
    let envelope = Envelope::new(request_id, Payload::TerminalResizeRequest {
        session_id: session_id.clone(),
        cols,
        rows,
    });

    let data = envelope.encode()?;
    let len = (data.len() as u32).to_le_bytes();
    use tokio::io::AsyncWriteExt;
    tokio::time::timeout(TERMINAL_CONNECT_TIMEOUT, async {
        send.write_all(&len).await.map_err(|e| format!("发送长度失败: {}", e))?;
        send.write_all(&data).await.map_err(|e| format!("发送数据失败: {}", e))?;
        send.flush().await.map_err(|e| format!("flush 失败: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "发送 resize 请求超时".to_string())??;

    // 读取响应（添加超时，忽略响应内容）
    let _ = tokio::time::timeout(TERMINAL_CONNECT_TIMEOUT, async {
        let mut len_buf = [0u8; 4];
        if recv.read_exact(&mut len_buf).await.is_ok() {
            let resp_len = u32::from_le_bytes(len_buf) as usize;
            if resp_len > 0 && resp_len < 1024 * 1024 {
                let mut resp_data = vec![0u8; resp_len];
                let _ = recv.read_exact(&mut resp_data).await;
            }
        }
    })
    .await;

    tracing::debug!("远程终端 resize: session_id={}, {}x{}", session_id, cols, rows);
    Ok(())
}