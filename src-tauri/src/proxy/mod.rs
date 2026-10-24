//! SOCKS5 over QUIC 代理会话管理
//!
//! 生命周期：
//! - proxy_start_session / browser_open：绑 SOCKS5 监听（支持固定端口）+ 启动内置浏览器
//!   （WebView2 独立 Environment，user-data-dir 按 server 隔离 + 代理参数锁定端口）
//! - QUIC 断开：suspend_session 仅停监听，浏览器窗口保留（页面显示连接错误）
//! - 重连后再次 start：检测到陈旧会话 → 沿用原端口重启监听，浏览器无感恢复
//! - browser_close_window（关浏览器窗口）：销毁 webview + Environment，监听保留
//! - proxy_stop_session / App 退出：停监听 + 销毁浏览器

pub mod browser;
pub mod browser_settings;
pub mod browser_window;
pub mod socks5;
pub mod suggest;
pub mod webview_host;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, State};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::connection::ConnectionManager;
use crate::errors::{codes, AppError};
use quirel_protocol::{Envelope, Payload};

/// 返回给前端的会话信息
#[derive(serde::Serialize)]
pub struct ProxySessionInfo {
    pub port: u16,
}

struct ProxySession {
    port: u16,
    /// 监听任务句柄（abort 即关闭监听）
    listener_task: tokio::task::JoinHandle<()>,
    /// 监听是否存活（suspend 时同步置 false，避免 is_finished 的微小竞态）
    listener_alive: bool,
    /// 内置浏览器宿主（None = 窗口已关但监听保留，可重建）
    host: std::sync::Mutex<Option<Arc<browser_window::BrowserWindowHost>>>,
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

    /// server 的 SOCKS5 监听端口（仅活跃会话；断线挂起返回 None）
    ///
    /// 供 suggest 等本地复用隧道的功能取端口。过滤 listener_alive：挂起会话
    /// 的端口已被释放，若不过滤，随机端口被其他会话复用时会话错接（联想词
    /// 经别的 server 隧道出去，搜索词跨服务器泄漏）。
    pub(crate) fn socks_port(&self, server_id: &str) -> Option<u16> {
        self.sessions
            .lock()
            .unwrap()
            .get(server_id)
            .filter(|s| s.listener_alive)
            .map(|s| s.port)
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
        // 挂起时隐藏 webview：多服务器会话并存时，后创建会话的 webview z-order
        // 恒高，切回旧服务器时残留可见的旧 webview 会压在新标签条上；重连成功
        // 后由前端 browser_set_visible effect 恢复可见性
        if let Some(host) = sess.host.lock().unwrap().as_ref() {
            host.set_visible(false);
        }
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

/// 移除会话并终止其监听任务与浏览器（幂等：会话不存在时静默）。
/// cleanup_all_sessions（App 退出）与 proxy_stop_session 的公共实现。
fn remove_and_terminate(app: &AppHandle, server_id: &str, reason: &str) {
    let sessions: State<ProxySessionManager> = app.state();
    let session = sessions.sessions.lock().unwrap().remove(server_id);
    if let Some(sess) = session {
        sess.listener_task.abort();
        // host drop（Arc 归零）→ BrowserWindowHost::Drop 通知 runtime 线程销毁全部 webview
        drop(sess.host.into_inner().unwrap());
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
) -> Result<(u16, tokio::task::JoinHandle<()>), AppError> {
    let sock_addr: std::net::SocketAddr = (std::net::Ipv4Addr::LOCALHOST, port).into();
    // 监听启动各环节失败统一报 proxy_listen_failed（用户视角：本地代理端口起不来），
    // 技术差异进 detail 供日志排查
    let listen_err = |detail: String| {
        AppError::new(codes::PROXY_LISTEN_FAILED)
            .with_param("port", port.to_string())
            .with_detail(detail)
    };
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .map_err(|e| listen_err(format!("create socket: {e}")))?;
    socket
        .set_reuse_address(true)
        .map_err(|e| listen_err(format!("set SO_REUSEADDR: {e}")))?;
    socket
        .set_nonblocking(true)
        .map_err(|e| listen_err(format!("set nonblocking: {e}")))?;
    socket
        .bind(&sock_addr.into())
        .map_err(|e| listen_err(format!("bind: {e}")))?;
    socket
        .listen(1024)
        .map_err(|e| listen_err(format!("listen: {e}")))?;
    // socket2 → std → tokio（非阻塞已设置，from_std 不会阻塞注册）
    let listener = tokio::net::TcpListener::from_std(std::net::TcpListener::from(socket))
        .map_err(|e| listen_err(format!("register listener: {e}")))?;
    let actual_port = listener
        .local_addr()
        .map_err(|e| listen_err(format!("local addr: {e}")))?
        .port();

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

/// 固定端口监听：与其他会话冲突或绑定失败时回落随机端口
///
/// 仅用于浏览器打开入口（browser_open 携带的用户设置固定端口）：回落保证
/// 浏览器总能打开（warn 记录固定端口不可用原因）。host 存活的恢复路径
/// 不可用本函数——Environment 代理参数锁定原端口，回落会让浏览器流量
/// 指向死端口，该路径由调用方直接 spawn 并保持报错。
async fn spawn_listener_with_fallback(
    app: &AppHandle,
    server_id: &str,
    fixed: u16,
    quic_conn: Arc<quinn::Connection>,
) -> Result<(u16, tokio::task::JoinHandle<()>), AppError> {
    // 与其他会话的固定端口冲突：同属"固定端口不可用"，一并回落
    if let Some(holder) = port_conflict_with(app, server_id, fixed) {
        tracing::warn!(
            "[Proxy] 固定端口 {} 与会话 {} 冲突，回落随机端口",
            fixed, holder
        );
        return spawn_socks5_listener(quic_conn, 0).await;
    }
    match spawn_socks5_listener(quic_conn.clone(), fixed).await {
        Ok(result) => Ok(result),
        Err(e) => {
            tracing::warn!(
                "[Proxy] 固定端口 {} 绑定失败（{}），回落随机端口",
                fixed,
                e.detail.unwrap_or_else(|| "未知错误".into())
            );
            spawn_socks5_listener(quic_conn, 0).await
        }
    }
}

/// 浏览器数据根目录（browser-data/，按 server 分子目录）
fn browser_data_root(app: &AppHandle) -> Result<PathBuf, AppError> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("app cache dir: {e}")))?;
    Ok(dir.join("browser-data"))
}

/// server_id → 路径安全片段（uuid；防御性净化，未来 id 来源变化也不产生路径穿越）
fn safe_server_id(server_id: &str) -> String {
    server_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// 浏览器数据目录（user-data-dir），按 server 隔离（spec 第 5 节第 4 条）。
/// 同时承载随服务器持久化的浏览器设置（browser_settings::load/save）。
pub(crate) fn browser_data_dir(app: &AppHandle, server_id: &str) -> Result<PathBuf, AppError> {
    let dir = browser_data_root(app)?.join(safe_server_id(server_id));
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("create browser data dir: {e}")))?;
    Ok(dir)
}

