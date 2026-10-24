//! 系统浏览器 spawn（流量走本地 SOCKS5 隧道）
//!
//! 场景：内置浏览器（webview）页面交给 OS 桌面浏览器继续操作，流量
//! 与内置浏览器同路——`--proxy-server` 指向本会话的 SOCKS5 监听端口
//! （127.0.0.1 → QUIC → 服务器出口），保证被墙站点在系统浏览器里同样可达。
//! 浏览器选择来自项目设置（settingsStore.browserId：auto/edge/chrome/firefox
//! /自定义路径），与旧版浏览器功能对齐。
//!
//! 关键：`--user-data-dir` 独立 profile 绝不能省——Chromium 系浏览器的
//! 单实例机制会把命令行 URL 转发给用户日常浏览器实例，新进程的
//! `--proxy-server` 参数随之被静默忽略（旧版实证）。独立 profile 的
//! 首个实例持有代理参数，后续打开同 URL 转发给它，代理语义保持。
//!
//! 局限：Firefox 命令行不支持 `--proxy-server`，参数被忽略后直连（打不开
//! 被墙站点）；候选表保留 Firefox 仅供用户显式选择，实际代理语义以
//! Chromium 系为准。

use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;
use tauri::Manager;

use crate::errors::{codes, AppError};
use super::ProxySessionManager;

/// 已知浏览器候选表（按优先顺序探测）：(路径, id, 显示名)
/// id 稳定持久化用；显示名供 UI 展示
const BROWSER_CANDIDATES: &[(&str, &str, &str)] = &[
    (
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        "edge",
        "Microsoft Edge",
    ),
    (
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        "edge",
        "Microsoft Edge",
    ),
    (
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        "chrome",
        "Google Chrome",
    ),
    (
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        "chrome",
        "Google Chrome",
    ),
    (
        r"C:\Program Files\Mozilla Firefox\firefox.exe",
        "firefox",
        "Mozilla Firefox",
    ),
];

/// 探测可用的浏览器路径，返回 (路径, id)
///
/// 自动模式默认行为：按候选顺序取第一个存在的
fn detect_browser() -> Option<(PathBuf, &'static str)> {
    for (path, id, _) in BROWSER_CANDIDATES {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some((p, id));
        }
    }
    None
}

/// 枚举系统上已安装的浏览器（去重，按候选顺序）
///
/// 返回 (id, 显示名, 路径)，供前端展示选择列表
fn list_browsers() -> Vec<(&'static str, &'static str, String)> {
    let mut seen: Vec<&str> = Vec::new();
    let mut result = Vec::new();
    for (path, id, name) in BROWSER_CANDIDATES {
        let p = PathBuf::from(path);
        if p.exists() && !seen.contains(id) {
            seen.push(id);
            result.push((*id, *name, path.to_string()));
        }
    }
    result
}

/// 本机可用浏览器信息（proxy_list_browsers 返回项）
#[derive(Serialize)]
pub struct BrowserInfo {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// 枚举系统上已安装的浏览器（供设置页"默认浏览器"下拉）
#[tauri::command]
pub fn proxy_list_browsers() -> Vec<BrowserInfo> {
    list_browsers()
        .into_iter()
        .map(|(id, name, path)| BrowserInfo {
            id: id.to_string(),
            name: name.to_string(),
            path,
        })
        .collect()
}

/// 根据用户设置解析浏览器（与旧版语义一致）
///
/// - `browser_id` 为空或 "auto"：走自动探测
/// - 匹配候选 id（edge/chrome/firefox）：返回该浏览器实际路径（未安装则 Err）
/// - 其他值视为自定义浏览器可执行文件路径（如便携版 Chrome/Chromium 系浏览器），
///   存在即可用——参数体系为 Chromium 通用
fn resolve_browser(browser_id: &str) -> Result<(PathBuf, String), AppError> {
    if browser_id.is_empty() || browser_id == "auto" {
        return detect_browser()
            .map(|(p, id)| (p, id.to_string()))
            .ok_or_else(|| AppError::new(codes::PROXY_BROWSER_NOT_FOUND));
    }

    // 按候选 id 匹配
    for (path, id, _) in BROWSER_CANDIDATES {
        if *id == browser_id {
            let p = PathBuf::from(path);
            if p.exists() {
                return Ok((p, id.to_string()));
            }
            return Err(
                AppError::new(codes::PROXY_BROWSER_NOT_INSTALLED).with_param("browser", browser_id)
            );
        }
    }

    // 自定义路径
    let p = PathBuf::from(browser_id);
    if p.is_file() {
        Ok((p, "custom".into()))
    } else {
        Err(
            AppError::new(codes::PROXY_BROWSER_PATH_INVALID)
                .with_param("path", browser_id.to_string()),
        )
    }
}

/// 把当前页面 URL 交给挂隧道的系统浏览器打开
///
/// - 浏览器按项目设置解析（browser_id）；隧道端口取自该服务器的活跃
///   SOCKS5 会话（`socks_port` 已过滤挂起会话，不会把流量串到别的服务器
///   隧道上）；无活跃会话即报错
/// - `url` 可空（地址栏是空白页/内部地址时仅启动浏览器开新标签页）；
///   有值时仅接受 http/https（防命令行注入：`-` 开头的字符串会被浏览器
///   解析为命令行参数）
#[tauri::command]
pub async fn browser_open_in_system(
    app: tauri::AppHandle,
    server_id: String,
    browser_id: String,
    url: Option<String>,
) -> Result<(), AppError> {
    // 只放行网页地址
    if let Some(u) = &url {
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            return Err(AppError::new(codes::PROXY_BROWSER_PATH_INVALID)
                .with_detail(format!("仅支持 http/https URL，收到: {u}")));
        }
    }

