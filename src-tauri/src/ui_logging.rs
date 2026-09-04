//! 前端日志桥接与应用版本信息
//!
//! - `log_write`：前端 logger 的落盘通道，把 UI 层日志汇入 tracing（client.log）
//! - `get_app_version`：返回编译期版本号（Cargo.toml 单一来源）
//!
//! 注：日志 target 统一为 `quirel_lib::ui`，Rust 侧 EnvFilter
//! `quirel_lib=info` 已覆盖，info 及以上级别可落盘。

/// 前端日志落盘通道
///
/// 前端 logger.ts 对 info/warn/error 调用本命令（fire-and-forget），
/// 使 UI 层问题与 Rust 层日志在同一份 client.log 中按时间线交错，便于排查。
#[tauri::command]
pub fn log_write(level: String, module: String, message: String, data: Vec<String>) {
    // 附加数据拼为单行（前端已做截断与序列化）
    let extra = if data.is_empty() {
        String::new()
    } else {
        format!(" {}", data.join(" "))
    };

    match level.as_str() {
        "error" => tracing::error!(target: "quirel_lib::ui", "[{module}] {message}{extra}"),
        "warn" => tracing::warn!(target: "quirel_lib::ui", "[{module}] {message}{extra}"),
        // 未知级别按 info 兜底，避免静默丢日志
        _ => tracing::info!(target: "quirel_lib::ui", "[{module}] {message}{extra}"),
    }
}

/// 返回客户端版本号（编译期常量，来源 Cargo.toml）
#[tauri::command]
pub fn get_app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
