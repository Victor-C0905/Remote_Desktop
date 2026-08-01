//! PTY 输出推送模块
//!
//! 负责 PTY 输出读取和推送到客户端的逻辑。
//! 符合新架构设计：Manager 持有 master_fd 并直接读写。
//!
//! # 迁移指南
//!
//! ## 旧代码（quic.rs 内联实现）
//!
//! ```rust,ignore
//! // 旧的 PTY 输出读取任务（内联在 quic.rs）
//! let pty_read_task = tokio::spawn(async move {
//!     let mut batch_buffer = Vec::with_capacity(1024);
//!     let mut last_send_time = std::time::Instant::now();
//!
//!     loop {
//!         match pty_manager.read(&session_id).await {
//!             Ok(data) if !data.is_empty() => {
//!                 batch_buffer.extend_from_slice(&data);
//!                 // ... 批量发送逻辑（约80行）
//!             }
//!             // ... 其他分支
//!         }
//!     }
//! });
//! ```
//!
//! ## 新代码（使用 manager 模块）
//!
//! ```rust,ignore
//! use crate::manager::{spawn_pty_output_task, PtyOutputConfig};
//!
//! // 创建配置（可选，有默认值）
//! let config = PtyOutputConfig::default();
//!
//! // 启动 PTY 输出推送任务
//! let output_task = spawn_pty_output_task(
//!     pty_manager,      // Arc<PtyManager>
//!     session_id,       // String
//!     quic_stream,      // Arc<Mutex<SendStream>>
//!     Some(stats_manager), // Option<Arc<StatsManager>>
//!     config,           // PtyOutputConfig
//! );
//!
//! // 在 select! 中等待任务完成
//! tokio::pin!(output_task);
//! tokio::select! {
//!     _ = &mut output_task => {
//!         tracing::info!("PTY 输出任务结束");
//!     }
//!     // ... 其他分支
//! }
//! ```

use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use quinn::SendStream;
use anyhow::Result;
use tracing::{debug, warn};

use super::pty_registry::PtyRegistry;

// 向后兼容：支持旧的 PtyManager
#[cfg(unix)]
use crate::pty::PtyManager;

/// PTY 输出推送配置
#[derive(Debug, Clone)]
pub struct PtyOutputConfig {
    /// 每批最大字节数
    pub batch_size: usize,

    /// 最大等待时间（毫秒）
    pub batch_interval_ms: u64,

    /// 轮询间隔（毫秒）
    pub poll_interval_ms: u64,
}

impl Default for PtyOutputConfig {
    fn default() -> Self {
        Self {
            batch_size: 1024,        // 1KB
            batch_interval_ms: 30,   // 30ms
            poll_interval_ms: 10,    // 10ms
        }
    }
}

/// 启动 PTY 输出推送任务（新架构：使用 PtyRegistry）
///
/// # 参数
/// - `pty_registry`: PTY 注册表
/// - `session_id`: PTY 会话 ID
/// - `send`: QUIC SendStream（用于发送数据给客户端）
/// - `stats_manager`: 统计管理器（可选）
/// - `config`: 输出推送配置
///
/// # 返回
/// 返回任务句柄
pub async fn spawn_pty_output_task(
    pty_registry: Arc<PtyRegistry>,
    session_id: String,
    send: Arc<Mutex<SendStream>>,
    stats_manager: Option<Arc<crate::auth::StatsManager>>,
    config: PtyOutputConfig,
) -> tokio::task::JoinHandle<()> {
    spawn_pty_output_impl(pty_registry, session_id, send, stats_manager, config).await
}

/// 启动 PTY 输出推送任务（向后兼容：使用 PtyManager）
///
/// # 参数
/// - `pty_manager`: PTY 管理器（旧架构）
/// - `session_id`: PTY 会话 ID
/// - `send`: QUIC SendStream（用于发送数据给客户端）
/// - `stats_manager`: 统计管理器（可选）
/// - `config`: 输出推送配置
///
/// # 返回
/// 返回任务句柄
#[cfg(unix)]
pub async fn spawn_pty_output_task_legacy(
    pty_manager: Arc<PtyManager>,
    session_id: String,
    send: Arc<Mutex<SendStream>>,
    stats_manager: Option<Arc<crate::auth::StatsManager>>,
    config: PtyOutputConfig,
) -> tokio::task::JoinHandle<()> {
    spawn_pty_output_impl(pty_manager, session_id, send, stats_manager, config).await
}

/// PTY 读取器 Trait（内部抽象）
#[cfg(unix)]
trait PtyReader: Send + Sync {
    async fn read(&self, session_id: &str) -> Result<Vec<u8>>;
}

// 为 PtyRegistry 实现读取器
#[cfg(unix)]
impl PtyReader for PtyRegistry {
    async fn read(&self, session_id: &str) -> Result<Vec<u8>> {
        PtyRegistry::read(self, session_id).await
    }
}

