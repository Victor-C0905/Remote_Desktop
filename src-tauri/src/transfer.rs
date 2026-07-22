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

/// 生成随机临时文件路径
///
/// 使用 UUID v4 生成不可预测的临时文件名，防止符号链接攻击。
/// 文件名格式: `{原文件名}.{8位随机hex}.tmp`
///
/// # 安全性
/// - UUID v4 提供 128 位随机性，截取前 8 个字符（32 位熵）已足够防止预测
/// - 临时文件名包含原文件名，便于调试和清理
fn generate_temp_path(path: &Path) -> PathBuf {
    let random_part = &uuid::Uuid::new_v4().to_string()[..8];

    let temp_name = format!(
        "{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        random_part
    );

    path.parent().unwrap_or(Path::new(".")).join(temp_name)
}

/// 查找已存在的临时文件（用于断点续传）
///
/// 搜索目录中匹配 `{原文件名}.*.tmp` 模式的文件，
/// 返回修改时间最新的一个（兼容旧的确定性 `.tmp` 命名格式）。
fn find_existing_temp_file(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let file_name = path.file_name()?.to_string_lossy();
    let prefix = format!("{}.", file_name);

    let entries = std::fs::read_dir(parent).ok()?;

    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            // 匹配 `{原文件名}.*.tmp` 模式（同时兼容旧的 `{原文件名}.tmp` 格式）
            name_str.starts_with(&prefix) && name_str.ends_with(".tmp")
        })
        .max_by_key(|e| {
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH)
        })
        .map(|e| e.path())
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
    /// 任务状态: "pending" | "active" | "paused" | "completed" | "error" | "cancelled"
    pub status: String,
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
                    task.status == "completed" || task.status == "failed" || task.status == "cancelled"
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
        let file_name = PathBuf::from(&local_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

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
            status: "pending".to_string(),
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
    pub async fn get_task(&self, task_id: &str) -> Option<TransferTask> {
        let tasks = self.tasks.lock().await;
        tasks.get(task_id).cloned()
    }

    /// 更新任务进度
    #[allow(dead_code)]
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
        if task.progress >= 100 {
            task.status = "completed".to_string();
        } else if task.status == "pending" {
            task.status = "active".to_string();
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
    #[allow(dead_code)]
    pub async fn mark_failed(&self, task_id: &str, error: String) -> Result<(), String> {
        let mut tasks = self.tasks.lock().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("任务不存在: {}", task_id))?;

        task.status = "error".to_string();  // ← 改为 "error"，与前端一致
        task.error = Some(error);

        // 发送进度事件
        let task_clone = task.clone();
        drop(tasks);
        self.emit_progress(&task_clone)?;

        Ok(())
    }

    /// 标记任务完成
    #[allow(dead_code)]
    pub async fn mark_completed(&self, task_id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.lock().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("任务不存在: {}", task_id))?;

        task.status = "completed".to_string();
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
        if task.status != "active" {
            return Err(format!("只能暂停活动中的任务，当前状态: {}", task.status));
        }

        task.status = "paused".to_string();

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
        if task.status != "paused" {
            return Err(format!("只能继续暂停的任务，当前状态: {}", task.status));
        }

        task.status = "active".to_string();

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
        if task.status != "error" {
            return Err(format!("只能重试失败的任务，当前状态: {}", task.status));
        }

        // 保存断点续传位置
        let resume_from = if task.transferred > 0 {
            Some(task.transferred)
        } else {
            None
        };

        // 重置任务状态
        task.status = "pending".to_string();
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
        let manager_clone = Arc::new(TransferManager::new(app_handle.clone()));
        let _ = manager_clone;  // 避免未使用警告

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

        // 在后台任务中重新执行传输
        tokio::spawn(async move {
            let result = perform_transfer(
                &conn,
                request_id,
                task_clone.direction.clone(),
                task_clone.remote_path.clone(),
                task_clone.local_path.clone(),
                &manager_clone,
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
    pub async fn cleanup_by_connection(&self, connection_id: &str) {
        // 1. 在锁内收集需要清理的任务
        let tasks_to_cancel = {
            let mut tasks = self.tasks.lock().await;

            // 先收集任务和ID
            let mut to_cancel = Vec::new();
            let mut ids = Vec::new();

            for (id, task) in tasks.iter_mut() {
                if task.server_id == connection_id {
                    task.status = "cancelled".to_string();
                    to_cancel.push(task.clone());
                    ids.push(id.clone());
                }
            }

            // 统一删除（迭代器已结束）
            for id in &ids {
                tasks.remove(id);
            }

            to_cancel
        };
        // ← 锁已释放

        let count = tasks_to_cancel.len();

        // 2. 发送事件
        for task in tasks_to_cancel {
            let _ = self.emit_progress(&task);
        }

        eprintln!("[TransferManager] 已清理连接 {} 的 {} 个任务", connection_id, count);
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
                task.status = "cancelled".to_string();
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

    /// 清理已完成的任务
    #[allow(dead_code)]
    pub async fn cleanup_completed(&self) {
        let mut tasks = self.tasks.lock().await;
        tasks.retain(|_, task| {
            task.status != "completed" && task.status != "failed" && task.status != "cancelled"
        });
    }
}

/* ── 文件流处理 ─────────────────────────────────────────── */

/// 文件读取器（用于上传）
pub struct FileReader {
    reader: BufReader<File>,
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
            reader: BufReader::new(file),
            file_size,
            transferred: 0,
            chunk_size: 64 * 1024,  // 64KB
        })
    }

    /// 读取下一个数据块
    pub fn read_next_chunk(&mut self) -> Result<Option<Vec<u8>>, String> {
        if self.transferred >= self.file_size {
            return Ok(None);
        }

        let remaining = self.file_size - self.transferred;
        let read_size = std::cmp::min(self.chunk_size as u64, remaining) as usize;

        let mut buffer = vec![0u8; read_size];
        let bytes_read = self.reader.read(&mut buffer)
            .map_err(|e| format!("读取文件失败: {}", e))?;

        if bytes_read == 0 {
            return Ok(None);
        }

        buffer.truncate(bytes_read);
        self.transferred += bytes_read as u64;

        Ok(Some(buffer))
    }

    /// 获取进度百分比 (0-100)
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

        // 尝试断点续传
        let (file, actual_resume_from) = if resume_from > 0 {
            if temp_path.exists() {
                // 验证临时文件完整性
                match Self::verify_temp_file_integrity(&temp_path, resume_from) {
                    Ok(mut existing_file) => {
                        // 跳转到断点位置
                        match existing_file.seek(SeekFrom::Start(resume_from)) {
                            Ok(_) => {
                                eprintln!(
                                    "[FileWriter] 断点续传: 从 {} 字节继续写入临时文件 {:?}",
                                    resume_from, temp_path
                                );
                                (BufWriter::new(existing_file), resume_from)
                            }
                            Err(e) => {
                                eprintln!(
                                    "[FileWriter] 跳转到断点位置失败，降级为重新传输: {}",
                                    e
                                );
                                Self::create_fresh_temp_file(&temp_path)?
                            }
                        }
                    }
                    Err(reason) => {
                        eprintln!(
                            "[FileWriter] 临时文件完整性校验失败: {}，降级为重新传输",
                            reason
                        );
                        Self::create_fresh_temp_file(&temp_path)?
                    }
                }
            } else {
                eprintln!(
                    "[FileWriter] 临时文件不存在，从头开始传输: {:?}",
                    temp_path
                );
                Self::create_fresh_temp_file(&temp_path)?
            }
        } else {
            Self::create_fresh_temp_file(&temp_path)?
        };

        Ok(Self {
            writer: file,
            file_size,
            transferred: actual_resume_from,
            is_temporary: true,
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

        eprintln!(
            "[FileWriter] 临时文件完整性校验通过: {:?} (大小: {} 字节)",
            temp_path, expected_size
        );

        Ok(file)
    }

    /// 创建全新的临时文件（截断模式）
    ///
    /// 用于：
    /// - 从头开始传输（resume_from == 0）
    /// - 断点续传降级（临时文件不存在或完整性校验失败）
    fn create_fresh_temp_file(temp_path: &PathBuf) -> Result<(BufWriter<File>, u64), String> {
        let file = File::create(temp_path)
            .map_err(|e| format!("无法创建临时文件: {}", e))?;
        Ok((BufWriter::new(file), 0))
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
        
        // 原子重命名：临时文件 -> 最终文件
        std::fs::rename(&self.temp_path, &self.path)
            .map_err(|e| format!("重命名文件失败: {}", e))?;
        
        Ok(())
    }

    /// 标记文件为已完成（不是临时文件）
    pub fn mark_completed(&mut self) {
        self.is_temporary = false;
    }

    /// 获取进度百分比 (0-100)
    pub fn progress(&self) -> u32 {
        if self.file_size == 0 {
            return 100;
        }
        (self.transferred as f64 / self.file_size as f64 * 100.0) as u32
    }
}

impl Drop for FileWriter {
    fn drop(&mut self) {
        // 如果是临时文件，清理临时文件
        // 注意：只清理临时文件，不清理最终文件
        if self.is_temporary {
            eprintln!("[FileWriter] 清理临时文件: {:?}", self.temp_path);
            let _ = std::fs::remove_file(&self.temp_path);
        }
    }
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
    eprintln!("[Transfer] 开始创建传输任务: server_id={}, direction={}", server_id, direction);
    let task_id = manager
        .create_task(server_id.clone(), direction.clone(), remote_path.clone(), local_path.clone())
        .await?;
    eprintln!("[Transfer] 任务创建成功: task_id={}", task_id);

    // 获取连接管理器和 QUIC Connection
    eprintln!("[Transfer] 获取连接管理器: server_id={}", server_id);
    let connection_manager = app_handle.state::<ConnectionManager>();
    
    let quic_conn = {
        eprintln!("[Transfer] 锁定连接映射表");
        let conns = connection_manager.connections.lock().unwrap();
        eprintln!("[Transfer] 当前连接数: {}", conns.len());
        
        let active_conn = conns.get(&server_id)
            .ok_or_else(|| {
                eprintln!("[Transfer] 错误: 服务器未连接: server_id={}, 已连接的服务器: {:?}", 
                    server_id, conns.keys().collect::<Vec<_>>());
                format!("服务器未连接: {}", server_id)
            })?;
        
        eprintln!("[Transfer] 找到活跃连接: server_id={}, quic_conn={:?}", server_id, active_conn.quic_conn);
        active_conn.quic_conn.clone()
    };

    // 如果没有 QUIC Connection，返回错误
    let conn = quic_conn.ok_or_else(|| {
        eprintln!("[Transfer] 错误: QUIC Connection 为 None: server_id={}", server_id);
        "未找到 QUIC Connection，可能只使用了 WebSocket 连接".to_string()
    })?;

    // 获取请求 ID
    let request_id = connection_manager.next_request_id();

    // 在后台任务中执行传输
    let task_id_clone = task_id.clone();
    let manager_clone = Arc::clone(&manager);
    let _server_id_clone = server_id.clone();
    let direction_clone = direction.clone();
    let remote_path_clone = remote_path.clone();
    let local_path_clone = local_path.clone();

    eprintln!("[Transfer] 启动后台传输任务: task_id={}, direction={}, remote={}",
        task_id_clone, direction_clone, remote_path_clone);

    tokio::spawn(async move {
        eprintln!("[Transfer] 后台任务开始执行: task_id={}", task_id_clone);

        let result = perform_transfer(
            &conn,
            request_id,
            direction_clone,
            remote_path_clone,
            local_path_clone,
            &manager_clone,
            &task_id_clone,
            None,  // 新任务从头开始传输
        ).await;

        eprintln!("[Transfer] 后台任务执行完成: task_id={}, result={:?}", task_id_clone, result);

        if let Err(e) = result {
            eprintln!("[Transfer] 文件传输失败: task_id={}, error={}", task_id_clone, e);
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
async fn perform_transfer(
    conn: &quinn::Connection,
    request_id: u32,
    direction: String,
    remote_path: String,
    local_path: String,
    manager: &TransferManager,
    task_id: &str,
    resume_from: Option<u64>,
) -> Result<(), String> {
    eprintln!("[Transfer] 开始执行文件传输: task_id={}, direction={}, remote={}, local={}, resume_from={:?}",
        task_id, direction, remote_path, local_path, resume_from);

    // 1. 发送 FileTransferRequest
    let file_size = if direction == "upload" {
        Some(std::fs::metadata(&local_path)
            .map_err(|e| format!("无法访问本地文件: {}", e))?
            .len())
    } else {
        None
    };

    let request_payload = Payload::FileTransferRequest {
        direction: direction.clone(),
        path: remote_path.clone(),
        file_size,
        chunk_size: Some(64 * 1024),
        resume_from,  // 添加断点续传参数
    };

    // 创建 Stream（添加超时）
    let (mut send, mut recv) = tokio::time::timeout(FILE_TRANSFER_STREAM_TIMEOUT, conn.open_bi())
        .await
        .map_err(|_| "打开文件传输 Stream 超时".to_string())?
        .map_err(|e| format!("打开 Stream 失败: {}", e))?;

    // 发送请求（添加超时）
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

    // 2. 接收 FileTransferAccept（添加超时）
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

    // 处理响应
    match response_envelope.payload {
        Payload::FileTransferAccept { session_id, file_size, chunk_size, mtime: _ } => {
            eprintln!("[Transfer] 文件传输已接受: session_id={}, file_size={}, chunk_size={}",
                session_id, file_size, chunk_size);

            // 保存 Agent 端的 session_id（用于后续取消等操作）
            {
                let mut tasks = manager.tasks.lock().await;
                if let Some(task) = tasks.get_mut(task_id) {
                    task.agent_session_id = Some(session_id.clone());
                }
            }

            // 更新任务的文件大小（下载时）
            if direction == "download" {
                eprintln!("[Transfer] 下载模式：更新文件大小");
                let mut tasks = manager.tasks.lock().await;
                if let Some(task) = tasks.get_mut(task_id) {
                    task.file_size = file_size;
                    // 克隆任务用于发送进度事件
                    let task_clone = task.clone();
                    drop(tasks);
                    // 发送进度事件，通知前端文件大小已更新
                    manager.emit_progress(&task_clone)?;
                }
            }

            // 3. 开始传输数据
            if direction == "upload" {
                // 上传逻辑
                let mut reader = FileReader::new(&local_path)?;
                let mut seq = 1u32;
                let start_time = std::time::Instant::now();

                while let Some(chunk) = reader.read_next_chunk()? {
                    // 检查任务状态：是否被暂停或取消
                    {
                        let tasks = manager.tasks.lock().await;
                        if let Some(task) = tasks.get(task_id) {
                            if task.status.as_str() == "paused" {
                                // 暂停传输，等待恢复
                                drop(tasks);
                                tracing::info!("传输已暂停: task_id={}", task_id);

                                // 等待恢复信号（轮询检查）
                                loop {
                                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                    let tasks = manager.tasks.lock().await;
                                    if let Some(task) = tasks.get(task_id) {
                                        match task.status.as_str() {
                                            "active" => {
                                                // 已恢复，继续传输
                                                drop(tasks);
                                                tracing::info!("传输已恢复: task_id={}", task_id);
                                                break;
                                            }
                                            "cancelled" => {
                                                // 已取消，终止传输
                                                drop(tasks);
                                                tracing::info!("传输已取消: task_id={}", task_id);
                                                return Err("传输已取消".to_string());
                                            }
                                            _ => {
                                                // 继续等待（paused 状态）
                                                drop(tasks);
                                                continue;
                                            }
                                        }
                                    } else {
                                        // 任务已删除
                                        return Err("任务已删除".to_string());
                                    }
                                }
                            } else if task.status.as_str() == "cancelled" {
                                // 已取消，终止传输
                                drop(tasks);
                                tracing::info!("传输已取消: task_id={}", task_id);
                                return Err("传输已取消".to_string());
                            }
                        } else {
                            // 任务已删除
                            drop(tasks);
                            return Err("任务已删除".to_string());
                        }
                    }

                    // 发送 FileChunk（添加超时保护）
                    let chunk_payload = Payload::FileChunk {
                        session_id: session_id.clone(),
                        seq,
                        data: chunk.clone(),
                        size: chunk.len() as u32,
                    };

                    // 使用 UUID 生成随机 request_id
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

                    seq += 1;

                    // 计算速度和进度
                    let elapsed = start_time.elapsed().as_secs();
                    let speed_bps = if elapsed > 0 {
                        reader.transferred / elapsed
                    } else {
                        0
                    };

                    // 更新进度
                    manager.update_progress(
                        task_id,
                        reader.transferred,
                        speed_bps,
                    ).await?;
                }

                // 发送 FileTransferComplete（添加超时）
                let complete_payload = Payload::FileTransferComplete {
                    session_id: session_id.clone(),
                    success: true,
                    mtime: None,
                    error: None,
                };

                // 使用 UUID 生成随机 request_id
                let complete_request_id = uuid::Uuid::new_v4().as_u128() as u32;
                let complete_envelope = Envelope::new(complete_request_id, complete_payload);
                let complete_bytes = complete_envelope.encode()?;
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

                // 标记完成
                manager.mark_completed(task_id).await?;
            } else {
                // 下载逻辑
                eprintln!("[Transfer] 开始下载逻辑：创建 FileWriter");
                let resume_pos = resume_from.unwrap_or(0);
                let mut writer = FileWriter::with_resume(&local_path, file_size, resume_pos)?;
                if resume_pos > 0 {
                    eprintln!("[Transfer] 断点续传: 从 {} 字节继续下载", writer.transferred);
                }
                let start_time = std::time::Instant::now();

                eprintln!("[Transfer] 进入接收循环：等待 FileChunk");
                loop {
                    // 检查任务状态：是否被暂停或取消
                    {
                        let tasks = manager.tasks.lock().await;
                        if let Some(task) = tasks.get(task_id) {
                            if task.status.as_str() == "paused" {
                                // 暂停传输，等待恢复
                                drop(tasks);
                                tracing::info!("传输已暂停: task_id={}", task_id);

                                // 等待恢复信号（轮询检查）
                                loop {
                                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                    let tasks = manager.tasks.lock().await;
                                    if let Some(task) = tasks.get(task_id) {
                                        match task.status.as_str() {
                                            "active" => {
                                                // 已恢复，继续传输
                                                drop(tasks);
                                                tracing::info!("传输已恢复: task_id={}", task_id);
                                                break;
                                            }
                                            "cancelled" => {
                                                // 已取消，终止传输
                                                drop(tasks);
                                                tracing::info!("传输已取消: task_id={}", task_id);
                                                return Err("传输已取消".to_string());
                                            }
                                            _ => {
                                                // 继续等待（paused 状态）
                                                drop(tasks);
                                                continue;
                                            }
                                        }
                                    } else {
                                        // 任务已删除
                                        return Err("任务已删除".to_string());
                                    }
                                }
                            } else if task.status.as_str() == "cancelled" {
                                // 已取消，终止传输
                                drop(tasks);
                                tracing::info!("传输已取消: task_id={}", task_id);
                                return Err("传输已取消".to_string());
                            }
                        } else {
                            // 任务已删除
                            drop(tasks);
                            return Err("任务已删除".to_string());
                        }
                    }

                    // 读取消息长度（添加超时）
                    eprintln!("[Transfer] 等待读取消息长度");
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
                            eprintln!("[Transfer] 读取数据块失败: {}", e);
                            manager.mark_failed(task_id, format!("读取数据块失败: {}", e)).await?;
                            return Err(format!("读取数据块失败: {}", e));
                        }
                        Err(_) => {
                            eprintln!("[Transfer] 读取数据块超时 ({}s)", FILE_CHUNK_TIMEOUT.as_secs());
                            manager.mark_failed(task_id, "读取数据块超时".to_string()).await?;
                            return Err("读取数据块超时".to_string());
                        }
                    };

                    // 解析消息
                    let chunk_envelope = Envelope::decode(&chunk_buf)?;

                    match chunk_envelope.payload {
                        Payload::FileChunk { data, .. } => {
                            // 写入文件
                            writer.write_chunk(&data)?;

                            // 计算速度
                            let elapsed = start_time.elapsed().as_secs();
                            let speed_bps = if elapsed > 0 {
                                writer.transferred / elapsed
                            } else {
                                0
                            };

                            // 更新进度
                            manager.update_progress(
                                task_id,
                                writer.transferred,
                                speed_bps,
                            ).await?;
                        }
                        Payload::FileTransferComplete { success, error, .. } => {
                            if success {
                                writer.finish()?;
                                writer.mark_completed();  // 标记为已完成，不是临时文件
                                manager.mark_completed(task_id).await?;
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

                // 检查传输是否完成
                if writer.transferred >= writer.file_size {
                    // 传输完成，调用 finish() 执行原子重命名
                    writer.finish()?;
                    writer.mark_completed();  // 标记为已完成，不是临时文件
                    manager.mark_completed(task_id).await?;
                } else {
                    // 传输不完整，标记为失败（writer drop 时会自动清理临时文件）
                    manager.mark_failed(task_id, "传输不完整".to_string()).await?;
                    return Err("传输不完整".to_string());
                }
            }

            Ok(())
        }
        Payload::Error { message, .. } => {
            Err(format!("Agent 返回错误: {}", message))
        }
        _ => Err("意外的响应类型".to_string()),
    }
}

/// 暂停文件传输
#[command]
pub async fn pause_transfer(task_id: String, app_handle: AppHandle) -> Result<(), String> {
    eprintln!("[Transfer] 暂停传输请求: task_id={}", task_id);

    // 获取全局 TransferManager
    let manager = app_handle.state::<Arc<TransferManager>>();

    // 调用暂停方法
    manager.pause_task(&task_id).await?;

    Ok(())
}

/// 继续文件传输
#[command]
pub async fn resume_transfer(task_id: String, app_handle: AppHandle) -> Result<(), String> {
    eprintln!("[Transfer] 继续传输请求: task_id={}", task_id);

    // 获取全局 TransferManager
    let manager = app_handle.state::<Arc<TransferManager>>();

    // 调用继续方法
    manager.resume_task(&task_id).await?;

    Ok(())
}

/// 重试文件传输（支持断点续传）
#[command]
pub async fn retry_transfer(task_id: String, app_handle: AppHandle) -> Result<(), String> {
    eprintln!("[Transfer] 重试传输请求: task_id={}", task_id);

    // 获取全局 TransferManager
    let manager = app_handle.state::<Arc<TransferManager>>();

    // 调用重试方法（支持断点续传）
    manager.retry_task(&task_id, &app_handle).await?;

    Ok(())
}

/// 取消文件传输
#[command]
pub async fn cancel_transfer(task_id: String, app_handle: AppHandle) -> Result<(), String> {
    eprintln!("[Transfer] 取消传输请求: task_id={}", task_id);

    // 获取全局 TransferManager
    let manager = app_handle.state::<Arc<TransferManager>>();

    // 调用取消方法（会同时通知 Agent 端清理传输会话）
    manager.cancel_task(&task_id, &app_handle).await?;

    Ok(())
}

/// 检查文件是否存在
#[command]
pub async fn check_file_exists(
    server_id: String,
    path: String,
    app_handle: tauri::AppHandle,
) -> Result<FileExistsInfo, String> {
    use crate::connection::{remote_send, Payload};

    eprintln!("[Transfer] 检查文件是否存在: server_id={}, path={}", server_id, path);

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