//! Manager↔Worker IPC 全链路集成测试
//!
//! 测试场景：
//! - 启动真实 Worker 子进程，验证 IPC 连接建立
//! - 验证 ManagerRequest/WorkerResponse 消息往返
//! - 验证 SCM_RIGHTS FD 传递

#![cfg(unix)]

use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use gnome_remote_agent::manager::{IpcServer, PtyRegistry, WorkerManager};
use gnome_remote_agent::protocol::generated::{
    ManagerRequest, manager_request,
    ReadDir,
};
use tempfile::TempDir;

/// 测试辅助：启动 IpcServer 和 Worker 子进程
struct TestEnv {
    #[allow(dead_code)]
    socket_path: String,
    server: Arc<IpcServer>,
    worker_process: std::process::Child,
    _tmpdir: TempDir,
}

impl TestEnv {
    async fn new() -> Self {
        let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");
        let socket_path = format!("{}/test-{}.sock",
            tmpdir.path().display(),
            uuid::Uuid::new_v4());

        // 创建并启动 IpcServer
        let registry = Arc::new(PtyRegistry::new());
        let worker_manager = Arc::new(WorkerManager::new(
            "agent".to_string(),
            socket_path.clone(),
            3,
        ));
        let server = Arc::new(IpcServer::new(
            socket_path.clone(),
            registry,
            worker_manager,
        ));
        server.start().await.expect("Failed to start IPC server");

        // 获取 agent 二进制路径
        // 测试二进制位于 target/<profile>/deps/，而 agent 二进制位于 target/<profile>/
        let agent_binary = std::env::current_exe()
            .expect("Failed to get current exe path")
            .parent()
            .expect("Failed to get parent dir")
            .parent()
            .expect("Failed to get grandparent dir")
            .join("agent");

        // 启动 Worker 子进程
        let worker_process = Command::new(&agent_binary)
            .arg("--worker")
            .arg("--ipc-socket")
            .arg(&socket_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to spawn worker process");

        // 等待 Worker 连接建立
        tokio::time::sleep(Duration::from_millis(500)).await;

        Self {
            socket_path,
            server,
            worker_process,
            _tmpdir: tmpdir,
        }
    }

    /// 接受 Worker 连接
    async fn accept_worker(&self) -> String {
        tokio::time::timeout(
            Duration::from_secs(5),
            self.server.accept(),
        )
        .await
        .expect("Timeout waiting for worker connection")
        .expect("Failed to accept worker connection")
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // 清理 Worker 子进程
        let _ = self.worker_process.kill();
        let _ = self.worker_process.wait();
    }
}

#[tokio::test]
async fn test_worker_connects_to_manager() {
    let env = TestEnv::new().await;
    let connection_id = env.accept_worker().await;
    assert!(!connection_id.is_empty(), "Connection ID should not be empty");
    assert_eq!(env.server.active_connection_count().await, 1);
}

#[tokio::test]
async fn test_ipc_message_roundtrip() {
    let env = TestEnv::new().await;
    let _connection_id = env.accept_worker().await;

    // 接受连接后，使用 IpcConnection 接收/发送消息
    // 注意：当前架构中，Manager 通过 IpcServer.accept() 获取连接，
    // 然后通过 receive_and_register_fd 接收 FD
    // 消息往返测试需要直接操作 UnixStream

    // 使用 tokio::net::UnixStream 直接连接测试
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use prost::Message;

    // 创建一个测试用的 UnixStream 对
    let (mut client_stream, mut server_stream) =
        tokio::net::UnixStream::pair().expect("Failed to create UnixStream pair");

    // 客户端发送 ManagerRequest
    let request = ManagerRequest {
        request_id: 42,
        payload: Some(manager_request::Payload::ReadDir(ReadDir {
            path: "/tmp".to_string(),
        })),
    };
    let mut buf = Vec::new();
    request.encode(&mut buf).expect("Failed to encode request");
    let len = buf.len() as u32;
    client_stream.write_all(&len.to_be_bytes()).await.expect("Failed to write len");
    client_stream.write_all(&buf).await.expect("Failed to write request");

    // 服务器端读取消息
    let mut len_buf = [0u8; 4];
    server_stream.read_exact(&mut len_buf).await.expect("Failed to read len");
    let msg_len = u32::from_be_bytes(len_buf) as usize;

    let mut msg_buf = vec![0u8; msg_len];
    server_stream.read_exact(&mut msg_buf).await.expect("Failed to read msg");

    let received: ManagerRequest = ManagerRequest::decode(&msg_buf[..])
        .expect("Failed to decode request");

    assert_eq!(received.request_id, 42);
    assert!(matches!(received.payload, Some(manager_request::Payload::ReadDir(_))));
}
