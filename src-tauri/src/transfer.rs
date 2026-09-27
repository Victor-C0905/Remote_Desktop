//! 文件传输管理器
//!
//! 负责管理文件上传/下载任务，发送进度事件到前端。

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{command, AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::connection::{ConnectionManager, Envelope, Payload};

// ── 裸二进制帧(数据平面) ─────────────────────────────────────
// 帧格式: [4B length LE][1B type][body], length = 1 + body.len()
//   type = 0x01 控制(JSON Envelope) / 0x02 数据块
// 数据块 body: [4B seq][4B size][data]
// 与 agent/src/protocol/raw_frame.rs 格式完全一致

const RAW_TYPE_CONTROL: u8 = 0x01;
const RAW_TYPE_DATA: u8 = 0x02;
const RAW_MAX_FRAME_LEN: u32 = 10 * 1024 * 1024;

/// 写数据帧: [4B len][1B type=0x02][4B seq][4B size][data]
async fn write_data_frame(send: &mut quinn::SendStream, seq: u32, data: &[u8]) -> Result<(), String> {
    let body_len = 8 + data.len();
    let total_len = (1 + body_len) as u32;
    let mut header = Vec::with_capacity(5 + body_len);
    header.extend_from_slice(&total_len.to_le_bytes());
    header.push(RAW_TYPE_DATA);
    header.extend_from_slice(&seq.to_le_bytes());
    header.extend_from_slice(&(data.len() as u32).to_le_bytes());
    header.extend_from_slice(data);
    send.write_all(&header).await.map_err(|e| format!("写数据帧失败: {}", e))
}

/// 写控制帧: [4B len][1B type=0x01][body]
async fn write_control_frame(send: &mut quinn::SendStream, body: &[u8]) -> Result<(), String> {
    let total_len = (1 + body.len()) as u32;
    let mut header = Vec::with_capacity(5 + body.len());
    header.extend_from_slice(&total_len.to_le_bytes());
    header.push(RAW_TYPE_CONTROL);
    header.extend_from_slice(body);
    send.write_all(&header).await.map_err(|e| format!("写控制帧失败: {}", e))
}

/// 读帧头: [4B len][1B type], 返回 (type, body_len)
async fn read_frame_header(recv: &mut quinn::RecvStream) -> Result<(u8, usize), String> {
    let mut buf = [0u8; 5];
    recv.read_exact(&mut buf).await.map_err(|e| format!("读帧头失败: {}", e))?;
    let len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    if len == 0 || len > RAW_MAX_FRAME_LEN {
        return Err(format!("帧长度非法: {}", len));
    }
    Ok((buf[4], len as usize - 1))
}

/// 读数据帧 body: [4B seq][4B size][data], 返回 (seq, data)
async fn read_data_body(recv: &mut quinn::RecvStream, body_len: usize) -> Result<(u32, Vec<u8>), String> {
    let mut buf = vec![0u8; body_len];
    recv.read_exact(&mut buf).await.map_err(|e| format!("读数据帧 body 失败: {}", e))?;
    if buf.len() < 8 {
        return Err(format!("数据帧 body 过短: {}", buf.len()));
    }
    let seq = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let size = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    let data = buf[8..].to_vec();
    if data.len() != size {
        return Err(format!("数据帧 size 不匹配: 声明 {} 实际 {}", size, data.len()));
    }
    Ok((seq, data))
}

/// 读控制帧 body: 返回原始字节(调用方再 Envelope::decode)
async fn read_control_body(recv: &mut quinn::RecvStream, body_len: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; body_len];
    recv.read_exact(&mut buf).await.map_err(|e| format!("读控制帧 body 失败: {}", e))?;
    Ok(buf)
}

// ── 传输状态枚举 ─────────────────────────────────────────────

/// 传输任务状态
///
/// 替代字符串状态，提供类型安全和编译期检查。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferStatus {
    Pending,
    Active,
    Paused,
    Completed,
    Error,
    Cancelled,
    /// 连接断开导致的中断（区别于文件/协议错误）
    Interrupted,
}

impl TransferStatus {
    /// 是否为终态（完成后不可变更）
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Error | Self::Cancelled | Self::Interrupted
        )
    }

    /// 是否可重试
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Error | Self::Cancelled | Self::Interrupted
        )
    }
}

impl std::fmt::Display for TransferStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Active => write!(f, "active"),
            Self::Paused => write!(f, "paused"),
            Self::Completed => write!(f, "completed"),
            Self::Error => write!(f, "error"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::Interrupted => write!(f, "interrupted"),
        }
    }
}

// ── 临时文件工具函数 ──────────────────────────────────────────

/// 生成确定性临时文件路径
///
/// 使用目标路径的 SHA-256 哈希生成临时文件名。
/// 同一个目标文件总是生成相同的临时文件名，便于：
/// - 断点续传：无需搜索，直接找到之前的临时文件
/// - 避免中文/长文件名导致的路径长度问题
///
/// 文件名格式: `quirel_{hash前16位}.tmp`
fn generate_temp_path(path: &Path) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let path_str = path.to_string_lossy();
    let mut hasher = DefaultHasher::new();
    path_str.hash(&mut hasher);
    let hash = hasher.finish();

    let temp_name = format!("quirel_{:016x}.tmp", hash);
    path.parent().unwrap_or(Path::new(".")).join(temp_name)
}

/// 查找已存在的临时文件（用于断点续传）
///
/// 使用确定性哈希生成临时文件名，直接检查是否存在。
fn find_existing_temp_file(path: &Path) -> Option<PathBuf> {
    let temp_path = generate_temp_path(path);
    if temp_path.exists() {
        Some(temp_path)
    } else {
        None
    }
}

/// 传输任务状态
#[derive(Debug, Clone, Serialize)]
pub struct TransferTask {
    /// 任务唯一标识
    pub id: String,
    /// 会话 ID（关联到连接）
    pub session_id: String,
    /// 传输方向: "upload" 或 "download"
    pub direction: String,
    /// 文件名
    pub file_name: String,
    /// 远程路径
    pub remote_path: String,
    /// 本地路径
    pub local_path: String,
    /// 文件总大小（字节）
    pub file_size: u64,
    /// 已传输大小（字节）
    pub transferred: u64,
    /// 进度百分比 (0-100)
    pub progress: u32,
    /// 传输速度（字节/秒）
    pub speed_bps: u64,
    /// 预估剩余时间（秒）
    pub eta_secs: u64,
    /// 任务状态
    pub status: TransferStatus,
    /// 错误信息（如果失败）
    pub error: Option<String>,
    /// 任务创建时间（Unix 时间戳，秒）
    pub created_at: u64,

    // ── 原始参数（用于重试和断点续传） ──
    /// 服务器 ID（原始参数）
    pub server_id: String,
    /// Agent 端的传输会话 ID（用于发送取消请求等操作）
    pub agent_session_id: Option<String>,
}

/// 传输管理器
pub struct TransferManager {
    /// Tauri 应用句柄（用于发送事件）
    app_handle: AppHandle,
    /// 活动任务映射表
    tasks: Arc<Mutex<HashMap<String, TransferTask>>>,
}

impl TransferManager {
    /// 创建新的传输管理器
    pub fn new(app_handle: AppHandle) -> Self {
        let manager = Self {
            app_handle,
            tasks: Arc::new(Mutex::new(HashMap::new())),
        };

        // 注意：start_cleanup_task() 不能在构造函数中调用
        // 因为此时 tokio 运行时可能还未启动
        // 需要在 Tauri 的 setup钩子中调用

        manager
    }

