//! 客户端驱动的 quireld 更新管理器
//!
//! 职责：staging 收纳（协议分块上传落盘）→ 校验（sha256/ELF 架构/版本探测）→
//! commit（备份 → 原子替换 → 触发热更新 → 确认）→ 失败自动回滚。
//!
//! 状态来源（三层版本链，暴露"Worker 新 / Manager 旧"半更新状态）：
//! - manager_version：本进程 env!("CARGO_PKG_VERSION")（编译期注入，不可伪造）
//! - committed_version：staging 目录 state.json 记录的最近成功 commit 版本
//!   （热更新后 Worker 即为该版本；Manager 仍为旧版本直到 systemctl restart）
//! - staged：staged.json 描述暂存区中的新二进制（等待 commit）
//!
//! 安全约束：
//! - staging 目录 0700、文件 0600（写入后显式设置）
//! - 大小上限（max_binary_mb）防磁盘配额耗尽
//! - ELF e_machine 与本机架构双重校验（uname 探测可能在安装阶段被篡改）
//! - commit 前强制备份，失败自动恢复；备份保留 keep_backups 份

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use crate::config::UpdatesConfig;
use crate::manager::signal_handler::ReloadTrigger;
use crate::protocol::{Payload, StagedUpdate, AgentUpdateLastResult};

/// Manager 进程版本（QUIC 层 / Manager 自身）
pub const MANAGER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// staging 内文件名约定
const STAGED_BINARY: &str = "quireld.new";
const STAGED_META: &str = "staged.json";
const COMMITTED_STATE: &str = "state.json";
/// 备份目录（与 update.sh/install.sh 惯例一致）
const BACKUP_DIR: &str = "/var/backups/quireld";
/// commit 后等待热更新完成的上限
const RELOAD_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// commit 后崩溃观察窗（10 分钟）：窗内 Worker 反复崩溃视为新二进制不可用
const OBSERVATION_WINDOW_SECS: u64 = 600;
/// 观察窗内允许的自动重启次数上限（第 3 次崩溃触发自动回滚到 commit 前备份）
const OBSERVE_MAX_CRASHES: u32 = 2;

/// staging 元数据（staged.json 持久化，Manager 重启后恢复）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StagedMeta {
    sha256: String,
    size: u64,
    version: String,
    staged_at: u64,
}

/// committed 状态（state.json 持久化：最近一次成功 commit 的版本）
/// backup/crashes 为崩溃观察窗扩展字段，旧版本落盘文件缺省兼容
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CommittedState {
    version: String,
    committed_at: u64,
    /// commit 时创建的备份路径（崩溃观察窗自动回滚的恢复源）
    #[serde(default)]
    backup: Option<String>,
    /// 观察窗内累计的 Worker 崩溃次数
    #[serde(default)]
    crashes: u32,
}

/// 更新管理器
pub struct UpdateManager {
    cfg: UpdatesConfig,
    /// 热更新触发通道（Manager::start 注入；None 时 commit 直接失败——未完成初始化）
    reload_tx: std::sync::Mutex<Option<mpsc::Sender<ReloadTrigger>>>,
    /// 热更新进行中标志（与 HotUpdateCoordinator 共享同一 AtomicBool）
    is_reloading: std::sync::Mutex<Option<Arc<AtomicBool>>>,
}

/// 更新命令授权拒绝时的统一错误（客户端直接展示；不含内部细节）
pub const UPDATE_REQUIRES_ROOT: &str = "仅 root 用户可执行 quireld 更新（请使用 root 账户登录）";

impl UpdateManager {
    /// 创建更新管理器（staging 状态从磁盘恢复）
    pub fn new(cfg: &UpdatesConfig) -> Result<Self, anyhow::Error> {
        let mgr = Self {
            cfg: cfg.clone(),
            reload_tx: std::sync::Mutex::new(None),
            is_reloading: std::sync::Mutex::new(None),
        };
        // staging 根目录的父目录必须已存在（/var/lib/quireld 由 install.sh 创建）；
        // 仅创建末级目录，不自动生成整条目录树（红线：禁止 create_dir_all 盲目建树）
        let root = Path::new(&cfg.staging_dir);
        if let Some(parent) = root.parent() {
            if !parent.exists() {
                anyhow::bail!("更新目录的父目录不存在: {}", parent.display());
            }
        }
        fs::create_dir_all(root)
            .map_err(|e| anyhow::anyhow!("创建更新目录失败: {e}"))?;
        set_dir_permissions(root, 0o700)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(mgr)
    }

