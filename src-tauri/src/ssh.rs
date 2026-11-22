//! SSH 客户端封装（russh）：面向 quireld 自动安装与服务器管理的最小能力面。
//!
//! 能力面（仅此四个入口，不暴露底层句柄）：
//! - [`connect`]：密码/私钥认证建连，返回会话句柄与观测到的 host key 指纹
//! - [`SshSession::exec`]：命令执行（支持 sudo -S 模板，密码经 stdin 写入后立即关闭）
//! - [`SshSession::upload_file`] / [`download_file`]：SFTP 单文件传输
//!
//! 安全约束（quirel-safety 红线）：
//! - 密码/passphrase 用 [`Zeroizing`] 缓冲，任何日志/错误 Display/子进程 argv 不得包含
//! - host key：预置期望指纹时在握手期（认证前）强制比对拒绝，防止 MITM 先截获
//!   凭据；未预置时仅记录指纹（TOFU），信任决策由 provision 层/用户确认后进行
//! - 远程命令执行完毕即关闭 stdin，避免命令继承终端等待输入导致挂起

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use russh::client::{self, Handle};
use russh::{Channel, ChannelMsg};
use russh_sftp::client::SftpSession;
use thiserror::Error;
use zeroize::Zeroizing;

/// 连接超时（TCP + 握手 + 认证整体上限）
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// 单命令执行超时
const EXEC_TIMEOUT: Duration = Duration::from_secs(120);
/// SFTP 单文件传输超时
const SFTP_TIMEOUT: Duration = Duration::from_secs(300);
/// run_channel 单流（stdout/stderr）累计输出上限：恶意服务器可在超时窗口内
/// 灌入海量数据耗尽客户端内存（2026-11-20 审计 B-02）；超限部分丢弃
const CHANNEL_OUTPUT_LIMIT: usize = 1024 * 1024;

/// 追加数据到输出缓冲，超出 [`CHANNEL_OUTPUT_LIMIT`] 的部分丢弃
/// （防恶意服务器灌爆客户端内存，2026-11-20 审计 B-02）
fn append_capped(buf: &mut Vec<u8>, data: &[u8]) {
    if buf.len() >= CHANNEL_OUTPUT_LIMIT {
        return;
    }
    let remaining = CHANNEL_OUTPUT_LIMIT - buf.len();
    let take = data.len().min(remaining);
    buf.extend_from_slice(&data[..take]);
}

#[derive(Debug, Error)]
pub enum SshError {
    #[error("连接失败: {0}")]
    Connect(String),
    #[error("认证失败: {0}")]
    Auth(String),
    #[error("命令执行失败: {0}")]
    Exec(String),
    #[error("文件传输失败: {0}")]
    Transfer(String),
    #[error("操作超时")]
    Timeout,
    #[error("SSH host key 与期望不一致，疑似中间人攻击，已在认证前中止连接")]
    HostKeyMismatch,
}

/// SSH 连接目标（host:port + 登录用户）
#[derive(Debug, Clone)]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub username: String,
}

/// 认证材料（与前端 AuthCredentials 对应；密码/私钥内容为敏感数据）
#[derive(Clone)]
pub enum SshAuthData {
    Password(Zeroizing<String>),
    Key {
        pem: Zeroizing<String>,
        passphrase: Option<Zeroizing<String>>,
    },
}

impl std::fmt::Debug for SshAuthData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 脱敏：任何 Debug 输出不得包含密码/私钥（红线）
        match self {
            Self::Password(_) => f.write_str("SshAuthData::Password(***)"),
            Self::Key { passphrase, .. } => f
                .debug_struct("SshAuthData::Key")
                .field("pem", &"***")
                .field("passphrase", &passphrase.as_ref().map(|_| "***"))
                .finish(),
        }
    }
}

/// sudo 执行模式（provision 层环境探测后归类）
#[derive(Clone, Debug)]
pub enum SudoMode {
    /// root 直登（无需 sudo）
    None,
    /// sudo 免密（NOPASSWD）
    Passwordless,
    /// sudo 需密码（密码经 stdin 注入）
    Password(Zeroizing<String>),
}

