/**
 * ArchiveViewer 树构建层（纯函数）
 *
 * 平铺 ArchiveEntry[] → 嵌套 ArchiveTreeNode[]：
 * - 目录条目缺失时由子路径隐含推导（zip 常见：只列文件不列目录）
 * - 同一目录的显式行与隐含推导去重（isDir 保留、文件 size 不被目录行覆盖）
 * - 目录 size = 子树文件累计
 * - 排序：目录在前，各自按 name 字典序
 */

import type { ArchiveEntry, ArchiveEntryKind } from './parsers';

export interface ArchiveTreeNode {
  name: string;
  /** 完整包内路径（无尾斜杠） */
  path: string;
  isDir: boolean;
  /** 条目类型（文件/目录/链接/特殊文件） */
  kind: ArchiveEntryKind;
  /** 链接目标（tar/7z 可识别时） */
  linkTarget?: string;
  /** 文件字节；目录为子树累计 */
  size: number;
  /** 目录才有；排序后 */
  children: ArchiveTreeNode[];
}

/** 内部构建节点（Map 按段名索引，isDir 冲突时目录优先） */
interface BuildNode {
  name: string;
  path: string;
  isDir: boolean;
  kind: ArchiveEntryKind;
  linkTarget?: string;
  size: number;      // 文件原值；目录构建完再累计
  children: Map<string, BuildNode>;
}

/** 递归转输出节点：目录 size 累计 + 子节点排序（目录在前按名） */
function toNode(n: BuildNode): ArchiveTreeNode {
  if (!n.isDir) {
    return {
      name: n.name,
      path: n.path,
      isDir: false,
      kind: n.kind,
      linkTarget: n.linkTarget,
      size: n.size,
      children: [],
    };
  }
  const children = [...n.children.values()].map(toNode)
    .sort((a, b) => (a.isDir === b.isDir ? a.name.localeCompare(b.name) : a.isDir ? -1 : 1));
  return {
    name: n.name,
    path: n.path,
    isDir: true,
    kind: 'dir',
    size: children.reduce((s, c) => s + c.size, 0),
    children,
  };
}

/** 当前目录的子节点（dirPath 为空表示包根目录） */
export function childrenAtPath(nodes: ArchiveTreeNode[], dirPath: string): ArchiveTreeNode[] {
  if (!dirPath) return nodes;
  let list = nodes;
  for (const seg of dirPath.split('/').filter(Boolean)) {
    const next = list.find((node) => node.isDir && node.name === seg);
    if (!next) return [];
    list = next.children;
  }
  return list;
}

/**
 * 选中集合中的最小根节点：祖先已选中时不再重复包含后代，
 * 避免提取/冲突比对时同一子树被计算两次。
 */
export function selectedRootNodes(
  nodes: ArchiveTreeNode[],
  selected: Set<string>,
): ArchiveTreeNode[] {
  const roots: ArchiveTreeNode[] = [];
  const walk = (list: ArchiveTreeNode[]) => {
    for (const node of list) {
      if (selected.has(node.path)) {
        roots.push(node);
      } else if (node.isDir) {
        walk(node.children);
      }
    }
  };
  walk(nodes);
  return roots;
}

/** 选中根节点展开为可提取的普通文件（链接/特殊文件跳过，按路径去重） */
export function collectExtractFiles(nodes: ArchiveTreeNode[]): ArchiveTreeNode[] {
  const files = new Map<string, ArchiveTreeNode>();
  const walk = (node: ArchiveTreeNode) => {
    if (node.isDir) {
      node.children.forEach(walk);
    } else if (node.kind === 'file') {
      files.set(node.path, node);
    }
  };
  nodes.forEach(walk);
  return [...files.values()];
}

export function buildTree(entries: ArchiveEntry[]): ArchiveTreeNode[] {
  const root = new Map<string, BuildNode>();

  /** 取段（或按需创建），dir 强制目录性；末段保留条目类型/链接目标 */
  const seg = (
    parent: Map<string, BuildNode>,
    name: string,
    parentPath: string,
    dir: boolean,
    kind?: ArchiveEntryKind,
    linkTarget?: string,
  ): BuildNode => {
    const existing = parent.get(name);
    if (existing) {
      if (kind && kind !== 'dir') {
        // 链接/特殊文件优先于“由子路径推导出的目录”，避免恶意
        // `link -> /etc` + `link/pwned` 被渲染成普通目录而漏过安全检查
        existing.kind = kind;
        existing.isDir = false;
        existing.linkTarget = linkTarget;
      } else if (dir && existing.kind !== 'symlink' && existing.kind !== 'hardlink' && existing.kind !== 'special') {
        // 目录推导只能提升普通文件；不能把已标记的链接/特殊文件改回目录
        existing.isDir = true;
        existing.kind = 'dir';
      }
      return existing;
    }
    const path = parentPath ? `${parentPath}/${name}` : name;
    const effectiveDir = dir && (!kind || kind === 'dir');
    const node: BuildNode = {
      name,
      path,
      isDir: effectiveDir,
      kind: effectiveDir ? 'dir' : (kind ?? 'file'),
      linkTarget: effectiveDir ? undefined : linkTarget,
      size: 0,
      children: new Map(),
    };
    parent.set(name, node);
    return node;
  };

  for (const e of entries) {
    // 目录显式行的尾斜杠去掉后统一分段
    const clean = e.path.replace(/\/+$/, '');
    if (!clean) continue; // 根目录行（个别工具会输出 "."）
    const parts = clean.split('/');
    // 祖先段全部按目录创建（隐含推导）；末段按条目自身类型
    let level = root;
    let acc = '';
    for (let i = 0; i < parts.length; i++) {
      const isLast = i === parts.length - 1;
      const node = seg(
        level, parts[i], acc,
        isLast ? e.isDir : true,
        isLast ? e.kind : undefined,
        isLast ? e.linkTarget : undefined,
      );
      acc = node.path;
      if (isLast && !e.isDir) {
        node.size = e.size; // 文件 size；目录行 size（0）不覆盖
      }
      level = node.children;
    }
  }

  return [...root.values()].map(toNode)
    .sort((a, b) => (a.isDir === b.isDir ? a.name.localeCompare(b.name) : a.isDir ? -1 : 1));
}
