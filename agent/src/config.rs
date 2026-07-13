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
    pub token: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityConfig {
    #[serde(default = "default_allowed_paths")]
    pub allowed_paths: Vec<String>,
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

fn default_quic_port() -> u16 { 8443 }
fn default_ws_port() -> u16 { 443 }
fn default_allowed_paths() -> Vec<String> { vec!["/home".into(), "/etc".into(), "/var/log".into(), "/opt".into()] }
fn default_blocked_commands() -> Vec<String> { vec!["rm -rf /".into(), "dd if=".into(), "mkfs.".into()] }
fn default_max_sessions() -> usize { 10 }
fn default_max_file_mb() -> u64 { 500 }
fn default_metrics_interval() -> u64 { 2 }
fn default_file_changes_delay() -> u64 { 100 }
fn default_process_scan_interval() -> u64 { 2 }
fn default_service_status_interval() -> u64 { 5 }

pub fn load(path: &str) -> Result<AgentConfig, anyhow::Error> {
    let p = Path::new(path);
    if !p.exists() {
        tracing::info!("配置文件 {} 不存在，使用默认配置", path);
        return Ok(default_config());
    }
    let content = fs::read_to_string(p)?;
    let cfg: AgentConfig = toml::from_str(&content)?;

    if cfg.auth.token.is_empty() {
        let auto_token = format!("gmr_{}", uuid::Uuid::new_v4());
        tracing::info!("🔑 自动生成 Token (请保存): {}", auto_token);

        let mut cfg_with_token = cfg;
        cfg_with_token.auth.token = auto_token.clone();

        if let Ok(updated_toml) = toml::to_string_pretty(&cfg_with_token) {
            let updated_toml = format!("# GNOME Remote Agent 配置\n# Token 已自动生成，请妥善保管\n\n{}", updated_toml);
            let _ = fs::write(p, updated_toml);
            tracing::info!("   Token 已写入 {}", path);
        }

        return Ok(cfg_with_token);
    }

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
        auth: AuthConfig { token: String::new() },
        security: SecurityConfig {
            allowed_paths: default_allowed_paths(),
            blocked_commands: default_blocked_commands(),
        },
        limits: LimitsConfig {
            max_terminal_sessions: 10,
            max_file_transfer_mb: 500,
            metrics_interval_secs: 2,
        },
        collectors: CollectorsConfig {
            metrics_interval_secs: 2,
            file_changes_delay_ms: 100,
            process_scan_interval_secs: 2,
            service_status_interval_secs: 5,
        },
    }
}