//! SOCKS5 代理流中继
//!
//! 代理流协议：首帧 ProxyOpen（envelope，长度前缀 JSON），Agent 回 ProxyOpenResponse，
//! 之后流内全部字节与目标 TCP 连接双向透传（盲转，不理解内容）。
//! 域名在本端解析（远程 DNS，客户端无 DNS 泄漏）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use quinn::{RecvStream, SendStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::auth::session::UserSession;
use quirel_protocol::{Envelope, Payload};

use super::quic::write_message;

/// 处理一条代理流：连接目标并双向透传
///
/// 进入此函数前，调用方已完成 ProxyOpen envelope 解析。
/// QUIC 流关闭（连接断开）时 copy 任务自动退出，TCP 连接随之关闭——
/// 与连接级清理天然联动，无需注册额外清理逻辑。
pub async fn handle_proxy_relay(
    mut send: SendStream,
    mut recv: RecvStream,
    request_id: u32,
    host: String,
    port: u16,
    session: &UserSession,
    activity: &Arc<AtomicU64>,
) -> Result<(), anyhow::Error> {
    tracing::info!(
        "[Proxy] 代理流打开: username={}, target={}:{}, uid={}",
        session.username, host, port, session.uid
    );

    // 连接目标（域名在本端解析 = 远程 DNS），10 秒超时防止目标不可达时拖死流。
    //
    // SSRF 防线（2026-11 审计 H3）：代理以 quireld（通常 root）身份发起
    // 连接，「root 网络视角可达」是本功能的预期语义；但云元数据端点
    // （链路本地地址 169.254.0.0/16 等）可被低权登录用户借道窃取云平台
    // 凭据，属无条件拦截目标。按解析后的实际 IP 判断，防域名绕过。
    let addrs: Vec<std::net::SocketAddr> = match tokio::net::lookup_host((host.as_str(), port)).await {
        Ok(resolved) => {
            let filtered: Vec<std::net::SocketAddr> =
                resolved.into_iter().filter(|a| !is_blocked_metadata_target(a.ip())).collect();
            if filtered.is_empty() {
                let resp = Envelope::new(
                    request_id,
                    Payload::ProxyOpenResponse {
                        success: false,
                        error: Some("目标地址被拒绝（云元数据/链路本地地址不可经代理访问）".to_string()),
                    },
                );
                let resp_bytes = resp.encode().map_err(|e| anyhow::anyhow!(e))?;
                write_message(&mut send, &resp_bytes).await?;
                tracing::warn!("[Proxy] 目标命中元数据拦截: {}:{} (uid={})", host, port, session.uid);
                return Ok(());
            }
            filtered
        }
        Err(e) => {
            // 回报失败，客户端据此回复 SOCKS5 拒绝
            let resp = Envelope::new(
                request_id,
                Payload::ProxyOpenResponse {
                    success: false,
                    error: Some(format!("解析目标失败: {}", e)),
                },
            );
            // encode() 返回 Result<_, String>，手动转为 anyhow
            let resp_bytes = resp.encode().map_err(|e| anyhow::anyhow!(e))?;
            write_message(&mut send, &resp_bytes).await?;
            tracing::warn!("[Proxy] 目标解析失败: {}:{}: {}", host, port, e);
            return Ok(());
        }
    };

    let tcp = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::net::TcpStream::connect(&addrs[..]),
    )
    .await
    {
        Ok(Ok(tcp)) => tcp,
        Ok(Err(e)) => {
            // 回报失败，客户端据此回复 SOCKS5 拒绝
            let resp = Envelope::new(
                request_id,
                Payload::ProxyOpenResponse {
                    success: false,
                    error: Some(format!("连接目标失败: {}", e)),
                },
            );
            // encode() 返回 Result<_, String>，手动转为 anyhow
            let resp_bytes = resp.encode().map_err(|e| anyhow::anyhow!(e))?;
            write_message(&mut send, &resp_bytes).await?;
            tracing::warn!("[Proxy] 目标连接失败: {}:{}: {}", host, port, e);
            return Ok(());
        }
        Err(_) => {
            // 回报失败（超时），客户端据此回复 SOCKS5 拒绝
            let resp = Envelope::new(
                request_id,
                Payload::ProxyOpenResponse {
                    success: false,
                    error: Some("连接目标超时(10s)".to_string()),
                },
            );
            // encode() 返回 Result<_, String>，手动转为 anyhow
            let resp_bytes = resp.encode().map_err(|e| anyhow::anyhow!(e))?;
            write_message(&mut send, &resp_bytes).await?;
            tracing::warn!("[Proxy] 目标连接超时(10s): {}:{}", host, port);
            return Ok(());
        }
    };

    // 回报成功，之后开始透传
    let resp = Envelope::new(
        request_id,
        Payload::ProxyOpenResponse {
            success: true,
            error: None,
        },
    );
    let resp_bytes = resp.encode().map_err(|e| anyhow::anyhow!(e))?;
    write_message(&mut send, &resp_bytes).await?;

    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();
    // 双向透传：join! 让两个方向各自独立跑完（半关闭互不干扰）
    let a = async {
        // 客户端→目标：自定义拷贝循环，每块拷贝后刷新连接活动时间
        let r = copy_with_activity(&mut recv, &mut tcp_tx, activity).await;
        // 客户端侧结束，向目标发 FIN
        let _ = tcp_tx.shutdown().await;
        if let Err(e) = r {
            tracing::debug!("[Proxy] 客户端→目标复制结束: {}", e);
        }
    };
    let b = async {
        let r = copy_with_activity(&mut tcp_rx, &mut send, activity).await;
        // 目标侧结束，结束 QUIC 发送流
        let _ = send.finish();
        if let Err(e) = r {
            tracing::debug!("[Proxy] 目标→客户端复制结束: {}", e);
        }
    };
    tokio::join!(a, b);

    tracing::debug!(
        "[Proxy] 代理流关闭: username={}, target={}:{}",
        session.username, host, port
    );
    Ok(())
}