impl std::fmt::Display for SudoMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => f.write_str("none"),
            Self::Passwordless => f.write_str("passwordless"),
            Self::Password(_) => f.write_str("password"),
        }
    }
}

/// 命令执行结果（exit_code 缺失表示通道异常关闭）
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub exit_code: Option<u32>,
    pub stdout: String,
    pub stderr: String,
}

impl ExecResult {
    pub fn ok(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// 单文件上传描述
pub struct UploadSpec<'a> {
    pub local: &'a Path,
    pub remote_path: &'a str,
    /// 远端文件权限（如 0o755 / 0o600）
    pub mode: u32,
}

/// russh 客户端 Handler：记录观测到的 host key 指纹；预置期望指纹时在握手期
/// （认证前）强制比对，不匹配直接拒绝——密码/私钥不会发送给对端（B-01）。
/// 未预置时只记录不决断（TOFU，信任决策由 provision 层/用户确认后进行）。
struct FingerprintHandler {
    host_key_fingerprint: Arc<Mutex<Option<String>>>,
    /// 握手被指纹比对拒绝时置位（connect 据此返回 HostKeyMismatch）
    rejected: Arc<AtomicBool>,
    /// 期望指纹（OpenSSH SHA256:base64）；None = TOFU 只记录
    expected: Option<String>,
}

#[async_trait]
impl client::Handler for FingerprintHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        // OpenSSH 风格 SHA-256 指纹（SHA256:base64）
        let fp = server_public_key
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string();
        *self.host_key_fingerprint.lock().unwrap() = Some(fp.clone());
        if !verify_host_key(self.expected.as_deref(), &fp) {
            self.rejected.store(true, Ordering::Relaxed);
            // 握手期拒绝：认证流程不会开始，凭据无从泄露
            return Ok(false);
        }
        Ok(true)
    }
}

/// 握手期信任决断：Some(expected) 强制精确比对（认证前）；None 放行（TOFU）
fn verify_host_key(expected: Option<&str>, observed: &str) -> bool {
    match expected {
        None => true,
        Some(expected) => expected == observed,
    }
}

/// SSH 会话句柄（连接 + 认证完成）
pub struct SshSession {
    handle: Handle<FingerprintHandler>,
    /// 观测到的 host key 指纹（OpenSSH SHA256:base64 格式）
    host_key_fingerprint: String,
    username: String,
}

impl std::fmt::Debug for SshSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 脱敏：不打印 handle 内部状态
        f.debug_struct("SshSession")
            .field("username", &self.username)
            .field("host_key_fingerprint", &self.host_key_fingerprint)
            .finish()
    }
}

impl SshSession {
    pub fn host_key_fingerprint(&self) -> &str {
        &self.host_key_fingerprint
    }