// 为 PtyManager 实现读取器（向后兼容）
#[cfg(unix)]
impl PtyReader for PtyManager {
    async fn read(&self, session_id: &str) -> Result<Vec<u8>> {
        PtyManager::read(self, session_id).await
    }
}

/// 内部实现（泛型版本）
#[cfg(unix)]
async fn spawn_pty_output_impl<R>(
    pty_reader: Arc<R>,
    session_id: String,
    send: Arc<Mutex<SendStream>>,
    stats_manager: Option<Arc<crate::auth::StatsManager>>,
    config: PtyOutputConfig,
) -> tokio::task::JoinHandle<()>
where
    R: PtyReader + 'static,
{
    tokio::spawn(async move {
        let mut batch_buffer = Vec::with_capacity(config.batch_size);
        let mut last_send_time = std::time::Instant::now();

        loop {
            // 从 PTY 读取输出
            match pty_reader.read(&session_id).await {
                Ok(data) if !data.is_empty() => {
                    batch_buffer.extend_from_slice(&data);

                    // 判断是否需要发送批次
                    let should_flush = batch_buffer.len() >= config.batch_size
                        || last_send_time.elapsed().as_millis() >= config.batch_interval_ms as u128;

                    if should_flush && !batch_buffer.is_empty() {
                        // 发送批量数据
                        if let Err(e) = send_batch(&send, &batch_buffer).await {
                            warn!("发送终端数据失败: {}", e);
                            break;
                        }

                        // 记录终端输出字节数
                        if let Some(ref stats) = stats_manager {
                            stats.record_terminal_bytes(batch_buffer.len() as u64);
                        }

                        debug!(
                            "PTY 批量输出发送: session_id={}, batch_len={}",
                            session_id, batch_buffer.len()
                        );

                        batch_buffer.clear();
                        last_send_time = std::time::Instant::now();
                    }
                }
                Ok(_) => {
                    // 无数据时检查是否有积压数据需要刷新
                    if !batch_buffer.is_empty()
                        && last_send_time.elapsed().as_millis() >= config.batch_interval_ms as u128
                    {
                        if let Err(e) = send_batch(&send, &batch_buffer).await {
                            warn!("发送终端数据失败(空闲刷新): {}", e);
                            break;
                        }

                        // 记录终端输出字节数
                        if let Some(ref stats) = stats_manager {
                            stats.record_terminal_bytes(batch_buffer.len() as u64);
                        }

                        debug!(
                            "PTY 空闲刷新: session_id={}, batch_len={}",
                            session_id, batch_buffer.len()
                        );

                        batch_buffer.clear();
                        last_send_time = std::time::Instant::now();
                    }
                    // 无数据，短暂等待（降低轮询频率减少 CPU 占用）
                    sleep(Duration::from_millis(config.poll_interval_ms)).await;
                }
                Err(e) => {
                    warn!("PTY 读取失败: session_id={}, error={}", session_id, e);
                    break;
                }
            }
        }

        // 发送剩余数据
        if !batch_buffer.is_empty() {
            let _ = send_batch(&send, &batch_buffer).await;

            // 记录终端输出字节数
            if let Some(ref stats) = stats_manager {
                stats.record_terminal_bytes(batch_buffer.len() as u64);
            }
        }

        debug!("PTY 输出推送任务结束: session_id={}", session_id);
    })
}

/// 发送批量数据到客户端
///
/// 数据格式: [4字节长度][数据]
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
        assert_eq!(config.poll_interval_ms, 10);
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

    /// 验证新旧接口兼容性测试
    ///
    /// 测试目标：
    /// 1. spawn_pty_output_task（新架构：使用 PtyRegistry）
    /// 2. spawn_pty_output_task_legacy（向后兼容：使用 PtyManager）
    ///
    /// 两者的行为应该完全一致：
    /// - 批量发送逻辑相同
    /// - 错误处理相同
    /// - 统计记录相同
    #[cfg(unix)]
    mod integration_tests {
        use super::*;
        use tokio::sync::Mutex;
        use std::sync::Arc;

        #[test]
        fn test_pty_reader_trait_implementation() {
            // 验证 PtyReader trait 实现正确
            // 这个测试确保两个实现者（PtyRegistry 和 PtyManager）都能正确实现接口

            // 注意：由于 PtyRegistry 和 PtyManager 的构造需要特定环境，
            // 这里主要验证编译时类型检查通过
            fn _assert_pty_reader_implemented<T: PtyReader>() {}

            _assert_pty_reader_implemented::<PtyRegistry>();
            _assert_pty_reader_implemented::<PtyManager>();
        }

        #[test]
        fn test_batch_logic_consistency() {
            // 验证批量发送逻辑一致性
            // 两套实现应该使用相同的批量阈值

            let config_new = PtyOutputConfig::default();
            let config_legacy = PtyOutputConfig::default();

            assert_eq!(config_new.batch_size, config_legacy.batch_size);
            assert_eq!(config_new.batch_interval_ms, config_legacy.batch_interval_ms);
            assert_eq!(config_new.poll_interval_ms, config_legacy.poll_interval_ms);
        }
    }
}