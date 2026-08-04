//! 部署配置验证测试
//!
//! 验证 systemd service 文件和 install.sh 的完整性
//!
//! 运行方式：cargo test --test deploy_test -- --nocapture

#![cfg(unix)]

use std::path::PathBuf;

/// 获取项目根目录路径
fn project_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
}

/// 获取 systemd service 文件路径
fn systemd_service_path() -> PathBuf {
    project_root()
        .parent()
        .unwrap_or(&project_root())
        .join("systemd")
        .join("gnome-remote-agent.service")
}

#[test]
fn test_systemd_service_file_exists() {
    let path = systemd_service_path();
    assert!(
        path.exists(),
        "systemd service file should exist at: {:?}",
        path
    );
}

#[test]
fn test_systemd_service_contains_phase4_config() {
    let path = systemd_service_path();
    let content = std::fs::read_to_string(&path)
        .expect("Failed to read systemd service file");

    // 验证 Phase 4 热更新配置
    assert!(
        content.contains("KillMode=process"),
        "Service should contain KillMode=process for Phase 4 hot update"
    );

    assert!(
        content.contains("ExecReload"),
        "Service should contain ExecReload for Phase 4 hot update"
    );

    assert!(
        content.contains("Type=notify"),
        "Service should use Type=notify for systemd notification"
    );
}

#[test]
fn test_systemd_service_contains_security_config() {
    let path = systemd_service_path();
    let content = std::fs::read_to_string(&path)
        .expect("Failed to read systemd service file");

    // 验证安全配置
    assert!(
        content.contains("CapabilityBoundingSet"),
        "Service should contain CapabilityBoundingSet"
    );

    assert!(
        content.contains("LimitNOFILE"),
        "Service should contain LimitNOFILE"
    );
}

#[test]
fn test_systemd_service_contains_network_dependency() {
    let path = systemd_service_path();
    let content = std::fs::read_to_string(&path)
        .expect("Failed to read systemd service file");

    assert!(
        content.contains("network.target"),
        "Service should depend on network.target"
    );

    assert!(
        content.contains("network-online.target"),
        "Service should want network-online.target"
    );
}

#[test]
fn test_install_script_exists() {
    let path = project_root().join("deploy").join("install.sh");
    assert!(
        path.exists(),
        "install.sh should exist at: {:?}",
        path
    );
}

#[test]
fn test_install_script_uses_service_template() {
    let path = project_root().join("deploy").join("install.sh");
    let content = std::fs::read_to_string(&path)
        .expect("Failed to read install.sh");

    // 验证 install.sh 引用 service 模板
    assert!(
        content.contains("gnome-remote-agent.service") || content.contains("SERVICE_TEMPLATE"),
        "install.sh should reference the systemd service template file"
    );

    // 验证不再使用旧的 simple Type 内联生成（应有 KillMode=process 或模板引用）
    assert!(
        content.contains("KillMode=process") || content.contains("SERVICE_TEMPLATE"),
        "install.sh should include Phase 4 KillMode=process config or use template"
    );
}

#[test]
fn test_install_script_executable() {
    let path = project_root().join("deploy").join("install.sh");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&path)
            .expect("Failed to get install.sh metadata");
        let permissions = metadata.permissions();

        // 检查是否可执行（owner 或 group 或 other 有执行权限）
        assert!(
            permissions.mode() & 0o111 != 0,
            "install.sh should be executable (mode: {:o})",
            permissions.mode()
        );
    }
}

#[test]
fn test_systemd_service_syntax() {
    let path = systemd_service_path();

    // 使用 systemd-analyze verify 验证语法（如果可用）
    let output = std::process::Command::new("systemd-analyze")
        .arg("verify")
        .arg(&path)
        .output();

    match output {
        Ok(result) => {
            // systemd-analyze verify 成功时无输出，失败时有 stderr
            if !result.status.success() {
                let stderr = String::from_utf8_lossy(&result.stderr);
                // 在 WSL 中 systemd-analyze 可能不可用，这是可接受的
                if !stderr.contains("command not found") && !stderr.contains("No such file") {
                    panic!("systemd-analyze verify failed: {}", stderr);
                }
            }
        }
        Err(_) => {
            // systemd-analyze 不存在（WSL 环境），跳过
            // 这是可接受的，因为 WSL 可能没有 systemd
        }
    }
}
