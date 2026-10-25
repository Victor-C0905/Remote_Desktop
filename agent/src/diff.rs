// agent/src/diff.rs
//! 文件差异应用模块
//!
//! Agent 只负责应用差异（无状态化架构）
//! 差异计算由客户端完成（流量优化）

// 差异类型定义已统一至 quirel-protocol crate（线上协议单一真相源），
// 此处 re-export 维持 `crate::diff::*` 既有引用路径不变
pub use quirel_protocol::{FileDiff, DiffType};

/// 应用差异到原文本（Agent 侧，无状态化）
///
/// 根据差异列表，生成新文本
///
/// # 参数
/// - `old_text`: 原文本（从文件系统读取）
/// - `diffs`: 差异列表（客户端计算并发送）
///
/// # 返回
/// 新文本
///
/// # 示例
/// ```rust
/// use agent::diff::{FileDiff, DiffType, apply_diff};
///
/// let old = "line1\nline2\nline3";
/// let diffs = vec![FileDiff {
///     diff_type: DiffType::Replace,
///     line_number: 2,
///     old_content: Some("line2\n".to_string()),
///     new_content: Some("modified\n".to_string()),
/// }];
/// let result = apply_diff(old, &diffs);
/// assert_eq!(result, "line1\nmodified\nline3");
/// ```
pub fn apply_diff(old_text: &str, diffs: &[FileDiff]) -> String {
    let mut lines: Vec<String> = old_text.lines().map(|s| s.to_string()).collect();

    // 应用规则（与客户端 diff.ts 协同）：
    // 1) 所有行号基于【原始旧文件】坐标，统一按行号降序应用
    //    （大坐标先应用，不影响更小坐标的索引）
    // 2) 行号相同的连续 insert 段按生成序的【逆序】应用——
    //    依序头插到同一位置，最终顺序与生成序一致
    // 3) 行号防御性钳制：0 视为 1（防 usize 下溢）；insert 越界钳到末尾
    //    （Vec::insert 越界会 panic；曾因客户端行号坐标系错误引发
    //    Worker 崩溃重启，客户端收 "Worker 路由失败: response dropped"）
    let mut sorted: Vec<&FileDiff> = diffs.iter().collect();
    sorted.sort_by(|a, b| b.line_number.cmp(&a.line_number)); // 稳定排序保留生成序

    // 相同行号的连续 insert 段 → 逆序
    let mut i = 0;
    while i < sorted.len() {
        let mut j = i;
        while j < sorted.len()
            && sorted[j].line_number == sorted[i].line_number
            && sorted[j].diff_type == DiffType::Insert
        {
            j += 1;
        }
        if j > i + 1 {
            sorted[i..j].reverse();
        }
        i = if j > i { j } else { i + 1 };
    }

    for diff in sorted {
        let line_idx = diff.line_number.saturating_sub(1);
        match diff.diff_type {
            DiffType::Delete => {
                if line_idx < lines.len() {
                    lines.remove(line_idx);
                }
            }
            DiffType::Replace => {
                if line_idx < lines.len() {
                    if let Some(new_content) = &diff.new_content {
                        let content = new_content.trim_end_matches('\n');
                        lines[line_idx] = content.to_string();
                    }
                }
            }
            DiffType::Insert => {
                if let Some(new_content) = &diff.new_content {
                    let content = new_content.trim_end_matches('\n');
                    let idx = line_idx.min(lines.len());
                    lines.insert(idx, content.to_string());
                }
            }
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_diff_mixed_delete_insert_same_line() {
        // 单行修改 = delete(2) + insert(3)：大坐标 insert 先应用，
        // 结果不因 delete 缩短数组而错位（与客户端回环一致）
        let old = "line1
line2
line3";
        let diffs = vec![
            FileDiff { diff_type: DiffType::Delete, line_number: 2, old_content: Some("line2".into()), new_content: None },
            FileDiff { diff_type: DiffType::Insert, line_number: 3, old_content: None, new_content: Some("modified
".into()) },
        ];
        assert_eq!(apply_diff(old, &diffs), "line1
modified
line3");
    }

    #[test]
    fn test_apply_diff_consecutive_inserts_same_line_order() {
        // 同一插入点的多个 insert：逆序应用，最终顺序与生成序一致
        let old = "a
z";
        let diffs = vec![
            FileDiff { diff_type: DiffType::Insert, line_number: 2, old_content: None, new_content: Some("i1
".into()) },
            FileDiff { diff_type: DiffType::Insert, line_number: 2, old_content: None, new_content: Some("i2
".into()) },
            FileDiff { diff_type: DiffType::Insert, line_number: 2, old_content: None, new_content: Some("i3
".into()) },
        ];
        assert_eq!(apply_diff(old, &diffs), "a
i1
i2
i3
z");
    }

    #[test]
    fn test_apply_diff_insert_at_end_clamped() {
        // 文件尾追加（line_number 超出末尾+1）：不 panic，钳制为追加
        let old = "line1\nline2";
        let diffs = vec![FileDiff {
            diff_type: DiffType::Insert,
            line_number: 100,
            old_content: None,
            new_content: Some("appended\n".to_string()),
        }];
        let result = apply_diff(old, &diffs);
        assert_eq!(result, "line1\nline2\nappended");
    }

    #[test]
    fn test_apply_diff_insert_line_zero_no_panic() {
        // line_number=0：不因 usize 下溢 panic，视为第 1 行
        let old = "line1";
        let diffs = vec![FileDiff {
            diff_type: DiffType::Insert,
            line_number: 0,
            old_content: None,
            new_content: Some("top\n".to_string()),
        }];
        let result = apply_diff(old, &diffs);
        assert_eq!(result, "top\nline1");
    }

    #[test]
    fn test_apply_diff_insert() {
        let old = "line1\nline2";
        let diffs = vec![FileDiff {
            diff_type: DiffType::Insert,
            line_number: 2,
            old_content: None,
            new_content: Some("inserted\n".to_string()),
        }];
        let result = apply_diff(old, &diffs);
        assert_eq!(result, "line1\ninserted\nline2");
    }

    #[test]
    fn test_apply_diff_delete() {
        let old = "line1\nline2\nline3";
        let diffs = vec![FileDiff {
            diff_type: DiffType::Delete,
            line_number: 2,
            old_content: Some("line2\n".to_string()),
            new_content: None,
        }];
        let result = apply_diff(old, &diffs);
        assert_eq!(result, "line1\nline3");
    }

    #[test]
    fn test_apply_diff_replace() {
        let old = "line1\nline2\nline3";
        let diffs = vec![FileDiff {
            diff_type: DiffType::Replace,
            line_number: 2,
            old_content: Some("line2\n".to_string()),
            new_content: Some("modified\n".to_string()),
        }];
        let result = apply_diff(old, &diffs);
        assert_eq!(result, "line1\nmodified\nline3");
    }
}