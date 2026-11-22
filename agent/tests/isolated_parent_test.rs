//! 隔离子进程父侧管道往返测试
//!
//! `cargo test` 的 `current_exe` 是测试 harness，无法解析 `--isolated-*` 参数；
//! 这里通过 `QUIRELD_EXECUTABLE` 指向 cargo 构建的真实 quireld 二进制，
//! 覆盖 `UserExecutor::spawn_isolated_writer/reader` 的管道与等待逻辑。
//!
//! 子命令本身的端到端行为见 `isolated_commands_test.rs`。

#![cfg(unix)]

use quireld::auth::{UserExecutor, UserIdentity, UserSession};
use std::io::{Read, Write};
use std::sync::Once;

/// 指向 cargo 构建的真实 quireld 二进制（Once 保证环境变量只写一次）
fn use_real_quireld() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var("QUIRELD_EXECUTABLE", env!("CARGO_BIN_EXE_quireld"));
    });
}

/// 以当前用户身份构造执行器，避免测试环境降权失败
fn executor_for_current_user() -> UserExecutor {
    let uid = nix::unistd::getuid().as_raw();
    let gid = nix::unistd::getgid().as_raw();
    let identity = UserIdentity::new(
        "testuser".to_string(),
        uid,
        gid,
        "/tmp".to_string(),
        "/bin/bash".to_string(),
    );
    let session = UserSession::new(identity);
    UserExecutor::new(&session)
}

/// spawn_isolated_writer：父进程写 pipe → 子进程落盘 → 等待并校验内容
#[test]
fn test_spawn_isolated_writer_round_trip() {
    use_real_quireld();

    let tmp = tempfile::tempdir().expect("创建 tempdir 失败");
    let final_path = tmp.path().join("test_upload.bin");
    let temp_path = format!("{}.tmp", final_path.to_string_lossy());
    let executor = executor_for_current_user();

    let data = vec![0xAA; 1024 * 100]; // 100KB
    let (mut pipe_writer, child_pid) = executor
        .spawn_isolated_writer(&temp_path, &final_path.to_string_lossy(), data.len() as u64)
        .expect("spawn_isolated_writer 失败");

    pipe_writer.write_all(&data).expect("写入 pipe 失败");
    drop(pipe_writer); // EOF signal

    executor.wait_isolated_child(child_pid).expect("wait 失败");

    let mut read_data = Vec::new();
    std::fs::File::open(&final_path)
        .expect("打开 final 失败")
        .read_to_end(&mut read_data)
        .expect("读取失败");
    assert_eq!(read_data, data);
}

/// spawn_isolated_reader：子进程读文件 → 父进程读 pipe → 校验内容
#[test]
fn test_spawn_isolated_reader_round_trip() {
    use_real_quireld();

    let tmp = tempfile::tempdir().expect("创建 tempdir 失败");
    let path = tmp.path().join("test_download.bin");
    let data = vec![0xBB; 1024 * 100];
    std::fs::write(&path, &data).expect("写入测试文件失败");
    let executor = executor_for_current_user();

    let (mut pipe_reader, child_pid) = executor
        .spawn_isolated_reader(&path.to_string_lossy())
        .expect("spawn_isolated_reader 失败");

    let mut read_data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match pipe_reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => read_data.extend_from_slice(&buf[..n]),
            Err(e) => panic!("读取 pipe 失败: {}", e),
        }
    }
    drop(pipe_reader);
    executor.wait_isolated_child(child_pid).expect("wait 失败");
    assert_eq!(read_data, data);
}

// ============================================================================
// A-04：全局 TRANSFER_SESSIONS 锁不得跨阻塞 abort/finish 持有
// ============================================================================