    /// 启动定期清理任务
    ///
    /// 每 5 分钟清理一次已完成的任务，保留最近 10 个
    /// 注意：此方法必须在 tokio 运行时中调用
    pub async fn start_cleanup_task(&self) {
        let tasks = self.tasks.clone();

        // 不再使用 tokio::spawn，而是直接运行清理循环
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300)); // 每5分钟清理一次

        loop {
            interval.tick().await;

            // 检查关闭标志
            if *crate::SHUTDOWN_FLAG.lock().await {
                break;
            }

            // 清理已完成的任务（保留最近10个）
            let mut tasks_guard = tasks.lock().await;

            // 收集已完成的任务（ID 和创建时间）
            let mut completed: Vec<(String, u64)> = tasks_guard
                .iter()
                .filter(|(_, task)| {
                    task.status.is_terminal()
                })
                .map(|(id, task)| (id.clone(), task.created_at))
                .collect();

            // 按创建时间排序（最旧的在前）
            completed.sort_by_key(|(_, created_at)| *created_at);

            // 保留最近10个已完成的任务（删除最旧的）
            if completed.len() > 10 {
                let to_remove = completed.len() - 10;
                let ids_to_remove: Vec<String> = completed.into_iter().take(to_remove).map(|(id, _)| id).collect();
                for id in ids_to_remove {
                    tasks_guard.remove(&id);
                }
                tracing::info!("[TransferManager] 自动清理了 {} 个已完成任务", to_remove);
            }
        }
    }

    /// 创建传输任务（简化版：仅创建任务，不实际传输）
    ///
    /// # 参数
    /// - `server_id`: 服务器 ID
    /// - `direction`: 传输方向 ("upload" 或 "download")
    /// - `remote_path`: 远程文件路径
    /// - `local_path`: 本地文件路径
    ///
    /// # 返回
    /// 成功返回任务 ID，失败返回错误信息
    pub async fn create_task(
        &self,
        server_id: String,
        direction: String,
        remote_path: String,
        local_path: String,
    ) -> Result<String, String> {
        // 生成任务 ID
        let task_id = Uuid::new_v4().to_string();

        // 获取文件大小（上传时从本地文件获取，下载时设为 0）
        let file_size = if direction == "upload" {
            std::fs::metadata(&local_path)
                .map(|m| m.len())
                .map_err(|e| format!("无法访问本地文件: {}", e))?
        } else {
            // 下载时文件大小由 Agent 返回，这里设为 0
            0
        };

        // 获取文件名
        // 上传时：使用 remote_path 的文件名（用户可能重命名）
        // 下载时：使用 local_path 的文件名（用户可能重命名）
        let file_name = if direction == "upload" {
            PathBuf::from(&remote_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        } else {
            PathBuf::from(&local_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        };

        // 创建任务
        let task = TransferTask {
            id: task_id.clone(),
            session_id: server_id.clone(),
            direction,
            file_name,
            remote_path,
            local_path: local_path.clone(),
            file_size,
            transferred: 0,
            progress: 0,
            speed_bps: 0,
            eta_secs: 0,
            status: TransferStatus::Pending,
            error: None,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            server_id: server_id,  // 保存原始参数
            agent_session_id: None,  // Agent 端 session_id，在传输开始后设置
        };

        // 保存任务
        {
            let mut tasks = self.tasks.lock().await;
            tasks.insert(task_id.clone(), task.clone());
        }

        // 发送进度事件到前端
        self.emit_progress(&task)?;

        tracing::info!(
            "创建传输任务: id={}, direction={}, file={}, size={} bytes",
            task_id,
            task.direction,
            task.file_name,
            task.file_size
        );

        Ok(task_id)
    }

    /// 发送进度事件到前端
    fn emit_progress(&self, task: &TransferTask) -> Result<(), String> {
        self.app_handle
            .emit("transfer-progress", task)
            .map_err(|e| format!("发送事件失败: {}", e))?;
        Ok(())
    }

    /// 获取任务信息
    #[allow(dead_code)]
    pub async fn get_task(&self, task_id: &str) -> Option<TransferTask> {
        let tasks = self.tasks.lock().await;
        tasks.get(task_id).cloned()
    }

    /// 更新任务进度
    pub async fn update_progress(
        &self,
        task_id: &str,
        transferred: u64,
        speed_bps: u64,
    ) -> Result<(), String> {
        let mut tasks = self.tasks.lock().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("任务不存在: {}", task_id))?;

        // 终态守卫：连接断开被标记 Interrupted 后，迟到的进度写入直接丢弃，
        // 避免「已中断」被覆写为进行中/已完成
        if task.status.is_terminal() {
            drop(tasks);
            return Ok(());
        }

        let old_progress = task.progress;  // ← 在更新前保存旧进度

        task.transferred = transferred;
        task.speed_bps = speed_bps;

        // 计算进度百分比
        if task.file_size > 0 {
            task.progress = ((transferred as f64 / task.file_size as f64) * 100.0) as u32;
        }

        // 计算 ETA（预估剩余时间）
        if speed_bps > 0 && transferred < task.file_size {
            let remaining = task.file_size - transferred;
            task.eta_secs = remaining / speed_bps;
        }

        // 更新状态
        // 注意：只有在上传时才自动完成，下载需要等待 FileTransferComplete 消息
        if task.progress >= 100 && task.direction == "upload" {
            task.status = TransferStatus::Completed;
        } else if task.status == TransferStatus::Pending {
            task.status = TransferStatus::Active;
        }

        // 节流：只在进度变化超过 1% 时发送事件
        // 小文件（< 1MB）不节流，保证进度可见
        let should_emit = if task.file_size < 1_000_000 {
            true  // 小文件：每次都发送
        } else {
            // 大文件：进度变化超过 1% 才发送
            // 注意：使用 old_progress（更新前的值）
            task.progress > old_progress || transferred == task.file_size
        };

        // 发送进度事件
        if should_emit {
            let task_clone = task.clone();
            drop(tasks);  // 释放锁
            self.emit_progress(&task_clone)?;
        } else {
            drop(tasks);  // 释放锁
        }

        Ok(())
    }

    /// 标记任务失败
    pub async fn mark_failed(&self, task_id: &str, error: String) -> Result<(), String> {
        let mut tasks = self.tasks.lock().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("任务不存在: {}", task_id))?;

        // 终态守卫：已被中断（连接断开）的任务不再接受迟到失败，
        // 保留「已中断」状态与其中断文案
        if task.status.is_terminal() {
            drop(tasks);
            tracing::debug!("忽略迟到的失败标记（任务已是终态）: id={}", task_id);
            return Ok(());
        }

        task.status = TransferStatus::Error;  // ← 改为 Error，与前端一致
        task.error = Some(error);

        // 发送进度事件
        let task_clone = task.clone();
        drop(tasks);
        self.emit_progress(&task_clone)?;

        Ok(())
    }

    /// 标记任务完成
    pub async fn mark_completed(&self, task_id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.lock().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("任务不存在: {}", task_id))?;

        // 终态守卫：已被中断的任务不接受迟到的完成标记，保留「已中断」状态
        if task.status.is_terminal() {
            drop(tasks);
            tracing::debug!("忽略迟到的完成标记（任务已是终态）: id={}", task_id);
            return Ok(());
        }

        task.status = TransferStatus::Completed;
        task.progress = 100;

        // 发送进度事件
        let task_clone = task.clone();
        drop(tasks);
        self.emit_progress(&task_clone)?;

        // 如果是上传，发送 upload-completed 事件（包含目录路径）
        if task_clone.direction == "upload" {
            // 提取目录路径
            let dir_path = if let Some(pos) = task_clone.remote_path.rfind('/') {
                if pos == 0 {
                    "/".to_string()
                } else {
                    task_clone.remote_path[..pos].to_string()
                }
            } else {
                "/".to_string()
            };

            self.app_handle
                .emit("upload-completed", serde_json::json!({
                    "remote_path": task_clone.remote_path,
                    "dir_path": dir_path,
                    "file_name": task_clone.file_name,
                }))
                .map_err(|e| format!("发送 upload-completed 事件失败: {}", e))?;
        }

        Ok(())
    }

    /// 暂停任务
    pub async fn pause_task(&self, task_id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.lock().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("任务不存在: {}", task_id))?;

        // 只能暂停活动任务
        if task.status != TransferStatus::Active {
            return Err(format!("只能暂停活动中的任务，当前状态: {}", task.status));
        }

        task.status = TransferStatus::Paused;

        // 发送进度事件
        let task_clone = task.clone();
        drop(tasks);
        self.emit_progress(&task_clone)?;

        tracing::info!("暂停传输任务: id={}", task_id);
        Ok(())
    }

    /// 继续任务
    pub async fn resume_task(&self, task_id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.lock().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("任务不存在: {}", task_id))?;

        // 只能继续暂停的任务
        if task.status != TransferStatus::Paused {
            return Err(format!("只能继续暂停的任务，当前状态: {}", task.status));
        }

        task.status = TransferStatus::Active;

        // 发送进度事件
        let task_clone = task.clone();
        drop(tasks);
        self.emit_progress(&task_clone)?;

        tracing::info!("继续传输任务: id={}", task_id);
        Ok(())
    }

    /// 重试任务（支持断点续传）
    ///
    /// # 参数
    /// - `task_id`: 任务 ID
    /// - `app_handle`: Tauri 应用句柄
    ///
    /// # 断点续传逻辑
    /// - 如果任务之前已有传输进度（transferred > 0），尝试断点续传
    /// - 否则从头开始传输
    pub async fn retry_task(&self, task_id: &str, app_handle: &AppHandle) -> Result<(), String> {
        let mut tasks = self.tasks.lock().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("任务不存在: {}", task_id))?;

        // 只能重试失败的任务
        if !task.status.is_retryable() {
            return Err(format!("只能重试失败的任务，当前状态: {}", task.status));
        }

        // 保存断点续传位置
        let resume_from = if task.transferred > 0 {
            Some(task.transferred)
        } else {
            None
        };

        // 重置任务状态
        task.status = TransferStatus::Pending;
        task.error = None;

        // 保存任务副本用于重新启动传输
        let task_clone = task.clone();

        // 发送进度事件
        self.emit_progress(&task_clone)?;

        tracing::info!(
            "重试传输任务: id={}, resume_from={:?}",
            task_id,
            resume_from
        );

        // 释放锁
        drop(tasks);

        // 重新启动传输（需要在后台任务中执行）
        let task_id_clone = task_clone.id.clone();

        // 获取连接管理器和 QUIC Connection
        let connection_manager = app_handle.state::<ConnectionManager>();

        let quic_conn = {
            let conns = connection_manager.connections.lock().unwrap();
            let active_conn = conns.get(&task_clone.server_id)
                .ok_or_else(|| format!("服务器未连接: {}", task_clone.server_id))?;
            active_conn.quic_conn.clone()
        };

        let conn = quic_conn.ok_or_else(|| {
            "未找到 QUIC Connection，可能只使用了 WebSocket 连接".to_string()
        })?;

        let request_id = connection_manager.next_request_id();

        // 使用现有的 manager（不是创建新的）
        let manager = app_handle.state::<Arc<TransferManager>>();

        // 在后台任务中重新执行传输
        let manager_clone = Arc::clone(&manager);
        tokio::spawn(async move {
            let result = perform_transfer(
                &conn,
                request_id,
                task_clone.direction.clone(),
                task_clone.remote_path.clone(),
                task_clone.local_path.clone(),
                Arc::clone(&manager_clone),
                &task_clone.id,
                resume_from,  // 传递断点续传位置
            ).await;

            if let Err(e) = result {
                tracing::error!("重试传输失败: task_id={}, error={}", task_id_clone, e);
                let _ = manager_clone.mark_failed(&task_id_clone, e).await;
            }
        });

        Ok(())
    }

    /// 清理指定连接的所有任务
    ///
    /// 连接断开（用户切换/网络中断）时的统一清理入口：
    /// - 非终态任务 → 标记 Interrupted 并**保留**（供切回服务器后重试）
    /// - 终态任务不动（由 start_cleanup_task 的 5 分钟周期按保留策略回收）
    pub async fn cleanup_by_connection(&self, connection_id: &str) {
        // 1. 锁内标记非终态任务为 Interrupted（保留记录）
        let interrupted_tasks = {
            let mut tasks = self.tasks.lock().await;
            let mut marked = Vec::new();
            for (_id, task) in tasks.iter_mut() {
                if task.server_id == connection_id && !task.status.is_terminal() {
                    task.status = TransferStatus::Interrupted;
                    task.error = Some("连接已断开，传输已中断".to_string());
                    task.speed_bps = 0;
                    task.eta_secs = 0;
                    marked.push(task.clone());
                }
            }
            marked
        };
        // ← 锁已释放

        let count = interrupted_tasks.len();

        // 2. 发送事件（前端收到 interrupted 状态更新）
        for task in interrupted_tasks {
            let _ = self.emit_progress(&task);
        }

        tracing::info!(connection_id, count, "已中断连接的传输任务（保留记录供重试）");
    }

    /// 统计未完成传输任务数（pending/active/paused）
    ///
    /// 供前端切换服务器前的中断确认使用。
    /// paused 也计入：连接断开时暂停中的任务同样会被置为 interrupted。
    /// 用 !is_terminal() 判定，与 cleanup_by_connection 的中断集合共享同一口径，
    /// 保证「确认弹窗的计数」与「实际被中断的任务数」始终一致。
    pub async fn count_active(&self) -> usize {
        let tasks = self.tasks.lock().await;
        tasks
            .values()
            .filter(|t| !t.status.is_terminal())
            .count()
    }

    /// 取消指定任务
    ///
    /// 取消时会同时通知 Agent 端清理对应的传输会话。
    /// 如果发送取消请求失败（如网络中断），不会阻塞本地取消流程，
    /// Agent 端的超时机制会兜底清理。
    pub async fn cancel_task(&self, task_id: &str, app_handle: &AppHandle) -> Result<(), String> {
        let (task_clone, agent_session_id) = {
            let mut tasks = self.tasks.lock().await;
            if let Some(task) = tasks.get_mut(task_id) {
                task.status = TransferStatus::Cancelled;
                let task_clone = task.clone();
                let agent_session_id = task.agent_session_id.clone();
                (task_clone, agent_session_id)
            } else {
                return Err(format!("任务 {} 不存在", task_id));
            }
        };

        // 发送进度事件到前端
        self.emit_progress(&task_clone)?;

        // 如果有 Agent 端的 session_id，发送取消请求通知 Agent 清理会话
        if let Some(session_id) = agent_session_id {
            let server_id = task_clone.server_id.clone();
            let task_id_owned = task_id.to_string();
            let cancel_payload = crate::connection::Payload::CancelFileTransfer {
                session_id,
                reason: "用户取消".to_string(),
            };

            // 异步发送取消请求，不等待结果（fire-and-forget）
            let app = app_handle.clone();
            tokio::spawn(async move {
                match crate::connection::remote_send(server_id.clone(), cancel_payload, app).await {
                    Ok(_) => {
                        tracing::info!(
                            "[TransferManager] 已通知 Agent 取消传输: task_id={}, server_id={}",
                            task_id_owned, server_id
                        );
                    }
                    Err(e) => {
                        // 发送失败不影响本地取消，Agent 端超时机制会兜底清理
                        tracing::warn!(
                            "[TransferManager] 通知 Agent 取消传输失败（本地已取消）: task_id={}, error={}",
                            task_id_owned, e
                        );
                    }
                }
            });
        }

        Ok(())
    }

    /// 列出所有任务
    pub async fn list_tasks(&self) -> Vec<TransferTask> {
        let tasks = self.tasks.lock().await;
        tasks.values().cloned().collect()
    }

    /// 获取任务状态
    pub async fn get_status(&self, task_id: &str) -> Option<TransferStatus> {
        let tasks = self.tasks.lock().await;
        tasks.get(task_id).map(|t| t.status)
    }

    /// 设置 Agent 端传输会话 ID
    pub async fn set_agent_session_id(&self, task_id: &str, session_id: String) {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.agent_session_id = Some(session_id);
        }
    }

    /// 更新文件大小（下载时，从 Agent 获取实际大小）
    pub async fn update_file_size(&self, task_id: &str, file_size: u64) -> Result<(), String> {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.file_size = file_size;
            let task_clone = task.clone();
            drop(tasks);
            self.emit_progress(&task_clone)?;
        }
        Ok(())
    }
}

