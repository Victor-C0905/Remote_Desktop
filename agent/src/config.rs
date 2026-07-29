use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub security: SecurityConfig,
    pub limits: LimitsConfig,
    pub collectors: CollectorsConfig, // 新增
    #[serde(default)]
    pub audit: AuditConfig,
    #[serde(default)]
    pub log: LogConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub bind: String,
    #[serde(default = "default_quic_port")]
    pub quic_port: u16,
    #[serde(default = "default_ws_port")]
    pub ws_port: u16,
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub ssh: SshAuthConfig,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SshAuthConfig {
    #[serde(default = "default_enable_pubkey")]
    pub enable_pubkey: bool,

    #[serde(default = "default_enable_password")]
    pub enable_password: bool,

    #[serde(default = "default_pam_service")]
    pub pam_service: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityConfig {
    // 移除：pub allowed_paths: Vec<String>,
    #[serde(default = "default_blocked_commands")]
    pub blocked_commands: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LimitsConfig {
    #[serde(default = "default_max_sessions")]
    pub max_terminal_sessions: usize,
    #[serde(default = "default_max_file_mb")]
    pub max_file_transfer_mb: u64,
    #[serde(default = "default_metrics_interval")]
    pub metrics_interval_secs: u64,
    /// 连接空闲超时（秒）：在此时间内无任何 Stream 活动的连接将被强制关闭
    /// 默认 300 秒（5 分钟）
    #[serde(default = "default_connection_idle_timeout")]
    pub connection_idle_timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CollectorsConfig {
    #[serde(default = "default_metrics_interval")]
    pub metrics_interval_secs: u64,

    #[serde(default = "default_file_changes_delay")]
    pub file_changes_delay_ms: u64,

    #[serde(default = "default_process_scan_interval")]
    pub process_scan_interval_secs: u64,

    #[serde(default = "default_service_status_interval")]
    pub service_status_interval_secs: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuditConfig {
    #[serde(default = "default_audit_log_path")]
    pub log_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LogConfig {
    /// 日志级别: trace, debug, info, warn, error
    #[serde(default = "default_log_level")]
    pub level: String,
    /// 日志输出目录
    #[serde(default = "default_log_dir")]
    pub dir: String,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            log_path: default_audit_log_path(),
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            dir: default_log_dir(),
        }
    }
}

fn default_quic_port() -> u16 { 8443 }
fn default_ws_port() -> u16 { 443 }
// 移除：fn default_allowed_paths() -> Vec<String> { vec!["/home".into(), "/etc".into(), "/var/log".into(), "/opt".into()] }
fn default_blocked_commands() -> Vec<String> { vec!["rm -rf /".into(), "dd if=".into(), "mkfs.".into()] }
fn default_max_sessions() -> usize { 10 }
fn default_max_file_mb() -> u64 { 500 }
fn default_metrics_interval() -> u64 { 2 }
fn default_connection_idle_timeout() -> u64 { 300 }  // 5 分钟
fn default_file_changes_delay() -> u64 { 100 }
fn default_process_scan_interval() -> u64 { 2 }
fn default_service_status_interval() -> u64 { 5 }
fn default_audit_log_path() -> String { "/var/log/gnome-remote/audit.log".into() }
fn default_log_level() -> String { "info".into() }
fn default_log_dir() -> String { "logs".into() }

// SSH 认证默认值
fn default_enable_pubkey() -> bool { true }
fn default_enable_password() -> bool { true }
fn default_pam_service() -> String { "sshd".into() }

pub fn load(path: &str) -> Result<AgentConfig, anyhow::Error> {
    let p = Path::new(path);
    if !p.exists() {
        tracing::info!("配置文件 {} 不存在，生成默认配置", path);
        let cfg = default_config();
        // 写入默认配置文件，方便用户查看和修改
        if let Ok(content) = toml::to_string_pretty(&cfg) {
            if let Err(e) = fs::write(p, &content) {
                tracing::warn!("写入默认配置文件失败: {}", e);
            } else {
                tracing::info!("默认配置已写入: {}", path);
            }
        }
        return Ok(cfg);
    }
    let content = fs::read_to_string(p)?;
    let cfg: AgentConfig = toml::from_str(&content)?;

    Ok(cfg)
}

fn default_config() -> AgentConfig {
    AgentConfig {
        server: ServerConfig {
            bind: "0.0.0.0".into(),
            quic_port: 8443,
            ws_port: 443,
            cert_path: "./cert.pem".into(),
            key_path: "./key.pem".into(),
        },
        auth: AuthConfig {
            ssh: SshAuthConfig {
                enable_pubkey: true,
                enable_password: true,
                pam_service: "sshd".into(),
            },
        },
        security: SecurityConfig {
            // 移除：allowed_paths: default_allowed_paths(),
            blocked_commands: default_blocked_commands(),
        },
        limits: LimitsConfig {
            max_terminal_sessions: 10,
            max_file_transfer_mb: 500,
            metrics_interval_secs: 2,
            connection_idle_timeout_secs: default_connection_idle_timeout(),
        },
        collectors: CollectorsConfig {
            metrics_interval_secs: 2,
            file_changes_delay_ms: 100,
            process_scan_interval_secs: 2,
            service_status_interval_secs: 5,
        },
        audit: AuditConfig::default(),
        log: LogConfig::default(),
    }
}