    /// 注入热更新通道与进行中标志（Manager::start 时调用）
    pub fn set_reload_channel(&self, tx: mpsc::Sender<ReloadTrigger>, is_reloading: Arc<AtomicBool>) {
        *self.reload_tx.lock().unwrap() = Some(tx);
        *self.is_reloading.lock().unwrap() = Some(is_reloading);
    }

    /// 更新状态查询（直接构造协议 Payload::AgentUpdateStatusResponse）
    pub fn status(&self) -> Payload {
        let committed = self.read_committed();
        let worker_version = committed.as_ref().map(|c| c.version.clone());
        let manager_lags_worker = worker_version
            .as_deref()
            .map(|w| w != MANAGER_VERSION)
            .unwrap_or(false);
        Payload::AgentUpdateStatusResponse {
            manager_version: MANAGER_VERSION.to_string(),
            worker_version,
            manager_lags_worker,
            is_reloading: self.is_reloading_now(),
            enabled: self.cfg.enabled,
            staged: self.read_staged().map(|(meta, _)| meta_to_staged(&meta)),
            last_result: self.read_last_result(),
        }
    }

    fn is_reloading_now(&self) -> bool {
        self.is_reloading
            .lock().unwrap()
            .as_ref()
            .map(|f| f.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    // ── 上传收纳 ─────────────────────────────────────────

    /// 开始上传：校验请求（大小/格式）并清理旧 staging
    pub fn begin_upload(&self, file_size: u64) -> Result<PathBuf, String> {
        if !self.cfg.enabled {
            return Err("更新功能已在服务器配置中关闭".to_string());
        }
        let max_bytes = self.cfg.max_binary_mb.saturating_mul(1024 * 1024);
        if file_size == 0 || file_size > max_bytes {
            return Err(format!("文件大小超出允许范围（上限 {} MB）", self.cfg.max_binary_mb));
        }
        // 清理旧 staging（残留的半上传状态不覆盖新上传）
        self.clear_staging().map_err(|e| e.to_string())?;
        Ok(self.staging_path(STAGED_BINARY))
    }

    /// 写入一个分块（追加模式；调用方保证顺序）
    pub fn write_chunk(&self, path: &Path, data: &[u8]) -> Result<(), String> {
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("staging 写入失败: {e}"))?;
        f.write_all(data).map_err(|e| format!("staging 写入失败: {e}"))
    }

    /// 完成上传：大小复核 → sha256 → ELF 架构 → 版本探测 → 落 staged.json
    pub fn finish_upload(&self, declared_sha256: &str, declared_size: u64, version: &str) -> Result<StagedUpdate, String> {
        let binary = self.staging_path(STAGED_BINARY);
        let actual_size = fs::metadata(&binary).map_err(|e| format!("staging 文件缺失: {e}"))?.len();
        if actual_size != declared_size {
            self.clear_staging().ok();
            return Err(format!("文件大小不符: 声明 {declared_size} 实际 {actual_size}"));
        }
        let actual_sha = sha256_hex(&binary).map_err(|e| format!("哈希计算失败: {e}"))?;
        if actual_sha != declared_sha256.to_lowercase() {
            self.clear_staging().ok();
            return Err("SHA-256 校验失败，已丢弃暂存文件".to_string());
        }
        // ELF 架构校验（与 Manager 本机架构一致；直读文件头无子进程）
        verify_elf_arch(&binary).map_err(|e| {
            self.clear_staging().ok();
            e
        })?;
        // 版本探测（子进程 --version，1.5s 超时；失败拒绝——防非 quireld 文件混入）
        let probed = probe_version(&binary).map_err(|e| {
            self.clear_staging().ok();
            e
        })?;
        if probed != version {
            tracing::warn!("版本探测值与声明不符: 声明 {version} 探测 {probed}（以探测值为准）");
        }

        let meta = StagedMeta {
            sha256: actual_sha,
            size: actual_size,
            version: probed,
            staged_at: current_secs(),
        };
        let meta_path = self.staging_path(STAGED_META);
        let json = serde_json::to_string(&meta).map_err(|e| format!("元数据序列化失败: {e}"))?;
        fs::write(&meta_path, json).map_err(|e| format!("元数据写入失败: {e}"))?;
        set_file_permissions(&meta_path, 0o600)?;
        set_file_permissions(&binary, 0o700)?;
        Ok(meta_to_staged(&meta))
    }

    // ── commit / rollback ────────────────────────────────

    /// 授权拒绝响应（不改动 staging/备份，不写 last_result）
    pub fn denied(&self, message: &str, rolled_back: bool) -> Payload {
        let (manager_version, worker_version, lags) = self.version_triple();
        Payload::AgentUpdateActionResult {
            success: false,
            error: Some(message.to_string()),
            manager_version,
            worker_version,
            manager_lags_worker: lags,
            rolled_back,
        }
    }

    /// 提交更新：校验 → 备份 → 原子替换 → 触发热更新 → 确认
    /// 返回 Payload::AgentUpdateActionResult
    pub async fn commit(&self, expected_sha256: &str, operator: &str) -> Payload {
        let result = self.commit_inner(expected_sha256, operator).await;
        let (success, error, rolled_back) = match &result {
            Ok(()) => (true, None, false),
            Err(e) => (false, Some(e.clone()), false),
        };
        self.record_last_result("update", success, error.clone());
        let (manager_version, worker_version, lags) = self.version_triple();
        Payload::AgentUpdateActionResult {
            success,
            error,
            manager_version,
            worker_version,
            manager_lags_worker: lags,
            rolled_back,
        }
    }

    async fn commit_inner(&self, expected_sha256: &str, operator: &str) -> Result<(), String> {
        if !self.cfg.enabled {
            return Err("更新功能已在服务器配置中关闭".to_string());
        }
        let (meta, _) = self.read_staged().ok_or_else(|| "暂存区为空，请先上传新二进制".to_string())?;
        if meta.sha256 != expected_sha256.to_lowercase() {
            return Err("提交哈希与暂存区不一致（请重新上传）".to_string());
        }

        let current = std::env::current_exe()
            .map_err(|e| format!("定位当前二进制失败: {e}"))?
            .canonicalize()
            .map_err(|e| format!("解析当前二进制路径失败: {e}"))?;
        let staged = self.staging_path(STAGED_BINARY);

        // 1. 备份当前二进制（保留 keep_backups 份）
        let backup = self.backup_current(&current)?;

        // 2. 原子替换（ETXTBSY：运行中二进制先 rm(unlink) 再 copy）
        replace_binary(&staged, &current)?;

        // 3. 触发热更新（Worker 换新二进制；Manager 保持旧进程）
        let tx = self.reload_tx.lock().unwrap().clone()
            .ok_or_else(|| "热更新通道未就绪（Manager 尚未完成启动）".to_string())?;
        let send_result = tx
            .send(ReloadTrigger::ClientCommand { operator: operator.to_string() })
            .await;
        if send_result.is_err() {
            self.restore_from(&backup, &current).await?;
            return Err("热更新通道已关闭，已自动回滚".to_string());
        }

        // 4. 等待 reload 完成（is_reloading 回落 false）
        let deadline = tokio::time::Instant::now() + RELOAD_WAIT_TIMEOUT;
        loop {
            if !self.is_reloading_now() {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                // 超时：无法确认新 Worker 状态 → 回滚到备份并再次 reload
                tracing::error!("热更新等待超时，自动回滚");
                self.restore_from(&backup, &current).await?;
                return Err("热更新等待超时，已自动回滚".to_string());
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        // 5. 记录 committed 状态（worker_version 依据 + 崩溃观察窗备份锚点）并清理 staging
        self.write_committed_with(&meta.version, Some(&backup))?;
        self.clear_staging()?;

        tracing::info!("quireld 更新完成: version={} operator={}", meta.version, operator);
        Ok(())
    }

    /// 手动回滚到最近备份（返回 Payload::AgentUpdateActionResult）
    pub async fn rollback(&self, operator: &str) -> Payload {
        let result = self.rollback_inner(operator).await;
        let (success, error) = match &result {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e.clone())),
        };
        self.record_last_result("rollback", success, error.clone());
        let (manager_version, worker_version, lags) = self.version_triple();
        Payload::AgentUpdateActionResult {
            success,
            error,
            manager_version,
            worker_version,
            manager_lags_worker: lags,
            rolled_back: true,
        }
    }

    async fn rollback_inner(&self, operator: &str) -> Result<(), String> {
        // 与 begin/commit 对齐：开关关闭时整条更新链路（含回滚）一律拒绝
        if !self.cfg.enabled {
            return Err("更新功能已在服务器配置中关闭".to_string());
        }
        let backup = latest_backup()?;
        let current = std::env::current_exe()
            .map_err(|e| format!("定位当前二进制失败: {e}"))?
            .canonicalize()
            .map_err(|e| format!("解析当前二进制路径失败: {e}"))?;

        let tx = self.reload_tx.lock().unwrap().clone()
            .ok_or_else(|| "热更新通道未就绪".to_string())?;

        replace_binary(&backup, &current)?;
        tx.send(ReloadTrigger::ClientCommand { operator: operator.to_string() })
            .await
            .map_err(|_| "热更新通道已关闭".to_string())?;

        // 等待 reload 完成
        let deadline = tokio::time::Instant::now() + RELOAD_WAIT_TIMEOUT;
        while self.is_reloading_now() {
            if tokio::time::Instant::now() >= deadline {
                return Err("热更新等待超时".to_string());
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        // committed 状态清除（回滚后 worker 版本未知，等 systemctl restart 对齐）
        let state_path = self.staging_path(COMMITTED_STATE);
        let _ = fs::remove_file(state_path);
        tracing::info!("quireld 已回滚: backup={} operator={}", backup.display(), operator);
        Ok(())
    }

    /// 从 committed 状态记录的备份恢复到指定当前路径（current 可注入便于测试）。
    /// 备份优先取 committed 记录值，缺失/不存在时回退最近备份；恢复后清除 committed 状态。
    pub async fn rollback_to_committed_backup(&self, current: &Path) -> Result<(), String> {
        let state = self
            .read_committed()
            .ok_or_else(|| "无已提交状态，无需回滚".to_string())?;
        let backup = match state.backup.as_deref() {
            Some(b) if Path::new(b).exists() => PathBuf::from(b),
            _ => latest_backup()?,
        };
        self.restore_from(&backup, current).await?;
        let _ = fs::remove_file(self.staging_path(COMMITTED_STATE));
        tracing::info!("quireld 崩溃自动回滚: backup={}", backup.display());
        Ok(())
    }

    /// 观察窗触发的自动回滚（crash_detector 调用）：恢复 commit 前备份并热更新，
    /// 结果落 last_result.json（action=auto-rollback）
    pub async fn auto_rollback_after_crash(&self) {
        let current = match std::env::current_exe()
            .map_err(|e| format!("定位当前二进制失败: {e}"))
            .and_then(|p| p.canonicalize().map_err(|e| format!("解析当前二进制路径失败: {e}")))
        {
            Ok(p) => p,
            Err(e) => {
                self.record_last_result("auto-rollback", false, Some(e.clone()));
                tracing::error!("崩溃自动回滚失败: {e}");
                return;
            }
        };
        match self.rollback_to_committed_backup(&current).await {
            Ok(()) => {
                self.record_last_result("auto-rollback", true, None);
                tracing::info!("崩溃观察窗触发自动回滚完成，新 Worker 为回滚后二进制");
            }
            Err(e) => {
                self.record_last_result("auto-rollback", false, Some(e.clone()));
                tracing::error!("崩溃自动回滚失败: {e}");
            }
        }
    }

    // ── 内部工具 ─────────────────────────────────────────

    fn staging_path(&self, name: &str) -> PathBuf {
        Path::new(&self.cfg.staging_dir).join(name)
    }

    fn read_staged(&self) -> Option<(StagedMeta, PathBuf)> {
        let meta_path = self.staging_path(STAGED_META);
        let json = fs::read_to_string(meta_path).ok()?;
        let meta: StagedMeta = serde_json::from_str(&json).ok()?;
        Some((meta, self.staging_path(STAGED_BINARY)))
    }

    fn read_committed(&self) -> Option<CommittedState> {
        let json = fs::read_to_string(self.staging_path(COMMITTED_STATE)).ok()?;
        serde_json::from_str(&json).ok()
    }

    fn write_committed_with(&self, version: &str, backup: Option<&Path>) -> Result<(), String> {
        let state = CommittedState {
            version: version.to_string(),
            committed_at: current_secs(),
            backup: backup.map(|p| p.to_string_lossy().to_string()),
            crashes: 0,
        };
        let path = self.staging_path(COMMITTED_STATE);
        let json = serde_json::to_string(&state).map_err(|e| format!("状态序列化失败: {e}"))?;
        fs::write(&path, json).map_err(|e| format!("状态写入失败: {e}"))?;
        set_file_permissions(&path, 0o600)
    }

    /// Worker 崩溃观察（commit 后观察窗内累计，超过阈值返回 true 触发自动回滚）。
    /// 观察窗外/无 committed 状态一律 false——正常崩溃走 WorkerManager 既有重启链路。
    pub fn observe_worker_crash(&self) -> bool {
        let Some(mut state) = self.read_committed() else { return false };
        if current_secs().saturating_sub(state.committed_at) > OBSERVATION_WINDOW_SECS {
            return false;
        }
        state.crashes += 1;
        let path = self.staging_path(COMMITTED_STATE);
        if let Ok(json) = serde_json::to_string(&state) {
            let _ = fs::write(&path, json);
            let _ = set_file_permissions(&path, 0o600);
        }
        state.crashes > OBSERVE_MAX_CRASHES
    }

    fn clear_staging(&self) -> Result<(), String> {
        for name in [STAGED_BINARY, STAGED_META] {
            let p = self.staging_path(name);
            if p.exists() {
                fs::remove_file(&p).map_err(|e| format!("清理 staging 失败: {e}"))?;
            }
        }
        Ok(())
    }

    fn read_last_result(&self) -> Option<AgentUpdateLastResult> {
        let path = self.staging_path("last_result.json");
        let json = fs::read_to_string(path).ok()?;
        serde_json::from_str(&json).ok()
    }

    fn record_last_result(&self, action: &str, success: bool, error: Option<String>) {
        let result = AgentUpdateLastResult {
            action: action.to_string(),
            success,
            error,
            timestamp: current_secs(),
        };
        let path = self.staging_path("last_result.json");
        if let Ok(json) = serde_json::to_string(&result) {
            let _ = fs::write(&path, json);
            let _ = set_file_permissions(&path, 0o600);
        }
    }

    /// 版本三元组（manager/worker/lags）
    fn version_triple(&self) -> (String, Option<String>, bool) {
        let committed = self.read_committed();
        let worker_version = committed.as_ref().map(|c| c.version.clone());
        let lags = worker_version
            .as_deref()
            .map(|w| w != MANAGER_VERSION)
            .unwrap_or(false);
        (MANAGER_VERSION.to_string(), worker_version, lags)
    }

    /// 备份当前二进制（时间戳命名，保留 keep_backups 份）
    fn backup_current(&self, current: &Path) -> Result<PathBuf, String> {
        fs::create_dir_all(BACKUP_DIR).map_err(|e| format!("创建备份目录失败: {e}"))?;
        let ts = chrono_secs_filename();
        let backup = Path::new(BACKUP_DIR).join(format!("quireld_{ts}"));
        fs::copy(current, &backup).map_err(|e| format!("备份失败: {e}"))?;
        set_file_permissions(&backup, 0o700)?;

        // 保留最近 keep_backups 份（install.sh/update.sh 同惯例）
        let mut backups: Vec<PathBuf> = fs::read_dir(BACKUP_DIR)
            .map_err(|e| format!("读取备份目录失败: {e}"))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("quireld_")).unwrap_or(false))
            .collect();
        backups.sort();
        while backups.len() > self.cfg.keep_backups {
            let oldest = backups.remove(0);
            let _ = fs::remove_file(oldest);
        }
        Ok(backup)
    }

    /// 从备份恢复并触发 reload（commit 失败路径）
    async fn restore_from(&self, backup: &Path, current: &Path) -> Result<(), String> {
        replace_binary(backup, current)?;
        // guard 临时值先释放再 await（MutexGuard 非 Send，不能跨 await 持有）
        let tx = self.reload_tx.lock().unwrap().clone();
        if let Some(tx) = tx {
            let _ = tx.send(ReloadTrigger::ClientCommand { operator: "auto-rollback".to_string() }).await;
        }
        Ok(())
    }
}

// ── 自由函数 ─────────────────────────────────────────────

fn meta_to_staged(meta: &StagedMeta) -> StagedUpdate {
    StagedUpdate {
        sha256: meta.sha256.clone(),
        size: meta.size,
        version: meta.version.clone(),
        staged_at: meta.staged_at,
    }
}

fn current_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 备份文件名时间戳（本地时间可读格式，与 update.sh 的 %Y%m%d_%H%M%S 对齐）
fn chrono_secs_filename() -> String {
    // 无 chrono 依赖场景的简化实现：Unix 秒即可保证唯一与排序
    format!("{}", current_secs())
}

fn sha256_hex(path: &Path) -> Result<String, String> {
    let mut f = fs::File::open(path).map_err(|e| format!("打开文件失败: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("读取失败: {e}"))?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// ELF 架构校验：magic + e_machine 与 Manager 本机架构一致
fn verify_elf_arch(path: &Path) -> Result<(), String> {
    let expected_machine: u16 = match std::env::consts::ARCH {
        "x86_64" => 0x3E,    // EM_X86_64
        "aarch64" => 0xB7,   // EM_AARCH64
        other => return Err(format!("不支持的运行架构: {other}")),
    };
    let mut f = fs::File::open(path).map_err(|e| format!("打开文件失败: {e}"))?;
    let mut header = [0u8; 20];
    f.read_exact(&mut header).map_err(|_| "文件过短，非有效 ELF".to_string())?;
    if &header[0..4] != b"\x7fELF" {
        return Err("非 ELF 文件，拒绝替换二进制".to_string());
    }
    let machine = u16::from_le_bytes([header[18], header[19]]);
    if machine != expected_machine {
        return Err(format!(
            "二进制架构不匹配: 期望 e_machine={expected_machine:#x} 实际 {machine:#x}"
        ));
    }
    Ok(())
}

/// staging 二进制 --version 探测（1.5s 看门狗超时 + 输出上限；
/// 输出取第二个空白字段并剥离 -dev 后缀）
fn probe_version(path: &Path) -> Result<String, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);
    const PROBE_OUTPUT_LIMIT: usize = 64 * 1024;

    let mut child = Command::new(path)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("版本探测执行失败: {e}"))?;

    // 看门狗：到点强杀（staged 二进制可能挂起不退出；正常完成后 kill 为
    // 无害 ESRCH）。注释旧称"1.5s 超时"实际 .output() 无任何超时（A-09）
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        std::thread::spawn(move || {
            std::thread::sleep(PROBE_TIMEOUT);
            unsafe { libc::kill(pid, libc::SIGKILL); }
        });
    }

