/**
 * 图片格式插件
 *
 * magic number 检测（浏览器原生解码的所有格式）+ 扩展名回退。
 * SVG 特例：文本格式无 binary magic，仅扩展名判定（在 <img> 中渲染是静态安全的，脚本不执行）。
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

/** 图片 magic number 表 */
const MAGIC_TABLE: Array<{ prefix: number[]; mime: string }> = [
  { prefix: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], mime: 'image/png' }, // PNG
  { prefix: [0xff, 0xd8, 0xff], mime: 'image/jpeg' }, // JPEG
  { prefix: [0x47, 0x49, 0x46, 0x38], mime: 'image/gif' }, // GIF87a/GIF89a
  { prefix: [0x42, 0x4d], mime: 'image/bmp' }, // BMP
  { prefix: [0x00, 0x00, 0x01, 0x00], mime: 'image/x-icon' }, // ICO
];

/** WebP: "RIFF" + 4 字节长度 + "WEBP"（偏移 8-11） */
const RIFF = [0x52, 0x49, 0x46, 0x46];
const WEBP = [0x57, 0x45, 0x42, 0x50];

/** 扩展名 → MIME 映射（回退路径） */
const EXTENSION_TABLE: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
  svg: 'image/svg+xml',
};

/** 判断前缀是否匹配 */
function startsWith(bytes: number[], prefix: number[], offset = 0): boolean {
  if (bytes.length < offset + prefix.length) return false;
  return prefix.every((b, i) => bytes[offset + i] === b);
}

export const imagePlugin: FileFormatPlugin = {
  id: 'image',
  detect(info: RemoteFileInfo): FormatMatch | null {
    const b = info.magicBytes;

    // magic 命中：高置信度
    for (const { prefix, mime } of MAGIC_TABLE) {
      if (startsWith(b, prefix)) {
        return { pluginId: 'image', category: 'image', mimeType: mime, confidence: 0.95 };
      }
    }
    // WebP 特殊结构
    if (startsWith(b, RIFF) && startsWith(b, WEBP, 8)) {
      return { pluginId: 'image', category: 'image', mimeType: 'image/webp', confidence: 0.95 };
    }

    // 扩展名回退：低置信度
    const mime = EXTENSION_TABLE[info.extension];
    if (mime) {
      return { pluginId: 'image', category: 'image', mimeType: mime, confidence: 0.6 };
    }
    return null;
  },
};
