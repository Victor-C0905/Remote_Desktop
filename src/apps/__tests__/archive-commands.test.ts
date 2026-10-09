import { describe, it, expect } from 'vitest';
import {
  classifyArchive, buildListCommand, buildExtractOneCommand, tmpDirFor,
} from '../ArchiveViewer/commands';

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
});

describe('buildListCommand 列表命令', () => {
  it('zip → unzip -l', () => {
    expect(buildListCommand('/x/a.zip', 'zip'))
      .toEqual({ command: 'unzip', args: ['-l', '/x/a.zip'] });
  });
  it('tar → tar -tvf（大小信息）', () => {
    expect(buildListCommand('/x/a.tar.gz', 'tar'))
      .toEqual({ command: 'tar', args: ['-tvf', '/x/a.tar.gz'] });
  });
  it('7z → 7z l -slt（机器可读）', () => {
    expect(buildListCommand('/x/a.7z', '7z'))
      .toEqual({ command: '7z', args: ['l', '-slt', '/x/a.7z'] });
  });
  it('rar → unrar l', () => {
    expect(buildListCommand('/x/a.rar', 'rar'))
      .toEqual({ command: 'unrar', args: ['l', '/x/a.rar'] });
  });
});

describe('buildExtractOneCommand 提取单条目（目录条目递归提取）', () => {
  it('zip', () => {
    expect(buildExtractOneCommand('/x/a.zip', 'dir/f.txt', 'zip', '/tmp/t'))
      .toEqual({ command: 'unzip', args: ['-o', '/x/a.zip', 'dir/f.txt', '-d', '/tmp/t'] });
  });
  it('tar（--no-wildcards 防条目名含通配符误匹配）', () => {
    expect(buildExtractOneCommand('/x/a.tar.gz', 'dir/f.txt', 'tar', '/tmp/t'))
      .toEqual({ command: 'tar', args: ['-xf', '/x/a.tar.gz', '-C', '/tmp/t', '--no-wildcards', '--', 'dir/f.txt'] });
  });
  it('7z', () => {
    expect(buildExtractOneCommand('/x/a.7z', 'dir/f.txt', '7z', '/tmp/t'))
      .toEqual({ command: '7z', args: ['x', '/x/a.7z', '-o/tmp/t', 'dir/f.txt', '-y'] });
  });
  it('rar', () => {
    expect(buildExtractOneCommand('/x/a.rar', 'dir/f.txt', 'rar', '/tmp/t'))
      .toEqual({ command: 'unrar', args: ['x', '-o+', '/x/a.rar', 'dir/f.txt', '/tmp/t/'] });
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
