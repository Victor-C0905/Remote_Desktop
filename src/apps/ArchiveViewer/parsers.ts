/**
 * ArchiveViewer 列表输出解析层（纯函数）
 *
 * 四种工具输出格式各异，各自解析为统一 ArchiveEntry[]。
 * 设计原则：解析失败（空输出/一行都不匹配）抛错，
 * 由 UI 显示「无法解析列表输出」+ 原始 stdout 摘要，绝不静默返回空表。
 *
 * 基线格式：GNU tar（WSL 实测校准）、unzip 6.x、p7zip 16+（-slt）、unrar 5+。
 * （busybox tar / rar3 旧格式为已知限制，见 spec §9。）
 */

/** 统一条目（与 tree.ts 解耦的最小契约） */
export interface ArchiveEntry {
  /** 包内路径（zip 目录条目带尾斜杠；tar 亦可由权限位判目录） */
  path: string;
  /** 字节；目录为 0 */
  size: number;
  isDir: boolean;
}

/** 规整路径：去首尾空白、去 ./ 前缀 */
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
 * tar -tvf 输出解析（GNU tar，WSL 实测校准；busybox 变体见 spec 边界）
 *
 * 行格式：`{权限} {owner/group} {size} {YYYY-MM-DD} {HH:MM} {name}[ -> {链接目标}]`
 * - 时间为可选段（老时间戳兼容不同 tar 变体）
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
 * 非条目块（如 "Listing archive:" header、统计块）无 Path key 自动跳过。
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
    if (!path) continue;
    const isDir = kv.get('Folder') === '+';
    entries.push({ path: normalizePath(path), size: Number(kv.get('Size') ?? 0), isDir });
  }
  return failIfEmpty(entries, out);
}

/**
 * unrar l 输出解析（unrar 5+；rar3 旧格式不支持）
 *
 * 行格式：` {Size} {Packed} {Ratio}% {YYYY-MM-DD} {HH:MM}  {Attr}  {Name}`
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
