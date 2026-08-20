//! 隔离子命令集成测试
//!
//! 通过 spawn agent 二进制子进程，验证 5 个隔离子命令的端到端行为：
//! - run_isolated_writer：stdin → 写文件 → rename
//! - run_isolated_reader：读文件 → stdout
//! - run_isolated_writer_part：stdin → 写段文件 → sync
//! - run_isolated_merger：合并段文件 → 最终文件
//! - run_metadata：文件元数据 → JSON stdout
//!
//! 同时验证 dispatch_isolated_command 的互斥检查（多个子命令同时指定应失败）。
//!
//! 使用子进程测试的原因：子命令函数内部调用 std::process::exit()，
//! 无法直接单元测试；通过 spawn 子进程验证退出码和 I/O 是最贴近真实场景的方案。

#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Stdio};

/// 获取当前进程的 uid/gid，用于子命令降权参数
///
/// 子命令在 dispatch_isolated_command 中会调用 setgid/setuid 降权，
/// 传当前用户的 uid/gid 可确保降权成功（setuid 到自己总是允许的）
fn current_uid_gid() -> (u32, u32) {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    (uid, gid)
}

/// 生成唯一的临时文件路径（包含进程 ID + 后缀，避免多线程测试冲突）
fn unique_path(suffix: &str) -> String {
    format!("/tmp/iso_test_{}_{}.dat", std::process::id(), suffix)
}

