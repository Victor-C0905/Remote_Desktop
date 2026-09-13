/**
 * 文本格式插件
 *
 * 依赖 Agent 端 isText 启发式（BOM/NUL/控制字符比例）。
 * SVG 例外：文本格式但路由到图片应用（image 插件负责）。
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

export const textPlugin: FileFormatPlugin = {
  id: 'text',
  detect(info: RemoteFileInfo): FormatMatch | null {
    // Agent 判定非文本（含二进制 magic）→ 不匹配
    if (!info.isText) {
      return null;
    }
    // SVG 是文本，但应路由到图片查看器；
    // HTML 也是文本，但应路由到本地浏览器打开（html 插件）
    if (info.extension === 'svg' || info.extension === 'html' || info.extension === 'htm') {
      return null;
    }
    return { pluginId: 'text', category: 'text', confidence: 0.9 };
  },
};
