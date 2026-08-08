//! 性能基准测试
//!
//! 建立当前性能基准数据，包括：
//! - PTY 吞吐量（写入/读取大块数据）
//! - IPC 往返延迟
//! - 并发会话创建基准
//!
//! 运行方式：cargo test --test bench_test -- --ignored --nocapture

#![cfg(unix)]

use std::time::Instant;
use gnome_remote_agent::worker::SessionManager;
use gnome_remote_agent::protocol::generated::{
    ReadDir, ExecuteCommand,
};
use gnome_remote_agent::worker::handlers::{file, command};
use nix::unistd::Pid;

/// 运行基准测试并输出结果（async 版本，避免 runtime 嵌套）
async fn bench<F, Fut>(name: &str, iterations: usize, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    // 预热
    f().await;

    let mut times = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        f().await;
        times.push(start.elapsed());
    }

    times.sort();
    let avg_us = times.iter().map(|t| t.as_micros()).sum::<u128>() / iterations as u128;
    let median_us = times[iterations / 2].as_micros();
    let p99_us = times[(iterations as f64 * 0.99) as usize].as_micros();

    println!(
        "  {}: avg={}μs, median={}μs, p99={}μs ({} iterations)",
        name, avg_us, median_us, p99_us, iterations
    );
}

#[tokio::test]
#[ignore = "性能基准测试，手动运行"]
async fn bench_read_dir() {
    println!("\n=== 性能基准: ReadDir ===");

    // 创建临时目录和文件
    let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");
    for i in 0..100 {
        std::fs::write(
            tmpdir.path().join(format!("file{}.txt", i)),
            "test",
        ).expect("Failed to write file");
    }

    let path = tmpdir.path().to_string_lossy().to_string();

    bench("ReadDir (100 files)", 100, || {
        let path = path.clone();
        async move {
            let req = ReadDir {
                path,
                uid: 0,
                gid: 0,
                username: "test".to_string(),
                home_dir: "/tmp".to_string(),
            };
            let _ = file::handle_read_dir(req).await;
        }
    }).await;
}

#[tokio::test]
#[ignore = "性能基准测试，手动运行"]
async fn bench_execute_command() {
    println!("\n=== 性能基准: ExecuteCommand ===");

    bench("echo hello", 50, || {
        async move {
            let req = ExecuteCommand {
                command: "echo".to_string(),
                args: vec!["hello".to_string()],
                working_directory: "/tmp".to_string(),
                uid: 0,
                gid: 0,
                username: "test".to_string(),
                home_dir: "/tmp".to_string(),
            };
            let _ = command::handle_execute_command(req).await;
        }
    }).await;
}

#[tokio::test]
#[ignore = "性能基准测试，手动运行"]
async fn bench_session_manager_concurrent() {
    println!("\n=== 性能基准: 并发会话注册 ===");

    let manager = SessionManager::new();

    bench("注册 100 个会话", 10, || {
        let mgr = manager.clone();
        async move {
            // 注册 100 个会话
            for i in 0..100 {
                mgr.register(
                    format!("bench-session-{}", i),
                    Pid::from_raw(i as i32),
                    format!("socket-bench-session-{}", i),
                    "/bin/bash".to_string(),
                ).await;
            }
            // 清理
            for i in 0..100 {
                mgr.unregister(&format!("bench-session-{}", i)).await;
            }
        }
    }).await;

    println!("\n性能基准测试完成");
}
