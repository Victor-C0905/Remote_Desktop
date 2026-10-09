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

    // 按行号排序（从大到小，避免索引错位）
    let mut sorted_diffs = diffs.to_vec();
    sorted_diffs.sort_by(|a, b| b.line_number.cmp(&a.line_number));

    // 应用差异
    for diff in sorted_diffs {
        let line_idx = diff.line_number - 1; // 转为 0-based 索引

        match diff.diff_type {
            DiffType::Insert => {
                if let Some(new_content) = &diff.new_content {
                    // lines() 方法会去掉换行符，因此数组中的元素不带 \n
                    // new_content 可能带尾部换行符（客户端按行发送），需要去掉
                    let content = new_content.trim_end_matches('\n');
                    lines.insert(line_idx, content.to_string());
                }
            }
            DiffType::Delete => {
                if line_idx < lines.len() {
                    lines.remove(line_idx);
                }
            }
            DiffType::Replace => {
                if line_idx < lines.len() {
                    if let Some(new_content) = &diff.new_content {
                        // 同 Insert，去掉尾部换行符避免 join 后多出空行
                        let content = new_content.trim_end_matches('\n');
                        lines[line_idx] = content.to_string();
                    }
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