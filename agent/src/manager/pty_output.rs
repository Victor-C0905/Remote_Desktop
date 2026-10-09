//! PTY 输出推送模块
//!
//! 从 Session 进程接收 PTY 输出（通过 UnixSocket 帧协议），批量发送给客户端。
//!
//! # 架构
//!
//! ```text
//! Session 进程 (持有 master_fd)
//!     ↓ PTY_OUTPUT 帧 (UnixSocket)
//! Manager PtyRegistry::read()
//!     ↓ (msg_type, data) via mpsc channel
//! pty_output 批量缓冲
//!     ↓ [4字节长度][数据]
//! QUIC SendStream → 客户端
//! ```
//!
//! # 设计要点
//!
//! `recv_frame` 内部使用 `read_exact`（阻塞式，等待完整帧）。
//! 如果直接用 `tokio::time::timeout` 包装 `recv_frame`，超时时 future 被丢弃，
//! 已读取的部分帧数据会丢失，导致流损坏。
//!
//! 解决方案：独立的读任务通过 mpsc channel 传递帧。
//! 主循环用 `select!` 同时监听 channel 和定时器：
//! - channel 收到帧 → 加入批量缓冲
//! - 定时器触发 → 刷新积压的小批量数据（定时器 future 被丢弃不影响读任务）
//!
//! # 批处理策略
//!
//! 收到 PTY_OUTPUT 帧后加入 batch_buffer，满足以下任一条件时刷新：
//! - batch_buffer 长度达到 batch_size（大小阈值）
//! - 距上次刷新超过 batch_interval_ms（时间阈值，保证交互响应性）
//! - 无新帧且积压超过 batch_interval_ms（定时器触发空闲刷新）

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{sleep, Duration};
use quinn::SendStream;
use anyhow::Result;
use tracing::{debug, info, warn};

use super::pty_registry::PtyRegistry;
use crate::worker::session_protocol::msg_type;

/// PTY 输出推送配置
#[derive(Debug, Clone)]
pub struct PtyOutputConfig {
    /// 每批最大字节数（达到后立即刷新）
    pub batch_size: usize,

    /// 最大等待时间（毫秒，超过后触发刷新）
    pub batch_interval_ms: u64,

    /// 空闲检测周期（毫秒，select! 定时器间隔）
    pub poll_interval_ms: u64,
}

impl Default for PtyOutputConfig {
    fn default() -> Self {
        Self {
            batch_size: 1024,        // 1KB
            batch_interval_ms: 30,   // 30ms
            poll_interval_ms: 50,    // 50ms（select! 定时器间隔）
        }
    }
}

