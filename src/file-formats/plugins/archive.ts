/**
 * 压缩包格式插件
 *
 * 识别：扩展名为主（复合扩展名从 path 判断）+ ZIP magic 兜底。
 * 解压命令由 Agent 统一构造（客户端只发送语义化）请求，避免参数注入。
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

/** ZIP 头 "PK\x03\x04" */
const ZIP_MAGIC = [0x50, 0x4b, 0x03, 0x04];

/** 复合压缩扩展名（file_info 的 extension 只取最后一段，x.tar.gz → "gz"，需从 path 判断） */
const COMPOUND_EXTS = ['.tar.gz', '.tar.bz2', '.tar.xz', '.tar.zst'];

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
 * 目标位置已有同名文件时的处理策略
 * - overwrite：覆盖（历史默认行为）
 * - skip：跳过已存在项，只解出缺失文件
 */
export type ExtractPolicy = 'overwrite' | 'skip';


