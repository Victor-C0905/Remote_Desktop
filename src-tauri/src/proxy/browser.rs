//! 系统浏览器探测与 spawn
//!
//! 通过 --proxy-server 启动参数让浏览器流量走本地 SOCKS5；
//! --user-data-dir 强制独立 profile（否则单实例机制会忽略代理参数）。
//!
//! 浏览器选择策略：
//! - `detect_browser()`：按候选顺序自动探测（默认行为）
//! - `list_browsers()`：枚举系统上已安装的浏览器（供前端选择）
//! - `resolve_browser()`：根据用户选择（id 或自定义路径）解析浏览器

use std::path::PathBuf;
use std::process::{Child, Command};

/// 已知浏览器候选表：(路径, id, 显示名)
/// id 稳定持久化用；显示名供 UI 展示
const BROWSER_CANDIDATES: &[( &str, &str, &str )] = &[
    (r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe", "edge", "Microsoft Edge"),
    (r"C:\Program Files\Microsoft\Edge\Application\msedge.exe", "edge", "Microsoft Edge"),
    (r"C:\Program Files\Google\Chrome\Application\chrome.exe", "chrome", "Google Chrome"),
    (r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe", "chrome", "Google Chrome"),
    (r"C:\Program Files\Mozilla Firefox\firefox.exe", "firefox", "Mozilla Firefox"),
];

/// 探测可用的浏览器路径，返回 (路径, id)
///
/// 自动模式默认行为：按候选顺序取第一个存在的
pub fn detect_browser() -> Option<(PathBuf, &'static str)> {
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
pub fn list_browsers() -> Vec<(&'static str, &'static str, String)> {
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

/// 根据用户选择解析浏览器
///
/// - `browser_id` 为空或 "auto"：走自动探测
/// - 匹配候选 id（edge/chrome/firefox）：返回该浏览器实际路径（未安装则 Err）
/// - 其他值视为自定义浏览器可执行文件路径（如便携版 Chrome/Chromium 系浏览器），
///   存在即可用——参数体系为 Chromium 通用
pub fn resolve_browser(browser_id: &str) -> Result<(PathBuf, String), String> {
    if browser_id.is_empty() || browser_id == "auto" {
        return detect_browser()
            .map(|(p, id)| (p, id.to_string()))
            .ok_or_else(|| "未找到 Edge/Chrome/Firefox，可在远程浏览应用中选择自定义浏览器路径".into());
    }

    // 按候选 id 匹配
    for (path, id, _) in BROWSER_CANDIDATES {
        if *id == browser_id {
            let p = PathBuf::from(path);
            if p.exists() {
                return Ok((p, id.to_string()));
            }
            return Err(format!("所选浏览器（{}）未安装", browser_id));
        }
    }

    // 自定义路径
    let p = PathBuf::from(browser_id);
    if p.is_file() {
        Ok((p, "custom".into()))
    } else {
        Err(format!("浏览器路径无效或不是文件: {}", browser_id))
    }
}

/// spawn 浏览器：所有流量走本地 SOCKS5 监听
pub fn spawn_browser(
    browser_path: &PathBuf,
    socks_port: u16,
    profile_dir: &PathBuf,
) -> Result<Child, String> {
    Command::new(browser_path)
        .arg(format!("--proxy-server=socks5://127.0.0.1:{}", socks_port))
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        // 强制新窗口：同 profile 再次 spawn 时浏览器会把 URL 转发给已运行实例，
        // 默认开成新标签页；--new-window 明确要求独立窗口
        .arg("--new-window")
        .arg("about:blank")
        .spawn()
        .map_err(|e| format!("启动浏览器失败: {}", e))
}

/// 终止占用指定 profile 的所有浏览器进程（残留实例清理）
///
/// 使用场景：上次 App 异常退出（崩溃/被强杀/dev 重启，RunEvent::Exit 未执行）后
/// 浏览器残留。残留实例的代理参数指向已死端口（无法被新会话接管），且占用
/// profile 单实例锁，导致新 spawn 的进程退化为"转发器"立即退出。
/// 匹配规则：进程名 = 浏览器可执行文件名 且 命令行包含 profile 路径，
/// 不会误杀用户日常浏览器实例（其 profile 路径不同）。
#[cfg(windows)]
pub fn terminate_profile_processes(
    browser_path: &PathBuf,
    profile_dir: &PathBuf,
) -> Result<(), String> {
    let exe_name = browser_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "浏览器路径缺少可执行文件名".to_string())?;
    // PowerShell 单引号字符串转义：' → ''（profile 路径来自 app_cache_dir，正常无引号，防御性处理）
    let exe_pat = exe_name.replace('\'', "''");
    let profile_pat = profile_dir.to_string_lossy().replace('\'', "''");
    // Get-CimInstance 枚举进程（含命令行）→ 按进程名 + 命令行匹配 → 强制终止。
    // -like 通配符匹配默认不区分大小写；全程单引号避免嵌套双引号转义问题
    let script = format!(
        "Get-CimInstance Win32_Process | Where-Object {{ $_.Name -eq '{}' -and $_.CommandLine -like '*{}*' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}",
        exe_pat, profile_pat
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .map_err(|e| format!("调用 PowerShell 清理残留浏览器失败: {}", e))?;
    if !status.success() {
        return Err(format!(
            "清理残留浏览器进程失败（exit={}）",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

/// Unix 版残留清理：pkill -f 按完整命令行匹配 profile 路径
#[cfg(unix)]
pub fn terminate_profile_processes(
    browser_path: &PathBuf,
    profile_dir: &PathBuf,
) -> Result<(), String> {
    let _ = browser_path; // pkill -f 直接按命令行匹配，无需进程名
    // pkill -f 的模式为扩展正则，路径特殊字符（. 等）需转义为字面量
    let pattern = profile_dir
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '/' || c == '_' || c == '-' {
                c.to_string()
            } else {
                format!(r"\{}", c)
            }
        })
        .collect::<String>();
    let status = Command::new("pkill")
        .arg("-f")
        .arg(&pattern)
        .status()
        .map_err(|e| format!("调用 pkill 清理残留浏览器失败: {}", e))?;
    // pkill 退出码 1 = 无匹配进程（本就是干净状态），不算错误
    if !status.success() && status.code() != Some(1) {
        return Err(format!(
            "清理残留浏览器进程失败（exit={}）",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_browser_returns_existing() {
        // Windows 开发环境必有 Edge 或 Chrome；找不到是环境异常
        let result = detect_browser();
        assert!(result.is_some(), "未探测到任何浏览器");
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