/// 启动 PTY 输出推送任务
///
/// 从 PtyRegistry 读取 Session 进程的输出，批量发送到 QUIC SendStream。
///
/// # 参数
///
/// - `pty_registry`: PTY 注册表
/// - `session_id`: 会话 ID
/// - `send`: QUIC SendStream（发送数据给客户端）
/// - `stats_manager`: 统计管理器（可选，记录终端输出字节数）
/// - `config`: 输出推送配置
///
/// # 返回
///
/// 返回任务句柄（JoinHandle），任务在 EOF 或连接断开时结束。
///
/// # 退出条件
///
/// - 收到 EOF 帧（bash 退出）：刷新剩余数据 → 注销会话 → 退出
/// - 读取错误（连接断开）：刷新剩余数据 → 注销会话 → 退出
/// - QUIC 发送失败：直接退出（客户端已断开）
#[cfg(unix)]
pub async fn spawn_pty_output_task_v2(
    pty_registry: Arc<PtyRegistry>,
    session_id: String,
    send: Arc<Mutex<SendStream>>,
    stats_manager: Option<Arc<crate::auth::StatsManager>>,
    config: PtyOutputConfig,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // 帧通道：读任务 → 主循环
        // 容量 32 足够缓冲终端输出突发（读任务在 channel 满时自然背压）
        let (frame_tx, mut frame_rx) = mpsc::channel::<Result<(u8, Vec<u8>)>>(32);

        // 启动独立读任务（持续从 Session socket 读取帧）
        // 读任务独立于主循环，即使主循环 select! 丢弃 recv future，
        // 读任务仍继续运行，不会丢失已读取的部分帧数据
        let read_registry = Arc::clone(&pty_registry);
        let read_session_id = session_id.clone();
        tokio::spawn(async move {
            loop {
                let result = read_registry.read(&read_session_id).await;
                let is_err = result.is_err();
                // channel 发送失败（主循环已退出）或读取错误 → 停止读任务
                if frame_tx.send(result).await.is_err() || is_err {
                    break;
                }
            }
            debug!("PTY 读任务结束: session_id={}", read_session_id);
        });

        let mut batch_buffer = Vec::with_capacity(config.batch_size);
        let mut last_send_time = std::time::Instant::now();

        loop {
            tokio::select! {
                // 收到帧
                Some(result) = frame_rx.recv() => {
                    match result {
                        Ok((frame_type, data)) if frame_type == msg_type::PTY_OUTPUT => {
                            // PTY 输出：加入批量缓冲
                            if !data.is_empty() {
                                batch_buffer.extend_from_slice(&data);
                            }

                            // 判断是否需要刷新
                            let should_flush = batch_buffer.len() >= config.batch_size
                                || last_send_time.elapsed().as_millis()
                                    >= config.batch_interval_ms as u128;

                            if should_flush && !batch_buffer.is_empty() {
                                if let Err(e) = send_batch(&send, &batch_buffer).await {
                                    warn!("发送终端数据失败: {}", e);
                                    break;
                                }
                                record_bytes(&stats_manager, batch_buffer.len());
                                debug!(
                                    "PTY 批量发送: session_id={}, len={}",
                                    session_id,
                                    batch_buffer.len()
                                );
                                batch_buffer.clear();
                                last_send_time = std::time::Instant::now();
                            }
                        }
                        Ok((frame_type, _)) if frame_type == msg_type::EOF => {
                            // bash 退出：刷新剩余数据并退出
                            info!("Session EOF: session_id={}", session_id);
                            flush_remaining(&send, &stats_manager, &mut batch_buffer).await;
                            break;
                        }
                        Ok((other_type, _)) => {
                            // 其他消息类型（Hello 等），忽略
                            debug!(
                                "Session 消息忽略: session_id={}, type=0x{:02x}",
                                session_id, other_type
                            );
                        }
                        Err(e) => {
                            // 读取错误：连接断开
                            warn!(
                                "Session 读取失败: session_id={}, error={}",
                                session_id, e
                            );
                            flush_remaining(&send, &stats_manager, &mut batch_buffer).await;
                            break;
                        }
                    }
                }
                // 定时器：空闲时检查是否有积压需要刷新
                // 仅当 batch_buffer 非空时启用此分支（避免空转）
                _ = sleep(Duration::from_millis(config.poll_interval_ms)),
                    if !batch_buffer.is_empty() =>
                {
                    if last_send_time.elapsed().as_millis()
                        >= config.batch_interval_ms as u128
                    {
                        if let Err(e) = send_batch(&send, &batch_buffer).await {
                            warn!("发送终端数据失败(空闲刷新): {}", e);
                            break;
                        }
                        record_bytes(&stats_manager, batch_buffer.len());
                        debug!(
                            "PTY 空闲刷新: session_id={}, len={}",
                            session_id,
                            batch_buffer.len()
                        );
                        batch_buffer.clear();
                        last_send_time = std::time::Instant::now();
                    }
                }
            }
        }

        // 清理：注销会话（发送 Close 给 Session 进程）
        let _ = pty_registry.unregister(&session_id).await;
        debug!("PTY 输出任务结束: session_id={}", session_id);
    })
}

/// 刷新剩余数据并记录统计
async fn flush_remaining(
    send: &Arc<Mutex<SendStream>>,
    stats_manager: &Option<Arc<crate::auth::StatsManager>>,
    batch_buffer: &mut Vec<u8>,
) {
    if !batch_buffer.is_empty() {
        let _ = send_batch(send, batch_buffer).await;
        record_bytes(stats_manager, batch_buffer.len());
        batch_buffer.clear();
    }
}

/// 记录终端输出字节数
fn record_bytes(stats_manager: &Option<Arc<crate::auth::StatsManager>>, len: usize) {
    if let Some(ref stats) = stats_manager {
        stats.record_terminal_bytes(len as u64);
    }
}

/// 发送批量数据到客户端
///
/// 数据格式: [4字节长度(小端)][数据]
async fn send_batch(send: &Arc<Mutex<SendStream>>, data: &[u8]) -> Result<()> {
    let len = (data.len() as u32).to_le_bytes();
    let mut send_guard = send.lock().await;
    send_guard.write_all(&len).await?;
    send_guard.write_all(data).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = PtyOutputConfig::default();
        assert_eq!(config.batch_size, 1024);
        assert_eq!(config.batch_interval_ms, 30);
        assert_eq!(config.poll_interval_ms, 50);
    }

    #[test]
    fn test_config_custom() {
        let config = PtyOutputConfig {
            batch_size: 2048,
            batch_interval_ms: 50,
            poll_interval_ms: 20,
        };
        assert_eq!(config.batch_size, 2048);
        assert_eq!(config.batch_interval_ms, 50);
        assert_eq!(config.poll_interval_ms, 20);
    }
}
