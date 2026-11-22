//! quireld 自动供应编排：SSH 探测 → 下载 → 校验 → 推送 → 安装 → 指纹回读。
//!
//! 流程（两个命令，向导状态机）：
//! - [`provision_detect`]：SSH 探测环境与安装状态（三态判定），返回给前端确认
//! - [`provision_server`]：按判定结果执行安装编排（全新/升级式/复用已装），
//!   完成后回读服务器证书指纹供 QUIC 预钉扎（首连无 TOFU 弹窗）
//!
//! 安全约束：
//! - 远端探测/安装命令均为客户端固定模板，不透传用户输入片段
//! - 二进制下载经 SHA256SUMS 严格校验，上传到服务器后 install.sh 再次比对哈希
//! - sudo 密码仅在内存 Zeroizing 中转；日志不包含凭据/证书指纹
//! - host key TOFU：detect 观测指纹 → 前端确认 → install 阶段强制比对

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{Emitter, Manager};
use zeroize::Zeroizing;

use crate::connection::compute_cert_fingerprint;
use crate::credentials;
use crate::errors::{codes, AppError};
use crate::ssh::{ExecResult, SshAuthData, SshSession, SshTarget, SudoMode, UploadSpec};

/// GitHub 发布仓库（与 scripts/publish-quireld.mjs 一致；客户端作为官网源的兜底）
pub const QUIREL_REPO: &str = "Victor-C0905/Remote_Desktop";
/// 官网静态下载源基础地址（主源；官网为 quireld.cn）。
///
/// 产物布局（由 scripts/publish-quireld.mjs 生成）：
/// - `{base}/{version}/quireld-{version}-release-linux-{arch}.tar.gz` + `SHA256SUMS`
/// - `{base}/install-quireld.sh`、`{base}/latest.json`、稳定别名与根 `SHA256SUMS`
///
/// 取包顺序：主源 → [`FALLBACK_RELEASE_BASES`] → GitHub Releases。
pub const DEFAULT_RELEASE_BASE: &str = "https://quireld.cn/downloads/quireld";
/// 官网备用静态源（主源不可达/404 时按序尝试；同一官网应用，仅域名不同）
pub const FALLBACK_RELEASE_BASES: &[&str] = &["https://quireld.icu/downloads/quireld"];
/// 客户端期望的 quireld 版本（lockstep：三 crate 版本统一）
pub const TARGET_QUIRELD_VERSION: &str = env!("CARGO_PKG_VERSION");
/// 远端暂存目录（$HOME 下，SFTP 推送无需 sudo）
const STAGE_DIR_NAME: &str = ".quirel-provision";
/// SFTP 单文件下载上限（证书/配置等小文件场景）
const SFTP_READ_LIMIT: u64 = 64 * 1024;
/// tarball 解压总大小上限
const EXTRACT_TOTAL_LIMIT: u64 = 200 * 1024 * 1024;

// ── 数据结构 ──────────────────────────────────────────────

/// 安装状态三态（探测判定结果）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum InstallState {
    /// 未安装（二进制与服务单元均不存在）→ 全新安装
    NotInstalled,
    /// 已安装（含版本/端口/证书路径，供升级式安装或复用）
    Installed,
    /// 损坏（服务注册但二进制损坏/版本探测失败）→ 重装
    Broken,
}

/// 已安装信息（从远端配置回读）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExistingInstall {
    pub version: String,
    pub quic_port: u16,
    pub cert_path: String,
}

/// sudo 模式归类（用户可见字符串）
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SudoKind {
    /// root 直登
    None,
    /// sudo 免密
    Passwordless,
    /// sudo 需密码（密码认证场景复用登录密码）
    Password,
    /// 无法自动获取 sudo（仅密钥认证 + sudo 需密码）→ 降级手动命令
    Unsupported,
}

/// 无法自动安装的具体原因（前端据此给出可操作提示）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SudoBlockReason {
    /// 服务器没有 sudo（也没有可用的 doas）
    NoSudo,
    /// 只有 doas，暂不支持自动安装
    DoasOnly,
    /// sudo 存在，但当前密钥登录无法提供密码
    SudoRequiresPassword,
}

/// 探测结果（前端展示并确认后再执行安装）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectResult {
    pub install_state: InstallState,
    pub existing: Option<ExistingInstall>,
    pub sudo_mode: SudoKind,
    /// sudo 不可用的具体原因（sudo_mode=unsupported 时非空）
    pub sudo_block_reason: Option<SudoBlockReason>,
    /// 服务器架构映射（amd64/arm64；未知架构报错）
    pub arch: String,
    pub is_root: bool,
    pub systemd_ok: bool,
    pub username: String,
    pub home_dir: String,
    /// SSH host key 指纹（OpenSSH SHA256:base64），前端确认后随安装请求回传比对
    pub host_key_fingerprint: String,
    /// 客户端期望的 quireld 版本（lockstep，与安装判定共用同一常量）
    pub target_version: String,
    /// 已装版本是否需要升级（Rust 侧比较，前端直接展示）
    pub needs_upgrade: bool,
}

/// 安装进度事件 payload（app.emit("provision-progress")）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionProgress {
    pub server_id: String,
    pub step: String,
    pub detail: String,
    pub percent: u32,
}

/// 安装结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionOutcome {
    /// installed=安装完成 / up_to_date=已装版本即最新（跳过安装）/ degraded=需手动安装
    pub status: String,
    /// 服务器证书指纹（SHA-256 hex 小写，与 QUIC 钉扎格式一致）
    pub cert_fingerprint: Option<String>,
    /// SSH host key 指纹（确认值）
    pub host_key_fingerprint: String,
    pub quireld_port: Option<u16>,
    /// degraded 时的手动安装命令（curl|bash）
    pub install_command: Option<String>,
    pub reason: Option<String>,
}

/// tauri command 参数公共部分（凭据从 keyring 读取，不经 IPC 明文传递）
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionParams {
    pub server_id: String,
    /// QUIC/SSH 同一台服务器：host 共用
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    pub username: String,
    /// "password" | "pubkey"
    pub auth_method: String,
}

fn default_ssh_port() -> u16 {
    22
}

/// 内部会话材料（凭据解码 + sudo 模式）
struct SessionCtx {
    auth: SshAuthData,
    sudo_kind: SudoKind,
    login_password: Option<Zeroizing<String>>,
}

// ── Tauri 命令 ────────────────────────────────────────────

/// 探测：SSH 连接 → 固定模板探测 → 三态判定（不修改服务器任何状态）
#[tauri::command]
#[tracing::instrument(skip(app, params), fields(server_id = %params.server_id, host = %params.host))]
pub async fn provision_detect(app: tauri::AppHandle, params: ProvisionParams) -> Result<DetectResult, AppError> {
    let ctx = build_session_ctx(&app, &params)?;
    // 首次探测无期望指纹（TOFU）：记录观测值交用户确认
    let mut session = connect_session(&params, &ctx, None).await?;
    let host_key_fingerprint = session.host_key_fingerprint().to_string();

    let probe = exec_checked(&mut session, PROBE_CMD, &ctx).await?;
    let parsed = parse_probe_output(&probe.stdout)
        .map_err(|e| AppError::new(codes::PROVISION_DETECT_FAILED).with_detail(format!("探测输出解析失败: {e}")))?;

    // 架构白名单映射（与 build.sh/install-quireld.sh 同一映射表）
    let arch = match parsed.arch.as_str() {
        "x86_64" => "amd64".to_string(),
        "aarch64" => "arm64".to_string(),
        other => {
            return Err(AppError::new(codes::PROVISION_UNSUPPORTED_ARCH)
                .with_param("arch", other)
                .with_detail(format!("不支持的架构: {other}")));
        }
    };

    let (sudo_mode, sudo_block_reason) = classify_sudo(
        parsed.is_root,
        parsed.sudo_nopass,
        parsed.has_sudo,
        parsed.has_doas,
        params.auth_method == "password" && ctx.login_password.is_some(),
    );

    // 安装状态三态判定
    let (install_state, existing, needs_upgrade) = if parsed.bin_found && parsed.version != "none" {
        let version = normalize_version(&parsed.version);
        // 读配置（端口/证书路径）；配置缺失时用探测默认值兜底
        let cfg = parse_quireld_config(&parsed.config_body);
        let quic_port = cfg.quic_port.unwrap_or(9443);
        let cert_path = cfg.cert_path.unwrap_or_else(|| "/var/lib/quireld/cert.pem".to_string());
        let needs = compare_versions(&version, TARGET_QUIRELD_VERSION) == std::cmp::Ordering::Less;
        (
            InstallState::Installed,
            Some(ExistingInstall { version, quic_port, cert_path }),
            needs,
        )
    } else if parsed.unit_found {
        // 服务注册但二进制损坏/版本探测失败 → Broken
        (InstallState::Broken, None, false)
    } else {
        (InstallState::NotInstalled, None, false)
    };

    Ok(DetectResult {
        install_state,
        existing,
        sudo_mode,
        sudo_block_reason,
        arch,
        is_root: parsed.is_root,
        systemd_ok: parsed.systemd_ok,
        username: params.username.clone(),
        home_dir: parsed.home_dir,
        host_key_fingerprint,
        target_version: TARGET_QUIRELD_VERSION.to_string(),
        needs_upgrade,
    })
}

