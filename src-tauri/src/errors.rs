use serde::Serialize;
use std::collections::HashMap;

/// 用户可见错误的错误码常量（snake_case）。
/// 约束：新增只能追加；code 语义一经发布不可变更（前端词典按 code 查表）。
pub mod codes {
    pub const CONNECTION_NOT_FOUND: &str = "connection_not_found";
    pub const PERMISSION_DENIED: &str = "permission_denied";
    pub const FRAME_LENGTH_INVALID: &str = "frame_length_invalid";
    pub const FRAME_WRITE_FAILED: &str = "frame_write_failed";
    pub const FILE_NOT_FOUND: &str = "file_not_found";
    pub const IO_FAILED: &str = "io_failed";
    pub const AUTH_FAILED: &str = "auth_failed";
    pub const OPERATION_FAILED: &str = "operation_failed"; // 兜底

    // ── 传输（transfer.rs）──
    // 用户视角文案由前端词典 errors.op.* 按 code 渲染；「三不暴露」约定：
    // 不暴露协议名词（stream/帧/QUIC）、不暴露库错误原文、不暴露内部机制，
    // 技术细节只进 detail / tracing 日志。
    pub const TRANSFER_TASK_NOT_FOUND: &str = "transfer_task_not_found";
    pub const TRANSFER_INVALID_STATE: &str = "transfer_invalid_state";
    pub const TRANSFER_SERVER_NOT_CONNECTED: &str = "transfer_server_not_connected";
    pub const TRANSFER_QUIC_UNAVAILABLE: &str = "transfer_quic_unavailable";
    pub const TRANSFER_INVALID_DIRECTION: &str = "transfer_invalid_direction";
    pub const TRANSFER_NETWORK_FAILED: &str = "transfer_network_failed";
    pub const TRANSFER_TIMEOUT: &str = "transfer_timeout";
    pub const TRANSFER_LOCAL_FILE_FAILED: &str = "transfer_local_file_failed";
    pub const TRANSFER_SAVE_FAILED: &str = "transfer_save_failed";
    pub const TRANSFER_UPLOAD_INCOMPLETE: &str = "transfer_upload_incomplete";
    pub const TRANSFER_AGENT_REJECTED: &str = "transfer_agent_rejected";
    pub const TRANSFER_PROTOCOL_UNEXPECTED: &str = "transfer_protocol_unexpected";
    pub const TRANSFER_CANCELLED: &str = "transfer_cancelled";
    pub const TRANSFER_TASK_DELETED: &str = "transfer_task_deleted";
    pub const TRANSFER_INTERRUPTED: &str = "transfer_interrupted";

    // ── 通信链路（connection.rs）──
    pub const CONNECTION_LOST: &str = "connection_lost"; // 连接已关闭/断开/响应通道关闭
    pub const NETWORK_FAILED: &str = "network_failed"; // 收发失败（打开 stream、写长度/数据）
    pub const RESPONSE_TIMEOUT: &str = "response_timeout"; // 等待响应超时
    pub const AGENT_REJECTED: &str = "agent_rejected"; // Agent 返回错误（message 透传）
    pub const UNEXPECTED_RESPONSE: &str = "unexpected_response"; // 意外响应类型/解析失败/base64 解码失败
    pub const FILE_NOT_UTF8: &str = "file_not_utf8"; // 文本读取无法解码为 UTF-8
    pub const TERMINAL_SESSION_NOT_FOUND: &str = "terminal_session_not_found"; // 终端会话不存在或已关闭

    // ── 远程浏览代理（proxy/）──
    pub const PROXY_PORT_CONFLICT: &str = "proxy_port_conflict"; // 固定端口被其他服务器的浏览会话占用
    pub const PROXY_LISTEN_FAILED: &str = "proxy_listen_failed"; // 本地 SOCKS5 监听启动失败
    pub const PROXY_BROWSER_NOT_FOUND: &str = "proxy_browser_not_found"; // 自动探测未找到可用浏览器
    pub const PROXY_BROWSER_NOT_INSTALLED: &str = "proxy_browser_not_installed"; // 所选浏览器未安装
    pub const PROXY_BROWSER_PATH_INVALID: &str = "proxy_browser_path_invalid"; // 自定义浏览器路径无效
    pub const PROXY_BROWSER_SPAWN_FAILED: &str = "proxy_browser_spawn_failed"; // 浏览器进程启动失败或立即退出
    pub const PROXY_BROWSER_IN_USE: &str = "proxy_browser_in_use"; // 浏览器正被其他会话使用
    pub const PROXY_CLEANUP_FAILED: &str = "proxy_cleanup_failed"; // 残留浏览器进程清理失败
}

/// 结构化错误：invoke reject 时序列化为 { code, params?, detail? }。
/// - code：前端词典键（用户可见文案由此生成）
/// - params：用户需要的值（文件名/长度/端口），供前端插值
/// - detail：原始技术上下文（英文），仅日志排查用，前端不作为正文展示
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub params: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl AppError {
    pub fn new(code: &str) -> Self {
        Self { code: code.to_string(), params: HashMap::new(), detail: None }
    }

    pub fn with_param(mut self, key: &str, value: impl Into<String>) -> Self {
        self.params.insert(key.to_string(), value.into());
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.detail.as_deref().unwrap_or(""))
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 序列化结构为_code_params() {
        let err = AppError::new(codes::FRAME_LENGTH_INVALID)
            .with_param("length", "42")
            .with_detail("bad frame");
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(
            json,
            r#"{"code":"frame_length_invalid","params":{"length":"42"},"detail":"bad frame"}"#
        );
    }

    #[test]
    fn 空参数与空detail不序列化() {
        let err = AppError::new(codes::IO_FAILED);
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, r#"{"code":"io_failed"}"#);
    }

    #[test]
    fn display_便于日志排查() {
        let err = AppError::new(codes::IO_FAILED).with_detail("disk error");
        assert_eq!(err.to_string(), "[io_failed] disk error");
    }
}
