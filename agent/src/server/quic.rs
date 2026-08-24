use anyhow::Result;
use quinn::{RecvStream, SendStream};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, Mutex};

// Unix平台特有的导入(终端功能)
use tokio::time::Duration;

use crate::config::AgentConfig;
use crate::subscription::SubscriptionManager;
use crate::event_bus::EventBus;
use crate::protocol::{Envelope, Payload};
use crate::auth::{Authenticator, CompositeAuthenticator, UserSession, UserExecutor, ChallengeManager, AuthRateLimiter, StatsManager, ConnectionCloseReason};
use crate::audit::AuditLogger;

// 导入 manager 模块的 PTY 输出任务
#[cfg(unix)]
use crate::manager::{spawn_pty_output_task_v2, PtyOutputConfig};

// 全局 Stream ID 计数器
static STREAM_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// 获取当前 Unix 时间戳（秒）
fn current_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 连接上下文，跟踪连接的生命周期和关联资源
///
/// 当 QUIC 连接关闭时，通过 shutdown 信号通知所有关联的持久 Stream 任务
/// （终端、订阅等），然后统一清理 PTY 会话、订阅和过期传输会话，
/// 防止资源泄漏。
pub struct ConnectionContext {
    /// 关闭信号广播通道（容量 1，只发最后一次信号）
    shutdown_tx: broadcast::Sender<()>,
    /// 关联的 PTY 会话 ID 列表（连接关闭时自动清理）
    pty_session_ids: Arc<Mutex<Vec<String>>>,
    /// 关联的订阅 Stream ID 列表（连接关闭时自动清理订阅）
    stream_ids: Arc<Mutex<Vec<u64>>>,
    /// 关联的传输会话 ID 列表（连接关闭时自动清理传输会话）
    transfer_session_ids: Arc<Mutex<Vec<String>>>,
    /// 已认证的用户会话信息
    session: Arc<Mutex<Option<UserSession>>>,
    /// 会话最后活动时间（用于会话超时检查）
    session_last_activity: Arc<AtomicU64>,
}

impl ConnectionContext {
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            shutdown_tx,
            pty_session_ids: Arc::new(Mutex::new(Vec::new())),
            stream_ids: Arc::new(Mutex::new(Vec::new())),
            transfer_session_ids: Arc::new(Mutex::new(Vec::new())),
            session: Arc::new(Mutex::new(None)),
            session_last_activity: Arc::new(AtomicU64::new(current_timestamp_secs())),
        }
    }

    /// 设置用户会话信息
    pub async fn set_session(&self, session: UserSession) {
        let mut sess = self.session.lock().await;
        *sess = Some(session);
        // 初始化会话活动时间
        self.session_last_activity.store(current_timestamp_secs(), Ordering::Relaxed);
    }

    /// 获取用户会话信息
    #[allow(dead_code)]
    pub async fn get_session(&self) -> Option<UserSession> {
        let sess = self.session.lock().await;
        sess.clone()
    }

    /// 更新会话活动时间（每次用户操作时调用）
    pub fn touch_session(&self) {
        self.session_last_activity.store(current_timestamp_secs(), Ordering::Relaxed);
    }

    /// 检查会话是否超时（24小时不活动）
    pub fn is_session_timeout(&self) -> bool {
        let now = current_timestamp_secs();
        let last = self.session_last_activity.load(Ordering::Relaxed);
        let idle_secs = now.saturating_sub(last);

        // 24小时 = 86400秒
        const SESSION_TIMEOUT_SECS: u64 = 24 * 60 * 60;

        idle_secs >= SESSION_TIMEOUT_SECS
    }

    /// 订阅关闭信号（每个需要响应关闭的 Stream 调用一次）
    pub fn subscribe_shutdown(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// 触发关闭信号，通知所有关联的 Stream 任务退出
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// 注册 PTY 会话（连接关闭时自动清理）
    #[allow(dead_code)]
    pub async fn register_pty_session(&self, session_id: String) {
        self.pty_session_ids.lock().await.push(session_id);
    }

    /// 注册订阅 Stream ID（连接关闭时自动清理订阅）
    pub async fn register_stream_id(&self, stream_id: u64) {
        self.stream_ids.lock().await.push(stream_id);
    }

    /// 注册传输会话 ID（连接关闭时自动清理传输会话）
    pub async fn register_transfer_session(&self, session_id: String) {
        self.transfer_session_ids.lock().await.push(session_id);
    }

    /// 清理指定连接的所有传输会话
    ///
    /// 从全局 TRANSFER_SESSIONS 中移除属于该连接的所有传输会话。
    /// TransferSession 的 Drop impl 会自动清理未完成上传的临时文件。
    pub async fn cleanup_transfer_sessions(&self) {
        let session_ids = self.transfer_session_ids.lock().await.clone();
        if session_ids.is_empty() {
            return;
        }

        let mut sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
        let mut cleaned = 0;
        for id in &session_ids {
            if sessions.remove(id).is_some() {
                cleaned += 1;
                tracing::info!(
                    "[ConnectionContext] 已清理传输会话: session_id={}",
                    id
                );
            }
        }
        if cleaned > 0 {
            tracing::info!(
                "[ConnectionContext] 共清理了 {} 个传输会话",
                cleaned
            );
        }
    }

    /// 清理所有关联资源（连接关闭后调用）
    ///
    /// 清理顺序：
    /// 1. 注销所有 PTY 会话（发送 Close 帧给 Session 进程，断开 UnixSocket 连接）
    /// 2. 移除所有订阅（停止数据采集器）
    /// 3. 清理超时的传输会话（安全兜底）
    pub async fn cleanup(
        &self,
        #[cfg(unix)] pty_registry: &crate::manager::PtyRegistry,
        subscription_manager: &SubscriptionManager,
    ) {
        // 1. 清理 PTY 会话（仅 Unix,使用 PtyRegistry）
        #[cfg(unix)]
        {
            let pty_ids = self.pty_session_ids.lock().await.clone();
            for session_id in &pty_ids {
                match pty_registry.unregister(session_id).await {
                    Ok(_) => {
                        tracing::info!("[ConnectionContext] 已清理 PTY 会话: {}", session_id);
                    }
                    Err(e) => {
                        // PTY 会话可能已被 handle_terminal_stream 自行清理，这是正常的
                        tracing::debug!("[ConnectionContext] PTY 会话清理（可能已移除）: {}: {}", session_id, e);
                    }
                }
            }
        }
        // 非 Unix 平台无 PTY 会话需要清理（PtyManager 非 Unix 桩实现无需调用）

        // 2. 清理订阅
        let sids = self.stream_ids.lock().await.clone();
        for stream_id in &sids {
            match subscription_manager.remove_all(*stream_id).await {
                Ok(_) => {
                    tracing::info!("[ConnectionContext] 已清理订阅: stream_id={}", stream_id);
                }
                Err(e) => {
                    tracing::warn!("[ConnectionContext] 清理订阅失败: stream_id={}: {}", stream_id, e);
                }
            }
        }

        // 3. 清理该连接关联的所有传输会话（即时清理，不再仅依赖超时）
        self.cleanup_transfer_sessions().await;

        // 4. 兜底：清理其他已超时的传输会话（防止非正常路径导致的泄漏）
        let mut sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
        let expired_ids: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| s.is_timeout())
            .map(|(id, _)| id.clone())
            .collect();
        for id in &expired_ids {
            if let Some(session) = sessions.remove(id) {
                tracing::info!(
                    "[ConnectionContext] 已清理过期传输会话: id={}, path={}, direction={}",
                    id, session.path, session.direction
                );
            }
        }
    }
}