/* ── 暂停等待逻辑 ──────────────────────────────────────── */

/// 等待暂停任务恢复
///
/// 如果任务被暂停，轮询等待直到恢复或取消。
/// - 返回 `Ok(())` 表示已恢复或未暂停
/// - 返回 `Err(msg)` 表示已取消或任务已删除
async fn wait_if_paused(
    manager: &TransferManager,
    task_id: &str,
) -> Result<(), String> {
    let status = match manager.get_status(task_id).await {
        Some(s) => s,
        None => return Err("任务已删除".to_string()),
    };

    if status != TransferStatus::Paused {
        return Ok(());
    }

    tracing::info!("传输已暂停: task_id={}", task_id);

    // 轮询等待恢复
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        match manager.get_status(task_id).await {
            Some(TransferStatus::Active) => {
                tracing::info!("传输已恢复: task_id={}", task_id);
                return Ok(());
            }
            Some(TransferStatus::Cancelled) => {
                tracing::info!("传输已取消: task_id={}", task_id);
                return Err("传输已取消".to_string());
            }
            Some(TransferStatus::Interrupted) => {
                // 连接断开：任务已被统一清理块标记中断，终止传输 future
                tracing::info!("传输已中断（连接断开）: task_id={}", task_id);
                return Err("传输已中断".to_string());
            }
            None => return Err("任务已删除".to_string()),
            _ => continue, // 继续等待（paused 状态）
        }
    }
}

/* ── 文件流处理 ─────────────────────────────────────────── */

/// 文件读取器（用于上传）
///
/// 使用 `Arc<std::sync::Mutex<BufReader<File>>>` 持有底层 reader，
/// 使得 `read_next_chunk` 可通过 `tokio::task::spawn_blocking` 异步读取，
/// 避免同步文件 IO 阻塞 tokio worker。
pub struct FileReader {
    reader: Arc<std::sync::Mutex<BufReader<File>>>,
    file_size: u64,
    pub transferred: u64,
    chunk_size: u32,
}

