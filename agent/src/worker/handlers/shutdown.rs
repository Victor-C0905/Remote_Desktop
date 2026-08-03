//! 优雅关闭处理器 - 处理 Manager 的 GracefulShutdown 请求
//!
//! 该模块负责：
//! - 接收 GracefulShutdown 请求
//! - 等待未决任务完成（宽限期内）
//! - 生成状态快照（Phase 4 暂不使用）
//! - 返回 ShutdownAck 给 Manager
//! - 通知 Worker 主循环退出（不杀死 Sessions）

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;

use crate::protocol::generated::{
    GracefulShutdown, ShutdownAck,
    WorkerResponse, worker_response,
};
use super::super::SessionManager;

/// 处理 GracefulShutdown 请求
///
/// # 参数
///
/// - `req`: GracefulShutdown 请求参数
/// - `session_manager`: 会话管理器实例
/// - `shutdown_notify`: 通知 Worker 主循环退出的 Notify 实例
///
/// # 返回
///
/// 返回 `WorkerResponse`，包含 `ShutdownAck`。
///
/// # 流程
///
/// 1. 记录日志
/// 2. 等待未决请求完成（最多 grace_period_secs 秒）
/// 3. 生成状态快照（Phase 4 返回空）
/// 4. 通过 Notify 通知主循环退出
/// 5. 返回 ShutdownAck
pub async fn handle_graceful_shutdown(
    req: GracefulShutdown,
    session_manager: &SessionManager,
    shutdown_notify: &Arc<Notify>,
) -> WorkerResponse {
    tracing::info!(
        "收到 GracefulShutdown: grace_period={}s, migrate={}, reason={}",
        req.grace_period_secs, req.migrate_state, req.reason
    );

    // 1. 等待未决请求完成（宽限期内轮询 all_idle）
    let grace_period = Duration::from_secs(req.grace_period_secs as u64);
    let deadline = Instant::now() + grace_period;

    while !session_manager.all_idle().await && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let all_completed = session_manager.all_idle().await;
    let sessions_count = session_manager.list().await.len() as u32;

    tracing::info!(
        "GracefulShutdown 处理完成: all_tasks_completed={}, sessions_count={}",
        all_completed, sessions_count
    );

    // 2. 生成状态快照（Phase 4 不使用，预留扩展点）
    if req.migrate_state {
        let _snapshot: Vec<u64> = session_manager.snapshot().await;
        // Phase 4 不实现状态迁移，仅记录日志
        tracing::debug!("状态迁移已请求，但 Phase 4 暂未实现");
    }

    // 3. 通知 Worker 主循环退出（通过 Notify）
    // 注意：Worker 退出时不杀死 Sessions，它们成为孤儿进程
    // Manager 仍持有 master_fd，数据流不中断
    //
    // 使用 notify_one() 而非 notify_waiters()：
    // notify_one() 在没有等待者时会存储一个 permit，
    // 这样下一轮 select! 中的 notified() 会立即完成。
    // notify_waiters() 只通知当前已注册的等待者，
    // 但此时 select! 已进入 request 分支，notified() 已被 drop，
    // 会导致通知丢失。
    shutdown_notify.notify_one();

    // 4. 返回 ShutdownAck
    WorkerResponse {
        payload: Some(worker_response::Payload::ShutdownAck(ShutdownAck {
            all_tasks_completed: all_completed,
            sessions_count,
        })),
        ..Default::default()
    }
}
