//! Phase 4 热更新集成测试
//!
//! 测试场景：
//! - OrphanProcessReaper Session 清理（新架构：不再 waitpid，只清理 PtyRegistry）
//! - WorkerManager 优雅关闭标志
//! - HotUpdateCoordinator 协调器创建和防重入
//! - SIGHUP 信号处理
//! - WorkerCrashDetector 启停

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;
use quireld::manager::{
    WorkerManager, PtyRegistry,
    OrphanProcessReaper, WorkerCrashDetector,
    HotUpdateCoordinator, ReloadTrigger, watch_sighup,
};
use tokio::sync::mpsc;

#[tokio::test]
async fn test_orphan_reaper_creation() {
    let pty_registry = Arc::new(PtyRegistry::new());
    let _reaper = OrphanProcessReaper::new(pty_registry);
}

#[tokio::test]
async fn test_orphan_reaper_cleanup_nonexistent() {
    // 新架构：cleanup_session 清理不存在的会话应返回 Ok
    let pty_registry = Arc::new(PtyRegistry::new());
    let reaper = OrphanProcessReaper::new(pty_registry);

    let result = reaper.cleanup_session("nonexistent-session").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_worker_manager_graceful_shutdown_flag() {
    let manager = WorkerManager::new(
        "/usr/bin/quireld".to_string(),
        "/tmp/test.sock".to_string(),
        3,
    );

    // 初始为 false
    assert!(!manager.is_graceful_shutdown().await);

    // 标记后为 true
    manager.mark_graceful_shutdown().await;
    assert!(manager.is_graceful_shutdown().await);

    // 重置后为 false
    manager.reset_graceful_shutdown().await;
    assert!(!manager.is_graceful_shutdown().await);
}

#[tokio::test]
async fn test_hot_update_coordinator_creation() {
    let worker_manager = Arc::new(WorkerManager::new(
        "/usr/bin/quireld".to_string(),
        "/tmp/test.sock".to_string(),
        3,
    ));
    let (_tx, rx) = mpsc::channel(16);

    let coordinator = HotUpdateCoordinator::new(worker_manager, rx);
    let flag = coordinator.is_reloading_flag();
    assert!(!flag.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn test_sighup_signal_handling() {
    let (tx, mut rx) = mpsc::channel(16);
    let handle = watch_sighup(tx);

    // 等待监听器启动
    tokio::time::sleep(Duration::from_millis(100)).await;

    // 发送 SIGHUP 给当前进程
    let pid = nix::unistd::Pid::from_raw(std::process::id() as i32);
    let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGHUP);

    // 验证收到事件
    let result = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
    assert!(result.is_ok(), "应在 2 秒内收到 SIGHUP 事件");

    let trigger = result.unwrap().unwrap();
    assert!(matches!(trigger, ReloadTrigger::UnixSignal));

    handle.abort();
}

#[tokio::test]
async fn test_crash_detector_start_stop() {
    let worker_manager = Arc::new(WorkerManager::new(
        "/nonexistent/binary".to_string(),
        "/tmp/test.sock".to_string(),
        3,
    ));

    let mut detector = WorkerCrashDetector::new(worker_manager);
    detector.start();

    tokio::time::sleep(Duration::from_millis(100)).await;

    detector.stop();
}

#[tokio::test]
async fn test_hot_update_coordinator_rejects_duplicate_trigger() {
    let worker_manager = Arc::new(WorkerManager::new(
        "/usr/bin/quireld".to_string(),
        "/tmp/test.sock".to_string(),
        3,
    ));
    let (_tx, rx) = mpsc::channel(16);

    let coordinator = HotUpdateCoordinator::new(worker_manager, rx);

    // 手动设置 is_reloading
    coordinator.is_reloading_flag()
        .store(true, std::sync::atomic::Ordering::SeqCst);

    // 应拒绝触发
    let result = coordinator.trigger_reload(ReloadTrigger::UnixSignal).await;
    assert!(result.is_err());
}