    /// 建连 + 认证（整体受 CONNECT_TIMEOUT 约束）
    ///
    /// `expected_host_key` 为 Some 时在握手期（认证前）强制比对 host key 指纹，
    /// 不匹配返回 [`SshError::HostKeyMismatch`]——此刻认证从未发生，凭据不会
    /// 发送给对端；None 时仅记录观测指纹（TOFU，由调用方/用户确认）。
    pub async fn connect(
        target: &SshTarget,
        auth: &SshAuthData,
        expected_host_key: Option<&str>,
    ) -> Result<Self, SshError> {
        let fingerprints = Arc::new(Mutex::new(None));
        let rejected = Arc::new(AtomicBool::new(false));
        let handler = FingerprintHandler {
            host_key_fingerprint: fingerprints.clone(),
            rejected: rejected.clone(),
            expected: expected_host_key.map(str::to_string),
        };

        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(120)),
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            ..Default::default()
        });

        let addr = (target.host.as_str(), target.port);
        let mut handle = match tokio::time::timeout(CONNECT_TIMEOUT, client::connect(config, addr, handler)).await
        {
            Ok(Ok(handle)) => handle,
            Ok(Err(e)) => {
                // 握手期指纹比对拒绝（check_server_key 返回 false）→ 专用错误，
                // 与普通连接失败区分，供上层给出"疑似中间人"提示
                if rejected.load(Ordering::Relaxed) {
                    return Err(SshError::HostKeyMismatch);
                }
                return Err(SshError::Connect(e.to_string()));
            }
            Err(_) => return Err(SshError::Timeout),
        };

        match auth {
            SshAuthData::Password(password) => {
                let ok = tokio::time::timeout(
                    CONNECT_TIMEOUT,
                    handle.authenticate_password(&target.username, password.as_str()),
                )
                .await
                .map_err(|_| SshError::Timeout)?
                .map_err(|e| SshError::Auth(e.to_string()))?;
                if !ok {
                    return Err(SshError::Auth("用户名或密码错误".to_string()));
                }
            }
            SshAuthData::Key { pem, passphrase } => {
                let key = russh::keys::decode_secret_key(pem.as_str(), passphrase.as_ref().map(|p| p.as_str()))
                    .map_err(|e| SshError::Auth(format!("私钥解析失败: {e}")))?;
                // PrivateKeyWithHashAlg 位于 keys::key 子模块（russh-keys 未顶层导出）
                let with_hash = russh::keys::key::PrivateKeyWithHashAlg::new(Arc::new(key), None)
                    .map_err(|e| SshError::Auth(format!("私钥构造失败: {e}")))?;
                let ok = tokio::time::timeout(
                    CONNECT_TIMEOUT,
                    handle.authenticate_publickey(&target.username, with_hash),
                )
                .await
                .map_err(|_| SshError::Timeout)?
                .map_err(|e| SshError::Auth(e.to_string()))?;
                if !ok {
                    return Err(SshError::Auth("公钥认证被拒绝".to_string()));
                }
            }
        }

        let host_key_fingerprint = fingerprints
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| SshError::Connect("未获取到服务器 host key".to_string()))?;

        Ok(Self { handle, host_key_fingerprint, username: target.username.clone() })
    }

    /// 执行命令（可选 sudo 模式；sudo -S 密码经 stdin 写入后立即关闭写端）
    ///
    /// 非 sudo 与免密 sudo：直接执行，stdout/stderr 分离收集。
    /// sudo -S：固定模板 `sudo -S -p '' -k bash -c <cmd>`，密码写入 stdin 后 shutdown，
    /// 防止远端命令继承等待输入的 stdin 导致会话挂起。
    pub async fn exec(&mut self, cmd: &str, sudo: &SudoMode, timeout: Duration) -> Result<ExecResult, SshError> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| SshError::Exec(e.to_string()))?;

        match sudo {
            SudoMode::None | SudoMode::Passwordless => {
                Self::run_channel(channel, cmd, None, timeout).await
            }
            SudoMode::Password(password) => {
                // shell 单引号包裹远端命令，避免内层引号/分号破坏 sudo bash -c 边界
                let quoted = format!("'{}'", cmd.replace('\'', r"'\''"));
                let sudo_cmd = format!("sudo -S -p '' -k bash -c {quoted}");
                let pw_line = format!("{}\n", password.as_str());
                Self::run_channel(channel, &sudo_cmd, Some(pw_line), timeout).await
            }
        }
    }

    /// 通道执行公共实现：exec → 可选 stdin 写入 → wait 循环收集输出/退出码
    async fn run_channel(
        mut channel: Channel<client::Msg>,
        cmd: &str,
        stdin_line: Option<String>,
        timeout: Duration,
    ) -> Result<ExecResult, SshError> {
        use tokio::io::AsyncWriteExt;

        channel
            .exec(false, cmd)
            .await
            .map_err(|e| SshError::Exec(e.to_string()))?;

        // sudo -S 密码注入：写入后立即关闭写端（红线：关闭标准输入流）
        if let Some(line) = stdin_line {
            let mut writer = channel.make_writer();
            writer
                .write_all(line.as_bytes())
                .await
                .map_err(|e| SshError::Exec(format!("stdin 写入失败: {e}")))?;
            writer
                .shutdown()
                .await
                .map_err(|e| SshError::Exec(format!("stdin 关闭失败: {e}")))?;
        }

        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        let mut exit_code: Option<u32> = None;

        let wait_loop = async {
            loop {
                match channel.wait().await {
                Some(ChannelMsg::Data { ref data }) => append_capped(&mut stdout, data),
                Some(ChannelMsg::ExtendedData { ref data, .. }) => append_capped(&mut stderr, data),
                    Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status),
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) => break,
                    None => break,
                    _ => {}
                }
            }
        };

        tokio::time::timeout(timeout, wait_loop)
            .await
            .map_err(|_| SshError::Timeout)?;

        Ok(ExecResult {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
        })
    }

    /// SFTP 上传本地文件到远端路径，并设置权限位
    pub async fn upload_file(&mut self, spec: UploadSpec<'_>) -> Result<u64, SshError> {
        let data = tokio::fs::read(spec.local)
            .await
            .map_err(|e| SshError::Transfer(format!("读取本地文件失败: {e}")))?;
        let size = data.len() as u64;

        let upload = async {
            let channel = self
                .handle
                .channel_open_session()
                .await
                .map_err(|e| SshError::Transfer(e.to_string()))?;
            let sftp = SftpSession::new(channel.into_stream())
                .await
                .map_err(|e| SshError::Transfer(e.to_string()))?;

            let mut remote = sftp
                .create(spec.remote_path)
                .await
                .map_err(|e| SshError::Transfer(format!("打开远端文件失败: {e}")))?;
            use tokio::io::AsyncWriteExt;
            remote
                .write_all(&data)
                .await
                .map_err(|e| SshError::Transfer(format!("写入远端文件失败: {e}")))?;
            remote
                .flush()
                .await
                .map_err(|e| SshError::Transfer(format!("flush 失败: {e}")))?;
            drop(remote);

            // 权限位（二进制 0755 / 脚本 0755 / 配置 0600）
            let meta = russh_sftp::protocol::FileAttributes {
                size: None,
                uid: None,
                user: None,
                gid: None,
                group: None,
                permissions: Some(spec.mode),
                atime: None,
                mtime: None,
            };
            sftp.set_metadata(spec.remote_path, meta)
                .await
                .map_err(|e| SshError::Transfer(format!("设置权限失败: {e}")))?;

            sftp.close().await.map_err(|e| SshError::Transfer(e.to_string()))?;
            Ok::<u64, SshError>(size)
        };

        tokio::time::timeout(SFTP_TIMEOUT, upload)
            .await
            .map_err(|_| SshError::Timeout)?
    }

    /// SFTP 下载远端文件（size_limit 防护，超限拒绝——证书等小文件场景）
    pub async fn download_file(&mut self, remote_path: &str, size_limit: u64) -> Result<Vec<u8>, SshError> {
        let download = async {
            let channel = self
                .handle
                .channel_open_session()
                .await
                .map_err(|e| SshError::Transfer(e.to_string()))?;
            let sftp = SftpSession::new(channel.into_stream())
                .await
                .map_err(|e| SshError::Transfer(e.to_string()))?;

            let meta = sftp
                .metadata(remote_path)
                .await
                .map_err(|e| SshError::Transfer(format!("读取远端文件信息失败: {e}")))?;
            if let Some(size) = meta.size {
                if size > size_limit {
                    return Err(SshError::Transfer(format!(
                        "远端文件超过大小上限: {size} > {size_limit}"
                    )));
                }
            }

            use tokio::io::AsyncReadExt;
            let mut remote = sftp
                .open(remote_path)
                .await
                .map_err(|e| SshError::Transfer(format!("打开远端文件失败: {e}")))?;
            // 不信任服务器自报 meta.size（None/谎报时 read_to_end 无界读尽）：
            // 按真实读取字节强制 size_limit，take 上限读（2026-11-20 审计 B-02）
            let mut limited = remote.take(size_limit + 1);
            let mut buf = Vec::new();
            limited
                .read_to_end(&mut buf)
                .await
                .map_err(|e| SshError::Transfer(format!("读取远端文件失败: {e}")))?;
            if buf.len() as u64 > size_limit {
                return Err(SshError::Transfer(format!(
                    "远端文件超过大小上限: {} > {size_limit}",
                    buf.len()
                )));
            }

            sftp.close().await.map_err(|e| SshError::Transfer(e.to_string()))?;
            Ok::<Vec<u8>, SshError>(buf)
        };

        tokio::time::timeout(SFTP_TIMEOUT, download)
            .await
            .map_err(|_| SshError::Timeout)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // check_server_key 是 client::Handler trait 方法，测试中需引入 trait 才可调用
    use russh::client::Handler as _;

    /// 测试 fixture（fake，非真实主机公钥）：ed25519 公钥的 openssh base64 段
    const FAKE_ED25519_PUB_B64: &str =
        "AAAAC3NzaC1lZDI1NTE5AAAAID6IFhLelEEJAUF5g6h6CE3eNwDwlYIHBMmCDi2fZHda";

    fn fake_server_key() -> russh::keys::PublicKey {
        russh::keys::parse_public_key_base64(FAKE_ED25519_PUB_B64).expect("fixture 必须可解析")
    }

    fn observed_fp_of_fake() -> String {
        fake_server_key()
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string()
    }

    /// TOFU 语义：未预置期望指纹 → 放行并记录观测值（探测阶段）
    #[tokio::test]
    async fn check_server_key_tofu_records_and_accepts() {
        let store = Arc::new(Mutex::new(None));
        let mut handler = FingerprintHandler {
            host_key_fingerprint: store.clone(),
            rejected: Arc::new(AtomicBool::new(false)),
            expected: None,
        };
        assert!(handler.check_server_key(&fake_server_key()).await.unwrap());
        assert_eq!(
            store.lock().unwrap().as_deref(),
            Some(observed_fp_of_fake().as_str())
        );
    }

    /// B-01 回归：期望指纹一致 → 握手期放行
    #[tokio::test]
    async fn check_server_key_matching_expected_accepts() {
        let mut handler = FingerprintHandler {
            host_key_fingerprint: Arc::new(Mutex::new(None)),
            rejected: Arc::new(AtomicBool::new(false)),
            expected: Some(observed_fp_of_fake()),
        };
        assert!(handler.check_server_key(&fake_server_key()).await.unwrap());
    }

    /// B-01 复现：期望指纹不符 → 握手期拒绝并置位 rejected（认证不会发生，
    /// 修复前 check_server_key 恒放行、比对发生在认证之后）
    #[tokio::test]
    async fn check_server_key_mismatched_expected_rejects_before_auth() {
        let store = Arc::new(Mutex::new(None));
        let rejected = Arc::new(AtomicBool::new(false));
        let mut handler = FingerprintHandler {
            host_key_fingerprint: store.clone(),
            rejected: rejected.clone(),
            // 合法格式但必然不同的期望指纹
            expected: Some("SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string()),
        };
        assert!(!handler.check_server_key(&fake_server_key()).await.unwrap());
        assert!(rejected.load(Ordering::Relaxed));
        // 观测指纹仍被记录，供上层提示/展示用
        assert_eq!(
            store.lock().unwrap().as_deref(),
            Some(observed_fp_of_fake().as_str())
        );
    }

    /// 信任决断纯函数：None 放行（TOFU），Some 强制精确比对
    #[test]
    fn verify_host_key_decision_matrix() {
        assert!(verify_host_key(None, "SHA256:any"));
        assert!(verify_host_key(Some("A"), "A"));
        assert!(!verify_host_key(Some("A"), "B"));
    }

    /// B-02 回归：输出累计到上限后丢弃后续数据，缓冲永不超过上限
    #[test]
    fn append_capped_drops_excess_beyond_limit() {
        // 恰好填满
        let mut buf = Vec::new();
        append_capped(&mut buf, &vec![0u8; CHANNEL_OUTPUT_LIMIT]);
        assert_eq!(buf.len(), CHANNEL_OUTPUT_LIMIT);
        // 超限部分整体丢弃
        append_capped(&mut buf, &[1u8; 100]);
        assert_eq!(buf.len(), CHANNEL_OUTPUT_LIMIT);
        // 未超限时正常追加
        let mut small = Vec::new();
        append_capped(&mut small, &[1, 2, 3]);
        assert_eq!(small, vec![1, 2, 3]);
        // 剩余额度内截断
        let mut partial = Vec::new();
        append_capped(&mut partial, &[1u8; CHANNEL_OUTPUT_LIMIT - 2]);
        append_capped(&mut partial, &[9u8; 10]);
        assert_eq!(partial.len(), CHANNEL_OUTPUT_LIMIT);
        assert_eq!(partial[CHANNEL_OUTPUT_LIMIT - 2], 9);
    }
}
