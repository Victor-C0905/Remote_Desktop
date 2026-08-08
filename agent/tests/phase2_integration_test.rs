//! 阶段 2 集成测试:PTY 创建迁移到 Worker
//!
//! 测试目标:
//! - Manager::create_pty_session 能通过 Worker 子进程创建 PTY
//! - PtyRegistry 中能找到创建的 session
//! - PTY 读写正常
//! - PTY resize 正常
//! - PTY 注销正常

#![cfg(unix)]

use std::time::Duration;
use gnome_remote_agent::config::AgentConfig;
use gnome_remote_agent::manager::Manager;
use gnome_remote_agent::auth::session::UserSession;
use std::path::PathBuf;

/// 创建测试用的 UserSession(使用当前用户信息)
fn create_test_user_session() -> UserSession {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    let username = std::env::var("USER").unwrap_or_else(|_| "test".to_string());
    let home_dir = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));

    UserSession {
        session_id: format!("test-session-{}", std::process::id()),
        username,
        uid,
        gid,
        home_dir,
        shell: PathBuf::from("/bin/bash"),
        created_at: std::time::SystemTime::now(),
    }
}

/// 测试通过 Worker 创建 PTY 会话
#[tokio::test]
async fn test_create_pty_session_via_worker() {
    let mut config = AgentConfig::default();
    config.worker.ipc_socket_path = format!(
        "/tmp/gnome-remote-phase2-test-{}.sock",
        std::process::id()
    );
    config.worker.agent_binary = env!("CARGO_BIN_EXE_agent").to_string();
    config.worker.max_restarts = 1;

    let manager = std::sync::Arc::new(
        Manager::new(&config).await.expect("Manager 创建失败")
    );
    manager.start().await.expect("Manager 启动失败");

    // 等待 Worker 连接
    tokio::time::sleep(Duration::from_secs(1)).await;

    let user_session = create_test_user_session();

    // 通过 Worker 创建 PTY
    let session_id = manager
        .create_pty_session("/bin/bash", 80, 24, None, &user_session)
        .await
        .expect("PTY 创建失败");

    assert!(!session_id.is_empty(), "session_id 不应为空");

    // 验证 PtyRegistry 中有该 session
    let registry = manager.pty_registry();
    let session = registry.get(&session_id).await;
    assert!(session.is_some(), "PtyRegistry 中应能找到 session");
    assert!(session.unwrap().connection.socket_name().contains("gnome-remote-session"), "应有有效的 socket_name");

    // 清理
    let removed = registry.unregister(&session_id).await;
    assert!(removed.is_ok(), "注销 session 应成功");

    // 停止 Manager
    manager.shutdown().await.expect("Manager 停止失败");

    // 清理 socket 文件
    let _ = std::fs::remove_file(&config.worker.ipc_socket_path);
}

/// 测试 PTY 读写
#[tokio::test]
async fn test_pty_read_write_via_worker() {
    let mut config = AgentConfig::default();
    config.worker.ipc_socket_path = format!(
        "/tmp/gnome-remote-phase2-rw-{}.sock",
        std::process::id()
    );
    config.worker.agent_binary = env!("CARGO_BIN_EXE_agent").to_string();
    config.worker.max_restarts = 1;

    let manager = std::sync::Arc::new(
        Manager::new(&config).await.expect("Manager 创建失败")
    );
    manager.start().await.expect("Manager 启动失败");

    tokio::time::sleep(Duration::from_secs(1)).await;

    let user_session = create_test_user_session();

    // 创建 PTY
    let session_id = manager
        .create_pty_session("/bin/bash", 80, 24, None, &user_session)
        .await
        .expect("PTY 创建失败");

    let registry = manager.pty_registry();

    // 写入 echo 命令
    tokio::time::sleep(Duration::from_millis(500)).await;
    let write_result = registry.write(&session_id, b"echo hello_phase2\n").await;
    assert!(write_result.is_ok(), "写入 PTY 应成功: {:?}", write_result.err());

    // 等待 shell 处理
    tokio::time::sleep(Duration::from_secs(1)).await;

    // 读取输出(可能需要多次读取才能获取完整输出)
    let mut found_hello = false;
    for _ in 0..10 {
        let read_result = registry.read(&session_id).await;
        if let Ok((_msg_type, data)) = read_result {
            if !data.is_empty() {
                let output = String::from_utf8_lossy(&data);
                if output.contains("hello_phase2") {
                    found_hello = true;
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(found_hello, "PTY 输出应包含 'hello_phase2'");

    // 清理
    let _ = registry.unregister(&session_id).await;
    manager.shutdown().await.expect("Manager 停止失败");
    let _ = std::fs::remove_file(&config.worker.ipc_socket_path);
}

/// 测试 PTY resize
#[tokio::test]
async fn test_pty_resize_via_worker() {
    let mut config = AgentConfig::default();
    config.worker.ipc_socket_path = format!(
        "/tmp/gnome-remote-phase2-resize-{}.sock",
        std::process::id()
    );
    config.worker.agent_binary = env!("CARGO_BIN_EXE_agent").to_string();
    config.worker.max_restarts = 1;

    let manager = std::sync::Arc::new(
        Manager::new(&config).await.expect("Manager 创建失败")
    );
    manager.start().await.expect("Manager 启动失败");

    tokio::time::sleep(Duration::from_secs(1)).await;

    let user_session = create_test_user_session();

    let session_id = manager
        .create_pty_session("/bin/bash", 80, 24, None, &user_session)
        .await
        .expect("PTY 创建失败");

    let registry = manager.pty_registry();

    // Resize 到 120x40
    let resize_result = registry.resize(&session_id, 120, 40).await;
    assert!(resize_result.is_ok(), "PTY resize 应成功: {:?}", resize_result.err());

    // 清理
    let _ = registry.unregister(&session_id).await;
    manager.shutdown().await.expect("Manager 停止失败");
    let _ = std::fs::remove_file(&config.worker.ipc_socket_path);
}