    // 输出上限读取（防恶意二进制灌爆内存；超限继续读到 EOF/超时）
    let mut stdout = child.stdout.take().ok_or("无法读取探测输出")?;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < PROBE_OUTPUT_LIMIT {
                    let take = n.min(PROBE_OUTPUT_LIMIT - buf.len());
                    buf.extend_from_slice(&chunk[..take]);
                }
            }
            Err(_) => break,
        }
    }
    let status = child.wait().map_err(|e| format!("版本探测等待失败: {e}"))?;
    if !status.success() {
        return Err(format!("版本探测退出码 {:?}", status.code()));
    }
    let stdout_str = String::from_utf8_lossy(&buf);
    let raw = stdout_str.split_whitespace().nth(1).ok_or("版本输出格式异常")?;
    Ok(raw.split('-').next().unwrap_or(raw).to_string())
}

/// 原子替换二进制：同目录 staging → fsync → rename 覆盖。
/// ETXTBSY 安全（rename 不打开目标写句柄，运行中进程持旧 inode 不受影响）；
/// 中途崩溃/盘满不会出现目标二进制缺失——旧方案 rm(unlink)→copy 的窗口
/// 在 rename 前目标已不存在（2026-11-20 审计 A-10）
fn replace_binary(from: &Path, to: &Path) -> Result<(), String> {
    let staged = to.with_extension("staging");
    // 复制到同目录临时名（rename 要求同文件系统）
    fs::copy(from, &staged).map_err(|e| format!("复制二进制失败: {e}"))?;
    // 内容落盘后再 rename（崩溃窗口不产生截断目标）
    {
        let f = fs::File::open(&staged).map_err(|e| format!("打开 staging 失败: {e}"))?;
        f.sync_all().map_err(|e| format!("staging fsync 失败: {e}"))?;
    }
    set_file_permissions(&staged, 0o755)?;
    fs::rename(&staged, to).map_err(|e| format!("替换二进制失败: {e}"))?;
    Ok(())
}

