# ArchiveViewer 压缩包内容浏览 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 双击压缩包打开 ArchiveViewer 独立窗口：浏览内容树（名称/大小/类型）、双击包内文件提取并按分类系统打开、工具栏触发全部解压。

**Architecture:** 纯函数层（commands/parsers/tree，全部可单测）+ UI 层（ArchiveViewer.tsx 单栏树视图）+ 接线（init.ts 窗口注册、FileManager.tsx archive 分支改造）。全程复用现有协议：`remote_execute_command`（白名单命令）+ `remote_mkdir` + `detectFileFormat`/`decideOpenTarget`（FileOpener 纯决策复用）。零新协议、零 agent 端改动。

**Tech Stack:** React + TypeScript + vitest + Tauri invoke。

**设计文档:** `docs/superpowers/specs/2026-09-17-archive-viewer-design.md`

**Git 规则（用户约束）:** 本计划所有「提交」步骤均为**提示用户执行**，执行者不得操作 git add/commit。

---

### Task 1: 命令构造层 commands.ts（TDD）

**Files:**
- Create: `src/apps/ArchiveViewer/commands.ts`
- Test: `src/apps/__tests__/archive-commands.test.ts`

- [ ] **Step 1: 写失败测试**

```ts
// src/apps/__tests__/archive-commands.test.ts
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `npm run test:run -- src/apps/__tests__/archive-commands.test.ts`（在 `e:\MyWork\gnome-remote` 下）
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现 commands.ts**

```ts
// src/apps/ArchiveViewer/commands.ts
/**
 * ArchiveViewer 命令构造层（纯函数）
 *
 * 职责：按压缩格式构造「列内容」「提取单条目」白名单命令 + 服务器临时目录路径。
 * 命令由前端构造、经 remote_execute_command 发往服务器 Worker argv 直执行。
 */

/** 支持的压缩格式 */
export type ArchiveKind = 'zip' | 'tar' | '7z' | 'rar';

/** 统一命令形态（与 buildExtractCommand 一致） */
export interface ShellCommand {
  command: string;
  args: string[];
}

/**
 * 从文件名推断压缩格式
 *
 * 注意：.gz/.bz2/.xz 单独出现（非 .tar.gz 等）不支持。
 * 大小写不敏感。
 */
export function classifyArchive(fileName: string): ArchiveKind | null {
  const lower = fileName.toLowerCase();
  if (lower.endsWith('.zip')) return 'zip';
  if (/\.(tar|tar\.gz|tgz|tar\.bz2|tar\.xz)$/.test(lower)) return 'tar';
  if (lower.endsWith('.7z')) return '7z';
  if (lower.endsWith('.rar')) return 'rar';
  return null;
}

/**
 * 构造「列内容」命令
 * - tar 用 -tvf（详单：权限/大小/时间）
 * - 7z 用 -slt（机器可读 key=value）
 * - zip/rar 用 -l 表格输出
 */
export function buildListCommand(archivePath: string, kind: ArchiveKind): ShellCommand {
  switch (kind) {
    case 'zip': return { command: 'unzip', args: ['-l', archivePath] };
    case 'tar': return { command: 'tar', args: ['-tvf', archivePath] };
    case '7z':  return { command: '7z', args: ['l', '-slt', archivePath] };
    case 'rar': return { command: 'unrar', args: ['l', archivePath] };
  }
}

/**
 * 构造「提取单条目」命令
 *
 * 条目为目录时：四种工具均递归提取整个子树（期望行为：浏览目录=提取该目录全部内容）。
 * tar 加 --no-wildcards：GNU tar 默认把命令行条目名按 glob 模式匹配，
 * 条目名含 * ? [ 时会误提取，显式关闭保证字面匹配（GNU tar 基线；见 spec 边界）。
 */
export function buildExtractOneCommand(
  archivePath: string,
  entryPath: string,
  kind: ArchiveKind,
  targetDir: string,
): ShellCommand {
  switch (kind) {
    case 'zip': return { command: 'unzip', args: ['-o', archivePath, entryPath, '-d', targetDir] };
    case 'tar': return { command: 'tar', args: ['-xf', archivePath, '-C', targetDir, '--no-wildcards', '--', entryPath] };
    case '7z':  return { command: '7z', args: ['x', archivePath, `-o${targetDir}`, entryPath, '-y'] };
    case 'rar': return { command: 'unrar', args: ['x', '-o+', archivePath, entryPath, `${targetDir}/`] };
  }
}

/**
 * 提取临时目录：/tmp/quireld-av/<stem>-<hash8>/
 *
 * 同一 (serverId, 压缩包路径) 稳定复用同一目录（重复打开不重复占空间，-o 覆盖）。
 * 不主动清理（spec §9：/tmp 重启自清，避免误删用户编辑中的副本）。
 */