impl FileReader {
    /// 创建文件读取器
    pub fn new(path: &str) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|e| format!("无法打开文件: {}", e))?;
        let metadata = file.metadata()
            .map_err(|e| format!("无法获取文件元数据: {}", e))?;
        let file_size = metadata.len();

        Ok(Self {
            reader: Arc::new(std::sync::Mutex::new(BufReader::new(file))),
            file_size,
            transferred: 0,
            chunk_size: 256 * 1024,  // 256KB(与服务端一致,减少帧数与 syscall 开销)
        })
    }

    /// 创建文件读取器(指定 offset 范围,用于多流并行上传)
    ///
    /// 从 `offset_start` 开始读取,直到 `offset_end`(不含)。
    /// 内部 `transferred` 从 0 开始计数,`file_size` 设为段长度,
    /// 使 `read_next_chunk` 无需修改即可只读段内数据。
    pub fn new_with_range(path: &str, offset_start: u64, offset_end: u64) -> Result<Self, String> {
        use std::io::{Seek, SeekFrom};

        let file = File::open(path)
            .map_err(|e| format!("无法打开文件: {}", e))?;
        let mut reader = BufReader::new(file);

        if offset_start > 0 {
            reader.seek(SeekFrom::Start(offset_start))
                .map_err(|e| format!("文件 seek 失败 (offset={}): {}", offset_start, e))?;
        }

        let segment_size = offset_end.saturating_sub(offset_start);

        Ok(Self {
            reader: Arc::new(std::sync::Mutex::new(reader)),
            file_size: segment_size,
            transferred: 0,
            chunk_size: 256 * 1024,
        })
    }

    /// 异步读取下一个数据块
    ///
    /// 使用 `spawn_blocking` 将同步文件读取移出 tokio worker，
    /// 避免大文件读取阻塞异步运行时。
    pub async fn read_next_chunk(&mut self) -> Result<Option<Vec<u8>>, String> {
        if self.transferred >= self.file_size {
            return Ok(None);
        }

        let remaining = self.file_size - self.transferred;
        let read_size = std::cmp::min(self.chunk_size as u64, remaining) as usize;

        let reader = self.reader.clone();
        let chunk = tokio::task::spawn_blocking(move || -> Result<Option<Vec<u8>>, std::io::Error> {
            let mut reader = reader.lock().unwrap();
            let mut buffer = vec![0u8; read_size];
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                Ok(None)
            } else {
                buffer.truncate(bytes_read);
                Ok(Some(buffer))
            }
        })
        .await
        .map_err(|e| format!("读取任务 panic: {}", e))?
        .map_err(|e| format!("读取文件失败: {}", e))?;

        if let Some(data) = &chunk {
            self.transferred += data.len() as u64;
        }

        Ok(chunk)
    }

    /// 获取进度百分比 (0-100)
    #[allow(dead_code)]
    pub fn progress(&self) -> u32 {
        if self.file_size == 0 {
            return 100;
        }
        (self.transferred as f64 / self.file_size as f64 * 100.0) as u32
    }
}

/// 文件写入器（用于下载）
///
/// 使用原子写入机制：先写入临时文件，完成后原子重命名到最终路径
/// 这样可以防止中断传输导致的不完整文件问题
pub struct FileWriter {
    writer: BufWriter<File>,
    file_size: u64,
    pub transferred: u64,
    /// 标记是否为临时文件（未完成传输）
    is_temporary: bool,
    /// 标记是否保留临时文件（取消/暂停时保留，用于断点续传）
    preserved: bool,
    /// 最终文件路径
    path: PathBuf,
    /// 临时文件路径（随机命名: `{原文件名}.{随机hex}.tmp`）
    temp_path: PathBuf,
}

impl FileWriter {
    /// 创建文件写入器（使用临时文件）
    ///
    /// # 参数
    /// - `path`: 最终文件路径
    /// - `file_size`: 文件总大小（字节）
    ///
    /// # 返回
    /// 成功返回 FileWriter，失败返回错误信息
    #[allow(dead_code)]
    pub fn new(path: &str, file_size: u64) -> Result<Self, String> {
        Self::with_resume(path, file_size, 0)
    }

    /// 创建支持断点续传的文件写入器
    ///
    /// # 参数
    /// - `path`: 最终文件路径
    /// - `file_size`: 文件总大小（字节）
    /// - `resume_from`: 从哪个字节开始写入（0 = 从头开始）
    ///
    /// # 断点续传逻辑
    /// - 如果 `resume_from` > 0，尝试打开已存在的临时文件并跳转到指定位置
    /// - 验证临时文件完整性：
    ///   1. 文件大小是否与 `resume_from` 一致
    ///   2. 确保已有数据已落盘（sync_all）
    /// - 如果临时文件不存在、大小不匹配或数据不完整，创建新文件并忽略 `resume_from`
    ///
    /// # 返回
    /// 成功返回 FileWriter，失败返回错误信息
    pub fn with_resume(path: &str, file_size: u64, resume_from: u64) -> Result<Self, String> {
        let path = PathBuf::from(path);

        // 确定临时文件路径：
        // - 断点续传时查找已存在的临时文件（格式: `{原文件名}.*.tmp`）
        // - 新传输时生成随机临时文件路径（格式: `{原文件名}.{随机hex}.tmp`）
        let temp_path = if resume_from > 0 {
            find_existing_temp_file(&path).unwrap_or_else(|| generate_temp_path(&path))
        } else {
            generate_temp_path(&path)
        };

        tracing::info!(
            "[FileWriter] 创建文件写入器:\n  最终路径: {} ({} 字符)\n  临时路径: {} ({} 字符)",
            path.display(),
            path.to_string_lossy().len(),
            temp_path.display(),
            temp_path.to_string_lossy().len()
        );

        // 检查路径长度是否超过 Windows 限制
        #[cfg(windows)]
        {
            let path_str = path.to_string_lossy();
            let temp_str = temp_path.to_string_lossy();
            if path_str.len() > 260 || temp_str.len() > 260 {
                tracing::warn!(
                    "[FileWriter] 路径长度超过 Windows 限制 (260 字符):\n  最终路径: {} 字符\n  临时路径: {} 字符",
                    path_str.len(),
                    temp_str.len()
                );
            }
        }

        // 尝试断点续传
        let (file, actual_resume_from) = if resume_from > 0 {
            if temp_path.exists() {
                // 验证临时文件完整性
                match Self::verify_temp_file_integrity(&temp_path, resume_from) {
                    Ok(mut existing_file) => {
                        // 跳转到断点位置
                        match existing_file.seek(SeekFrom::Start(resume_from)) {
                            Ok(_) => {
                                tracing::debug!(
                                    resume_from,
                                    temp_path = %temp_path.display(),
                                    "断点续传: 继续写入临时文件"
                                );
                                (BufWriter::new(existing_file), resume_from)
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    "跳转到断点位置失败，降级为重新传输"
                                );
                                Self::create_fresh_temp_file(&temp_path)?
                            }
                        }
                    }
                    Err(reason) => {
                        tracing::warn!(
                            reason = %reason,
                            "临时文件完整性校验失败，降级为重新传输"
                        );
                        Self::create_fresh_temp_file(&temp_path)?
                    }
                }
            } else {
                tracing::debug!(
                    temp_path = %temp_path.display(),
                    "临时文件不存在，从头开始传输"
                );
                Self::create_fresh_temp_file(&temp_path)?
            }
        } else {
            // resume_from == 0，但可能有保留的临时文件（之前的下载被取消/删除）
            if temp_path.exists() {
                let existing_size = temp_path.metadata().map(|m| m.len()).unwrap_or(0);
                if existing_size > 0 {
                    tracing::debug!(
                        temp_path = %temp_path.display(),
                        existing_size,
                        "发现保留的临时文件，尝试断点续传"
                    );
                    // 打开已有文件，追加模式
                    let file = std::fs::OpenOptions::new()
                        .write(true)
                        .append(true)
                        .open(&temp_path)
                        .map_err(|e| format!("打开保留的临时文件失败: {}", e))?;
                    (BufWriter::new(file), existing_size)
                } else {
                    Self::create_fresh_temp_file(&temp_path)?
                }
            } else {
                Self::create_fresh_temp_file(&temp_path)?
            }
        };

        // 如果文件已有数据，标记为 preserved（防止 Drop 删除）
        let has_existing_data = actual_resume_from > 0;

