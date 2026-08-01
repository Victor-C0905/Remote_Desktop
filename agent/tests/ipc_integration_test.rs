//! IpcServer 集成测试
//!
//! 测试 IpcServer 与 PtyRegistry 和 WorkerManager 的集成。
//! 仅在 Unix 系统上运行。

#![cfg(unix)]

use std::sync::Arc;
use gnome_remote_agent::manager::{IpcServer, PtyRegistry, WorkerManager, WorkerStatus};
use gnome_remote_agent::manager::pty_registry::UserInfo;

#[tokio::test]
async fn test_ipc_server_with_pty_registry() {
    // 创建 PtyRegistry
    let registry = Arc::new(PtyRegistry::new());

    // 创建 WorkerManager
    let worker_manager = Arc::new(WorkerManager::new(
        "/usr/bin/agent".to_string(),
        "/tmp/test.sock".to_string(),
        3
    ));

    // 创建 IpcServer（集成 WorkerManager）
    let socket_path = format!("/tmp/test_ipc_{}.sock", uuid::Uuid::new_v4());
    let server = IpcServer::new(socket_path.clone(), registry.clone(), worker_manager.clone());

    // 启动服务器
    server.start().await.expect("Failed to start IPC server");

    // 验证启动后没有活动连接
    assert_eq!(server.active_connection_count().await, 0);

    // 停止服务器
    server.stop().await.expect("Failed to stop IPC server");

    // 清理
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_cleanup_connection() {
    let registry = Arc::new(PtyRegistry::new());
    let worker_manager = Arc::new(WorkerManager::new(
        "/usr/bin/agent".to_string(),
        "/tmp/test.sock".to_string(),
        3
    ));
    let socket_path = format!("/tmp/test_ipc_cleanup_{}.sock", uuid::Uuid::new_v4());
    let server = IpcServer::new(socket_path.clone(), registry.clone(), worker_manager.clone());

    server.start().await.expect("Failed to start IPC server");

    // 清理不存在的连接应该成功
    let result = server.cleanup_connection("non-existent-id").await;
    assert!(result.is_ok());

    server.stop().await.expect("Failed to stop IPC server");
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_cleanup_by_worker_pid() {
    let registry = Arc::new(PtyRegistry::new());
    let worker_manager = Arc::new(WorkerManager::new(
        "/usr/bin/agent".to_string(),
        "/tmp/test.sock".to_string(),
        3
    ));
    let socket_path = format!("/tmp/test_ipc_pid_{}.sock", uuid::Uuid::new_v4());
    let server = IpcServer::new(socket_path.clone(), registry.clone(), worker_manager.clone());

    server.start().await.expect("Failed to start IPC server");

    // 清理不存在的 worker_pid 应该成功
    let result = server.cleanup_by_worker_pid(12345).await;
    assert!(result.is_ok());

    server.stop().await.expect("Failed to stop IPC server");
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_receive_and_register_fd_invalid_connection() {
    let registry = Arc::new(PtyRegistry::new());
    let worker_manager = Arc::new(WorkerManager::new(
        "/usr/bin/agent".to_string(),
        "/tmp/test.sock".to_string(),
        3
    ));
    let socket_path = format!("/tmp/test_ipc_fd_{}.sock", uuid::Uuid::new_v4());
    let server = IpcServer::new(socket_path.clone(), registry.clone(), worker_manager.clone());

    server.start().await.expect("Failed to start IPC server");

    // 尝试使用不存在的 connection_id 注册应该失败
    let user_info = UserInfo {
        username: "test".to_string(),
        uid: 1000,
        gid: 1000,
    };

    let result = server.receive_and_register_fd(
        "non-existent-connection",
        "test-session".to_string(),
        user_info,
    ).await;

    assert!(result.is_err());

    server.stop().await.expect("Failed to stop IPC server");
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_multiple_start_stop_cycles() {
    let registry = Arc::new(PtyRegistry::new());
    let worker_manager = Arc::new(WorkerManager::new(
        "/usr/bin/agent".to_string(),
        "/tmp/test.sock".to_string(),
        3
    ));
    let socket_path = format!("/tmp/test_ipc_cycle_{}.sock", uuid::Uuid::new_v4());

    // 第一次启动-停止循环
    {
        let server = IpcServer::new(socket_path.clone(), registry.clone(), worker_manager.clone());
        server.start().await.expect("Failed to start IPC server (1st cycle)");
        server.stop().await.expect("Failed to stop IPC server (1st cycle)");
    }

    // 第二次启动-停止循环（验证 Socket 文件正确清理）
    {
        let server = IpcServer::new(socket_path.clone(), registry.clone(), worker_manager.clone());
        server.start().await.expect("Failed to start IPC server (2nd cycle)");
        server.stop().await.expect("Failed to stop IPC server (2nd cycle)");
    }

    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_worker_status_event_integration() {
    // 测试 WorkerManager 和 IpcServer 的事件集成
    let registry = Arc::new(PtyRegistry::new());
    let worker_manager = Arc::new(WorkerManager::new(
        "/usr/bin/agent".to_string(),
        "/tmp/test_worker.sock".to_string(),
        3
    ));
    let socket_path = format!("/tmp/test_worker_integration_{}.sock", uuid::Uuid::new_v4());
    let server = IpcServer::new(socket_path.clone(), registry.clone(), worker_manager.clone());

    server.start().await.expect("Failed to start IPC server");

    // 订阅 WorkerManager 事件
    let mut event_rx = worker_manager.subscribe();

    // 启动 Worker（会失败，因为没有真实的二进制文件）
    let _ = worker_manager.start().await;

    // 如果启动成功，应该收到 Starting 事件
    // 但由于路径不存在，启动会失败，所以我们只验证 subscribe() 能正常工作

    server.stop().await.expect("Failed to stop IPC server");
    let _ = std::fs::remove_file(&socket_path);
}

#[tokio::test]
async fn test_manager_creation() {
    // 测试 Manager 创建（需要配置）
    use gnome_remote_agent::config::AgentConfig;

    let config = AgentConfig::default();
    let result = gnome_remote_agent::manager::Manager::new(&config).await;
    assert!(result.is_ok());
}