export function tmpDirFor(serverId: string, archivePath: string): string {
  const base = archivePath.split('/').pop() ?? 'archive';
  const stem = base.replace(/\.(zip|tar|tar\.gz|tgz|tar\.bz2|tar\.xz|7z|rar)$/i, '') || 'archive';
  return `/tmp/quireld-av/${stem}-${hash8(serverId + archivePath)}/`;
}

/** djb2 哈希取低 32 位，8 位 hex（非加密用途，仅隔离目录） */
function hash8(input: string): string {
  let h = 5381;
  for (let i = 0; i < input.length; i++) {
    h = ((h << 5) + h + input.charCodeAt(i)) >>> 0;
  }
  return h.toString(16).padStart(8, '0');
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `npm run test:run -- src/apps/__tests__/archive-commands.test.ts`
Expected: PASS（全部用例）

- [ ] **Step 5: 提交点（用户操作 git）**

提示用户：`git add src/apps/ArchiveViewer/commands.ts src/apps/__tests__/archive-commands.test.ts && git commit -m "new: ArchiveViewer 命令构造层"`

---

### Task 2: 列表解析层 parsers.ts（TDD）

**Files:**
- Create: `src/apps/ArchiveViewer/parsers.ts`
- Test: `src/apps/__tests__/archive-parsers.test.ts`

- [ ] **Step 1: 在 WSL 生成真实 fixture 校准（防记忆偏差）**

Run:
```
wsl -e bash -l -c "cd /tmp && mkdir -p avfix && cd avfix && mkdir -p dir && echo hello > dir/file.txt && touch dir/empty.bin && zip -q r.zip dir && tar czf r.tar.gz dir && 7z a -slt r.7z dir >/dev/null 2>&1; unzip -l r.zip; echo ---; tar -tvf r.tar.gz; echo ---; 7z l -slt r.7z | head -30"
```
Expected: 三段真实输出。**若与下方 fixture 结构不符，以真实输出为准修正测试 fixture 与解析正则。**

- [ ] **Step 2: 写失败测试**

```ts
// src/apps/__tests__/archive-parsers.test.ts
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

describe('parseTarListing（tar -tvf 详单）', () => {
  const OUT = `-rw-r--r-- root/root      1234 2024-01-01 12:00 dir/file.txt
drwxr-xr-x root/root         0 2024-01-01 12:00 dir/
-rw-r--r-- root/root        56 2024-01-01 12:00 ./top.txt`;
  it('解析权限位/大小/路径；d 前缀或尾斜杠判目录；去 ./ 前缀', () => {
    expect(parseTarListing(OUT)).toEqual<ArchiveEntry[]>([
      { path: 'dir/file.txt', size: 1234, isDir: false },
      { path: 'dir/', size: 0, isDir: true },
      { path: 'top.txt', size: 56, isDir: false },
    ]);
  });
  it('老时间戳（无 HH:MM，超 182 天）也能解析', () => {
    expect(parseTarListing('-rw-r--r-- root/root  1234 2020-01-01 old.txt'))
      .toEqual<ArchiveEntry[]>([{ path: 'old.txt', size: 1234, isDir: false }]);
  });
  it('符号链接行 size 取 0 不报错', () => {
    const r = parseTarListing('lrwxrwxrwx root/root         0 2024-01-01 12:00 ln -> target');
    expect(r).toEqual<ArchiveEntry[]>([{ path: 'ln', size: 0, isDir: false }]);
  });
  it('空输出抛错', () => {
    expect(() => parseTarListing('')).toThrow();
  });
});

describe('parse7zListing（7z l -slt key=value）', () => {
  const OUT = `
Path = dir/file.txt
Folder = -
Size = 1234

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
----------- ---------- ------ ---------- -----  -------  ------
      1234       1234  100%  2024-01-01 12:00  -rw-r--r--  dir/file.txt
         0          0   0%  2024-01-01 12:00  drw-r--r--  dir/`;
  it('Attr 首字符 d 或尾斜杠判目录', () => {
    expect(parseUnrarListing(OUT)).toEqual<ArchiveEntry[]>([
      { path: 'dir/file.txt', size: 1234, isDir: false },
      { path: 'dir/', size: 0, isDir: true },
    ]);
  });
  it('空输出抛错', () => {
    expect(() => parseUnrarListing('')).toThrow();
  });
});
```

- [ ] **Step 3: 跑测试确认失败**

Run: `npm run test:run -- src/apps/__tests__/archive-parsers.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 4: 实现 parsers.ts**

```ts
// src/apps/ArchiveViewer/parsers.ts
/**
 * ArchiveViewer 列表输出解析层（纯函数）
 *
 * 四种工具输出格式各异，各自解析为统一 ArchiveEntry[]。
 * 设计原则：解析失败（空输出/一行都不匹配）抛错，
 * 由 UI 显示「无法解析列表输出」+ 原始 stdout 摘要，绝不静默返回空表。
 *
 * 基线格式：GNU tar、unzip 6.x、p7zip 16+（-slt）、unrar 5+。
 * （busybox tar / rar3 旧格式为已知限制，见 spec §9。）
 */

/** 统一条目（与 tree.ts 解耦的最小契约） */
export interface ArchiveEntry {
  /** 包内路径（zip/rar 目录条目带尾斜杠；tar 亦可由权限位判目录） */
  path: string;
  /** 字节；目录为 0 */
  size: number;
  isDir: boolean;
}

/** 规整路径：去 ./ 前缀、去首尾空白 */
function normalizePath(p: string): string {
  return p.trim().replace(/^\.\//, '');
}

/** 命中行为 0 时抛错（对空输入/格式不识别统一处理） */
function failIfEmpty(parsed: ArchiveEntry[], raw: string): ArchiveEntry[] {
  if (parsed.length === 0) {
    throw new Error(`无法解析列表输出（格式不识别或压缩包为空）: ${raw.slice(0, 80)}`);
  }
  return parsed;
}

/**
 * unzip -l 输出解析
 *
 * 行格式：` {Length:>9}  {YYYY-MM-DD} {HH:MM}   {Name}`
 * Header（Archive:/Length...）、分隔线、footer（N files）不匹配行正则被跳过。
 * 目录条目 Name 以 / 结尾，Length 为 0。
 */
export function parseUnzipListing(out: string): ArchiveEntry[] {
  if (!out.trim()) throw new Error('unzip -l 输出为空');
  const re = /^ *(\d+)\s+\d{4}-\d{2}-\d{2} \d{2}:\d{2}\s+(.+?)\s*$/;
  const entries: ArchiveEntry[] = [];
  for (const line of out.split('\n')) {
    const m = line.match(re);
    if (!m) continue;
    const path = normalizePath(m[2]);
    const isDir = path.endsWith('/');
    entries.push({ path, size: isDir ? 0 : Number(m[1]), isDir });
  }
  return failIfEmpty(entries, out);
}

/**
 * tar -tvf 输出解析（GNU tar；busybox 变体见 spec 边界）
 *
 * 行格式：`{权限} {owner/group} {size} {YYYY-MM-DD}[ {HH:MM}] {name}[ -> {链接目标}]`
 * - 超过 182 天的条目省略时间（GNU tar 行为），正则把时间设为可选
 * - 符号链接行（首字符 l）size 取 0，链接目标丢弃
 */
export function parseTarListing(out: string): ArchiveEntry[] {
  if (!out.trim()) throw new Error('tar -tvf 输出为空');
  const re = /^([bcdlps-][-rwxstT+]{9})\s+\S+\/\S+\s+(\d+)\s+\d{4}-\d{2}-\d{2}(?:\s+\d{2}:\d{2})?\s+(.+)$/;
  const entries: ArchiveEntry[] = [];
  for (const line of out.split('\n')) {
    const m = line.match(re);
    if (!m) continue;
    let path = normalizePath(m[3].split(' -> ')[0]);
    const isDir = m[1][0] === 'd' || path.endsWith('/');
    if (path.endsWith('/')) path = path.slice(0, -1) + '/'; // 规整后保持尾斜杠语义
    const size = m[1][0] === 'l' ? 0 : Number(m[2]);
    entries.push({ path, size, isDir });
  }
  return failIfEmpty(entries, out);
}

/**
 * 7z l -slt 输出解析（机器可读 key = value，空行分块）
 *
 * 关键 key：Path / Folder（+ 为目录）/ Size（可能缺失）
 */
export function parse7zListing(out: string): ArchiveEntry[] {
  if (!out.trim()) throw new Error('7z l -slt 输出为空');
  const entries: ArchiveEntry[] = [];
  for (const block of out.split(/\n\s*\n/)) {
    const kv = new Map<string, string>();
    for (const line of block.split('\n')) {
      const m = line.match(/^(\w+)\s*=\s*(.*)$/);
      if (m) kv.set(m[1], m[2].trim());
    }
    const path = kv.get('Path');
    if (!path) continue; // 非条目块（如 header 统计）
    const isDir = kv.get('Folder') === '+';
    entries.push({ path: normalizePath(path), size: Number(kv.get('Size') ?? 0), isDir });
  }
  return failIfEmpty(entries, out);
}

/**
 * unrar l 输出解析（unrar 5+；rar3 旧格式不支持）
 *
 * 行格式：` {Size:>11} {Packed} {Ratio} {YYYY-MM-DD} {HH:MM}  {Attr}  {Name}`
 * Attr 首字符 d 或 Name 尾斜杠 → 目录。
 */
export function parseUnrarListing(out: string): ArchiveEntry[] {
  if (!out.trim()) throw new Error('unrar l 输出为空');
  const re = /^ *(\d+)\s+\d+\s+\d+%\s+\d{4}-\d{2}-\d{2} \d{2}:\d{2}\s+([d-][-.rwxsStT]{9})\s+(.+?)\s*$/;
  const entries: ArchiveEntry[] = [];
  for (const line of out.split('\n')) {
    const m = line.match(re);
    if (!m) continue;
    const path = normalizePath(m[3]);
    const isDir = m[2][0] === 'd' || path.endsWith('/');
    entries.push({ path, size: isDir ? 0 : Number(m[1]), isDir });
  }
  return failIfEmpty(entries, out);
}
```

- [ ] **Step 5: 跑测试确认通过**

Run: `npm run test:run -- src/apps/__tests__/archive-parsers.test.ts`
Expected: PASS。若 Step 1 真实 fixture 与假设不符，同步修正测试与正则后再跑。

- [ ] **Step 6: 提交点（用户操作 git）**

提示用户：`git add src/apps/ArchiveViewer/parsers.ts src/apps/__tests__/archive-parsers.test.ts && git commit -m "new: ArchiveViewer 四格式列表解析器"`

---

### Task 3: 树构建层 tree.ts（TDD）

**Files:**
- Create: `src/apps/ArchiveViewer/tree.ts`
- Test: `src/apps/__tests__/archive-tree.test.ts`

- [ ] **Step 1: 写失败测试**

```ts
// src/apps/__tests__/archive-tree.test.ts
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `npm run test:run -- src/apps/__tests__/archive-tree.test.ts`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现 tree.ts**

```ts
// src/apps/ArchiveViewer/tree.ts
/**
 * ArchiveViewer 树构建层（纯函数）
 *
 * 平铺 ArchiveEntry[] → 嵌套 ArchiveTreeNode[]：
 * - 目录条目缺失时由子路径隐含推导（zip 常见：只列文件不列目录）
 * - 同一目录的显式行与隐含推导去重（isDir 保留、size 不覆盖）
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
    // 祖先段全部按目录创建（隐含推导）
    let level = root;
    let acc = '';
    for (let i = 0; i < parts.length; i++) {
      const isLast = i === parts.length - 1;
      const node = seg(level, parts[i], acc, isLast ? e.isDir || !isLast : true);
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
```

- [ ] **Step 4: 跑测试确认通过**

Run: `npm run test:run -- src/apps/__tests__/archive-tree.test.ts`
Expected: PASS

- [ ] **Step 5: 提交点（用户操作 git）**

提示用户：`git add src/apps/ArchiveViewer/tree.ts src/apps/__tests__/archive-tree.test.ts && git commit -m "new: ArchiveViewer 树构建"`

---

### Task 4: ArchiveViewer UI + 窗口注册 + FileManager 分支改造

**Files:**
- Create: `src/apps/ArchiveViewer/ArchiveViewer.tsx`
- Create: `src/apps/ArchiveViewer/ArchiveViewer.css`
- Modify: `src/window-system/init.ts`（末尾 `log.info` 前追加注册）
- Modify: `src/apps/FileManager.tsx:836-842`（archive 分支改为打开窗口）
- Modify: `src/apps/FileManager.tsx`（删除 extractDialog 死代码：168 行 state、957-998 行 runExtract、2354-2390 行 ExtractDialog JSX、buildExtractCommand import 若不再使用）

- [ ] **Step 1: 实现 ArchiveViewer.css**

```css
/* src/apps/ArchiveViewer/ArchiveViewer.css
 * 单栏树视图（VSCode 风格）：工具栏 + 滚动树 + 内嵌解压对话框
 */
.archive-viewer {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-1, #1e1e1e);
  color: var(--text-1, #d4d4d4);
}
.av-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-1, #333);
  font-size: 13px;
  flex-shrink: 0;
}
.av-toolbar .av-name { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.av-toolbar .av-meta { color: var(--text-3, #888); white-space: nowrap; }
.av-toolbar .av-spacer { flex: 1; }
.av-btn {
  padding: 4px 12px;
  font-size: 13px;
  border: 1px solid var(--border-1, #444);
  border-radius: 4px;
  background: transparent;
  color: inherit;
  cursor: pointer;
}
.av-btn:hover:not(:disabled) { background: rgba(255, 255, 255, 0.08); }
.av-btn:disabled { opacity: 0.4; cursor: default; }
.av-btn-primary { background: #0e639c; border-color: #0e639c; color: #fff; }
.av-btn-primary:hover:not(:disabled) { background: #1177bb; }

.av-body { flex: 1; overflow: auto; padding: 6px 4px; }
.av-loading, .av-error {
  display: flex; flex-direction: column; align-items: center; justify-content: center;
  height: 100%; gap: 10px; color: var(--text-3, #888); font-size: 13px;
}
.av-error pre {
  max-width: 80%; max-height: 40%; overflow: auto;
  background: rgba(255,255,255,0.04); padding: 8px; border-radius: 4px;
  font-size: 12px; white-space: pre-wrap;
}

.av-row {
  display: flex; align-items: center; gap: 6px;
  padding: 3px 8px; font-size: 13px; cursor: default;
  border-radius: 4px; user-select: none;
}
.av-row:hover { background: rgba(255, 255, 255, 0.06); }
.av-row.av-extracting { opacity: 0.5; }
.av-icon { width: 18px; text-align: center; flex-shrink: 0; }
.av-row .av-row-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.av-row .av-row-size { color: var(--text-3, #888); font-size: 12px; flex-shrink: 0; }
.av-row[data-dir="true"] { cursor: pointer; }

.av-dialog-backdrop {
  position: absolute; inset: 0; background: rgba(0,0,0,0.5);
  display: flex; align-items: center; justify-content: center; z-index: 10;
}
.av-dialog {
  background: var(--bg-2, #252526); border: 1px solid var(--border-1, #444);
  border-radius: 8px; padding: 16px; min-width: 320px; font-size: 13px;
  display: flex; flex-direction: column; gap: 12px;
}
.av-dialog h3 { margin: 0; font-size: 14px; }
.av-dialog .av-dialog-btns { display: flex; gap: 8px; justify-content: flex-end; }
```

- [ ] **Step 2: 实现 ArchiveViewer.tsx**

> 窗口创建/读取协议对齐现有模式：`useWindowManager()` 取 manager（[FileManager.tsx:120](e:/MyWork/gnome-remote/src/apps/FileManager.tsx)）；`invoke('remote_execute_command', {serverId, command, args, workingDirectory, timeoutSecs})` 返回 `{stdout, stderr, exitCode}`（[FileManager.tsx:974-981](e:/MyWork/gnome-remote/src/apps/FileManager.tsx)）；提取文件的打开路由复用 `detectFileFormat` + `decideOpenTarget`（决策复用），窗口创建 switch 与 FileManager handleOpen 同模式（首版接受这 ~40 行模式重复，避免重构 FileManager 核心流程）。

```tsx
// src/apps/ArchiveViewer/ArchiveViewer.tsx
/**
 * 压缩包内容浏览（独立窗口应用）
 *
 * 数据流：remote_execute_command（列表命令）→ 解析器 → 树 → 单栏树视图
 * - 双击文件：提取到 /tmp/quireld-av/<hash>/ → detectFileFormat 分类路由打开
 * - 双击目录：展开/收起
 * - 工具栏「全部解压」：内嵌两选项对话框（stem 子目录 / 压缩包所在目录）→ buildExtractCommand
 * 设计：docs/superpowers/specs/2026-09-17-archive-viewer-design.md
 */

import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { createLogger } from '../../utils/logger';
import { useWindowManager } from '../../window-system/WindowManagerContext';
import { detectFileFormat, decideOpenTarget, createDefaultRegistry } from '../../file-formats/FileOpener';
import { buildExtractCommand } from '../../file-formats/plugins/archive';
import {
  classifyArchive, buildListCommand, buildExtractOneCommand, tmpDirFor, type ArchiveKind,
} from './commands';
import {
  parseUnzipListing, parseTarListing, parse7zListing, parseUnrarListing, type ArchiveEntry,
} from './parsers';
import { buildTree, type ArchiveTreeNode } from './tree';
import './ArchiveViewer.css';

const log = createLogger('ArchiveViewer');

/** remote_execute_command 返回结构（与 FileManager 一致） */
interface ExecResult { stdout: string; stderr: string; exitCode: number; }

interface ArchiveViewerProps {
  windowId: string;
  preloadData?: { path: string; serverId: string };
}

/** 按 kind 选择解析器（解析失败抛错由调用方呈现） */
function parseListing(kind: ArchiveKind, out: string): ArchiveEntry[] {
  switch (kind) {
    case 'zip': return parseUnzipListing(out);
    case 'tar': return parseTarListing(out);
    case '7z':  return parse7zListing(out);
    case 'rar': return parseUnrarListing(out);
  }
}

/** 人类可读大小 */
function fmtSize(n: number): string {
  if (n <= 0) return '-';
  const units = ['B', 'KB', 'MB', 'GB'];
  let v = n, i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

export function ArchiveViewer({ preloadData }: ArchiveViewerProps) {
  const { manager } = useWindowManager();
  const path = preloadData?.path;
  const serverId = preloadData?.serverId;

  const [tree, setTree] = useState<ArchiveTreeNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<{ msg: string; detail?: string } | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [extracting, setExtracting] = useState<string | null>(null);
  const [extractDialog, setExtractDialog] = useState<'ask' | 'running' | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const fileName = useMemo(() => path?.split('/').pop() ?? '', [path]);
  const kind = useMemo(() => classifyArchive(fileName), [fileName]);

  // 统计：文件数 + 总大小
  const stats = useMemo(() => {
    let files = 0, total = 0;
    const walk = (nodes: ArchiveTreeNode[]) => {
      for (const n of nodes) {
        if (n.isDir) walk(n.children); else { files++; total += n.size; }
      }
    };
    walk(tree);
    return { files, total };
  }, [tree]);

  /** 加载列表：列表命令 → 解析 → 树 */
  const loadListing = useCallback(async () => {
    if (!path || !serverId || !kind) {
      setError({ msg: kind ? '未指定压缩包或未连接服务器' : `不支持的压缩格式: ${fileName}` });
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const cmd = buildListCommand(path, kind);
      const r = await invoke<ExecResult>('remote_execute_command', {
        serverId, command: cmd.command, args: cmd.args,
        workingDirectory: '/', timeoutSecs: 30,
      });
      if (r.exitCode !== 0) {
        setError({ msg: `列表命令失败（exit ${r.exitCode}）`, detail: r.stderr || r.stdout });
        return;
      }
      setTree(buildTree(parseListing(kind, r.stdout)));
      setExpanded(new Set());
    } catch (err) {
      log.error('加载压缩包列表失败:', err);
      setError({ msg: '无法解析列表输出', detail: String(err) });
    } finally {
      setLoading(false);
    }
  }, [path, serverId, kind, fileName]);

  useEffect(() => { loadListing(); }, [loadListing]);

  /** 双击文件：提取 → 分类路由打开（决策复用 FileOpener，窗口创建对齐 FileManager handleOpen） */
  const openEntry = useCallback(async (entry: ArchiveTreeNode) => {
    if (entry.isDir || !path || !serverId || !kind || extracting) return;
    setExtracting(entry.path);
    try {
      const tmpDir = tmpDirFor(serverId, path);
      await invoke('remote_mkdir', { serverId, path: tmpDir });
      const cmd = buildExtractOneCommand(path, entry.path, kind, tmpDir);
      const r = await invoke<ExecResult>('remote_execute_command', {
        serverId, command: cmd.command, args: cmd.args,
        workingDirectory: '/', timeoutSecs: 60,
      });
      if (r.exitCode !== 0) {
        setNotice(`提取失败: ${r.stderr || r.stdout || `exit ${r.exitCode}`}`);
        return;
      }
      const localPath = `${tmpDir}${tmpDir.endsWith('/') ? '' : '/'}${entry.path}`;
      // 分类路由：与 FileManager handleOpen 相同的决策与确认流
      const info = await detectFileFormat(serverId, localPath);
      const decision = decideOpenTarget(info, createDefaultRegistry());
      if (decision.needsSizeConfirm) {
        const mb = (info.size / 1024 / 1024).toFixed(1);
        if (!confirm(`文件较大（${mb} MB），加载可能需要一些时间。仍要打开吗？`)) return;
      }
      const pre = { path: localPath, serverId };
      switch (decision.kind) {
        case 'text':  manager.create('editor',        { serverId, preloadData: pre }); break;
        case 'image': manager.create('image-viewer',    { serverId, preloadData: { ...pre, mimeType: decision.mimeType } }); break;
        case 'pdf':   manager.create('pdf-viewer',     { serverId, preloadData: pre }); break;
        case 'hex':   manager.create('hex-viewer',     { serverId, preloadData: pre }); break;
        case 'archive': manager.create('archive-viewer', { serverId, preloadData: pre }); break;
        case 'browser-local':
          await invoke('remote_open_locally', { serverId, remotePath: localPath });
          break;
        case 'directory': log.warn('提取出了目录，忽略'); break;
      }
    } catch (err) {
      log.error('提取/打开失败:', err);
      setNotice(`打开失败: ${err}`);
    } finally {
      setExtracting(null);
    }
  }, [path, serverId, kind, extracting, manager]);

  /** 全部解压（对齐 FileManager runExtract 语义：先 mkdir 再白名单命令） */
  const extractAll = useCallback(async (mode: 'folder' | 'here') => {
    if (!path || !serverId || extractDialog === 'running') return;
    const parent = path.slice(0, path.lastIndexOf('/')) || '/';
    const stem = fileName.replace(/\.(zip|tar|tar\.gz|tgz|tar\.bz2|tar\.xz|7z|rar)$/i, '');
    const targetDir = mode === 'folder' ? (parent === '/' ? `/${stem}` : `${parent}/${stem}`) : parent;
    setExtractDialog('running');
    try {
      if (mode === 'folder') {
        await invoke('remote_mkdir', { serverId, path: targetDir });
      }
      const cmd = buildExtractCommand(path, targetDir, fileName);
      const r = await invoke<ExecResult>('remote_execute_command', {
        serverId, command: cmd.command, args: cmd.args,
        workingDirectory: parent, timeoutSecs: 600,
      });
      if (r.exitCode === 0) {
        setExtractDialog(null);
        setNotice(`已解压到 ${targetDir}`);
      } else {
        setNotice(`解压失败: ${r.stderr || r.stdout || `exit ${r.exitCode}`}`);
      }
    } catch (err) {
      log.error('解压失败:', err);
      setNotice(`解压失败: ${err}`);
    } finally {
      setExtractDialog(null);
    }
  }, [path, serverId, fileName, extractDialog]);

  /** 双击目录行：切换展开 */
  const toggleDir = useCallback((node: ArchiveTreeNode) => {
    setExpanded(prev => {
      const next = new Set(prev);
      if (next.has(node.path)) next.delete(node.path); else next.add(node.path);
      return next;
    });
  }, []);

  /** 递归渲染行（缩进按深度） */
  const renderRows = (nodes: ArchiveTreeNode[], depth: number) =>
    nodes.map((n) => (
      <div key={n.path}>
        <div
          className={`av-row${extracting === n.path ? ' av-extracting' : ''}`}
          data-dir={n.isDir}
          style={{ paddingLeft: 8 + depth * 16 }}
          onDoubleClick={() => (n.isDir ? toggleDir(n) : openEntry(n))}
        >
          <span className="av-icon">{n.isDir ? (expanded.has(n.path) ? '📂' : '📁') : '📄'}</span>
          <span className="av-row-name">{n.name}</span>
          <span className="av-row-size">{n.isDir ? '' : fmtSize(n.size)}</span>
        </div>
        {n.isDir && expanded.has(n.path) && renderRows(n.children, depth + 1)}
      </div>
    ));

  return (
    <div className="archive-viewer">
      <div className="av-toolbar">
        <span className="av-name">📦 {fileName}</span>
        {tree.length > 0 && (
          <span className="av-meta">{stats.files} 个文件 · 共 {fmtSize(stats.total)}</span>
        )}
        <span className="av-spacer" />
        {notice && <span className="av-meta">{notice}</span>}
        <button className="av-btn" onClick={loadListing} disabled={loading || !kind}>刷新</button>
        <button
          className="av-btn av-btn-primary"
          onClick={() => setExtractDialog('ask')}
          disabled={loading || !kind || !tree.length}
        >全部解压</button>
      </div>

      <div className="av-body">
        {loading ? (
          <div className="av-loading">读取压缩包内容…（大 tar.gz 可能需要数秒）</div>
        ) : error ? (
          <div className="av-error">
            <span>{error.msg}</span>
            {error.detail && <pre>{error.detail}</pre>}
            <button className="av-btn" onClick={loadListing}>重试</button>
          </div>
        ) : (
          renderRows(tree, 0)
        )}
      </div>

      {extractDialog && (
        <div className="av-dialog-backdrop" onClick={() => extractDialog !== 'running' && setExtractDialog(null)}>
          <div className="av-dialog" onClick={(e) => e.stopPropagation()}>
            <h3>解压 {fileName}</h3>
            {extractDialog === 'ask' ? (
              <>
                <button className="av-btn" onClick={() => extractAll('folder')}>
                  解压到独立文件夹（{path?.slice(0, path.lastIndexOf('/'))}/{fileName.replace(/\.(zip|tar|tar\.gz|tgz|tar\.bz2|tar\.xz|7z|rar)$/i, '')}/）
                </button>
                <button className="av-btn" onClick={() => extractAll('here')}>解压到当前位置</button>
                <div className="av-dialog-btns">
                  <button className="av-btn" onClick={() => setExtractDialog(null)}>取消</button>
                </div>
              </>
            ) : (
              <span>解压中…</span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 3: init.ts 注册窗口**

在 [init.ts](e:/MyWork/gnome-remote/src/window-system/init.ts) 中：顶部 import 区（第 15 行 PDFViewer import 之后）加

```ts
import { ArchiveViewer } from '../apps/ArchiveViewer/ArchiveViewer';
```

`log.info('Registered apps: ...')` 之前追加：

```ts
  // Archive Viewer - 压缩包内容浏览（文件格式路由目标，非主动入口）
  registry.register({
    id: 'archive-viewer',
    title: '压缩包查看器',
    icon: '📦',
    defaultSize: { width: 760, height: 560 },
    minSize: { width: 420, height: 320 },
    allowMultipleInstances: true,
    component: ArchiveViewer,
    showOnDesktop: false,
    showOnDock: false,
  });
```

- [ ] **Step 4: FileManager.tsx archive 分支改造（836-842 行）**

替换为：

```tsx
        case 'archive': {
          // 压缩包：打开 ArchiveViewer 内容浏览（提取单文件/全部解压入口都在其中）
          manager.create('archive-viewer', {
            serverId: activeServerId,
            preloadData: { path: fullPath, serverId: activeServerId },
          });
          break;
        }
```

- [ ] **Step 5: 删除 FileManager.tsx 中 extractDialog 死代码**

三处：
1. 第 168 行 `const [extractDialog, setExtractDialog] = useState<...>({ ... })` 整块 state（含类型声明）
2. `runExtract` 整个 useCallback（约 957-998 行）
3. `extractDialog && createPortal(...)` 整块 JSX（约 2354-2390 行）

保留判断：`buildExtractCommand` import（第 13 行）若 FileManager 内不再有其他引用则删除（ArchiveViewer 自己 import）；`.ed-btn` 样式类若仅解压对话框使用，FileManager.css 中的规则一并删除。

- [ ] **Step 6: 类型检查 + 全量测试**

Run: `npm run test:run`（在 `e:\MyWork\gnome-remote` 下）；再 `npx tsc --noEmit`
Expected: 全部测试 PASS、tsc 零错误。`manager.create('archive-viewer', ...)` 的 id 无类型约束（字符串），无需额外类型注册。

- [ ] **Step 7: 提交点（用户操作 git）**

提示用户：`git add -A src/apps/ArchiveViewer src/window-system/init.ts src/apps/FileManager.tsx src/apps/FileManager.css && git commit -m "new: 压缩包内容浏览 ArchiveViewer"`

---

### Task 5: 集成验证（WSL 真实环境）

**Files:** 无新改动（发现问题回上游 task 修复）

- [ ] **Step 1: 构建前端**

Run: `npm run build`（在 `e:\MyWork\gnome-remote` 下）
Expected: 构建成功零错误。

- [ ] **Step 2: WSL 造真实压缩包（含中文/空格/深层目录/空文件）**

```
wsl -e bash -l -c "mkdir -p /tmp/avtest/deep/deeper && cd /tmp/avtest && echo hello > 'deep/deeper/note txt.txt' && printf '' > empty.bin && echo data > '中 文件.log' && zip -q r test.zip deep empty.bin '中 文件.log' && tar czf test.tar.gz deep empty.bin '中 文件.log' && ls -la"
```

- [ ] **Step 3: 启动应用手动验证（用户配合）**

提示用户启动 tauri dev，连接服务器后到 `/tmp/avtest`：
- 双击 `test.zip` → ArchiveViewer 打开，树含 deep/、empty.bin、中 文件.log，大小正确
- 双击 `deep/deeper/note txt.txt` → 提取并打开编辑器（空格路径 OK）
- 双击 `test.tar.gz` → 列表正常（tar 路径）
- 「全部解压」→ 独立文件夹选项 → 成功提示 → 文件管理器刷新可见 test/ 目录
- 双击 `中 文件.log` → 编辑器打开（中文条目名 OK；若乱码记录为已知限制）
- 双击一个 .zip 里的 zip（造一个嵌套 zip）→ 递归打开 ArchiveViewer

- [ ] **Step 4: 遗留问题记录 + 提交点（用户操作 git）**

发现的偏差修复后，提示用户提交最终改动并 review `git diff`。

---

## Self-Review 结果

1. **Spec 覆盖**：§3 命令→Task 1；§4 解析→Task 2；§5 树→Task 3；§6 UI→Task 4；§7 分支改造→Task 4 Step 4-5；§9 边界已内嵌（--no-wildcards、timeoutSecs、/tmp 不清理）；§10 错误处理→Task 4 Step 2（error 视图/notice/重试）。无缺口。
2. **占位符扫描**：无 TBD/TODO；所有代码步骤含完整代码。
3. **类型一致性**：`ArchiveEntry {path,size,isDir}`（parsers 定义，tree/tests 消费）✓；`ShellCommand {command,args}`（commands 内部，与 FileManager ExecResult 分离无冲突）✓；`ArchiveTreeNode` tree 定义、ArchiveViewer 消费 ✓；`classifyArchive` 返回 `ArchiveKind | null`、ArchiveViewer 幂等消费 null ✓。
