/**
 * PDF 格式插件
 *
 * magic number: %PDF-（25 50 44 46 2D）
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

/** PDF 文件头 "%PDF-" */
const PDF_MAGIC = [0x25, 0x50, 0x44, 0x46, 0x2d];

export const pdfPlugin: FileFormatPlugin = {
  id: 'pdf',
  detect(info: RemoteFileInfo): FormatMatch | null {
    // magic 命中：高置信度
    if (info.magicBytes.length >= PDF_MAGIC.length && PDF_MAGIC.every((b, i) => info.magicBytes[i] === b)) {
      return { pluginId: 'pdf', category: 'pdf', confidence: 0.95 };
    }
    // 扩展名命中：低置信度（文件可能改错了扩展名，但 read 后 pdfjs 会报错兜底）
    if (info.extension === 'pdf') {
      return { pluginId: 'pdf', category: 'pdf', confidence: 0.6 };
    }
    return null;
  },
};
