//! 客户端驱动的 quireld 更新：检查 / 应用 / 回滚
//!
//! 链路：GitHub Releases 下载（复用 provision.rs 的下载/校验逻辑）→
//! 经 QUIC 同流推送（AgentUpdateUploadRequest + FileChunk 分块）→ staging 校验
//! → commit（服务端备份/原子替换/热更新/失败自动回滚）。
//!
//! 安全约束：
//! - 无 "agent_update" 能力的旧 Agent 直接拒绝（盲发新 payload 会导致旧 Agent 断流）
//! - commit 超时单独处理：超时后先查 status 再判断（服务端可能实际已完成）
//! - 进度经 "agent-update-progress" 事件推送前端

use serde::Serialize;
use tauri::{Emitter, Manager};

use crate::connection;
use crate::errors::{codes, AppError};
use crate::provision::{download_release, sha256_hex_file};
use quirel_protocol::{Payload, StagedUpdate};

/// 更新检查结果（前端版本卡片展示）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    /// 是否支持更新（无 agent_update 能力的旧 Agent → false）
    pub supported: bool,
    /// 服务端更新开关（false 时客户端隐藏更新 UI）
    pub enabled: bool,
    /// Manager 版本（QUIC 层进程）
    pub manager_version: Option<String>,
    /// Worker 版本（最近 commit 版本；None = 未知/未 commit 过）
    pub worker_version: Option<String>,
    /// Worker 新于 Manager（半更新状态，UI 提示需 systemctl restart）
    pub manager_lags_worker: bool,
    /// GitHub 最新版本（获取失败为 None）
    pub latest_version: Option<String>,
    /// 是否有可用更新（latest > manager 时 true；latest 未知为 None）
    pub update_available: Option<bool>,
    /// 暂存区中的二进制（上传成功等待 commit）
    pub staged: Option<StagedUpdate>,
    /// 最近一次失败操作的原因
    pub last_error: Option<String>,
}

/// 版本管理卡片用：更新应用/回滚结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateActionOutcome {
    pub success: bool,
    pub error: Option<String>,
    pub manager_version: Option<String>,
    pub worker_version: Option<String>,
    pub manager_lags_worker: bool,
    pub rolled_back: bool,
}

// ── Tauri 命令 ────────────────────────────────────────────

/// 检查更新：服务端状态（版本/staged/能力门控）+ GitHub 最新版本比对
#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn agent_update_check(server_id: String, app: tauri::AppHandle) -> Result<UpdateCheck, AppError> {
    // 能力门控：旧 Agent 无法解码 agent_update_* payload，盲发会断流拆连
    let caps = connection::get_agent_capabilities(server_id.clone(), app.clone())?;
    if !caps.contains(&"agent_update".to_string()) {
        return Ok(UpdateCheck {
            supported: false,
            enabled: false,
            manager_version: None,
            worker_version: None,
            manager_lags_worker: false,
            latest_version: None,
            update_available: None,
            staged: None,
            last_error: None,
        });
    }

    let status = send_update_command(&server_id, Payload::AgentUpdateStatus {}, app.clone()).await?;
    let (manager_version, worker_version, manager_lags_worker, enabled, staged, last_error) =
        match status.payload {
            Payload::AgentUpdateStatusResponse {
                manager_version, worker_version, manager_lags_worker,
                enabled: cfg_enabled, staged, last_result,
                ..
            } => (
                Some(manager_version),
                worker_version,
                manager_lags_worker,
                cfg_enabled,
                staged,
                last_result.filter(|r| !r.success).and_then(|r| r.error),
            ),
            other => {
                return Err(AppError::new(codes::UNEXPECTED_RESPONSE)
                    .with_detail(format!("期望更新状态响应，收到: {}", other.type_name())));
            }
        };

    // GitHub 最新版本（失败不阻断状态展示——latest_version=None 时 UI 不显示更新按钮）
    let latest_version = fetch_latest_version().await;
    // 更新判定以客户端版本为锚（lockstep：服务器不得升到超过客户端的版本，
    // 否则破坏「同版本号 = 协议兼容」承诺）；latest 仅作展示参考
    let update_available = update_available_for(manager_version.as_deref());

    Ok(UpdateCheck {
        supported: true,
        enabled,
        manager_version,
        worker_version,
        manager_lags_worker,
        latest_version,
        update_available,
        staged,
        last_error,
    })
}

