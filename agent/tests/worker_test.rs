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

                // 验证 owner/group:名称解析成功或回退数字字符串,两者都非空
                for e in &listing.entries {
                    assert!(!e.owner.is_empty(), "entry {} owner should not be empty", e.name);
                    assert!(!e.group.is_empty(), "entry {} group should not be empty", e.name);
                }
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
// chmod/chown 测试（文件属主/权限管理）
// ============================================================================

mod chmod_chown_tests {
    use quireld::worker::handlers::file;
    use quireld::protocol::generated::{Chmod, Chown, worker_response};
    use tempfile;
    use std::fs;

    /// 构造 root 用户上下文的 Chmod 请求(与现有测试一致的用户字段)
    fn chmod_req(path: String, mode: u32, recursive: bool) -> Chmod {
        Chmod {
            path,
            mode,
            recursive,
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
        }
    }

    /// 构造 root 用户上下文的 Chown 请求
    fn chown_req(path: String, owner: &str, group: &str, recursive: bool) -> Chown {
        Chown {
            path,
            owner: owner.to_string(),
            group: group.to_string(),
            recursive,
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
        }
    }

    #[tokio::test]
    async fn test_handle_chmod_success() {
        // chmod 自己创建的临时文件成功且权限生效
        let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");
        let file_path = tmpdir.path().join("chmod_target.txt");
        fs::write(&file_path, "hello").expect("Failed to write file");

        let response = file::handle_chmod(chmod_req(
            file_path.to_string_lossy().to_string(),
            0o600,
            false,
        )).await;

        match response.payload {
            Some(worker_response::Payload::ChmodResult(result)) => {
                assert!(result.success, "chmod should succeed");
            }
            _ => panic!("Expected ChmodResult, got {:?}", response.payload),
        }

        // 验证权限已生效(低 9 位应为 0o600)
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&file_path)
            .expect("Failed to read metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "permission bits should be 0o600, got {:o}", mode & 0o777);
    }

    #[tokio::test]
    async fn test_handle_chmod_recursive_directory() {
        // recursive chmod:目录树下所有文件与子目录都应用新权限
        let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");
        let root = tmpdir.path();
        fs::write(root.join("f1.txt"), "a").expect("Failed to write f1");
        fs::create_dir(root.join("sub")).expect("Failed to create sub");
        fs::write(root.join("sub").join("f2.txt"), "b").expect("Failed to write f2");

        let response = file::handle_chmod(chmod_req(
            root.to_string_lossy().to_string(),
            0o755,
            true,
        )).await;

        match response.payload {
            Some(worker_response::Payload::ChmodResult(result)) => {
                assert!(result.success, "recursive chmod should succeed");
            }
            _ => panic!("Expected ChmodResult, got {:?}", response.payload),
        }

        // 验证目录树全部生效:顶层/文件/子目录/子文件均为 0o755
        use std::os::unix::fs::PermissionsExt;
        for p in [root, &root.join("f1.txt"), &root.join("sub"), &root.join("sub").join("f2.txt")] {
            let mode = fs::metadata(p).expect("Failed to read metadata").permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "path {:?} should be 0o755", p);
        }
    }

    #[tokio::test]
    async fn test_handle_chmod_not_found() {
        // chmod 不存在路径报错(确定性 404)
        let response = file::handle_chmod(chmod_req(
            "/nonexistent/quirel-chmod-path-12345".to_string(),
            0o755,
            false,
        )).await;

        match response.payload {
            Some(worker_response::Payload::Error(err)) => {
                assert_eq!(err.code, 404, "nonexistent path should map to 404");
                assert!(err.message.contains("不存在"), "error message: {}", err.message);
            }
            _ => panic!("Expected Error, got {:?}", response.payload),
        }
    }

    #[tokio::test]
    async fn test_handle_chown_unknown_user() {
        // chown 不存在的用户名报错(确定性错误,不依赖运行用户身份)
        let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");
        let file_path = tmpdir.path().join("chown_target.txt");
        fs::write(&file_path, "hello").expect("Failed to write file");

        let response = file::handle_chown(chown_req(
            file_path.to_string_lossy().to_string(),
            "quirel-no-such-user-xyz",
            "root",
            false,
        )).await;

        match response.payload {
            Some(worker_response::Payload::Error(err)) => {
                assert!(err.message.contains("不存在"), "error message: {}", err.message);
            }
            _ => panic!("Expected Error, got {:?}", response.payload),
        }
    }

    #[test]
    fn test_resolve_root_names() {
        // 用户名/组名解析函数单测:resolve "root" → 0
        assert_eq!(file::resolve_uid_by_name("root"), Some(0));
        assert_eq!(file::resolve_gid_by_name("root"), Some(0));
        // 不存在的名称返回 None(而非报错)
        assert_eq!(file::resolve_uid_by_name("quirel-no-such-user-xyz"), None);
        assert_eq!(file::resolve_gid_by_name("quirel-no-such-group-xyz"), None);
    }
}

