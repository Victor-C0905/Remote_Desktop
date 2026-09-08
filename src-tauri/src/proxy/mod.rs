//! SOCKS5 over QUIC 代理会话管理
//!
//! 生命周期：
//! - proxy_start_session：本地 SOCKS5 监听（支持固定端口）+ spawn 浏览器
//! - QUIC 断开：suspend_session 仅停监听，浏览器保留（固定端口保证重连后浏览器无需重开）
//! - 重连后再次 start：检测到陈旧会话 → 沿用原端口重启监听，浏览器无感恢复
//! - proxy_stop_session（关浏览窗口）/ App 退出：停监听 + kill 浏览器
//! - 残留自愈（spawn_primary_browser）：App 异常退出后浏览器残留（占用
//!   profile 且代理参数指向死端口），新 spawn 会退化为转发进程 → 探测到
//!   后清理残留实例并重开真正的主进程

pub mod browser;
pub mod socks5;

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, State};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::connection::ConnectionManager;
use quirel_protocol::{Envelope, Payload};

/// 返回给前端的会话信息
#[derive(serde::Serialize)]
pub struct ProxySessionInfo {
    pub port: u16,
    pub browser: String,
}

/// 系统已安装的浏览器（前端选择列表）
#[derive(serde::Serialize)]
pub struct BrowserInfo {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// 列出本机可用浏览器（供远程浏览应用展示选择）
#[tauri::command]
pub fn proxy_list_browsers() -> Vec<BrowserInfo> {
    browser::list_browsers()
        .into_iter()
        .map(|(id, name, path)| BrowserInfo { id: id.into(), name: name.into(), path })
        .collect()
}

struct ProxySession {
    port: u16,
    /// 浏览器名（会话信息展示 / profile 目录定位）
    browser_name: String,
    /// 监听任务句柄（abort 即关闭监听）
    listener_task: tokio::task::JoinHandle<()>,
    /// 监听是否存活（suspend 时同步置 false，避免 is_finished 的微小竞态）
    listener_alive: bool,
    /// 浏览器子进程（stop 时 kill）
    browser_child: Arc<Mutex<Option<Child>>>,
}

/// 代理会话管理器（Tauri managed state）
pub struct ProxySessionManager {
    sessions: Mutex<HashMap<String, ProxySession>>,
    /// 会话创建串行锁：proxy_start_session 的"检查会话→绑端口→注册"必须原子，
    /// 否则并发调用（如前端 StrictMode 双触发）会双重绑定同一端口，
    /// 后到者报"端口被占用"误判为启动失败（实际前者已成功）
    creation_lock: tokio::sync::Mutex<()>,
}

impl ProxySessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            creation_lock: tokio::sync::Mutex::new(()),
        }
    }
}