/// 无痕浏览数据目录（持久目录的兄弟目录）：每次无痕会话即弃即焚——
/// 开启前清残留 + 会话结束（runtime 退出）删除。settings.json 等持久
/// 数据始终在 browser_data_dir，不受无痕清空影响。
fn browser_incognito_dir(app: &AppHandle, server_id: &str) -> Result<PathBuf, AppError> {
    Ok(browser_data_root(app)?.join(format!("{}-incognito", safe_server_id(server_id))))
}

/// 取主窗口 HWND（Controller 的 parent）
///
/// 返回句柄整数值而非 HWND：HWND 含裸指针不满足 Send，跨 await 持有会使
/// command future 非 Send（tauri 要求 Send）；spawn 线程内还原为 HWND
/// （与 browser_window.rs SendHwnd 同一语义）。
fn main_window_hwnd(app: &AppHandle) -> Result<isize, AppError> {
    app.get_webview_window("main")
        .ok_or_else(|| AppError::new(codes::PROXY_BROWSER_WINDOW_FAILED).with_detail("main window not found"))?
        .hwnd()
        // hwnd() 成功即返回真实窗口句柄（非 0）：句柄为 0/NULL 的情形在
        // tauri 内部已作为 Err 返回，故 Ok 分支非 0 有保证，spawn 侧无需二次校验
        .map(|h| h.0 as isize)
        .map_err(|e| AppError::new(codes::PROXY_BROWSER_WINDOW_FAILED).with_detail(format!("hwnd: {e}")))
}

