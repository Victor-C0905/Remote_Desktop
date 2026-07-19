//! 传输会话管理模块
//!
//! 提供文件传输会话的状态跟踪，支持上传和下载操作。

use crate::file_stream::{FileStreamReader, FileStreamWriter};
use crate::protocol::TransferDirection;
use std::time::SystemTime;

/// 传输会话状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferStatus {
    /// 传输中
    Active,
    /// 已暂停
    Paused,
    /// 已取消
    Cancelled,
    /// 已完成
    Completed,
    /// 失败
    Error,
}

/// 传输会话状态
///
/// # 设计说明
///
/// 当前版本使用公开字段以便于 handler 层直接操作。
/// 未来版本可以考虑改进封装性：
/// - 使用私有字段 + 公开访问器方法
/// - 提供状态转换方法（pause, cancel, complete）
/// - 添加字段验证逻辑
pub struct TransferSession {
    /// 会话 ID
    pub session_id: String,
    /// 传输方向
    pub direction: TransferDirection,
    /// 文件路径
    pub path: String,
    /// 文件总大小
    pub file_size: u64,
    /// 已传输字节数
    pub transferred: u64,
    /// 分块大小
    pub chunk_size: u32,
    /// 传输状态
    pub status: TransferStatus,
    /// 开始时间
    pub started_at: Option<SystemTime>,

    /// 文件流处理器（上传时使用 writer，下载时使用 reader）
    ///
    /// 注意：同一时刻只能有一个为 Some，由 handler 层保证互斥关系
    pub writer: Option<FileStreamWriter>,

    /// 文件流处理器（上传时使用 writer，下载时使用 reader）
    ///
    /// 注意：同一时刻只能有一个为 Some，由 handler 层保证互斥关系
    pub reader: Option<FileStreamReader>,
}

impl TransferSession {
    /// 创建新的传输会话
    pub fn new(
        session_id: String,
        direction: TransferDirection,
        path: String,
        file_size: u64,
        chunk_size: u32,
    ) -> Self {
        Self {
            session_id,
            direction,
            path,
            file_size,
            transferred: 0,
            chunk_size,
            status: TransferStatus::Active,
            started_at: Some(SystemTime::now()),
            writer: None,
            reader: None,
        }
    }

    /// 获取进度百分比
    pub fn progress(&self) -> u32 {
        if self.file_size == 0 {
            return 100;
        }
        (self.transferred as f64 / self.file_size as f64 * 100.0) as u32
    }

    /// 检查是否超时（1小时）
    pub fn is_timeout(&self) -> bool {
        if let Some(started_at) = self.started_at {
            let elapsed = SystemTime::now()
                .duration_since(started_at)
                .unwrap_or_default()
                .as_secs();

            return elapsed > 3600;  // 1小时
        }
        false
    }
}