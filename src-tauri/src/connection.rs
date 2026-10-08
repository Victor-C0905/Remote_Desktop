use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::mpsc;

// ── 连接状态 ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub server_id: String,
    pub host: String,
    pub port: u16,
    #[serde(rename = "transport")]
    pub transport_type: String,
    #[serde(rename = "status")]
    pub status: String,
    #[serde(rename = "rttMs")]
    pub rtt_ms: f64,
    #[serde(rename = "connectedAt")]
    pub connected_at: u64,
}

// ── 认证凭据 ───────────────────────────────────────

/// 认证方式（与前端 AuthMethod 枚举对应）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    Password,
    PubKey,
}

/// 认证凭据结构体（与前端 AuthCredentials 接口对应）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// 认证方式
    pub method: AuthMethod,
    /// 用户名
    pub username: String,
    /// 密码（密码认证时使用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// SSH私钥内容（公钥认证时使用）
    #[serde(skip_serializing_if = "Option::is_none", rename = "private_key")]
    pub private_key: Option<String>,
    /// 私钥密码（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
}

// ── 线上协议（单一真相源：quirel-protocol crate）──────────
// 此前此处有一份与 agent 侧协议镜像的手写定义（Envelope/Payload 全部变体/
// SubscriptionType/FileDiff/DiffType/FileEntry/MetricsSnapshot/DiskInfo/MountInfo/
// 统计快照/StatsResponse 及 Envelope::new/encode/decode 实现），已收拢至
// 共享 crate，消除双份维护。既有引用路径不变（crate::connection::Payload 等照旧可用）。
// 与 quirel-protocol 的 lib.rs 导出面保持对齐；其中部分嵌套类型
// （DiffType/DiskInfo/各统计快照）客户端代码暂未直接按名引用，故 allow。
#[allow(unused_imports)]
pub use quirel_protocol::{
    Envelope, Payload, SubscriptionType,
    FileDiff, DiffType, FileEntry, MetricsSnapshot, DiskInfo, MountInfo,
    AuthStatsSnapshot, ConnectionStatsSnapshot, PerformanceStatsSnapshot,
    ResponseTimePercentiles, StatsResponse,
    AuthErrorCode,
};

// ── 连接管理器 ───────────────────────────────────────

pub struct ConnectionManager {
    pub connections: Mutex<HashMap<String, ActiveConnection>>,
    request_counter: Mutex<u32>,
}

pub struct ActiveConnection {
    #[allow(dead_code)]
    info: ConnectionInfo,
    tx: mpsc::Sender<ClientRequest>,
    // QUIC Connection（用于创建持久 Stream）
    pub quic_conn: Option<Arc<quinn::Connection>>,
    // 持久 Stream 监听任务
    subscription_task: Option<tokio::task::JoinHandle<()>>,
    // 用户主动断开标志：remote_disconnect 置位（先于 close，避免竞态误判），
    // 统一清理块据此区分 connection-lost 事件来源，前端仅对网络断开自动重连
    pub user_initiated: Arc<AtomicBool>,
    // Agent 能力声明（认证响应携带；旧 Agent 为空列表）
    pub capabilities: Vec<String>,
}

/// 连接丢失的触发源（用于日志区分）
#[derive(Debug, Clone)]
pub enum ConnectionLostSource {
    /// 心跳 Ping 超时
    Heartbeat,
    /// QUIC 连接关闭（idle_timeout / 对端 close）
    QuicClosed,
    /// 业务请求发送失败
    SendFailed,
}

impl std::fmt::Display for ConnectionLostSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionLostSource::Heartbeat => write!(f, "心跳超时"),
            ConnectionLostSource::QuicClosed => write!(f, "QUIC 连接关闭"),
            ConnectionLostSource::SendFailed => write!(f, "发送失败"),
        }
    }
}

/// 连接失败的结构化错误（remote_connect 的错误通道）
///
/// code：分类（必填）；detail：用户可读上下文（如主机名/操作指引），
/// 不含实现细节。完整技术细节只进 tracing 日志（{:?} 记录源错误）。
/// 前端按 code 查映射表得到标题/消息/行动建议/retryable。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectError {
    /// 数值形式序列化（前端 parseConnectError 契约：{"code":201,...}）；
    /// 协议线上 AuthResponse.code 仍为变体名字符串，两处通道互不影响
    #[serde(serialize_with = "serialize_code_as_i32")]
    pub code: AuthErrorCode,
    pub detail: Option<String>,
}

/// ConnectError.code 的数值序列化（AuthErrorCode 默认序列化为变体名字符串）
fn serialize_code_as_i32<S: serde::Serializer>(code: &AuthErrorCode, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_i32(code.as_i32())
}

impl ConnectError {
    fn new(code: AuthErrorCode) -> Self {
        Self { code, detail: None }
    }

    fn with_detail(code: AuthErrorCode, detail: impl Into<String>) -> Self {
        Self { code, detail: Some(detail.into()) }
    }
}

/// 旧版 Agent（无结构化 code）的错误文案分类。
/// 覆盖 Agent 侧已知中文文案 + 原有 4 关键词白名单语义；
/// 无法识别 → Unknown。
fn classify_legacy_error(msg: &str) -> AuthErrorCode {
    if msg.contains("请求过于频繁") {
        AuthErrorCode::RateLimited
    } else if msg.contains("锁定") {
        AuthErrorCode::AccountLocked
    } else if msg.contains("用户名或密码错误") {
        AuthErrorCode::InvalidCredentials
    } else if msg.contains("公钥未授权") {
        AuthErrorCode::PubkeyNotAuthorized
    } else if msg.contains("签名验证失败") || msg.contains("公钥验证失败") {
        AuthErrorCode::SignatureVerificationFailed
    } else if msg.contains("认证服务暂时不可用") {
        AuthErrorCode::AuthServiceUnavailable
    } else if msg.contains("超时") || msg.contains("timeout") || msg.contains("timed out") {
        AuthErrorCode::StreamTimeout
    } else if msg.contains("QUIC 连接失败") || msg.contains("认证请求失败") {
        AuthErrorCode::ConnectTimeout
    } else {
        AuthErrorCode::Unknown
    }
}