    // 隧道端口：必须来自该服务器的活跃会话
    let socks_port = app
        .state::<ProxySessionManager>()
        .socks_port(&server_id)
        .ok_or_else(|| {
            AppError::new(codes::CONNECTION_NOT_FOUND)
                .with_detail("该服务器无活跃的代理会话，无法为系统浏览器提供隧道")
        })?;

    let (browser_path, browser_id_resolved) = resolve_browser(&browser_id)?;

    // 独立 profile（App 缓存目录下固定一份）：隔离用户日常实例，
    // 保证 --proxy-server 生效（见模块文档）；首次创建含首启动画跳过
    let profile_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::new(codes::PROXY_BROWSER_ENV_FAILED).with_detail(e.to_string()))?
        .join("system-browser");

    // async command 里的阻塞 spawn：进程启动是毫秒级操作，spawn 即返回
    // （不等浏览器退出），无需 spawn_blocking
    let mut cmd = Command::new(&browser_path);
    cmd.arg(format!("--proxy-server=socks5://127.0.0.1:{socks_port}"))
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        // 强制新窗口：同 profile 的存活实例会把 URL 转发给自身（单实例机制），
        // 默认开成后台标签页；--new-window 明确要求独立窗口
        .arg("--new-window");
    if let Some(u) = &url {
        cmd.arg(u);
    }
    cmd.spawn()
        .map_err(|e| AppError::new(codes::PROXY_BROWSER_SPAWN_FAILED).with_detail(e.to_string()))?;

    tracing::info!(
        "[Browser] 已用系统浏览器打开（走隧道 socks5://127.0.0.1:{socks_port}）: \
         browser={browser_id_resolved} url={}",
        url.as_deref().unwrap_or("(新标签页)")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_browser_returns_existing() {
        // Windows 开发环境必有 Edge 或 Chrome；找不到是环境异常
        let result = detect_browser();
        assert!(result.is_some(), "未探测到任何系统浏览器");
        let (path, _) = result.unwrap();
        assert!(path.exists());
    }

    #[test]
    fn resolve_auto_equals_detect() {
        let (auto_path, _) = resolve_browser("auto").unwrap();
        let (detect_path, _) = detect_browser().unwrap();
        assert_eq!(auto_path, detect_path);
    }

    #[test]
    fn resolve_unknown_id_errors() {
        // 非候选 id 且非存在路径 → 报错
        assert!(resolve_browser("nonexistent-browser-xyz").is_err());
    }

    #[test]
    fn resolve_existing_path_as_custom() {
        // 任意存在的文件路径可作为自定义浏览器
        let (path, _) = detect_browser().unwrap();
        let (resolved, id) = resolve_browser(&path.to_string_lossy()).unwrap();
        assert_eq!(resolved, path);
        assert_eq!(id, "custom");
    }

    #[test]
    fn list_browsers_deduped() {
        let list = list_browsers();
        // 去重：同 id 只出现一次
        let ids: Vec<&str> = list.iter().map(|(id, _, _)| *id).collect();
        for (i, id) in ids.iter().enumerate() {
            assert!(!ids[i + 1..].contains(id), "id 重复: {}", id);
        }
        // 所有路径存在
        for (_, _, path) in &list {
            assert!(PathBuf::from(path).exists());
        }
    }
}
