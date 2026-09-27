import { describe, it, expect } from 'vitest';
import { buildTree } from '../ArchiveViewer/tree';
import type { ArchiveEntry } from '../ArchiveViewer/parsers';

describe('buildTree 平铺条目 → 嵌套树', () => {
  it('隐含目录推导：只有文件路径也能生成目录节点', () => {
    const entries: ArchiveEntry[] = [{ path: 'a/b/f.txt', size: 10, isDir: false }];
    const tree = buildTree(entries);
    expect(tree).toHaveLength(1);
    expect(tree[0].name).toBe('a');
    expect(tree[0].isDir).toBe(true);
    expect(tree[0].children[0].name).toBe('b');
    expect(tree[0].children[0].children[0]).toMatchObject({ name: 'f.txt', size: 10, isDir: false });
  });

  it('目录显式行与隐含推导去重（size 不被覆盖）', () => {
    const entries: ArchiveEntry[] = [
      { path: 'dir/', size: 0, isDir: true },
      { path: 'dir/f.txt', size: 7, isDir: false },
    ];
    const tree = buildTree(entries);
    expect(tree).toHaveLength(1);
    expect(tree[0].children).toHaveLength(1);
    expect(tree[0].children[0].size).toBe(7);
  });

  it('目录 size 为子树累计', () => {
    const entries: ArchiveEntry[] = [
      { path: 'd/a.txt', size: 3, isDir: false },
      { path: 'd/sub/b.txt', size: 5, isDir: false },
    ];
    const tree = buildTree(entries);
    expect(tree[0].size).toBe(8);
  });

  it('根级文件与目录混排；目录在前按名、文件按名排序', () => {
    const entries: ArchiveEntry[] = [
      { path: 'z.txt', size: 1, isDir: false },
      { path: 'b/', size: 0, isDir: true },
      { path: 'a/', size: 0, isDir: true },
      { path: 'm.txt', size: 1, isDir: false },
    ];
    const tree = buildTree(entries);
    expect(tree.map((n) => n.name)).toEqual(['a', 'b', 'm.txt', 'z.txt']);
  });

  it('尾斜杠目录的显式行路径与子文件统一（d/ 与 d/x.txt）', () => {
    const entries: ArchiveEntry[] = [
      { path: 'd/', size: 0, isDir: true },
      { path: 'd/x.txt', size: 2, isDir: false },
    ];
    const tree = buildTree(entries);
    expect(tree[0].children.map((c: { name: string }) => c.name)).toEqual(['x.txt']);
  });
});