/// A-04 回归：cancel 的 abort（阻塞 waitpid）不得持全局 TRANSFER_SESSIONS 锁。
///
/// SIGSTOP 停住隔离子进程，使 cancel 的 abort 卡死在 waitpid（SIGTERM 对
/// stopped 进程保持 pending，需 SIGCONT 才投递）；此刻经 handle_envelope 对
/// 另一会话发起 CancelFileTransfer——其锁获取必须立即成功。旧实现在 abort
/// 期间持有全局 tokio Mutex，第二个请求会永久阻塞，本测试以 2s 超时判红。
#[tokio::test]
async fn cancel_abort_does_not_hold_global_transfer_lock() {
    use_real_quireld();

    use quireld::audit::AuditLogger;
    use quireld::auth::StatsManager;
    use quireld::config::AgentConfig;
    use quireld::file_stream::PipeFileStreamWriter;
    use quireld::handler::{handle_envelope, TRANSFER_SESSIONS};
    use quireld::protocol::{Envelope, Payload};
    use quireld::transfer_session::TransferSession;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::timeout;

    let uid = nix::unistd::getuid().as_raw();
    let executor = executor_for_current_user();
    let make_session = || {
        let identity = UserIdentity::new(
            "testuser".to_string(),
            uid,
            nix::unistd::getgid().as_raw(),
            "/tmp".to_string(),
            "/bin/bash".to_string(),
        );
        UserSession::new(identity)
    };

    let tmp = tempfile::tempdir().expect("创建 tempdir 失败");
    let final_path = tmp.path().join("a.bin");
    let temp_path = format!("{}.tmp", final_path.to_string_lossy());

    // 会话 A：真实 writer 子进程，SIGSTOP 使其停在 read(stdin)（保持 stdin
    // 打开避免子进程先 EOF 退出），后续 abort 的 kill(SIGTERM) 对 stopped
    // 进程保持 pending → waitpid 无限阻塞
    let (pipe_writer, child_pid) = executor
        .spawn_isolated_writer(&temp_path, &final_path.to_string_lossy(), 0)
        .expect("spawn_isolated_writer 失败");
    unsafe { libc::kill(child_pid, libc::SIGSTOP); }

    let writer = PipeFileStreamWriter::new(pipe_writer, child_pid, executor, 0, temp_path);
    let sid_a = "sess-a04-a".to_string();
    let sid_b = "sess-a04-b".to_string();
    let mut session_a = TransferSession::new(
        sid_a.clone(),
        "upload".to_string(),
        final_path.to_string_lossy().to_string(),
        0,
        1,
        uid,
    );
    session_a.writer = Some(writer);
    let session_b = TransferSession::new(
        sid_b.clone(),
        "download".to_string(),
        "/tmp/a04-b.bin".to_string(),
        0,
        1,
        uid,
    );
    TRANSFER_SESSIONS.lock().await.insert(sid_a.clone(), session_a);
    TRANSFER_SESSIONS.lock().await.insert(sid_b.clone(), session_b);

    let cfg = Arc::new(AgentConfig::default());
    let stats = Arc::new(StatsManager::new());
    let audit = Arc::new(
        AuditLogger::new(&tmp.path().join("audit.log").to_string_lossy())
            .expect("创建审计日志失败"),
    );

    // 并发发起 cancel A：remove A（随即释放锁）→ spawn_blocking abort → 卡在 waitpid
    let cancel_a = {
        let (cfg, stats, audit) = (Arc::clone(&cfg), Arc::clone(&stats), Arc::clone(&audit));
        let sid = sid_a.clone();
        let sess = make_session();
        tokio::spawn(async move {
            let envelope = Envelope::new(
                1,
                Payload::CancelFileTransfer { session_id: sid, reason: "a04-test".to_string() },
            );
            handle_envelope(&envelope, &cfg, &sess, stats, audit).await
        })
    };

    // 等待 cancel A 进入 waitpid 阻塞态
    tokio::time::sleep(Duration::from_millis(300)).await;

    // 关键断言：此刻 B 的取消请求必须能立即拿到全局锁并完成
    let resp_b = timeout(Duration::from_secs(2), {
        let (cfg, stats, audit) = (Arc::clone(&cfg), Arc::clone(&stats), Arc::clone(&audit));
        let sid = sid_b.clone();
        let sess = make_session();
        async move {
            let envelope = Envelope::new(
                2,
                Payload::CancelFileTransfer { session_id: sid, reason: "a04-test".to_string() },
            );
            handle_envelope(&envelope, &cfg, &sess, stats, audit).await
        }
    })
    .await
    .expect("旧实现在 abort 期间持全局锁，B 的请求会超时（A-04 复现）");
    match resp_b.payload {
        Payload::CancelFileTransferResponse { success, .. } => assert!(success),
        other => panic!("预期 CancelFileTransferResponse，实际 {:?}", other),
    }

    // 清理：SIGCONT 投递 pending 的 SIGTERM → 子进程退出 → cancel A 完成
    unsafe { libc::kill(child_pid, libc::SIGCONT); }
    let resp_a = timeout(Duration::from_secs(5), cancel_a)
        .await
        .expect("cancel A 应在 SIGCONT 后完成")
        .expect("cancel A 任务 join 失败");
    match resp_a.payload {
        Payload::CancelFileTransferResponse { success, .. } => assert!(success),
        other => panic!("预期 CancelFileTransferResponse，实际 {:?}", other),
    }
}