/// 目录权限设置（Unix）
#[cfg(unix)]
fn set_dir_permissions(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("设置目录权限失败: {e}"))
}

#[cfg(not(unix))]
fn set_dir_permissions(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(())
}

/// 文件权限设置（Unix）
#[cfg(unix)]
fn set_file_permissions(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("设置文件权限失败: {e}"))
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path, _mode: u32) -> Result<(), String> {
    Ok(())
}

/// 最新备份（文件名按字典序即时间序）
fn latest_backup() -> Result<PathBuf, String> {
    let mut backups: Vec<PathBuf> = fs::read_dir(BACKUP_DIR)
        .map_err(|e| format!("读取备份目录失败: {e}"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().map(|n| n.to_string_lossy().starts_with("quireld_")).unwrap_or(false))
        .collect();
    backups.sort();
    backups.pop().ok_or_else(|| "没有可用备份".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg(dir: &Path) -> UpdatesConfig {
        UpdatesConfig {
            enabled: true,
            staging_dir: dir.to_string_lossy().to_string(),
            max_binary_mb: 1,
            keep_backups: 3,
        }
    }

    /// 最小 ELF 头构造（magic + e_machine）
    fn elf_bytes(machine: u16) -> Vec<u8> {
        let mut v = vec![0x7f, b'E', b'L', b'F'];
        v.extend_from_slice(&[0u8; 14]); // 其余 magic/类字段
        v.extend_from_slice(&machine.to_le_bytes()); // 偏移 18-19
        v
    }

    #[test]
    fn elf校验_本机架构通过_异架构拒绝() {
        let dir = std::env::temp_dir().join(format!("quirel-ut-elf-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good");
        let expected: u16 = match std::env::consts::ARCH {
            "x86_64" => 0x3E,
            "aarch64" => 0xB7,
            _ => panic!("untested arch"),
        };
        fs::write(&good, elf_bytes(expected)).unwrap();
        assert!(verify_elf_arch(&good).is_ok());

        let wrong_machine = if expected == 0x3E { 0xB7 } else { 0x3E };
        let bad = dir.join("bad");
        fs::write(&bad, elf_bytes(wrong_machine)).unwrap();
        assert!(verify_elf_arch(&bad).is_err());

        // 非 ELF 拒绝
        let not_elf = dir.join("not_elf");
        fs::write(&not_elf, b"#!/bin/sh\necho hi").unwrap();
        assert!(verify_elf_arch(&not_elf).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 上传流程_大小不符拒绝_哈希不符拒绝() {
        let dir = std::env::temp_dir().join(format!("quirel-ut-up-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mgr = UpdateManager::new(&test_cfg(&dir)).unwrap();

        // begin_upload：超限拒绝
        assert!(mgr.begin_upload(2 * 1024 * 1024).is_err());
        assert!(mgr.begin_upload(0).is_err());
        let path = mgr.begin_upload(16).unwrap();
        assert!(path.ends_with(STAGED_BINARY));

        // 写入 16 字节
        mgr.write_chunk(&path, &[0u8; 16]).unwrap();

        // 大小不符拒绝
        assert!(mgr.finish_upload("00", 32, "0.3.3").is_err());
        // 哈希不符拒绝
        assert!(mgr.finish_upload("ff", 16, "0.3.3").is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn staging元数据恢复_重启后status可读() {
        let dir = std::env::temp_dir().join(format!("quirel-ut-meta-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = test_cfg(&dir);
        let mgr = UpdateManager::new(&cfg).unwrap();

        // 模拟 staged.json 落盘
        let meta = StagedMeta {
            sha256: "ab12".to_string(),
            size: 100,
            version: "0.3.3".to_string(),
            staged_at: 1700000000,
        };
        fs::write(
            mgr.staging_path(STAGED_META),
            serde_json::to_string(&meta).unwrap(),
        ).unwrap();

        let status = mgr.status();
        let Payload::AgentUpdateStatusResponse {
            manager_version, enabled, staged, ..
        } = status
        else {
            panic!("status 应返回 AgentUpdateStatusResponse 变体");
        };
        assert_eq!(manager_version, MANAGER_VERSION);
        assert!(enabled);
        let staged = staged.expect("应恢复 staged 状态");
        assert_eq!(staged.sha256, "ab12");
        assert_eq!(staged.version, "0.3.3");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn 父目录缺失时拒绝创建staging() {
        // 红线：禁止 create_dir_all 自动创建多级父目录
        let missing_root = std::env::temp_dir().join(format!(
            "quirel-ut-noparent-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&missing_root);
        let cfg = UpdatesConfig {
            staging_dir: missing_root.join("a/b/updates").to_string_lossy().to_string(),
            ..test_cfg(&missing_root)
        };
        assert!(UpdateManager::new(&cfg).is_err());
    }

    #[tokio::test]
    async fn 崩溃观察窗_窗口内超阈值触发并可回滚到记录备份() {
        let root = std::env::temp_dir().join(format!("quirel-ut-obsw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mgr = UpdateManager::new(&test_cfg(&root)).unwrap();

        // 旧备份内容 OLD，当前二进制内容 NEW；committed 状态记录备份路径
        let backup = root.join("quireld_1700000000");
        fs::write(&backup, b"OLD").unwrap();
        let current = root.join("current-bin");
        fs::write(&current, b"NEW").unwrap();
        let state = serde_json::json!({
            "version": "0.3.3",
            "committed_at": current_secs(),
            "backup": backup.to_string_lossy(),
            "crashes": 0
        });
        fs::write(mgr.staging_path(COMMITTED_STATE), state.to_string()).unwrap();

        // 观察窗内：前两次崩溃交给既有重启，第三次触发自动回滚
        assert!(!mgr.observe_worker_crash());
        assert!(!mgr.observe_worker_crash());
        assert!(mgr.observe_worker_crash());

        // 回滚到 committed 记录的备份，并清除 committed 状态（worker 版本回到未知）
        mgr.rollback_to_committed_backup(&current).await.unwrap();
        assert_eq!(fs::read(&current).unwrap(), b"OLD");
        assert!(fs::metadata(mgr.staging_path(COMMITTED_STATE)).is_err());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn 崩溃观察窗_窗口外或无状态不计入() {
        let root = std::env::temp_dir().join(format!("quirel-ut-obso-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mgr = UpdateManager::new(&test_cfg(&root)).unwrap();

        // 无 committed 状态：不计数不触发
        assert!(!mgr.observe_worker_crash());

        // committed_at 早于观察窗：不触发（正常崩溃走既有重启链路）
        let state = serde_json::json!({
            "version": "0.3.3",
            "committed_at": current_secs().saturating_sub(601),
            "crashes": 0
        });
        fs::write(mgr.staging_path(COMMITTED_STATE), state.to_string()).unwrap();
        assert!(!mgr.observe_worker_crash());

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn rollback_受更新开关门控() {
        // enabled=false 时 rollback 直接拒绝（与 begin/commit 门控对齐），
        // 错误信息须明确提示开关关闭，而非"没有可用备份"
        let dir = std::env::temp_dir().join(format!("quirel-ut-rbgate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cfg = UpdatesConfig {
            enabled: false,
            staging_dir: dir.to_string_lossy().to_string(),
            max_binary_mb: 1,
            keep_backups: 3,
        };
        let mgr = UpdateManager::new(&cfg).unwrap();

        let result = mgr.rollback("test").await;
        let Payload::AgentUpdateActionResult { success, error, .. } = result else {
            panic!("rollback 应返回 AgentUpdateActionResult");
        };
        assert!(!success);
        let err_msg = error.unwrap_or_default();
        assert!(err_msg.contains("关闭"), "实际错误: {err_msg}");
        let _ = fs::remove_dir_all(&dir);
    }
}
