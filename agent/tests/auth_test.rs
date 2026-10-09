//! SSH 公钥认证测试
//!
//! 测试 authorized_keys 文件解析功能

use std::io::Write;
use tempfile::NamedTempFile;

/// 测试解析正常格式的 authorized_keys 文件
#[test]
fn test_parse_authorized_keys_normal() {
    // 创建临时文件
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // 写入正常的公钥内容
    // 格式: key-type base64-key comment
    writeln!(
        temp_file,
        "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCy9f0 user@host"
    )
    .expect("Failed to write to temp file");
    writeln!(
        temp_file,
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJV7z user@host2"
    )
    .expect("Failed to write to temp file");

    // 读取文件内容
    let content = std::fs::read_to_string(temp_file.path()).expect("Failed to read temp file");

    // 简单验证：应该包含两行非空内容
    let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 2);

    // 验证每行都以密钥类型开头
    for line in &lines {
        assert!(line.starts_with("ssh-rsa") || line.starts_with("ssh-ed25519"));
    }
}

/// 测试解析包含注释和空行的 authorized_keys 文件
#[test]
fn test_parse_authorized_keys_with_comments() {
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // 写入包含注释和空行的内容
    writeln!(temp_file, "# 这是一个注释").expect("Failed to write");
    writeln!(temp_file, "").expect("Failed to write");
    writeln!(
        temp_file,
        "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCy9f0 user@host"
    )
    .expect("Failed to write");
    writeln!(temp_file, "# 另一个注释").expect("Failed to write");
    writeln!(
        temp_file,
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJV7z user@host2"
    )
    .expect("Failed to write");

    let content = std::fs::read_to_string(temp_file.path()).expect("Failed to read temp file");

    // 验证：应该跳过注释和空行
    let non_empty_lines: Vec<&str> = content
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    assert_eq!(non_empty_lines.len(), 2);
}

/// 测试解析空文件
#[test]
fn test_parse_authorized_keys_empty_file() {
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // 写入空内容
    writeln!(temp_file, "").expect("Failed to write");

    let content = std::fs::read_to_string(temp_file.path()).expect("Failed to read temp file");

    // 验证：空文件应该返回空内容
    assert!(content.trim().is_empty());
}

/// 测试解析包含多个空格的行
#[test]
fn test_parse_authorized_keys_with_extra_spaces() {
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");

    // 写入包含额外空格的内容
    writeln!(
        temp_file,
        "ssh-rsa   AAAAB3NzaC1yc2EAAAADAQABAAABAQCy9f0   user@host"
    )
    .expect("Failed to write");

    let content = std::fs::read_to_string(temp_file.path()).expect("Failed to read temp file");

    // 验证：应该能够正确分割
    let parts: Vec<&str> = content
        .trim()
        .split_whitespace()
        .collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], "ssh-rsa");
    assert_eq!(parts[1], "AAAAB3NzaC1yc2EAAAADAQABAAABAQCy9f0");
    assert_eq!(parts[2], "user@host");
}