/// 为会话创建浏览器 host（Environment + 首个 tab；睡眠超时由前端设置传入，
/// 恢复路径无入参时默认 120 分钟——计划 Step 4）
///
/// 浏览器设置随服务器持久化（data_dir/settings.json）：无痕态改用即弃
/// 兄弟目录（开启前清残留），正常态目录原样复用（登录态保留）
async fn create_host(
    app: &AppHandle,
    server_id: &str,
    port: u16,
    sleep_minutes: u64,
    first_url: &str,
) -> Result<Arc<browser_window::BrowserWindowHost>, AppError> {
    let data_dir = browser_data_dir(app, server_id)?;
    let settings = browser_settings::load(&data_dir);
    let profile_dir = if settings.incognito {
        let incognito_dir = browser_incognito_dir(app, server_id)?;
        // 残留清理：上次无痕会话可能未及删除（崩溃/强杀），开启前兜底；
        // NotFound 属正常（目录本就应不存在）
        if let Err(e) = std::fs::remove_dir_all(&incognito_dir) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("[Proxy] 无痕数据目录残留清理失败（尽力继续）: {e}");
            }
        }
        std::fs::create_dir_all(&incognito_dir)
            .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("create incognito data dir: {e}")))?;
        incognito_dir
    } else {
        data_dir
    };
    let hwnd = main_window_hwnd(app)?;
    browser_window::BrowserWindowHost::spawn(
        app.clone(),
        server_id.to_string(),
        profile_dir,
        port,
        hwnd,
        std::time::Duration::from_secs(sleep_minutes * 60),
        first_url.to_string(),
        settings,
    )
    .await
}

/// 默认首页 URL：当前设置搜索引擎的官网（随服务器持久化于 settings.json）。
/// 秒开依赖 webview user_data_dir 的持久化 HTTP 缓存。目录定位失败时回退
/// 默认引擎（Google）官网——首页解析不应阻断浏览器打开。
fn default_home_url(app: &AppHandle, server_id: &str) -> String {
    browser_data_dir(app, server_id)
        .map(|dir| browser_settings::load(&dir).search_engine.home_url())
        .unwrap_or_else(|_| browser_settings::SearchEngine::default().home_url())
}

/// 启动代理会话：SOCKS5 监听（支持固定端口）+ 内置浏览器
///
/// 会话状态机：
/// - 无会话 → 全新启动（绑端口 + 创建 host）
/// - 监听活跃 + host 在 → no-op（返回现状，前端聚焦窗口）
/// - 监听活跃 + host 无（窗口刚关）→ 原端口重建 host（Environment 重新创建，登录态保留）
/// - 陈旧会话（断线挂起）→ 原端口（host 在时）或设置端口重启监听；host 无则重建
#[tauri::command]
pub async fn proxy_start_session(
    server_id: String,
    port: Option<u16>,
    app: AppHandle,
) -> Result<ProxySessionInfo, AppError> {
    // 恢复路径无 sleep_minutes 入参，统一默认 120（重建 host 场景罕见，
    // 设置变更主要影响新会话——计划 Step 4）
    let first_url = default_home_url(&app, &server_id);
    start_session_inner(&server_id, port, 120, &first_url, &app).await
}

/// browser_open(server_id, url?, sleep_minutes?, port?)：前端打开浏览器唯一入口（spec 第 8 节）
///
/// port 为用户设置的固定 SOCKS5 端口（None/0 = 随机端口）：透传给会话启动，
/// 使"固定端口"设置在 App 重启后仍生效（无它则恒随机端口，设置沦为死配置）。
#[tauri::command]
pub async fn browser_open(
    server_id: String,
    url: Option<String>,
    sleep_minutes: Option<u64>,
    port: Option<u16>,
    app: AppHandle,
) -> Result<ProxySessionInfo, AppError> {
    // url 未指定 → 默认首页 = 当前设置搜索引擎的官网（settings.json）
    let first_url = url.unwrap_or_else(|| default_home_url(&app, &server_id));
    start_session_inner(
        &server_id,
        port,
        sleep_minutes.unwrap_or(120),
        &first_url,
        &app,
    )
    .await
}

