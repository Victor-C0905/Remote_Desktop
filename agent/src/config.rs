use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
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
    #[serde(default)]
    pub worker: WorkerConfig,
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

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0".into(),
            quic_port: default_quic_port(),
            ws_port: default_ws_port(),
            cert_path: "./cert.pem".into(),
            key_path: "./key.pem".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub ssh: SshAuthConfig,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            ssh: SshAuthConfig::default(),
        }
    }
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

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            blocked_commands: default_blocked_commands(),
        }
    }
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

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_terminal_sessions: default_max_sessions(),
            max_file_transfer_mb: default_max_file_mb(),
            metrics_interval_secs: default_metrics_interval(),
            connection_idle_timeout_secs: default_connection_idle_timeout(),
        }
    }
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

impl Default for CollectorsConfig {
    fn default() -> Self {
        Self {
            metrics_interval_secs: default_metrics_interval(),
            file_changes_delay_ms: default_file_changes_delay(),
            process_scan_interval_secs: default_process_scan_interval(),
            service_status_interval_secs: default_service_status_interval(),
        }
    }
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkerConfig {
    /// Agent 二进制路径
    #[serde(default = "default_agent_binary")]
    pub agent_binary: String,

    /// IPC Socket 路径
    #[serde(default = "default_ipc_socket_path")]
    pub ipc_socket_path: String,

    /// 最大重启次数
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,

    /// IPC 请求 channel 容量（Manager 侧 dispatcher 的 mpsc channel 大小）
    /// 满时 send().await 会等待，提供背压；默认 128 足够
    #[serde(default = "default_ipc_channel_capacity")]
    pub ipc_channel_capacity: usize,
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

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            agent_binary: default_agent_binary(),
            ipc_socket_path: default_ipc_socket_path(),
            max_restarts: default_max_restarts(),
            ipc_channel_capacity: default_ipc_channel_capacity(),
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

// Worker 默认值
/// 默认 Agent 二进制路径：自动获取当前可执行文件的绝对路径
///
/// 设计原则：开箱即用，用户无需在配置文件中手动指定 worker.agent_binary。
/// 通过 std::env::current_exe() 获取当前进程的可执行文件路径，
/// Manager 用同一路径 + --worker 参数启动 Worker 子进程。
/// 这样无论部署到 /usr/local/bin/ 还是其他目录都能正确工作。
fn default_agent_binary() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "./agent".into())
}
fn default_ipc_socket_path() -> String { "/tmp/gnome-remote-worker.sock".into() }
fn default_max_restarts() -> u32 { 3 }
fn default_ipc_channel_capacity() -> usize { 128 }

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
        worker: WorkerConfig::default(),
    }
}