/// 安装编排：按 detect 结果走对应分支，完成后回读证书指纹。
/// options 为 detect 结果经用户确认后的回传选项（含 host key 指纹确认值）。
#[tauri::command]
#[tracing::instrument(skip(app, params), fields(server_id = %params.server_id, host = %params.host))]
pub async fn provision_server(
    app: tauri::AppHandle,
    params: ProvisionParams,
    options: ProvisionOptions,
) -> Result<ProvisionOutcome, AppError> {
    let ctx = build_session_ctx(&app, &params)?;
    // host key 指纹在握手期（认证前）强制比对：不匹配即拒绝连接，
    // 密码/私钥不会发送给对端（B-01，防 MITM 先截获凭据）
    let mut session = connect_session(&params, &ctx, Some(&options.host_key_fingerprint)).await?;

    // 纵深防御：握手期已强制比对，此处保留观测值与 detect 确认值的二次校验
    let observed = session.host_key_fingerprint().to_string();
    if observed != options.host_key_fingerprint {
        return Err(AppError::new(codes::SSH_HOST_KEY_CHANGED)
            .with_detail("SSH host key 与探测时不一致，疑似中间人攻击，已中止安装".to_string()));
    }

    // Unsupported sudo 无法自动安装 → 返回降级提示（无 sudo/doas 时不再给注定失败的 root 命令）
    if ctx.sudo_kind == SudoKind::Unsupported {
        let (reason, command) = match options.sudo_block_reason {
            Some(SudoBlockReason::NoSudo) => (
                "当前账号没有管理员权限（服务器未安装 sudo）。请让服务器管理员安装 quireld，或改用具备 sudo 的账号。"
                    .to_string(),
                None,
            ),
            Some(SudoBlockReason::DoasOnly) => (
                "服务器使用 doas 而非 sudo，暂不支持自动安装。请让管理员手动安装，或改用具备 sudo 的账号。"
                    .to_string(),
                None,
            ),
            Some(SudoBlockReason::SudoRequiresPassword) => (
                "当前为密钥登录，sudo 需要密码，无法自动安装。请改用密码登录，或为该账号配置免密 sudo。"
                    .to_string(),
                Some(bootstrap_install_command()),
            ),
            None => (
                "当前账号无法自动取得管理员权限。请改用密码登录、配置免密 sudo，或让管理员执行下方命令。"
                    .to_string(),
                Some(bootstrap_install_command()),
            ),
        };
        return Ok(degraded_outcome(&observed, &reason, command));
    }

    // 确定执行路径
    let use_upgrade = match &options.install_state {
        Some(InstallState::Installed) => {
            let existing = options.existing.as_ref().ok_or_else(|| {
                AppError::new(codes::PROVISION_DETECT_FAILED).with_detail("缺少已安装信息")
            })?;
            compare_versions(&existing.version, TARGET_QUIRELD_VERSION) == std::cmp::Ordering::Less
        }
        Some(_) | None => false,
    };
    let skip_install = matches!(&options.install_state, Some(InstallState::Installed))
        && !use_upgrade;

    let home_dir = options.home_dir.clone().unwrap_or_else(|| format!("/home/{}", params.username));
    let stage_dir = format!("{home_dir}/{STAGE_DIR_NAME}");
    let emit = |step: &str, detail: String, percent: u32| {
        let _ = app.emit(
            "provision-progress",
            ProvisionProgress { server_id: params.server_id.clone(), step: step.to_string(), detail, percent },
        );
    };

    // 路径 A：已装版本即最新 → 跳过下载安装，直接复用
    let mut local_dir: Option<PathBuf> = None;
    if skip_install {
        emit("detect", "已安装版本即为最新，跳过安装".to_string(), 10);
    } else {
        // 1. 下载（官网源优先 → GitHub 兜底 → 临时目录 → SHA256 校验 → 解压）
        emit("download", "获取 quireld 安装包".to_string(), 15);
        let arch = options.arch.as_deref().unwrap_or("amd64");
        let cache_dir = app.path().app_cache_dir().ok();
        let pkg = download_release(arch, cache_dir.as_deref()).await?;
        emit("verify", "SHA-256 校验通过，解压完成".to_string(), 40);
        local_dir = Some(pkg.extract_dir.clone());

        // 2. 计算二进制哈希（客户端侧首重校验）
        let binary_path = pkg.extract_dir.join(&pkg.dir_name).join("quireld");
        let binary_hash = sha256_hex_file(&binary_path)?;

        // 3. 上传（SFTP 推送到 $HOME/.quirel-provision）
        emit("upload", "通过 SFTP 推送安装文件".to_string(), 55);
        let install_sh = pkg.extract_dir.join(&pkg.dir_name).join("install.sh");
        let systemd_tpl = pkg.extract_dir.join(&pkg.dir_name).join("systemd/quireld.service");
        upload_stage_files(&mut session, &stage_dir, &UploadSet {
            binary: &binary_path,
            install_sh: &install_sh,
            systemd_tpl: if systemd_tpl.exists() { Some(&systemd_tpl) } else { None },
            config: if use_upgrade { None } else { Some(pkg.extract_dir.join(&pkg.dir_name).join("quireld.toml")) },
        })
        .await?;

        // 4. 安装（远端执行 install.sh；二进制哈希由脚本再校验一次）
        emit("install", "执行远端安装（sudo）".to_string(), 70);
        let cmd = build_install_command(&stage_dir, &binary_hash, use_upgrade, options.quireld_port);
        let sudo = sudo_for(&ctx);
        let result = session
            .exec(&cmd, &sudo, std::time::Duration::from_secs(180))
            .await
            .map_err(|e| AppError::new(codes::SSH_EXEC_FAILED).with_detail(format!("安装命令执行失败: {e}")))?;
        if !result.ok() {
            // 取 stderr 末 8 行作为失败上下文（install.sh 自身日志已足够诊断）
            let lines: Vec<&str> = result.stderr.lines().collect();
            let start = lines.len().saturating_sub(8);
            let tail = lines[start..].join("\n");
            return Err(AppError::new(codes::PROVISION_INSTALL_FAILED)
                .with_detail(format!("install.sh 退出码 {:?}\n{tail}", result.exit_code)));
        }
        let install = parse_install_ok(&result.stdout)
            .map_err(|e| AppError::new(codes::PROVISION_INSTALL_FAILED).with_detail(e))?;

        // 5. 清理远端暂存目录（$HOME 下无需 sudo；best-effort）
        let _ = session.exec(&format!("rm -rf {}", shell_quote_path(&stage_dir)), &SudoMode::None, std::time::Duration::from_secs(30)).await;

        // 安装输出的端口与配置路径（升级模式由脚本回读已装配置）
        return finish_with_cert(&mut session, &ctx, &install.config_path, install.port, &observed, &emit).await;
    }

    // 路径 B：复用已装（跳过安装）→ 直接回读配置与证书
    let existing = options.existing.as_ref().ok_or_else(|| {
        AppError::new(codes::PROVISION_DETECT_FAILED).with_detail("缺少已安装信息")
    })?;
    let _ = local_dir;
    finish_with_cert(&mut session, &ctx, &existing.cert_path, existing.quic_port, &observed, &emit).await
}

/// 安装确认选项（前端展示判定结果后回传）
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionOptions {
    /// detect 返回的三态（复用已装/升级式安装/全新安装由此分流）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_state: Option<InstallState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing: Option<ExistingInstall>,
    /// 用户选择的 QUIC 端口（全新安装时使用；升级/复用忽略）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quireld_port: Option<u16>,
    /// detect 观测并经用户确认的 SSH host key 指纹（安装时强制比对）
    pub host_key_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    /// detect 判定的 sudo 不可用原因（仅用于降级提示，不参与权限判断）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sudo_block_reason: Option<SudoBlockReason>,
}

// ── 探测命令与解析 ────────────────────────────────────────