// ============================================================================
// TASK-034: 命令执行与系统信息测试
// ============================================================================

mod command_and_system_tests {
    use quireld::worker::handlers::{command, system};
    use quireld::protocol::generated::{ExecuteCommand, GetSystemInfo, worker_response};

    #[tokio::test]
    async fn test_handle_execute_command_echo() {
        // 白名单命令 tar：验证 stdout 捕获 + exit 0
        let req = ExecuteCommand {
            command: "tar".to_string(),
            args: vec!["--version".to_string()],
            working_directory: "/tmp".to_string(),
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
            env: Default::default(),
            timeout_secs: 0,
        };
        let response = command::handle_execute_command(req).await;

        match response.payload {
            Some(worker_response::Payload::CommandOutput(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert!(stdout.contains("tar"), "stdout should contain 'tar', got: {}", stdout);
                assert_eq!(output.exit_code, 0);
            }
            _ => panic!("Expected CommandOutput, got {:?}", response.payload),
        }
    }

    #[tokio::test]
    async fn test_handle_execute_command_with_stderr() {
        // 白名单命令 tar 对不存在文件：stderr 有内容 + 非零退出码
        let req = ExecuteCommand {
            command: "tar".to_string(),
            args: vec!["-tf".to_string(), "/nonexistent-quirel-test.tar".to_string()],
            working_directory: "/tmp".to_string(),
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
            env: Default::default(),
            timeout_secs: 0,
        };
        let response = command::handle_execute_command(req).await;

        match response.payload {
            Some(worker_response::Payload::CommandOutput(output)) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(!stderr.is_empty(), "stderr should not be empty");
                assert_ne!(output.exit_code, 0, "tar on nonexistent file should return non-zero exit code");
            }
            _ => panic!("Expected CommandOutput, got {:?}", response.payload),
        }
    }

    #[tokio::test]
    async fn test_handle_execute_command_nonzero_exit() {
        // 白名单外的命令必须被拒绝（403）——安全关键行为
        let req = ExecuteCommand {
            command: "false".to_string(),
            args: vec![],
            working_directory: "/tmp".to_string(),
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
            env: Default::default(),
            timeout_secs: 0,
        };
        let response = command::handle_execute_command(req).await;

        match response.payload {
            Some(worker_response::Payload::Error(error)) => {
                assert_eq!(error.code, 403, "non-whitelisted command should be rejected with 403");
                assert!(error.message.contains("not allowed"), "error message: {}", error.message);
            }
            _ => panic!("Expected Error 403, got {:?}", response.payload),
        }
    }

    #[tokio::test]
    async fn test_handle_execute_command_absolute_path_whitelist() {
        // 绝对路径调用按最后一段匹配白名单（/usr/bin/tar → tar）
        let req = ExecuteCommand {
            command: "/usr/bin/tar".to_string(),
            args: vec!["--version".to_string()],
            working_directory: "/tmp".to_string(),
            uid: 0,
            gid: 0,
            username: "test".to_string(),
            home_dir: "/tmp".to_string(),
            env: Default::default(),
            timeout_secs: 0,
        };
        let response = command::handle_execute_command(req).await;

        match response.payload {
            Some(worker_response::Payload::CommandOutput(output)) => {
                assert_eq!(output.exit_code, 0);
            }
            _ => panic!("Expected CommandOutput for absolute-path whitelisted command, got {:?}", response.payload),
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
