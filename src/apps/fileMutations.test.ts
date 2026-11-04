import { describe, it, expect } from 'vitest';
import { applyBrowseChanges, applySearchChanges, parentDirOf, baseNameOf } from './fileMutations';
import type { FileEntry } from './fileTypes';

const entry = (name: string, isDir = false, extra: Partial<FileEntry> = {}): FileEntry => ({
  name,
  is_dir: isDir,
  size: 0,
  mtime: '2026-11-03T00:00:00Z',
  permissions: 'rwxr-xr-x',
  owner: 'root',
  group: 'root',
  ...extra,
});

describe('parentDirOf / baseNameOf', () => {
  it('根目录特判', () => {
    expect(parentDirOf('/')).toBe('/');
    expect(parentDirOf('/a')).toBe('/');
    expect(parentDirOf('/a/b')).toBe('/a');
    expect(baseNameOf('/a/b')).toBe('b');
  });
});

describe('applyBrowseChanges', () => {
  const cwd = '/home/u';

  it('插入新条目并清除 path 字段', () => {
    const out = applyBrowseChanges([entry('a')], cwd, [
      { kind: 'upsert', path: '/home/u/b', entry: entry('b', true, { path: '/x/y' }) },
    ]);
    expect(out.map((e) => e.name)).toEqual(['a', 'b']);
    expect(out[1].path).toBeUndefined();
  });

  it('同路径 upsert 覆盖元数据而非重复插入', () => {
    const out = applyBrowseChanges([entry('a', false, { size: 10 })], cwd, [
      { kind: 'upsert', path: '/home/u/a', entry: entry('a', false, { size: 99 }) },
    ]);
    expect(out).toHaveLength(1);
    expect(out[0].size).toBe(99);
  });

  it('忽略其他目录的 upsert/remove', () => {
    const list = [entry('a')];
    expect(applyBrowseChanges(list, cwd, [
      { kind: 'upsert', path: '/home/u/sub/b', entry: entry('b') },
      { kind: 'remove', path: '/home/u/sub/a' },
    ])).toEqual(list);
  });

  it('删除直属条目', () => {
    const out = applyBrowseChanges([entry('a'), entry('b')], cwd, [
      { kind: 'remove', path: '/home/u/a' },
    ]);
    expect(out.map((e) => e.name)).toEqual(['b']);
  });

  it('同目录改名保留原元数据', () => {
    const old = entry('a', false, { size: 5 });
    const out = applyBrowseChanges([old], cwd, [
      { kind: 'rename', from: '/home/u/a', to: '/home/u/b' },
    ]);
    expect(out[0].name).toBe('b');
    expect(out[0].size).toBe(5);
  });

  it('跨目录移出则移除', () => {
    const out = applyBrowseChanges([entry('a')], cwd, [
      { kind: 'rename', from: '/home/u/a', to: '/home/u/sub/a' },
    ]);
    expect(out).toEqual([]);
  });
});

describe('applySearchChanges', () => {
  const root = '/home/u';

  it('新命中按名称前缀与下钻层数过滤', () => {
    const out = applySearchChanges([], root, 'ab', 1, [
      { kind: 'upsert', path: '/home/u/abc', entry: entry('abc') },
      { kind: 'upsert', path: '/home/u/xyz', entry: entry('xyz') },
      { kind: 'upsert', path: '/home/u/sub1/abc', entry: entry('abc') },
      { kind: 'upsert', path: '/home/u/sub1/sub2/abc', entry: entry('abc') },
    ]);
    expect(out.map((e) => e.path)).toEqual(['/home/u/abc', '/home/u/sub1/abc']);
  });

  it('depth=0 仅 root 直属条目', () => {
    const out = applySearchChanges([], root, 'a', 0, [
      { kind: 'upsert', path: '/home/u/a', entry: entry('a') },
      { kind: 'upsert', path: '/home/u/sub/a', entry: entry('a') },
    ]);
    expect(out.map((e) => e.path)).toEqual(['/home/u/a']);
  });

  it('删除命中按完整路径移除', () => {
    const out = applySearchChanges(
      [entry('a', false, { path: '/home/u/sub/a' })],
      root, 'a', 1,
      [{ kind: 'remove', path: '/home/u/sub/a' }]
    );
    expect(out).toEqual([]);
  });

  it('删除目录时其下命中一并移除', () => {
    const out = applySearchChanges(
      [
        entry('sub', true, { path: '/home/u/sub' }),
        entry('a', false, { path: '/home/u/sub/a' }),
        entry('ab', false, { path: '/home/u/ab' }),
      ],
      root, 'a', 2,
      [{ kind: 'remove', path: '/home/u/sub' }]
    );
    expect(out.map((e) => e.path)).toEqual(['/home/u/ab']);
  });

  it('改名后不再匹配前缀则从结果中消失', () => {
    const out = applySearchChanges(
      [entry('abc', false, { path: '/home/u/abc' })],
      root, 'ab', 0,
      [{ kind: 'rename', from: '/home/u/abc', to: '/home/u/xyz' }]
    );
    expect(out).toEqual([]);
  });

  it('改名后仍匹配前缀则更新路径与名称', () => {
    const out = applySearchChanges(
      [entry('abc', false, { path: '/home/u/abc' })],
      root, 'ab', 0,
      [{ kind: 'rename', from: '/home/u/abc', to: '/home/u/abd' }]
    );
    expect(out[0].name).toBe('abd');
    expect(out[0].path).toBe('/home/u/abd');
  });

  it('目录改名时其后代命中路径同步改写', () => {
    const out = applySearchChanges(
      [entry('ab', true, { path: '/home/u/abc' }), entry('abd', false, { path: '/home/u/abc/abd' })],
      root, 'ab', 1,
      [{ kind: 'rename', from: '/home/u/abc', to: '/home/u/xyz' }]
    );
    // 目录自身不再匹配 'ab' 前缀而消失；后代名称仍匹配但父路径已改写
    expect(out.map((e) => e.path)).toEqual(['/home/u/xyz/abd']);
  });
});
