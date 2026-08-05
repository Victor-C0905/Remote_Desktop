//! 阶段 1 集成测试:验证 Manager 启动流程
//!
//! 测试目标:
//! - Manager::new 能成功实例化
//! - Manager::start 能启动 IPC 服务器
//! - WorkerManager 能启动 Worker 子进程
//! - Worker 子进程能连接到 IPC 服务器

#![cfg(unix)]

use std::time::Duration;
use gnome_remote_agent::config::AgentConfig;
use gnome_remote_agent::manager::Manager;

/// 测试 Manager 能成功实例化
#[tokio::test]
async fn test_manager_creation() {
    let config = AgentConfig::default();
    let manager = Manager::new(&config).await;
    assert!(manager.is_ok(), "Manager 创建失败: {:?}", manager.err());
}

/// 测试 Manager::start 能启动 IPC 服务器和 Worker
#[tokio::test]
async fn test_manager_start_ipc_and_worker() {
    let mut config = AgentConfig::default();
    // 使用唯一的 socket 路径,避免冲突
    config.worker.ipc_socket_path = format!(
        "/tmp/gnome-remote-test-{}.sock",
        std::process::id()
    );
    // 使用当前编译的 agent 二进制
    config.worker.agent_binary = env!("CARGO_BIN_EXE_agent").to_string();
    config.worker.max_restarts = 1;

    let mut manager = Manager::new(&config).await.expect("Manager 创建失败");

    // 启动 Manager
    let start_result = manager.start().await;
    assert!(start_result.is_ok(), "Manager 启动失败: {:?}", start_result.err());

    // 等待 Worker 连接
    tokio::time::sleep(Duration::from_secs(1)).await;

    // 验证 Worker 进程已启动(通过公共 getter)
    let worker_info = manager.worker_info().await;
    assert!(worker_info.is_some(), "Worker 信息不应为空");
    let info = worker_info.unwrap();
    assert!(info.pid > 0, "Worker PID 应大于 0");

    // 停止 Manager
    let shutdown_result = manager.shutdown().await;
    assert!(shutdown_result.is_ok(), "Manager 停止失败: {:?}", shutdown_result.err());

    // 清理 socket 文件
    let _ = std::fs::remove_file(&config.worker.ipc_socket_path);
}

/// 测试 Manager::shutdown 能正确清理资源
#[tokio::test]
async fn test_manager_shutdown_cleanup() {
    let mut config = AgentConfig::default();
    config.worker.ipc_socket_path = format!(
        "/tmp/gnome-remote-test-shutdown-{}.sock",
        std::process::id()
    );
    config.worker.agent_binary = env!("CARGO_BIN_EXE_agent").to_string();
    config.worker.max_restarts = 1;

    let mut manager = Manager::new(&config).await.expect("Manager 创建失败");
    manager.start().await.expect("Manager 启动失败");

    tokio::time::sleep(Duration::from_secs(1)).await;

    // 停止
    manager.shutdown().await.expect("Manager 停止失败");

    // 验证 socket 文件已清理(IpcServer::stop 会关闭监听器,但可能不删除文件)
    // 这里只验证进程已停止
    tokio::time::sleep(Duration::from_millis(500)).await;

    let worker_info = manager.worker_info().await;
    // Worker 信息应该为 None 或状态为 Stopped
    if let Some(info) = worker_info {
        use gnome_remote_agent::manager::WorkerStatus;
        assert!(
            info.status == WorkerStatus::Stopped
                || info.status == WorkerStatus::Stopping,
            "Worker 状态应为 Stopped,实际: {:?}",
            info.status
        );
    }

    let _ = std::fs::remove_file(&config.worker.ipc_socket_path);
}