        Ok(Self {
            writer: file,
            file_size,
            transferred: actual_resume_from,
            is_temporary: true,
            preserved: has_existing_data,
            path,
            temp_path,
        })
    }

    /// 验证临时文件完整性（用于断点续传）
    ///
    /// # 验证步骤
    /// 1. 打开现有临时文件（读写模式）
    /// 2. 获取文件大小并验证与 `expected_size` 匹配
    /// 3. sync_all 确保之前写入的数据完全落盘
    /// 4. 再次读取大小确认一致性
    ///
    /// # 返回
    /// - `Ok(File)`: 验证通过，返回可用于继续写入的文件句柄
    /// - `Err(String)`: 验证失败的原因
    fn verify_temp_file_integrity(
        temp_path: &PathBuf,
        expected_size: u64,
    ) -> Result<File, String> {
        // 打开现有临时文件（读写模式，不截断）
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(temp_path)
            .map_err(|e| format!("无法打开临时文件 {:?}: {}", temp_path, e))?;

        // 获取文件大小
        let metadata = file
            .metadata()
            .map_err(|e| format!("无法获取临时文件元数据: {}", e))?;
        let temp_size = metadata.len();

        // 验证文件大小是否匹配断点位置
        if temp_size != expected_size {
            return Err(format!(
                "临时文件大小不匹配: 实际 {} 字节, 期望 {} 字节",
                temp_size, expected_size
            ));
        }

        // sync 确保之前写入的数据完全落盘
        // 即使之前进程异常退出导致部分数据还在内核缓冲区，
        // sync 也会将其写入磁盘，避免续传后数据不一致
        file.sync_all()
            .map_err(|e| format!("同步临时文件到磁盘失败: {}", e))?;

        // 再次读取大小确认一致性（排除 sync 期间的并发修改）
        let post_sync_metadata = file
            .metadata()
            .map_err(|e| format!("同步后重新获取文件元数据失败: {}", e))?;
        if post_sync_metadata.len() != expected_size {
            return Err(format!(
                "同步后临时文件大小发生变化: {} -> {}",
                temp_size,
                post_sync_metadata.len()
            ));
        }

        tracing::debug!(
            temp_path = %temp_path.display(),
            expected_size,
            "临时文件完整性校验通过"
        );

        Ok(file)
    }

    /// 创建或打开临时文件
    ///
    /// 如果文件已存在（保留的断点续传文件），以追加模式打开，不截断数据。
    /// 如果文件不存在，创建新文件。
    fn create_fresh_temp_file(temp_path: &PathBuf) -> Result<(BufWriter<File>, u64), String> {
        let exists = temp_path.exists();
        tracing::debug!(
            temp_path = %temp_path.display(),
            exists,
            "{}临时文件",
            if exists { "打开已有" } else { "创建新" }
        );

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(!exists)  // 仅在文件不存在时截断
            .open(temp_path)
            .map_err(|e| format!("无法创建临时文件: {:?}\n错误: {}", temp_path, e))?;

        // 如果文件已存在，获取已有数据大小
        let existing_size = if exists {
            file.metadata().map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        if existing_size > 0 {
            tracing::debug!(existing_size, "断点续传: 已有数据");
            // 追加到文件末尾
            file.seek(std::io::SeekFrom::End(0))
                .map_err(|e| format!("定位到文件末尾失败: {}", e))?;
        }

        Ok((BufWriter::new(file), existing_size))
    }

    /// 写入数据块
    pub fn write_chunk(&mut self, data: &[u8]) -> Result<(), String> {
        self.writer.write_all(data)
            .map_err(|e| format!("写入文件失败: {}", e))?;
        self.transferred += data.len() as u64;
        Ok(())
    }

    /// 完成写入（原子操作）
    ///
    /// # 操作流程
    /// 1. 刷新缓冲区到磁盘
    /// 2. 同步文件元数据到磁盘
    /// 3. 标记为完成（防止 Drop 删除临时文件）
    /// 4. 原子重命名：临时文件 -> 最终文件
    pub fn finish(&mut self) -> Result<(), String> {
        // 刷新缓冲区
        self.writer.flush()
            .map_err(|e| format!("刷新文件失败: {}", e))?;

        // 同步到磁盘
        self.writer.get_ref().sync_all()
            .map_err(|e| format!("同步文件失败: {}", e))?;

        // 标记为完成，防止 Drop 删除
        self.is_temporary = false;

        // 检查临时文件是否存在
        if !std::path::Path::new(&self.temp_path).exists() {
            let temp_str = self.temp_path.to_string_lossy();
            let path_str = self.path.to_string_lossy();
            let error = format!(
                "临时文件不存在: {} (长度: {} 字符)\n最终路径: {} (长度: {} 字符)",
                temp_str, temp_str.len(), path_str, path_str.len()
            );
            tracing::error!("{}", error);
            return Err(error);
        }

        // 原子重命名：临时文件 -> 最终文件
        std::fs::rename(&self.temp_path, &self.path)
            .map_err(|e| {
                tracing::error!(
                    "重命名文件失败: {} -> {}\n错误: {}",
                    self.temp_path.display(),
                    self.path.display(),
                    e
                );
                format!("重命名文件失败: {}", e)
            })?;

        tracing::info!("文件重命名成功: {} -> {}", self.temp_path.display(), self.path.display());
        Ok(())
    }

    /// 标记文件为已完成（不是临时文件）
    pub fn mark_completed(&mut self) {
        self.is_temporary = false;
    }

    /// 保留临时文件（取消/暂停时调用）
    ///
    /// 防止 Drop 删除临时文件，以便后续断点续传
    pub fn preserve(&mut self) {
        self.preserved = true;
        // 刷新缓冲区确保数据落盘
        let _ = self.writer.flush();
        let _ = self.writer.get_ref().sync_all();
    }

    /// 获取进度百分比 (0-100)
    #[allow(dead_code)]
    pub fn progress(&self) -> u32 {
        if self.file_size == 0 {
            return 100;
        }
        (self.transferred as f64 / self.file_size as f64 * 100.0) as u32
    }
}

impl Drop for FileWriter {
    fn drop(&mut self) {
        // 只在错误时清理临时文件
        // 取消/暂停时保留临时文件，以便后续断点续传
        if self.is_temporary && !self.preserved {
            tracing::debug!(temp_path = %self.temp_path.display(), "清理临时文件");
            let _ = std::fs::remove_file(&self.temp_path);
        } else if self.preserved {
            tracing::debug!(temp_path = %self.temp_path.display(), "保留临时文件（用于断点续传）");
        }
    }
}

/// Tauri Command: 判断本地路径是否为普通文件
///
/// 拖拽上传事件只提供路径（不区分文件/目录），目录无法按文件传输，
/// 前端在创建传输任务前调用此命令过滤掉目录与不存在的路径
#[command]
pub fn local_path_is_file(path: String) -> bool {
    std::fs::metadata(&path).map(|m| m.is_file()).unwrap_or(false)
}

/// Tauri Command: 开始文件传输
///
/// # 参数
/// - `server_id`: 服务器 ID
/// - `direction`: 传输方向 ("upload" 或 "download")
/// - `remote_path`: 远程文件路径
/// - `local_path`: 本地文件路径
///
/// # 返回
/// 成功返回任务 ID，失败返回错误信息
#[command]
pub async fn transfer_file(
    server_id: String,
    direction: String,
    remote_path: String,
    local_path: String,
    app_handle: AppHandle,
) -> Result<String, String> {
    tracing::info!(
        "文件传输请求: server_id={}, direction={}, remote={}, local={}",
        server_id, direction, remote_path, local_path
    );

    // 验证参数
    if direction != "upload" && direction != "download" {
        return Err(format!("无效的传输方向: {}", direction));
    }

    // 获取全局 TransferManager
    let manager = app_handle.state::<Arc<TransferManager>>();
    let manager = Arc::clone(&manager);  // 克隆 Arc，共享状态

    // 创建任务
    tracing::info!(server_id, direction, "开始创建传输任务");
    let task_id = manager
        .create_task(server_id.clone(), direction.clone(), remote_path.clone(), local_path.clone())
        .await?;
    tracing::info!(task_id, "任务创建成功");

    // 获取连接管理器和 QUIC Connection
    let connection_manager = app_handle.state::<ConnectionManager>();
    
    let quic_conn = {
        let conns = connection_manager.connections.lock().unwrap();
        
        let active_conn = conns.get(&server_id)
            .ok_or_else(|| {
                tracing::warn!(server_id, "服务器未连接");
                format!("服务器未连接: {}", server_id)
            })?;
        
        active_conn.quic_conn.clone()
    };

    // 如果没有 QUIC Connection，返回错误
    let conn = quic_conn.ok_or_else(|| {
        tracing::error!(server_id, "QUIC Connection 为 None");
        "未找到 QUIC Connection，可能只使用了 WebSocket 连接".to_string()
    })?;

    // 获取请求 ID
    let request_id = connection_manager.next_request_id();

    // 在后台任务中执行传输
    let task_id_clone = task_id.clone();
    let manager_clone = Arc::clone(&manager);
    let direction_clone = direction.clone();
    let remote_path_clone = remote_path.clone();
    let local_path_clone = local_path.clone();

    tracing::info!(
        task_id = %task_id_clone,
        direction = %direction_clone,
        remote = %remote_path_clone,
        "启动后台传输任务"
    );

    tokio::spawn(async move {
        tracing::info!(task_id = %task_id_clone, "后台任务开始执行");

        let result = perform_transfer(
            &conn,
            request_id,
            direction_clone,
            remote_path_clone,
            local_path_clone,
            Arc::clone(&manager_clone),
            &task_id_clone,
            None,  // 新任务从头开始传输
        ).await;

        tracing::info!(task_id = %task_id_clone, ?result, "后台任务执行完成");

        if let Err(e) = result {
            tracing::error!(task_id = %task_id_clone, error = %e, "文件传输失败");
            let _ = manager_clone.mark_failed(&task_id_clone, e).await;
        }
    });

    Ok(task_id)
}

/// 文件传输 Stream 操作超时时间（300秒 / 5分钟）
/// 文件传输可能涉及大文件，使用更宽松的超时
const FILE_TRANSFER_STREAM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// 文件传输单块操作超时时间（60秒）
/// 单个数据块的读写操作不应超过此时间
const FILE_CHUNK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// 执行文件传输的内部函数（支持断点续传）
///
/// # 参数
/// - `conn`: QUIC 连接
/// - `request_id`: 请求 ID
/// - `direction`: 传输方向（"upload" 或 "download"）
/// - `remote_path`: 远程文件路径
/// - `local_path`: 本地文件路径
/// - `manager`: 传输管理器
/// - `task_id`: 任务 ID
/// - `resume_from`: 断点续传位置（可选，从哪个字节开始）
/// 根据文件大小计算多流并行数
///
/// - `< 10MB`: 1 流(单流,避免开销)
/// - `10MB-100MB`: 2 流
/// - `≥ 100MB`: 4 流
fn compute_stream_count(file_size: u64) -> u32 {
    if file_size >= 100 * 1024 * 1024 {
        4
    } else if file_size >= 10 * 1024 * 1024 {
        2
    } else {
        1
    }
}

