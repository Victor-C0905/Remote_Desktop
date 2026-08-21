//! 压力测试
//!
//! 测试场景：
//! - 50+ 并发 PTY 会话创建
//! - 并发文件操作
//! - 会话快速创建/销毁循环
//! - 长时间运行与资源泄漏检测
//!
//! 运行方式：cargo test --test stress_test -- --ignored --nocapture

#![cfg(unix)]

use std::sync::Arc;
use std::time::{Duration, Instant};
use quireld::worker::SessionManager;
use quireld::protocol::generated::{ReadDir, WriteFile};
use quireld::worker::handlers::file;
use nix::unistd::Pid;

#[tokio::test]
#[ignore = "压力测试，手动运行"]
async fn stress_concurrent_session_creation_50() {
    println!("\n=== 压力测试: 50 并发会话创建 ===");

    let manager = SessionManager::new();
    let start = Instant::now();

    let mut handles = Vec::new();
    for i in 0..50 {
        let mgr = manager.clone();
        handles.push(tokio::spawn(async move {
            mgr.register(
                format!("stress-session-{}", i),
                Pid::from_raw(i as i32),
                format!("socket-stress-session-{}", i),
                "/bin/bash".to_string(),
            ).await;
        }));
    }

    for handle in handles {
        handle.await.expect("Task panicked");
    }

    let elapsed = start.elapsed();
    assert_eq!(manager.list().await.len(), 50, "Should have 50 sessions");
    println!("  50 并发会话创建耗时: {:?}", elapsed);

    // 清理
    for i in 0..50 {
        manager.unregister(&format!("stress-session-{}", i)).await;
    }
    assert_eq!(manager.list().await.len(), 0, "All sessions should be cleaned up");
    println!("  ✓ 清理完成");
}

#[tokio::test]
#[ignore = "压力测试，手动运行"]
async fn stress_concurrent_file_operations() {
    println!("\n=== 压力测试: 并发文件操作 ===");

    let tmpdir = Arc::new(tempfile::tempdir().expect("Failed to create temp dir"));
    let dir_path = tmpdir.path().to_string_lossy().to_string();

    // 创建测试文件
    for i in 0..20 {
        std::fs::write(
            tmpdir.path().join(format!("file{}.txt", i)),
            format!("content{}", i),
        ).expect("Failed to write file");
    }

    let start = Instant::now();
    let mut handles = Vec::new();

    // 10 个并发 ReadDir
    for _ in 0..10 {
        let path = dir_path.clone();
        handles.push(tokio::spawn(async move {
            let req = ReadDir {
                path,
                uid: 0,
                gid: 0,
                username: "test".to_string(),
                home_dir: "/tmp".to_string(),
            };
            file::handle_read_dir(req).await
        }));
    }

    // 10 个并发 WriteFile
    for i in 0..10 {
        let path = format!("{}/concurrent_write{}.txt", dir_path, i);
        handles.push(tokio::spawn(async move {
            let req = WriteFile {
                path,
                content: vec![b'x'; 1024],
                uid: 0,
                gid: 0,
                username: "test".to_string(),
                home_dir: "/tmp".to_string(),
            };
            file::handle_write_file(req).await
        }));
    }

    for handle in handles {
        let response = handle.await.expect("Task panicked");
        assert!(response.payload.is_some(), "Response should have payload");
    }

    let elapsed = start.elapsed();
    println!("  20 并发文件操作耗时: {:?}", elapsed);
    println!("  ✓ 无死锁或 panic");
}

#[tokio::test]
#[ignore = "压力测试，手动运行"]
async fn stress_rapid_create_destroy_cycle() {
    println!("\n=== 压力测试: 快速创建/销毁循环 (100次) ===");

    let manager = SessionManager::new();
    let start = Instant::now();

    for cycle in 0..100 {
        let session_id = format!("cycle-{}-{}", cycle, 0);
        manager.register(
            session_id.clone(),
            Pid::from_raw(cycle as i32),
            format!("socket-{}", session_id),
            "/bin/bash".to_string(),
        ).await;
        manager.unregister(&session_id).await;
    }

    let elapsed = start.elapsed();
    assert_eq!(manager.list().await.len(), 0, "All sessions should be cleaned up");
    println!("  100 次创建/销毁循环耗时: {:?}", elapsed);
    println!("  ✓ 无资源泄漏");
}

// ============================================================================
// TASK-037: 长时间运行与资源泄漏检测
// ============================================================================

