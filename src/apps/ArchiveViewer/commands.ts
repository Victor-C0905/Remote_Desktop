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
