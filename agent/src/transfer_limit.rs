//! 上传大小限制：全局热值 + quireld.toml 写回
//!
//! 值的单一事实来源在 Agent 端：
//! - 内存热值（AtomicU64）：修改后立即对当前进程所有会话生效（含其他已登录用户）
//! - 配置文件持久化（toml_edit 保留注释）：重启后从配置恢复
//! 写回失败时热值不回滚（persisted=false，本次进程生命周期内仍生效）。

use std::sync::atomic::{AtomicU64, Ordering};

use crate::protocol::Payload;

lazy_static::lazy_static! {
    /// Agent 自身配置文件路径（启动时由 main 注入，写回用；测试可注入临时路径）
    static ref CONFIG_PATH: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
}

/// 配置写回临时文件自增计数（与 pid 组合保证同进程并发写回不互踩）
static TMP_WRITE_SEQ: AtomicU64 = AtomicU64::new(0);

/// 全局热值：当前进程的上传大小限制（MB）。未 init 时为默认值 500
static MAX_FILE_TRANSFER_MB: AtomicU64 = AtomicU64::new(500);

/// 值域：1 MB – 100 GB
pub const MIN_LIMIT_MB: u64 = 1;
pub const MAX_LIMIT_MB: u64 = 102400;

/// Agent 能力声明（认证响应上报给客户端）。
/// 客户端凭此决定是否可用新协议命令——避免向旧 Agent 发送其无法解码的 payload
/// （旧 Agent 解码失败会断流，客户端主循环会把整条连接误判为断开并拆除）。
/// 新增协议功能时在此追加。
pub const AGENT_CAPABILITIES: &[&str] = &["transfer_limit"];

/// 修改结果：新值 + 是否已持久化到配置文件
#[derive(Debug, PartialEq)]
pub struct SetOutcome {
    pub max_file_transfer_mb: u64,
    pub persisted: bool,
}

#[derive(Debug, PartialEq)]
pub enum TransferLimitError {
    /// 非 root 会话（沿用 HTTP 语义 403）
    NotRoot,
    /// 超出值域（沿用 HTTP 语义 400）
    InvalidValue(u64),
}

/// 错误 → 协议 Error 响应（纯函数：与 handler 解耦，映射可单测）。
/// 错误码语义发布后不可变更：403=权限、400=参数；文案为用户视角（「三不暴露」）。
pub fn limit_error_payload(e: &TransferLimitError) -> Payload {
    match e {
        TransferLimitError::NotRoot => Payload::Error {
            code: 403,
            message: "权限不足：需要以 root 用户连接才能修改".to_string(),
        },
        TransferLimitError::InvalidValue(v) => Payload::Error {
            code: 400,
            message: format!(
                "上传大小限制需在 {}–{} MB 之间（当前输入: {}）",
                MIN_LIMIT_MB,
                MAX_LIMIT_MB,
                v
            ),
        },
    }
}

/// 启动时初始化（main 在 config::load 之后调用）
pub fn init(max_mb: u64, config_path: &str) {
    MAX_FILE_TRANSFER_MB.store(max_mb, Ordering::SeqCst);
    *CONFIG_PATH.lock().unwrap() = Some(config_path.to_string());
}

/// 当前限制（MB）——上传大小检查读此热值
pub fn current_mb() -> u64 {
    MAX_FILE_TRANSFER_MB.load(Ordering::SeqCst)
}

/// 修改限制（仅 root）。root 校验先于值域校验（非 root 的非法值也报 403）。
/// 先更新热值（立即全局生效），再写回配置文件；写回失败不回滚热值。
pub fn set_limit(uid: u32, new_mb: u64) -> Result<SetOutcome, TransferLimitError> {
    if uid != 0 {
        return Err(TransferLimitError::NotRoot);
    }
    if !(MIN_LIMIT_MB..=MAX_LIMIT_MB).contains(&new_mb) {
        return Err(TransferLimitError::InvalidValue(new_mb));
    }
    // 先热生效：对该进程所有会话（含其他已登录用户）立即生效
    MAX_FILE_TRANSFER_MB.store(new_mb, Ordering::SeqCst);
    let persisted = match persist_to_config(new_mb) {
        Ok(()) => true,
        Err(e) => {
            // 热值不回滚：本次进程生命周期内已生效；重启后回到配置文件旧值
            tracing::warn!(error = %e, new_mb, "上传大小限制写回配置文件失败，重启后恢复原值");
            false
        }
    };
    Ok(SetOutcome { max_file_transfer_mb: new_mb, persisted })
}

/// 写回配置文件（读取启动时注入的路径）
fn persist_to_config(new_mb: u64) -> Result<(), String> {
    let path = CONFIG_PATH
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "配置文件路径未初始化".to_string())?;
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取配置文件失败: {}", e))?;
    let new_content = apply_limit_to_toml(&content, new_mb)?;
    // 原子写：先写同目录临时文件再 rename 替换——直接 truncate 覆盖在写中途
    // 崩溃/断电会截断配置文件，导致 Agent 下次无法启动；tmp 名带 pid+自增计数防并发写互踩
    let tmp_path = format!(
        "{}.{}-{}.tmp",
        path,
        std::process::id(),
        TMP_WRITE_SEQ.fetch_add(1, Ordering::Relaxed)
    );
    std::fs::write(&tmp_path, new_content)
        .map_err(|e| format!("写入配置文件失败: {}", e))?;
    if let Err(e) = std::fs::rename(&tmp_path, &path) {
        // rename 失败：尽力清理 tmp（清理失败也只 warn），返回 Err 由调用方决定热值语义
        if let Err(remove_err) = std::fs::remove_file(&tmp_path) {
            tracing::warn!("清理配置写回临时文件失败: {}", remove_err);
        }
        return Err(format!("写入配置文件失败: {}", e));
    }
    Ok(())
}