/// 环境探测固定模板（单次 exec；客户端固定，远端不接任意参数）
const PROBE_CMD: &str = r#"echo "ARCH=$(uname -m)"; echo "UID=$(id -u)"; echo "UNAME=$(id -un)"; echo "HOMEDIR=$HOME"; echo "SYSTEMD=$(systemctl is-system-running 2>&1 || true)"; if sudo -n true 2>/dev/null; then echo "SUDO_NOPASS=1"; else echo "SUDO_NOPASS=0"; fi; echo "SUDO_BIN=$(command -v sudo >/dev/null 2>&1 && echo 1 || echo 0)"; echo "DOAS_BIN=$(command -v doas >/dev/null 2>&1 && echo 1 || echo 0)"; echo "BIN=$(command -v quireld 2>/dev/null || echo none)"; echo "UNIT=$(systemctl list-unit-files quireld.service --no-legend 2>/dev/null || true)"; echo "VER=$(/usr/local/bin/quireld --version 2>/dev/null || echo none)"; echo "CFG_BEGIN"; cat /etc/quireld/quireld.toml 2>/dev/null || true; echo "CFG_END""#;

struct ProbeParsed {
    arch: String,
    uid: u32,
    is_root: bool,
    home_dir: String,
    systemd_ok: bool,
    sudo_nopass: bool,
    has_sudo: bool,
    has_doas: bool,
    bin_found: bool,
    unit_found: bool,
    version: String,
    config_body: String,
}

/// sudo 模式归类 + 不可自动安装的具体原因
///
/// 只做判定，不改远端状态。`has_password` 表示本次是密码登录且客户端持有密码，
/// 可用于 `sudo -S`；密钥登录时无法注入 sudo 密码。
fn classify_sudo(
    is_root: bool,
    sudo_nopass: bool,
    has_sudo: bool,
    has_doas: bool,
    has_password: bool,
) -> (SudoKind, Option<SudoBlockReason>) {
    if is_root {
        return (SudoKind::None, None);
    }
    if sudo_nopass {
        return (SudoKind::Passwordless, None);
    }
    if has_sudo && has_password {
        return (SudoKind::Password, None);
    }
    let reason = if !has_sudo && has_doas {
        SudoBlockReason::DoasOnly
    } else if !has_sudo {
        SudoBlockReason::NoSudo
    } else {
        SudoBlockReason::SudoRequiresPassword
    };
    (SudoKind::Unsupported, Some(reason))
}

fn parse_probe_output(stdout: &str) -> Result<ProbeParsed, String> {
    let mut arch = String::new();
    let mut uid: u32 = u32::MAX;
    let mut home_dir = String::new();
    let mut systemd = String::new();
    let mut sudo_nopass = false;
    let mut has_sudo = false;
    let mut has_doas = false;
    let mut bin_found = false;
    let mut unit_found = false;
    let mut version = "none".to_string();
    let mut config_body = String::new();
    let mut in_config = false;

    for line in stdout.lines() {
        if line == "CFG_BEGIN" { in_config = true; continue; }
        if line == "CFG_END" { in_config = false; continue; }
        if in_config { config_body.push_str(line); config_body.push('\n'); continue; }
        if let Some(v) = line.strip_prefix("ARCH=") { arch = v.trim().to_string(); }
        else if let Some(v) = line.strip_prefix("UID=") { uid = v.trim().parse().unwrap_or(u32::MAX); }
        else if let Some(v) = line.strip_prefix("HOMEDIR=") { home_dir = v.trim().to_string(); }
        else if let Some(v) = line.strip_prefix("SYSTEMD=") { systemd = v.trim().to_string(); }
        else if line.trim() == "SUDO_NOPASS=1" { sudo_nopass = true; }
        else if line.trim() == "SUDO_BIN=1" { has_sudo = true; }
        else if line.trim() == "DOAS_BIN=1" { has_doas = true; }
        else if let Some(v) = line.strip_prefix("BIN=") { bin_found = v.trim() != "none" && !v.trim().is_empty(); }
        else if let Some(v) = line.strip_prefix("UNIT=") { unit_found = !v.trim().is_empty(); }
        else if let Some(v) = line.strip_prefix("VER=") { version = v.trim().to_string(); }
    }

    if arch.is_empty() || uid == u32::MAX {
        return Err("关键探测字段缺失（ARCH/UID）".to_string());
    }
    let systemd_ok = matches!(systemd.as_str(), "running" | "degraded");
    Ok(ProbeParsed {
        arch,
        uid,
        is_root: uid == 0,
        home_dir: if home_dir.is_empty() { "/root".to_string() } else { home_dir },
        systemd_ok,
        sudo_nopass,
        has_sudo,
        has_doas,
        bin_found,
        unit_found,
        version,
        config_body,
    })
}

/// quireld.toml 行解析（quic_port / cert_path）
struct QuireldConfig {
    quic_port: Option<u16>,
    cert_path: Option<String>,
}

fn parse_quireld_config(body: &str) -> QuireldConfig {
    let mut out = QuireldConfig { quic_port: None, cert_path: None };
    for line in body.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("quic_port") {
            let n: String = v.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(p) = n.parse() { out.quic_port = Some(p); }
        } else if let Some(v) = line.strip_prefix("cert_path") {
            // v 形如 ` = "./cert.pem"`：先去空白露 '='，再去 '=' 与引号
            let v = v.trim().trim_start_matches('=').trim().trim_matches('"');
            if !v.is_empty() { out.cert_path = Some(v.to_string()); }
        }
    }
    out
}

/// `quireld 0.3.3 (build info)` → `0.3.3`
fn normalize_version(raw: &str) -> String {
    let first = raw.split_whitespace().nth(1).unwrap_or(raw);
    first.split('-').next().unwrap_or(first).to_string()
}

/// 语义化版本解析：核心数字段 + 预发布标识（构建元数据 + 后缀不参与比较）
fn parse_version_parts(s: &str) -> (Vec<u64>, Option<Vec<&str>>) {
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };
    let core = core.split('+').next().unwrap_or(core);
    let core: Vec<u64> = core.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let pre = pre.map(|p| {
        p.split('+').next().unwrap_or(p).split('.').collect::<Vec<&str>>()
    });
    (core, pre)
}

/// 语义化版本比较（semver 2.0.0 子集，无第三方依赖）
///
/// - 核心段按数字比较，缺省补 0
/// - 同核心时：有预发布后缀 < 无预发布后缀
/// - 预发布标识逐个比较：双方皆数字按数值（beta.2 < beta.10），数字 < 非数字，
///   否则字典序；前缀相同且一方耗尽 → 耗尽方更低
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let (ca, pa) = parse_version_parts(a);
    let (cb, pb) = parse_version_parts(b);
    for i in 0..ca.len().max(cb.len()) {
        let x = ca.get(i).copied().unwrap_or(0);
        let y = cb.get(i).copied().unwrap_or(0);
        if x != y { return x.cmp(&y); }
    }
    match (pa, pb) {
        (None, None) => std::cmp::Ordering::Equal,
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(x), Some(y)) => {
            for i in 0..x.len().max(y.len()) {
                match (x.get(i), y.get(i)) {
                    (None, None) => break,
                    (None, Some(_)) => return std::cmp::Ordering::Less,
                    (Some(_), None) => return std::cmp::Ordering::Greater,
                    (Some(xi), Some(yi)) => {
                        let ord = match (xi.parse::<u64>(), yi.parse::<u64>()) {
                            (Ok(nx), Ok(ny)) => nx.cmp(&ny),
                            (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                            (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                            (Err(_), Err(_)) => xi.cmp(yi),
                        };
                        if ord != std::cmp::Ordering::Equal { return ord; }
                    }
                }
            }
            std::cmp::Ordering::Equal
        }
    }
}

/// install.sh 机器可读结果行解析：QUIRELD_INSTALL_OK port=<n> config=<path>
struct InstallOk {
    port: u16,
    config_path: String,
}

fn parse_install_ok(stdout: &str) -> Result<InstallOk, String> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.starts_with("QUIRELD_INSTALL_OK "))
        .ok_or("install.sh 未输出机器可读结果行")?;
    let mut port = 0u16;
    let mut config_path = "/etc/quireld/quireld.toml".to_string();
    for tok in line.split_whitespace().skip(1) {
        if let Some(v) = tok.strip_prefix("port=") {
            port = v.parse().unwrap_or(0);
        } else if let Some(v) = tok.strip_prefix("config=") {
            config_path = v.to_string();
        }
    }
    if port == 0 {
        return Err(format!("结果行端口无效: {line}"));
    }
    Ok(InstallOk { port, config_path })
}

// ── 会话与凭据 ────────────────────────────────────────────

