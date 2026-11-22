//! 传输会话管理模块
//!
//! 提供文件传输会话的状态跟踪，支持上传和下载操作。
//! 支持单流(向后兼容)与大文件多流并行(方案 A: 独立段+合并)。

use crate::file_stream::{PipeFileStreamReader, PipeFileStreamWriter};
use std::collections::HashMap;
use std::time::SystemTime;

/// 单次传输最大并行流数上限。
///
/// stream_count 来自客户端声明（认证用户可发任意 u32），无上限会按其
/// 预分配内存（part_offsets/part_paths，约 40 字节/流）——u32::MAX 约
/// 96GB → alloc-abort 杀死共享 Manager（跨用户 DoS，2026-11-20 审计 A-02）。
pub const MAX_STREAM_COUNT: u32 = 16;

/// 协商 stream_count：None/0/1 视为单流（向后兼容），钳制到 `[1, MAX_STREAM_COUNT]`。
///
/// 客户端声明值不可信；Accept 回显服务端协商结果，客户端按回显值开流，
/// 因此钳制对既有客户端透明。
pub fn negotiate_stream_count(raw: Option<u32>) -> u32 {
    raw.unwrap_or(1).max(1).min(MAX_STREAM_COUNT)
}

/// 传输会话状态
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
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
    /// 传输方向（"upload" 或 "download"）
    pub direction: String,
    /// 文件路径
    pub path: String,
    /// 文件总大小
    pub file_size: u64,
    /// 已传输字节数
    pub transferred: u64,
    /// 分块大小
    #[allow(dead_code)]
    pub chunk_size: u32,
    /// 传输状态
    pub status: TransferStatus,
    /// 开始时间
    pub started_at: Option<SystemTime>,
    /// 属主 uid（创建该会话的已认证用户）。
    ///
    /// 会话仅按 session_id（UUID）索引，任何拿到 ID 的已认证连接此前
    /// 即可注入数据块/强制 finish/取消他人传输——属主绑定是 UUID 之外
    /// 的第二道防线（2026-11 审计 M2）
    pub owner_uid: u32,

    /// 文件流处理器（上传时使用 writer，下载时使用 reader）
    ///
    /// 注意：同一时刻只能有一个为 Some，由 handler 层保证互斥关系
    ///
    /// 多流模式下：本字段承载主 stream（stream_index=0）的 writer，
    /// 其余 N-1 个 stream 的 writer 存于 `part_writers`。
    pub writer: Option<PipeFileStreamWriter>,

    /// 文件流处理器（上传时使用 writer，下载时使用 reader）
    ///
    /// 注意：同一时刻只能有一个为 Some，由 handler 层保证互斥关系
    pub reader: Option<PipeFileStreamReader>,

    // ===== 多流并行字段（方案 A: 独立段+合并，Task 4a） =====
    //
    // 设计说明：
    // - `stream_count == 1` 时为单流模式（向后兼容），下面三个字段全部为空/0，
    //   走原有 `writer`/`reader` + `transferred` 路径，merge 逻辑不触发。
    // - `stream_count > 1` 时为多流并行模式：
    //   * 主 stream (index=0) 用 `writer` 字段
    //   * 其余 N-1 个 stream 用 `part_writers` HashMap<u32, PipeFileStreamWriter>
    //   * 每个 stream 独立临时段文件，路径存于 `part_paths`（按 stream_index 索引）
    //   * 每个 stream 负责的 offset 段存于 `part_offsets`（按 stream_index 索引）
    //   * 全部 N 个 stream 完成 → 合并段文件 → rename 为最终文件
    //
    // 这些字段在 Task 4b（服务端多流调度与合并）和 Task 4c（客户端分片调度）启用前
    // 暂为 unused，加 `#[allow(dead_code)]` 显式标注以保持零警告策略的清晰度。

    /// 多流并行数（1=单流默认，向后兼容；>1=多流并行）
    #[allow(dead_code)]
    pub stream_count: u32,

    /// 已完成段数（每 stream finish 后 +1，达到 stream_count 时触发合并）
    #[allow(dead_code)]
    pub completed_streams: u32,

    /// 各段 offset 范围 `(offset_start, offset_end)`，按 stream_index 索引（0..N-1）
    /// 单流时为空 Vec（用 `transferred` 跟踪）
    #[allow(dead_code)]
    pub part_offsets: Vec<(u64, u64)>,

    /// 各段临时文件路径，按 stream_index 索引（0..N-1，含主 stream 在 0）
    /// 单流时为空 Vec（主 writer 内部已跟踪 temp_path）
    #[allow(dead_code)]
    pub part_paths: Vec<String>,

    /// 多流段 writers（stream_index 1..N-1）
    /// 主 stream (index=0) 的 writer 存于 `writer` 字段，此处不重复
    #[allow(dead_code)]
    pub part_writers: HashMap<u32, PipeFileStreamWriter>,

    /// 已加入的 stream_index 集合（去重判据）。
    /// writer 被完成路径 take 后 `part_writers` 不再含该 index，重复 join
    /// 会靠它拒绝，否则新 writer 覆盖 part_paths[idx] 造成段泄漏/错段
    /// （2026-11-20 审计 A-17）
    #[allow(dead_code)]
    pub joined_streams: std::collections::HashSet<u32>,
}

