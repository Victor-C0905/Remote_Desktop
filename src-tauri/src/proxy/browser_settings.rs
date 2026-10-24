//! 浏览器设置：随服务器持久化（存于该 server 的浏览器数据目录 settings.json）
//!
//! 分层：
//! - SearchEngine / BrowserSettings：纯数据模型（serde，与前端 camelCase 对齐）
//! - load / save：磁盘读写（读失败回退默认值，不阻断浏览器打开）
//! - search_url / percent_encode：搜索词 → 搜索结果页 URL（供 normalize_url 调用）
//!
//! 存储位置沿用 browser_data_dir（按 server 隔离）：无痕态使用兄弟即弃目录，
//! settings.json 始终在持久目录，不受无痕清空影响。

use std::path::Path;

use crate::errors::{codes, AppError};

/// 内置搜索引擎（当前 Google / Bing；serde 值即前端传入的字符串）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchEngine {
    Google,
    Bing,
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::Google
    }
}

impl SearchEngine {
    /// 搜索词 → 搜索结果页 URL
    pub fn search_url(self, query: &str) -> String {
        match self {
            Self::Google => format!("https://www.google.com/search?q={}", percent_encode(query)),
            Self::Bing => format!("https://www.bing.com/search?q={}", percent_encode(query)),
        }
    }

    /// 浏览器首页 / 新标签页默认 URL：对应搜索引擎官网。
    /// 秒开依赖 webview user_data_dir 的持久化 HTTP 缓存（首次全量加载，
    /// 之后静态资源命中缓存）。
    pub fn home_url(self) -> String {
        match self {
            Self::Google => "https://www.google.com/".into(),
            Self::Bing => "https://www.bing.com/".into(),
        }
    }
}

/// 浏览器设置（serde camelCase 与前端 TS 类型直接对齐）
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BrowserSettings {
    /// 地址栏搜索词使用的默认搜索引擎
    pub search_engine: SearchEngine,
    /// 无痕浏览（Environment 使用即弃数据目录，会话结束清空）
    pub incognito: bool,
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            search_engine: SearchEngine::default(),
            incognito: false,
        }
    }
}

/// 设置文件名（位于浏览器数据目录 = WebView2 user-data-dir，与浏览器数据同生共死）
const FILE_NAME: &str = "settings.json";

/// 读取设置：文件缺失/损坏一律回退默认值（设置读取不应阻断浏览器打开）
pub fn load(dir: &Path) -> BrowserSettings {
    match std::fs::read(dir.join(FILE_NAME)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => BrowserSettings::default(),
    }
}

/// 保存设置（整文件覆盖写；体积小、频率低，无需原子替换）
pub fn save(dir: &Path, settings: &BrowserSettings) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("serialize browser settings: {e}")))?;
    std::fs::write(dir.join(FILE_NAME), bytes)
        .map_err(|e| AppError::new(codes::IO_FAILED).with_detail(format!("write browser settings: {e}")))?;
    Ok(())
}

/// 最小百分比编码（RFC 3986 unreserved 之外全转义；查询串够用，避免引依赖）
/// 供 search_url 与 suggest 联想请求路径共用
pub(crate) fn percent_encode(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for b in query.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 搜索url_google与bing() {
        assert_eq!(
            SearchEngine::Google.search_url("rust async book"),
            "https://www.google.com/search?q=rust%20async%20book"
        );
        assert_eq!(
            SearchEngine::Bing.search_url("rust async book"),
            "https://www.bing.com/search?q=rust%20async%20book"
        );
    }

    #[test]
    fn 搜索url_非ascii按utf8字节编码() {
        // 空 → %20；铁/锈/战/争 各 3 字节
        assert_eq!(
            SearchEngine::Bing.search_url("铁锈 战争"),
            "https://www.bing.com/search?q=%E9%93%81%E9%94%88%20%E6%88%98%E4%BA%89"
        );
    }

    #[test]
    fn 首页url_搜索引擎官网() {
        assert_eq!(SearchEngine::Google.home_url(), "https://www.google.com/");
        assert_eq!(SearchEngine::Bing.home_url(), "https://www.bing.com/");
    }

    #[test]
    fn 设置读写往返() {
        let dir = std::env::temp_dir().join("quirel-test-browser-settings");
        std::fs::create_dir_all(&dir).unwrap();
        let s = BrowserSettings { search_engine: SearchEngine::Bing, incognito: true };
        save(&dir, &s).unwrap();
        assert_eq!(load(&dir), s);
        // 清理测试残留
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 设置读取_文件缺失回退默认() {
        assert_eq!(
            load(Path::new("Z:/quirel-nonexistent-dir")),
            BrowserSettings::default()
        );
    }
}