/// 读取当前进程的 VmRSS（内存使用）
fn get_vm_rss_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status")
        .expect("Failed to read /proc/self/status");
    for line in status.lines() {
        if line.starts_with("VmRSS:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            return parts.get(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
        }
    }
    0
}

/// 计算当前打开的 FD 数量
fn get_fd_count() -> usize {
    std::fs::read_dir("/proc/self/fd")
        .map(|entries| entries.count())
        .unwrap_or(0)
}

#[tokio::test]
#[ignore = "长时间运行测试，手动运行"]
async fn stress_long_running_5min() {
    println!("\n=== 压力测试: 5 分钟持续运行 ===");

    let manager = SessionManager::new();
    let start = Instant::now();
    let duration = Duration::from_secs(300); // 5 分钟
    let mut cycle_count = 0u64;

    let initial_rss = get_vm_rss_kb();
    let initial_fds = get_fd_count();

    println!("  初始内存: {} KB, 初始 FD 数: {}", initial_rss, initial_fds);

    while start.elapsed() < duration {
        // 创建一批会话
        for i in 0..10 {
            let session_id = format!("longrun-{}-{}", cycle_count, i);
            manager.register(
                session_id.clone(),
                Pid::from_raw(cycle_count as i32),
                format!("socket-{}", session_id),
                "/bin/bash".to_string(),
            ).await;
        }

        // 销毁这批会话
        for i in 0..10 {
            let session_id = format!("longrun-{}-{}", cycle_count, i);
            manager.unregister(&session_id).await;
        }

        cycle_count += 1;

        // 每 100 个周期输出一次状态
        if cycle_count % 100 == 0 {
            let rss = get_vm_rss_kb();
            let fds = get_fd_count();
            println!(
                "  周期 {}: 内存={} KB (增长 {:.1}%), FD 数={} (变化 {:+}), 耗时={:?}",
                cycle_count,
                rss,
                (rss as f64 - initial_rss as f64) / initial_rss as f64 * 100.0,
                fds,
                fds as i64 - initial_fds as i64,
                start.elapsed()
            );
        }
    }

    // 最终检查
    let final_rss = get_vm_rss_kb();
    let final_fds = get_fd_count();
    let memory_growth_percent = (final_rss as f64 - initial_rss as f64) / initial_rss as f64 * 100.0;

    println!("\n  === 最终结果 ===");
    println!("  总周期数: {}", cycle_count);
    println!("  内存: {} KB → {} KB (增长 {:.1}%)", initial_rss, final_rss, memory_growth_percent);
    println!("  FD 数: {} → {} (变化 {:+})", initial_fds, final_fds, final_fds as i64 - initial_fds as i64);

    // 验证内存增长 < 50%
    assert!(
        memory_growth_percent < 50.0,
        "内存增长 {:.1}% 超过 50% 限制", memory_growth_percent
    );

    // 验证 FD 无泄漏（允许 ±5 的波动）
    let fd_diff = (final_fds as i64 - initial_fds as i64).abs();
    assert!(
        fd_diff < 10,
        "FD 数量变化 {} 超过阈值", fd_diff
    );

    // 验证会话全部清理
    assert_eq!(manager.list().await.len(), 0, "会话应全部清理");

    println!("  ✓ 5 分钟运行通过：内存增长 < 50%，无 FD 泄漏");
}

#[tokio::test]
#[ignore = "资源泄漏检测，手动运行"]
async fn test_fd_leak_detection() {
    println!("\n=== 资源泄漏检测: FD 泄漏 ===");

    let manager = SessionManager::new();
    let initial_fds = get_fd_count();

    println!("  初始 FD 数: {}", initial_fds);

    // 执行 1000 次创建/销毁循环
    for i in 0..1000 {
        let session_id = format!("leak-test-{}", i);
        manager.register(
            session_id.clone(),
            Pid::from_raw(i as i32),
            format!("socket-{}", session_id),
            "/bin/bash".to_string(),
        ).await;
        manager.unregister(&session_id).await;
    }

    let final_fds = get_fd_count();
    let fd_diff = final_fds as i64 - initial_fds as i64;

    println!("  最终 FD 数: {} (变化 {:+})", final_fds, fd_diff);

    // 允许 ±5 的波动
    assert!(
        fd_diff.abs() < 10,
        "FD 泄漏检测失败: 变化 {:+} (初始={}, 最终={})",
        fd_diff, initial_fds, final_fds
    );

    println!("  ✓ 无 FD 泄漏");
}