impl TransferSession {
    /// 创建新的传输会话（单流模式，向后兼容）
    ///
    /// 等价于 `new_multi_stream(..., 1)` 但 part_offsets/part_paths/part_writers 全为空，
    /// 走原有 `writer`/`reader` + `transferred` 路径，不触发合并逻辑。
    pub fn new(
        session_id: String,
        direction: String,
        path: String,
        file_size: u64,
        chunk_size: u32,
        owner_uid: u32,
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
            owner_uid,
            writer: None,
            reader: None,
            // 多流字段默认值（单流模式）
            stream_count: 1,
            completed_streams: 0,
            part_offsets: Vec::new(),
            part_paths: Vec::new(),
            part_writers: HashMap::new(),
            joined_streams: std::collections::HashSet::new(),
        }
    }

    /// 创建多流并行传输会话（方案 A: 独立段+合并）
    ///
    /// # 参数
    /// - `session_id`: 传输会话 ID
    /// - `direction`: 传输方向（"upload" 或 "download"）
    /// - `path`: 最终文件路径（合并后 rename 的目标）
    /// - `file_size`: 文件总大小
    /// - `chunk_size`: 单 chunk 大小
    /// - `stream_count`: 多流并行数（>1 启用多流）
    /// - `part_offsets`: 各 stream 的 offset 范围 `(start, end)`，长度 == stream_count
    ///
    /// # 说明
    /// - 主 stream (index=0) 的 writer 由调用方后续通过 `writer` 字段设置
    /// - 其余 stream (index=1..N-1) 的 writer 通过 `part_writers` HashMap 设置
    /// - 各 stream 的临时段文件路径通过 `part_paths` 设置（合并时按 offset 顺序拼接）
    /// - `transferred` 字段在多流模式下不作为总进度，改用 `completed_streams` 跟踪
    #[allow(dead_code)]
    pub fn new_multi_stream(
        session_id: String,
        direction: String,
        path: String,
        file_size: u64,
        chunk_size: u32,
        stream_count: u32,
        part_offsets: Vec<(u64, u64)>,
        owner_uid: u32,
    ) -> Self {
        assert!(
            (1..=MAX_STREAM_COUNT).contains(&stream_count),
            "stream_count 必须在 1..={}，实际 = {}（调用方必须先经 negotiate_stream_count 钳制）",
            MAX_STREAM_COUNT,
            stream_count
        );
        assert_eq!(
            part_offsets.len(),
            stream_count as usize,
            "part_offsets 长度必须等于 stream_count"
        );

        // 预分配 part_paths 槽位（由 Task 4b 调用方按 stream_index 填入临时段路径）
        let part_paths = vec![String::new(); stream_count as usize];

        Self {
            session_id,
            direction,
            path,
            file_size,
            transferred: 0,
            chunk_size,
            status: TransferStatus::Active,
            started_at: Some(SystemTime::now()),
            owner_uid,
            writer: None,
            reader: None,
            stream_count,
            completed_streams: 0,
            part_offsets,
            part_paths,
            part_writers: HashMap::new(),
            joined_streams: std::collections::HashSet::new(),
        }
    }

    /// 检查是否为多流会话
    #[allow(dead_code)]
    pub fn is_multi_stream(&self) -> bool {
        self.stream_count > 1
    }

    /// 标记 stream_index 已加入；返回 false = 重复加入。
    ///
    /// 去重判据（A-17）：writer 被完成路径 take 后 `part_writers` 不再含
    /// 该 index，重复 join 必须以本集合拒绝。
    #[allow(dead_code)]
    pub fn mark_joined(&mut self, stream_index: u32) -> bool {
        self.joined_streams.insert(stream_index)
    }

    /// 标记某个 stream 段已完成，返回是否所有 stream 均已完成（触发合并条件）
    ///
    /// 多流模式下每 stream finish 后调用，`completed_streams` 达到 `stream_count`
    /// 即表示所有段已落盘，可进入合并阶段。
    #[allow(dead_code)]
    pub fn mark_stream_completed(&mut self) -> bool {
        self.completed_streams = self.completed_streams.saturating_add(1);
        self.completed_streams >= self.stream_count
    }

    /// 获取进度百分比
    #[allow(dead_code)]
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

    /// 标记上传完成（临时文件已重命名为最终文件）
    ///
    /// 调用此方法后，Drop 不会再清理临时文件。
    /// 通常在 `writer.finish()` 成功后调用。
    pub fn mark_completed(&mut self) {
        // writer.finish() 内部已标记 completed，这里仅更新状态
        self.status = TransferStatus::Completed;
    }
}

