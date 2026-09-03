//! SOCKS5 over QUIC 代理会话管理
//!
//! 生命周期：proxy_start_session 起本地 SOCKS5 监听 + spawn 浏览器；
//! QUIC 连接断开 → 所有代理流关闭 → 浏览器整体断网（设计使然，与登录态绑定）。

pub mod browser;
pub mod socks5;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::sync::Mutex;

use tauri::{AppHandle, Manager, State};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

use crate::connection::ConnectionManager;
use quirel_protocol::{Envelope, Payload};

/// 返回给前端的会话信息
#[derive(serde::Serialize)]
pub struct ProxySessionInfo {
    pub port: u16,
    pub browser: String,
}

struct ProxySession {
    port: u16,
    /// 监听任务句柄（abort 即关闭监听）
    listener_task: tokio::task::JoinHandle<()>,
    /// 浏览器子进程（stop 时 kill）
    browser_child: Arc<Mutex<Option<Child>>>,
}

/// 代理会话管理器（Tauri managed state）
pub struct ProxySessionManager {
    sessions: Mutex<HashMap<String, ProxySession>>,
}

impl ProxySessionManager {
    pub fn new() -> Self {
        Self { sessions: Mutex::new(HashMap::new()) }
    }
}

impl Default for ProxySessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 连接断开时的同步清理（由连接管理在移除连接时调用）：
/// abort 监听任务并 kill 浏览器，代理流本身随 QUIC 连接死亡自动关闭。
pub fn cleanup_session(app: &AppHandle, server_id: &str) {
    remove_and_terminate(app, server_id, "连接断开，代理会话已清理");
}

/// App 退出时清理全部代理会话（RunEvent::Exit 调用）：
/// 遍历清理，避免浏览器子进程残留指向已死的代理端口。
pub fn cleanup_all_sessions(app: &AppHandle) {
    let ids: Vec<String> = {
        let sessions: State<ProxySessionManager> = app.state();
        let guard = sessions.sessions.lock().unwrap();
        let ids = guard.keys().cloned().collect();
        drop(guard);
        ids
    };
    for id in ids {
        remove_and_terminate(app, &id, "App 退出，代理会话已清理");
    }
}

/// 移除会话并终止其监听任务与浏览器进程（幂等：会话不存在时静默）。
/// cleanup_session（断连清理）与 proxy_stop_session（手动停止）的公共实现。
fn remove_and_terminate(app: &AppHandle, server_id: &str, reason: &str) {
    let sessions: State<ProxySessionManager> = app.state();
    let session = sessions.sessions.lock().unwrap().remove(server_id);
    if let Some(sess) = session {
        sess.listener_task.abort();
        if let Some(mut child) = sess.browser_child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        tracing::info!("[Proxy] {}: server_id={}, port={}", reason, server_id, sess.port);
    }
}

/// QUIC 流上的长度前缀帧读写（与 Agent 端 read_message/write_message 同构）
async fn write_frame(send: &mut quinn::SendStream, data: &[u8]) -> Result<(), String> {
    send.write_all(&(data.len() as u32).to_le_bytes())
        .await
        .map_err(|e| format!("写帧头失败: {}", e))?;
    send.write_all(data).await.map_err(|e| format!("写帧失败: {}", e))
}