async fn perform_transfer(
    conn: &quinn::Connection,
    request_id: u32,
    direction: String,
    remote_path: String,
    local_path: String,
    manager: Arc<TransferManager>,
    task_id: &str,
    resume_from: Option<u64>,
) -> Result<(), String> {
    tracing::info!(
        task_id, direction, remote = %remote_path, local = %local_path, ?resume_from,
        "开始执行文件传输"
    );

    // 计算多流并行数(仅上传,基于文件大小)
    let stream_count = if direction == "upload" {
        let local_size = std::fs::metadata(&local_path)
            .map(|m| m.len())
            .unwrap_or(0);
        compute_stream_count(local_size)
    } else {
        1
    };

    // ── 1. 握手：发送请求 + 接收响应 ──
    let (session_id, file_size, frame_mode, mut send, mut recv) =
        handshake_transfer(conn, request_id, &direction, &remote_path, &local_path, resume_from, stream_count)
            .await?;

    // 保存 Agent 端的 session_id
    manager.set_agent_session_id(task_id, session_id.clone()).await;

    // 下载时更新文件大小
    if direction == "download" {
        manager.update_file_size(task_id, file_size).await?;
    }

    // ── 2. 执行传输 ──
    if direction == "upload" {
        if stream_count > 1 {
            run_multi_stream_upload(
                conn, &session_id, &local_path, file_size, stream_count,
                Arc::clone(&manager), task_id, send, recv,
            ).await
        } else {
            run_upload_loop(&mut send, &session_id, &local_path, &manager, task_id, &frame_mode, None, None).await
        }
    } else {
        run_download_loop(&mut recv, &session_id, &local_path, file_size, resume_from, &manager, task_id, &frame_mode).await
    }
}

