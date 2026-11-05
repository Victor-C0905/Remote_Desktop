import { describe, it, expect } from 'vitest';
import { classifyArchive, tmpDirFor } from '../ArchiveViewer/commands';

describe('classifyArchive 按文件名推断压缩格式', () => {
  it('识别各扩展名', () => {
    expect(classifyArchive('a.zip')).toBe('zip');
    expect(classifyArchive('a.tar')).toBe('tar');
    expect(classifyArchive('a.tar.gz')).toBe('tar');
    expect(classifyArchive('a.tgz')).toBe('tar');
    expect(classifyArchive('a.tar.bz2')).toBe('tar');
    expect(classifyArchive('a.tar.xz')).toBe('tar');
    expect(classifyArchive('a.7z')).toBe('7z');
    expect(classifyArchive('a.rar')).toBe('rar');
    // 大小写不敏感
    expect(classifyArchive('A.ZIP')).toBe('zip');
  });
  it('非压缩包返回 null', () => {
    expect(classifyArchive('a.txt')).toBeNull();
    expect(classifyArchive('a.gz')).toBeNull(); // 单纯 .gz（非 .tar.gz）不支持
  });
  it('tar.zst 归为 tar 家族', () => {
    expect(classifyArchive('a.tar.zst')).toBe('tar');
  });
});

describe('tmpDirFor 临时提取目录', () => {
  it('格式为 /tmp/quireld-av/<stem>-<hash8>/', () => {
    const d = tmpDirFor('srv1', '/x/data.zip');
    expect(d).toMatch(/^\/tmp\/quireld-av\/data-[0-9a-f]{8}\/$/);
  });
  it('复合扩展名取 stem（a.tar.gz → a）', () => {
    expect(tmpDirFor('srv1', '/x/a.tar.gz')).toMatch(/^\/tmp\/quireld-av\/a-[0-9a-f]{8}\/$/);
  });
  it('同输入稳定、异输入不同', () => {
    expect(tmpDirFor('s', '/x/a.zip')).toBe(tmpDirFor('s', '/x/a.zip'));
    expect(tmpDirFor('s', '/x/a.zip')).not.toBe(tmpDirFor('s', '/x/b.zip'));
  });
});
