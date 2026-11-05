/**
 * 压缩包条目路径安全判定（客户端侧最后一道防线）
 *
 * 工具（tar/unzip/7z/unrar）自身的 Zip Slip 防护因版本而异；客户端在
 * 拼接临时目录路径、构造 argv 之前必须先做规范化校验：
 * - 拒绝绝对路径与 `..` 段，防止计算出的本地路径逃出提取目录；
 * - 单条目提取额外拒绝以 `-` 开头的文件名，避免被工具当成命令行开关。
 */

import type { ArchiveTreeNode } from './tree';
import type { ArchiveEntryKind } from './parsers';

/** 阻止「全部解压」的安全问题类型 */
export type ArchiveSafetyIssue = 'traversal' | 'special' | 'unsafeLink';

/** 绝对路径 / .. / NUL：任何操作都不应接受 */
export function hasTraversal(path: string): boolean {
  if (!path || path.includes('\0')) return true;
  if (path.startsWith('/')) return true;
  return path.split('/').some((part) => part === '..');
}

/** 单条目提取的额外校验：basename 以 - 开头会触发工具选项解析 */
export function isSafeExtractOnePath(path: string): boolean {
  if (hasTraversal(path)) return false;
  const parts = path.split('/');
  const base = parts[parts.length - 1] ?? '';
  return !base.startsWith('-');
}

/** 只有普通文件允许单独提取/打开；链接/特殊文件一律拒绝 */
export function isSafeExtractOneEntry(path: string, kind?: ArchiveEntryKind): boolean {
  return isSafeExtractOnePath(path) && (kind ?? 'file') === 'file';
}

/** 链接目标未知或绝对/含 .. 时视为不安全 */
export function isUnsafeLinkTarget(target: string | undefined): boolean {
  if (!target) return true;
  return hasTraversal(target);
}

/** 整棵树是否包含绝对路径或 .. 条目（用于阻止「全部解压」） */
export function treeHasTraversal(nodes: ArchiveTreeNode[]): boolean {
  for (const node of nodes) {
    if (hasTraversal(node.path)) return true;
    if (node.isDir && treeHasTraversal(node.children)) return true;
  }
  return false;
}

/**
 * 找出阻止全部解压的首个安全问题：
 * - traversal：条目路径为绝对路径或含 ..
 * - special：设备/FIFO/socket 等特殊文件（root 下 tar 会真的创建）
 * - unsafeLink：链接目标未知，或为绝对路径/含 ..
 */
export function findArchiveSafetyIssue(nodes: ArchiveTreeNode[]): ArchiveSafetyIssue | null {
  for (const node of nodes) {
    if (hasTraversal(node.path)) return 'traversal';
    if (node.kind === 'special') return 'special';
    if ((node.kind === 'symlink' || node.kind === 'hardlink') && isUnsafeLinkTarget(node.linkTarget)) {
      return 'unsafeLink';
    }
    if (node.isDir) {
      const child = findArchiveSafetyIssue(node.children);
      if (child) return child;
    }
  }
  return null;
}
