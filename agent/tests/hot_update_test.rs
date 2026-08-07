//! Phase 4 热更新集成测试
//!
//! 测试场景：
//! - OrphanProcessReaper 孤儿进程回收
//! - WorkerManager 优雅关闭标志
//! - HotUpdateCoordinator 协调器创建和防重入
//! - SIGHUP 信号处理
//! - WorkerCrashDetector 启停

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;
use gnome_remote_agent::manager::{
    WorkerManager, PtyRegistry,
    OrphanProcessReaper, WorkerCrashDetector,
    HotUpdateCoordinator, ReloadTrigger, watch_sighup,
};
use tokio::sync::mpsc;

#[tokio::test]
async fn test_orphan_reaper_creation() {
    let pty_registry = Arc::new(PtyRegistry::new());
    let reaper = OrphanProcessReaper::new(pty_registry);

    assert_eq!(reaper.session_count().await, 0);
}

#[tokio::test]
async fn test_orphan_reaper_register_and_lookup() {
    let pty_registry = Arc::new(PtyRegistry::new());
    let reaper = OrphanProcessReaper::new(pty_registry);

    use nix::unistd::Pid;
    reaper.register(Pid::from_raw(12345), "session-1".to_string()).await;

    let pid = reaper.get_pid("session-1").await;
    assert!(pid.is_some());
    assert_eq!(pid.unwrap(), Pid::from_raw(12345));

    assert_eq!(reaper.session_count().await, 1);
}

#[tokio::test]
async fn test_orphan_reaper_reap_nonexistent() {
    let pty_registry = Arc::new(PtyRegistry::new());
    let reaper = OrphanProcessReaper::new(pty_registry);

    use nix::unistd::Pid;
    reaper.register(Pid::from_raw(99999), "ghost".to_string()).await;

    // 回收不存在的进程（应返回 ECHILD 但不 panic）
    let result = reaper.reap_zombie(Pid::from_raw(99999)).await;
    assert!(result.is_ok());

    // 映射应已清理
    assert_eq!(reaper.session_count().await, 0);
}

#[tokio::test]
async fn test_worker_manager_graceful_shutdown_flag() {
    let manager = WorkerManager::new(
        "/usr/bin/agent".to_string(),
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
        "/usr/bin/agent".to_string(),
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
        "/usr/bin/agent".to_string(),
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