/// 目标 IP 是否为被拦截的云元数据链路本地地址
///
/// - v4: 169.254.0.0/16（`is_link_local`，覆盖 169.254.169.254 元数据端点）
/// - v6: fe80::/10（`is_unicast_link_local`）；v4-mapped（::ffff: 前缀）
///   形式还原为 v4 后判断，防绕过
fn is_blocked_metadata_target(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_link_local(),
        std::net::IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(mapped) => mapped.is_link_local(),
            None => v6.is_unicast_link_local(),
        },
    }
}

/// 带活动时间刷新的拷贝：每成功拷贝一块就更新 last_activity，
/// 让空闲超时检查器能看到代理流的活跃（否则长下载会在 300s 被整连误杀）
async fn copy_with_activity<R, W>(
    src: &mut R,
    dst: &mut W,
    activity: &Arc<AtomicU64>,
) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = src.read(&mut buf).await?;
        if n == 0 {
            return Ok(()); // EOF
        }
        dst.write_all(&buf[..n]).await?;
        // 秒级时间戳，与 TimeoutChecker 的 last_activity 语义一致
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        activity.store(now, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::is_blocked_metadata_target;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn metadata_link_local_blocked() {
        // 云平台元数据端点（AWS/GCP/Azure 等均为 169.254.169.254）
        assert!(is_blocked_metadata_target(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))));
        assert!(is_blocked_metadata_target(IpAddr::V4(Ipv4Addr::new(169, 254, 0, 1))));
        // v4-mapped v6 形式不得绕过（::ffff:169.254.169.254）
        assert!(is_blocked_metadata_target(IpAddr::V6(
            Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0xa9fe, 0xa9fe)
        )));
        // v6 链路本地
        assert!(is_blocked_metadata_target(IpAddr::V6(
            Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)
        )));
    }

    #[test]
    fn normal_targets_allowed() {
        // 本地/内网/公网目标不受影响（root 网络视角是功能预期语义）
        assert!(!is_blocked_metadata_target(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!is_blocked_metadata_target(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))));
        assert!(!is_blocked_metadata_target(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4))));
        // 169.255.x.x 不在 169.254/16 内
        assert!(!is_blocked_metadata_target(IpAddr::V4(Ipv4Addr::new(169, 255, 1, 1))));
        assert!(!is_blocked_metadata_target(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    }
}