fn build_session_ctx(app: &tauri::AppHandle, params: &ProvisionParams) -> Result<SessionCtx, AppError> {
    let raw = credentials::credential_get(params.server_id.clone(), app.clone())
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("读取凭据失败: {e}")))?
        .ok_or_else(|| AppError::new(codes::AUTH_FAILED).with_detail("凭据不存在，请先保存服务器配置"))?;
    // keyring 载荷 JSON：{ password?, privateKey?, passphrase? }
    let secrets: HashMap<String, String> = serde_json::from_str(&raw)
        .map_err(|e| AppError::new(codes::AUTH_FAILED).with_detail(format!("凭据载荷解析失败: {e}")))?;

    let get_secret = |k: &str| secrets.get(k).filter(|v| !v.is_empty()).map(|v| Zeroizing::new(v.clone()));

    let auth = if params.auth_method == "password" {
        let password = get_secret("password")
            .ok_or_else(|| AppError::new(codes::AUTH_FAILED).with_detail("密码认证但凭据中无密码"))?;
        SshAuthData::Password(password.clone())
    } else {
        let pem = get_secret("privateKey")
            .ok_or_else(|| AppError::new(codes::AUTH_FAILED).with_detail("公钥认证但凭据中无私钥"))?;
        SshAuthData::Key { pem, passphrase: get_secret("passphrase") }
    };

    let login_password = get_secret("password");
    // sudo 模式在 detect 已归类；install 复用同一归类（凭据重新读取）
    let sudo_kind = if params.auth_method == "password" && login_password.is_some() {
        SudoKind::Password
    } else {
        SudoKind::Unsupported
    };
    Ok(SessionCtx { auth, sudo_kind, login_password })
}

async fn connect_session(
    params: &ProvisionParams,
    ctx: &SessionCtx,
    expected_host_key: Option<&str>,
) -> Result<SshSession, AppError> {
    let target = SshTarget { host: params.host.clone(), port: params.ssh_port, username: params.username.clone() };
    SshSession::connect(&target, &ctx.auth, expected_host_key)
        .await
        .map_err(|e| match e {
            crate::ssh::SshError::Auth(msg) => {
                AppError::new(codes::SSH_AUTH_FAILED).with_detail(msg)
            }
            crate::ssh::SshError::HostKeyMismatch => {
                AppError::new(codes::SSH_HOST_KEY_CHANGED).with_detail(
                    "SSH host key 与探测时不一致，疑似中间人攻击，已在认证前中止连接".to_string(),
                )
            }
            other => AppError::new(codes::SSH_CONNECT_FAILED).with_detail(other.to_string()),
        })
}

/// sudo 模式映射（SessionCtx 归类 → ssh::SudoMode）
fn sudo_for(ctx: &SessionCtx) -> SudoMode {
    match &ctx.sudo_kind {
        SudoKind::None => SudoMode::None,
        SudoKind::Passwordless => SudoMode::Passwordless,
        SudoKind::Password => SudoMode::Password(
            ctx.login_password.clone().unwrap_or_else(|| Zeroizing::new(String::new())),
        ),
        SudoKind::Unsupported => SudoMode::None, // 上层已拦截，兜底直执行
    }
}

async fn exec_checked(session: &mut SshSession, cmd: &str, ctx: &SessionCtx) -> Result<ExecResult, AppError> {
    let result = session
        .exec(cmd, &sudo_for(ctx), std::time::Duration::from_secs(60))
        .await
        .map_err(|e| match e {
            crate::ssh::SshError::Timeout => AppError::new(codes::RESPONSE_TIMEOUT).with_detail("探测命令超时"),
            other => AppError::new(codes::SSH_EXEC_FAILED).with_detail(other.to_string()),
        })?;
    if !result.ok() {
        return Err(AppError::new(codes::SSH_EXEC_FAILED)
            .with_detail(format!("探测命令退出码 {:?}: {}", result.exit_code, result.stderr)));
    }
    Ok(result)
}

fn degraded_outcome(host_key_fingerprint: &str, reason: &str, install_command: Option<String>) -> ProvisionOutcome {
    ProvisionOutcome {
        status: "degraded".to_string(),
        cert_fingerprint: None,
        host_key_fingerprint: host_key_fingerprint.to_string(),
        quireld_port: None,
        install_command,
        reason: Some(reason.to_string()),
    }
}

/// 降级手动安装命令：官网引导脚本优先；官网源被禁用时回落 GitHub
fn bootstrap_install_command() -> String {
    bootstrap_command_for(release_base_url().as_deref())
}

fn bootstrap_command_for(base: Option<&str>) -> String {
    match base {
        Some(base) => format!("curl -fsSL {}/install-quireld.sh | sudo bash", base.trim_end_matches('/')),
        None => format!(
            "curl -fsSL https://github.com/{QUIREL_REPO}/releases/latest/download/install-quireld.sh | sudo bash"
        ),
    }
}

// ── 下载与解压（阶段三 agent_update 复用）────────────────

/// GitHub release 资产下载结果
pub struct ReleasePackage {
    pub version: String,
    pub tarball: PathBuf,
    /// 解压根目录
    pub extract_dir: PathBuf,
    /// 包内顶层目录名（quireld-{ver}-release-linux-{arch}）
    pub dir_name: String,
}

/// release tag：版本钉扎（客户端期望的 quireld 版本 = 自身版本，lockstep）
fn release_tag_for_target() -> String {
    format!("v{TARGET_QUIRELD_VERSION}")
}

/// 从 release 资产列表中选取目标 tarball 与 SHA256SUMS 的下载地址
///
/// 资产命名契约（publish-quireld.mjs / build.sh）：
/// - tarball：`quireld-{ver}-release-linux-{arch}.tar.gz`（版本名，客户端按此精确匹配）
/// - 校验清单：SHA256SUMS 同时含版本名与稳定别名条目
pub fn pick_release_assets<'a>(
    assets: &'a [(String, String)],
    tarball_name: &str,
) -> Result<(&'a str, &'a str), String> {
    let tarball_url = assets
        .iter()
        .find(|(n, _)| n == tarball_name)
        .map(|(_, u)| u.as_str())
        .ok_or_else(|| format!("release 缺少资产 {tarball_name}"))?;
    let sums_url = assets
        .iter()
        .find(|(n, _)| n == "SHA256SUMS")
        .map(|(_, u)| u.as_str())
        .ok_or("release 缺少 SHA256SUMS")?;
    Ok((tarball_url, sums_url))
}

/// 获取指定 tag 的 release 资产（含 tag 对应版本）
async fn fetch_release_assets(client: &reqwest::Client, tag: &str) -> Result<(String, Vec<(String, String)>), AppError> {
    let url = format!("https://api.github.com/repos/{QUIREL_REPO}/releases/tags/{tag}");
    let resp = client
        .get(&url)
        .header("User-Agent", "quirel-client")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::new(codes::PROVISION_GITHUB_UNAVAILABLE).with_detail(format!("GitHub API 请求失败: {e}")))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(AppError::new(codes::PROVISION_GITHUB_UNAVAILABLE)
            .with_detail(format!("release {tag} 不存在（对应版本尚未发布，请手动安装或更新客户端）")));
    }
    if !resp.status().is_success() {
        return Err(AppError::new(codes::PROVISION_GITHUB_UNAVAILABLE)
            .with_detail(format!("GitHub API 返回 {}", resp.status())));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::new(codes::PROVISION_GITHUB_UNAVAILABLE).with_detail(format!("GitHub API 响应解析失败: {e}")))?;
    let version = tag.trim_start_matches('v').to_string();
    let assets = body["assets"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?.to_string();
            let url = a["browser_download_url"].as_str()?.to_string();
            Some((name, url))
        })
        .collect();
    Ok((version, assets))
}

/// 当前生效的官网静态源列表（按优先级）。
/// 环境变量 QUIREL_RELEASE_BASE 可覆盖：支持逗号或空白分隔的多个源；
/// 显式置空则禁用全部官网源（只走 GitHub 兜底）。
pub fn release_base_urls() -> Vec<String> {
    match std::env::var("QUIREL_RELEASE_BASE") {
        Ok(raw) => parse_release_bases(Some(&raw)),
        Err(_) => parse_release_bases(None),
    }
}

/// 源列表解析（纯函数，便于单测）：
/// - None（未设置）→ 默认主源 + 备用源
/// - 空串/纯空白 → 空列表（禁用官网源）
/// - 其他 → 按逗号/空白拆分，逐个去尾部斜杠
fn parse_release_bases(raw: Option<&str>) -> Vec<String> {
    match raw {
        None => std::iter::once(DEFAULT_RELEASE_BASE)
            .chain(FALLBACK_RELEASE_BASES.iter().copied())
            .map(|s| s.trim_end_matches('/').to_string())
            .collect(),
        Some(s) if s.trim().is_empty() => Vec::new(),
        Some(s) => s
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(|p| p.trim().trim_end_matches('/'))
            .filter(|p| !p.is_empty())
            .map(|p| p.to_string())
            .collect(),
    }
}

/// 主源地址（手动安装命令展示等单值场景）
pub fn release_base_url() -> Option<String> {
    release_base_urls().into_iter().next()
}

/// 官网静态源的资产地址（确定性命名，不依赖任何 API）
pub fn static_asset_urls(base: &str, version: &str, arch: &str) -> (String, String) {
    let dir = format!("{}/{}", base.trim_end_matches('/'), version);
    (
        format!("{dir}/quireld-{version}-release-linux-{arch}.tar.gz"),
        format!("{dir}/SHA256SUMS"),
    )
}