async fn start_session_inner(
    server_id: &str,
    requested_port: Option<u16>,
    sleep_minutes: u64,
    first_url: &str,
    app: &AppHandle,
) -> Result<ProxySessionInfo, AppError> {
    // 串行化整个创建流程（跨 await 持锁）：并发调用排队执行，
    // 后到者会看到先到者刚注册的会话并走"已存在"路径，不再双重绑端口
    let sessions_state = app.state::<ProxySessionManager>();
    let _creation_guard = sessions_state.creation_lock.lock().await;

    let requested_port = requested_port.filter(|p| *p > 0);

    // 从 ConnectionManager 取该 server 的 QUIC 连接（代理流只能走 QUIC）
    let quic_conn = {
        let manager: State<ConnectionManager> = app.state();
        let conns = manager.connections.lock().unwrap();
        conns.get(server_id)
            .and_then(|c| c.quic_conn.clone())
            .ok_or_else(|| AppError::new(codes::CONNECTION_NOT_FOUND))?
    };

    // ── 会话已存在：按状态分发 ─────────────────────────────
    let existing = {
        let sessions: State<ProxySessionManager> = app.state();
        let guard = sessions.sessions.lock().unwrap();
        guard.get(server_id).map(|s| {
            (s.port, s.listener_alive, s.host.lock().unwrap().is_some())
        })
    };

    if let Some((sess_port, listener_alive, host_alive)) = existing {
        if listener_alive {
            if host_alive {
                // 活跃会话：no-op
                return Ok(ProxySessionInfo { port: sess_port });
            }
            // 窗口刚关（监听保留）：原端口重建 host（Environment 重新创建）
            let host = create_host(app, server_id, sess_port, sleep_minutes, first_url).await?;
            let sessions: State<ProxySessionManager> = app.state();
            if let Some(sess) = sessions.sessions.lock().unwrap().get_mut(server_id) {
                *sess.host.lock().unwrap() = Some(host);
            } else {
                // 竞态防御：create_host 期间会话可能被并发移除（如 proxy_stop_session
                // / App 退出走的 remove_and_terminate）。新 host 未入册即成孤儿，
                // 显式 request_close 通知 runtime 线程销毁窗口并报错（幂等；即便
                // 遗漏，局部 Arc drop 也会经 BrowserWindowHost::Drop 兜底关闭）
                host.request_close();
                return Err(AppError::new(codes::PROXY_BROWSER_WINDOW_CLOSED)
                    .with_detail("session removed concurrently while recreating host"));
            }
            tracing::info!("[Proxy] 浏览器窗口重建（监听未变）: server_id={}, port={}", server_id, sess_port);
            return Ok(ProxySessionInfo { port: sess_port });
        }

        // ── 陈旧会话（断线挂起）：重建监听 ──
        // host 还在 → 必须沿用原端口（Environment 代理参数指向它），绑定失败
        // 保持报错（回落会让浏览器流量指向死端口）；
        // host 已关 → 无端口约束：带固定端口请求时失败回落随机端口
        // （Environment 将以实际端口重建），无请求时沿用原端口（现状语义）
        let (actual_port, listener_task) = match (host_alive, requested_port) {
            (true, _) | (false, None) => {
                if let Some(holder) = port_conflict_with(app, server_id, sess_port) {
                    return Err(
                        AppError::new(codes::PROXY_PORT_CONFLICT)
                            .with_param("port", sess_port.to_string())
                            .with_param("holder", holder),
                    );
                }
                spawn_socks5_listener(quic_conn, sess_port).await?
            }
            (false, Some(fixed)) => {
                spawn_listener_with_fallback(app, server_id, fixed, quic_conn).await?
            }
        };

        // 旧 QUIC 连接对象已随断线更换：旧端口下池连接全部随旧流死亡，
        // 清空防复用首查必失败一次；随即按实际端口重预热热连接池（后台
        // 执行，与下方 host 重建并行）
        suggest::clear_streams_for_port(sess_port).await;
        tokio::spawn(suggest::warm_up_connections(actual_port));

        // host 已关才重建；还开着则无感恢复。失败时回收监听任务
        // （否则端口监听永久泄漏——沿用旧实现同路径的防泄漏语义）
        let new_host = if host_alive {
            None
        } else {
            Some(match create_host(app, server_id, actual_port, sleep_minutes, first_url).await {
                Ok(h) => h,
                Err(e) => {
                    listener_task.abort();
                    return Err(e);
                }
            })
        };

        let sessions: State<ProxySessionManager> = app.state();
        let mut guard = sessions.sessions.lock().unwrap();
        if let Some(sess) = guard.get_mut(server_id) {
            sess.port = actual_port;
            sess.listener_task = listener_task;
            sess.listener_alive = true;
            if let Some(host) = new_host {
                *sess.host.lock().unwrap() = Some(host);
            }
            // 恢复成功后重发全量标签列表，驱动前端标签条回灌：断线期间前端
            // 已清空 tabs，此后仅增量 page-state 事件（空列表上合并不生效），
            // 不补发则标签条空白直到用户新建/关闭标签
            if let Some(host) = sess.host.lock().unwrap().as_ref() {
                host.refresh_tabs();
            }
        } else {
            // 竞态防御：spawn_socks5_listener 之后、get_mut 之前会话可能被并发
            // 移除（remove_and_terminate）。新监听任务无人接管会成孤儿（端口
            // 监听永久泄漏），必须 abort；new_host 为局部 Arc，函数返回 drop
            // 时经 BrowserWindowHost::Drop 自动通知 runtime 销毁窗口，无需额外处理
            listener_task.abort();
            return Err(AppError::new(codes::PROXY_BROWSER_WINDOW_CLOSED)
                .with_detail("session removed concurrently during listener rebuild"));
        }
        tracing::info!(
            "[Proxy] 会话已恢复: server_id={}, socks_port={}, host_alive={}",
            server_id, actual_port, host_alive
        );
        return Ok(ProxySessionInfo { port: actual_port });
    }

    // ── 全新会话 ─────────────────────────────────────────
    // 固定端口：冲突或绑定失败回落随机端口（保证浏览器能打开，warn 记录）；
    // 随机端口（None）：维持现状
    let (port, listener_task) = match requested_port {
        Some(fixed) => spawn_listener_with_fallback(app, server_id, fixed, quic_conn).await?,
        None => spawn_socks5_listener(quic_conn, 0).await?,
    };

    // 创建 host；失败时回收监听任务（否则端口监听永久泄漏）
    let host = match create_host(app, server_id, port, sleep_minutes, first_url).await {
        Ok(h) => h,
        Err(e) => {
            listener_task.abort();
            return Err(e);
        }
    };

    let sessions: State<ProxySessionManager> = app.state();
    sessions.sessions.lock().unwrap().insert(
        server_id.to_string(),
        ProxySession {
            port,
            listener_task,
            listener_alive: true,
            host: std::sync::Mutex::new(Some(host)),
        },
    );

    // 兜底重发全量标签列表：首个 tab 的 emit_tabs 发生在 runtime 线程启动时
    // （早于 browser_open 返回），若前端 browser://tabs 监听尚未注册完成则
    // 事件丢失——tabs 停留空列表，此后仅增量 page-state（空列表上合并不
    // 生效），表现为占位提示常显、地址栏为空。返回前补发一次（此刻前端
    // invoke 尚未 resolve、监听必然就位），幂等无害。
    if let Some(sess) = sessions.sessions.lock().unwrap().get(server_id) {
        if let Some(host) = sess.host.lock().unwrap().as_ref() {
            host.refresh_tabs();
        }
    }

    tracing::info!("[Proxy] 会话已启动: server_id={}, socks_port={}", server_id, port);
    // 联想词连接预热：建连挪到启动时刻，首次联想直接用热连接（失败静默）
    tokio::spawn(suggest::warm_up_connections(port));
    Ok(ProxySessionInfo { port })
}

