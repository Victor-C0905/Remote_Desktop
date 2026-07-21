//! 文件传输管理器
//!
//! 负责管理文件上传/下载任务，发送进度事件到前端。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{command, AppHandle, Emitter, Manager};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::connection::{ConnectionManager, Envelope, Payload};

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

    // ── 原始参数（用于重试和断点续传） ──
    /// 服务器 ID（原始参数）
    pub server_id: String,
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
        Self {
            app_handle,
            tasks: Arc::new(Mutex::new(HashMap::new())),
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
            server_id: server_id,  // 保存原始参数
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

    /// 取消任务
    #[allow(dead_code)]
    pub async fn cancel_task(&self, task_id: &str) -> Result<(), String> {
        let mut tasks = self.tasks.lock().await;
        let task = tasks
            .get_mut(task_id)
            .ok_or_else(|| format!("任务不存在: {}", task_id))?;

        if task.status != "completed" && task.status != "failed" {
            task.status = "cancelled".to_string();
        }

        // 发送进度事件
        let task_clone = task.clone();
        drop(tasks);
        self.emit_progress(&task_clone)?;

        Ok(())
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
pub struct FileWriter {
    writer: BufWriter<File>,
    file_size: u64,
    pub transferred: u64,
}

impl FileWriter {
    /// 创建文件写入器
    pub fn new(path: &str, file_size: u64) -> Result<Self, String> {
        let file = File::create(path)
            .map_err(|e| format!("无法创建文件: {}", e))?;

        Ok(Self {
            writer: BufWriter::new(file),
            file_size,
            transferred: 0,
        })
    }

    /// 写入数据块
    pub fn write_chunk(&mut self, data: &[u8]) -> Result<(), String> {
        self.writer.write_all(data)
            .map_err(|e| format!("写入文件失败: {}", e))?;
        self.transferred += data.len() as u64;
        Ok(())
    }

    /// 完成写入
    pub fn finish(&mut self) -> Result<(), String> {
        self.writer.flush()
            .map_err(|e| format!("刷新文件失败: {}", e))?;
        Ok(())
    }

    /// 获取进度百分比 (0-100)
    pub fn progress(&self) -> u32 {
        if self.file_size == 0 {
            return 100;
        }
        (self.transferred as f64 / self.file_size as f64 * 100.0) as u32
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

    // 创建 Stream
    let (mut send, mut recv) = conn.open_bi().await
        .map_err(|e| format!("打开 Stream 失败: {}", e))?;

    // 发送请求
    let request_envelope = Envelope::new(request_id, request_payload);
    let request_bytes = request_envelope.encode()?;
    let request_len = (request_bytes.len() as u32).to_le_bytes();

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    send.write_all(&request_len).await
        .map_err(|e| format!("发送请求长度失败: {}", e))?;
    send.write_all(&request_bytes).await
        .map_err(|e| format!("发送请求数据失败: {}", e))?;
    send.flush().await
        .map_err(|e| format!("刷新发送流失败: {}", e))?;

    // 2. 接收 FileTransferAccept
    let mut response_len_buf = [0u8; 4];
    recv.read_exact(&mut response_len_buf).await
        .map_err(|e| format!("读取响应长度失败: {}", e))?;
    let response_len = u32::from_le_bytes(response_len_buf) as usize;

    let mut response_buf = vec![0u8; response_len];
    recv.read_exact(&mut response_buf).await
        .map_err(|e| format!("读取响应数据失败: {}", e))?;

    let response_envelope = Envelope::decode(&response_buf)?;

    // 处理响应
    match response_envelope.payload {
        Payload::FileTransferAccept { session_id, file_size, chunk_size, mtime: _ } => {
            eprintln!("[Transfer] 文件传输已接受: session_id={}, file_size={}, chunk_size={}",
                session_id, file_size, chunk_size);

            // 更新任务的文件大小（下载时）
            if direction == "download" {
                eprintln!("[Transfer] 下载模式：更新文件大小");
                let mut tasks = manager.tasks.lock().await;
                if let Some(task) = tasks.get_mut(task_id) {
                    task.file_size = file_size;
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

                    // 发送 FileChunk
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

                    send.write_all(&chunk_len).await
                        .map_err(|e| format!("发送块长度失败: {}", e))?;
                    send.write_all(&chunk_bytes).await
                        .map_err(|e| format!("发送块数据失败: {}", e))?;

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

                // 发送 FileTransferComplete
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

                send.write_all(&complete_len).await
                    .map_err(|e| format!("发送完成消息失败: {}", e))?;
                send.write_all(&complete_bytes).await
                    .map_err(|e| format!("发送完成数据失败: {}", e))?;
                send.flush().await
                    .map_err(|e| format!("刷新发送流失败: {}", e))?;

                // 标记完成
                manager.mark_completed(task_id).await?;
            } else {
                // 下载逻辑
                eprintln!("[Transfer] 开始下载逻辑：创建 FileWriter");
                let mut writer = FileWriter::new(&local_path, file_size)?;
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

                    // 读取消息长度
                    eprintln!("[Transfer] 等待读取消息长度");
                    let mut chunk_len_buf = [0u8; 4];
                    match recv.read_exact(&mut chunk_len_buf).await {
                        Ok(_) => {
                            eprintln!("[Transfer] 成功读取消息长度");
                        }
                        Err(e) => {
                            eprintln!("[Transfer] 读取块长度失败: {}", e);
                            break;
                        }
                    }

                    let chunk_len = u32::from_le_bytes(chunk_len_buf) as usize;
                    let mut chunk_buf = vec![0u8; chunk_len];

                    match recv.read_exact(&mut chunk_buf).await {
                        Ok(_) => {}
                        Err(e) => {
                            tracing::error!("读取块数据失败: {}", e);
                            break;
                        }
                    }

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

    // 调用取消方法
    manager.cancel_task(&task_id).await?;

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