/// 源站不可用（可回落另一来源）与完整性/本地错误（必须中断）的区分。
///
/// 只把「不可达 / 404 / 资产缺失」视为可回落；SHA-256 不匹配、本地 IO 失败
/// 一律中断，避免回落动作掩盖源站投毒或磁盘问题。
fn is_source_unavailable(err: &AppError) -> bool {
    err.code == codes::PROVISION_GITHUB_UNAVAILABLE
}

/// 下载/版本查询共用的 HTTP 客户端（带超时，替代裸 Client::new()）：
/// - connect_timeout 10s：连接阶段卡死快速失败
/// - read_timeout 30s：reqwest 语义为「每次读取操作的空闲超时，成功读取后重置」，
///   流式下载大文件不会被总时长误杀，传输停滞超 30s 才判失败
/// 不设整体 timeout：tarball 体积大，慢速网络需要更长的总传输时间。
pub fn build_http_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .read_timeout(std::time::Duration::from_secs(30))
        .build()
}

/// release 本地缓存布局：{cache_dir}/releases/{version}/{tarball_name}，
/// 旁挂 {tarball_name}.sha256 记录下载时经 SHA256SUMS 校验的哈希
fn release_cache_paths(cache_dir: &Path, version: &str, tarball_name: &str) -> (PathBuf, PathBuf) {
    let tarball = cache_dir.join("releases").join(version).join(tarball_name);
    let sidecar = tarball.with_file_name(format!("{tarball_name}.sha256"));
    (tarball, sidecar)
}

/// 缓存命中复验：tarball 实际哈希必须与旁挂记录一致（防半截写入/磁盘损坏）。
/// 记录缺失/非法/文件缺失一律视为未命中。
/// 注：缓存与客户端同信任域（本机用户目录），复验目标是损坏而非本地攻击者。
/// 信任边界说明（2026-11-20 审计 B-04）：install.sh --binary-hash 与服务端
/// staging 校验比对的均是随 tarball 一并送达的哈希，属自洽校验，对"本地
/// 攻击者同时篡改 tarball+sidecar"并无独立信任锚；该威胁与 keyring 主密钥
/// 同域（同用户写权限），设计上声明接受。如需收紧，应在缓存命中时从源拉取
/// SHA256SUMS 复验（独立锚）。
fn cache_entry_valid(tarball: &Path, sidecar: &Path) -> bool {
    let Ok(expected) = std::fs::read_to_string(sidecar) else { return false };
    let expected = expected.trim().to_lowercase();
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    match sha256_hex_file(tarball) {
        Ok(actual) => actual == expected,
        Err(_) => false,
    }
}

/// 下载校验成功后写入本地缓存（best-effort：失败不影响安装主流程），
/// 并清理其他版本目录（lockstep 钉扎，旧版本缓存不会再被请求）
fn store_cache_entry(cache_dir: Option<&Path>, version: &str, tarball_name: &str, tarball: &Path, verified_sha: &str) {
    let Some(cache) = cache_dir else { return };
    let (dest, sidecar) = release_cache_paths(cache, version, tarball_name);
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(tarball, &dest)?;
        std::fs::write(&sidecar, format!("{verified_sha}\n"))?;
        Ok(())
    })();
    if let Err(e) = result {
        tracing::debug!("[Provision] release 缓存写入失败（忽略）: {e}");
        let _ = std::fs::remove_file(&dest);
        let _ = std::fs::remove_file(&sidecar);
        return;
    }
    if let Some(releases_dir) = dest.parent().and_then(|p| p.parent()) {
        if let Ok(entries) = std::fs::read_dir(releases_dir) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy() != version {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
    }
}

/// 下载 release：本地缓存命中（复验哈希）→ 官网静态源优先，GitHub 兜底
/// （两者都按客户端版本钉扎）→ 校验 SHA-256 → 安全解压 → 写缓存。
/// cache_dir 传 None 时禁用缓存（纯在线下载）。
pub async fn download_release(arch: &str, cache_dir: Option<&Path>) -> Result<ReleasePackage, AppError> {
    let client = build_http_client()
        .map_err(|e| AppError::new(codes::PROVISION_GITHUB_UNAVAILABLE).with_detail(format!("HTTP 客户端初始化失败: {e}")))?;
    let version = TARGET_QUIRELD_VERSION.to_string();
    let tarball_name = format!("quireld-{version}-release-linux-{arch}.tar.gz");
    let dir_name = tarball_name.trim_end_matches(".tar.gz").to_string();

    // 0. 本地缓存命中 → 跳过下载，直接解压到临时目录
    if let Some(cache) = cache_dir {
        let (cached_tarball, sidecar) = release_cache_paths(cache, &version, &tarball_name);
        if cache_entry_valid(&cached_tarball, &sidecar) {
            let work = std::env::temp_dir().join(format!("quirel-provision-{}", uuid::Uuid::new_v4()));
            let extract_dir = work.join("extract");
            match extract_tarball_checked(&cached_tarball, &extract_dir) {
                Ok(()) => {
                    tracing::info!("[Provision] 命中本地 release 缓存，跳过下载: version={version} arch={arch}");
                    return Ok(ReleasePackage { version, tarball: cached_tarball, extract_dir, dir_name });
                }
                Err(e) => {
                    // 缓存解压失败（损坏等）→ 清掉缓存条目，走正常在线下载
                    cleanup_dir(&work);
                    let _ = std::fs::remove_file(&cached_tarball);
                    let _ = std::fs::remove_file(&sidecar);
                    tracing::warn!("[Provision] 本地缓存解压失败，回退在线下载: {}", e.detail.as_deref().unwrap_or(""));
                }
            }
        }
    }

    // 1. 官网静态源（主源 → 备用源，按序尝试；同一官网应用的不同域名）
    for base in release_base_urls() {
        let (tarball_url, sums_url) = static_asset_urls(&base, &version, arch);
        match fetch_and_extract_retry(&client, &tarball_url, &sums_url, &tarball_name).await {
            Ok((tarball, extract_dir, sha)) => {
                tracing::info!("[Provision] release 下载完成: version={version} arch={arch} source={base}");
                store_cache_entry(cache_dir, &version, &tarball_name, &tarball, &sha);
                return Ok(ReleasePackage { version, tarball, extract_dir, dir_name });
            }
            Err(e) if is_source_unavailable(&e) => {
                // 当前官网源不可用 → 尝试下一个官网源（全部失败再走 GitHub 兜底）
                tracing::warn!("[Provision] 官网源不可用，尝试下一个来源: source={base}");
            }
            Err(e) => return Err(e),
        }
    }

    // 2. GitHub 兜底（tag = 客户端版本）
    let (gh_version, assets) = fetch_release_assets(&client, &release_tag_for_target()).await?;
    let (tarball_url, sums_url) = pick_release_assets(&assets, &tarball_name)
        .map_err(|e| AppError::new(codes::PROVISION_GITHUB_UNAVAILABLE).with_detail(e))?;
    let (tarball, extract_dir, sha) = fetch_and_extract_retry(&client, &tarball_url, &sums_url, &tarball_name).await?;
    store_cache_entry(cache_dir, &gh_version, &tarball_name, &tarball, &sha);

    Ok(ReleasePackage {
        version: gh_version,
        tarball,
        extract_dir,
        dir_name,
    })
}

/// 源不可用类失败重试一次（短暂等待避开瞬时网络抖动）。
/// 仅传输层/源不可达错误可重试；SHA-256 不匹配等完整性错误绝不重试，
/// 避免重试动作掩盖源站投毒。第二次仍失败按原错误码上抛（调用方可回落 GitHub）。
async fn fetch_and_extract_retry(
    client: &reqwest::Client,
    tarball_url: &str,
    sums_url: &str,
    tarball_name: &str,
) -> Result<(PathBuf, PathBuf, String), AppError> {
    match fetch_and_extract(client, tarball_url, sums_url, tarball_name).await {
        Ok(v) => Ok(v),
        Err(e) if is_source_unavailable(&e) => {
            tracing::warn!("[Provision] 下载失败（源不可用），1 秒后重试一次: {}", e.detail.as_deref().unwrap_or(""));
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            fetch_and_extract(client, tarball_url, sums_url, tarball_name).await
        }
        Err(e) => Err(e),
    }
}

