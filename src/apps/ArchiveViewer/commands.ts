/**
 * ArchiveViewer 客户端纯函数
 *
 * 解压/列表命令已全部下沉到 Agent（语义化请求），客户端只保留：
 * - 按文件名推断压缩格式（UI 能力查找）
 * - 单条目提取的临时目录计算
 */

/** 支持的压缩格式 */
export type ArchiveKind = 'zip' | 'tar' | '7z' | 'rar';

/**
 * 从文件名推断压缩格式
 *
 * 注意：.gz/.bz2/.xz 单独出现（非 .tar.gz 等）不支持。
 * 大小写不敏感。
 */
export function classifyArchive(fileName: string): ArchiveKind | null {
  const lower = fileName.toLowerCase();
  if (lower.endsWith('.zip')) return 'zip';
  if (/\.(tar|tar\.gz|tgz|tar\.bz2|tar\.xz|tar\.zst)$/.test(lower)) return 'tar';
  if (lower.endsWith('.7z')) return '7z';
  if (lower.endsWith('.rar')) return 'rar';
  return null;
}

/**
 * 提取临时目录：/tmp/quireld-av/<stem>-<hash8>/
 *
 * 同一 (serverId, 压缩包路径) 稳定复用同一目录（重复打开不重复占空间，-o 覆盖）。
 * 不主动清理（spec §9：/tmp 重启自清，避免误删用户编辑中的副本）。
 */
export function tmpDirFor(serverId: string, archivePath: string): string {
  const base = archivePath.split('/').pop() ?? 'archive';
  const stem = base.replace(/\.(zip|tar|tar\.gz|tgz|tar\.bz2|tar\.xz|tar\.zst|7z|rar)$/i, '') || 'archive';
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
