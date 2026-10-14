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
