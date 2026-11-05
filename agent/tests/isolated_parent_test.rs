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