impl Default for ProxySessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 连接断开时的挂起（由连接管理在移除连接时调用）：
/// 仅 abort 监听任务，**浏览器保留**（固定端口下重连后无需重开浏览器）。
/// 代理流本身随 QUIC 连接死亡自动关闭。彻底清理（关浏览器）交给
/// proxy_stop_session（关浏览窗口）或 App 退出。
pub fn suspend_session(app: &AppHandle, server_id: &str) {
    let sessions: State<ProxySessionManager> = app.state();
    let mut guard = sessions.sessions.lock().unwrap();
    if let Some(sess) = guard.get_mut(server_id) {
        sess.listener_task.abort();
        sess.listener_alive = false;
        tracing::info!(
            "[Proxy] 连接断开，代理挂起（浏览器保留等待重连）: server_id={}, port={}",
            server_id, sess.port
        );
    }
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
///
/// 握手阶段（open_bi + ProxyOpen 响应）限时 10 秒：僵尸/病态连接
/// （如休眠唤醒后未判死的连接）下快速失败，让浏览器立即收到
/// 连接关闭而非无限转圈等自身超时。
async fn handle_socks5_conn(
    mut tcp: TcpStream,
    quic_conn: Arc<quinn::Connection>,
) -> Result<(), String> {
    // 1. SOCKS5 握手（greeting + CONNECT）
    socks5::socks5_greeting(&mut tcp).await?;
    let target = socks5::socks5_read_connect(&mut tcp).await?;

    // 2. 开 QUIC 双向流，发 ProxyOpen 首帧（限时，防病态连接挂死）
    let (mut quic_tx, mut quic_rx) = tokio::time::timeout(
        Duration::from_secs(10),
        quic_conn.open_bi(),
    )
    .await
    .map_err(|_| "开代理流超时".to_string())?
    .map_err(|e| format!("开代理流失败: {}", e))?;
    // 代理流内 request_id 无匹配语义，固定 0
    let open = Envelope::new(
        0,
        Payload::ProxyOpen { host: target.host, port: target.port },
    );
    write_frame(&mut quic_tx, &open.encode()?).await?;

    // 3. 读 ProxyOpenResponse（限时，防病态连接挂死）
    // 双 ?：外层解 timeout 的 Elapsed（已转 String），内层解 read_frame 的 String 错误
    let resp_data = tokio::time::timeout(
        Duration::from_secs(10),
        read_frame(&mut quic_rx),
    )
    .await
    .map_err(|_| "等待代理响应超时".to_string())??
    .ok_or("代理流提前关闭")?;
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
    // 双向透传：两方向并发（保留半关闭排空语义）+ 收尾宽限。
    // 不限时的问题：浏览器关闭连接（FIN）后，浏览器→Agent 方向结束，但
    // Agent→浏览器方向在对端为 HTTP keep-alive 目标时永不 EOF → 任务与
    // TcpStream 永久泄漏 → CLOSE_WAIT 无限堆积。Windows 下（std 监听不带
    // SO_REUSEADDR）残留 Socket 会阻塞同端口重新 bind（10048），表现为
    // "端口绑定失败且重试无效"（泄漏任务随 QUIC 连接存活，重启 App 才消失）。
    // 宽限 10s：足够排空对端剩余数据；活跃传输（下载中）不会触发——
    // 计时仅在某一方向已经结束时才启动。
    const RELAY_LINGER_SECS: u64 = 10;
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
    tokio::pin!(a, b);
    let linger = Duration::from_secs(RELAY_LINGER_SECS);
    tokio::select! {
        _ = &mut a => {
            // 浏览器方向先结束：给 Agent 方向宽限期排空，超时强制关闭防泄漏
            if tokio::time::timeout(linger, &mut b).await.is_err() {
                tracing::debug!("[Proxy] 收尾宽限超时，强制关闭（防 CLOSE_WAIT 泄漏）");
            }
        }
        _ = &mut b => {
            // Agent 方向先结束：同理给浏览器方向宽限（浏览器可能仍在发送）
            if tokio::time::timeout(linger, &mut a).await.is_err() {
                tracing::debug!("[Proxy] 收尾宽限超时，强制关闭（防 CLOSE_WAIT 泄漏）");
            }
        }
    }
    Ok(())
}

/// 在指定端口启动 SOCKS5 监听任务（port=0 时系统随机分配）
///
/// 返回实际绑定端口与监听任务句柄。
///
/// bind 使用 socket2 设置 SO_REUSEADDR：会话停止/挂起后立即重绑同端口。
/// Windows 下 std/tokio 的 TcpListener::bind 默认不带该选项，旧会话残留的
/// CLOSE_WAIT/TIME_WAIT Socket（中继泄漏或 TCP 收尾延迟）会让重新 bind
/// 失败（10048），表现为"端口绑定失败且重试无效"。与 Agent 端
/// WebSocket/QUIC 监听的项目惯例一致（见 agent quic.rs / websocket.rs）。
async fn spawn_socks5_listener(
    quic_conn: Arc<quinn::Connection>,
    port: u16,
) -> Result<(u16, tokio::task::JoinHandle<()>), String> {
    let sock_addr: std::net::SocketAddr = (std::net::Ipv4Addr::LOCALHOST, port).into();
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .map_err(|e| format!("创建 SOCKS5 socket 失败: {}", e))?;
    socket
        .set_reuse_address(true)
        .map_err(|e| format!("设置 SO_REUSEADDR 失败: {}", e))?;
    socket
        .set_nonblocking(true)
        .map_err(|e| format!("设置非阻塞模式失败: {}", e))?;
    socket
        .bind(&sock_addr.into())
        .map_err(|e| format!("SOCKS5 监听绑定端口 {} 失败: {}", port, e))?;
    socket
        .listen(1024)
        .map_err(|e| format!("SOCKS5 监听启动失败: {}", e))?;
    // socket2 → std → tokio（非阻塞已设置，from_std 不会阻塞注册）
    let listener = tokio::net::TcpListener::from_std(std::net::TcpListener::from(socket))
        .map_err(|e| format!("SOCKS5 监听注册失败: {}", e))?;
    let actual_port = listener.local_addr().map_err(|e| e.to_string())?.port();

    // 监听循环
    let quic_for_accept = quic_conn;
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

    Ok((actual_port, listener_task))
}

/// 端口占用预检：同一固定端口只允许一个活跃会话
///
/// 返回占用者 server_id（用于友好报错），无冲突时返回 None。
fn port_conflict_with(app: &AppHandle, server_id: &str, port: u16) -> Option<String> {
    let sessions: State<ProxySessionManager> = app.state();
    let guard = sessions.sessions.lock().unwrap();
    guard.iter()
        .find(|(id, s)| **id != server_id && s.listener_alive && s.port == port)
        .map(|(id, _)| id.clone())
}

/// 误杀保护：其他活跃会话是否正在使用同一浏览器（同 profile 主进程）
///
/// 同 profile 单实例下，其他活跃会话的浏览器主进程必须保留（其窗口依赖它）。
/// 残留清理前必须检查，否则会杀掉其他会话正在使用的浏览器。
fn other_active_session_uses_browser(app: &AppHandle, server_id: &str, browser_name: &str) -> bool {
    let sessions: State<ProxySessionManager> = app.state();
    let guard = sessions.sessions.lock().unwrap();
    guard.iter().any(|(id, s)| {
        *id != server_id
            && s.listener_alive
            && s.browser_name == browser_name
            // 浏览器主进程仍在运行（try_wait 出错时保守视为存活）
            && s.browser_child.lock().unwrap().as_mut()
                .map(|c| c.try_wait().map(|st| st.is_none()).unwrap_or(true))
                .unwrap_or(false)
    })
}

/// 探测 spawn 出的浏览器进程是否"快速退出"（转发进程特征）
///
/// 同 profile 已有实例（如上次 App 异常退出残留的浏览器）时，新 spawn 的
/// 进程只是转发器：把 --new-window 请求转给已运行实例后数百毫秒内退出；
/// 真正的主进程持续存活。探测窗口 ~750ms（150ms × 5 轮）：
/// - 期间任一时刻发现已退出 → 转发进程
/// - 全程存活 → 主进程（正常启动最多增加 150ms 首轮等待）
async fn exited_quickly(child: &mut Child) -> bool {
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        if matches!(child.try_wait(), Ok(Some(_))) {
            return true;
        }
    }
    false
}