/// 纯函数：把新限制写入 toml 文本（toml_edit 精准改值，保留注释与格式）。
/// 与配置业务解耦，便于单测。
fn apply_limit_to_toml(content: &str, new_mb: u64) -> Result<String, String> {
    let mut doc = content
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("配置文件解析失败: {}", e))?;
    if !doc.contains_key("limits") {
        doc["limits"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    // toml_edit 整数内部为 i64（无 From<u64>）；值域 ≤102400，as i64 无损
    doc["limits"]["max_file_transfer_mb"] = toml_edit::value(new_mb as i64);
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_limit_preserves_comments_and_other_keys() {
        // toml_edit 核心价值：精准改值，段落注释与其他配置项原样保留
        let content = r#"# 服务配置
[server]
bind = "0.0.0.0"

# 传输限制
[limits]
max_terminal_sessions = 10
max_file_transfer_mb = 500
metrics_interval_secs = 2
"#;
        let out = apply_limit_to_toml(content, 2048).unwrap();
        assert!(out.contains("# 服务配置"), "段前注释必须保留");
        assert!(out.contains("# 传输限制"));
        assert!(out.contains("max_terminal_sessions = 10"));
        assert!(out.contains("metrics_interval_secs = 2"));
        assert!(out.contains("max_file_transfer_mb = 2048"));
        assert!(!out.contains("max_file_transfer_mb = 500"));
    }

    #[test]
    fn apply_limit_creates_missing_limits_section() {
        // 防御：老配置缺 [limits] 段落时补建（默认配置必有该段落）
        let out = apply_limit_to_toml("[server]\nbind = \"0.0.0.0\"\n", 100).unwrap();
        assert!(out.contains("[limits]"));
        assert!(out.contains("max_file_transfer_mb = 100"));
    }

    #[test]
    fn apply_limit_rejects_invalid_toml() {
        assert!(apply_limit_to_toml("not [ valid toml", 1).is_err());
    }

    #[test]
    fn set_limit_scenarios() {
        // 全局静态是进程级共享的：set_limit 相关场景合并为单个测试函数顺序执行，
        // 避免并行测试互相干扰热值
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("quireld.toml");
        let cfg_path_str = cfg_path.to_str().unwrap().to_string();
        std::fs::write(&cfg_path, "[limits]\nmax_file_transfer_mb = 500\n").unwrap();

        // 1) init：启动时从配置初始化热值与写回路径
        init(500, &cfg_path_str);
        assert_eq!(current_mb(), 500);

        // 2) root 修改成功：热值更新 + 写回成功
        let out = set_limit(0, 2048).unwrap();
        assert!(out.persisted);
        assert_eq!(out.max_file_transfer_mb, 2048);
        assert_eq!(current_mb(), 2048);
        assert!(std::fs::read_to_string(&cfg_path).unwrap().contains("max_file_transfer_mb = 2048"));

        // 3) 非 root 拒绝（403 语义）：热值不变
        assert_eq!(set_limit(1000, 512), Err(TransferLimitError::NotRoot));
        assert_eq!(current_mb(), 2048);

        // 4) 值域拒绝（400 语义）：0 与 102401 均非法，热值不变
        assert_eq!(set_limit(0, 0), Err(TransferLimitError::InvalidValue(0)));
        assert_eq!(set_limit(0, 102401), Err(TransferLimitError::InvalidValue(102401)));
        assert_eq!(current_mb(), 2048);

        // 5) 写回失败：热值不回滚，persisted=false（重启后回到配置旧值）
        init(500, "/nonexistent-quireld-test-dir/quireld.toml");
        let out = set_limit(0, 4096).unwrap();
        assert!(!out.persisted);
        assert_eq!(current_mb(), 4096);

        // 收尾恢复：不把热值 4096 与失效写回路径残留给后续测试（tempdir 仍存活，路径有效）
        init(500, &cfg_path_str);
    }

    #[test]
    fn limit_error_payload_maps_codes_and_user_text() {
        // 错误码语义发布后不可变更（403=权限、400=参数）；
        // 文案须为用户视角（「三不暴露」：不说 uid/session/内部机制）
        match limit_error_payload(&TransferLimitError::NotRoot) {
            Payload::Error { code, message } => {
                assert_eq!(code, 403);
                assert_eq!(message, "权限不足：需要以 root 用户连接才能修改");
            }
            _ => panic!("NotRoot 应映射为 Error payload"),
        }
        match limit_error_payload(&TransferLimitError::InvalidValue(0)) {
            Payload::Error { code, message } => {
                assert_eq!(code, 400);
                assert!(message.contains("1–102400"), "文案需包含值域: {}", message);
                assert!(message.contains("当前输入: 0"));
            }
            _ => panic!("InvalidValue 应映射为 Error payload"),
        }
    }
}
