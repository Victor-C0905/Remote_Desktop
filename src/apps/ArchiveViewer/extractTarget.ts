/**
 * 解压目标解析（纯函数）
 *
 * 提取对话框默认目标 = 压缩包所在目录；可选追加独立子文件夹（stem/）。
 * 隐藏文件名（如 .tar.gz）剥离扩展名后为空时回退 archive，
 * 避免把父目录（甚至根目录）当成子文件夹名。
 */

/** 压缩包所在目录（根目录回退 '/'） */
export function archiveParentPath(archivePath: string): string {
  return archivePath.slice(0, archivePath.lastIndexOf('/')) || '/';
}

/** 剥离压缩扩展名后的名称；空 stem 回退 archive */
export function archiveStemName(fileName: string): string {
  return fileName.replace(/\.(zip|tar|tar\.gz|tgz|tar\.bz2|tar\.xz|tar\.zst|7z|rar)$/i, '') || 'archive';
}