/// spawn 浏览器并确保其为 profile 的主进程（非转发进程）
///
/// 残留场景自愈：上次 App 异常退出后浏览器残留（代理参数指向已死端口，
/// 无法接管），新 spawn 会退化为转发进程。探测到后：
/// 1. 误杀保护：其他活跃会话在用同浏览器 → 报错（同 profile 单实例，
///    本会话无法获得独立主进程）
/// 2. 清理残留实例（按 profile 路径精确匹配，见 browser::terminate_profile_processes）
/// 3. 等待 profile 单实例锁随进程退出释放，重新 spawn 真正的主进程
async fn spawn_primary_browser(
    app: &AppHandle,
    server_id: &str,
    browser_path: &PathBuf,
    browser_name: &str,
    port: u16,
    profile: &PathBuf,
) -> Result<Child, String> {
    let mut child = browser::spawn_browser(browser_path, port, profile)?;

    if !exited_quickly(&mut child).await {
        // 持续存活 → 真正的主进程
        return Ok(child);
    }

    // 快速退出 → profile 被其他实例占用
    if other_active_session_uses_browser(app, server_id, browser_name) {
        return Err(format!(
            "浏览器 {} 正被其他服务器的浏览会话使用，请先关闭其浏览窗口",
            browser_name
        ));
    }

    // 残留实例带着旧代理参数（指向已死端口），清理后重开
    tracing::info!(
        "[Proxy] 检测到 profile 被残留浏览器占用，清理后重开: {}",
        profile.display()
    );
    browser::terminate_profile_processes(browser_path, profile)?;
    // 等待 profile 单实例锁随残留进程退出释放（子进程句柄释放有延迟）
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut child = browser::spawn_browser(browser_path, port, profile)?;
    if exited_quickly(&mut child).await {
        // 二次验证仍快速退出：清理未生效或锁未释放，报错让用户手动处理
        return Err(format!(
            "浏览器启动后立即退出（profile 可能仍被占用: {}），请手动关闭相关浏览器进程后重试",
            profile.display()
        ));
    }
    Ok(child)
}