/// 传输握手：发送 FileTransferRequest + 接收 FileTransferAccept
///
/// 返回 (session_id, file_size, frame_mode, send_stream, recv_stream)
async fn handshake_transfer(
    conn: &quinn::Connection,
    request_id: u32,
    direction: &str,
    remote_path: &str,
    local_path: &str,
    resume_from: Option<u64>,
    stream_count: u32,
) -> Result<(String, u64, String, quinn::SendStream, quinn::RecvStream), String> {
    // 获取文件大小（上传时）
    let file_size = if direction == "upload" {
        Some(std::fs::metadata(local_path)
            .map_err(|e| format!("无法访问本地文件: {}", e))?
            .len())
    } else {
        None
    };

    let request_payload = Payload::FileTransferRequest {
        direction: direction.to_string(),
        path: remote_path.to_string(),
        file_size,
        chunk_size: Some(256 * 1024),
        resume_from,
        frame_mode: "raw".to_string(),
        stream_count: Some(stream_count),
    };

    // 创建 Stream
    let (mut send, mut recv) = tokio::time::timeout(FILE_TRANSFER_STREAM_TIMEOUT, conn.open_bi())
        .await
        .map_err(|_| "打开文件传输 Stream 超时".to_string())?
        .map_err(|e| format!("打开 Stream 失败: {}", e))?;

    // 发送请求
    let request_envelope = Envelope::new(request_id, request_payload);
    let request_bytes = request_envelope.encode()?;
    let request_len = (request_bytes.len() as u32).to_le_bytes();

    use tokio::io::AsyncWriteExt;
    tokio::time::timeout(FILE_CHUNK_TIMEOUT, async {
        send.write_all(&request_len).await
            .map_err(|e| format!("发送请求长度失败: {}", e))?;
        send.write_all(&request_bytes).await
            .map_err(|e| format!("发送请求数据失败: {}", e))?;
        send.flush().await
            .map_err(|e| format!("刷新发送流失败: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| "发送文件传输请求超时".to_string())??;

    // 接收响应
    let response_buf = tokio::time::timeout(FILE_CHUNK_TIMEOUT, async {
        let mut response_len_buf = [0u8; 4];
        recv.read_exact(&mut response_len_buf).await
            .map_err(|e| format!("读取响应长度失败: {}", e))?;
        let response_len = u32::from_le_bytes(response_len_buf) as usize;

        let mut response_buf = vec![0u8; response_len];
        recv.read_exact(&mut response_buf).await
            .map_err(|e| format!("读取响应数据失败: {}", e))?;
        Ok::<Vec<u8>, String>(response_buf)
    })
    .await
    .map_err(|_| "接收文件传输响应超时".to_string())??;

    let response_envelope = Envelope::decode(&response_buf)?;

    match response_envelope.payload {
        Payload::FileTransferAccept { session_id, file_size, frame_mode, .. } => {
            tracing::info!(
                session_id, file_size, frame_mode,
                "文件传输已接受"
            );
            Ok((session_id, file_size, frame_mode, send, recv))
        }
        Payload::Error { message, .. } => {
            Err(format!("Agent 返回错误: {}", message))
        }
        _ => {
            Err("Agent 返回了意外的响应类型".to_string())
        }
    }
}

/// 上传循环：读取本地文件 → 发送数据帧/块 → 发送 Complete
///
/// - `range`: 多流并行时指定段范围 `(offset_start, offset_end)`,`None` 则读整个文件
/// - `total_transferred`: 多流并行时的共享进度计数器,`None` 则用单流进度
async fn run_upload_loop(
    send: &mut quinn::SendStream,
    session_id: &str,
    local_path: &str,
    manager: &TransferManager,
    task_id: &str,
    frame_mode: &str,
    range: Option<(u64, u64)>,
    total_transferred: Option<Arc<std::sync::atomic::AtomicU64>>,
) -> Result<(), String> {
    let raw_mode = frame_mode == "raw";
    let mut reader = match range {
        Some((start, end)) => FileReader::new_with_range(local_path, start, end)?,
        None => FileReader::new(local_path)?,
    };
    let mut seq = 1u32;
    let start_time = std::time::Instant::now();

    while let Some(chunk) = reader.read_next_chunk().await? {
        // 检查任务状态
        if let Err(e) = wait_if_paused(manager, task_id).await {
            return Err(e);
        }
        check_cancelled_or_deleted(manager, task_id).await?;

        if raw_mode {
            // ===== 裸帧模式: 直接写二进制数据帧 =====
            tokio::time::timeout(FILE_CHUNK_TIMEOUT, write_data_frame(send, seq, &chunk))
                .await
                .map_err(|_| format!("发送数据块超时 ({}s)", FILE_CHUNK_TIMEOUT.as_secs()))??;
        } else {
            // ===== JSON 模式(旧端回退) =====
            let chunk_payload = Payload::FileChunk {
                session_id: session_id.to_string(),
                seq,
                data: chunk.clone(),
                size: chunk.len() as u32,
            };

            let chunk_request_id = uuid::Uuid::new_v4().as_u128() as u32;
            let chunk_envelope = Envelope::new(chunk_request_id, chunk_payload);
            let chunk_bytes = chunk_envelope.encode()?;
            let chunk_len = (chunk_bytes.len() as u32).to_le_bytes();

            tokio::time::timeout(FILE_CHUNK_TIMEOUT, async {
                send.write_all(&chunk_len).await
                    .map_err(|e| format!("发送块长度失败: {}", e))?;
                send.write_all(&chunk_bytes).await
                    .map_err(|e| format!("发送块数据失败: {}", e))?;
                Ok::<(), String>(())
            })
            .await
            .map_err(|_| format!("发送数据块超时 ({}s)", FILE_CHUNK_TIMEOUT.as_secs()))??;
        }

        seq += 1;

        // 更新进度
        let elapsed = start_time.elapsed().as_secs();
        if let Some(ref total) = total_transferred {
            // 多流模式:累加到共享计数器,报告总和
            total.fetch_add(chunk.len() as u64, std::sync::atomic::Ordering::Relaxed);
            let total_val = total.load(std::sync::atomic::Ordering::Relaxed);
            let speed_bps = if elapsed > 0 { total_val / elapsed } else { 0 };
            manager.update_progress(task_id, total_val, speed_bps).await?;
        } else {
            // 单流模式:用 reader.transferred
            let speed_bps = if elapsed > 0 { reader.transferred / elapsed } else { 0 };
            manager.update_progress(task_id, reader.transferred, speed_bps).await?;
        }
    }

    // 发送 FileTransferComplete
    send_complete_message(send, session_id, raw_mode).await?;

    // 单流模式才标记完成(多流由 run_multi_stream_upload 统一标记)
    if total_transferred.is_none() {
        manager.mark_completed(task_id).await?;
    }
    Ok(())
}

/// 计算多流并行各段 offset 范围(等分 file_size 为 N 段)
fn compute_part_offsets(file_size: u64, stream_count: u32) -> Vec<(u64, u64)> {
    let n = stream_count as u64;
    let part = file_size / n;
    let mut offsets = Vec::with_capacity(stream_count as usize);
    let mut cur = 0u64;
    for i in 0..stream_count as u64 {
        let start = cur;
        let end = if i == n - 1 { file_size } else { cur + part };
        offsets.push((start, end));
        cur = end;
    }
    offsets
}

/// 多流并行上传
///
/// 1. 等分 file_size 为 N 段,计算各段 offset
/// 2. 主 stream (index 0): 用握手已有的 send/recv,run_upload_loop 上传段 0
/// 3. 非主 stream (1..N-1): 各开 open_bi → MultiStreamJoin → ACK → run_upload_loop
/// 4. 所有 stream 并发上传(tokio::spawn)
/// 5. 等所有段上传完成 → 主 stream recv 等待 MultiStreamMergeComplete
async fn run_multi_stream_upload(
    conn: &quinn::Connection,
    session_id: &str,
    local_path: &str,
    file_size: u64,
    stream_count: u32,
    manager: Arc<TransferManager>,
    task_id: &str,
    mut primary_send: quinn::SendStream,
    mut primary_recv: quinn::RecvStream,
) -> Result<(), String> {
    use std::sync::atomic::AtomicU64;
    use tokio::io::AsyncWriteExt;

    // 1. 计算各段 offset
    let part_offsets = compute_part_offsets(file_size, stream_count);
    tracing::info!(task_id, stream_count, ?part_offsets, "多流上传开始");

    // 2. 共享进度计数器
    let total_transferred = Arc::new(AtomicU64::new(0));

    // 3. 主 stream (index 0) 上传任务
    let primary_range = part_offsets[0];
    let primary_session_id = session_id.to_string();
    let primary_local_path = local_path.to_string();
    let primary_task_id = task_id.to_string();
    let primary_total = Arc::clone(&total_transferred);
    let primary_manager = Arc::clone(&manager);
    let primary_handle = tokio::spawn(async move {
        run_upload_loop(
            &mut primary_send,
            &primary_session_id,
            &primary_local_path,
            &primary_manager,
            &primary_task_id,
            "raw",
            Some(primary_range),
            Some(primary_total),
        ).await
    });

    // 4. 非主 stream (1..N-1): open_bi → MultiStreamJoin → ACK → spawn upload
    let mut secondary_handles: Vec<tokio::task::JoinHandle<Result<(), String>>> = Vec::new();
    for stream_index in 1..stream_count {
        let range = part_offsets[stream_index as usize];

        // 开新 bi-stream
        let (mut send, mut recv) = tokio::time::timeout(
            FILE_TRANSFER_STREAM_TIMEOUT,
            conn.open_bi(),
        ).await
        .map_err(|_| format!("打开 stream {} 超时", stream_index))?
        .map_err(|e| format!("打开 stream {} 失败: {}", stream_index, e))?;

        // 发送 MultiStreamJoin (握手用 4字节len + JSON,和单流 handshake_transfer 一致)
        // 不能用 write_control_frame(5字节 raw 帧),agent 端 read_message 期望 4字节len + JSON
        let join_payload = Payload::MultiStreamJoin {
            session_id: session_id.to_string(),
            stream_index,
            offset_start: range.0,
            offset_end: range.1,
        };
        let join_env = Envelope::new(0, join_payload);
        let join_bytes = join_env.encode()?;
        let join_len = (join_bytes.len() as u32).to_le_bytes();
        send.write_all(&join_len).await.map_err(|e| format!("stream {} 发送 join 长度失败: {}", stream_index, e))?;
        send.write_all(&join_bytes).await.map_err(|e| format!("stream {} 发送 join 数据失败: {}", stream_index, e))?;
        send.flush().await.map_err(|e| format!("stream {} flush join 失败: {}", stream_index, e))?;

        // 接收 ACK (4字节len + JSON,和 agent write_message 一致)
        let mut ack_len_buf = [0u8; 4];
        recv.read_exact(&mut ack_len_buf).await
            .map_err(|e| format!("stream {} 读 ACK 长度失败: {}", stream_index, e))?;
        let ack_len = u32::from_le_bytes(ack_len_buf) as usize;
        let mut ack_body = vec![0u8; ack_len];
        recv.read_exact(&mut ack_body).await
            .map_err(|e| format!("stream {} 读 ACK 数据失败: {}", stream_index, e))?;
        let ack_env = Envelope::decode(&ack_body)?;
        match &ack_env.payload {
            Payload::FileTransferAccept { .. } => {
                tracing::info!(stream_index, "多流 stream 加入成功");
            }
            Payload::Error { message, .. } => {
                return Err(format!("stream {} 加入被拒: {}", stream_index, message));
            }
            _ => return Err(format!("stream {} 收到意外响应", stream_index)),
        }

        // 启动段上传任务
        let sec_session_id = session_id.to_string();
        let sec_local_path = local_path.to_string();
        let sec_task_id = task_id.to_string();
        let sec_total = Arc::clone(&total_transferred);
        let sec_manager = Arc::clone(&manager);
        let handle = tokio::spawn(async move {
            run_upload_loop(
                &mut send,
                &sec_session_id,
                &sec_local_path,
                &sec_manager,
                &sec_task_id,
                "raw",
                Some(range),
                Some(sec_total),
            ).await
        });
        secondary_handles.push(handle);
    }

    // 5. 等待主 stream 上传完成
    let primary_result = primary_handle.await
        .map_err(|e| format!("主 stream 任务 panic: {}", e))?;

    // 6. 等待所有非主 stream 完成
    let mut all_ok = primary_result.is_ok();
    for handle in secondary_handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::error!("非主 stream 上传失败: {}", e);
                all_ok = false;
            }
            Err(e) => {
                tracing::error!("非主 stream 任务 panic: {}", e);
                all_ok = false;
            }
        }
    }

    if !all_ok {
        let _ = manager.mark_failed(task_id, "部分 stream 上传失败".to_string()).await;
        return Err("部分 stream 上传失败".to_string());
    }

    // 7. 等待 MultiStreamMergeComplete (主 stream recv)
    tracing::info!(task_id, "所有段上传完成,等待合并结果");
    // 合并结果用 4字节len + JSON(和 agent write_message 一致,不是 raw 帧)
    let mut merge_len_buf = [0u8; 4];
    primary_recv.read_exact(&mut merge_len_buf).await
        .map_err(|e| format!("读取合并结果长度失败: {}", e))?;
    let merge_len = u32::from_le_bytes(merge_len_buf) as usize;
    let mut merge_body = vec![0u8; merge_len];
    primary_recv.read_exact(&mut merge_body).await
        .map_err(|e| format!("读取合并结果数据失败: {}", e))?;
    let merge_env = Envelope::decode(&merge_body)?;

    match merge_env.payload {
        Payload::MultiStreamMergeComplete { success, error, .. } => {
            if success {
                tracing::info!(task_id, "多流合并完成");
                manager.mark_completed(task_id).await?;
                Ok(())
            } else {
                let err_msg = error.unwrap_or_else(|| "合并失败".to_string());
                let _ = manager.mark_failed(task_id, err_msg.clone()).await;
                Err(err_msg)
            }
        }
        _ => Err("期望 MultiStreamMergeComplete".to_string()),
    }
}

/// 下载循环：接收数据帧/块 → 写入临时文件 → 收到 Complete 后重命名
async fn run_download_loop(
    recv: &mut quinn::RecvStream,
    _session_id: &str,
    local_path: &str,
    file_size: u64,
    resume_from: Option<u64>,
    manager: &TransferManager,
    task_id: &str,
    frame_mode: &str,
) -> Result<(), String> {
    let raw_mode = frame_mode == "raw";
    let resume_pos = resume_from.unwrap_or(0);
    let mut writer = FileWriter::with_resume(local_path, file_size, resume_pos)?;
    tracing::info!(
        temp_path = %writer.temp_path.display(),
        final_path = %writer.path.display(),
        transferred = writer.transferred,
        frame_mode,
        "FileWriter 创建成功"
    );

    let start_time = std::time::Instant::now();

    loop {
        // 检查任务状态
        if let Err(e) = wait_if_paused(manager, task_id).await {
            writer.preserve();
            return Err(e);
        }
        if let Err(e) = check_cancelled_or_deleted(manager, task_id).await {
            writer.preserve();
            return Err(e);
        }

        if raw_mode {
            // ===== 裸帧模式: [4B len][1B type][body] =====
            // 帧类型枚举: Data(文件数据) | Control(FileTransferComplete) | 其他
            enum RawFrame {
                Data(Vec<u8>),
                Control(Vec<u8>),
            }

            let frame_result = tokio::time::timeout(FILE_CHUNK_TIMEOUT, async {
                let (type_byte, body_len) = read_frame_header(recv).await?;
                match type_byte {
                    RAW_TYPE_DATA => {
                        let (_seq, data) = read_data_body(recv, body_len).await?;
                        Ok::<RawFrame, String>(RawFrame::Data(data))
                    }
                    RAW_TYPE_CONTROL => {
                        let body = read_control_body(recv, body_len).await?;
                        Ok(RawFrame::Control(body))
                    }
                    _ => Err(format!("未知帧类型: {}", type_byte)),
                }
            })
            .await;

            match frame_result {
                Ok(Ok(RawFrame::Data(data))) => {
                    writer.write_chunk(&data)?;
                    let elapsed = start_time.elapsed().as_secs();
                    let speed_bps = if elapsed > 0 { writer.transferred / elapsed } else { 0 };
                    manager.update_progress(task_id, writer.transferred, speed_bps).await?;
                }
                Ok(Ok(RawFrame::Control(body))) => {
                    let envelope = Envelope::decode(&body)?;
                    match envelope.payload {
                        Payload::FileTransferComplete { success, error, .. } => {
                            if success {
                                writer.finish()?;
                                writer.mark_completed();
                                manager.mark_completed(task_id).await?;
                                tracing::info!("下载完成并重命名成功(裸帧)");
                            } else {
                                manager.mark_failed(
                                    task_id,
                                    error.unwrap_or_else(|| "未知错误".to_string())
                                ).await?;
                            }
                            break;
                        }
                        _ => {
                            tracing::warn!("收到意外的控制帧 payload");
                        }
                    }
                }
                Ok(Err(e)) => {
                    manager.mark_failed(task_id, format!("读取数据帧失败: {}", e)).await?;
                    return Err(format!("读取数据帧失败: {}", e));
                }
                Err(_) => {
                    manager.mark_failed(task_id, "读取数据帧超时".to_string()).await?;
                    return Err("读取数据帧超时".to_string());
                }
            }
        } else {
            // ===== JSON 模式(旧端回退) =====
            let read_result = tokio::time::timeout(FILE_CHUNK_TIMEOUT, async {
                let mut chunk_len_buf = [0u8; 4];
                recv.read_exact(&mut chunk_len_buf).await
                    .map_err(|e| format!("读取块长度失败: {}", e))?;
                let chunk_len = u32::from_le_bytes(chunk_len_buf) as usize;
                let mut chunk_buf = vec![0u8; chunk_len];
                recv.read_exact(&mut chunk_buf).await
                    .map_err(|e| format!("读取块数据失败: {}", e))?;
                Ok::<Vec<u8>, String>(chunk_buf)
            })
            .await;

            let chunk_buf = match read_result {
                Ok(Ok(buf)) => buf,
                Ok(Err(e)) => {
                    manager.mark_failed(task_id, format!("读取数据块失败: {}", e)).await?;
                    return Err(format!("读取数据块失败: {}", e));
                }
                Err(_) => {
                    manager.mark_failed(task_id, "读取数据块超时".to_string()).await?;
                    return Err("读取数据块超时".to_string());
                }
            };

            let chunk_envelope = Envelope::decode(&chunk_buf)?;

            match chunk_envelope.payload {
                Payload::FileChunk { data, .. } => {
                    writer.write_chunk(&data)?;
                    let elapsed = start_time.elapsed().as_secs();
                    let speed_bps = if elapsed > 0 { writer.transferred / elapsed } else { 0 };
                    manager.update_progress(task_id, writer.transferred, speed_bps).await?;
                }
                Payload::FileTransferComplete { success, error, .. } => {
                    if success {
                        writer.finish()?;
                        writer.mark_completed();
                        manager.mark_completed(task_id).await?;
                        tracing::info!("下载完成并重命名成功");
                    } else {
                        manager.mark_failed(
                            task_id,
                            error.unwrap_or_else(|| "未知错误".to_string())
                        ).await?;
                    }
                    break;
                }
                Payload::Error { message, .. } => {
                    manager.mark_failed(task_id, message).await?;
                    break;
                }
                _ => {
                    tracing::warn!("收到意外的消息类型");
                }
            }
        }
    }

    // 处理未收到 FileTransferComplete 的情况
    if writer.is_temporary {
        if writer.transferred >= writer.file_size {
            writer.finish()?;
            writer.mark_completed();
            manager.mark_completed(task_id).await?;
        } else {
            manager.mark_failed(task_id, "传输不完整".to_string()).await?;
            return Err("传输不完整".to_string());
        }
    }

    Ok(())
}

/// 检查任务是否已取消或已删除
async fn check_cancelled_or_deleted(
    manager: &TransferManager,
    task_id: &str,
) -> Result<(), String> {
    match manager.get_status(task_id).await {
        Some(TransferStatus::Cancelled) => {
            tracing::info!("传输已取消: task_id={}", task_id);
            Err("传输已取消".to_string())
        }
        None => Err("任务已删除".to_string()),
        _ => Ok(()),
    }
}

/// 发送 FileTransferComplete 消息
async fn send_complete_message(
    send: &mut quinn::SendStream,
    session_id: &str,
    raw_mode: bool,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    let complete_payload = Payload::FileTransferComplete {
        session_id: session_id.to_string(),
        success: true,
        mtime: None,
        error: None,
    };

    let complete_request_id = uuid::Uuid::new_v4().as_u128() as u32;
    let complete_envelope = Envelope::new(complete_request_id, complete_payload);
    let complete_bytes = complete_envelope.encode()?;

    if raw_mode {
        // 裸帧模式: 控制帧承载 JSON Envelope
        tokio::time::timeout(FILE_CHUNK_TIMEOUT, write_control_frame(send, &complete_bytes))
            .await
            .map_err(|_| "发送传输完成消息超时".to_string())??;
        send.flush().await.map_err(|e| format!("刷新发送流失败: {}", e))?;
    } else {
        // JSON 模式: [4B len][JSON bytes]
        let complete_len = (complete_bytes.len() as u32).to_le_bytes();
        tokio::time::timeout(FILE_CHUNK_TIMEOUT, async {
            send.write_all(&complete_len).await
                .map_err(|e| format!("发送完成消息失败: {}", e))?;
            send.write_all(&complete_bytes).await
                .map_err(|e| format!("发送完成数据失败: {}", e))?;
            send.flush().await
                .map_err(|e| format!("刷新发送流失败: {}", e))?;
            Ok::<(), String>(())
        })
        .await
        .map_err(|_| "发送传输完成消息超时".to_string())??;
    }

    Ok(())
}

/// 暂停文件传输
#[command]
pub async fn pause_transfer(task_id: String, app_handle: AppHandle) -> Result<(), String> {
    tracing::info!(task_id, "暂停传输请求");

    // 获取全局 TransferManager
    let manager = app_handle.state::<Arc<TransferManager>>();

    // 调用暂停方法
    manager.pause_task(&task_id).await?;

    Ok(())
}

/// 继续文件传输
#[command]
pub async fn resume_transfer(task_id: String, app_handle: AppHandle) -> Result<(), String> {
    tracing::info!(task_id, "继续传输请求");

    // 获取全局 TransferManager
    let manager = app_handle.state::<Arc<TransferManager>>();

    // 调用继续方法
    manager.resume_task(&task_id).await?;

    Ok(())
}

/// 重试文件传输（支持断点续传）
#[command]
pub async fn retry_transfer(task_id: String, app_handle: AppHandle) -> Result<(), String> {
    tracing::info!(task_id, "重试传输请求");

    // 获取全局 TransferManager
    let manager = app_handle.state::<Arc<TransferManager>>();

    // 调用重试方法（支持断点续传）
    manager.retry_task(&task_id, &app_handle).await?;

    Ok(())
}

/// 取消文件传输
#[command]
pub async fn cancel_transfer(task_id: String, app_handle: AppHandle) -> Result<(), String> {
    tracing::info!(task_id, "取消传输请求");

    // 获取全局 TransferManager
    let manager = app_handle.state::<Arc<TransferManager>>();

    // 调用取消方法（会同时通知 Agent 端清理传输会话）
    manager.cancel_task(&task_id, &app_handle).await?;

    Ok(())
}

/// 查询未完成传输任务数（切换服务器前的中断确认）
#[command]
pub async fn get_active_transfer_count(app_handle: AppHandle) -> Result<u32, String> {
    let manager = app_handle.state::<Arc<TransferManager>>();
    Ok(manager.count_active().await as u32)
}

/// 检查文件是否存在
#[command]
pub async fn check_file_exists(
    server_id: String,
    path: String,
    app_handle: tauri::AppHandle,
) -> Result<FileExistsInfo, String> {
    use crate::connection::{remote_send, Payload};

    tracing::info!(server_id, path, "检查文件是否存在");

    // 发送请求
    let request = Payload::FileExistsRequest { path };
    let response = remote_send(server_id, request, app_handle).await?;

    // 处理响应
    match response.payload {
        Payload::FileExistsResponse { exists, size, mtime } => {
            Ok(FileExistsInfo { exists, size, mtime })
        }
        Payload::Error { message, .. } => {
            Err(format!("Agent 返回错误: {}", message))
        }
        _ => Err("意外的响应类型".to_string()),
    }
}

/// 文件存在信息
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileExistsInfo {
    pub exists: bool,
    pub size: Option<u64>,
    pub mtime: Option<u64>,
}

// ── 单元测试 ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_is_terminal() {
        // interrupted 是终态：可被 5 分钟清理任务回收，不再变更
        assert!(TransferStatus::Interrupted.is_terminal());
    }

    #[test]
    fn interrupted_is_retryable() {
        // interrupted 可重试：切回原服务器后重试
        assert!(TransferStatus::Interrupted.is_retryable());
    }

    #[test]
    fn interrupted_display_lowercase() {
        assert_eq!(TransferStatus::Interrupted.to_string(), "interrupted");
    }

    #[test]
    fn interrupted_serializes_lowercase() {
        // serde 序列化必须与前端 TS 类型字面量一致
        let json = serde_json::to_string(&TransferStatus::Interrupted).unwrap();
        assert_eq!(json, "\"interrupted\"");
    }

    #[test]
    fn active_is_not_terminal() {
        // 回归保护：非终态判断不受影响
        assert!(!TransferStatus::Active.is_terminal());
        assert!(!TransferStatus::Pending.is_terminal());
        assert!(!TransferStatus::Paused.is_terminal());
    }
}