/// 从一组确定性 URL 取校验清单 + tarball → 校验 → 解压。
/// 返回 (tarball 路径, 解压目录, 经 SHA256SUMS 校验的哈希)。
///
/// 失败时清理临时目录；错误码约定：
/// - PROVISION_GITHUB_UNAVAILABLE：源不可达/资产缺失（调用方可回落）
/// - PROVISION_SHA256_MISMATCH：完整性校验失败（调用方必须中断）
async fn fetch_and_extract(
    client: &reqwest::Client,
    tarball_url: &str,
    sums_url: &str,
    tarball_name: &str,
) -> Result<(PathBuf, PathBuf, String), AppError> {
    // SHA256SUMS 严格解析：文件名精确匹配该 tarball 行
    let sums_resp = client
        .get(sums_url)
        .header("User-Agent", "quirel-client")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| AppError::new(codes::PROVISION_GITHUB_UNAVAILABLE).with_detail(format!("SHA256SUMS 下载失败: {e}")))?;
    let sums = sums_resp
        .text()
        .await
        .map_err(|e| AppError::new(codes::PROVISION_GITHUB_UNAVAILABLE).with_detail(format!("SHA256SUMS 读取失败: {e}")))?;
    let expected = parse_sha256_sums(&sums, tarball_name)
        .ok_or_else(|| AppError::new(codes::PROVISION_SHA256_MISMATCH).with_detail("SHA256SUMS 中无该 tarball 条目"))?;

    // 临时工作目录（进程退出/完成即清理）
    let work = std::env::temp_dir().join(format!("quirel-provision-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&work)
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("创建临时目录失败: {e}")))?;

    // 流式下载 tarball
    let tarball_path = work.join(tarball_name);
    let download = async {
        let mut resp = client
            .get(tarball_url)
            .header("User-Agent", "quirel-client")
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| AppError::new(codes::PROVISION_GITHUB_UNAVAILABLE).with_detail(format!("tarball 下载失败: {e}")))?;
        use std::io::Write;
        let mut file = std::fs::File::create(&tarball_path)
            .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("创建临时文件失败: {e}")))?;
        // 下载流累计上限（B-08）：read_timeout 仅空闲超时，慢速无限流不触发；
        // 上限取解压预算 2 倍，超限中断（压缩比上限的粗放兜底）
        const DOWNLOAD_TOTAL_LIMIT: u64 = 2 * EXTRACT_TOTAL_LIMIT;
        let mut total: u64 = 0;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| AppError::new(codes::PROVISION_GITHUB_UNAVAILABLE).with_detail(format!("下载中断: {e}")))?
        {
            total += chunk.len() as u64;
            if total > DOWNLOAD_TOTAL_LIMIT {
                return Err(AppError::new(codes::PROVISION_SHA256_MISMATCH)
                    .with_detail(format!("tarball 超过大小上限 {} 字节，已中止下载", DOWNLOAD_TOTAL_LIMIT)));
            }
            file.write_all(&chunk)
                .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("写入临时文件失败: {e}")))?;
        }
        Ok::<(), AppError>(())
    };
    if let Err(e) = download.await {
        cleanup_dir(&work);
        return Err(e);
    }

    // 客户端侧哈希校验（第一重；远端 install.sh --binary-hash 为第二重）
    let actual = match sha256_hex_file(&tarball_path) {
        Ok(v) => v,
        Err(e) => {
            cleanup_dir(&work);
            return Err(e);
        }
    };
    if actual != expected {
        cleanup_dir(&work);
        return Err(AppError::new(codes::PROVISION_SHA256_MISMATCH)
            .with_detail(format!("tarball 哈希不匹配: 期望 {expected} 实际 {actual}")));
    }

    // 解压（条目校验：拒绝绝对路径/.. 与非常规文件；总大小上限）
    let extract_dir = work.join("extract");
    if let Err(e) = extract_tarball_checked(&tarball_path, &extract_dir) {
        cleanup_dir(&work);
        return Err(e);
    }

    Ok((tarball_path, extract_dir, expected))
}

/// SHA256SUMS 行解析（严格 `^([0-9a-f]{64})\s{2}(.+)$`，文件名精确匹配）
pub fn parse_sha256_sums(content: &str, file_name: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim_end();
        if let Some(idx) = line.find("  ") {
            let (hash, name) = (&line[..idx], line[idx + 2..].trim());
            if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) && name == file_name {
                return Some(hash.to_lowercase());
            }
        }
    }
    None
}

pub(crate) fn sha256_hex_file(path: &Path) -> Result<String, AppError> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("打开文件失败: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("读取文件失败: {e}")))?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// tar.gz 安全解压：拒绝绝对路径/`..`/非常规文件，总大小上限，剥离特殊权限位
fn extract_tarball_checked(tarball: &Path, out_dir: &Path) -> Result<(), AppError> {
    let f = std::fs::File::open(tarball)
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("打开 tarball 失败: {e}")))?;
    let gz = flate2::read::GzDecoder::new(f);
    let mut archive = tar::Archive::new(gz);
    std::fs::create_dir_all(out_dir)
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("创建解压目录失败: {e}")))?;

    let mut total: u64 = 0;
    for entry in archive
        .entries()
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("读取 tarball 条目失败: {e}")))?
    {
        let mut entry = entry
            .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("tarball 条目损坏: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("条目路径异常: {e}")))?
            .into_owned();
        // 条目校验：绝对路径 / `..` 组件一律拒绝
        if path.is_absolute() || path.components().any(|c| c == std::path::Component::ParentDir) {
            return Err(AppError::new(codes::PROVISION_SHA256_MISMATCH)
                .with_detail(format!("tarball 条目路径非法: {}", path.display())));
        }
        let is_file = entry.header().entry_type().is_file();
        let is_dir = entry.header().entry_type().is_dir();
        if !is_file && !is_dir {
            return Err(AppError::new(codes::PROVISION_SHA256_MISMATCH)
                .with_detail(format!("tarball 含非常规条目: {}", path.display())));
        }
        if is_file {
            let size = entry.size();
            total += size;
            if total > EXTRACT_TOTAL_LIMIT {
                return Err(AppError::new(codes::PROVISION_SHA256_MISMATCH)
                    .with_detail("tarball 解压总量超上限"));
            }
        }
        // unpack 默认不保留 setuid/setgid（未启用 preserve_permissions）
        entry
            .unpack_in(out_dir)
            .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("解压失败: {e}")))?;
    }
    Ok(())
}

fn cleanup_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

// ── 上传与安装收尾 ────────────────────────────────────────

struct UploadSet<'a> {
    binary: &'a Path,
    install_sh: &'a Path,
    systemd_tpl: Option<&'a Path>,
    /// 升级模式为 None（不动 /etc 配置）
    config: Option<PathBuf>,
}

/// SFTP 推送安装文件到远端暂存目录（家目录，无需 sudo）
async fn upload_stage_files(session: &mut SshSession, stage_dir: &str, set: &UploadSet<'_>) -> Result<(), AppError> {
    // 建目录（单级，逐个创建；父目录为 $HOME 必然存在；路径经 shell 引用包裹）
    let quoted_dir = shell_quote_path(stage_dir);
    let _ = session.exec(&format!("mkdir -p {quoted_dir}"), &SudoMode::None, std::time::Duration::from_secs(30))
        .await
        .map_err(|e| AppError::new(codes::SSH_EXEC_FAILED).with_detail(format!("创建暂存目录失败: {e}")))?;
    if let Some(_tpl) = set.systemd_tpl {
        let _ = session.exec(&format!("mkdir -p {quoted_dir}/systemd"), &SudoMode::None, std::time::Duration::from_secs(30))
            .await
            .map_err(|e| AppError::new(codes::SSH_EXEC_FAILED).with_detail(format!("创建 systemd 目录失败: {e}")))?;
    }

    let uploads: Vec<(&Path, String, u32)> = [
        Some((set.binary, format!("{stage_dir}/quireld"), 0o755)),
        Some((set.install_sh, format!("{stage_dir}/install.sh"), 0o755)),
        set.config.as_ref().map(|c| (c.as_path(), format!("{stage_dir}/quireld.toml"), 0o600)),
        set.systemd_tpl.map(|t| (t, format!("{stage_dir}/systemd/quireld.service"), 0o644)),
    ]
    .into_iter()
    .flatten()
    .collect();

    for (local, remote, mode) in uploads {
        if !local.exists() {
            return Err(AppError::new(codes::IO_FAILED).with_detail(format!("本地文件缺失: {}", local.display())));
        }
        session
            .upload_file(UploadSpec { local, remote_path: &remote, mode })
            .await
            .map_err(|e| AppError::new(codes::SSH_EXEC_FAILED).with_detail(format!("推送 {remote} 失败: {e}")))?;
    }
    Ok(())
}

