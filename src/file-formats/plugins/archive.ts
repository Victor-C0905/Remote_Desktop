/**
 * 压缩包格式插件
 *
 * 双击 → FileManager 解压流程（服务器端 unzip/tar/7z/unrar 原生命令，Worker 白名单强制）
 * 识别：扩展名为主（复合扩展名从 path 判断）+ ZIP magic 兜底
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

/** ZIP 头 "PK\x03\x04" */
const ZIP_MAGIC = [0x50, 0x4b, 0x03, 0x04];

/** 复合压缩扩展名（file_info 的 extension 只取最后一段，x.tar.gz → "gz"，需从 path 判断） */
const COMPOUND_EXTS = ['.tar.gz', '.tar.bz2', '.tar.xz'];

/** 单段压缩扩展名（extension 字段可直接判断） */
const SIMPLE_EXTS = ['zip', 'tar', '7z', 'rar', 'tgz'];

export const archivePlugin: FileFormatPlugin = {
  id: 'archive',
  detect(info: RemoteFileInfo): FormatMatch | null {
    const lowerPath = info.path.toLowerCase();
    const byExt =
      SIMPLE_EXTS.includes(info.extension) ||
      COMPOUND_EXTS.some((ext) => lowerPath.endsWith(ext));
    if (byExt) {
      return { pluginId: 'archive', category: 'archive', confidence: 0.9 };
    }
    // magic 兜底（改错扩展名的 zip）
    if (
      info.magicBytes.length >= ZIP_MAGIC.length &&
      ZIP_MAGIC.every((b, i) => info.magicBytes[i] === b)
    ) {
      return { pluginId: 'archive', category: 'archive', confidence: 0.95 };
    }
    return null;
  },
};

/**
 * 构造解压命令（仅白名单命令；Worker 端 argv 直执行，路径无需转义）
 *
 * @param archivePath 压缩包绝对路径
 * @param targetDir   解压目标目录（须已存在：tar -C 要求；unzip/7z/unrar 自动创建，先 mkdir 统一）
 * @param fileName    压缩包文件名（用于按扩展名分发）
 */
export function buildExtractCommand(
  archivePath: string,
  targetDir: string,
  fileName: string,
): { command: string; args: string[] } {
  const lower = fileName.toLowerCase();
  if (lower.endsWith('.zip')) {
    return { command: 'unzip', args: ['-o', archivePath, '-d', targetDir] };
  }
  if (/\.(tar|tar\.gz|tgz|tar\.bz2|tar\.xz)$/.test(lower)) {
    return { command: 'tar', args: ['-xf', archivePath, '-C', targetDir] };
  }
  if (lower.endsWith('.7z')) {
    return { command: '7z', args: ['x', archivePath, `-o${targetDir}`, '-y'] };
  }
  // .rar
  return { command: 'unrar', args: ['x', '-o+', archivePath, `${targetDir}/`] };
}