/// 应用更新：下载 → 分块上传 → commit（服务端备份/替换/热更新/自动回滚）
#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn agent_update_apply(server_id: String, app: tauri::AppHandle) -> Result<UpdateActionOutcome, AppError> {
    ensure_update_supported(&server_id, app.clone()).await?;
    emit_progress(&app, &server_id, "download", "下载最新 quireld", 5);

    // 服务器架构：由 provision_detect 阶段写入 ServerConfig 后经前端传入不可行
    // （命令参数简化为环境变量覆盖）；默认 amd64，Agent 侧 ELF e_machine 校验
    // 会拒绝错架构（不会误替换），报错即提示用户联系手动处理。
    let arch = std::env::var("QUIREL_UPDATE_ARCH").unwrap_or_else(|_| "amd64".to_string());
    let cache_dir = app.path().app_cache_dir().ok();
    let pkg = download_release(&arch, cache_dir.as_deref()).await?;
    let binary_path = pkg.extract_dir.join(&pkg.dir_name).join("quireld");
    let binary_sha = sha256_hex_file(&binary_path)?;
    emit_progress(&app, &server_id, "verify", "SHA-256 校验通过", 20);

    // 同流多帧上传（请求 + 分块 + 完成 → 服务端校验落盘 staging）
    let data = std::fs::read(&binary_path)
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("读取二进制失败: {e}")))?;
    let upload_resp = connection::send_agent_update_upload(
        &server_id, &app, data.len() as u64, &binary_sha, &pkg.version, data,
    ).await?;
    let staged = match upload_resp {
        Payload::AgentUpdateUploadResponse { success, error, staged } => {
            if success {
                staged.ok_or_else(|| AppError::new(codes::UNEXPECTED_RESPONSE)
                    .with_detail("上传成功但未返回 staged 信息"))?
            } else {
                return Err(AppError::new(codes::UPDATE_STAGING_REJECTED)
                    .with_detail(error.unwrap_or_else(|| "服务端拒绝暂存".to_string())));
            }
        }
        other => {
            return Err(AppError::new(codes::UNEXPECTED_RESPONSE)
                .with_detail(format!("期望上传结果，收到: {}", other.type_name())));
        }
    };
    emit_progress(&app, &server_id, "staged", &format!("服务端校验通过（{}），已暂存", staged.version), 60);

    // commit（服务端备份 → 替换 → 热更新 → 失败自动回滚）
    emit_progress(&app, &server_id, "commit", "提交更新（服务端热更新中）", 75);
    let commit_resp = connection::remote_send(
        server_id.clone(),
        Payload::AgentUpdateCommitRequest { expected_sha256: binary_sha },
        app.clone(),
    )
    .await;
    let commit_payload = match commit_resp {
        Ok(env) => env.payload,
        // commit 含热更新等待（远超 remote_send 兜底超时）——超时后先查 status：
        // staged 已清空 = 服务端实际完成（不可盲目重试，is_reloading 防重入会拒绝）
        Err(e) if e.code == codes::RESPONSE_TIMEOUT => {
            emit_progress(&app, &server_id, "confirm", "提交超时，查询服务端实际状态", 90);
            let status = send_update_command(&server_id, Payload::AgentUpdateStatus {}, app.clone()).await?;
            status.payload
        }
        Err(e) => return Err(e),
    };

    let outcome = match commit_payload {
        Payload::AgentUpdateActionResult {
            success, error, manager_version, worker_version, manager_lags_worker, rolled_back,
        } => {
            emit_progress(&app, &server_id, "done", if success { "更新完成" } else { "更新失败" }, 100);
            UpdateActionOutcome {
                success,
                error,
                manager_version: Some(manager_version),
                worker_version,
                manager_lags_worker,
                rolled_back,
            }
        }
        // 超时后查得的 status：完成/失败判定与版本回填统一走 outcome_from_status
        status @ Payload::AgentUpdateStatusResponse { .. } => {
            let outcome = outcome_from_status(&status)?;
            emit_progress(&app, &server_id, "done", if outcome.success { "更新完成（超时后确认）" } else { "更新失败" }, 100);
            outcome
        }
        other => {
            return Err(AppError::new(codes::UNEXPECTED_RESPONSE)
                .with_detail(format!("期望提交结果，收到: {}", other.type_name())));
        }
    };
    Ok(outcome)
}

