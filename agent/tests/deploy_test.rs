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
        .join("quireld.service")
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

    // 验证 Type=simple（quireld 未实现 sd_notify）
    assert!(
        content.contains("Type=simple"),
        "Service should use Type=simple (quireld does not implement sd_notify)"
    );

    // Phase 4 热更新配置（KillMode/ExecReload）当前被注释
    // 需要等 main.rs 集成 Manager 架构后才能启用
    // 验证配置文件中存在相关注释说明（非启用状态）
    let has_killmode_comment = content
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .any(|line| line.contains("KillMode=process"));
    assert!(
        has_killmode_comment,
        "Service should contain commented KillMode=process (Phase 4 not yet integrated)"
    );
}

#[test]
fn test_systemd_service_contains_security_config() {
    let path = systemd_service_path();
    let content = std::fs::read_to_string(&path)
        .expect("Failed to read systemd service file");

    // 验证资源限制配置
    assert!(
        content.contains("LimitNOFILE"),
        "Service should contain LimitNOFILE"
    );

    // 验证未使用 CapabilityBoundingSet 限制能力
    // quireld 需要读取所有用户的 .ssh/authorized_keys 文件（公钥认证）
    // 和切换用户身份（PAM 认证），需要完整能力
    let has_capability_restriction = content
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .any(|line| line.contains("CapabilityBoundingSet"));
    assert!(
        !has_capability_restriction,
        "Service should NOT restrict capabilities (breaks authorized_keys reading)"
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
        content.contains("quireld.service") || content.contains("SERVICE_TEMPLATE"),
        "install.sh should reference the systemd service template file"
    );

    // 验证 install.sh 包含 Type=simple（当前部署版本）
    assert!(
        content.contains("Type=simple"),
        "install.sh should use Type=simple (current quireld version)"
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

/// 安装脚本必须落地独立 PAM 服务、日志轮转与防火墙提示（安全基线）
#[test]
fn test_install_script_sets_up_pam_logrotate_and_firewall_hint() {
    let path = project_root().join("deploy").join("install.sh");
    let content = std::fs::read_to_string(&path).expect("Failed to read install.sh");

    assert!(content.contains("/etc/pam.d/$SERVICE_NAME"), "缺少 PAM 服务写入");
    assert!(content.contains("@include common-auth"), "PAM 服务缺少 common-auth");
    assert!(content.contains("@include common-account"), "PAM 服务缺少 common-account");
    assert!(content.contains("/etc/logrotate.d/$SERVICE_NAME"), "缺少日志轮转配置");
    assert!(content.contains("copytruncate"), "日志轮转需 copytruncate（无需 reopen）");
    assert!(content.contains("print_firewall_hint"), "缺少防火墙提示");
}

/// 升级失败必须自动回滚最近备份并复检（不得只打印提示后退出）
#[test]
fn test_install_script_upgrade_auto_rollback() {
    let path = project_root().join("deploy").join("install.sh");
    let content = std::fs::read_to_string(&path).expect("Failed to read install.sh");

    assert!(content.contains("尝试自动回滚到最近备份"), "升级失败缺少自动回滚动作");
    assert!(content.contains("LATEST_BACKUP"), "缺少最近备份选取");
    assert!(content.contains("已回滚到备份"), "缺少回滚成功提示");
    assert!(content.contains("回滚后服务仍无法启动"), "缺少回滚失败的人工处理提示");
    // 备份保留策略的 glob 必须带通配符（否则 ls 匹配不到任何备份文件）
    assert!(
        content.contains("\"$BACKUP_DIR/${SERVICE_NAME}_\"*"),
        "备份清理/选取的 glob 缺少通配符"
    );
}

/// 一键引导脚本必须按「主域 → 备用域 → GitHub」取包，且校验失败立即中断
#[test]
fn test_bootstrap_script_multi_domain_sources() {
    let path = project_root().join("deploy").join("install-quireld.sh");
    let content = std::fs::read_to_string(&path).expect("Failed to read install-quireld.sh");

    assert!(content.contains("https://quireld.cn/downloads/quireld"), "缺少官网主域");
    assert!(content.contains("https://quireld.icu/downloads/quireld"), "缺少官网备用域");
    assert!(content.contains("SITE_BASE_URLS"), "缺少多源列表变量");
    assert!(content.contains("FETCH_RC -eq 2"), "SHA-256 校验失败必须中断");
    assert!(content.contains("GitHub Releases"), "缺少 GitHub 兜底");
}

/// 全新安装失败必须给出排查与卸载清理指引
#[test]
fn test_install_script_fresh_failure_cleanup_hint() {
    let path = project_root().join("deploy").join("install.sh");
    let content = std::fs::read_to_string(&path).expect("Failed to read install.sh");

    assert!(content.contains("排查与清理指引"), "全新安装失败缺少排查指引");
    assert!(content.contains("systemctl disable --now"), "卸载指引缺少停用服务步骤");
    assert!(content.contains("systemctl daemon-reload"), "卸载指引缺少 daemon-reload");
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
