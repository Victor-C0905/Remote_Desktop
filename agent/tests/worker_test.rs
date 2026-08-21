//! Worker 业务逻辑测试
//!
//! 测试场景：
//! - PTY 会话创建（PtyFactory）
//! - SessionManager 注册/注销/并发访问

#![cfg(unix)]

use quireld::worker::{PtyFactory, SessionManager};
use quireld::protocol::generated::{ReadDir, ReadFile, WriteFile};
use nix::unistd::Pid;

#[tokio::test]
async fn test_pty_factory_creation() {
    let factory = PtyFactory::new();
    // 只验证创建成功，不实际调用 create（需要 IpcClient）
    drop(factory);
    assert!(true, "PtyFactory created successfully");
}

#[tokio::test]
async fn test_session_manager_register_unregister() {
    let manager = SessionManager::new();

    // 注册会话
    manager.register(
        "test-session-1".to_string(),
        Pid::from_raw(12345),
        format!("socket-{}", "test-session-1"),
        "/bin/bash".to_string(),
    ).await;
    assert_eq!(manager.list().await.len(), 1);

    // 注销会话
    manager.unregister("test-session-1").await;
    assert_eq!(manager.list().await.len(), 0);
}

#[tokio::test]
async fn test_session_manager_concurrent_access() {
    let manager = SessionManager::new();
    let manager_clone = manager.clone();

    // 并发注册
    let handle = tokio::spawn(async move {
        for i in 0..10 {
            manager_clone.register(
                format!("session-{}", i),
                Pid::from_raw(i as i32),
                format!("socket-{}", i),
                "/bin/bash".to_string(),
            ).await;
        }
    });

    // 同时在主线程注册
    for i in 10..20 {
        manager.register(
            format!("session-{}", i),
            Pid::from_raw(i as i32),
            format!("socket-{}", i),
            "/bin/bash".to_string(),
        ).await;
    }

    handle.await.expect("Task panicked");

    assert_eq!(manager.list().await.len(), 20);
}

// ============================================================================
// TASK-033: 文件操作业务测试
// ============================================================================

mod file_operation_tests {
    use super::*;
    use quireld::worker::handlers::file;
    use quireld::protocol::generated::worker_response;
    use tempfile;
    use std::fs;

    #[tokio::test]
    async fn test_handle_read_dir_success() {
        // 创建临时目录和测试文件
        let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");
        let dir_path = tmpdir.path();

        // 创建测试文件
        fs::write(dir_path.join("file1.txt"), "hello").expect("Failed to write file1");
        fs::write(dir_path.join("file2.txt"), "world").expect("Failed to write file2");
        fs::create_dir(dir_path.join("subdir")).expect("Failed to create subdir");

        // 调用 handle_read_dir
        let req = ReadDir {
            path: dir_path.to_string_lossy().to_string(),
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
        };
        let response = file::handle_read_dir(req).await;

        // 验证响应
        match response.payload {
            Some(worker_response::Payload::DirListing(listing)) => {
                assert!(listing.entries.len() >= 3, "Should have at least 3 entries");

                // 验证条目
                let names: Vec<&str> = listing.entries.iter()
                    .map(|e| e.name.as_str())
                    .collect();
                assert!(names.contains(&"file1.txt"));
                assert!(names.contains(&"file2.txt"));
                assert!(names.contains(&"subdir"));
            }
            _ => panic!("Expected DirListing, got {:?}", response.payload),
        }
    }

    #[tokio::test]
    async fn test_handle_read_dir_not_found() {
        let req = ReadDir {
            path: "/nonexistent/path/12345".to_string(),
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
        };
        let response = file::handle_read_dir(req).await;

        match response.payload {
            Some(worker_response::Payload::Error(err)) => {
                assert_eq!(err.code, 404);
                assert!(err.message.contains("not found"));
            }
            _ => panic!("Expected Error, got {:?}", response.payload),
        }
    }