/// 手动回滚到最近备份
#[tauri::command]
#[tracing::instrument(skip(app), fields(server_id = %server_id))]
pub async fn agent_update_rollback(server_id: String, app: tauri::AppHandle) -> Result<UpdateActionOutcome, AppError> {
    ensure_update_supported(&server_id, app.clone()).await?;
    let resp = send_update_command(&server_id, Payload::AgentUpdateRollbackRequest {}, app.clone()).await?;
    match resp.payload {
        Payload::AgentUpdateActionResult {
            success, error, manager_version, worker_version, manager_lags_worker, rolled_back,
        } => Ok(UpdateActionOutcome {
            success,
            error,
            manager_version: Some(manager_version),
            worker_version,
            manager_lags_worker,
            rolled_back,
        }),
        other => Err(AppError::new(codes::UNEXPECTED_RESPONSE)
            .with_detail(format!("期望回滚结果，收到: {}", other.type_name()))),
    }
}

// ── 内部工具 ─────────────────────────────────────────────

/// commit 超时后经 status 帧确认服务端实际结果：
/// - staged 已清空 + last_result 为成功 update → 实际完成（manager/worker 版本
///   取响应真实值，不得以时间戳冒充版本号）
/// - 其余（仍处暂存 / 无成功记录 / 响应类型不符）→ 提交失败，给出可行动的错误
fn outcome_from_status(status: &Payload) -> Result<UpdateActionOutcome, AppError> {
    match status {
        Payload::AgentUpdateStatusResponse {
            manager_version,
            worker_version,
            manager_lags_worker,
            staged: None,
            last_result: Some(r),
            ..
        } if r.success && r.action == "update" => Ok(UpdateActionOutcome {
            success: true,
            error: None,
            manager_version: Some(manager_version.clone()),
            worker_version: worker_version.clone(),
            manager_lags_worker: *manager_lags_worker,
            rolled_back: false,
        }),
        Payload::AgentUpdateStatusResponse { staged, last_result, .. } => {
            Err(AppError::new(codes::UPDATE_COMMIT_FAILED).with_detail(
                last_result
                    .as_ref()
                    .and_then(|r| r.error.clone())
                    .unwrap_or_else(|| if staged.is_some() {
                        "提交超时且服务端仍处暂存状态，请稍后重试".to_string()
                    } else {
                        "提交超时且无法确认服务端状态".to_string()
                    }),
            ))
        }
        other => Err(AppError::new(codes::UNEXPECTED_RESPONSE)
            .with_detail(format!("期望更新状态响应，收到: {}", other.type_name()))),
    }
}

async fn ensure_update_supported(server_id: &str, app: tauri::AppHandle) -> Result<(), AppError> {
    let caps = connection::get_agent_capabilities(server_id.to_string(), app)?;
    if !caps.contains(&"agent_update".to_string()) {
        return Err(AppError::new(codes::UPDATE_NOT_SUPPORTED)
            .with_detail("当前服务器 Agent 版本不支持客户端驱动更新"));
    }
    Ok(())
}

/// 发送更新命令并取回 Envelope（错误帧 → AppError）
async fn send_update_command(server_id: &str, payload: Payload, app: tauri::AppHandle) -> Result<quirel_protocol::Envelope, AppError> {
    let env = connection::remote_send(server_id.to_string(), payload, app).await?;
    if let Payload::Error { code, message } = env.payload {
        return Err(AppError::new(codes::UPDATE_AGENT_REJECTED)
            .with_detail(format!("Agent 返回错误（code={code}）: {message}")));
    }
    Ok(env)
}

fn emit_progress(app: &tauri::AppHandle, server_id: &str, step: &str, detail: &str, percent: u32) {
    let _ = app.emit(
        "agent-update-progress",
        serde_json::json!({ "serverId": server_id, "step": step, "detail": detail, "percent": percent }),
    );
}

/// 最新版本号（官网 latest.json 优先，GitHub API 兜底；失败返回 None——UI 不显示更新按钮）
async fn fetch_latest_version() -> Option<String> {
    let client = crate::provision::build_http_client().ok()?;

    // 1. 官网静态源（主源 → 备用源，与取包顺序一致）
    for base in crate::provision::release_base_urls() {
        let url = format!("{}/latest.json", base);
        if let Some(version) = fetch_site_latest(&client, &url).await {
            return Some(version);
        }
    }

    // 2. GitHub 兜底
    let url = format!("https://api.github.com/repos/{}/releases/latest", crate::provision::QUIREL_REPO);
    let resp = client
        .get(&url)
        .header("User-Agent", "quirel-client")
        .send()
        .await
        .ok()?;
    let body: serde_json::Value = resp.json().await.ok()?;
    let tag = body["tag_name"].as_str()?;
    Some(tag.trim_start_matches('v').to_string())
}