/// 收尾：回读配置确认 cert_path → SFTP 下载证书 → 指纹计算 → 清理临时目录
async fn finish_with_cert(
    session: &mut SshSession,
    ctx: &SessionCtx,
    config_path: &str,
    port: u16,
    host_key_fingerprint: &str,
    emit: &(dyn Fn(&str, String, u32) + Send + Sync),
) -> Result<ProvisionOutcome, AppError> {
    emit("read_cert", "回读服务器证书".to_string(), 88);

    // 读已装配置，解析 cert_path（相对路径基于 WorkingDirectory /var/lib/quireld）
    let cfg_out = session
        .exec(&format!("cat {}", shell_quote_path(config_path)), &sudo_for(ctx), std::time::Duration::from_secs(30))
        .await
        .map_err(|e| AppError::new(codes::SSH_EXEC_FAILED).with_detail(format!("读取配置失败: {e}")))?;
    let cfg = parse_quireld_config(&cfg_out.stdout);
    let raw_cert_path = cfg.cert_path.unwrap_or_else(|| "./cert.pem".to_string());
    let cert_path = resolve_cert_path(&raw_cert_path);

    // SFTP 下载证书（64KB 上限：自签单证书远小于此值）
    let cert_pem = session
        .download_file(&cert_path, SFTP_READ_LIMIT)
        .await
        .map_err(|e| AppError::new(codes::PROVISION_CERT_READ_FAILED)
            .with_detail(format!("证书下载失败 ({cert_path}): {e}")))?;

    // PEM → DER → SHA-256 hex（与 PinningCertVerifier 格式一致）
    let cert_fingerprint = pem_to_fingerprint(&cert_pem)?;

    emit("connect", "安装完成，准备连接".to_string(), 100);
    Ok(ProvisionOutcome {
        status: "installed".to_string(),
        cert_fingerprint: Some(cert_fingerprint),
        host_key_fingerprint: host_key_fingerprint.to_string(),
        quireld_port: Some(port),
        install_command: None,
        reason: None,
    })
}

/// cert_path 解析：相对路径 → WorkingDirectory 前缀；`~` 不出现在 root 配置中，忽略
fn resolve_cert_path(raw: &str) -> String {
    if raw.starts_with('/') {
        raw.to_string()
    } else {
        let raw = raw.trim_start_matches("./");
        format!("/var/lib/quireld/{raw}")
    }
}

/// PEM（可能含链）取第一张证书 → DER → 指纹
fn pem_to_fingerprint(pem_bytes: &[u8]) -> Result<String, AppError> {
    let text = std::str::from_utf8(pem_bytes)
        .map_err(|e| AppError::new(codes::PROVISION_CERT_READ_FAILED).with_detail(format!("证书非 UTF-8: {e}")))?;
    let block = pem::parse(text)
        .map_err(|e| AppError::new(codes::PROVISION_CERT_READ_FAILED).with_detail(format!("PEM 解析失败: {e}")))?;
    Ok(compute_cert_fingerprint(&block.contents()))
}

/// 路径 shell 单引号包裹（远端路径来自探测/install 输出，非用户输入，防御性包裹）
fn shell_quote_path(p: &str) -> String {
    format!("'{}'", p.replace('\'', r"'\''"))
}

