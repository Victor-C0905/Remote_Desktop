//! 系统浏览器探测与 spawn
//!
//! 通过 --proxy-server 启动参数让浏览器流量走本地 SOCKS5；
//! --user-data-dir 强制独立 profile（否则单实例机制会忽略代理参数）。

use std::path::PathBuf;
use std::process::{Child, Command};

/// 探测顺序：Edge（Windows 必装）→ Chrome
const BROWSER_CANDIDATES: &[&str] = &[
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
];

/// 探测可用的浏览器路径，返回 (路径, 名称)
pub fn detect_browser() -> Option<(PathBuf, &'static str)> {
    for (path, name) in BROWSER_CANDIDATES.iter().zip(["edge", "edge", "chrome", "chrome"]) {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some((p, name));
        }
    }
    None
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
        .arg("about:blank")
        .spawn()
        .map_err(|e| format!("启动浏览器失败: {}", e))
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
}
