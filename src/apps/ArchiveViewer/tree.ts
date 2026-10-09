/**
 * ArchiveViewer 树构建层（纯函数）
 *
 * 平铺 ArchiveEntry[] → 嵌套 ArchiveTreeNode[]：
 * - 目录条目缺失时由子路径隐含推导（zip 常见：只列文件不列目录）
 * - 同一目录的显式行与隐含推导去重（isDir 保留、文件 size 不被目录行覆盖）
 * - 目录 size = 子树文件累计
 * - 排序：目录在前，各自按 name 字典序
 */

import type { ArchiveEntry } from './parsers';

export interface ArchiveTreeNode {
  name: string;
  /** 完整包内路径（无尾斜杠） */
  path: string;
  isDir: boolean;
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
  size: number;      // 文件原值；目录构建完再累计
  children: Map<string, BuildNode>;
}

/** 递归转输出节点：目录 size 累计 + 子节点排序（目录在前按名） */
function toNode(n: BuildNode): ArchiveTreeNode {
  if (!n.isDir) {
    return { name: n.name, path: n.path, isDir: false, size: n.size, children: [] };
  }
  const children = [...n.children.values()].map(toNode)
    .sort((a, b) => (a.isDir === b.isDir ? a.name.localeCompare(b.name) : a.isDir ? -1 : 1));
  return {
    name: n.name,
    path: n.path,
    isDir: true,
    size: children.reduce((s, c) => s + c.size, 0),
    children,
  };
}

export function buildTree(entries: ArchiveEntry[]): ArchiveTreeNode[] {
  const root = new Map<string, BuildNode>();

  /** 取段（或按需创建），dir 强制目录性 */
  const seg = (parent: Map<string, BuildNode>, name: string, parentPath: string, dir: boolean): BuildNode => {
    const existing = parent.get(name);
    if (existing) {
      if (dir) existing.isDir = true;
      return existing;
    }
    const path = parentPath ? `${parentPath}/${name}` : name;
    const node: BuildNode = { name, path, isDir: dir, size: 0, children: new Map() };
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
      const node = seg(level, parts[i], acc, isLast ? e.isDir : true);
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