/// 启动代理会话：SOCKS5 监听（支持固定端口）+ 浏览器
///
/// - `browser_id`：空/"auto" 自动探测；候选 id（edge/chrome/firefox）选指定浏览器；
///   其他值视为自定义浏览器可执行文件路径
/// - `port`：固定端口（断线重连后浏览器无需重开的前提）；None/0 时随机分配
///
/// 会话状态机：
/// - 无会话 → 全新启动（绑端口 + spawn 浏览器）
/// - 会话活跃（监听 + 浏览器都在）→ 仅再开一个浏览器窗口
/// - 会话陈旧（断线挂起 / 浏览器被手动关闭）→ 原端口重启监听；
///   浏览器还活着则不重开（无感恢复），死了则重新 spawn
#[tauri::command]
pub async fn proxy_start_session(
    server_id: String,
    browser_id: Option<String>,
    port: Option<u16>,
    app: AppHandle,
) -> Result<ProxySessionInfo, String> {
    // 串行化整个创建流程（跨 await 持锁）：并发调用排队执行，
    // 后到者会看到先到者刚注册的会话并走"已存在"路径，不再双重绑端口
    let sessions_state = app.state::<ProxySessionManager>();
    let _creation_guard = sessions_state.creation_lock.lock().await;

    // 从 ConnectionManager 取该 server 的 QUIC 连接（代理流只能走 QUIC）
    let quic_conn = {
        let manager: State<ConnectionManager> = app.state();
        let conns = manager.connections.lock().unwrap();
        conns.get(&server_id)
            .and_then(|c| c.quic_conn.clone())
            .ok_or_else(|| format!("服务器 {} 未连接或连接类型非 QUIC，无法启动代理", server_id))?
    };

    // 解析浏览器选择（auto / 指定 id / 自定义路径）
    let browser_id = browser_id.unwrap_or_default();
    let (browser_path, browser_name) = browser::resolve_browser(&browser_id)?;
    let requested_port = port.filter(|p| *p > 0);

    // ── 会话已存在：按状态分发 ─────────────────────────────
    let existing_state = {
        let sessions: State<ProxySessionManager> = app.state();
        let guard = sessions.sessions.lock().unwrap();
        guard.get(&server_id).map(|s| {
            (
                s.port,
                s.browser_name.clone(),
                s.listener_alive,
                // 浏览器主进程是否仍在运行（try_wait 出错时保守视为存活）
                s.browser_child.lock().unwrap().as_mut()
                    .map(|c| c.try_wait().map(|st| st.is_none()).unwrap_or(true))
                    .unwrap_or(false),
            )
        })
    };

    if let Some((sess_port, sess_browser_name, listener_alive, browser_alive)) = existing_state {
        if !listener_alive {
            // ── 陈旧会话（断线挂起）：重建监听 ──
            // 浏览器还活着 → 必须沿用原端口（浏览器代理参数指向它）；
            // 浏览器已关 → 无端口约束，优先用新设置的固定端口
            let rebuild_port = if browser_alive { sess_port } else { requested_port.unwrap_or(sess_port) };
            if let Some(holder) = port_conflict_with(&app, &server_id, rebuild_port) {
                return Err(format!(
                    "端口 {} 已被服务器 {} 的浏览会话占用，请先关闭其浏览窗口或在设置中更换端口",
                    rebuild_port, holder
                ));
            }

            // 绑定失败（如端口被非本应用进程占用）时直接报错，不 spawn 浏览器
            let (actual_port, listener_task) = spawn_socks5_listener(quic_conn.clone(), rebuild_port).await?;

            // 浏览器已关才重开；还开着则无感恢复
            let new_child = if browser_alive {
                None
            } else {
                let profile = browser_profile_dir(&app, &browser_name)?;
                // spawn_primary_browser：残留实例占用 profile 时清理后重开
                // （否则新进程退化为转发进程，浏览器仍用旧代理参数连不上）
                Some(match spawn_primary_browser(
                    &app, &server_id, &browser_path, &browser_name, actual_port, &profile,
                ).await {
                    Ok(c) => c,
                    Err(e) => {
                        listener_task.abort();
                        return Err(e);
                    }
                })
            };

            // 更新会话条目（期间条目被并发替换的概率极低，覆盖即可）
            let sessions: State<ProxySessionManager> = app.state();
            let mut guard = sessions.sessions.lock().unwrap();
            if let Some(sess) = guard.get_mut(&server_id) {
                sess.port = actual_port;
                sess.browser_name = browser_name.to_string();
                sess.listener_task = listener_task;
                sess.listener_alive = true;
                if let Some(child) = new_child {
                    *sess.browser_child.lock().unwrap() = Some(child);
                }
            }

            tracing::info!(
                "[Proxy] 会话已恢复: server_id={}, socks_port={}, browser_alive={}, browser={}",
                server_id, actual_port, browser_alive, browser_name
            );
            return Ok(ProxySessionInfo { port: actual_port, browser: browser_name.to_string() });
        }

        // ── 监听仍在运行（连接未断）──
        if browser_alive {
            // 活跃会话：只 spawn 新浏览器窗口（同 profile），不重建监听
            let profile = browser_profile_dir(&app, &browser_name)?;
            // 浏览器同 profile 单实例：后续 spawn 只发消息即退出，无需跟踪 Child
            let _ = browser::spawn_browser(&browser_path, sess_port, &profile)?;
            return Ok(ProxySessionInfo { port: sess_port, browser: sess_browser_name });
        }

        // 监听活着但浏览器被手动关闭：连接没变，无需重绑端口，补 spawn 浏览器即可
        let profile = browser_profile_dir(&app, &browser_name)?;
        // spawn_primary_browser：残留实例占用 profile 时清理后重开（同上）
        let child = match spawn_primary_browser(
            &app, &server_id, &browser_path, &browser_name, sess_port, &profile,
        ).await {
            Ok(c) => c,
            Err(e) => return Err(e),
        };
        let sessions: State<ProxySessionManager> = app.state();
        let mut guard = sessions.sessions.lock().unwrap();
        if let Some(sess) = guard.get_mut(&server_id) {
            *sess.browser_child.lock().unwrap() = Some(child);
            sess.browser_name = browser_name.to_string();
        }
        tracing::info!(
            "[Proxy] 浏览器重启（监听未变）: server_id={}, socks_port={}, browser={}",
            server_id, sess_port, browser_name
        );
        return Ok(ProxySessionInfo { port: sess_port, browser: browser_name.to_string() });
    }

    // ── 全新会话 ─────────────────────────────────────────
    let bind_port = requested_port.unwrap_or(0);
    if let Some(holder) = port_conflict_with(&app, &server_id, bind_port) {
        return Err(format!(
            "端口 {} 已被服务器 {} 的浏览会话占用，请先关闭其浏览窗口或在设置中更换端口",
            bind_port, holder
        ));
    }
    let (port, listener_task) = spawn_socks5_listener(quic_conn.clone(), bind_port).await?;

    // spawn 浏览器；失败时必须回收已启动的监听任务（否则端口监听永久泄漏）
    let profile = match browser_profile_dir(&app, &browser_name) {
        Ok(p) => p,
        Err(e) => {
            listener_task.abort();
            return Err(e);
        }
    };
    // spawn_primary_browser：残留实例占用 profile 时清理后重开
    // （App 异常退出后浏览器残留场景，新 spawn 否则退化为转发进程，
    //   残留浏览器仍用旧代理参数 → 页面连接错误且重试无效）
    let child = match spawn_primary_browser(
        &app, &server_id, &browser_path, &browser_name, port, &profile,
    ).await {
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
            browser_name: browser_name.to_string(),
            listener_task,
            listener_alive: true,
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

/// 代理浏览器的独立 profile 目录，按浏览器分离
///
/// Edge/Chrome/Firefox 的 profile 数据互不兼容，共用同一目录会导致
/// 后切换的浏览器报"配置文件无法打开"并重建（登录态丢失）。
/// 每个浏览器一个子目录，各自的登录态/书签/扩展独立保留。
fn browser_profile_dir(app: &AppHandle, browser_name: &str) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("获取缓存目录失败: {}", e))?;
    // 目录名做基础净化（浏览器名来自固定候选表或自定义路径，通常安全，防御性处理）
    let safe_name = browser_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>();
    let profile = dir.join("browser-profile").join(safe_name);
    std::fs::create_dir_all(&profile).map_err(|e| format!("创建 profile 目录失败: {}", e))?;
    Ok(profile)
}