    #[tokio::test]
    async fn test_handle_read_file_success() {
        let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");
        let file_path = tmpdir.path().join("test.txt");
        fs::write(&file_path, "test content").expect("Failed to write file");

        let req = ReadFile {
            path: file_path.to_string_lossy().to_string(),
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
        };
        let response = file::handle_read_file(req).await;

        match response.payload {
            Some(worker_response::Payload::FileContent(content)) => {
                let text = String::from_utf8(content.content).expect("Invalid UTF-8");
                assert_eq!(text, "test content");
            }
            _ => panic!("Expected FileContent, got {:?}", response.payload),
        }
    }

    #[tokio::test]
    async fn test_handle_write_file_success() {
        let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");
        let file_path = tmpdir.path().join("output.txt");
        let test_data = b"hello world";

        let req = WriteFile {
            path: file_path.to_string_lossy().to_string(),
            content: test_data.to_vec(),
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
        };
        let response = file::handle_write_file(req).await;

        match response.payload {
            Some(worker_response::Payload::WriteResult(result)) => {
                assert_eq!(result.size as usize, test_data.len());
            }
            _ => panic!("Expected WriteResult, got {:?}", response.payload),
        }

        // 验证文件内容
        let content = fs::read(&file_path).expect("Failed to read file");
        assert_eq!(content, test_data);
    }
}

// ============================================================================
// TASK-034: 命令执行与系统信息测试
// ============================================================================

mod command_and_system_tests {
    use super::*;
    use quireld::worker::handlers::{command, system};
    use quireld::protocol::generated::{ExecuteCommand, GetSystemInfo, worker_response};

    #[tokio::test]
    async fn test_handle_execute_command_echo() {
        let req = ExecuteCommand {
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
            working_directory: "/tmp".to_string(),
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
        };
        let response = command::handle_execute_command(req).await;

        match response.payload {
            Some(worker_response::Payload::CommandOutput(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert!(stdout.contains("hello"), "stdout should contain 'hello', got: {}", stdout);
                assert_eq!(output.exit_code, 0);
            }
            _ => panic!("Expected CommandOutput, got {:?}", response.payload),
        }
    }

    #[tokio::test]
    async fn test_handle_execute_command_with_stderr() {
        // 使用 sh -c 'echo error >&2' 来产生 stderr
        let req = ExecuteCommand {
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "echo error >&2".to_string()],
            working_directory: "/tmp".to_string(),
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
        };
        let response = command::handle_execute_command(req).await;

        match response.payload {
            Some(worker_response::Payload::CommandOutput(output)) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(stderr.contains("error"), "stderr should contain 'error', got: {}", stderr);
                assert_eq!(output.exit_code, 0);
            }
            _ => panic!("Expected CommandOutput, got {:?}", response.payload),
        }
    }

    #[tokio::test]
    async fn test_handle_execute_command_nonzero_exit() {
        let req = ExecuteCommand {
            command: "false".to_string(),
            args: vec![],
            working_directory: "/tmp".to_string(),
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
        };
        let response = command::handle_execute_command(req).await;

        match response.payload {
            Some(worker_response::Payload::CommandOutput(output)) => {
                assert_ne!(output.exit_code, 0, "false command should return non-zero exit code");
            }
            _ => panic!("Expected CommandOutput, got {:?}", response.payload),
        }
    }

    #[tokio::test]
    async fn test_handle_get_system_info() {
        let req = GetSystemInfo {};
        let response = system::handle_get_system_info(req).await;

        match response.payload {
            Some(worker_response::Payload::SystemInfo(info)) => {
                // 验证主机名不为空
                assert!(!info.hostname.is_empty(), "Hostname should not be empty");
                // 验证操作系统名不为空
                assert!(!info.os_name.is_empty(), "OS name should not be empty");
            }
            _ => panic!("Expected SystemInfo, got {:?}", response.payload),
        }
    }
}