/// 官网 latest.json（`{ "version": "0.3.3" }`）拉取与解析
async fn fetch_site_latest(client: &reqwest::Client, url: &str) -> Option<String> {
    let resp = client
        .get(url)
        .header("User-Agent", "quirel-client")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = resp.text().await.ok()?;
    parse_latest_json(&body)
}

fn parse_latest_json(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let version = value["version"].as_str()?;
    let trimmed = version.trim_start_matches('v');
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

/// 服务端是否有可更新版本：以客户端期望版本（lockstep）为锚。
/// None = 服务端版本未知；Some(true) 仅当客户端版本 > 服务端版本——
/// 服务端超前于客户端时返回 false（不得经客户端升到不兼容版本）。
pub fn update_available_for(manager_version: Option<&str>) -> Option<bool> {
    manager_version.map(|cur| {
        compare_semver(crate::provision::TARGET_QUIRELD_VERSION, cur) == std::cmp::Ordering::Greater
    })
}

/// 客户端语义化版本比较（统一委托 provision::compare_versions，含预发布后缀）
pub fn compare_semver(a: &str, b: &str) -> std::cmp::Ordering {
    crate::provision::compare_versions(a, b)
}

#[cfg(test)]
mod tests {
    use super::{compare_semver, outcome_from_status, parse_latest_json, update_available_for};
    use quirel_protocol::{AgentUpdateLastResult, Payload, StagedUpdate};

    #[test]
    fn 官网latest解析_支持v前缀与预发布版本() {
        assert_eq!(parse_latest_json(r#"{"version":"0.3.3"}"#).as_deref(), Some("0.3.3"));
        assert_eq!(parse_latest_json(r#"{"version":"v0.4.0-beta.1"}"#).as_deref(), Some("0.4.0-beta.1"));
        assert_eq!(parse_latest_json(r#"{"version":""}"#), None);
        assert_eq!(parse_latest_json("not-json"), None);
    }

    #[test]
    fn 超时确认_服务端已完成时回填真实版本() {
        let status = Payload::AgentUpdateStatusResponse {
            manager_version: "0.3.3".to_string(),
            worker_version: Some("0.3.3".to_string()),
            manager_lags_worker: false,
            is_reloading: false,
            enabled: true,
            staged: None,
            last_result: Some(AgentUpdateLastResult {
                action: "update".to_string(),
                success: true,
                error: None,
                timestamp: 1700000000,
            }),
        };
        let out = outcome_from_status(&status).unwrap();
        assert!(out.success);
        // worker 版本取响应真实值，不得以 last_result 时间戳冒充
        assert_eq!(out.worker_version, Some("0.3.3".to_string()));
        assert_eq!(out.manager_version, Some("0.3.3".to_string()));
        assert!(!out.rolled_back);
    }

    #[test]
    fn 超时确认_仍处暂存或无成功记录时报错() {
        let staged = Payload::AgentUpdateStatusResponse {
            manager_version: "0.3.3".to_string(),
            worker_version: None,
            manager_lags_worker: false,
            is_reloading: false,
            enabled: true,
            staged: Some(StagedUpdate {
                sha256: "ab".to_string(),
                size: 1,
                version: "0.3.4".to_string(),
                staged_at: 0,
            }),
            last_result: None,
        };
        assert!(outcome_from_status(&staged).is_err());

        let unknown = Payload::AgentUpdateStatusResponse {
            manager_version: "0.3.3".to_string(),
            worker_version: None,
            manager_lags_worker: false,
            is_reloading: false,
            enabled: true,
            staged: None,
            last_result: None,
        };
        assert!(outcome_from_status(&unknown).is_err());
    }

    #[test]
    fn 更新判定_以客户端版本为锚() {
        assert_eq!(update_available_for(None), None);
        assert_eq!(update_available_for(Some("0.3.2")), Some(true));
        assert_eq!(update_available_for(Some("0.3.3")), Some(false));
        assert_eq!(update_available_for(Some("0.3.4")), Some(false));
    }

    #[test]
    fn 版本比较_数字段语义() {
        assert_eq!(compare_semver("0.3.3", "0.3.3"), std::cmp::Ordering::Equal);
        assert_eq!(compare_semver("0.3.2", "0.3.3"), std::cmp::Ordering::Less);
        assert_eq!(compare_semver("0.10.0", "0.9.9"), std::cmp::Ordering::Greater);
    }
}
