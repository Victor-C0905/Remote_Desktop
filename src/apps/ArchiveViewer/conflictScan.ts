/**
 * 解压冲突扫描（纯逻辑 + 注入目录读取）
 *
 * 目的：解压前找出目标位置真正会与归档冲突的条目，而不是只看顶层。
 * - 顶层同名普通文件 → 冲突
 * - 顶层同名目录 → 目录合并本身不算冲突，继续向下比对子项
 * - 文件 vs 目录（任一侧）→ 冲突
 * - 目标不存在 / 不可读 → 该子树按无冲突处理，真实错误交给解压命令
 *
 * 只读取「归档与目标都存在」的目录，正常解压只有少量 read_dir 调用。
 */

import type { ArchiveTreeNode } from './tree';

export interface ConflictNode {
  name: string;
  isDir: boolean;
  children: ConflictNode[];
}

export interface ConflictScanResult {
  /** 真正会冲突的条目数（同名文件 / 类型冲突） */
  count: number;
  /** 冲突路径预览（相对目标目录，最多 MAX_CONFLICT_PREVIEW 条） */
  paths: string[];
  /** 已读取的目录数 */
  scannedDirs: number;
  /** 超过扫描上限，仍有目录未检查 */
  truncated: boolean;
}

/** 单次扫描最多读取的目录数（防止目标巨大时拖慢解压确认） */
export const MAX_CONFLICT_SCAN_DIRS = 200;
/** 冲突预览最多展示的路径数 */
export const MAX_CONFLICT_PREVIEW = 20;

/** 归档树 → 冲突比对树 */
export function toConflictNodes(nodes: ArchiveTreeNode[]): ConflictNode[] {
  return nodes.map((node) => ({
    name: node.name,
    isDir: node.isDir,
    children: toConflictNodes(node.children),
  }));
}

/** 勾选文件路径 → 冲突比对树（自动补齐上级目录骨架） */
export function pathsToConflictNodes(paths: string[]): ConflictNode[] {
  const root: ConflictNode[] = [];
  const dirIndex = new Map<string, ConflictNode>();

  /** 建好并返回 dirs 路径对应的 children 列表 */
  const ensureDir = (segments: string[]): ConflictNode[] => {
    let list = root;
    let full = '';
    for (const seg of segments) {
      full = full ? `${full}/${seg}` : seg;
      let dir = dirIndex.get(full);
      if (!dir) {
        dir = { name: seg, isDir: true, children: [] };
        dirIndex.set(full, dir);
        list.push(dir);
      }
      list = dir.children;
    }
    return list;
  };

  for (const path of paths) {
    const segments = path.split('/').filter(Boolean);
    if (segments.length === 0) continue;
    const parent = ensureDir(segments.slice(0, -1));
    const name = segments[segments.length - 1];
    if (!parent.some((node) => node.name === name && !node.isDir)) {
      parent.push({ name, isDir: false, children: [] });
    }
  }
  return root;
}

export interface TargetDirEntry {
  name: string;
  is_dir: boolean;
}

/**
 * 递归比对目标目录与归档节点。
 *
 * @param readDir 读取目标目录顶层条目；抛错表示目录不存在/不可读（跳过该子树）
 */
export async function scanConflicts(
  targetDir: string,
  nodes: ConflictNode[],
  readDir: (path: string) => Promise<TargetDirEntry[]>,
  maxDirs = MAX_CONFLICT_SCAN_DIRS,
): Promise<ConflictScanResult> {
  let count = 0;
  let scannedDirs = 0;
  let truncated = false;
  const paths: string[] = [];
  // rel：相对目标目录的内部路径，用于冲突预览展示
  const queue: Array<{ dir: string; rel: string; nodes: ConflictNode[] }> = [
    { dir: targetDir, rel: '', nodes },
  ];

  while (queue.length > 0) {
    if (scannedDirs >= maxDirs) {
      truncated = true;
      break;
    }
    const current = queue.shift()!;
    let entries: TargetDirEntry[];
    try {
      entries = await readDir(current.dir);
    } catch {
      // 目录不存在（独立文件夹首次解压）或不可读：跳过该子树
      continue;
    }
    scannedDirs += 1;
    const byName = new Map(entries.map((entry) => [entry.name, entry]));

    for (const node of current.nodes) {
      const existing = byName.get(node.name);
      if (!existing) continue;
      // 文件 vs 任何已存在项、目录 vs 文件：都是实质冲突
      if (!node.isDir || !existing.is_dir) {
        count += 1;
        if (paths.length < MAX_CONFLICT_PREVIEW) {
          paths.push(current.rel ? `${current.rel}/${node.name}` : node.name);
        }
        continue;
      }
      // 目录 vs 目录：允许合并，继续检查下一层
      if (node.children.length > 0) {
        const base = current.dir.replace(/\/$/, '');
        queue.push({
          dir: `${base}/${node.name}`,
          rel: current.rel ? `${current.rel}/${node.name}` : node.name,
          nodes: node.children,
        });
      }
    }
  }

  return { count, paths, scannedDirs, truncated };
}