/// 构建远端安装命令（stage_dir 经 shell 单引号包裹，防家目录含空格等特殊字符）
fn build_install_command(stage_dir: &str, binary_hash: &str, use_upgrade: bool, port: Option<u16>) -> String {
    let dir = shell_quote_path(stage_dir);
    let mut cmd = format!("bash {dir}/install.sh --yes --stagedir {dir} --binary-hash {binary_hash}");
    if use_upgrade {
        cmd.push_str(" --upgrade");
    } else {
        cmd.push_str(&format!(" --port {}", port.unwrap_or(9443)));
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 资产选择_版本名tarball与校验清单() {
        let assets = vec![
            ("quireld-0.3.3-release-linux-amd64.tar.gz".to_string(), "https://dl/t1".to_string()),
            ("quireld-linux-amd64.tar.gz".to_string(), "https://dl/t2".to_string()),
            ("SHA256SUMS".to_string(), "https://dl/s".to_string()),
        ];
        let (tarball, sums) =
            pick_release_assets(&assets, "quireld-0.3.3-release-linux-amd64.tar.gz").unwrap();
        assert_eq!(tarball, "https://dl/t1");
        assert_eq!(sums, "https://dl/s");
    }

    #[test]
    fn 资产选择_缺tarball或校验清单报错() {
        let no_sums = vec![("quireld-0.3.3-release-linux-amd64.tar.gz".to_string(), "u".to_string())];
        assert!(pick_release_assets(&no_sums, "quireld-0.3.3-release-linux-amd64.tar.gz").is_err());
        let no_tarball = vec![("SHA256SUMS".to_string(), "u".to_string())];
        assert!(pick_release_assets(&no_tarball, "quireld-0.3.3-release-linux-amd64.tar.gz").is_err());
    }

    #[test]
    fn release_tag_与客户端版本一致() {
        assert_eq!(release_tag_for_target(), format!("v{TARGET_QUIRELD_VERSION}"));
    }

    #[test]
    fn 官网静态源_资产地址确定性拼接() {
        let (tarball, sums) =
            static_asset_urls("https://quireld.cn/downloads/quireld/", "0.3.3", "amd64");
        assert_eq!(
            tarball,
            "https://quireld.cn/downloads/quireld/0.3.3/quireld-0.3.3-release-linux-amd64.tar.gz"
        );
        assert_eq!(sums, "https://quireld.cn/downloads/quireld/0.3.3/SHA256SUMS");
    }

    #[test]
    fn 官网源列表_默认主备顺序() {
        let bases = parse_release_bases(None);
        assert_eq!(
            bases,
            vec![
                "https://quireld.cn/downloads/quireld".to_string(),
                "https://quireld.icu/downloads/quireld".to_string(),
            ]
        );
        // 常量本身不带尾部斜杠，避免拼接出双斜杠
        assert!(!DEFAULT_RELEASE_BASE.ends_with('/'));
        assert!(FALLBACK_RELEASE_BASES.iter().all(|b| !b.ends_with('/')));
    }

    #[test]
    fn 官网源列表_环境覆盖支持多源与置空禁用() {
        // 逗号 / 空白分隔均可，尾部斜杠被剥离
        let bases = parse_release_bases(Some("https://a.example/dl/ , https://b.example/dl"));
        assert_eq!(
            bases,
            vec!["https://a.example/dl".to_string(), "https://b.example/dl".to_string()]
        );
        // 显式置空 → 禁用官网源（只走 GitHub）
        assert!(parse_release_bases(Some("")).is_empty());
        assert!(parse_release_bases(Some("   ")).is_empty());
        // 仅分隔符 → 空列表
        assert!(parse_release_bases(Some(" , ")).is_empty());
    }

    #[test]
    fn 回落判定_仅源不可用允许回落() {
        // 源不可达/404 可以切 GitHub
        assert!(is_source_unavailable(&AppError::new(codes::PROVISION_GITHUB_UNAVAILABLE)));
        // 哈希不匹配与本地 IO 错误必须中断（不得回落掩盖问题）
        assert!(!is_source_unavailable(&AppError::new(codes::PROVISION_SHA256_MISMATCH)));
        assert!(!is_source_unavailable(&AppError::new(codes::IO_FAILED)));
    }

    #[test]
    fn 降级安装命令_官网优先且可回落GitHub() {
        let site = bootstrap_command_for(Some("https://quireld.cn/downloads/quireld/"));
        assert_eq!(site, "curl -fsSL https://quireld.cn/downloads/quireld/install-quireld.sh | sudo bash");

        let github = bootstrap_command_for(None);
        assert!(github.contains("github.com/Victor-C0905/Remote_Desktop/releases/latest/download/install-quireld.sh"));
    }

    #[test]
    fn 安装命令_路径经单引号包裹() {
        let cmd = build_install_command("/home/my user/.quirel-provision", "abc123", false, Some(9443));
        // 目录段必须整体引住（含空格家目录不得拆成两个 shell 词）
        assert!(cmd.contains("bash '/home/my user/.quirel-provision'/install.sh"), "实际: {cmd}");
        assert!(cmd.contains("--stagedir '/home/my user/.quirel-provision'"), "实际: {cmd}");
        assert!(cmd.contains("--port 9443"), "实际: {cmd}");
        assert!(!cmd.contains("--upgrade"), "实际: {cmd}");

        let upgrade = build_install_command("/root/.q", "abc", true, None);
        assert!(upgrade.contains("--upgrade"), "实际: {upgrade}");
        assert!(!upgrade.contains("--port"), "实际: {upgrade}");
    }

    #[test]
    fn 版本比较_数字段语义() {
        assert_eq!(compare_versions("0.3.3", "0.3.3"), std::cmp::Ordering::Equal);
        assert_eq!(compare_versions("0.3.2", "0.3.3"), std::cmp::Ordering::Less);
        assert_eq!(compare_versions("0.10.0", "0.9.9"), std::cmp::Ordering::Greater);
        assert_eq!(compare_versions("1.0", "1.0.0"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn 版本比较_预发布后缀低于正式版() {
        assert_eq!(compare_versions("0.3.0-beta.1", "0.3.0"), std::cmp::Ordering::Less);
        assert_eq!(compare_versions("0.3.0", "0.3.0-beta.1"), std::cmp::Ordering::Greater);
        assert_eq!(compare_versions("0.3.0-beta.1", "0.3.0-beta.1"), std::cmp::Ordering::Equal);
        // 数字标识按数值比较（beta.2 < beta.10）
        assert_eq!(compare_versions("0.3.0-beta.2", "0.3.0-beta.10"), std::cmp::Ordering::Less);
        // 同位置：数字标识 < 非数字标识
        assert_eq!(compare_versions("0.3.0-1", "0.3.0-beta"), std::cmp::Ordering::Less);
        // 非数字标识按字典序
        assert_eq!(compare_versions("0.3.0-alpha", "0.3.0-beta"), std::cmp::Ordering::Less);
        // 共享前缀相同、标识更多的一方更高（semver 规范优先级示例链）
        assert_eq!(compare_versions("0.3.0-beta", "0.3.0-beta.1"), std::cmp::Ordering::Less);
        let chain = ["alpha", "alpha.1", "alpha.beta", "beta", "beta.2", "beta.11", "rc.1"];
        for w in chain.windows(2) {
            assert_eq!(
                compare_versions(&format!("0.3.0-{}", w[0]), &format!("0.3.0-{}", w[1])),
                std::cmp::Ordering::Less,
                "{} 应低于 {}",
                w[0], w[1]
            );
        }
    }

    #[test]
    fn 版本归一化_剥离名称与构建后缀() {
        assert_eq!(normalize_version("quireld 0.3.3 (build abc)"), "0.3.3");
        assert_eq!(normalize_version("quireld 0.3.3-dev (build abc)"), "0.3.3");
        assert_eq!(normalize_version("0.3.3"), "0.3.3");
    }

    #[test]
    fn sha256解析_精确文件名匹配() {
        let content = "abc123def456abc123def456abc123def456abc123def456abc123def456abc1  quireld-0.3.3-release-linux-amd64.tar.gz\ndeadbeef  other.tar.gz\n";
        assert_eq!(
            parse_sha256_sums(content, "quireld-0.3.3-release-linux-amd64.tar.gz"),
            Some("abc123def456abc123def456abc123def456abc123def456abc123def456abc1".to_string())
        );
        assert_eq!(parse_sha256_sums(content, "missing.tar.gz"), None);
        // 非法哈希长度拒绝
        let bad = "short  quireld-0.3.3-release-linux-amd64.tar.gz";
        assert_eq!(parse_sha256_sums(bad, "quireld-0.3.3-release-linux-amd64.tar.gz"), None);
    }

    #[test]
    fn 安装结果行解析() {
        let out = ">>> [1/3] 安装程序...\n✓ 安装成功！\nQUIRELD_INSTALL_OK port=9443 config=/etc/quireld/quireld.toml mode=install";
        let ok = parse_install_ok(out).unwrap();
        assert_eq!(ok.port, 9443);
        assert_eq!(ok.config_path, "/etc/quireld/quireld.toml");
        assert!(parse_install_ok("no marker").is_err());
    }

    #[test]
    fn 配置解析_端口与证书路径() {
        let body = "[server]\nbind = \"0.0.0.0\"\nquic_port = 9443\ncert_path = \"./cert.pem\"\n";
        let cfg = parse_quireld_config(body);
        assert_eq!(cfg.quic_port, Some(9443));
        assert_eq!(cfg.cert_path, Some("./cert.pem".to_string()));
    }

    #[test]
    fn 证书路径解析_相对与绝对() {
        assert_eq!(resolve_cert_path("./cert.pem"), "/var/lib/quireld/cert.pem");
        assert_eq!(resolve_cert_path("cert.pem"), "/var/lib/quireld/cert.pem");
        assert_eq!(resolve_cert_path("/etc/quireld/cert.pem"), "/etc/quireld/cert.pem");
    }

    #[test]
    fn 探测输出解析_完整字段() {
        let out = "ARCH=x86_64\nUID=0\nUNAME=root\nHOMEDIR=/root\nSYSTEMD=running\nSUDO_NOPASS=1\nSUDO_BIN=1\nDOAS_BIN=0\nBIN=/usr/local/bin/quireld\nUNIT=quireld.service enabled enabled\nVER=quireld 0.3.3 (build)\nCFG_BEGIN\nquic_port = 9443\nCFG_END";
        let p = parse_probe_output(out).unwrap();
        assert_eq!(p.arch, "x86_64");
        assert!(p.is_root);
        assert!(p.sudo_nopass);
        assert!(p.has_sudo);
        assert!(!p.has_doas);
        assert!(p.bin_found && p.unit_found);
        assert_eq!(normalize_version(&p.version), "0.3.3");
        assert_eq!(parse_quireld_config(&p.config_body).quic_port, Some(9443));
    }

    #[test]
    fn sudo归类_无sudo与仅有doas给出明确原因() {
        // root 直登
        assert_eq!(classify_sudo(true, false, false, false, false), (SudoKind::None, None));
        // 免密 sudo
        assert_eq!(classify_sudo(false, true, true, false, false), (SudoKind::Passwordless, None));
        // 密码登录 + sudo → 可注入密码
        assert_eq!(classify_sudo(false, false, true, false, true), (SudoKind::Password, None));
        // 没有 sudo，也没有 doas
        assert_eq!(
            classify_sudo(false, false, false, false, true),
            (SudoKind::Unsupported, Some(SudoBlockReason::NoSudo))
        );
        // 只有 doas
        assert_eq!(
            classify_sudo(false, false, false, true, true),
            (SudoKind::Unsupported, Some(SudoBlockReason::DoasOnly))
        );
        // sudo 存在但密钥登录无法提供密码
        assert_eq!(
            classify_sudo(false, false, true, false, false),
            (SudoKind::Unsupported, Some(SudoBlockReason::SudoRequiresPassword))
        );
    }

    #[test]
    fn 探测输出解析_字段缺失报错() {
        assert!(parse_probe_output("garbage").is_err());
    }

    #[test]
    fn http客户端_带超时可构建() {
        assert!(build_http_client().is_ok());
    }

    #[test]
    fn 缓存路径_按版本与tarball名布局() {
        let (tarball, sidecar) = release_cache_paths(
            Path::new("/cache"),
            "0.3.3",
            "quireld-0.3.3-release-linux-amd64.tar.gz",
        );
        assert_eq!(
            tarball,
            Path::new("/cache/releases/0.3.3/quireld-0.3.3-release-linux-amd64.tar.gz")
        );
        assert_eq!(
            sidecar,
            Path::new("/cache/releases/0.3.3/quireld-0.3.3-release-linux-amd64.tar.gz.sha256")
        );
    }

    /// 临时目录夹具（测试结束即清理）
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("quirel-cache-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn 缓存复验_哈希一致命中_损坏或缺失不命中() {
        let tmp = TempDir::new();
        let tarball = tmp.0.join("pkg.tar.gz");
        let sidecar = tmp.0.join("pkg.tar.gz.sha256");
        std::fs::write(&tarball, b"fake tarball bytes").unwrap();
        let real_sha = sha256_hex_file(&tarball).unwrap();

        // 旁挂记录缺失 → 未命中
        assert!(!cache_entry_valid(&tarball, &sidecar));
        // 记录正确 → 命中
        std::fs::write(&sidecar, format!("{real_sha}\n")).unwrap();
        assert!(cache_entry_valid(&tarball, &sidecar));
        // 记录非法（长度/字符）→ 未命中
        std::fs::write(&sidecar, "short").unwrap();
        assert!(!cache_entry_valid(&tarball, &sidecar));
        // 记录被篡改（与文件实际哈希不符）→ 未命中
        std::fs::write(&sidecar, format!("{}\n", "a".repeat(64))).unwrap();
        assert!(!cache_entry_valid(&tarball, &sidecar));
        // tarball 缺失 → 未命中
        std::fs::write(&sidecar, format!("{real_sha}\n")).unwrap();
        std::fs::remove_file(&tarball).unwrap();
        assert!(!cache_entry_valid(&tarball, &sidecar));
    }

    #[test]
    fn 缓存写入_落盘可复验且清理旧版本() {
        let tmp = TempDir::new();
        let src = tmp.0.join("src.tar.gz");
        std::fs::write(&src, b"fake tarball bytes").unwrap();
        let sha = sha256_hex_file(&src).unwrap();
        // 预置旧版本缓存目录（应被清理）
        let old_dir = tmp.0.join("releases").join("0.0.1");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(old_dir.join("old.tar.gz"), b"old").unwrap();

        store_cache_entry(Some(&tmp.0), "0.3.3", "pkg.tar.gz", &src, &sha);

        let (dest, sidecar) = release_cache_paths(&tmp.0, "0.3.3", "pkg.tar.gz");
        assert!(cache_entry_valid(&dest, &sidecar));
        assert!(!old_dir.exists(), "旧版本缓存目录应被清理");
        // cache_dir=None 时不写任何文件
        store_cache_entry(None, "0.3.3", "pkg.tar.gz", &src, &sha);
    }
}
