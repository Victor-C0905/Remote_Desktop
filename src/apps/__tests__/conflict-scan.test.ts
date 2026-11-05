import {
  pathsToConflictNodes, scanConflicts, toConflictNodes,
  type ConflictNode, type TargetDirEntry,
} from '../ArchiveViewer/conflictScan';
import type { ArchiveTreeNode } from '../ArchiveViewer/tree';

interface Entry { name: string; is_dir: boolean }

/** 用固定目录表模拟 remote_read_dir；缺失路径抛错（等价目标不存在） */
function reader(dirs: Record<string, Entry[]>) {
  return async (path: string): Promise<TargetDirEntry[]> => {
    const entries = dirs[path];
    if (!entries) throw new Error(`missing: ${path}`);
    return entries;
  };
}

function file(name: string): Entry { return { name, is_dir: false }; }
function dir(name: string): Entry { return { name, is_dir: true }; }

function node(name: string, isDir: boolean, children: ConflictNode[] = []): ConflictNode {
  return { name, isDir, children };
}

function treeNode(name: string, isDir: boolean, children: ArchiveTreeNode[] = []): ArchiveTreeNode {
  return { name, path: name, isDir, kind: isDir ? 'dir' : 'file', size: 0, children };
}

describe('scanConflicts 递归冲突检测', () => {
  it('顶层同名文件算冲突', async () => {
    const result = await scanConflicts(
      '/out', [node('a.txt', false)],
      reader({ '/out': [file('a.txt')] }),
    );
    expect(result).toEqual({ count: 1, paths: ['a.txt'], scannedDirs: 1, truncated: false });
  });

  it('同名目录下存在同名文件才算冲突（目录合并本身不算）', async () => {
    const result = await scanConflicts(
      '/out',
      [node('dir', true, [node('a.txt', false), node('b.txt', false)])],
      reader({
        '/out': [dir('dir')],
        '/out/dir': [file('a.txt')],
      }),
    );
    expect(result.count).toBe(1);
    expect(result.paths).toEqual(['dir/a.txt']);
    expect(result.scannedDirs).toBe(2);
  });

  it('同名目录存在但内部无同名项 → 无冲突（直接合并）', async () => {
    const result = await scanConflicts(
      '/out',
      [node('dir', true, [node('a.txt', false)])],
      reader({
        '/out': [dir('dir')],
        '/out/dir': [file('other.txt')],
      }),
    );
    expect(result.count).toBe(0);
  });

  it('类型冲突：归档目录 vs 目标文件算冲突，且不再向下扫描', async () => {
    const result = await scanConflicts(
      '/out',
      [node('dir', true, [node('a.txt', false)])],
      reader({ '/out': [file('dir')] }),
    );
    expect(result).toEqual({ count: 1, paths: ['dir'], scannedDirs: 1, truncated: false });
  });

  it('目标目录不存在 → 无冲突', async () => {
    const result = await scanConflicts(
      '/out', [node('a.txt', false)], reader({}),
    );
    expect(result).toEqual({ count: 0, paths: [], scannedDirs: 0, truncated: false });
  });

  it('超过扫描上限时标记 truncated', async () => {
    const result = await scanConflicts(
      '/out',
      [node('a', true, [node('x', true)]), node('b', true, [node('y', true)])],
      reader({
        '/out': [dir('a'), dir('b')],
        '/out/a': [dir('x')],
      }),
      1,
    );
    expect(result.truncated).toBe(true);
    expect(result.scannedDirs).toBe(1);
  });
});

describe('冲突树构建', () => {
  it('归档树保留目录结构', () => {
    const nodes = toConflictNodes([
      treeNode('dir', true, [treeNode('a.txt', false)]),
      treeNode('b.txt', false),
    ]);
    expect(nodes).toEqual([
      node('dir', true, [node('a.txt', false)]),
      node('b.txt', false),
    ]);
  });

  it('勾选路径自动补齐上级目录骨架并去重', () => {
    const nodes = pathsToConflictNodes(['dir/sub/a.txt', 'dir/b.txt', 'dir/sub/a.txt']);
    expect(nodes).toEqual([
      node('dir', true, [
        node('sub', true, [node('a.txt', false)]),
        node('b.txt', false),
      ]),
    ]);
  });
});