impl Default for ConnectionContext {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn run(
    cfg: AgentConfig,
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    subscription_manager: Arc<SubscriptionManager>,
    event_bus: Arc<EventBus>,
    authenticator: Arc<CompositeAuthenticator>,
    audit_log: Arc<AuditLogger>,
    #[cfg(unix)]
    manager: Arc<crate::manager::Manager>,
) -> Result<()> {
    let addr = format!("{}:{}", cfg.server.bind, cfg.server.quic_port);
    tracing::info!("🔵 QUIC 服务器监听: {}", addr);

    let idle_timeout_secs = cfg.limits.connection_idle_timeout_secs;
    tracing::info!("⏱️  连接空闲超时: {} 秒", idle_timeout_secs);

    let server_config = build_server_config(certs, key)?;

    // 手动创建 UDP socket 并设置 SO_REUSEADDR，解决 systemctl restart 时端口释放延迟问题
    // 旧进程刚被 kill，内核可能还未完全释放 UDP 端口，导致 bind 失败
    // SO_REUSEADDR 必须在 bind 之前设置才能生效（与 WebSocket 侧实现一致）
    // 同时加重试机制（最多 5 次，每次 1 秒），应对端口被活跃进程暂时占用的情况
    let socket = {
        let sock_addr: std::net::SocketAddr = addr.parse()?;
        let mut retries = 0;
        const MAX_RETRIES: u32 = 5;
        loop {
            // 先创建 socket，设置 SO_REUSEADDR，再 bind（顺序很重要）
            match socket2::Socket::new(
                socket2::Domain::for_address(sock_addr),
                socket2::Type::DGRAM,
                None,
            ) {
                Ok(sock) => {
                    sock.set_reuse_address(true)?;
                    sock.set_nonblocking(true)?;

                    match sock.bind(&sock_addr.into()) {
                        Ok(()) => {
                            tracing::info!("🔵 QUIC socket 已绑定: {}", addr);
                            break std::net::UdpSocket::from(sock);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && retries < MAX_RETRIES => {
                            retries += 1;
                            tracing::warn!(
                                "QUIC 端口 {} 被占用，等待重试 ({}/{}): {}",
                                addr, retries, MAX_RETRIES, e
                            );
                            // sock 在此处 drop，释放 fd
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!(
                                "QUIC bind {} 失败 (重试 {} 次后放弃): {}",
                                addr, retries, e
                            ));
                        }
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("创建 QUIC socket 失败: {}", e));
                }
            }
        }
    };

    let endpoint = quinn::Endpoint::new(
        quinn::EndpointConfig::default(),
        Some(server_config),
        socket,
        Arc::new(quinn::TokioRuntime),
    )?;

    // 创建全局认证速率限制器
    let rate_limiter = Arc::new(AuthRateLimiter::new());
    tracing::info!("🛡️  认证速率限制器已启用");

    // 创建全局挑战管理器（用于公钥认证）
    let challenge_manager = Arc::new(ChallengeManager::new());
    tracing::info!("🔐 挑战管理器已启用");

    // 创建全局统计管理器
    let stats_manager = Arc::new(StatsManager::new());
    tracing::info!("📊 统计管理器已启用");

    // 启动定期清理任务（每5分钟清理过期记录）
    let rate_limiter_clone = rate_limiter.clone();
    let challenge_manager_clone = challenge_manager.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            // 清理速率限制器过期记录
            rate_limiter_clone.cleanup_expired().await;
            // 清理挑战管理器过期挑战
            challenge_manager_clone.cleanup_expired().await;
            tracing::debug!("认证速率限制器和挑战管理器清理完成");
        }
    });

    while let Some(incoming) = endpoint.accept().await {
        let cfg_clone = cfg.clone();
        let subscription_manager_clone = subscription_manager.clone();
        let event_bus_clone = event_bus.clone();
        let authenticator_clone = authenticator.clone();
        let audit_log_clone = audit_log.clone();
        let rate_limiter_clone = rate_limiter.clone();
        let challenge_manager_clone = challenge_manager.clone();
        let stats_manager_clone = stats_manager.clone();
        let timeout_secs = idle_timeout_secs;
        #[cfg(unix)]
        let manager_clone = manager.clone();
        tokio::spawn(async move {
            let conn = incoming.await;
            match conn {
                Ok(connection) => {
                    if let Err(e) = handle_connection(
                        connection,
                        &cfg_clone,
                        subscription_manager_clone,
                        event_bus_clone,
                        authenticator_clone,
                        audit_log_clone,
                        rate_limiter_clone,
                        challenge_manager_clone,
                        stats_manager_clone,
                        timeout_secs,
                        #[cfg(unix)]
                        manager_clone,
                    ).await {
                        tracing::warn!("QUIC 连接错误: {}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("QUIC 连接握手失败: {}", e);
                }
            }
        });
    }

    Ok(())
}

fn build_server_config(
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<quinn::ServerConfig> {
    let mut quic_config = quinn::ServerConfig::with_single_cert(certs, key)
        .map_err(|e| {
            tracing::warn!("[QUIC] 服务器配置构建失败: {}", e);
            e
        })?;
    
    // 配置传输参数：Bbr 拥塞控制 + 大窗口 + keep_alive
    let mut transport = quinn::TransportConfig::default();
    transport.max_idle_timeout(None); // 禁用空闲超时，连接不会因无活动而关闭
    transport.keep_alive_interval(Some(std::time::Duration::from_secs(5))); // 每5秒发送保持活跃包
    // 流控窗口：8MB per-stream / 64MB connection-wide，提升高 BDP 链路吞吐
    transport.stream_receive_window(quinn::VarInt::from_u32(8 * 1024 * 1024));  // 8MB
    transport.receive_window(quinn::VarInt::from_u32(64 * 1024 * 1024));         // 64MB
    // Bbr 拥塞控制：相比默认 Cubic 更适合高带宽高延迟链路，充分发挥设备性能
    transport.congestion_controller_factory(std::sync::Arc::new(quinn::congestion::BbrConfig::default()));

    quic_config.transport_config(std::sync::Arc::new(transport));
    Ok(quic_config)
}

async fn handle_connection(
    connection: quinn::Connection,
    cfg: &AgentConfig,
    subscription_manager: Arc<SubscriptionManager>,
    event_bus: Arc<EventBus>,
    authenticator: Arc<CompositeAuthenticator>,
    audit_log: Arc<AuditLogger>,
    rate_limiter: Arc<AuthRateLimiter>,
    challenge_manager: Arc<ChallengeManager>,
    stats_manager: Arc<StatsManager>,
    idle_timeout_secs: u64,
    #[cfg(unix)]
    manager: Arc<crate::manager::Manager>,
) -> Result<()> {
    let remote = connection.remote_address();
    tracing::info!("✅ 新的 QUIC 连接来自: {}", remote);

    // 记录连接打开
    stats_manager.record_connection_opened();

    // 创建连接上下文，跟踪该连接的所有关联资源
    let ctx = Arc::new(ConnectionContext::new());

    // 每个连接独立的活动时间追踪（Arc<AtomicU64> 存储最近活动的 Unix 时间戳秒数）
    let last_activity: Arc<AtomicU64> = Arc::new(AtomicU64::new(current_timestamp_secs()));

    // 用于通知超时检查任务退出的信号
    let (timeout_cancel_tx, mut timeout_cancel_rx) = tokio::sync::oneshot::channel::<()>();

    // ========== 认证流程 ==========
    // 等待客户端的第一个 Stream（认证流）
    let auth_stream = connection.accept_bi().await?;
    let (mut auth_send, mut auth_recv) = auth_stream;

    // 读取认证请求
    let auth_data = read_message(&mut auth_recv).await?;
    if auth_data.is_none() {
        tracing::warn!("认证流已关闭: remote={}", remote);

        // 发送明确的错误响应
        if let Err(e) = send_auth_response(&mut auth_send, 0, false, Some("认证流异常关闭"), None).await {
            tracing::debug!("发送认证流关闭响应失败: {}", e);
        }

        connection.close(0u32.into(), b"auth stream closed");
        return Ok(());
    }

    let auth_data = auth_data.unwrap();

    // 解析认证请求
    let auth_envelope = match Envelope::decode(&auth_data) {
        Ok(envelope) => envelope,
        Err(e) => {
            tracing::error!(
                "❌ 认证请求解析失败: remote={}, error={}, data_len={}",
                remote,
                e,
                auth_data.len()
            );
            send_auth_response(&mut auth_send, 0, false, Some("协议格式错误"), None).await?;
            connection.close(0u32.into(), b"invalid protocol");
            return Ok(());
        }
    };

    tracing::info!(
        "📥 收到认证请求: remote={}, request_id={}, payload_type={}",
        remote,
        auth_envelope.request_id,
        auth_envelope.payload.type_name()
    );

    // 获取客户端IP地址（用于速率限制）
    let client_ip = connection.remote_address().ip().to_string();

    // 验证是否为认证请求
    let session = match &auth_envelope.payload {
        // ========== 密码认证（PAM）==========
        Payload::AuthPasswordRequest { username, password } => {
            tracing::info!("收到密码认证请求: remote={}, username={}", remote, username);

            // 1. 检查IP速率限制
            if !rate_limiter.is_ip_allowed(&client_ip).await? {
                tracing::warn!(
                    "IP速率限制触发: ip={}, username={}",
                    client_ip,
                    username
                );
                audit_log.log_auth_failure(username, 0, "ip_rate_limited");
                stats_manager.record_rate_limited();

                send_auth_response(
                    &mut auth_send,
                    auth_envelope.request_id,
                    false,
                    Some("请求过于频繁，请稍后再试"),
                    None,
                ).await?;

                connection.close(0u32.into(), b"rate limited");
                return Ok(());
            }

            // 2. 检查用户名锁定状态
            if !rate_limiter.is_username_allowed(username).await? {
                tracing::warn!(
                    "用户名被锁定: username={}, remote={}",
                    username,
                    remote
                );
                audit_log.log_auth_failure(username, 0, "account_locked");
                stats_manager.record_account_locked();

                send_auth_response(
                    &mut auth_send,
                    auth_envelope.request_id,
                    false,
                    Some("账户暂时锁定，请15分钟后再试"),
                    None,
                ).await?;

                connection.close(0u32.into(), b"account locked");
                return Ok(());
            }

            // 调用认证器进行密码认证
            match authenticator.authenticate_password(username, password) {
                Ok(crate::auth::AuthResult::Success(identity)) => {
                    // 认证成功：清除失败记录
                    rate_limiter.clear_failures(username).await;

                    // 创建用户会话
                    let session = UserSession::new(identity);

                    // 保存会话到连接上下文
                    ctx.set_session(session.clone()).await;

                    // 记录审计日志
                    audit_log.log_auth_success(&session.username, session.uid, "password");

                    // 记录认证统计
                    stats_manager.record_auth_success("password");

                    // 发送认证成功响应
                    send_auth_response(&mut auth_send, auth_envelope.request_id, true, None, Some(&session.session_id)).await?;

                    tracing::info!("✅ 密码认证成功: remote={}, user={}", remote, session.username);
                    session
                }
                Ok(crate::auth::AuthResult::Failure) => {
                    // 认证失败：记录失败
                    rate_limiter.record_failure(username).await;

                    // 认证失败
                    audit_log.log_auth_failure(username, 0, "invalid_password");

                    // 记录认证统计
                    stats_manager.record_auth_failure("password");

                    // 发送认证失败响应
                    send_auth_response(&mut auth_send, auth_envelope.request_id, false, Some("用户名或密码错误"), None).await?;

                    tracing::warn!("❌ 密码认证失败: remote={}, username={}", remote, username);
                    connection.close(0u32.into(), b"authentication failed");
                    return Ok(());
                }
                Err(e) => {
                    // 记录失败（系统错误也算失败）
                    rate_limiter.record_failure(username).await;

                    // 记录详细日志
                    tracing::error!("密码认证系统错误: remote={}, username={}, error={}", remote, username, e);
                    audit_log.log_auth_failure(username, 0, "auth_error");

                    // 记录认证统计
                    stats_manager.record_auth_failure("password");

                    // 返回通用错误（不暴露细节）
                    send_auth_response(&mut auth_send, auth_envelope.request_id, false, Some("认证服务暂时不可用"), None).await?;
                    connection.close(0u32.into(), b"authentication failed");
                    return Ok(());
                }
            }
        }

        // ========== 公钥认证（SSH）==========
        Payload::AuthPubKeyRequest { username, public_key } => {
            tracing::info!(
                "🔐 收到公钥认证请求: remote={}, username={}, pubkey_len={}",
                remote,
                username,
                public_key.len()
            );

            // 1. 检查IP速率限制
            if !rate_limiter.is_ip_allowed(&client_ip).await? {
                tracing::warn!(
                    "IP速率限制触发: ip={}, username={}",
                    client_ip,
                    username
                );
                audit_log.log_auth_failure(username, 0, "ip_rate_limited");
                stats_manager.record_rate_limited();

                send_auth_response(
                    &mut auth_send,
                    auth_envelope.request_id,
                    false,
                    Some("请求过于频繁，请稍后再试"),
                    None,
                ).await?;

                connection.close(0u32.into(), b"rate limited");
                return Ok(());
            }

            // 2. 检查用户名锁定状态
            if !rate_limiter.is_username_allowed(username).await? {
                tracing::warn!(
                    "用户名被锁定: username={}, remote={}",
                    username,
                    remote
                );
                audit_log.log_auth_failure(username, 0, "account_locked");
                stats_manager.record_account_locked();

                send_auth_response(
                    &mut auth_send,
                    auth_envelope.request_id,
                    false,
                    Some("账户暂时锁定，请15分钟后再试"),
                    None,
                ).await?;

                connection.close(0u32.into(), b"account locked");
                return Ok(());
            }

            // ⚠️ 安全检查：验证公钥是否在用户的 authorized_keys 中
            let ssh_auth = crate::auth::ssh::SshAuthenticator::new();
            let pubkey_authorized = match ssh_auth.check_pubkey_in_authorized_keys(&username, &public_key) {
                Ok(authorized) => authorized,
                Err(e) => {
                    tracing::error!("检查公钥授权失败: username={}, error={}", username, e);
                    audit_log.log_auth_failure(&username, 0, "pubkey_check_error");

                    send_auth_response(
                        &mut auth_send,
                        auth_envelope.request_id,
                        false,
                        Some("认证服务暂时不可用"),
                        None,
                    ).await?;

                    connection.close(0u32.into(), b"authentication failed");
                    return Ok(());
                }
            };

            if !pubkey_authorized {
                tracing::warn!(
                    "🚫 公钥未授权: remote={}, username={}, pubkey_len={}",
                    remote,
                    username,
                    public_key.len()
                );
                audit_log.log_auth_failure(&username, 0, "pubkey_not_authorized");

                // 记录失败（公钥未授权也算失败）
                rate_limiter.record_failure(username).await;

                // 记录认证统计
                stats_manager.record_auth_failure("pubkey");

                send_auth_response(
                    &mut auth_send,
                    auth_envelope.request_id,
                    false,
                    Some("公钥未授权"),
                    None,
                ).await?;

                connection.close(0u32.into(), b"authentication failed");
                return Ok(());
            }

            tracing::info!("✅ 公钥已授权: username={}", username);

            // 第二步：生成挑战
            let (challenge_id, challenge_data) = challenge_manager
                .generate_challenge(username.clone(), public_key.clone())
                .await?;

            tracing::debug!(
                "生成公钥认证挑战: username={}, challenge_id={}, challenge_len={}",
                username,
                challenge_id,
                challenge_data.len()
            );

            // 发送挑战给客户端
            let challenge_payload = Envelope::new(
                auth_envelope.request_id,
                Payload::AuthPubKeyChallenge {
                    challenge: challenge_data,
                    challenge_id: challenge_id.clone(),
                },
            );

            match challenge_payload.encode() {
                Ok(challenge_bytes) => {
                    if let Err(e) = write_message(&mut auth_send, &challenge_bytes).await {
                        tracing::error!("发送公钥认证挑战失败: {}", e);
                        connection.close(0u32.into(), b"challenge send failed");
                        return Ok(());
                    }
                }
                Err(e) => {
                    tracing::error!("编码公钥认证挑战失败: {}", e);
                    connection.close(0u32.into(), b"challenge encode failed");
                    return Ok(());
                }
            }

            tracing::info!(
                "✅ 公钥认证挑战已发送: username={}, challenge_id={}",
                username,
                challenge_id
            );

            // 第二步：等待客户端的响应 Stream
            let response_stream = connection.accept_bi().await;
            match response_stream {
                Ok((mut resp_send, mut resp_recv)) => {
                    // 读取响应数据
                    let resp_data = read_message(&mut resp_recv).await?;
                    if resp_data.is_none() {
                        tracing::warn!("公钥认证响应流已关闭: remote={}", remote);
                        connection.close(0u32.into(), b"response stream closed");
                        return Ok(());
                    }

                    let resp_data = resp_data.unwrap();
                    let resp_envelope = Envelope::decode(&resp_data).map_err(|e| anyhow::anyhow!(e))?;

                    // 处理公钥认证响应
                    match resp_envelope.payload {
                        Payload::AuthPubKeyResponse {
                            challenge_id: resp_challenge_id,
                            signature,
                            public_key: resp_public_key,
                        } => {
                            tracing::info!(
                                "📥 收到公钥认证响应: challenge_id={}, signature_len={}, pubkey_len={}",
                                resp_challenge_id,
                                signature.len(),
                                resp_public_key.len()
                            );

                            // 验证挑战-响应并验证签名
                            match challenge_manager
                                .verify_response(&resp_challenge_id, &signature, &resp_public_key)
                                .await
                            {
                                Ok((verified_username, challenge_data, expected_public_key)) => {
                                    tracing::info!(
                                        "公钥挑战验证成功: username={}, challenge_id={}",
                                        verified_username,
                                        resp_challenge_id
                                    );

                                    // 验证公钥匹配
                                    if expected_public_key != resp_public_key {
                                        tracing::warn!(
                                            "公钥不匹配: remote={}, challenge_id={}",
                                            remote, resp_challenge_id
                                        );
                                        audit_log.log_auth_failure(&verified_username, 0, "pubkey_mismatch");

                                        // 记录失败
                                        rate_limiter.record_failure(&verified_username).await;

                                        // 记录认证统计
                                        stats_manager.record_auth_failure("pubkey");

                                        send_auth_response(
                                            &mut resp_send,
                                            resp_envelope.request_id,
                                            false,
                                            Some("公钥验证失败"),
                                            None,
                                        ).await?;

                                        connection.close(0u32.into(), b"authentication failed");
                                        return Ok(());
                                    }

                                    // 使用SshAuthenticator验证签名
                                    let ssh_auth = crate::auth::ssh::SshAuthenticator::new();
                                    match ssh_auth.verify_signature(&resp_public_key, &signature, &challenge_data) {
                                        Ok(true) => {
                                            tracing::debug!("签名验证成功: challenge_id={}", resp_challenge_id);

                                            // 认证成功：清除失败记录
                                            rate_limiter.clear_failures(&verified_username).await;

                                            // 获取真实的用户信息
                                            let user_info = match crate::auth::get_user_info(&verified_username) {
                                                Ok(info) => info,
                                                Err(e) => {
                                                    tracing::error!("获取用户信息失败: {}", e);
                                                    audit_log.log_auth_failure(&verified_username, 0, "user_info_failed");

                                                    send_auth_response(
                                                        &mut resp_send,
                                                        resp_envelope.request_id,
                                                        false,
                                                        Some("认证服务暂时不可用"),
                                                        None,
                                                    ).await?;

                                                    connection.close(0u32.into(), b"authentication failed");
                                                    return Ok(());
                                                }
                                            };

                                            let session = UserSession::new(user_info);

                                            // 保存会话到连接上下文
                                            ctx.set_session(session.clone()).await;

                                            // 记录审计日志
                                            audit_log.log_auth_success(&session.username, session.uid, "pubkey");

                                            // 记录认证统计
                                            stats_manager.record_auth_success("pubkey");

                                            // 发送认证成功响应
                                            send_auth_response(
                                                &mut resp_send,
                                                resp_envelope.request_id,
                                                true,
                                                None,
                                                Some(&session.session_id),
                                            ).await?;

                                            tracing::info!(
                                                "✅ 公钥认证成功: remote={}, user={}",
                                                remote,
                                                session.username
                                            );

                                            session
                                        }
                                        Ok(false) => {
                                            tracing::warn!(
                                                "签名验证失败: remote={}, challenge_id={}",
                                                remote, resp_challenge_id
                                            );
                                            audit_log.log_auth_failure(&verified_username, 0, "invalid_signature");

                                            // 记录失败
                                            rate_limiter.record_failure(&verified_username).await;

                                            // 记录认证统计
                                            stats_manager.record_auth_failure("pubkey");

                                            send_auth_response(
                                                &mut resp_send,
                                                resp_envelope.request_id,
                                                false,
                                                Some("签名验证失败"),
                                                None,
                                            ).await?;

                                            connection.close(0u32.into(), b"authentication failed");
                                            return Ok(());
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "签名验证错误: remote={}, challenge_id={}, error={}",
                                                remote, resp_challenge_id, e
                                            );
                                            audit_log.log_auth_failure(&verified_username, 0, "signature_error");

                                            // 记录失败
                                            rate_limiter.record_failure(&verified_username).await;

                                            // 记录认证统计
                                            stats_manager.record_auth_failure("pubkey");

                                            send_auth_response(
                                                &mut resp_send,
                                                resp_envelope.request_id,
                                                false,
                                                Some("签名验证失败"),
                                                None,
                                            ).await?;

                                            connection.close(0u32.into(), b"authentication failed");
                                            return Ok(());
                                        }
                                    }
                                }
                                Err(e) => {
                                    // 验证失败
                                    tracing::warn!(
                                        "公钥挑战验证失败: remote={}, challenge_id={}, error={}",
                                        remote,
                                        resp_challenge_id,
                                        e
                                    );
                                    audit_log.log_auth_failure(&username, 0, "pubkey_challenge_failed");

                                    // 记录失败（挑战过期也算失败）
                                    rate_limiter.record_failure(username).await;

                                    // 记录认证统计
                                    stats_manager.record_auth_failure("pubkey");

                                    // 发送认证失败响应
                                    send_auth_response(
                                        &mut resp_send,
                                        resp_envelope.request_id,
                                        false,
                                        Some("公钥验证失败"),
                                        None,
                                    ).await?;

                                    connection.close(0u32.into(), b"authentication failed");
                                    return Ok(());
                                }
                            }
                        }
                        other => {
                            tracing::warn!("期望公钥认证响应，收到: {:?}", other);
                            connection.close(0u32.into(), b"expected pubkey response");
                            return Ok(());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("接受公钥认证响应流失败: {}", e);
                    connection.close(0u32.into(), b"response stream failed");
                    return Ok(());
                }
            }
        }

        other => {
            tracing::warn!("期望认证请求,收到: {:?}", other);
            connection.close(0u32.into(), b"expected auth request");
            return Ok(());
        }
    };

    // 启动定期超时检查任务
    // 每 30 秒检查一次自上次活动是否超过 idle_timeout_secs
    let connection_clone = connection.clone();
    let last_activity_clone = last_activity.clone();
    let ctx_clone = ctx.clone();
    let stats_manager_clone = stats_manager.clone();
    let timeout_check_handle = tokio::spawn(async move {
        let check_interval = Duration::from_secs(30);

        loop {
            tokio::select! {
                _ = &mut timeout_cancel_rx => {
                    tracing::debug!("[TimeoutChecker] 收到取消信号，退出超时检查任务: remote={}", remote);
                    break;
                }
                _ = tokio::time::sleep(check_interval) => {
                    let now = current_timestamp_secs();
                    let last = last_activity_clone.load(Ordering::Relaxed);
                    let idle_secs = now.saturating_sub(last);

                    // 检查连接空闲超时
                    if idle_secs >= idle_timeout_secs {
                        tracing::warn!(
                            "[TimeoutChecker] 连接空闲超时: remote={}, idle={}s, threshold={}s，即将关闭连接",
                            remote, idle_secs, idle_timeout_secs
                        );
                        // 关闭 QUIC 连接，触发 accept_bi 返回错误，退出主循环
                        // 资源清理由主循环退出后统一执行，避免重复清理
                        connection_clone.close(0x01_u32.into(), b"connection idle timeout");
                        break;
                    } else {
                        tracing::debug!(
                            "[TimeoutChecker] 连接活跃: remote={}, idle={}s/{}s",
                            remote, idle_secs, idle_timeout_secs
                        );
                    }

                    // 检查会话超时（24小时不活动）
                    if ctx_clone.is_session_timeout() {
                        tracing::warn!(
                            "[TimeoutChecker] 会话超时（24小时不活动）: remote={}，即将关闭连接",
                            remote
                        );
                        // 记录会话超时统计
                        stats_manager_clone.record_session_timeout();
                        connection_clone.close(0x02_u32.into(), b"session timeout");
                        break;
                    }
                }
            }
        }
    });

    // 主循环：接受新 Stream，每次接受成功后重置活动时间
    while let Ok(stream) = connection.accept_bi().await {
        // 每次接受到新 Stream，重置活动时间
        last_activity.store(current_timestamp_secs(), Ordering::Relaxed);

        let cfg_inner = cfg.clone();
        let subscription_manager_inner = subscription_manager.clone();
        let event_bus_inner = event_bus.clone();
        let ctx_inner = ctx.clone();
        let session_inner = session.clone();
        let stats_manager_inner = stats_manager.clone();  // 克隆 stats_manager
        let audit_log_inner = audit_log.clone();  // 克隆 audit_log
        let last_activity_inner = last_activity.clone();  // 传递 last_activity，事件推送成功时更新
        #[cfg(unix)]
        let manager_inner = manager.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_stream(
                stream,
                &cfg_inner,
                subscription_manager_inner,
                event_bus_inner,
                ctx_inner,
                &session_inner,
                stats_manager_inner,  // 传递 stats_manager 参数
                audit_log_inner,  // 传递 audit_log 参数
                last_activity_inner,  // 传递 last_activity
                #[cfg(unix)]
                manager_inner,
            ).await {
                tracing::warn!("QUIC Stream 处理错误: {}", e);
            }
        });
    }

    // 连接关闭（正常断开或超时），取消超时检查任务并触发清理
    let _ = timeout_cancel_tx.send(());
    timeout_check_handle.abort();

    // 确保资源被清理（无论正常关闭还是超时）
    ctx.shutdown();
    #[cfg(unix)]
    ctx.cleanup(manager.pty_registry(), &subscription_manager).await;
    #[cfg(not(unix))]
    ctx.cleanup(&subscription_manager).await;

    // 记录连接关闭统计（暂时记录为正常关闭）
    // TODO: 将来可以根据超时检查任务的状态来判断关闭原因
    stats_manager.record_connection_closed(ConnectionCloseReason::Normal);

    tracing::info!("QUIC 连接关闭，资源已清理: {}", remote);

    Ok(())
}

async fn handle_stream(
    stream: (SendStream, RecvStream),
    cfg: &AgentConfig,
    subscription_manager: Arc<SubscriptionManager>,
    event_bus: Arc<EventBus>,
    ctx: Arc<ConnectionContext>,
    session: &UserSession,
    stats_manager: Arc<StatsManager>,  // 新增参数：统计管理器
    audit_log: Arc<AuditLogger>,  // 新增参数：审计日志记录器
    last_activity: Arc<AtomicU64>,  // 事件推送成功时更新，防止误判空闲超时
    #[cfg(unix)]
    manager: Arc<crate::manager::Manager>,
) -> Result<()> {
    let (mut send, mut recv) = stream;

    // 生成唯一的 Stream ID
    let stream_id = STREAM_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    tracing::debug!("Stream ID: {}", stream_id);

    // 检查会话是否超时（在处理请求前检查）
    if ctx.is_session_timeout() {
        let idle_secs = current_timestamp_secs() - ctx.session_last_activity.load(Ordering::Relaxed);

        tracing::warn!(
            "会话已超时: username={}, idle_time={}s, stream_id={}",
            session.username,
            idle_secs,
            stream_id
        );

        // 记录会话超时统计
        stats_manager.record_session_timeout();

        // 返回错误响应
        let error_response = Envelope::new(
            0,
            Payload::Error {
                code: 401,
                message: "会话已过期，请重新登录".to_string(),
            },
        );

        // 发送错误响应
        match error_response.encode() {
            Ok(error_bytes) => {
                if let Err(e) = write_message(&mut send, &error_bytes).await {
                    tracing::warn!("发送会话超时错误响应失败: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!("编码会话超时错误响应失败: {}", e);
            }
        }

        // 退出函数（会话已超时）
        return Ok(());
    }

    // 更新会话活动时间（防止会话超时）
    ctx.touch_session();

    // 读取第一条消息（订阅请求）
    let data = read_message(&mut recv).await?;
    if data.is_none() {
        tracing::info!("Stream 关闭: stream_id={}", stream_id);
        return Ok(());
    }

    let data = data.unwrap();
    let envelope = Envelope::decode(&data).map_err(|e| anyhow::anyhow!(e))?;

    // 处理订阅请求
    match &envelope.payload {
        Payload::Subscribe { server_id, types } => {
            tracing::info!("订阅请求: server_id={}, types={}", server_id, types.len());

            // 添加订阅
            let subscribed_types = subscription_manager.subscribe(stream_id, types.clone()).await?;

            // 注册到连接上下文（连接关闭时自动清理）
            ctx.register_stream_id(stream_id).await;

            // 发送确认响应
            let response = Envelope::new(
                envelope.request_id,
                Payload::SubscribeAck {
                    success: true,
                    subscribed_types,
                },
            );
            match response.encode() {
                Ok(resp_bytes) => {
                    if let Err(e) = write_message(&mut send, &resp_bytes).await {
                        tracing::warn!("发送响应失败: {}", e);
                        return Ok(());
                    }
                }
                Err(e) => {
                    tracing::warn!("编码响应失败: {}", e);
                    return Ok(());
                }
            }

            // 订阅 EventBus（用于事件推送）
            let mut event_rx = event_bus.subscribe();
            // 订阅连接关闭信号
            let mut shutdown_rx = ctx.subscribe_shutdown();

            tracing::info!("开始事件推送: stream_id={}", stream_id);

            // 进入事件推送循环（带关闭信号响应）
            loop {
                tokio::select! {
                    result = event_rx.recv() => {
                        match result {
                            Ok(event) => {
                                // 检查是否有订阅者
                                if subscription_manager.has_subscribers(&event.event_type).await {
                                    // 发送事件
                                    let event_envelope = Envelope::new(
                                        0, // 事件推送不需要 request_id
                                        Payload::Event {
                                            event_type: event.event_type.clone(),
                                            data: event.data.clone(),
                                            timestamp: event.timestamp,
                                        },
                                    );

                                    match event_envelope.encode() {
                                        Ok(resp_bytes) => {
                                            if let Err(e) = write_message(&mut send, &resp_bytes).await {
                                                tracing::warn!("事件推送失败: {}", e);
                                                break;
                                            }
                                            // 事件推送成功说明连接活跃，更新 last_activity 防止误判空闲超时
                                            last_activity.store(current_timestamp_secs(), Ordering::Relaxed);
                                            tracing::debug!("事件推送成功: event_type={}", event.event_type);
                                        }
                                        Err(e) => {
                                            tracing::warn!("编码事件失败: {}", e);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("EventBus 接收失败: {}", e);
                                break;
                            }
                        }
                    }
                    // 收到连接关闭信号，立即退出推送循环
                    _ = shutdown_rx.recv() => {
                        tracing::info!("收到关闭信号，停止事件推送: stream_id={}", stream_id);
                        break;
                    }
                }
            }

            // 移除订阅
            subscription_manager.remove_all(stream_id).await?;
            tracing::info!("停止事件推送: stream_id={}", stream_id);
        }

        #[cfg(unix)]
        Payload::TerminalSpawnRequest { shell, cols, rows, working_directory } => {
            tracing::info!("终端创建请求: shell={}, cols={}, rows={}, cwd={:?}, user={}",
                shell, cols, rows, working_directory, session.username);

            // 新架构:通过 Worker 创建 Session 进程(openpty+fork),Session 进程持有 master_fd
            // Manager 通过 UnixSocket 帧协议与 Session 进程通信(不再使用 SCM_RIGHTS)
            let session_id = match manager.create_pty_session(
                &shell, *cols as u32, *rows as u32, working_directory.as_deref(), session
            ).await {
                Ok(id) => id,
                Err(e) => {
                    // 创建失败时发送错误响应给客户端，避免 stream 被直接关闭
                    // 导致客户端收到 "stream finished early (0 bytes read)" 错误
                    tracing::error!("创建 PTY 会话失败: {}", e);
                    let error_response = Envelope::new(
                        envelope.request_id,
                        Payload::Error {
                            code: -1,
                            message: format!("创建终端会话失败: {}", e),
                        },
                    );
                    if let Ok(resp_bytes) = error_response.encode() {
                        let _ = write_message(&mut send, &resp_bytes).await;
                    }
                    return Ok(());
                }
            };

            // 注册到连接上下文（连接关闭时自动清理 PTY 会话）
            ctx.register_pty_session(session_id.clone()).await;

            // 订阅连接关闭信号
            let shutdown_rx = ctx.subscribe_shutdown();

            // 修改：先进入终端双向数据隧道循环（数据接收任务会立即启动）
            // 然后在 handle_terminal_stream 内部发送响应，确保接收任务已就绪
            handle_terminal_stream(
                session_id.clone(),
                send,
                recv,
                manager.pty_registry().clone(),  // PtyRegistry(内部通过 SessionConnection socket 通信)
                envelope.request_id,  // 传递 request_id 用于发送响应
                shutdown_rx,          // 传递关闭信号
                stats_manager.clone(), // 传递统计管理器
            ).await?;

            tracing::info!("终端 Stream 结束: session_id={}", session_id);
        }

        #[cfg(not(unix))]
        Payload::TerminalSpawnRequest { .. } => {
            tracing::warn!("终端功能仅支持 Unix 平台");
            let response = Envelope::new(
                envelope.request_id,
                Payload::Error { code: -1, message: "终端功能仅支持 Unix 平台".to_string() },
            );
            if let Ok(resp_bytes) = response.encode() {
                write_message(&mut send, &resp_bytes).await?;
            }
        }

        #[cfg(unix)]
        Payload::TerminalData { session_id, data, is_input } => {
            // 单条终端数据消息（用于非持久连接）
            if *is_input {
                // 通过 PtyRegistry 写入(内部走 SessionConnection socket)
                manager.pty_registry().write(&session_id, &data).await?;
            } else {
                // 输出数据不应该从客户端发送
                tracing::warn!("收到意外的终端输出数据: session_id={}", session_id);
            }

            // 发送空响应
            let response = Envelope::new(envelope.request_id, Payload::TerminalSpawnResponse { session_id: session_id.clone() });
            if let Ok(resp_bytes) = response.encode() {
                write_message(&mut send, &resp_bytes).await?;
            }
        }

        #[cfg(not(unix))]
        Payload::TerminalData { .. } => {
            tracing::warn!("终端功能仅支持 Unix 平台");
            let response = Envelope::new(
                envelope.request_id,
                Payload::Error { code: -1, message: "终端功能仅支持 Unix 平台".to_string() },
            );
            if let Ok(resp_bytes) = response.encode() {
                write_message(&mut send, &resp_bytes).await?;
            }
        }

        #[cfg(unix)]
        Payload::TerminalResizeRequest { session_id, cols, rows } => {
            // 调整远程 PTY 大小(通过 PtyRegistry 发送 RESIZE 帧给 Session 进程)
            match manager.pty_registry().resize(&session_id, *cols, *rows).await {
                Ok(()) => {
                    tracing::info!("PTY resize 成功: session_id={}, {}x{}", session_id, cols, rows);
                    let response = Envelope::new(envelope.request_id, Payload::TerminalResizeResponse);
                    if let Ok(resp_bytes) = response.encode() {
                        write_message(&mut send, &resp_bytes).await?;
                    }
                }
                Err(e) => {
                    tracing::warn!("PTY resize 失败: {}", e);
                    let response = Envelope::new(
                        envelope.request_id,
                        Payload::Error { code: -1, message: format!("resize 失败: {}", e) },
                    );
                    if let Ok(resp_bytes) = response.encode() {
                        write_message(&mut send, &resp_bytes).await?;
                    }
                }
            }
        }

        // 文件传输请求
        Payload::FileTransferRequest { direction, path, file_size, chunk_size, resume_from, frame_mode, stream_count } => {
            tracing::info!("文件传输请求: direction={:?}, path={}, resume_from={:?}, frame_mode={}, stream_count={:?}", direction, path, resume_from, frame_mode, stream_count);

            match crate::handler::handle_file_transfer_request(envelope.request_id, direction, path, *file_size, *chunk_size, *resume_from, cfg, session, audit_log.clone(), frame_mode, *stream_count).await {
                Ok(response) => {
                    // 注册传输会话到连接上下文（连接关闭时自动清理）
                    if let Payload::FileTransferAccept { ref session_id, .. } = response.payload {
                        ctx.register_transfer_session(session_id.clone()).await;
                    }

                    // 发送 FileTransferAccept 响应
                    match response.encode() {
                        Ok(resp_bytes) => {
                            if let Err(e) = write_message(&mut send, &resp_bytes).await {
                                tracing::warn!("发送响应失败: {}", e);
                                return Ok(());
                            }
                        }
                        Err(e) => {
                            tracing::warn!("编码响应失败: {}", e);
                            return Ok(());
                        }
                    }

                    // 如果是下载，启动异步发送任务
                    if direction == "download" {
                        // 获取 session_id 与协商的 frame_mode
                        if let Payload::FileTransferAccept { session_id, file_size, chunk_size, frame_mode, .. } = response.payload {
                            handle_file_download_stream(
                                session_id,
                                send,
                                path.to_string(),
                                file_size,
                                chunk_size,
                                stats_manager.clone(),
                                audit_log.clone(),
                                session.username.clone(),
                                session.uid,
                                frame_mode,
                            ).await?;
                        }
                    } else {
                        // 上传：等待客户端发送 FileChunk
                        // 注意：上传需要从 response 中获取 session_id 与 frame_mode
                        if let Payload::FileTransferAccept { session_id, file_size, frame_mode, .. } = response.payload {
                            handle_file_upload_stream(
                                recv,
                                send,
                                session_id,
                                path.to_string(),
                                file_size,
                                audit_log.clone(),
                                session,
                                frame_mode,
                            ).await?;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("文件传输请求失败: {}", e);
                    let response = Envelope::new(
                        envelope.request_id,
                        Payload::Error { code: -1, message: e.to_string() },
                    );
                    if let Ok(resp_bytes) = response.encode() {
                        write_message(&mut send, &resp_bytes).await?;
                    }
                }
            }
        }

        // 处理断开连接请求（客户端主动断开）
        Payload::DisconnectRequest {} => {
            tracing::info!("收到客户端断开连接请求，清理所有资源...");

            // 1. 触发关闭信号（通知所有订阅推送任务停止）
            ctx.shutdown();

            // 2. 清理传输会话
            ctx.cleanup_transfer_sessions().await;

            // 3. 发送确认响应
            let response = Envelope::new(
                envelope.request_id,
                Payload::DisconnectResponse { success: true },
            );
            match response.encode() {
                Ok(resp_bytes) => {
                    if let Err(e) = write_message(&mut send, &resp_bytes).await {
                        tracing::warn!("发送断开响应失败: {}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("编码断开响应失败: {}", e);
                }
            }

            tracing::info!("客户端断开连接处理完成");
        }

        // 多流加入握手（非主 stream 的第一个控制帧）
        // 客户端在主 stream 的 FileTransferAccept 后，为其余 N-1 个 stream 各 open_bi 发送此 payload
        Payload::MultiStreamJoin { session_id, stream_index, offset_start, offset_end } => {
            tracing::info!(
                "多流加入请求: session_id={}, stream_index={}, offset=[{},{}), stream_id={}",
                session_id, stream_index, offset_start, offset_end, stream_id
            );

            match crate::handler::handle_multi_stream_join(
                session_id,
                *stream_index,
                *offset_start,
                *offset_end,
                session,
            ).await {
                Ok((file_size, chunk_size, _frame_mode)) => {
                    // 发送 ACK（复用 FileTransferAccept，客户端据此确认 join 成功）
                    let ack = Envelope::new(
                        envelope.request_id,
                        Payload::FileTransferAccept {
                            session_id: session_id.clone(),
                            file_size,
                            chunk_size,
                            mtime: None,
                            frame_mode: "raw".to_string(),
                            stream_count: 1, // 非 0 表示此 stream 自己的 stream_count=1（本 stream 独立）
                        },
                    );
                    match ack.encode() {
                        Ok(resp_bytes) => {
                            if let Err(e) = write_message(&mut send, &resp_bytes).await {
                                tracing::warn!("发送多流加入 ACK 失败: {}", e);
                                return Ok(());
                            }
                        }
                        Err(e) => {
                            tracing::warn!("编码多流加入 ACK 失败: {}", e);
                            return Ok(());
                        }
                    }

                    // 从全局会话管理器取走 part writer（由 handle_multi_stream_join 注册）
                    let part_writer = {
                        let mut sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
                        match sessions.get_mut(session_id) {
                            Some(s) => s.part_writers.remove(stream_index)
                                .ok_or_else(|| anyhow::anyhow!(
                                    "part writer 不存在: session_id={}, stream_index={}",
                                    session_id, stream_index
                                ))?,
                            None => return Err(anyhow::anyhow!(
                                "会话不存在: {}", session_id
                            )),
                        }
                    };

                    // 进入 part-upload 循环（接收 chunk → write → finish → mark completed）
                    handle_multi_stream_part_upload(
                        recv,
                        part_writer,
                        session_id.clone(),
                        *stream_index,
                        file_size,
                        audit_log.clone(),
                        session.username.clone(),
                        session.uid,
                    ).await?;
                }
                Err(e) => {
                    tracing::error!("多流加入失败: session_id={}, stream_index={}, err={}", session_id, stream_index, e);
                    let response = Envelope::new(
                        envelope.request_id,
                        Payload::Error { code: -1, message: e },
                    );
                    if let Ok(resp_bytes) = response.encode() {
                        let _ = write_message(&mut send, &resp_bytes).await;
                    }
                }
            }
        }

        _ => {
            // 阶段 3:优先尝试路由到 Worker(ReadDir/ReadFile/WriteFile 等)
            // Worker 不处理的请求返回 None,回退到本地 handler
            #[cfg(unix)]
            {
                match manager.route_to_worker(&envelope.payload, session).await {
                    Ok(Some(worker_payload)) => {
                        // Worker 已处理,构造响应并发送
                        let response = Envelope::new(envelope.request_id, worker_payload);
                        match response.encode() {
                            Ok(resp_bytes) => {
                                if let Err(e) = write_message(&mut send, &resp_bytes).await {
                                    tracing::warn!("发送 Worker 响应失败: {}", e);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("编码 Worker 响应失败: {}", e);
                            }
                        }
                        return Ok(());
                    }
                    Ok(None) => {
                        // 该请求不需要路由到 Worker,回退到本地 handler
                        tracing::debug!("请求不经 Worker 路由,使用本地 handler: {:?}", envelope.payload.type_name());
                    }
                    Err(e) => {
                        // Worker 路由失败,返回错误响应
                        // 使用 {:?} 打印完整错误链,定位底层原因
                        tracing::error!("Worker 路由失败: {:?}", e);
                        let response = Envelope::new(
                            envelope.request_id,
                            Payload::Error {
                                code: -1,
                                message: format!("Worker 路由失败: {:?}", e),
                            },
                        );
                        match response.encode() {
                            Ok(resp_bytes) => {
                                if let Err(e) = write_message(&mut send, &resp_bytes).await {
                                    tracing::warn!("发送错误响应失败: {}", e);
                                }
                            }
                            Err(e) => {
                                tracing::warn!("编码错误响应失败: {}", e);
                            }
                        }
                        return Ok(());
                    }
                }
            }

            // 本地 handler 处理(不经 Worker 的请求,或非 Unix 平台)
            let response = crate::handler::handle_envelope(&envelope, cfg, session, stats_manager.clone(), audit_log.clone()).await;
            match response.encode() {
                Ok(resp_bytes) => {
                    if let Err(e) = write_message(&mut send, &resp_bytes).await {
                        tracing::warn!("发送响应失败: {}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("编码响应失败: {}", e);
                }
            }
        }
    }

    Ok(())
}

async fn read_message(recv: &mut RecvStream) -> Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match recv.read_exact(&mut len_buf).await {
        Ok(()) => {
            let len = u32::from_le_bytes(len_buf) as usize;
            if len == 0 || len > 10 * 1024 * 1024 {
                tracing::warn!("[QUIC] 无效消息长度: {}", len);
                anyhow::bail!("无效的消息长度: {}", len);
            }
            let mut data = vec![0u8; len];
            recv.read_exact(&mut data).await
                .map_err(|e| {
                    tracing::warn!("[QUIC] 读取消息失败: {}", e);
                    e
                })?;
            Ok(Some(data))
        }
        Err(quinn::ReadExactError::FinishedEarly(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

async fn write_message(send: &mut SendStream, data: &[u8]) -> Result<()> {
    let len = (data.len() as u32).to_le_bytes();
    send.write_all(&len).await
        .map_err(|e| {
            tracing::warn!("[QUIC] 写入消息失败: {}", e);
            e
        })?;
    send.write_all(data).await
        .map_err(|e| {
            tracing::warn!("[QUIC] 写入消息失败: {}", e);
            e
        })?;
    Ok(())
}

/// 发送认证响应
async fn send_auth_response(
    send: &mut SendStream,
    request_id: u32,
    success: bool,
    error: Option<&str>,
    session_id: Option<&str>,
) -> Result<()> {
    let response = Envelope::new(
        request_id,
        Payload::AuthResponse {
            success,
            error: error.map(|s| s.to_string()),
            session_id: session_id.map(|s| s.to_string()),
        },
    );

    let resp_bytes = response.encode().map_err(|e| anyhow::anyhow!(e))?;
    write_message(send, &resp_bytes).await?;

    Ok(())
}

/// 处理终端持久 Stream（双向数据隧道）
///
/// 新架构:PTY 读写通过 PtyRegistry(内部走 SessionConnection UnixSocket 帧协议),
/// 不再直接操作 master_fd,也不再需要 Manager 引用。
#[cfg(unix)]
async fn handle_terminal_stream(
    session_id: String,
    send: SendStream,
    mut recv: RecvStream,
    pty_registry: Arc<crate::manager::PtyRegistry>,  // PtyRegistry(内部通过 SessionConnection socket 通信)
    request_id: u32,  // 用于发送响应
    shutdown_rx: broadcast::Receiver<()>,  // 连接关闭信号
    stats_manager: Arc<StatsManager>,  // 统计管理器
) -> Result<()> {
    tracing::info!("终端双向隧道启动: session_id={}", session_id);

    // 使用 Arc 包装 send，让两个任务都能访问
    let send = Arc::new(tokio::sync::Mutex::new(send));

    // ── 关键修改：先启动客户端输入读取任务，确保数据接收通道就绪 ────
    // 这样可以避免客户端发送的早期输入数据丢失
    let session_id_clone = session_id.clone();
    let pty_registry_clone = pty_registry.clone();
    let (client_read_started_tx, client_read_started_rx) = tokio::sync::oneshot::channel();

    let client_read_task = tokio::spawn(async move {
        // 立即通知主任务：客户端读取任务已启动
        let _ = client_read_started_tx.send(());

        loop {
            // 从客户端读取输入
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
                            // 写入 PTY(阶段 2:通过 PtyRegistry 写入)
                            if let Err(e) = pty_registry_clone.write(&session_id_clone, &data).await {
                                tracing::warn!("写入 PTY 失败: {}", e);
                                break;
                            }
                            tracing::debug!("客户端输入写入 PTY: len={}", len);
                        }
                        Err(e) => {
                            tracing::warn!("读取客户端数据失败: {}", e);
                            break;
                        }
                    }
                }
                Err(quinn::ReadExactError::FinishedEarly(_)) => {
                    tracing::info!("客户端关闭发送端: session_id={}", session_id_clone);
                    break;
                }
                Err(e) => {
                    tracing::warn!("读取客户端长度失败: {}", e);
                    break;
                }
            }
        }
        tracing::info!("客户端读取任务结束: session_id={}", session_id_clone);
    });

    // 等待客户端读取任务启动（确保接收通道就绪）
    client_read_started_rx.await?;

    // 发送响应给客户端（此时数据接收任务已经就绪）
    let response = Envelope::new(
        request_id,
        Payload::TerminalSpawnResponse { session_id: session_id.clone() },
    );
    match response.encode() {
        Ok(resp_bytes) => {
            let mut send_guard = send.lock().await;
            if let Err(e) = write_message(&mut send_guard, &resp_bytes).await {
                tracing::warn!("发送终端响应失败: {}", e);
                drop(send_guard);
                client_read_task.abort();
                return Ok(());
            }
        }
        Err(e) => {
            tracing::warn!("编码终端响应失败: {}", e);
            client_read_task.abort();
            return Ok(());
        }
    }

    tracing::info!("终端会话创建成功，响应已发送: session_id={}", session_id);

    // 启动 PTY 输出推送任务
    // 新架构:从 Session 进程 UnixSocket 读取 PTY 输出,批量发送到客户端
    let config = PtyOutputConfig::default();
    let pty_read_task = spawn_pty_output_task_v2(
        pty_registry.clone(),
        session_id.clone(),
        send.clone(),
        Some(stats_manager.clone()),
        config,
    ).await;

    // 订阅连接关闭信号（需要在 select! 之前可变绑定）
    let mut shutdown_rx = shutdown_rx;

    // 使用 pin! 宏固定 JoinHandle，这样可以在 select! 中使用可变引用
    tokio::pin!(pty_read_task);
    tokio::pin!(client_read_task);

    // 等待任一任务结束或收到关闭信号
    tokio::select! {
        _ = &mut pty_read_task => {
            tracing::info!("PTY 读取任务先结束: session_id={}", session_id);
        }
        _ = &mut client_read_task => {
            tracing::info!("客户端读取任务先结束: session_id={}", session_id);
        }
        // 收到连接关闭信号，强制终止双向隧道
        _ = shutdown_rx.recv() => {
            tracing::info!("收到关闭信号，强制终止终端会话: session_id={}", session_id);
            pty_read_task.abort();
            client_read_task.abort();
        }
    }

    // 清理 PTY 会话(通过 PtyRegistry 注销,内部发送 Close 帧给 Session 进程)
    pty_registry.unregister(&session_id).await?;

    Ok(())
}
// ✅ 优化: 删除Windows平台的stub实现,因为终端功能仅支持Unix
// #[cfg(not(unix))] 的 handle_terminal_stream 已删除

/// 处理文件下载流（发送文件数据）
async fn handle_file_download_stream(
    session_id: String,
    mut send: SendStream,
    path: String,
    file_size: u64,
    _chunk_size: u32,
    stats_manager: Arc<StatsManager>,
    audit_log: Arc<AuditLogger>,
    username: String,
    uid: u32,
    frame_mode: String,
) -> Result<()> {
    use crate::protocol::raw_frame;
    let raw_mode = frame_mode == "raw";
    tracing::info!("开始发送文件: session_id={}, path={}, size={}, frame_mode={}", session_id, path, file_size, frame_mode);

    // 从全局会话管理器获取 session 并取出 reader
    let mut sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
    let session = sessions.get_mut(&session_id)
        .ok_or_else(|| anyhow::anyhow!("会话不存在: {}", session_id))?;

    let mut reader = session.reader.take()
        .ok_or_else(|| anyhow::anyhow!("文件读取器不存在"))?;

    drop(sessions);  // 释放锁

    let mut seq = 1u32;
    let mut download_success = true;

    // 读取并发送文件块
    loop {
        let chunk = match reader.read_next_chunk() {
            Ok(Some(c)) => c,
            Ok(None) => break,  // EOF
            Err(e) => {
                tracing::error!("读取文件块失败: {}", e);
                download_success = false;
                break;
            }
        };

        if raw_mode {
            // ===== 裸帧模式: 直接写二进制数据帧 =====
            let raw_chunk = raw_frame::RawChunk { seq, data: chunk.clone() };
            if let Err(e) = raw_frame::write_data_chunk(&mut send, &raw_chunk).await {
                tracing::error!("发送文件块失败(裸帧): {}", e);
                download_success = false;
                break;
            }
        } else {
            // ===== JSON 模式(旧客户端回退) =====
            let chunk_payload = Payload::FileChunk {
                session_id: session_id.clone(),
                seq,
                data: chunk.clone(),
                size: chunk.len() as u32,
            };

            let chunk_envelope = Envelope::new(0, chunk_payload);  // request_id 不重要
            let chunk_bytes = chunk_envelope.encode().map_err(|e| anyhow::anyhow!("{}", e))?;

            if let Err(e) = write_message(&mut send, &chunk_bytes).await {
                tracing::error!("发送文件块失败: {}", e);
                download_success = false;
                break;
            }
        }

        // 记录文件传输字节数
        stats_manager.record_file_transfer_bytes(chunk.len() as u64);

        seq += 1;
        tracing::debug!("已发送块: seq={}, size={}", seq-1, chunk.len());
    }

    // 发送 FileTransferComplete
    if raw_mode {
        // ===== 裸帧模式: 控制帧承载 JSON Envelope =====
        let complete_payload = Payload::FileTransferComplete {
            session_id: session_id.clone(),
            success: download_success,
            mtime: None,
            error: None,
        };
        let complete_envelope = Envelope::new(0, complete_payload);
        let complete_bytes = complete_envelope.encode().map_err(|e| anyhow::anyhow!("{}", e))?;
        raw_frame::write_control_frame(&mut send, &complete_bytes).await?;
    } else {
        // ===== JSON 模式 =====
        let complete_payload = Payload::FileTransferComplete {
            session_id: session_id.clone(),
            success: download_success,
            mtime: None,
            error: None,
        };

        let complete_envelope = Envelope::new(0, complete_payload);
        let complete_bytes = complete_envelope.encode().map_err(|e| anyhow::anyhow!("{}", e))?;
        write_message(&mut send, &complete_bytes).await?;
    }

    // 审计日志：记录下载完成/失败
    if download_success {
        audit_log.log_file_operation(&username, uid, "download_complete", &path, file_size);
    } else {
        audit_log.log_file_operation(&username, uid, "download_failed", &path, file_size);
    }

    tracing::info!("文件发送完成: session_id={}", session_id);
    Ok(())
}

/// 处理文件上传流（接收文件数据）
///
/// # 临时文件清理保障
///
/// - 正常完成：`writer.finish()` 将临时文件重命名为最终文件
/// - 客户端发送失败标志：`writer.abort()` 删除临时文件
/// - 流中断（客户端断连）：writer 被 drop，其 Drop impl 自动删除临时文件
/// - 会话被移除：从 HashMap 中 remove 后 drop，TransferSession::Drop 自动清理
async fn handle_file_upload_stream(
    mut recv: RecvStream,
    mut send: SendStream,
    session_id: String,
    path: String,
    file_size: u64,
    audit_log: Arc<AuditLogger>,
    user_session: &UserSession,
    frame_mode: String,
) -> Result<()> {
    use crate::protocol::raw_frame;
    let raw_mode = frame_mode == "raw";
    tracing::info!("开始接收文件: session_id={}, path={}, size={}, frame_mode={}", session_id, path, file_size, frame_mode);

    // 从全局会话管理器获取 session 并取出 writer
    let mut sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
    let session = sessions.get_mut(&session_id)
        .ok_or_else(|| anyhow::anyhow!("会话不存在: {}", session_id))?;

    let mut writer = session.writer.take()
        .ok_or_else(|| anyhow::anyhow!("文件写入器不存在"))?;
    let is_multi = session.is_multi_stream();

    drop(sessions);  // 释放锁

    let mut upload_success = false;

    // 接收文件块
    loop {
        if raw_mode {
            // ===== 裸帧模式: [4B len][1B type][body] =====
            let (type_byte, body_len) = match raw_frame::read_frame(&mut recv).await {
                Ok(t) => t,
                Err(e) => {
                    // 流中断(客户端断连)— writer 的 Drop impl 会自动清理临时文件
                    tracing::warn!(
                        "文件上传流中断(裸帧读失败): session_id={}, path={}, 已传输: {}/{}, err: {}",
                        session_id, path, writer.transferred(), file_size, e
                    );
                    break;
                }
            };
            match type_byte {
                raw_frame::TYPE_DATA => {
                    let chunk = raw_frame::read_data_chunk_body(&mut recv, body_len).await?;
                    writer.write_chunk(&chunk.data).map_err(|e| anyhow::anyhow!("{}", e))?;
                }
                raw_frame::TYPE_CONTROL => {
                    let body = raw_frame::read_control_body(&mut recv, body_len).await?;
                    let env = Envelope::decode(&body).map_err(|e| anyhow::anyhow!("{}", e))?;
                    match env.payload {
                        Payload::FileTransferComplete { success, error, .. } => {
                            if success {
                                let finish_result = tokio::task::spawn_blocking(move || writer.finish())
                                    .await
                                    .map_err(|e| anyhow::anyhow!("finish 任务 panic: {}", e))?;
                                finish_result.map_err(|e| anyhow::anyhow!("{}", e))?;
                                upload_success = true;
                                tracing::info!("文件接收完成(裸帧): session_id={}, path={}", session_id, path);
                            } else {
                                tokio::task::spawn_blocking(move || writer.abort())
                                    .await
                                    .map_err(|e| anyhow::anyhow!("abort 任务 panic: {}", e))?;
                                tracing::error!("文件传输失败(客户端报告,裸帧): session_id={}, error={:?}", session_id, error);
                            }
                            break;
                        }
                        _ => return Err(anyhow::anyhow!("非预期控制帧 payload")),
                    }
                }
                _ => return Err(anyhow::anyhow!("未知帧类型: {}", type_byte)),
            }
        } else {
            // ===== JSON 模式(旧客户端回退) =====
            let data = read_message(&mut recv).await?;
            if data.is_none() {
                // 流中断（客户端断连）— writer 的 Drop impl 会自动清理临时文件
                tracing::warn!(
                    "文件上传流中断（客户端断连）: session_id={}, path={}, 已传输: {}/{}",
                    session_id, path, writer.transferred(), file_size
                );
                break;
            }

            let data = data.unwrap();
            let envelope = Envelope::decode(&data).map_err(|e| anyhow::anyhow!("{}", e))?;

            match envelope.payload {
                Payload::FileChunk { data, .. } => {
                    writer.write_chunk(&data).map_err(|e| anyhow::anyhow!("{}", e))?;
                }
                Payload::FileTransferComplete { success, error, .. } => {
                    if success {
                        // spawn_blocking 避免 sync waitpid(flush+sync+rename)阻塞 tokio worker
                        let finish_result = tokio::task::spawn_blocking(move || writer.finish())
                            .await
                            .map_err(|e| anyhow::anyhow!("finish 任务 panic: {}", e))?;
                        finish_result.map_err(|e| anyhow::anyhow!("{}", e))?;
                        upload_success = true;
                        tracing::info!("文件接收完成: session_id={}, path={}", session_id, path);
                    } else {
                        // 客户端报告失败，abort 删除临时文件
                        // spawn_blocking 避免 sync waitpid(kill+waitpid)阻塞 tokio worker
                        tokio::task::spawn_blocking(move || writer.abort())
                            .await
                            .map_err(|e| anyhow::anyhow!("abort 任务 panic: {}", e))?;
                        tracing::error!("文件传输失败（客户端报告）: session_id={}, error={:?}", session_id, error);
                    }
                    break;
                }
                _ => {}
            }
        }
    }

    if is_multi {
        // ===== 多流模式:主 stream 段写入完成 → 轮询等待其他 stream → 合并 → 发送结果 =====

        if upload_success {
            // 标记主 stream (index=0) 完成
            let all_done = {
                let mut sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
                match sessions.get_mut(&session_id) {
                    Some(s) => s.mark_stream_completed(),
                    None => false,
                }
            };

            // 如果不是最后一个完成的 stream，轮询等待其他 stream
            if !all_done {
                tracing::info!(
                    "多流主 stream 完成,等待其他 stream: session_id={}",
                    session_id
                );
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
                    match sessions.get(&session_id) {
                        Some(s) => {
                            if s.status == crate::transfer_session::TransferStatus::Error {
                                tracing::warn!(
                                    "多流其他 stream 失败,中止合并: session_id={}",
                                    session_id
                                );
                                break;
                            }
                            if s.completed_streams >= s.stream_count {
                                tracing::info!(
                                    "多流所有 stream 完成: session_id={}",
                                    session_id
                                );
                                break;
                            }
                        }
                        None => {
                            tracing::warn!(
                                "多流会话不存在(可能被清理): session_id={}",
                                session_id
                            );
                            break;
                        }
                    }
                }
            }

            // 检查合并条件:所有 stream 完成且无错误
            let should_merge = {
                let sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
                match sessions.get(&session_id) {
                    Some(s) => s.status != crate::transfer_session::TransferStatus::Error
                        && s.completed_streams >= s.stream_count,
                    None => false,
                }
            };

            if should_merge {
                // 获取 part_paths(按 stream_index 顺序)和 final_path
                let (part_paths, final_path) = {
                    let sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
                    match sessions.get(&session_id) {
                        Some(s) => (s.part_paths.clone(), s.path.clone()),
                        None => (Vec::new(), String::new()),
                    }
                };

                // 创建 executor 并触发合并(spawn_blocking 避免阻塞 tokio worker)
                let executor = UserExecutor::new(user_session);
                let merge_result = tokio::task::spawn_blocking(move || {
                    let child_pid = executor.spawn_isolated_merger(part_paths, &final_path)?;
                    executor.wait_isolated_child(child_pid)
                })
                .await;

                let (merge_success, merge_error) = match merge_result {
                    Ok(Ok(())) => (true, None),
                    Ok(Err(e)) => (false, Some(format!("{}", e))),
                    Err(e) => (false, Some(format!("merge 任务 panic: {}", e))),
                };

                // 发送 MultiStreamMergeComplete 给客户端(裸帧控制帧)
                let merge_env = Envelope::new(
                    0,
                    Payload::MultiStreamMergeComplete {
                        session_id: session_id.clone(),
                        success: merge_success,
                        error: merge_error,
                    },
                );
                if let Ok(bytes) = merge_env.encode() {
                    if let Err(e) = raw_frame::write_control_frame(&mut send, &bytes).await {
                        tracing::warn!("发送 MultiStreamMergeComplete 失败: {}", e);
                    }
                }

                // 审计
                if merge_success {
                    audit_log.log_file_operation(
                        &user_session.username, user_session.uid,
                        "upload_complete", &path, file_size,
                    );
                } else {
                    audit_log.log_file_operation(
                        &user_session.username, user_session.uid,
                        "upload_merge_failed", &path, file_size,
                    );
                }
            } else {
                // 合并条件不满足(有 stream 失败或会话不存在)
                let merge_env = Envelope::new(
                    0,
                    Payload::MultiStreamMergeComplete {
                        session_id: session_id.clone(),
                        success: false,
                        error: Some("部分段上传失败,合并中止".to_string()),
                    },
                );
                if let Ok(bytes) = merge_env.encode() {
                    let _ = raw_frame::write_control_frame(&mut send, &bytes).await;
                }
                audit_log.log_file_operation(
                    &user_session.username, user_session.uid,
                    "upload_failed", &path, file_size,
                );
            }

            // 移除会话(合并完成后或失败后)
            let mut sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
            if let Some(mut removed_session) = sessions.remove(&session_id) {
                removed_session.mark_completed();
            }
        } else {
            // 主 stream 上传失败:标记会话为错误,通知客户端
            {
                let mut sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
                if let Some(s) = sessions.get_mut(&session_id) {
                    s.status = crate::transfer_session::TransferStatus::Error;
                }
            }

            let merge_env = Envelope::new(
                0,
                Payload::MultiStreamMergeComplete {
                    session_id: session_id.clone(),
                    success: false,
                    error: Some("主 stream 上传失败".to_string()),
                },
            );
            if let Ok(bytes) = merge_env.encode() {
                let _ = raw_frame::write_control_frame(&mut send, &bytes).await;
            }

            audit_log.log_file_operation(
                &user_session.username, user_session.uid,
                "upload_failed", &path, file_size,
            );

            let mut sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
            sessions.remove(&session_id);
        }
    } else {
        // ===== 单流模式(向后兼容) =====
        // 审计日志：记录上传完成/失败
        if upload_success {
            audit_log.log_file_operation(&user_session.username, user_session.uid, "upload_complete", &path, file_size);
        } else {
            audit_log.log_file_operation(&user_session.username, user_session.uid, "upload_failed", &path, file_size);
        }

        // 上传完成后，从全局会话管理器中移除会话，防止内存泄漏
        // 如果流中断，writer 已被 drop（Drop impl 清理临时文件），也需要移除会话
        let mut sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
        if let Some(mut removed_session) = sessions.remove(&session_id) {
            if upload_success {
                removed_session.mark_completed();
                tracing::debug!("已移除完成的上传会话: session_id={}", session_id);
            } else {
                tracing::info!("已移除中断的上传会话: session_id={}", session_id);
                // TransferSession::Drop 会自动清理 writer 中残留的临时文件
            }
        }
    }

    Ok(())
}

/// 处理多流段上传(非主 stream,stream_index 1..N-1)
///
/// 接收客户端发送的段数据块,写入 part_writer,完成后标记段完成。
/// 非主 stream 不负责合并 — 主 stream (handle_file_upload_stream) 负责轮询
/// 所有段完成状态并触发合并(spawn_isolated_merger)。
///
/// # 流程
/// 1. 循环读取裸帧(TYPE_DATA 数据块 / TYPE_CONTROL 控制帧)
/// 2. TYPE_DATA: write_chunk → part_writer(经 pipe 传给隔离子进程)
/// 3. TYPE_CONTROL + FileTransferComplete:
///    - success=true: spawn_blocking(part_writer.finish()) — 子进程 flush+sync 后 _exit(0)
///    - success=false: spawn_blocking(part_writer.abort()) — kill 子进程 + 清理临时段文件
/// 4. 标记段完成(mark_stream_completed)或失败(会话 status=Error)
///
/// # 注意
/// - part_writer 持唯一 pipe FD,不可 Clone,被 move 到 spawn_blocking 内消费
/// - 段失败时标记会话为 Error,主 stream 轮询检测后中止合并
async fn handle_multi_stream_part_upload(
    mut recv: RecvStream,
    mut part_writer: crate::file_stream::PipeFileStreamWriter,
    session_id: String,
    stream_index: u32,
    file_size: u64,
    audit_log: Arc<AuditLogger>,
    username: String,
    uid: u32,
) -> Result<()> {
    use crate::protocol::raw_frame;

    let mut upload_success = false;

    loop {
        let (type_byte, body_len) = match raw_frame::read_frame(&mut recv).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(
                    "多流段上传中断: session_id={}, stream={}, err={}",
                    session_id, stream_index, e
                );
                break;
            }
        };

        match type_byte {
            raw_frame::TYPE_DATA => {
                let chunk = raw_frame::read_data_chunk_body(&mut recv, body_len).await?;
                part_writer.write_chunk(&chunk.data).map_err(|e| anyhow::anyhow!("{}", e))?;
            }
            raw_frame::TYPE_CONTROL => {
                let body = raw_frame::read_control_body(&mut recv, body_len).await?;
                let env = Envelope::decode(&body).map_err(|e| anyhow::anyhow!("{}", e))?;
                match env.payload {
                    Payload::FileTransferComplete { success, error, .. } => {
                        if success {
                            // finish part writer(子进程 flush+sync 后 _exit(0),不 rename)
                            // 不使用 ? 提前返回,确保 post-loop 标记段完成/失败始终执行
                            match tokio::task::spawn_blocking(move || part_writer.finish()).await {
                                Ok(Ok(())) => {
                                    upload_success = true;
                                    tracing::info!(
                                        "多流段完成: session_id={}, stream={}",
                                        session_id, stream_index
                                    );
                                }
                                Ok(Err(e)) => {
                                    tracing::error!(
                                        "多流段 finish 失败: session_id={}, stream={}, err={}",
                                        session_id, stream_index, e
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "多流段 finish panic: session_id={}, stream={}, err={}",
                                        session_id, stream_index, e
                                    );
                                }
                            }
                        } else {
                            if let Err(e) = tokio::task::spawn_blocking(move || part_writer.abort()).await {
                                tracing::error!(
                                    "多流段 abort panic: session_id={}, stream={}, err={}",
                                    session_id, stream_index, e
                                );
                            }
                            tracing::error!(
                                "多流段失败(客户端报告): session_id={}, stream={}, error={:?}",
                                session_id, stream_index, error
                            );
                        }
                        break;
                    }
                    _ => return Err(anyhow::anyhow!("非预期控制帧 payload")),
                }
            }
            _ => return Err(anyhow::anyhow!("未知帧类型: {}", type_byte)),
        }
    }

    // 审计日志
    if upload_success {
        audit_log.log_file_operation(
            &username, uid, "upload_part_complete",
            &format!("stream={}", stream_index), file_size
        );
    } else {
        audit_log.log_file_operation(
            &username, uid, "upload_part_failed",
            &format!("stream={}", stream_index), file_size
        );
    }

    // 标记段完成或失败
    // 非主 stream 不负责合并,主 stream 会轮询 completed_streams 并触发合并
    {
        let mut sessions = crate::handler::TRANSFER_SESSIONS.lock().await;
        if let Some(s) = sessions.get_mut(&session_id) {
            if upload_success {
                s.mark_stream_completed();
            } else {
                // 段失败:标记会话为错误,主 stream 检测后中止合并
                s.status = crate::transfer_session::TransferStatus::Error;
            }
        }
    }

    Ok(())
}