enum ClientRequest {
    Send {
        envelope: Envelope,
        response_tx: tokio::sync::oneshot::Sender<Result<Vec<u8>, String>>,
    },
    Disconnect,
    /// 被动检测到连接丢失（由心跳/status/发送失败触发）
    /// 仅用于通知主循环退出，不携带响应通道
    ConnectionLost {
        source: ConnectionLostSource,
    },
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Mutex::new(HashMap::new()),
            request_counter: Mutex::new(0),
        }
    }

    pub fn next_request_id(&self) -> u32 {
        let mut counter = self.request_counter.lock().unwrap();
        *counter += 1;
        *counter
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tauri Commands ─────────────────────────────────

#[tauri::command]
#[tracing::instrument(skip(credentials, app), fields(server_id = %server_id, host = %host, port = port))]
pub async fn remote_connect(
    server_id: String,
    host: String,
    port: u16,
    credentials: Option<Credentials>,
    cert_fingerprint: Option<String>,
    app: tauri::AppHandle,
) -> Result<ConnectionInfo, ConnectError> {
    let manager = app.state::<ConnectionManager>();

    // 安装 CryptoProvider
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 先尝试 QUIC
    let quic_result = try_quic_connect(&host, port).await;

    if let Ok((conn, rtt, server_cert_fingerprint)) = quic_result {
        tracing::info!("QUIC 连接成功: {}:{} (RTT={}ms)", host, port, rtt);
        let info = ConnectionInfo {
            server_id: server_id.clone(),
            host: host.clone(),
            port,
            transport_type: "quic".into(),
            status: "connected".into(),
            rtt_ms: rtt,
            connected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };

        // ── 证书钉扎校验（SSH known_hosts 模式） ──────────
        // 1. 已知指纹且匹配 → 跳过确认
        // 2. 首次连接（无已知指纹）→ 展示指纹，用户确认后存储
        // 3. 指纹不匹配 → 警告用户（可能服务器重装或 MITM），由用户决定
        let need_confirm = match &cert_fingerprint {
            None => true,                                   // 首次连接
            Some(known) if known == &server_cert_fingerprint => false,  // 指纹匹配
            Some(_) => true,                                // 指纹不匹配
        };

        if need_confirm {
            let is_first = cert_fingerprint.is_none();
            let fingerprint_display = PinningCertVerifier::format_fingerprint(&server_cert_fingerprint);

            let title = if is_first {
                "首次连接 - 确认服务器证书"
            } else {
                "⚠️ 服务器证书已变更"
            };
            let message = if is_first {
                format!(
                    "这是首次连接到该服务器。\n\n服务器证书指纹 (SHA-256):\n{}\n\n请确认您信任此服务器。",
                    fingerprint_display
                )
            } else {
                format!(
                    "⚠️ 警告：服务器证书与之前记录的不一致！\n可能是服务器重装或存在中间人攻击风险。\n\n新证书指纹 (SHA-256):\n{}\n\n是否信任新证书？",
                    fingerprint_display
                )
            };

            // 使用原生对话框（在独立线程上阻塞，避免阻塞 tokio 运行时）
            let app_clone = app.clone();
            let title_clone = title.to_string();
            let message_clone = message.clone();
            let kind = if is_first {
                tauri_plugin_dialog::MessageDialogKind::Info
            } else {
                tauri_plugin_dialog::MessageDialogKind::Warning
            };
            let accepted = tokio::task::spawn_blocking(move || {
                app_clone.dialog()
                    .message(message_clone)
                    .title(title_clone)
                    .kind(kind)
                    .buttons(tauri_plugin_dialog::MessageDialogButtons::YesNo)
                    .blocking_show()
            }).await.unwrap_or(false);

            if !accepted {
                conn.close(0u32.into(), b"cert rejected");
                return Err(ConnectError::new(AuthErrorCode::CertificateRejected));
            }

            // 通知前端存储证书指纹
            app.emit("cert-trusted", serde_json::json!({
                "server_id": &server_id,
                "fingerprint": &server_cert_fingerprint,
            })).map_err(|e| {
                tracing::warn!("[TLS] 发送证书信任事件失败: {}", e);
                ConnectError::new(AuthErrorCode::Unknown)
            })?;

            tracing::info!("[TLS] 证书已信任并存储: server_id={}", server_id);
        }

        // 认证
        let creds = credentials.ok_or_else(|| ConnectError::new(AuthErrorCode::MissingCredentials))?;

        // Agent 能力声明（两条认证路径填充；旧 Agent 为空列表，用于门控新协议命令）
        let agent_capabilities: Vec<String>;

        // 根据 method 执行不同的认证流程
        match creds.method {
            AuthMethod::Password => {
                // 密码认证流程（单步）
                let password = creds.password.ok_or_else(|| ConnectError::new(AuthErrorCode::MissingCredentials))?;
                let auth_payload = Payload::AuthPasswordRequest {
                    username: creds.username.clone(),
                    password,
                };

                // 发送认证请求（增加错误处理）
                let resp = send_and_receive_quic(&conn, manager.next_request_id(), auth_payload).await
                    .map_err(|e| {
                        tracing::warn!("[Connection] 认证请求失败: {:?}", e);
                        // 传输层错误：超时类与断连类区分
                        if e.contains("超时") || e.contains("timeout") {
                            ConnectError::new(AuthErrorCode::StreamTimeout)
                        } else {
                            ConnectError::new(AuthErrorCode::ConnectionLost)
                        }
                    })?;

                let envelope = Envelope::decode(&resp)
                    .map_err(|e| {
                        tracing::warn!("[Connection] 解析认证响应失败: {:?}", e);
                        ConnectError::new(AuthErrorCode::ProtocolError)
                    })?;

                // 验证返回类型（增加类型检查）
                match envelope.payload {
                    Payload::AuthResponse { success, error, session_id: _, code, capabilities } => {
                        if !success {
                            // 优先结构化 code；旧版 Agent 按文案 fallback 分类
                            let err_code = code
                                .unwrap_or_else(|| classify_legacy_error(error.as_deref().unwrap_or("")));
                            // 关闭连接（Agent 确认拒绝，close reason 如实标注）
                            conn.close(0u32.into(), b"authentication failed");
                            return Err(ConnectError::with_detail(err_code, error.unwrap_or_else(|| "认证失败".to_string())));
                        }
                        // 认证成功：捕获能力声明（旧 Agent 无此字段 → 空列表）
                        agent_capabilities = capabilities.unwrap_or_default();
                    }
                    other => {
                        conn.close(0u32.into(), b"unexpected response");
                        tracing::warn!("[Connection] 期望 AuthResponse，收到: {:?}", other);
                        return Err(ConnectError::new(AuthErrorCode::ProtocolError));
                    }
                }
            }
            AuthMethod::PubKey => {
                // 公钥认证流程（多步挑战-响应）
                let private_key = creds.private_key.ok_or_else(|| ConnectError::new(AuthErrorCode::MissingCredentials))?;

                tracing::info!("[Connection] 开始公钥认证: username={}", creds.username);

                // 执行公钥认证
                let (session_id, pk_capabilities) = perform_pubkey_auth(
                    &conn,
                    creds.username.clone(),
                    private_key,
                    creds.passphrase,
                ).await.map_err(|e| {
                    tracing::error!("[Connection] 公钥认证失败: {:?}", e);
                    // close reason 真实化：网络类（100-199）与 Agent 确认拒绝区分，
                    // 避免服务端日志将网络超时误判为认证失败
                    let reason: &'static [u8] = if (100..=199).contains(&e.code.as_i32()) {
                        b"auth network timeout"
                    } else {
                        b"authentication failed"
                    };
                    conn.close(0u32.into(), reason);
                    e
                })?;
                agent_capabilities = pk_capabilities;

                tracing::info!("[Connection] 公钥认证成功: username={}, session_id={:?}", creds.username, session_id);
            }
        }

        // 启动消息循环（含心跳和连接状态监听）
        let (tx, mut rx) = mpsc::channel::<ClientRequest>(32);
        // 用户主动断开标志（与 ActiveConnection 共享，见结构体注释）
        let user_initiated = Arc::new(AtomicBool::new(false));
        {
            let mut conns = manager.connections.lock().unwrap();
            conns.insert(server_id.clone(), ActiveConnection {
                info: info.clone(),
                tx: tx.clone(),
                quic_conn: Some(Arc::new(conn.clone())),
                subscription_task: None,
                user_initiated: user_initiated.clone(),
                capabilities: agent_capabilities,
            });
        }

        let server_id_clone = server_id.clone();
        let app_handle = app.clone();
        let conn_clone = conn.clone();
        // 主循环内捕获 user_initiated 标志，统一清理块据此分类事件来源
        let user_initiated_clone = user_initiated.clone();

        tokio::spawn(async move {
            // ── 心跳任务（Watchdog）：快速检测连接断开 ────────
            // 业界标准（TeamViewer/AnyDesk 级别）：
            //   - 间隔 5s：每 5 秒发一次 Ping 探测连接活性
            //   - 超时 5s：等待 Pong 响应的最长时间
            //   - 最坏延迟：5s(间隔) + 5s(超时) = **10s**
            //   - 最佳延迟：conn.closed() 瞬间触发
            //
            // 独立通道设计：
            //   - 心跳直接用 conn.clone() 调用 send_and_receive_quic
            //   - 不走主循环队列，避免被耗时业务请求阻塞
            //   - Quinn 支持同一连接上多个并发 stream，互不干扰
            //
            // 只通知不清理：
            //   - 检测到断连后只发 ConnectionLost 给主循环
            //   - 不执行 cleanup / emit，由主循环统一清理
            let heartbeat_tx = tx.clone();
            let heartbeat_server_id = server_id_clone.clone();
            let heartbeat_conn = conn_clone.clone();
            let heartbeat_app = app_handle.clone();  // RTT 回传事件用（每 5s emit 一次）
            let heartbeat_task = tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
                let mut last_tick = std::time::Instant::now();
                loop {
                    interval.tick().await;

                    // 检测时间跳跃（电脑休眠/唤醒）：两次 tick 间隔远超设定值
                    let tick_gap = last_tick.elapsed();
                    last_tick = std::time::Instant::now();
                    if tick_gap > std::time::Duration::from_secs(15) {
                        tracing::warn!(
                            "检测到时间跳跃 ({}s)，可能从休眠唤醒，主动检测连接状态",
                            tick_gap.as_secs()
                        );
                        // 休眠唤醒后连接可能已失效，用短超时快速检测
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        let envelope = Envelope::new(0, Payload::Ping { timestamp: now });
                        let alive = matches!(
                            tokio::time::timeout(
                                std::time::Duration::from_secs(3),
                                send_and_receive_quic(&heartbeat_conn, 0, envelope.payload),
                            ).await,
                            Ok(Ok(_))
                        );
                        // 跨休眠的 QUIC 连接一律主动重建，即使小包 Ping 成功：
                        // - 唤醒后网络路径状态已变（防火墙重载/网卡重置/MTU 变化），
                        //   控制面小包通过不代表数据面健康（MTU 黑洞：小包通、大包丢）
                        // - 休眠前挂起的旧代理流传输状态（拥塞窗口/重传队列）不可信
                        // - 若不重建：连接显示"存活"但代理持续失败，且心跳 Ping
                        //   一直成功导致连接永不判死、永不自愈
                        // 走 ConnectionLost → 前端自动重连 → 新连接（状态干净）
                        if alive {
                            tracing::info!("休眠唤醒后连接仍可响应，但传输状态不可信，主动重建");
                        } else {
                            tracing::warn!("休眠唤醒后连接已失效，触发断开");
                        }
                        let _ = heartbeat_tx.send(ClientRequest::ConnectionLost {
                            source: ConnectionLostSource::Heartbeat,
                        }).await;
                        break;
                    }

                    // 快速退出：连接已关闭
                    if heartbeat_conn.close_reason().is_some() {
                        break;
                    }

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let envelope = Envelope::new(0, Payload::Ping { timestamp: now });

                    // 独立通道：直接调用 send_and_receive_quic，不走主循环队列
                    // ping_start：RTT 测量起点（Instant 单调时钟，不受系统时间跳变影响）
                    let ping_start = std::time::Instant::now();
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        send_and_receive_quic(&heartbeat_conn, 0, envelope.payload),
                    ).await {
                        Ok(Ok(_)) => {
                            // Pong 正常收到，连接存活；
                            // 顺便回传真实 RTT（Settings 延迟显示，免额外的 ping 请求）
                            let _ = heartbeat_app.emit("server_rtt", serde_json::json!({
                                "server_id": heartbeat_server_id,
                                "rtt_ms": ping_start.elapsed().as_millis() as u64,
                            }));
                        }
                        Ok(Err(e)) => {
                            tracing::warn!("心跳发送失败，连接可能已断开: {}", e);
                            let _ = heartbeat_tx.send(ClientRequest::ConnectionLost {
                                source: ConnectionLostSource::Heartbeat,
                            }).await;
                            break;
                        }
                        Err(_) => {
                            tracing::warn!("Ping 超时 (5s)，连接无响应: {}", heartbeat_server_id);
                            let _ = heartbeat_tx.send(ClientRequest::ConnectionLost {
                                source: ConnectionLostSource::Heartbeat,
                            }).await;
                            break;
                        }
                    }
                }
                // 心跳任务结束：不执行 cleanup / emit，由主循环统一清理
            });
            
            // 连接状态监听任务
            // 只通知主循环，不执行 cleanup / emit
            // 竞态安全：心跳和 status 可能同时检测到断连，都发 ConnectionLost。
            // 主循环处理第一个后 break，第二个消息随 channel drop 自动丢弃。
            let status_conn = conn_clone.clone();
            let status_tx = tx.clone();
            let status_server_id = server_id_clone.clone();
            let status_task = tokio::spawn(async move {
                status_conn.closed().await;
                // 记录关闭原因（区分：Agent 主动关闭如 idle/session 超时与重启、
                // 网络中断超时、本地关闭等；缺此信息时 quic_closed 事件无法归因，
                // 如 2026-09-05 15:19 的断开原因至今无法从日志确认）
                let reason = status_conn
                    .close_reason()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "未知".to_string());
                tracing::warn!("QUIC 连接已关闭: {}, 原因: {}", status_server_id, reason);
                let _ = status_tx.send(ClientRequest::ConnectionLost {
                    source: ConnectionLostSource::QuicClosed,
                }).await;
            });
            
            // 主循环需要 tx clone 用于 SendFailed 时通知自己
            let main_tx = tx.clone();

            // 连接丢失来源（用于 connection-lost 事件分类）
            // - Some(source)：心跳/status/发送失败明确触发
            // - None：Send 分支检测到 close_reason 直接 break，或通道关闭自然退出
            //   （此时由 user_initiated 标志区分用户主动/网络断开）
            let mut lost_source: Option<ConnectionLostSource> = None;

            // 主消息循环
            while let Some(req) = rx.recv().await {
                match req {
                    ClientRequest::Send { envelope, response_tx } => {
                        // 检查连接状态
                        if conn_clone.close_reason().is_some() {
                            let _ = response_tx.send(Err("连接已关闭".into()));
                            break;
                        }
                        let result = send_and_receive_quic(&conn_clone, envelope.request_id, envelope.payload).await;
                        if result.is_err() {
                            let _ = response_tx.send(Err("连接已断开".into()));
                            // 通过 ConnectionLost 通知自己退出，清理逻辑只有一个入口
                            let _ = main_tx.send(ClientRequest::ConnectionLost {
                                source: ConnectionLostSource::SendFailed,
                            }).await;
                            continue;
                        }
                        let _ = response_tx.send(result);
                    }
                    ClientRequest::Disconnect => break,
                    ClientRequest::ConnectionLost { source } => {
                        tracing::info!("连接丢失（{}）: {}", source, server_id_clone);
                        lost_source = Some(source);
                        break;
                    }
                }
            }

            // ════════════════════════════════════════════════════════════
            // 统一清理块（唯一清理入口）
            // 退出原因：Disconnect / ConnectionLost / 主循环自然结束
            // 所有清理集中在此处，保证只执行一次
            // ════════════════════════════════════════════════════════════

            // 1. 停止检测任务（防止心跳/status 在清理后还发 ConnectionLost）
            heartbeat_task.abort();
            status_task.abort();

            // 2. 显式关闭 QUIC 连接（幂等：已关闭则无操作）
            //    主动断开：确保连接立即关闭，释放资源
            //    被动断开：close_reason() 已有值，close() 是无操作
            if conn_clone.close_reason().is_none() {
                conn_clone.close(0u32.into(), b"client closing");
                tracing::info!("QUIC 连接已主动关闭: {}", server_id_clone);
            }

            // 3. 清理传输任务（取消该连接所有未完成的传输）
            let tm = app_handle.state::<std::sync::Arc<crate::transfer::TransferManager>>();
            tm.cleanup_by_connection(&server_id_clone).await;

            // 4. 挂起代理会话（仅停 SOCKS5 监听，浏览器保留——固定端口下
            //    重连后自动恢复，无需重开浏览器；彻底清理在关浏览窗口/App 退出时）
            crate::proxy::suspend_session(&app_handle, &server_id_clone);

            // 5. 从 ConnectionManager 移除连接条目
            //    同时 abort subscription_task，避免被动断开时的残留读取日志
            if let Ok(mut conns) = app_handle.state::<ConnectionManager>().connections.lock() {
                if let Some(active_conn) = conns.remove(&server_id_clone) {
                    if let Some(task) = active_conn.subscription_task {
                        task.abort();
                    }
                }
            }

            // 6. 通知前端（只 emit 一次）
            //    事件携带来源：前端仅对网络断开（heartbeat/quic_closed/send_failed）
            //    触发自动重连，用户主动断开（user_initiated）不重连
            let source_str = if user_initiated_clone.load(Ordering::SeqCst) {
                "user_initiated"
            } else {
                match lost_source {
                    Some(ConnectionLostSource::Heartbeat) => "heartbeat",
                    Some(ConnectionLostSource::QuicClosed) => "quic_closed",
                    Some(ConnectionLostSource::SendFailed) => "send_failed",
                    // Send 分支检测到 close_reason 直接 break / 通道关闭自然退出：
                    // 非用户主动断开，一律视为网络断开
                    None => "quic_closed",
                }
            };
            // 断连原因分类（close code 0x01 idle / 0x02 session；其余网络断）
            // 计算须在主动 close 之前已定型：close_reason() 为 None 时（本地主动关闭）
            // 不携带 code，前端按通用网络断开展示
            let lost_code = match conn_clone.close_reason() {
                Some(quinn::ConnectionError::ApplicationClosed(close)) => match close.error_code.into_inner() {
                    1 => Some(AuthErrorCode::StreamTimeout),
                    2 => Some(AuthErrorCode::SessionExpired),
                    _ => None,
                },
                Some(quinn::ConnectionError::TimedOut) => Some(AuthErrorCode::StreamTimeout),
                _ => None,
            };
            let _ = app_handle.emit("connection-lost", serde_json::json!({
                "server_id": &server_id_clone,
                "source": source_str,
                "code": lost_code.map(|c| c.as_i32()),
            }));

            tracing::info!("连接清理完成: {}", server_id_clone);
        });

        return Ok(info);
    }

    // QUIC 失败，返回结构化错误
    Err(quic_result.unwrap_err())
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_disconnect(server_id: String, app: tauri::AppHandle) -> Result<(), String> {
    tracing::info!("[Connection] 主动断开: {}", server_id);

    // 1. 获取连接句柄（quic_conn + tx + user_initiated 标志）
    let manager = app.state::<ConnectionManager>();
    let conn_info = {
        let conns = manager.connections.lock().unwrap();
        conns.get(&server_id).map(|c| (c.quic_conn.clone(), c.tx.clone(), c.user_initiated.clone()))
    };

    let (quic_conn, tx, user_initiated) = match conn_info {
        Some(v) => v,
        None => return Err("未找到该服务器的连接".into()),
    };

    // 2. 标记用户主动断开（先于 close 置位，避免主循环 Send 分支
    //    检测到 close_reason 提前退出时误判为网络断开，触发前端自动重连）
    user_initiated.store(true, Ordering::SeqCst);

    // 3. 发送 DisconnectRequest（fire-and-forget，不等待响应）
    //    Agent 收到后清理传输会话等资源；收不到则靠 conn.closed() 兜底
    let request_id = manager.next_request_id();
    let envelope = Envelope::new(request_id, Payload::DisconnectRequest {});
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let _ = tx.try_send(ClientRequest::Send { envelope, response_tx });
    // 不等待 response_rx —— 立即关闭连接

    // 4. 立即关闭 QUIC 连接
    //    主循环会在下次 send 时检测到 close_reason，或 status_task 的
    //    conn.closed() 触发，任一都会通过统一清理块完成清理
    if let Some(conn) = quic_conn {
        conn.close(0u32.into(), b"client disconnect");
    }

    // 5. 发送 Disconnect 通知主循环退出（触发统一清理块）
    let _ = tx.send(ClientRequest::Disconnect).await;

    tracing::info!("[Connection] 主动断开完成: {}", server_id);
    Ok(())
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_ping(server_id: String, app: tauri::AppHandle) -> Result<PingResult, String> {
    tracing::debug!("[Ping] server_id={}", server_id);
    let manager = app.state::<ConnectionManager>();
    
    let (tx, request_id) = {
        let conns = manager.connections.lock().unwrap();
        let conn = conns.get(&server_id).ok_or("未找到连接")?;
        (conn.tx.clone(), manager.next_request_id())
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let envelope = Envelope::new(request_id, Payload::Ping { timestamp: now });
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    
    tx.send(ClientRequest::Send { envelope, response_tx }).await.map_err(|_| "发送请求失败")?;
    let data = response_rx.await.map_err(|_| "等待响应超时")??;

    let resp = Envelope::decode(&data)?;
    if let Payload::Pong { timestamp, server_time } = resp.payload {
        let latency = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64 - timestamp;
        Ok(PingResult { ok: true, latency_ms: latency as f64, server_time })
    } else {
        Err("收到意外的响应类型".into())
    }
}

#[derive(Debug, Serialize)]
pub struct PingResult {
    pub ok: bool,
    pub latency_ms: f64,
    pub server_time: u64,
}

#[tauri::command]
#[tracing::instrument(skip(payload, app), fields(server_id = %server_id))]
pub async fn remote_send(server_id: String, payload: Payload, app: tauri::AppHandle) -> Result<Envelope, String> {
    let manager = app.state::<ConnectionManager>();
    
    let (tx, request_id) = {
        let conns = manager.connections.lock().unwrap();
        let conn = conns.get(&server_id).ok_or_else(|| {
            tracing::warn!("[RemoteSend] 未找到连接: server_id={}", server_id);
            "未找到连接".to_string()
        })?;
        (conn.tx.clone(), manager.next_request_id())
    };

    let envelope = Envelope::new(request_id, payload);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    
    tx.send(ClientRequest::Send { envelope, response_tx }).await.map_err(|e| {
        tracing::warn!("[RemoteSend] 发送请求失败: server_id={}, error={}", server_id, e);
        "发送请求失败".to_string()
    })?;
    // 外层超时保护：如果内部 send_and_receive_quic 的超时未能触发，此层兜底
    let data = tokio::time::timeout(
        std::time::Duration::from_secs(STREAM_TIMEOUT_SECS + 5), // 比 Stream 超时多 5 秒作为缓冲
        response_rx,
    )
    .await
    .map_err(|_| {
        tracing::warn!("[RemoteSend] 等待响应超时: server_id={}", server_id);
        "等待响应超时".to_string()
    })?
    .map_err(|_| {
        tracing::warn!("[RemoteSend] 等待响应通道关闭: server_id={}", server_id);
        "等待响应超时".to_string()
    })??;
    Envelope::decode(&data)
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_read_dir(server_id: String, path: String, app: tauri::AppHandle) -> Result<RemoteReadDirResponse, String> {
    tracing::debug!("[ReadDir] server_id={}, path={}", server_id, path);
    let resp = remote_send(server_id, Payload::ReadDirRequest { path }, app).await?;
    match resp.payload {
        Payload::ReadDirResponse { path, entries } => Ok(RemoteReadDirResponse { path, entries }),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[derive(Debug, Serialize)]
pub struct RemoteReadDirResponse {
    pub path: String,
    pub entries: Vec<FileEntry>,
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_get_current_user(server_id: String, app: tauri::AppHandle) -> Result<CurrentUser, String> {
    tracing::debug!("[GetCurrentUser] server_id={}", server_id);
    // 诊断：此命令失败会静默导致前端 homeDir 为 null（文件管理器初始目录停留在 /），
    // 必须在 Rust 侧记录具体原因（解码失败/变体不符/Agent 错误）
    let resp = match remote_send(server_id, Payload::GetCurrentUser, app).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("[GetCurrentUser] 请求失败: error={}", e);
            return Err(e);
        }
    };
    match resp.payload {
        Payload::CurrentUserResponse { username, home_dir } => {
            tracing::debug!("[GetCurrentUser] 成功: username={}, home_dir={}", username, home_dir);
            Ok(CurrentUser { username, home_dir })
        }
        Payload::Error { message, .. } => {
            tracing::warn!("[GetCurrentUser] Agent 返回错误: message={}", message);
            Err(message)
        }
        other => {
            tracing::warn!("[GetCurrentUser] 意外响应类型: 实际类型={}", other.type_name());
            Err("意外响应".into())
        }
    }
}

/// 当前用户信息（包含真实家目录，root 用户为 /root，普通用户从 /etc/passwd 读取）
#[derive(Debug, Serialize)]
pub struct CurrentUser {
    pub username: String,
    pub home_dir: String,
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_get_mounts(server_id: String, app: tauri::AppHandle) -> Result<Vec<MountInfo>, String> {
    tracing::debug!("[GetMounts] server_id={}", server_id);
    let resp = remote_send(server_id, Payload::GetMounts, app).await?;
    match resp.payload {
        Payload::MountsResponse { mounts } => Ok(mounts),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_get_path_suggestions(server_id: String, path: String, app: tauri::AppHandle) -> Result<Vec<String>, String> {
    tracing::debug!("[GetPathSuggestions] server_id={}, path={}", server_id, path);
    let resp = remote_send(server_id, Payload::GetPathSuggestionsRequest { path }, app).await?;
    match resp.payload {
        Payload::PathSuggestionsResponse { suggestions } => Ok(suggestions),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_get_metrics(server_id: String, app: tauri::AppHandle) -> Result<MetricsSnapshot, String> {
    tracing::debug!("[GetMetrics] server_id={}", server_id);
    let resp = remote_send(server_id, Payload::MetricsSubscribeRequest {}, app).await?;
    match resp.payload {
        Payload::MetricsData(metrics) => Ok(metrics),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_read_file(server_id: String, path: String, app: tauri::AppHandle) -> Result<RemoteReadFileResponse, String> {
    tracing::debug!("[ReadFile] server_id={}, path={}", server_id, path);
    let resp = remote_send(server_id, Payload::ReadFileRequest { path }, app).await?;
    match resp.payload {
        Payload::ReadFileResponse { path, content, mtime, size } => {
            // Agent 返回 base64 编码的原始字节（支持二进制文件）
            // 客户端文本编辑器需要 UTF-8 文本，这里解码 base64 并尝试 UTF-8 转换
            use base64::Engine;
            let content_bytes = base64::engine::general_purpose::STANDARD
                .decode(content.as_bytes())
                .map_err(|e| format!("base64 解码失败: {}", e))?;
            // UTF-8 优先；失败时回退 GB18030（GBK 超集，覆盖中文服务器常见编码）
            // 注：二进制文件不会走到这里——FileOpener 先经 file_info 路由，is_text=false 不进编辑器
            let content_text = match String::from_utf8(content_bytes) {
                Ok(s) => s,
                Err(e) => {
                    let (decoded, _, had_errors) = encoding_rs::GB18030.decode(e.as_bytes());
                    if had_errors {
                        return Err(format!("文件不是有效的 UTF-8 文本（可能为二进制文件）: {}", e));
                    }
                    tracing::info!("[ReadFile] UTF-8 解码失败，已按 GB18030 回退解码");
                    decoded.into_owned()
                }
            };
            Ok(RemoteReadFileResponse { path, content: content_text, mtime, size })
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[derive(Debug, Serialize)]
pub struct RemoteReadFileResponse {
    pub path: String,
    pub content: String,
    pub mtime: u64, // 文件修改时间（Unix timestamp）
    pub size: u64,
}

/// 文件格式探测结果（camelCase 序列化，与前端风格对齐）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileInfo {
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub is_text: bool,
    pub extension: String,
    /// 文件头部字节（JSON 数组传输，前端用于 magic number 检测）
    pub magic_bytes: Vec<u8>,
}

/// 文件格式探测（双击文件时的格式路由依据）
#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_file_info(server_id: String, path: String, app: tauri::AppHandle) -> Result<RemoteFileInfo, String> {
    tracing::debug!("[FileInfo] server_id={}, path={}", server_id, path);
    // 传输失败加 [transport] 前缀：前端据此区分「连接故障」（不降级，直接报错）
    // 与「Agent 端错误」（旧 Agent 无此命令/文件级失败 → 降级回退编辑器）。
    // 前缀为机器可识别标记，语义一旦发布不可变更。
    let resp = match remote_send(server_id, Payload::FileInfoRequest { path }, app).await {
        Ok(resp) => resp,
        Err(e) => return Err(format!("[transport] {e}")),
    };
    match resp.payload {
        Payload::FileInfoResponse { path, size, is_dir, is_text, extension, magic_bytes } => {
            Ok(RemoteFileInfo { path, size, is_dir, is_text, extension, magic_bytes })
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

/// 二进制文件读取响应（base64 传输，图片/PDF/十六进制查看器使用）
#[derive(Debug, Serialize)]
pub struct RemoteBinaryFile {
    pub path: String,
    /// 文件原始字节的 base64 编码（前端 atob 解码）
    pub base64: String,
    pub mtime: u64,
    pub size: u64,
}

/// 读取二进制文件（图片/PDF/十六进制视图）
///
/// 与 remote_read_file 的区别：不做 UTF-8 转换，直接透传 base64，
/// 前端按用途解码（data URI / pdfjs Uint8Array / hex dump）。
#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_read_file_binary(server_id: String, path: String, app: tauri::AppHandle) -> Result<RemoteBinaryFile, String> {
    tracing::debug!("[ReadFileBinary] server_id={}, path={}", server_id, path);
    let resp = remote_send(server_id, Payload::ReadFileRequest { path }, app).await?;
    match resp.payload {
        Payload::ReadFileResponse { path, content, mtime, size } => {
            Ok(RemoteBinaryFile { path, base64: content, mtime, size })
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

// ── 远程命令执行（解压等文件操作）──────────────────────────

/// 远程命令执行结果（stdout/stderr 已转文本：UTF-8 优先，GB18030 回退）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// 执行服务器白名单命令（unzip/tar/7z/unrar；Worker 端强制白名单）
///
/// 解压等文件操作走此通道：argv 直执行不经 shell，路径无需转义。
/// 超时独立于 remote_send 的 30s Stream 超时（大压缩包解压耗时长），
/// 故不直接复用 remote_send，而是内联其发送模式 + 自定义外层超时。
#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn remote_execute_command(
    server_id: String,
    command: String,
    args: Vec<String>,
    working_directory: Option<String>,
    timeout_secs: Option<u32>,
    app: tauri::AppHandle,
) -> Result<RemoteCommandOutput, String> {
    use base64::Engine;
    tracing::info!("[ExecuteCommand] server_id={}, command={}, args={:?}", server_id, command, args);

    // 外层超时兜底：比命令自身 timeout 多 30s（Agent 端负责 kill 命令进程并回错误）
    let cmd_timeout = timeout_secs.unwrap_or(600);
    let outer_timeout = std::time::Duration::from_secs(cmd_timeout as u64 + 30);

    let manager = app.state::<ConnectionManager>();
    let (tx, request_id) = {
        let conns = manager.connections.lock().unwrap();
        let conn = conns.get(&server_id).ok_or("未找到连接")?;
        (conn.tx.clone(), manager.next_request_id())
    };

    let envelope = Envelope::new(request_id, Payload::ExecuteCommandRequest {
        command,
        args,
        working_directory,
        timeout_secs: cmd_timeout,
    });
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    tx.send(ClientRequest::Send { envelope, response_tx }).await
        .map_err(|_| "发送请求失败".to_string())?;

    let data = tokio::time::timeout(outer_timeout, response_rx)
        .await
        .map_err(|_| {
            tracing::warn!("[ExecuteCommand] 命令执行超时: server_id={}", server_id);
            "命令执行超时".to_string()
        })?
        .map_err(|_| "等待响应通道关闭".to_string())??;

    let resp = Envelope::decode(&data)?;
    match resp.payload {
        Payload::CommandOutputResponse { stdout, stderr, exit_code } => {
            // base64 → 字节 → UTF-8 优先，GB18030 回退（服务器命令输出可能是中文 GBK）
            let decode_text = |b64: String| -> String {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64.as_bytes())
                    .unwrap_or_default();
                match String::from_utf8(bytes) {
                    Ok(s) => s,
                    Err(e) => {
                        let (decoded, _, _) = encoding_rs::GB18030.decode(e.as_bytes());
                        decoded.into_owned()
                    }
                }
            };
            Ok(RemoteCommandOutput {
                stdout: decode_text(stdout),
                stderr: decode_text(stderr),
                exit_code,
            })
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

/// 下载远程文件到本地临时目录并用系统默认应用打开（HTML → 浏览器）
///
/// 数据流：ReadFileRequest（base64 全量）→ %TEMP%/quirel-view/<文件名> → opener
/// 临时文件不主动清理（系统 temp 自然回收）
#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, remote_path = %remote_path))]
pub async fn remote_open_locally(
    server_id: String,
    remote_path: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    use base64::Engine;
    use tauri_plugin_opener::OpenerExt;
    tracing::info!("[OpenLocally] server_id={}, path={}", server_id, remote_path);

    // 1. 读取远程文件（base64 全量；SIZE_LIMITS.browserLocal 20MB 确认在前端完成）
    let resp = remote_send(server_id, Payload::ReadFileRequest { path: remote_path.clone() }, app.clone()).await?;
    let base64_content = match resp.payload {
        Payload::ReadFileResponse { content, .. } => content,
        Payload::Error { message, .. } => return Err(message),
        _ => return Err("意外响应".into()),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_content.as_bytes())
        .map_err(|e| format!("base64 解码失败: {}", e))?;

    // 2. 写入本地临时目录（%TEMP%/quirel-view/，不存在则创建，同名覆盖）
    let file_name = remote_path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("download.html");
    let dir = std::env::temp_dir().join("quirel-view");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败: {}", e))?;
    let local_path = dir.join(file_name);
    std::fs::write(&local_path, &bytes).map_err(|e| format!("写入临时文件失败: {}", e))?;

    // 3. 系统默认应用打开（HTML 的默认应用即浏览器）
    app.opener()
        .open_path(local_path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| format!("调用本地应用失败: {}", e))?;

    Ok(local_path.to_string_lossy().to_string())
}

#[tauri::command]
#[tracing::instrument(skip(content, app), fields(server_id = %server_id, path = %path))]
pub async fn remote_write_file(server_id: String, path: String, content: String, app: tauri::AppHandle) -> Result<RemoteWriteFileResponse, String> {
    tracing::info!("[WriteFile] server_id={}, path={}", server_id, path);
    // Agent 协议要求 content 为 base64 编码（支持二进制文件）
    // 前端传入的是 UTF-8 文本，这里编码为 base64
    use base64::Engine;
    let content_b64 = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
    let resp = remote_send(server_id, Payload::WriteFileRequest { path, content: content_b64 }, app).await?;
    match resp.payload {
        Payload::WriteFileResponse { path, mtime, size } => Ok(RemoteWriteFileResponse { path, mtime, size }),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[derive(Debug, Serialize)]
pub struct RemoteWriteFileResponse {
    pub path: String,
    pub mtime: u64, // 文件修改时间（写入后的新 mtime）
    pub size: u64,
}

#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id, path = %path))]
pub async fn remote_delete(server_id: String, path: String, app: tauri::AppHandle) -> Result<bool, String> {
    tracing::info!("[DeleteFile] server_id={}, path={}", server_id, path);
    let resp = remote_send(server_id, Payload::DeleteRequest { path }, app).await?;
    match resp.payload {
        Payload::DeleteResponse { success } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_mkdir(server_id: String, path: String, app: tauri::AppHandle) -> Result<bool, String> {
    tracing::debug!("[Connection] remote_mkdir: server_id={}, path={}", server_id, path);

    let resp = remote_send(server_id, Payload::MkdirRequest { path }, app).await?;

    match resp.payload {
        Payload::MkdirResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_rename(
    server_id: String,
    old_path: String,
    new_path: String,
    app: tauri::AppHandle
) -> Result<bool, String> {
    tracing::debug!("[Connection] remote_rename: old={}, new={}", old_path, new_path);

    let resp = remote_send(server_id, Payload::RenameRequest { old_path, new_path }, app).await?;

    match resp.payload {
        Payload::RenameResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn remote_copy(
    server_id: String,
    src: String,
    dst: String,
    app: tauri::AppHandle
) -> Result<bool, String> {
    tracing::debug!("[Connection] remote_copy: src={}, dst={}", src, dst);

    let resp = remote_send(server_id, Payload::CopyRequest { src, dst }, app).await?;

    match resp.payload {
        Payload::CopyResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

/// 修改远程文件/目录权限（chmod）
/// - `mode`: 八进制数值（如 755）
/// - `recursive`: 目录场景下递归应用到子文件与子目录
#[tauri::command]
pub async fn remote_chmod(
    server_id: String,
    path: String,
    mode: u32,
    recursive: bool,
    app: tauri::AppHandle
) -> Result<bool, String> {
    tracing::debug!("[Connection] remote_chmod: server_id={}, path={}, mode={:o}, recursive={}", server_id, path, mode, recursive);

    let resp = remote_send(server_id, Payload::ChmodRequest { path, mode, recursive }, app).await?;

    match resp.payload {
        Payload::ChmodResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

/// 修改远程文件/目录所有者（chown）
/// - `owner`/`group`: 目标用户名/组名
/// - `recursive`: 目录场景下递归应用到子文件与子目录
#[tauri::command]
pub async fn remote_chown(
    server_id: String,
    path: String,
    owner: String,
    group: String,
    recursive: bool,
    app: tauri::AppHandle
) -> Result<bool, String> {
    tracing::debug!("[Connection] remote_chown: server_id={}, path={}, owner={}, group={}, recursive={}", server_id, path, owner, group, recursive);

    let resp = remote_send(server_id, Payload::ChownRequest { path, owner, group, recursive }, app).await?;

    match resp.payload {
        Payload::ChownResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

/// 应用差异到远程文件（流量优化）
///
/// # 参数
/// - `server_id`: 服务器 ID
/// - `path`: 文件路径
/// - `base_mtime`: 基准 mtime（客户端缓存的版本）
/// - `diffs`: 差异列表（客户端计算）
///
/// # 返回
/// 应用差异后的响应（新的 mtime）
#[tauri::command]
pub async fn remote_apply_diff(
    server_id: String,
    path: String,
    base_mtime: u64,
    diffs: Vec<FileDiff>, // 使用本地定义的 FileDiff
    app: tauri::AppHandle
) -> Result<RemoteApplyDiffResponse, String> {
    tracing::debug!("[Connection] remote_apply_diff: server_id={}, path={}, base_mtime={}, diffs={}", 
        server_id, path, base_mtime, diffs.len());

    // 发送差异到 Agent
    let resp = remote_send(
        server_id,
        Payload::ApplyDiffRequest {
            path,
            base_mtime,
            diffs,
        },
        app
    ).await?;

    // 处理响应
    match resp.payload {
        Payload::ApplyDiffResponse { path, success, new_mtime, error } => {
            Ok(RemoteApplyDiffResponse {
                path,
                success,
                new_mtime,
                error,
            })
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

/// 应用差异响应结构体
#[derive(Debug, serde::Serialize)]
pub struct RemoteApplyDiffResponse {
    pub path: String,
    pub success: bool,
    pub new_mtime: u64,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn remote_move(
    server_id: String,
    src: String,
    dst: String,
    app: tauri::AppHandle
) -> Result<bool, String> {
    tracing::debug!("[Connection] remote_move: src={}, dst={}", src, dst);

    let resp = remote_send(server_id, Payload::MoveRequest { src, dst }, app).await?;

    match resp.payload {
        Payload::MoveResponse { success, .. } => Ok(success),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

#[tauri::command]
pub async fn get_stats(server_id: String, stats_type: String, app: tauri::AppHandle) -> Result<StatsResponse, String> {
    tracing::debug!("[Connection] get_stats: server_id={}, stats_type={}", server_id, stats_type);

    let resp = remote_send(server_id, Payload::GetStats { stats_type }, app).await?;

    match resp.payload {
        // 共享 crate 的 Payload::StatsResponse 为 newtype 变体（内含 StatsResponse），
        // 序列化字节与旧内联字段变体完全一致，直接透传
        Payload::StatsResponse(stats) => Ok(stats),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

// ── 上传大小限制（设置页「文件→传输设置」）─────────────────────────
//
// 值的单一事实来源在 Agent 端，客户端不持久化（每次进入设置页查询）。
// 前端必须先经 get_agent_capabilities 确认支持，再调用本组命令：
// 旧 Agent 无法解码新 payload，直接发送会断流，主循环会把整条连接误判为断开。

/// Tauri Command: 查询当前连接 Agent 的能力列表
///
/// 空列表 = 旧 Agent（不支持新协议命令）；未连接返回 Err
#[tauri::command]
pub fn get_agent_capabilities(server_id: String, app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let manager = app.state::<ConnectionManager>();
    let conns = manager.connections.lock().unwrap();
    conns
        .get(&server_id)
        .map(|c| c.capabilities.clone())
        .ok_or_else(|| "未连接服务器".to_string())
}

/// 上传大小限制信息（透传 Agent 响应；字段命名与 StatsResponse 一致为 snake_case）
#[derive(Debug, serde::Serialize)]
pub struct TransferLimitInfo {
    pub max_file_transfer_mb: u64,
    pub editable: bool,
    pub persisted: bool,
}

/// Tauri Command: 查询 Agent 上传大小限制（任何已认证用户）
#[tauri::command]
pub async fn get_transfer_limit(server_id: String, app: tauri::AppHandle) -> Result<TransferLimitInfo, String> {
    let resp = remote_send(server_id, Payload::GetTransferLimit {}, app).await?;
    match resp.payload {
        Payload::TransferLimitResponse { max_file_transfer_mb, editable, persisted } =>
            Ok(TransferLimitInfo { max_file_transfer_mb, editable, persisted }),
        // Agent 的 403/400 响应已是用户视角文案（映射在 Agent 端有单测锁定），原样透传
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

/// Tauri Command: 修改 Agent 上传大小限制（仅 root；立即对该服务器所有会话生效）
#[tauri::command]
pub async fn set_transfer_limit(server_id: String, max_file_transfer_mb: u64, app: tauri::AppHandle) -> Result<TransferLimitInfo, String> {
    tracing::debug!("[Connection] set_transfer_limit: server_id={}, max_file_transfer_mb={}", server_id, max_file_transfer_mb);

    let resp = remote_send(server_id, Payload::SetTransferLimit { max_file_transfer_mb }, app).await?;
    match resp.payload {
        Payload::TransferLimitResponse { max_file_transfer_mb, editable, persisted } =>
            Ok(TransferLimitInfo { max_file_transfer_mb, editable, persisted }),
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

// ── 订阅相关 Tauri Commands ─────────────────────────────────

#[tauri::command]
pub async fn subscribe(
    server_id: String,
    types: Vec<SubscriptionType>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let manager = app.state::<ConnectionManager>();

    // 使用锁保护整个订阅流程，避免竞态条件
    let quic_conn = {
        let mut conns = manager.connections.lock().unwrap();
        if let Some(active_conn) = conns.get_mut(&server_id) {
            // 检查是否已有订阅任务
            if active_conn.subscription_task.is_some() {
                tracing::info!("已有订阅任务: server_id={}", server_id);
                return Ok(()); // 已订阅，直接返回
            }

            // 获取 QUIC Connection
            active_conn.quic_conn.clone()
        } else {
            return Err("未找到连接".into());
        }
    };

    // 创建持久 Stream（用于订阅请求和事件监听）
    if let Some(conn) = quic_conn {
        let app_handle = app.clone();
        let server_id_clone = server_id.clone();
        let types_clone = types.clone();

        // 创建持久 Stream
        let stream = tokio::time::timeout(
            std::time::Duration::from_secs(STREAM_TIMEOUT_SECS),
            conn.open_bi(),
        )
        .await
        .map_err(|_| "创建 Stream 超时".to_string())?
        .map_err(|e| format!("创建 Stream 失败: {}", e))?;

        let (mut send, mut recv) = stream;

        // 发送 Subscribe payload
        let request_id = 1; // 使用固定的 request_id
        let subscribe_payload = Envelope::new(request_id, Payload::Subscribe {
            server_id: server_id_clone.clone(),
            types: types_clone.clone(),
        });

        if let Ok(data) = subscribe_payload.encode() {
            use tokio::io::AsyncWriteExt;
            // 发送消息长度
            let len = (data.len() as u32).to_le_bytes();
            // 发送操作添加超时
            let send_result = tokio::time::timeout(
                std::time::Duration::from_secs(STREAM_TIMEOUT_SECS),
                async {
                    send.write_all(&len).await.map_err(|e| format!("发送订阅请求长度失败: {}", e))?;
                    send.write_all(&data).await.map_err(|e| format!("发送订阅请求数据失败: {}", e))?;
                    send.flush().await.ok();
                    Ok::<(), String>(())
                },
            )
            .await;
            match send_result {
                Ok(Ok(())) => {}
                _ => return Err("发送订阅请求超时".into()),
            }
        }

        // 等待 SubscribeAck 响应（添加超时）
        let ack_result = tokio::time::timeout(
            std::time::Duration::from_secs(STREAM_TIMEOUT_SECS),
            async {
                let mut len_buf = [0u8; 4];
                recv.read_exact(&mut len_buf).await.map_err(|_| "读取响应长度失败".to_string())?;
                let len = u32::from_le_bytes(len_buf) as usize;
                let mut data_buf = vec![0u8; len];
                recv.read_exact(&mut data_buf).await.map_err(|_| "读取响应数据失败".to_string())?;
                Ok::<Vec<u8>, String>(data_buf)
            },
        )
        .await;
        let data_buf = match ack_result {
            Ok(Ok(buf)) => buf,
            _ => return Err("等待订阅响应超时".into()),
        };

        if let Ok(resp_envelope) = Envelope::decode(&data_buf) {
            if let Payload::SubscribeAck { success, .. } = resp_envelope.payload {
                if !success {
                    return Err("订阅失败".into());
                }
            } else {
                return Err("意外响应".into());
            }
        } else {
            return Err("解析响应失败".into());
        }

        // 启动监听任务
        let task = tokio::spawn(async move {
            tracing::info!("订阅成功: server_id={}", server_id_clone);

            // 监听 Event payload
            // 每条消息读取超时 60 秒，防止僵死 Stream
            let per_msg_timeout = std::time::Duration::from_secs(60);
            loop {
                // 读取消息长度（带超时）
                let mut len_buf = [0u8; 4];
                let read_len_result = tokio::time::timeout(per_msg_timeout, recv.read_exact(&mut len_buf)).await;
                match read_len_result {
                    Ok(Ok(_)) => {
                        let len = u32::from_le_bytes(len_buf) as usize;
                        let mut data_buf = vec![0u8; len];
                        // 读取消息数据（带超时）
                        let read_data_result = tokio::time::timeout(per_msg_timeout, recv.read_exact(&mut data_buf)).await;
                        match read_data_result {
                            Ok(Ok(_)) => {
                                // 解析 Event payload
                                if let Ok(event_envelope) = Envelope::decode(&data_buf) {
                                    if let Payload::Event { event_type, data, timestamp } = event_envelope.payload {
                                        // 转发到前端 Tauri Event
                                        let event_data = serde_json::json!({
                                            "server_id": server_id_clone,
                                            "event_type": event_type,
                                            "data": data,
                                            "timestamp": timestamp,
                                        });

                                        app_handle.emit("subscription_event", event_data).ok();
                                    }
                                }
                            }
                            Ok(Err(e)) => {
                                tracing::warn!("读取事件数据失败: {}", e);
                                break;
                            }
                            Err(_) => {
                                tracing::warn!("读取事件数据超时 ({}s)", per_msg_timeout.as_secs());
                                break;
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("读取事件长度失败: {}", e);
                        break;
                    }
                    Err(_) => {
                        tracing::warn!("读取事件长度超时 ({}s)", per_msg_timeout.as_secs());
                        break;
                    }
                }
            }
        });

        // 保存监听任务（使用锁保护）
        {
            let mut conns = manager.connections.lock().unwrap();
            if let Some(active_conn) = conns.get_mut(&server_id) {
                active_conn.subscription_task = Some(task);
            }
        }

        tracing::info!("订阅请求已发送并确认: server_id={}, types={}", server_id, types.len());
        Ok(())
    } else {
        Err("未找到 QUIC Connection".into())
    }
}

#[tauri::command]
pub async fn unsubscribe(
    server_id: String,
    types: Vec<SubscriptionType>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let manager = app.state::<ConnectionManager>();

    // 停止持久 Stream 监听任务
    let task_opt = {
        let mut conns = manager.connections.lock().unwrap();
        if let Some(active_conn) = conns.get_mut(&server_id) {
            active_conn.subscription_task.take()
        } else {
            None
        }
    };

    if let Some(task) = task_opt {
        // 使用 tokio::select! 等待任务完成或超时
        use tokio::time::{sleep, Duration};
        tokio::select! {
            _ = task => {
                tracing::info!("订阅监听任务已正常停止: server_id={}", server_id);
            }
            _ = sleep(Duration::from_secs(2)) => {
                // 超时，强制 abort
                tracing::warn!("订阅监听任务超时，强制停止: server_id={}", server_id);
            }
        }
    } else {
        tracing::info!("未找到订阅监听任务: server_id={}", server_id);
    }

    // 发送 Unsubscribe payload
    let resp = remote_send(server_id.clone(), Payload::Unsubscribe {
        server_id: server_id.clone(),
        types,
    }, app).await?;

    match resp.payload {
        Payload::UnsubscribeAck { success } => {
            if success {
                tracing::info!("取消订阅成功: server_id={}", server_id);
                Ok(())
            } else {
                Err("取消订阅失败".into())
            }
        }
        Payload::Error { message, .. } => Err(message),
        _ => Err("意外响应".into()),
    }
}

// ── QUIC 客户端 ───────────────────────────────────────

async fn try_quic_connect(host: &str, port: u16) -> Result<(quinn::Connection, f64, String), ConnectError> {
    // 安装 CryptoProvider
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 解析域名或 IP 地址
    let addr = if host.contains(':') || host.parse::<std::net::IpAddr>().is_ok() {
        // 已经是 IP 地址格式
        format!("{}:{}", host, port).parse().map_err(|e| {
            tracing::warn!("[QUIC] 地址解析失败: host={}:{}, error={:?}", host, port, e);
            ConnectError::with_detail(AuthErrorCode::DnsFailed, host)
        })?
    } else {
        // 需要解析域名，优先使用 IPv4
        let addr_str = format!("{}:{}", host, port);
        let resolved_addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host(&addr_str)
            .await
            .map_err(|e| {
                tracing::warn!("[QUIC] DNS 解析失败: host={}, error={:?}", host, e);
                ConnectError::with_detail(AuthErrorCode::DnsFailed, host)
            })?
            .collect();

        // 优先选择 IPv4 地址
        resolved_addrs
            .iter()
            .find(|addr| addr.is_ipv4())
            .copied()
            .or_else(|| resolved_addrs.first().copied())
            .ok_or_else(|| ConnectError::with_detail(AuthErrorCode::DnsFailed, host))?
    };

    // 创建客户端配置（证书钉扎：提取指纹供后续校验）
    let observed_fingerprint = Arc::new(Mutex::new(None));
    let client_config = build_quic_client_config(observed_fingerprint.clone())
        .map_err(|e| {
            tracing::warn!("[QUIC] 客户端配置构建失败: {}", e);
            ConnectError::new(AuthErrorCode::NetworkUnreachable)
        })?;

    // 创建 Endpoint
    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .map_err(|e| {
            tracing::warn!("[QUIC] 创建 Endpoint 失败: {}", e);
            ConnectError::new(AuthErrorCode::NetworkUnreachable)
        })?;

    endpoint.set_default_client_config(client_config);

    let start = std::time::Instant::now();

    // 添加超时机制（5 秒）
    let conn = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        endpoint
            .connect(addr, "quirel")
            .map_err(|e| {
                tracing::warn!("[QUIC] 发起连接失败: addr={}, error={:?}", addr, e);
                ConnectError::new(AuthErrorCode::NetworkUnreachable)
            })?
    )
    .await
    .map_err(|_| {
        tracing::warn!("[QUIC] 连接超时 (5秒): addr={}", addr);
        ConnectError::new(AuthErrorCode::ConnectTimeout)
    })?
    .map_err(|e| {
        tracing::warn!("[QUIC] 握手失败: addr={}, error={:?}", addr, e);
        ConnectError::new(AuthErrorCode::TlsHandshakeFailed)
    })?;

    // 提取握手过程中观测到的服务器证书指纹
    let fingerprint = observed_fingerprint
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| {
            tracing::warn!("[QUIC] 未能获取服务器证书指纹");
            ConnectError::new(AuthErrorCode::TlsHandshakeFailed)
        })?;

    Ok((conn, start.elapsed().as_secs_f64() * 1000.0, fingerprint))
}

fn build_quic_client_config(
    observed_fingerprint: Arc<Mutex<Option<String>>>,
) -> Result<quinn::ClientConfig, String> {
    // 证书钉扎：提取服务器证书指纹供后续校验（SSH known_hosts 模式）
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(PinningCertVerifier {
            observed_fingerprint,
        }))
        .with_no_client_auth();

    // 传输层配置：Bbr 拥塞控制 + 大窗口 + keep_alive + idle_timeout
    // idle_timeout: 30 秒无数据收发 → QUIC 自动关闭连接
    // 配合心跳(2s)使用：心跳每 2s 发一次，若连续超时说明连接异常
    // 30s 空闲超时给予网络波动足够的恢复时间，同时不会让僵死连接长期占用资源
    let mut transport = quinn::TransportConfig::default();
    if let Ok(timeout) = quinn::IdleTimeout::try_from(std::time::Duration::from_secs(30)) {
        transport.max_idle_timeout(Some(timeout));
    }
    transport.keep_alive_interval(Some(std::time::Duration::from_secs(5))); // 每5秒发送保持活跃包
    // 并发双向流上限：quinn 默认仅 100 条，浏览器代理场景不够用——
    // 1. 正常浏览：多主机 × 每主机多连接，重标签页时可瞬时超 100 条；
    // 2. 历史 bug 曾让泄漏的代理流永不关闭，耗尽预算后 open_bi 无限等待，
    //    心跳 Ping 同样卡死 → 5s 超时 → 误判连接死亡 → 监听挂起（2026-09-05 实录）。
    // 1024 提供充足余量，配合中继收尾宽限（proxy 模块）双保险。
    transport.max_concurrent_bidi_streams(quinn::VarInt::from_u32(1024));
    // 流控窗口：8MB per-stream / 64MB connection-wide，与服务端对称
    transport.stream_receive_window(quinn::VarInt::from_u32(8 * 1024 * 1024));  // 8MB
    transport.receive_window(quinn::VarInt::from_u32(64 * 1024 * 1024));         // 64MB
    // Bbr 拥塞控制：相比默认 Cubic 更适合高带宽高延迟链路
    transport.congestion_controller_factory(std::sync::Arc::new(quinn::congestion::BbrConfig::default()));

    let mut client = quinn::ClientConfig::new(std::sync::Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
            .map_err(|e| {
                tracing::warn!("[QUIC] TLS 配置错误: {}", e);
                format!("QUIC TLS 配置错误: {}", e)
            })?
    ));
    client.transport_config(std::sync::Arc::new(transport));

    Ok(client)
}

/// 证书钉扎验证器（SSH known_hosts 模式）
///
/// 替代无脑跳过证书验证，实现证书指纹提取：
/// - TLS 握手时提取服务器证书的 SHA-256 指纹
/// - 总是允许握手通过（真正的指纹校验在 remote_connect 中做）
/// - remote_connect 根据已知指纹决定是否需要用户确认
///
/// 安全模型（与 SSH known_hosts 一致）：
/// 1. 首次连接：展示指纹给用户确认，确认后存储
/// 2. 后续连接：自动校验指纹是否匹配
/// 3. 指纹不匹配：警告用户（可能服务器重装或 MITM），由用户决定
#[derive(Debug)]
struct PinningCertVerifier {
    /// 回传本次握手获取的服务器证书指纹（SHA-256，纯十六进制小写）
    observed_fingerprint: Arc<Mutex<Option<String>>>,
}

impl PinningCertVerifier {
    /// 计算证书 DER 数据的 SHA-256 指纹（纯十六进制小写）
    fn compute_fingerprint(cert_der: &[u8]) -> String {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(cert_der);
        let hash = hasher.finalize();
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }

    /// 格式化指纹用于显示（每两位用冒号分隔，便于阅读）
    fn format_fingerprint(hex: &str) -> String {
        hex.as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap_or("??"))
            .collect::<Vec<_>>()
            .join(":")
    }
}

impl rustls::client::danger::ServerCertVerifier for PinningCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let fingerprint = Self::compute_fingerprint(end_entity.as_ref());
        tracing::debug!("[TLS] 服务器证书指纹: {}", Self::format_fingerprint(&fingerprint));

        // 回传指纹供 remote_connect 校验
        *self.observed_fingerprint.lock().unwrap() = Some(fingerprint);

        // 允许握手通过（指纹校验在 remote_connect 中做，以便获取实际指纹展示给用户）
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

/// Stream 操作默认超时时间（30秒）
/// 适用于常规请求/响应操作（目录列表、文件读写元数据等）
const STREAM_TIMEOUT_SECS: u64 = 30;

/// 执行公钥认证流程（挑战-响应）
///
/// # 参数
/// - `conn`: QUIC 连接
/// - `username`: 用户名
/// - `private_key`: OpenSSH 格式的私钥内容
/// - `passphrase`: 私钥密码（可选）
///
/// # 返回
/// 成功返回 (session_id, Agent 能力声明)，失败返回错误信息
async fn perform_pubkey_auth(
    conn: &quinn::Connection,
    username: String,
    private_key: String,
    passphrase: Option<String>,
) -> Result<(Option<String>, Vec<String>), ConnectError> {
    use tokio::io::AsyncWriteExt;

    tracing::info!("[PubKeyAuth] 开始公钥认证流程: username={}", username);

    // ── 第一步：解析私钥 ───────────────────────────────
    tracing::info!("[PubKeyAuth] 私钥原始长度={}字节", private_key.len());
    tracing::info!("[PubKeyAuth] 私钥前150字符: {}", &private_key.chars().take(150).collect::<String>());

    tracing::debug!("[PubKeyAuth] 解析私钥（长度={}字节）", private_key.len());

    // 清理私钥内容：
    // 1. 去除BOM标记（\u{FEFF}）
    // 2. 统一换行符（将 \r\n 转为 \n）
    // 3. 去除首尾空白
    let private_key_cleaned = private_key
        .strip_prefix('\u{FEFF}')
        .unwrap_or(&private_key)
        .replace("\r\n", "\n")
        .trim()
        .to_string();

    tracing::debug!("[PubKeyAuth] 私钥清理后长度={}字节", private_key_cleaned.len());

    // 调试：输出私钥的前150个字符
    let preview_len = std::cmp::min(150, private_key_cleaned.len());
    tracing::debug!("[PubKeyAuth] 私钥预览（前{}字符）: {:?}", preview_len, &private_key_cleaned[..preview_len]);

    // 检查是否包含不可见字符
    let has_null = private_key_cleaned.contains('\u{0000}');
    let has_non_printable = private_key_cleaned.chars().any(|c| {
        c.is_control() && c != '\n' && c != '\r' && c != '\t'
    });

    if has_null {
        tracing::error!("[PubKeyAuth] 私钥包含 NUL 字节（\\0），这通常是二进制数据被错误读取导致的");
    }
    if has_non_printable {
        tracing::error!("[PubKeyAuth] 私钥包含不可打印的控制字符");
    }

    // 检查私钥格式
    if !private_key_cleaned.starts_with("-----BEGIN") {
        tracing::error!(
            "[PubKeyAuth] 不支持的私钥格式（应以 -----BEGIN 开头），当前开头: {}",
            &private_key_cleaned[..std::cmp::min(50, private_key_cleaned.len())]
        );
        return Err(ConnectError::with_detail(
            AuthErrorCode::InvalidKeyFormat,
            "不支持的私钥格式。请使用OpenSSH格式的私钥文件（通常以 -----BEGIN OPENSSH PRIVATE KEY----- 开头）",
        ));
    }

    // ── 第二步：自动检测私钥格式并解析 ─────────────────────
    // 自动检测私钥格式并解析
    tracing::debug!("[PubKeyAuth] 解析私钥（长度={}字节）", private_key_cleaned.len());

    let key = parse_private_key_auto(&private_key_cleaned, passphrase.as_deref())
        .map_err(|e| {
            tracing::error!("[PubKeyAuth] 私钥解析失败: {}", e);
            ConnectError::with_detail(AuthErrorCode::KeyParseFailed, e)
        })?;

    tracing::info!("[PubKeyAuth] 私钥解析成功: 算法={}", key.algorithm());

    // 提取公钥并转换为 SSH 格式字符串（如 "ssh-rsa AAAA..."）
    tracing::info!("[PubKeyAuth] 尝试提取公钥...");
    let public_key_str = key.public_key().to_openssh().map_err(|e| {
        tracing::error!("[PubKeyAuth] 公钥 SSH 格式转换失败: {}", e);
        ConnectError::new(AuthErrorCode::KeyParseFailed)
    })?;

    tracing::info!("[PubKeyAuth] 公钥 SSH 格式: {}", &public_key_str[..std::cmp::min(50, public_key_str.len())]);

    let public_key = public_key_str.into_bytes();

    tracing::info!("[PubKeyAuth] 公钥提取成功（长度={}字节）", public_key.len());

    // ── 第二步：创建 Stream 并发送公钥请求 ───────────────
    tracing::debug!("[PubKeyAuth] 创建 QUIC Stream...");

    let timeout_duration = std::time::Duration::from_secs(STREAM_TIMEOUT_SECS);
    let (mut send, mut recv) = tokio::time::timeout(timeout_duration, conn.open_bi())
        .await
        .map_err(|_| ConnectError::new(AuthErrorCode::StreamTimeout))?
        .map_err(|e| {
            tracing::warn!("[PubKeyAuth] 创建 Stream 失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;

    // 发送 AuthPubKeyRequest
    let request_id = 0; // 使用固定的 request_id（认证请求）
    let envelope = Envelope::new(request_id, Payload::AuthPubKeyRequest {
        username: username.clone(),
        public_key: public_key.clone(),
    });

    let bytes = envelope.encode().map_err(|e| {
        tracing::warn!("[PubKeyAuth] 请求编码失败: {}", e);
        ConnectError::new(AuthErrorCode::ProtocolError)
    })?;
    let len = (bytes.len() as u32).to_le_bytes();

    tracing::debug!("[PubKeyAuth] 发送公钥请求（{}字节）...", bytes.len());

    tokio::time::timeout(timeout_duration, async {
        send.write_all(&len).await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 发送长度失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        send.write_all(&bytes).await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 发送数据失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        send.flush().await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 刷新发送缓冲区失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        Ok::<(), ConnectError>(())
    })
    .await
    .map_err(|_| ConnectError::new(AuthErrorCode::StreamTimeout))??;

    tracing::debug!("[PubKeyAuth] 公钥请求已发送，等待挑战...");

    // ── 第三步：接收挑战 ─────────────────────────────────
    let challenge_data = tokio::time::timeout(timeout_duration, async {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 读取挑战长度失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;

        let mut data = vec![0u8; resp_len];
        recv.read_exact(&mut data).await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 读取挑战数据失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        Ok::<Vec<u8>, ConnectError>(data)
    })
    .await
    .map_err(|_| ConnectError::new(AuthErrorCode::StreamTimeout))??;

    let challenge_envelope = Envelope::decode(&challenge_data)
        .map_err(|e| {
            tracing::warn!("[PubKeyAuth] 挑战解码失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ProtocolError)
        })?;
    let (challenge, challenge_id) = match challenge_envelope.payload {
        Payload::AuthPubKeyChallenge { challenge, challenge_id } => {
            tracing::debug!("[PubKeyAuth] 收到挑战（长度={}字节，id={}）", challenge.len(), challenge_id);
            (challenge, challenge_id)
        }
        Payload::Error { code, message } => {
            // 归因黑洞补丁：Agent 主动通知的失败（区别于裸网络断开）
            tracing::warn!("[PubKeyAuth] Agent 返回错误: code={}, message={}", code, message);
            let err_code = AuthErrorCode::from_i32(code).unwrap_or(AuthErrorCode::Unknown);
            return Err(ConnectError::with_detail(err_code, message));
        }
        other => {
            tracing::error!("[PubKeyAuth] 期望挑战，收到: {:?}", other);
            return Err(ConnectError::new(AuthErrorCode::ProtocolError));
        }
    };

    // ── 第四步：签名挑战 ─────────────────────────────────
    tracing::info!("[PubKeyAuth] 使用私钥签名挑战...");
    tracing::info!("[PubKeyAuth] 挑战长度: {} 字节", challenge.len());

    // 使用 SSH 标准签名格式
    // 根据密钥类型选择合适的签名方式
    let signature_bytes = match key.algorithm() {
        ssh_key::Algorithm::Rsa { hash } => {
            tracing::info!("[PubKeyAuth] 使用 RSA 签名，hash={:?}", hash);

            // 获取 RSA 私钥的原始数据
            let rsa_keypair = match key.key_data() {
                ssh_key::private::KeypairData::Rsa(rsa) => rsa,
                _ => return Err(ConnectError::new(AuthErrorCode::KeyParseFailed)),
            };

            // 构造 RSA 私钥用于签名
            use rsa::pkcs1v15::SigningKey;
            use rsa::sha2::Sha256;
            use rsa::signature::{Signer, SignatureEncoding};

            // 从 ssh_key 的 RSA keypair 构造 rsa::RsaPrivateKey
            let n = rsa_keypair.public.n.as_bytes();
            let e = rsa_keypair.public.e.as_bytes();
            let d = rsa_keypair.private.d.as_bytes();
            let p = rsa_keypair.private.p.as_bytes();
            let q = rsa_keypair.private.q.as_bytes();

            tracing::info!("[PubKeyAuth] RSA 密钥参数: n={}字节, e={}字节, d={}字节", n.len(), e.len(), d.len());

            // 构造 RSA 私钥
            let rsa_priv = rsa::RsaPrivateKey::from_components(
                rsa::BigUint::from_bytes_be(n),
                rsa::BigUint::from_bytes_be(e),
                rsa::BigUint::from_bytes_be(d),
                vec![
                    rsa::BigUint::from_bytes_be(p),
                    rsa::BigUint::from_bytes_be(q),
                ],
            ).map_err(|e| {
                tracing::error!("[PubKeyAuth] RSA 密钥构造失败: {}", e);
                ConnectError::new(AuthErrorCode::KeyParseFailed)
            })?;

            // 使用 PKCS#1 v1.5 签名（SSH 标准使用的方式）
            let signing_key = SigningKey::<Sha256>::new(rsa_priv);
            let signature = signing_key.sign(&challenge);

            tracing::info!("[PubKeyAuth] RSA 签名成功（长度={}字节）", signature.to_bytes().len());

            // 包装成 SSH 协议格式：
            // string  算法名（如 "rsa-sha2-256"）
            // string  签名数据
            let sig_bytes = signature.to_bytes();
            let algo_name = b"rsa-sha2-256";
            let mut ssh_signature = Vec::new();
            // 算法名（4字节大端长度 + 数据）
            ssh_signature.extend_from_slice(&(algo_name.len() as u32).to_be_bytes());
            ssh_signature.extend_from_slice(algo_name);
            // 签名数据（4字节大端长度 + 数据）
            ssh_signature.extend_from_slice(&(sig_bytes.len() as u32).to_be_bytes());
            ssh_signature.extend_from_slice(&sig_bytes);

            tracing::info!("[PubKeyAuth] SSH格式签名（长度={}字节）", ssh_signature.len());
            ssh_signature
        }
        ssh_key::Algorithm::Ed25519 => {
            tracing::info!("[PubKeyAuth] 使用 Ed25519 签名");

            // 使用 ssh_key 的标准签名
            let sshsig = key.sign("quirel", ssh_key::HashAlg::default(), &challenge).map_err(|e| {
                tracing::error!("[PubKeyAuth] Ed25519 签名失败: {}", e);
                ConnectError::new(AuthErrorCode::KeyParseFailed)
            })?;

            let sig_pem = sshsig.to_pem(ssh_key::LineEnding::default()).map_err(|e| {
                tracing::error!("[PubKeyAuth] 签名 PEM 编码失败: {}", e);
                ConnectError::new(AuthErrorCode::KeyParseFailed)
            })?;

            sig_pem.into_bytes()
        }
        other => {
            tracing::error!("[PubKeyAuth] 不支持的密钥算法: {:?}", other);
            return Err(ConnectError::new(AuthErrorCode::InvalidKeyFormat));
        }
    };

    tracing::info!("[PubKeyAuth] 签名成功（长度={}字节）", signature_bytes.len());

    // ── 第五步：发送签名响应 ─────────────────────────────
    // 注意：服务器端期望在一个新的 Stream 上接收响应
    tracing::info!("[PubKeyAuth] 创建新的 QUIC Stream 发送签名响应...");

    let (mut resp_send, mut resp_recv) = tokio::time::timeout(timeout_duration, conn.open_bi())
        .await
        .map_err(|_| ConnectError::new(AuthErrorCode::StreamTimeout))?
        .map_err(|e| {
            tracing::warn!("[PubKeyAuth] 创建响应 Stream 失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;

    let response_envelope = Envelope::new(request_id, Payload::AuthPubKeyResponse {
        challenge_id: challenge_id.clone(),
        signature: signature_bytes,
        public_key: public_key.clone(),
    });

    let bytes = response_envelope.encode().map_err(|e| {
        tracing::warn!("[PubKeyAuth] 响应编码失败: {}", e);
        ConnectError::new(AuthErrorCode::ProtocolError)
    })?;
    let len = (bytes.len() as u32).to_le_bytes();

    tracing::info!("[PubKeyAuth] 发送签名响应...");

    tokio::time::timeout(timeout_duration, async {
        resp_send.write_all(&len).await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 发送签名长度失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        resp_send.write_all(&bytes).await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 发送签名数据失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        resp_send.flush().await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 刷新发送缓冲区失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        Ok::<(), ConnectError>(())
    })
    .await
    .map_err(|_| ConnectError::new(AuthErrorCode::StreamTimeout))??;

    tracing::info!("[PubKeyAuth] 签名响应已发送，等待最终认证结果...");

    // ── 第六步：接收最终认证结果 ───────────────────────
    let final_data = tokio::time::timeout(timeout_duration, async {
        let mut len_buf = [0u8; 4];
        resp_recv.read_exact(&mut len_buf).await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 读取认证结果长度失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;

        let mut data = vec![0u8; resp_len];
        resp_recv.read_exact(&mut data).await.map_err(|e| {
            tracing::warn!("[PubKeyAuth] 读取认证结果数据失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ConnectionLost)
        })?;
        Ok::<Vec<u8>, ConnectError>(data)
    })
    .await
    .map_err(|_| ConnectError::new(AuthErrorCode::StreamTimeout))??;

    let final_envelope = Envelope::decode(&final_data)
        .map_err(|e| {
            tracing::warn!("[PubKeyAuth] 认证结果解码失败: {:?}", e);
            ConnectError::new(AuthErrorCode::ProtocolError)
        })?;
    match final_envelope.payload {
        Payload::AuthResponse { success, error, session_id, code, capabilities } => {
            if success {
                tracing::info!("[PubKeyAuth] 公钥认证成功: username={}, session_id={:?}", username, session_id);
                // 认证成功：携带能力声明返回（旧 Agent 无此字段 → 空列表）
                Ok((session_id, capabilities.unwrap_or_default()))
            } else {
                // 优先使用结构化 code；旧版 Agent 无 code 时按文案 fallback 分类
                let err_code = code
                    .unwrap_or_else(|| classify_legacy_error(error.as_deref().unwrap_or("")));
                tracing::error!("[PubKeyAuth] 公钥认证失败: code={:?}, error={:?}", err_code, error);
                Err(ConnectError::with_detail(err_code, error.unwrap_or_else(|| "公钥认证失败".to_string())))
            }
        }
        Payload::Error { code, message } => {
            tracing::error!("[PubKeyAuth] Agent 返回错误: code={}, message={}", code, message);
            let err_code = AuthErrorCode::from_i32(code).unwrap_or(AuthErrorCode::Unknown);
            Err(ConnectError::with_detail(err_code, message))
        }
        other => {
            tracing::error!("[PubKeyAuth] 期望认证结果，收到: {:?}", other);
            Err(ConnectError::new(AuthErrorCode::ProtocolError))
        }
    }
}

async fn send_and_receive_quic(conn: &quinn::Connection, request_id: u32, payload: Payload) -> Result<Vec<u8>, String> {
    let timeout_duration = std::time::Duration::from_secs(STREAM_TIMEOUT_SECS);

    // Stream 打开也需要超时保护，避免在连接异常时无限等待
    let (mut send, mut recv) = tokio::time::timeout(timeout_duration, conn.open_bi())
        .await
        .map_err(|_| {
            tracing::warn!("[QUIC] 打开 Stream 超时: request_id={}", request_id);
            "打开 Stream 超时".to_string()
        })?
        .map_err(|e| {
            tracing::warn!("[QUIC] 打开 Stream 失败: request_id={}, error={}", request_id, e);
            format!("打开 Stream 失败: {}", e)
        })?;

    let bytes = Envelope::new(request_id, payload).encode()?;
    let len = (bytes.len() as u32).to_le_bytes();

    // 发送操作也需要超时
    tokio::time::timeout(timeout_duration, async {
        send.write_all(&len).await.map_err(|e| format!("发送长度失败: {}", e))?;
        send.write_all(&bytes).await.map_err(|e| format!("发送数据失败: {}", e))?;
        send.finish().map_err(|e| format!("关闭写入流失败: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| {
        tracing::warn!("[QUIC] 发送数据超时: request_id={}", request_id);
        "发送数据超时".to_string()
    })??;

    // 接收操作也需要超时
    let data = tokio::time::timeout(timeout_duration, async {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await.map_err(|e| format!("读取响应长度失败: {}", e))?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;

        let mut data = vec![0u8; resp_len];
        recv.read_exact(&mut data).await.map_err(|e| format!("读取响应数据失败: {}", e))?;
        Ok::<Vec<u8>, String>(data)
    })
    .await
    .map_err(|_| {
        tracing::warn!("[QUIC] 接收响应超时: request_id={}", request_id);
        "接收响应超时".to_string()
    })??;

    Ok(data)
}

// ── PEM 格式私钥解析辅助函数 ─────────────────────────────────

/// 解析 PEM 格式的 RSA 私钥（阿里云等云服务商常用）
fn parse_pem_rsa(pem: &str, passphrase: Option<&str>) -> Result<ssh_key::PrivateKey, String> {
    use rsa::RsaPrivateKey;
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;
    use ssh_key::private::{RsaKeypair, KeypairData};

    tracing::debug!("[PEM] 开始解析 PEM RSA 私钥");

    // 检查是否有非空密码（空字符串视为无密码）
    if passphrase.filter(|s| !s.is_empty()).is_some() {
        return Err("加密的PEM私钥暂不支持。请使用ssh-keygen解密：\nssh-keygen -p -f <私钥文件>\n或转换为OpenSSH格式：\nssh-keygen -i -f <PEM私钥> > id_rsa".to_string());
    }

    // 尝试解析未加密的私钥（支持PKCS#1和PKCS#8）
    let rsa_key = if pem.contains("-----BEGIN PRIVATE KEY-----") {
        // PKCS#8 格式
        tracing::debug!("[PEM] 检测到 PKCS#8 格式");
        RsaPrivateKey::from_pkcs8_pem(pem)
            .map_err(|e| {
                tracing::error!("[PEM] PKCS#8 RSA 解析失败: {}", e);
                format!("PKCS#8 RSA 解析失败: {}", e)
            })?
    } else {
        // PKCS#1 格式
        tracing::debug!("[PEM] 检测到 PKCS#1 格式");
        RsaPrivateKey::from_pkcs1_pem(pem)
            .map_err(|e| {
                tracing::error!("[PEM] PKCS#1 RSA 解析失败: {}", e);
                format!("PKCS#1 RSA 解析失败: {}", e)
            })?
    };

    // 直接从 rsa::RsaPrivateKey 转换为 ssh_key 格式
    let rsa_keypair = RsaKeypair::try_from(&rsa_key)
        .map_err(|e| {
            tracing::error!("[PEM] 转换为 SSH 密钥失败: {}", e);
            format!("转换为 SSH 密钥失败: {}", e)
        })?;

    // 构造 ssh_key::PrivateKey
    let private_key = ssh_key::PrivateKey::new(
        KeypairData::Rsa(rsa_keypair),
        ""  // comment
    ).map_err(|e| {
        tracing::error!("[PEM] 构造 SSH 私钥失败: {}", e);
        format!("构造 SSH 私钥失败: {}", e)
    })?;

    Ok(private_key)
}

// ── 私钥自动格式检测 ─────────────────────────────────────

/// 自动检测私钥格式并解析
///
/// 支持的格式（按优先级）：
/// 1. OpenSSH 格式（现代标准）
/// 2. PEM PKCS#8 格式（通用PEM）
/// 3. PEM PKCS#1 RSA 格式（阿里云等云服务商）
fn parse_private_key_auto(key_data: &str, passphrase: Option<&str>) -> Result<ssh_key::PrivateKey, String> {
    tracing::debug!("[KeyParser] 开始自动检测私钥格式");

    // 关键调试信息（INFO级别，确保显示）
    tracing::info!("[KeyParser] 私钥长度: {} 字节", key_data.len());
    tracing::info!("[KeyParser] 私钥前100字符: {}", &key_data.chars().take(100).collect::<String>());
    tracing::info!("[KeyParser] 密码字段: {:?}", passphrase);

    // 1. 尝试 OpenSSH 格式（优先级最高）
    if key_data.contains("-----BEGIN OPENSSH PRIVATE KEY-----") {
        tracing::debug!("[KeyParser] 尝试解析 OpenSSH 格式");
        match parse_openssh_key(key_data, passphrase) {
            Ok(key) => {
                tracing::info!("[KeyParser] ✅ 成功解析 OpenSSH 格式密钥");
                return Ok(key);
            }
            Err(e) => {
                tracing::debug!("[KeyParser] ❌ OpenSSH 格式解析失败: {}", e);
            }
        }
    }

    // 2. 尝试 PEM 格式
    if key_data.contains("-----BEGIN") {
        tracing::debug!("[KeyParser] 尝试解析 PEM 格式");

        // 检查是否加密
        if key_data.contains("ENCRYPTED") {
            tracing::warn!("[KeyParser] 检测到加密私钥");
            if passphrase.is_none() {
                return Err("私钥已加密，请在'私钥密码'字段输入密码。".to_string());
            }
        }

        match parse_pem_rsa(key_data, passphrase) {
            Ok(key) => {
                tracing::info!("[KeyParser] ✅ 成功解析 PEM 格式密钥");
                return Ok(key);
            }
            Err(e) => {
                tracing::debug!("[KeyParser] ❌ PEM 格式解析失败: {}", e);
            }
        }
    }

    // 3. 所有格式都失败，返回友好的错误提示
    Err(generate_key_parse_error(key_data))
}

/// 解析 OpenSSH 格式私钥
fn parse_openssh_key(key_data: &str, passphrase: Option<&str>) -> Result<ssh_key::PrivateKey, String> {
    if let Some(pwd) = passphrase {
        ssh_key::PrivateKey::from_openssh(key_data)
            .map_err(|e| format!("OpenSSH格式解析失败: {}", e))?
            .decrypt(pwd.as_bytes())
            .map_err(|e| format!("私钥解密失败（密码错误）: {}", e))
    } else {
        ssh_key::PrivateKey::from_openssh(key_data)
            .map_err(|e| format!("OpenSSH格式解析失败: {}（如果私钥有密码保护，请输入密码）", e))
    }
}

/// 生成友好的密钥解析错误提示
fn generate_key_parse_error(key_data: &str) -> String {
    let mut error_msg = String::from("无法解析私钥文件。\n\n");

    // 分析可能的问题
    if key_data.contains("PuTTY") {
        error_msg.push_str("检测到 PuTTY 格式（.ppk）。请使用以下方法转换：\n");
        error_msg.push_str("1. 打开 PuTTYgen\n");
        error_msg.push_str("2. 加载您的 .ppk 文件\n");
        error_msg.push_str("3. 点击 'Conversions' -> 'Export OpenSSH key'\n");
        error_msg.push_str("4. 保存为新的 OpenSSH 格式文件\n\n");
    } else if !key_data.contains("-----BEGIN") {
        error_msg.push_str("未检测到有效的私钥格式。请检查：\n");
        error_msg.push_str("1. 文件是否完整（包含 -----BEGIN 和 -----END 标记）\n");
        error_msg.push_str("2. 是否是私钥文件（公钥文件无法用于认证）\n\n");
    } else {
        error_msg.push_str("支持的格式：\n");
        error_msg.push_str("✅ OpenSSH 格式（推荐）\n");
        error_msg.push_str("✅ PEM 格式 - PKCS#1 RSA（阿里云、AWS等云服务商）\n");
        error_msg.push_str("✅ PEM 格式 - PKCS#8（通用PEM格式）\n\n");

        error_msg.push_str("不支持的格式：\n");
        error_msg.push_str("❌ PuTTY 格式（.ppk）- 需要先转换为 OpenSSH 格式\n");
        error_msg.push_str("❌ SSH.com 格式 - 需要先转换\n\n");

        error_msg.push_str("如果私钥有密码保护，请在'私钥密码'字段输入密码。");
    }

    error_msg
}

#[cfg(test)]
mod connect_error_tests {
    use super::*;

    /// 旧版 Agent（无结构化 code）的中文文案 → 最近似 AuthErrorCode
    #[test]
    fn classify_legacy_error_maps_known_agent_texts() {
        assert_eq!(classify_legacy_error("请求过于频繁，请稍后再试"), AuthErrorCode::RateLimited);
        assert_eq!(classify_legacy_error("账户暂时锁定，请15分钟后再试"), AuthErrorCode::AccountLocked);
        assert_eq!(classify_legacy_error("用户名或密码错误"), AuthErrorCode::InvalidCredentials);
        assert_eq!(classify_legacy_error("公钥未授权"), AuthErrorCode::PubkeyNotAuthorized);
        assert_eq!(classify_legacy_error("签名验证失败"), AuthErrorCode::SignatureVerificationFailed);
        assert_eq!(classify_legacy_error("公钥验证失败"), AuthErrorCode::SignatureVerificationFailed);
        assert_eq!(classify_legacy_error("认证服务暂时不可用"), AuthErrorCode::AuthServiceUnavailable);
        assert_eq!(classify_legacy_error("QUIC 连接超时 (5秒)"), AuthErrorCode::StreamTimeout);
        assert_eq!(classify_legacy_error("QUIC 连接失败: QUIC 握手失败: ..."), AuthErrorCode::ConnectTimeout);
        assert_eq!(classify_legacy_error("任何未识别的文本"), AuthErrorCode::Unknown);
    }

    /// ConnectError 序列化形状（前端 parseConnectError 的解析契约）
    #[test]
    fn connect_error_serializes_code_and_detail() {
        let e = ConnectError::with_detail(AuthErrorCode::AccountLocked, "prod-1");
        assert_eq!(serde_json::to_string(&e).unwrap(), r#"{"code":201,"detail":"prod-1"}"#);
        let e2 = ConnectError::new(AuthErrorCode::ConnectionLost);
        assert_eq!(serde_json::to_string(&e2).unwrap(), r#"{"code":106,"detail":null}"#);
    }
}