/// 停止代理会话：关监听 + 销毁浏览器
#[tauri::command]
pub async fn proxy_stop_session(server_id: String, app: AppHandle) -> Result<(), String> {
    remove_and_terminate(&app, &server_id, "会话已停止");
    Ok(())
}

/// 会话状态（proxy_session_status 返回）
#[derive(serde::Serialize)]
pub struct ProxySessionStatus {
    pub port: u16,
    /// 监听是否存活（false = 断线挂起，需重建）
    pub listener_alive: bool,
    /// 浏览器窗口是否开着（host 存在）
    pub browser_alive: bool,
}

/// 查询代理会话状态（无会话返回 null）
///
/// browser_alive = 浏览器窗口开着（host 存在）。前端据此区分重连后的
/// 恢复策略：窗口开着 → 调 proxy_start_session 仅重建监听（无感恢复）；
/// 窗口已关或无会话 → 等待用户主动打开。
#[tauri::command]
pub fn proxy_session_status(server_id: String, app: AppHandle) -> Option<ProxySessionStatus> {
    let sessions: State<ProxySessionManager> = app.state();
    let guard = sessions.sessions.lock().unwrap();
    guard.get(&server_id).map(|s| ProxySessionStatus {
        port: s.port,
        listener_alive: s.listener_alive,
        browser_alive: s.host.lock().unwrap().is_some(),
    })
}