/// Drop 保护：当 TransferSession 被意外 drop 时清理未完成的上传临时文件
///
/// # 触发场景
/// - `ConnectionContext::cleanup()` 清理超时会话
/// - 手动从 `TRANSFER_SESSIONS` HashMap 中移除会话
/// - 进程 panic 导致栈展开
///
/// # 设计说明
/// - 仅对上传方向的会话进行清理（下载不需要清理临时文件）
/// - 调用 writer 的 abort() 方法删除临时文件
/// - PipeFileStreamWriter 自身也有 Drop 保护，这里显式调用是为了确保
///   子进程和临时文件在 session 被移除时立即清理，而非等到 writer 被 drop
/// - 多流模式下：同时 abort 主 writer 与所有 part_writers，确保所有隔离子进程
///   被回收、所有临时段文件被清理
impl Drop for TransferSession {
    fn drop(&mut self) {
        if self.direction == "upload" && self.status != TransferStatus::Completed {
            if self.writer.is_some() || !self.part_writers.is_empty() {
                tracing::warn!(
                    "[TransferSession] 清理未完成的上传会话: session_id={}, path={}, streams={}/{} (completed_streams={})",
                    self.session_id,
                    self.path,
                    self.part_writers.len() + if self.writer.is_some() { 1 } else { 0 },
                    self.stream_count,
                    self.completed_streams,
                );
            }
            // 主 stream writer
            if let Some(ref mut writer) = self.writer {
                writer.abort();
            }
            // 多流 part_writers（stream_index 1..N-1）
            for (_idx, writer) in self.part_writers.iter_mut() {
                writer.abort();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_single_stream_defaults() {
        // 单流构造：多流字段全部为默认值，is_multi_stream 返回 false
        let s = TransferSession::new(
            "sess-1".to_string(),
            "upload".to_string(),
            "/tmp/file.bin".to_string(),
            1024,
            256 * 1024,
            1000,
        );
        assert_eq!(s.stream_count, 1);
        assert_eq!(s.completed_streams, 0);
        assert!(s.part_offsets.is_empty());
        assert!(s.part_paths.is_empty());
        assert!(s.part_writers.is_empty());
        assert!(!s.is_multi_stream());
        assert_eq!(s.transferred, 0);
    }

    /// A-02 回归：协商函数钳制客户端声明值（None/0 → 1，超限 → MAX）
    #[test]
    fn negotiate_stream_count_clamps_untrusted_input() {
        assert_eq!(negotiate_stream_count(None), 1);
        assert_eq!(negotiate_stream_count(Some(0)), 1);
        assert_eq!(negotiate_stream_count(Some(1)), 1);
        assert_eq!(negotiate_stream_count(Some(4)), 4);
        assert_eq!(negotiate_stream_count(Some(MAX_STREAM_COUNT)), MAX_STREAM_COUNT);
        // 复现：u32::MAX 声明值被钳制，不再按其预分配内存
        assert_eq!(negotiate_stream_count(Some(u32::MAX)), MAX_STREAM_COUNT);
    }

    /// A-02 回归：构造器守卫拒绝超限 stream_count（拦住绕过协商函数的调用方）
    #[test]
    #[should_panic(expected = "stream_count 必须在 1..=")]
    fn new_multi_stream_rejects_stream_count_over_limit() {
        let offsets = vec![(0, 1); (MAX_STREAM_COUNT + 1) as usize];
        TransferSession::new_multi_stream(
            "sess-over".to_string(),
            "upload".to_string(),
            "/tmp/big.bin".to_string(),
            1024,
            256 * 1024,
            MAX_STREAM_COUNT + 1,
            offsets,
            1000,
        );
    }

    /// A-02 回归：上限值可正常构造（边界不过杀）
    #[test]
    fn new_multi_stream_accepts_max_stream_count() {
        let offsets = vec![(0, 1); MAX_STREAM_COUNT as usize];
        let s = TransferSession::new_multi_stream(
            "sess-max".to_string(),
            "upload".to_string(),
            "/tmp/big.bin".to_string(),
            1024,
            256 * 1024,
            MAX_STREAM_COUNT,
            offsets,
            1000,
        );
        assert_eq!(s.part_paths.len(), MAX_STREAM_COUNT as usize);
        assert!(s.is_multi_stream());
    }

    #[test]
    fn test_new_multi_stream_fields() {
        // 多流构造：stream_count=4，part_offsets 长度匹配，part_paths 预分配槽位
        let offsets = vec![(0, 256), (256, 512), (512, 768), (768, 1024)];
        let s = TransferSession::new_multi_stream(
            "sess-ms".to_string(),
            "upload".to_string(),
            "/tmp/big.bin".to_string(),
            1024,
            256 * 1024,
            4,
            offsets.clone(),
            1000,
        );
        assert_eq!(s.stream_count, 4);
        assert!(s.is_multi_stream());
        assert_eq!(s.part_offsets, offsets);
        assert_eq!(s.part_paths.len(), 4);
        // 槽位预分配为空字符串，由 Task 4b 调用方填入
        for p in &s.part_paths {
            assert!(p.is_empty());
        }
        assert!(s.part_writers.is_empty()); // 由 Task 4b 调用方填入
    }

    #[test]
    #[should_panic(expected = "part_offsets 长度必须等于 stream_count")]
    fn test_new_multi_stream_mismatched_offsets_panics() {
        // stream_count=4 但只给 3 个 offset → 立即 panic（防静默错位）
        let _ = TransferSession::new_multi_stream(
            "sess-bad".to_string(),
            "upload".to_string(),
            "/tmp/x.bin".to_string(),
            1024,
            256 * 1024,
            4,
            vec![(0, 256), (256, 512), (512, 768)],
            1000,
        );
    }

    #[test]
    fn test_mark_stream_completed_merge_trigger() {
        // 4 流会话：前 3 次 mark 返回 false，第 4 次返回 true（触发合并条件）
        let offsets = vec![(0, 256), (256, 512), (512, 768), (768, 1024)];
        let mut s = TransferSession::new_multi_stream(
            "sess-ms".to_string(),
            "upload".to_string(),
            "/tmp/big.bin".to_string(),
            1024,
            256 * 1024,
            4,
            offsets,
            1000,
        );
        assert!(!s.mark_stream_completed(), "第 1 次不应触发合并");
        assert!(!s.mark_stream_completed(), "第 2 次不应触发合并");
        assert!(!s.mark_stream_completed(), "第 3 次不应触发合并");
        assert_eq!(s.completed_streams, 3);
        assert!(s.mark_stream_completed(), "第 4 次（全部完成）应触发合并");
        assert_eq!(s.completed_streams, 4);
    }

    #[test]
    fn test_mark_stream_completed_single_stream() {
        // 单流会话：1 次 mark 即触发（stream_count=1）
        let mut s = TransferSession::new(
            "sess-1".to_string(),
            "upload".to_string(),
            "/tmp/file.bin".to_string(),
            1024,
            256 * 1024,
            1000,
        );
        assert!(s.mark_stream_completed(), "单流 1 次 mark 即应触发");
    }
}
#[cfg(test)]
mod joined_tests {
    use super::*;

    /// A-17 复现：writer 被完成路径 take 后（part_writers 变空），重复 join
    /// 必须被 joined 集合拒绝——旧判据 contains_key 在 take 后失效，新 writer
    /// 覆盖 part_paths[idx] 造成段泄漏/合并错段
    #[test]
    fn mark_joined_dedups_after_writer_take() {
        let offsets = vec![(0, 1); 3];
        let mut s = TransferSession::new_multi_stream(
            "sess-join".to_string(),
            "upload".to_string(),
            "/tmp/big.bin".to_string(),
            1024,
            256 * 1024,
            3,
            offsets,
            1000,
        );
        assert!(s.mark_joined(1), "首次加入应成功");
        // 模拟完成路径 take writer 后 part_writers 变空，重复 join 仍须被拒
        s.part_writers.remove(&1);
        assert!(!s.mark_joined(1), "重复加入必须被拒绝（A-17）");
        assert!(s.mark_joined(2), "其他 index 不受影响");
    }
}