async fn read_frame(recv: &mut quinn::RecvStream) -> Result<Option<Vec<u8>>, String> {
    let mut len_buf = [0u8; 4];
    match recv.read_exact(&mut len_buf).await {
        Ok(()) => {}
        Err(quinn::ReadExactError::FinishedEarly(_)) => return Ok(None),
        Err(e) => return Err(format!("读帧头失败: {}", e)),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut data = vec![0u8; len];
    recv.read_exact(&mut data).await.map_err(|e| format!("读帧失败: {}", e))?;
    Ok(Some(data))
}

/// 单条 SOCKS5 连接的处理：握手 → 开 QUIC 代理流 → 双向透传
///
/// 契约：任何 Err 返回后调用方必须让 TcpStream 被 drop（关闭连接），
/// 避免浏览器侧挂等。
async fn handle_socks5_conn(
    mut tcp: TcpStream,
    quic_conn: Arc<quinn::Connection>,
) -> Result<(), String> {
    // 1. SOCKS5 握手（greeting + CONNECT）
    socks5::socks5_greeting(&mut tcp).await?;
    let target = socks5::socks5_read_connect(&mut tcp).await?;

    // 2. 开 QUIC 双向流，发 ProxyOpen 首帧
    let (mut quic_tx, mut quic_rx) = quic_conn
        .open_bi()
        .await
        .map_err(|e| format!("开代理流失败: {}", e))?;
    // 代理流内 request_id 无匹配语义，固定 0
    let open = Envelope::new(
        0,
        Payload::ProxyOpen { host: target.host, port: target.port },
    );
    write_frame(&mut quic_tx, &open.encode()?).await?;

    // 3. 读 ProxyOpenResponse
    let resp_data = read_frame(&mut quic_rx).await?.ok_or("代理流提前关闭")?;
    let resp = Envelope::decode(&resp_data).map_err(|e| format!("解码响应失败: {}", e))?;
    match resp.payload {
        Payload::ProxyOpenResponse { success: true, .. } => {}
        Payload::ProxyOpenResponse { success: false, error } => {
            // Agent 连不上目标 → 回 SOCKS5 拒绝（0x01 general failure）
            socks5::socks5_reply(&mut tcp, 0x01).await?;
            return Err(error.unwrap_or_else(|| "目标连接失败".into()));
        }
        other => {
            socks5::socks5_reply(&mut tcp, 0x01).await?;
            return Err(format!("意外响应: {}", other.type_name()));
        }
    }

    // 4. 回 SOCKS5 成功，之后双向透传
    socks5::socks5_reply(&mut tcp, 0x00).await?;
    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();
    // 双向透传：join! 两方向独立跑完（与 Agent 端一致，半关闭互不截断）
    let a = async {
        let r = tokio::io::copy(&mut tcp_rx, &mut quic_tx).await;
        let _ = quic_tx.finish();
        if let Err(e) = r {
            tracing::debug!("[Proxy] 浏览器→Agent 透传结束: {}", e);
        }
    };
    let b = async {
        let r = tokio::io::copy(&mut quic_rx, &mut tcp_tx).await;
        let _ = tcp_tx.shutdown().await;
        if let Err(e) = r {
            tracing::debug!("[Proxy] Agent→浏览器 透传结束: {}", e);
        }
    };
    tokio::join!(a, b);
    Ok(())
}

/// 启动代理会话：SOCKS5 监听 + 浏览器
#[tauri::command]
pub async fn proxy_start_session(
    server_id: String,
    app: AppHandle,
) -> Result<ProxySessionInfo, String> {
    // 从 ConnectionManager 取该 server 的 QUIC 连接（代理流只能走 QUIC）
    let quic_conn = {
        let manager: State<ConnectionManager> = app.state();
        let conns = manager.connections.lock().unwrap();
        conns.get(&server_id)
            .and_then(|c| c.quic_conn.clone())
            .ok_or_else(|| format!("服务器 {} 未连接或连接类型非 QUIC，无法启动代理", server_id))?
    };

    // 会话已存在：只 spawn 新浏览器窗口（同 profile），不重建监听
    {
        let sessions: State<ProxySessionManager> = app.state();
        let existing = sessions.sessions.lock().unwrap();
        if let Some(sess) = existing.get(&server_id) {
            let port = sess.port;
            drop(existing);
            let (path, _) = browser::detect_browser()
                .ok_or("未找到 Edge/Chrome，无法启动浏览")?;
            let profile = browser_profile_dir(&app)?;
            // 浏览器同 profile 单实例：后续 spawn 只发消息即退出，无需跟踪 Child
            let _ = browser::spawn_browser(&path, port, &profile)?;
            return Ok(ProxySessionInfo { port, browser: "existing".into() });
        }
    }

    // 探测浏览器
    let (browser_path, browser_name) =
        browser::detect_browser().ok_or("未找到 Edge/Chrome，无法启动浏览")?;

    // 绑定本地随机端口
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("SOCKS5 监听绑定失败: {}", e))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    // 监听循环
    let quic_for_accept = quic_conn.clone();
    let listener_task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((tcp, _addr)) => {
                    let quic = quic_for_accept.clone();
                    tokio::spawn(async move {
                        // Err 时 tcp 被 drop 关闭（见 handle_socks5_conn 契约）
                        if let Err(e) = handle_socks5_conn(tcp, quic).await {
                            tracing::debug!("[Proxy] SOCKS5 连接结束: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("[Proxy] SOCKS5 监听退出: {}", e);
                    break;
                }
            }
        }
    });

    // spawn 浏览器；失败时必须回收已启动的监听任务（否则随机端口监听永久泄漏）
    let profile = match browser_profile_dir(&app) {
        Ok(p) => p,
        Err(e) => {
            listener_task.abort();
            return Err(e);
        }
    };
    let child = match browser::spawn_browser(&browser_path, port, &profile) {
        Ok(c) => c,
        Err(e) => {
            listener_task.abort();
            return Err(e);
        }
    };

    // 注册会话
    let sessions: State<ProxySessionManager> = app.state();
    sessions.sessions.lock().unwrap().insert(
        server_id.clone(),
        ProxySession {
            port,
            listener_task,
            browser_child: Arc::new(Mutex::new(Some(child))),
        },
    );

    tracing::info!(
        "[Proxy] 会话已启动: server_id={}, socks_port={}, browser={}",
        server_id, port, browser_name
    );
    Ok(ProxySessionInfo { port, browser: browser_name.to_string() })
}

/// 停止代理会话：关监听 + kill 浏览器
#[tauri::command]
pub async fn proxy_stop_session(server_id: String, app: AppHandle) -> Result<(), String> {
    remove_and_terminate(&app, &server_id, "会话已停止");
    Ok(())
}

/// 浏览器独立 profile 目录（app cache 下）
fn browser_profile_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("获取缓存目录失败: {}", e))?;
    let profile = dir.join("browser-profile");
    std::fs::create_dir_all(&profile).map_err(|e| format!("创建 profile 目录失败: {}", e))?;
    Ok(profile)
}
