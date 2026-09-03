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

    // 连接目标（域名在本端解析 = 远程 DNS），10 秒超时防止目标不可达时拖死流
    let tcp = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::net::TcpStream::connect((host.as_str(), port)),
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
