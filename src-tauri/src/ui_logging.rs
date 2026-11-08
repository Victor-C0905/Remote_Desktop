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
///
/// async：同步 command 会在主线程执行，而热路径（如窗口拖动时逐帧的 bounds
/// 上报日志）每帧都可能触发本命令，必须挪到异步运行时，避免与合成/布局争抢。
#[tauri::command]
pub async fn log_write(level: String, module: String, message: String, data: Vec<String>) {
    // 附加数据拼为单行（前端已做截断与序列化）
    let extra = if data.is_empty() {
        String::new()
    } else {
        format!(" {}", data.join(" "))
    };

    // 统一 `ui:` 前缀（审计 低-8）：前端任意字符串直接进 client.log 可
    // 伪造形如 Rust 日志的条目（renderer 被攻破时的纵深防御）——所有 UI
    // 日志带显式来源标记，排查时可区分「来自前端桥接」与「Rust 原生」
    match level.as_str() {
        "error" => tracing::error!(target: "quirel_lib::ui", "ui: [{module}] {message}{extra}"),
        "warn" => tracing::warn!(target: "quirel_lib::ui", "ui: [{module}] {message}{extra}"),
        // 未知级别按 info 兜底，避免静默丢日志
        _ => tracing::info!(target: "quirel_lib::ui", "ui: [{module}] {message}{extra}"),
    }
}

/// 返回客户端版本号（编译期常量，来源 Cargo.toml；debug 构建追加 -dev 后缀）
#[tauri::command]
pub fn get_app_version() -> &'static str {
    crate::APP_VERSION
}

/// 返回构建标识（编译期注入：分支@git describe，见 build.rs 与 AGENTS.md §5）
///
/// 与版本号双轨：版本号仅在 release 时变，构建标识区分每次开发/测试构建。
#[tauri::command]
pub fn get_build_info() -> &'static str {
    env!("QUIREL_BUILD_INFO")
}
