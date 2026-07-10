// src/apps/TextEditor/utils/diff.ts
/**
 * 文件差异计算工具（客户端侧）
 *
 * 使用 LCS（最长公共子序列）算法计算差异
 * 用于文件编辑器的流量优化，只传输修改的部分
 */

import type { FileChange } from '../types/editor';

/**
 * 计算两个文本的差异（LCS 算法）
 *
 * @param oldContent - 原文本
 * @param newContent - 新文本
 * @returns 差异列表
 */
export function calculateDiff(oldContent: string, newContent: string): FileChange[] {
  const oldLines = oldContent.split('\n');
  const newLines = newContent.split('\n');

  // 使用 LCS 算法计算最长公共子序列
  const lcs = longestCommonSubsequence(oldLines, newLines);

  // 基于 LCS 生成差异
  const diffs = generateDiff(oldLines, newLines, lcs);

  return diffs;
}

/**
 * 最长公共子序列（LCS）算法
 *
 * @param a - 旧文本行数组
 * @param b - 新文本行数组
 * @returns 最长公共子序列（行数组）
 */
function longestCommonSubsequence(a: string[], b: string[]): string[] {
  // DP 表
  const dp: number[][] = Array(a.length + 1)
    .fill(0)
    .map(() => Array(b.length + 1).fill(0));

  // 填充 DP 表
  for (let i = 1; i <= a.length; i++) {
    for (let j = 1; j <= b.length; j++) {
      if (a[i - 1] === b[j - 1]) {
        dp[i][j] = dp[i - 1][j - 1] + 1;
      } else {
        dp[i][j] = Math.max(dp[i][j - 1], dp[i - 1][j]);
      }
    }
  }

  // 回溯找 LCS
  const lcs: string[] = [];
  let i = a.length;
  let j = b.length;

  while (i > 0 && j > 0) {
    if (a[i - 1] === b[j - 1]) {
      lcs.push(a[i - 1]);
      i--;
      j--;
    } else if (dp[i - 1][j] > dp[i][j - 1]) {
      i--;
    } else {
      j--;
    }
  }

  return lcs.reverse();
}

/**
 * 基于 LCS 生成差异列表
 *
 * @param oldLines - 旧文本行数组
 * @param newLines - 新文本行数组
 * @param lcs - 最长公共子序列
 * @returns 差异列表（客户端 FileChange 格式）
 *
 * @example
 * ```typescript
 * const diffs = generateDiff(oldLines, newLines, lcs);
 * const tauriDiffs = convertToTauriFormat(diffs); // 转换为 Tauri 格式
 * ```
 */
function generateDiff(oldLines: string[], newLines: string[], lcs: string[]): FileChange[] {
  const diffs: FileChange[] = [];
  let oldIdx = 0;
  let newIdx = 0;
  let lcsIdx = 0;

  while (oldIdx < oldLines.length || newIdx < newLines.length) {
    if (lcsIdx < lcs.length && oldIdx < oldLines.length && newIdx < newLines.length) {
      // 检查是否匹配 LCS
      if (oldLines[oldIdx] === lcs[lcsIdx] && newLines[newIdx] === lcs[lcsIdx]) {
        // 相等，无差异
        oldIdx++;
        newIdx++;
        lcsIdx++;
        continue;
      }
    }

    // 检查删除（旧文本有，新文本没有）
    if (oldIdx < oldLines.length && (lcsIdx >= lcs.length || oldLines[oldIdx] !== lcs[lcsIdx])) {
      diffs.push({
        type: 'delete',
        line: oldIdx + 1, // 行号从 1 开始
        old: oldLines[oldIdx],
      });
      oldIdx++;
      continue;
    }

    // 检查插入（新文本有，旧文本没有）
    if (newIdx < newLines.length && (lcsIdx >= lcs.length || newLines[newIdx] !== lcs[lcsIdx])) {
      diffs.push({
        type: 'insert',
        line: newIdx + 1, // 行号从 1 开始
        new: newLines[newIdx],
      });
      newIdx++;
      continue;
    }

    // 安全退出
    if (oldIdx < oldLines.length) oldIdx++;
    if (newIdx < newLines.length) newIdx++;
  }

  return diffs;
}

/**
 * Tauri 端期望的差异格式（对应 Rust 的 FileDiff 结构体）
 */
interface TauriFileDiff {
  diff_type: 'insert' | 'delete' | 'replace';
  line_number: number;
  old_content?: string;
  new_content?: string;
}

/**
 * 将客户端 FileChange 格式转换为 Tauri 端 FileDiff 格式
 *
 * 用于发送到 Agent 的差异同步接口
 *
 * @param diffs - 客户端格式的差异列表
 * @returns Tauri 端格式的差异列表
 *
 * @example
 * ```typescript
 * const clientDiffs = calculateDiff(oldContent, newContent);
 * const tauriDiffs = convertToTauriFormat(clientDiffs);
 * await invoke('remote_apply_diff', { diffs: tauriDiffs });
 * ```
 */
export function convertToTauriFormat(diffs: FileChange[]): TauriFileDiff[] {
  return diffs.map(diff => {
    const result: TauriFileDiff = {
      diff_type: diff.type,
      line_number: diff.line,
    };

    // 添加可选字段（根据差异类型）
    if (diff.type === 'delete' && 'old' in diff) {
      result.old_content = diff.old;
    } else if (diff.type === 'insert' && 'new' in diff) {
      result.new_content = diff.new;
    } else if (diff.type === 'replace') {
      if ('old' in diff) result.old_content = diff.old;
      if ('new' in diff) result.new_content = diff.new;
    }

    return result;
  });
}

/**
 * 应用差异到原文本（用于测试）
 *
 * @param oldContent - 原文本
 * @param diffs - 差异列表
 * @returns 新文本
 */
export function applyDiff(oldContent: string, diffs: FileChange[]): string {
  const lines = oldContent.split('\n');

  // 按行号排序（从大到小，避免索引错位）
  const sortedDiffs = [...diffs].sort((a, b) => b.line - a.line);

  // 应用差异
  for (const diff of sortedDiffs) {
    const lineIdx = diff.line - 1; // 转为 0-based 索引

    switch (diff.type) {
      case 'insert':
        if (diff.new) {
          lines.splice(lineIdx, 0, diff.new);
        }
        break;

      case 'delete':
        if (lineIdx < lines.length) {
          lines.splice(lineIdx, 1);
        }
        break;

      case 'replace':
        if (lineIdx < lines.length && diff.new) {
          lines[lineIdx] = diff.new;
        }
        break;
    }
  }

  return lines.join('\n');
}

/**
 * 测试差异计算
 */
export function testDiff(): void {
  const oldText = 'line1\nline2\nline3';
  const newText = 'line1\nmodified\nline3';
  const diffs = calculateDiff(oldText, newText);
  console.log('Diffs:', diffs);

  const applied = applyDiff(oldText, diffs);
  console.log('Applied:', applied);
  console.log('Match:', applied === newText);
}