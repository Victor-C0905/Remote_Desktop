import { describe, it, expect } from 'vitest';
import {
  parseUnzipListing, parseTarListing, parse7zListing, parseUnrarListing, type ArchiveEntry,
} from '../ArchiveViewer/parsers';

describe('parseUnzipListing（unzip -l 表格）', () => {
  const OUT = `Archive:  r.zip
  Length      Date    Time    Name
---------  ---------- -----   ----
      123  2024-01-01 12:00   dir/file.txt
        0  2024-01-01 12:00   dir/
---------                     -------
      123                     2 files`;
  it('解析文件与目录行', () => {
    expect(parseUnzipListing(OUT)).toEqual<ArchiveEntry[]>([
      { path: 'dir/file.txt', size: 123, isDir: false },
      { path: 'dir/', size: 0, isDir: true },
    ]);
  });
  it('空输出抛错（格式不可识别而非静默空表）', () => {
    expect(() => parseUnzipListing('')).toThrow();
  });
});

describe('parseTarListing（tar -tvf 详单，GNU tar 真实输出）', () => {
  // WSL GNU tar 校准：老时间戳同样带 HH:MM；符号链接为 `name -> target`
  const OUT = `drwxr-xr-x root/root         0 2026-09-22 17:13 dir/
-rw-r--r-- root/root         0 2026-09-22 17:13 dir/empty.bin
-rw-r--r-- root/root         6 2026-09-22 17:13 dir/file.txt
-rw-r--r-- root/root        56 2026-09-22 17:13 ./top.txt
lrwxrwxrwx root/root         0 2026-09-22 17:13 lnk -> dir/file.txt
-rw-r--r-- root/root         0 2020-01-01 12:00 old.txt`;
  it('解析权限位/大小/路径；d 前缀或尾斜杠判目录；去 ./ 前缀；链接目标丢弃', () => {
    expect(parseTarListing(OUT)).toEqual<ArchiveEntry[]>([
      { path: 'dir/', size: 0, isDir: true },
      { path: 'dir/empty.bin', size: 0, isDir: false },
      { path: 'dir/file.txt', size: 6, isDir: false },
      { path: 'top.txt', size: 56, isDir: false },
      { path: 'lnk', size: 0, isDir: false },
      { path: 'old.txt', size: 0, isDir: false },
    ]);
  });
  it('空输出抛错', () => {
    expect(() => parseTarListing('')).toThrow();
  });
  it('一行都不匹配抛错（不静默空表）', () => {
    expect(() => parseTarListing('随机输出\n格式不认识')).toThrow();
  });
});

describe('parse7zListing（7z l -slt key=value）', () => {
  const OUT = `
Listing archive: r.7z

--
Path = dir/file.txt
Folder = -
Size = 1234
Attributes = A_ -rw-r--r--

Path = dir
Folder = +
Size = 0

Path = top.txt
Folder = -
Size = 56`;
  it('按块解析；Folder=+ 判目录', () => {
    expect(parse7zListing(OUT)).toEqual<ArchiveEntry[]>([
      { path: 'dir/file.txt', size: 1234, isDir: false },
      { path: 'dir', size: 0, isDir: true },
      { path: 'top.txt', size: 56, isDir: false },
    ]);
  });
  it('缺 Size 的目录条目 size 为 0', () => {
    const r = parse7zListing('Path = d\nFolder = +');
    expect(r).toEqual<ArchiveEntry[]>([{ path: 'd', size: 0, isDir: true }]);
  });
  it('空输出抛错', () => {
    expect(() => parse7zListing('')).toThrow();
  });
});

describe('parseUnrarListing（unrar l 表格，unrar 5+）', () => {
  const OUT = `UNRAR 6.24

Archive: r.rar
Details: RAR 5

 Size      Packed Ratio  Date    Time    Attr    Name
--------  ------ ----- ---------- -----  -------  ---------
     1234       1234  100%  2024-01-01 12:00  -rw-r--r--  dir/file.txt
         0          0    0%  2024-01-01 12:00  drw-r--r--  dir`;
  it('Attr 首字符 d 或尾斜杠判目录', () => {
    expect(parseUnrarListing(OUT)).toEqual<ArchiveEntry[]>([
      { path: 'dir/file.txt', size: 1234, isDir: false },
      { path: 'dir', size: 0, isDir: true },
    ]);
  });
  it('空输出抛错', () => {
    expect(() => parseUnrarListing('')).toThrow();
  });
});