/// 测试 isolated-reader 子命令：读文件 → stdout
#[test]
fn test_isolated_reader() {
    let agent_bin = env!("CARGO_BIN_EXE_agent");
    let (uid, gid) = current_uid_gid();

    let test_path = unique_path("reader_input");
    let test_content = b"hello isolated reader\nline2\nline3";

    // 创建测试文件
    std::fs::write(&test_path, test_content).expect("创建测试文件失败");

    // spawn reader 子进程
    let output = Command::new(agent_bin)
        .arg("--isolated-reader").arg(&test_path)
        .arg("--uid").arg(uid.to_string())
        .arg("--gid").arg(gid.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("启动 reader 子进程失败");

    // 验证退出码为 0
    assert_eq!(
        output.status.code(),
        Some(0),
        "reader 应成功退出, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 验证 stdout 内容匹配文件内容
    assert_eq!(
        output.stdout,
        test_content,
        "reader stdout 应匹配文件内容"
    );

    // 清理
    std::fs::remove_file(&test_path).ok();
}

/// 测试 isolated-writer 子命令：stdin → 写文件 → rename
#[test]
fn test_isolated_writer() {
    let agent_bin = env!("CARGO_BIN_EXE_agent");
    let (uid, gid) = current_uid_gid();

    let temp_path = unique_path("writer_temp");
    let final_path = unique_path("writer_final");
    let test_content = b"hello isolated writer\ndata chunk 2\nend";

    // spawn writer 子进程（stdin piped）
    let mut child = Command::new(agent_bin)
        .arg("--isolated-writer").arg(&temp_path)
        .arg("--final-path").arg(&final_path)
        .arg("--uid").arg(uid.to_string())
        .arg("--gid").arg(gid.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动 writer 子进程失败");

    // 通过 stdin 传数据
    {
        let stdin = child.stdin.as_mut().expect("获取 stdin 失败");
        stdin.write_all(test_content).expect("写入 stdin 失败");
    }
    // drop stdin 触发 EOF（子进程读到 EOF 后 flush+sync+rename+exit）
    drop(child.stdin.take());

    let status = child.wait().expect("等待 writer 子进程失败");

    // 验证退出码为 0
    assert_eq!(
        status.code(),
        Some(0),
        "writer 应成功退出"
    );

    // 验证 final 文件内容匹配 stdin 输入
    let content = std::fs::read(&final_path).expect("读取 final 文件失败");
    assert_eq!(
        content,
        test_content,
        "final 文件内容应匹配 stdin 输入"
    );

    // 清理
    std::fs::remove_file(&final_path).ok();
}

/// 测试 isolated-writer-part 子命令：stdin → 写段文件 → sync（不 rename）
#[test]
fn test_isolated_writer_part() {
    let agent_bin = env!("CARGO_BIN_EXE_agent");
    let (uid, gid) = current_uid_gid();

    let part_path = unique_path("writer_part");
    let test_content = b"part data chunk 1\npart data chunk 2";

    // spawn writer_part 子进程
    let mut child = Command::new(agent_bin)
        .arg("--isolated-writer-part").arg(&part_path)
        .arg("--uid").arg(uid.to_string())
        .arg("--gid").arg(gid.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("启动 writer_part 子进程失败");

    {
        let stdin = child.stdin.as_mut().expect("获取 stdin 失败");
        stdin.write_all(test_content).expect("写入 stdin 失败");
    }
    drop(child.stdin.take());

    let status = child.wait().expect("等待 writer_part 子进程失败");
    assert_eq!(status.code(), Some(0), "writer_part 应成功退出");

    // 验证段文件内容匹配 stdin 输入
    let content = std::fs::read(&part_path).expect("读取段文件失败");
    assert_eq!(content, test_content, "段文件内容应匹配 stdin 输入");

    // 清理
    std::fs::remove_file(&part_path).ok();
}

/// 测试 isolated-merger 子命令：合并段文件 → 最终文件
#[test]
fn test_isolated_merger() {
    let agent_bin = env!("CARGO_BIN_EXE_agent");
    let (uid, gid) = current_uid_gid();

    // 创建两个段文件
    let part1_path = unique_path("merger_part1");
    let part2_path = unique_path("merger_part2");
    let final_path = unique_path("merger_final");

    let part1_content = b"part1-data";
    let part2_content = b"part2-data";
    std::fs::write(&part1_path, part1_content).expect("创建段文件1失败");
    std::fs::write(&part2_path, part2_content).expect("创建段文件2失败");

    // spawn merger 子进程（part_paths 逗号分隔）
    let part_paths = format!("{},{}", part1_path, part2_path);
    let output = Command::new(agent_bin)
        .arg("--isolated-merger")
        .arg("--part-paths").arg(&part_paths)
        .arg("--final-path").arg(&final_path)
        .arg("--uid").arg(uid.to_string())
        .arg("--gid").arg(gid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("启动 merger 子进程失败");

    assert_eq!(
        output.status.code(),
        Some(0),
        "merger 应成功退出, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 验证最终文件内容 = part1 + part2（按顺序拼接）
    let content = std::fs::read(&final_path).expect("读取最终文件失败");
    let mut expected = Vec::new();
    expected.extend_from_slice(part1_content);
    expected.extend_from_slice(part2_content);
    assert_eq!(content, expected, "最终文件内容应为段文件按序拼接");

    // merger 应删除段文件
    assert!(!std::path::Path::new(&part1_path).exists(), "段文件1应被删除");
    assert!(!std::path::Path::new(&part2_path).exists(), "段文件2应被删除");

    // 清理
    std::fs::remove_file(&final_path).ok();
}

/// 测试 metadata 子命令：文件元数据 → JSON stdout
#[test]
fn test_metadata() {
    let agent_bin = env!("CARGO_BIN_EXE_agent");
    let (uid, gid) = current_uid_gid();

    let test_path = unique_path("metadata");
    let test_content = b"metadata test content";
    std::fs::write(&test_path, test_content).expect("创建测试文件失败");

    let output = Command::new(agent_bin)
        .arg("--metadata").arg(&test_path)
        .arg("--uid").arg(uid.to_string())
        .arg("--gid").arg(gid.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("启动 metadata 子进程失败");

    assert_eq!(
        output.status.code(),
        Some(0),
        "metadata 应成功退出, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 解析 JSON 输出
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"size\""), "应包含 size 字段: {}", stdout);
    assert!(stdout.contains("\"mtime\""), "应包含 mtime 字段: {}", stdout);
    // 验证 size 匹配文件大小
    assert!(
        stdout.contains(&format!("\"size\":{}", test_content.len())),
        "size 应匹配文件大小: {}",
        stdout
    );

    // 清理
    std::fs::remove_file(&test_path).ok();
}

/// 测试 dispatch 互斥检查：同时指定两个子命令应失败
#[test]
fn test_dispatch_mutex_multiple_commands() {
    let agent_bin = env!("CARGO_BIN_EXE_agent");
    let (uid, gid) = current_uid_gid();

    // 同时指定 --isolated-writer 和 --isolated-reader 应失败
    // dispatch_isolated_command 检测到 active.len() == 2，返回 Err
    let output = Command::new(agent_bin)
        .arg("--isolated-writer").arg("/tmp/iso_mutex_a")
        .arg("--isolated-reader").arg("/tmp/iso_mutex_b")
        .arg("--uid").arg(uid.to_string())
        .arg("--gid").arg(gid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("启动子进程失败");

    // 应非 0 退出（dispatch 返回 Err → main 返回 Err → 进程退出码 1）
    assert_ne!(
        output.status.code(),
        Some(0),
        "多个子命令同时指定应失败退出"
    );
}

/// 测试无子命令时 dispatch 返回 false（正常进入 Manager/Worker 模式）
///
/// 注意：不传 --isolated-* 参数时，dispatch 返回 Ok(false)，main 继续执行
/// Manager 模式会加载配置。这里只验证不因 dispatch 错误而退出。
/// 由于 Manager 模式会尝试绑定端口等，这里用 --help 验证参数解析正常。
#[test]
fn test_no_isolated_command_help() {
    let agent_bin = env!("CARGO_BIN_EXE_agent");

    // --help 由 clap 处理，验证二进制可正常启动且参数解析无误
    let output = Command::new(agent_bin)
        .arg("--help")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("启动 agent --help 失败");

    assert_eq!(output.status.code(), Some(0), "--help 应成功退出");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("GNOME Remote"), "应包含 about 描述: {}", stdout);
    assert!(stdout.contains("Usage: agent"), "应包含 Usage 行: {}", stdout);
}
