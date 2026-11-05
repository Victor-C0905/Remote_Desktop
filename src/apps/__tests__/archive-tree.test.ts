import { describe, it, expect } from 'vitest';
import {
  buildTree, childrenAtPath, collectExtractFiles, selectedRootNodes,
} from '../ArchiveViewer/tree';
import { findArchiveSafetyIssue } from '../ArchiveViewer/pathSafety';
import type { ArchiveEntry } from '../ArchiveViewer/parsers';

describe('buildTree 平铺条目 → 嵌套树', () => {
  it('隐含目录推导：只有文件路径也能生成目录节点', () => {
    const entries: ArchiveEntry[] = [{ path: 'a/b/f.txt', size: 10, isDir: false, kind: 'file' }];
    const tree = buildTree(entries);
    expect(tree).toHaveLength(1);
    expect(tree[0].name).toBe('a');
    expect(tree[0].isDir).toBe(true);
    expect(tree[0].children[0].name).toBe('b');
    expect(tree[0].children[0].children[0]).toMatchObject({ name: 'f.txt', size: 10, isDir: false });
  });

  it('目录显式行与隐含推导去重（size 不被覆盖）', () => {
    const entries: ArchiveEntry[] = [
      { path: 'dir/', size: 0, isDir: true, kind: 'dir' },
      { path: 'dir/f.txt', size: 7, isDir: false, kind: 'file' },
    ];
    const tree = buildTree(entries);
    expect(tree).toHaveLength(1);
    expect(tree[0].children).toHaveLength(1);
    expect(tree[0].children[0].size).toBe(7);
  });

  it('目录 size 为子树累计', () => {
    const entries: ArchiveEntry[] = [
      { path: 'd/a.txt', size: 3, isDir: false, kind: 'file' },
      { path: 'd/sub/b.txt', size: 5, isDir: false, kind: 'file' },
    ];
    const tree = buildTree(entries);
    expect(tree[0].size).toBe(8);
  });

  it('根级文件与目录混排；目录在前按名、文件按名排序', () => {
    const entries: ArchiveEntry[] = [
      { path: 'z.txt', size: 1, isDir: false, kind: 'file' },
      { path: 'b/', size: 0, isDir: true, kind: 'dir' },
      { path: 'a/', size: 0, isDir: true, kind: 'dir' },
      { path: 'm.txt', size: 1, isDir: false, kind: 'file' },
    ];
    const tree = buildTree(entries);
    expect(tree.map((n) => n.name)).toEqual(['a', 'b', 'm.txt', 'z.txt']);
  });

  it('链接 + 子路径仍按链接标记，安全检查不被目录推导绕过', () => {
    const entries: ArchiveEntry[] = [
      { path: 'link', size: 0, isDir: false, kind: 'symlink', linkTarget: '/etc' },
      { path: 'link/pwned.txt', size: 4, isDir: false, kind: 'file' },
    ];
    const tree = buildTree(entries);
    expect(tree).toHaveLength(1);
    expect(tree[0].kind).toBe('symlink');
    expect(tree[0].isDir).toBe(false);
    expect(findArchiveSafetyIssue(tree)).toBe('unsafeLink');
  });

  it('导航与选中辅助：子目录查找、最小根节点、文件展开', () => {
    const entries: ArchiveEntry[] = [
      { path: 'a/f1.txt', size: 1, isDir: false, kind: 'file' },
      { path: 'a/f2.txt', size: 2, isDir: false, kind: 'file' },
      { path: 'a/link', size: 0, isDir: false, kind: 'symlink', linkTarget: '/etc' },
      { path: 'b.txt', size: 3, isDir: false, kind: 'file' },
    ];
    const tree = buildTree(entries);
    expect(childrenAtPath(tree, '').map((n) => n.name)).toEqual(['a', 'b.txt']);
    expect(childrenAtPath(tree, 'a').map((n) => n.name)).toEqual(['f1.txt', 'f2.txt', 'link']);
    expect(childrenAtPath(tree, 'missing')).toEqual([]);

    // 祖先已选中时不再计入后代，避免同一子树重复提取
    const roots = selectedRootNodes(tree, new Set(['a', 'a/f1.txt', 'b.txt']));
    expect(roots.map((n) => n.path)).toEqual(['a', 'b.txt']);
    // 目录展开为普通文件；链接/特殊文件跳过
    expect(collectExtractFiles(roots).map((f) => f.path))
      .toEqual(['a/f1.txt', 'a/f2.txt', 'b.txt']);
  });

  it('尾斜杠目录的显式行路径与子文件统一（d/ 与 d/x.txt）', () => {
    const entries: ArchiveEntry[] = [
      { path: 'd/', size: 0, isDir: true, kind: 'dir' },
      { path: 'd/x.txt', size: 2, isDir: false, kind: 'file' },
    ];
    const tree = buildTree(entries);
    expect(tree[0].children.map((c: { name: string }) => c.name)).toEqual(['x.txt']);
  });
});
