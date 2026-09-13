/**
 * HTML 格式插件
 *
 * 双击 → 下载到本地临时目录 → 系统默认浏览器打开
 * （Tauri webview 受 CSP 限制，不适合直接渲染本地 HTML；系统浏览器独立进程隔离更安全）
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

export const htmlPlugin: FileFormatPlugin = {
  id: 'html',
  detect(info: RemoteFileInfo): FormatMatch | null {
    if (info.extension === 'html' || info.extension === 'htm') {
      return { pluginId: 'html', category: 'browser-local', confidence: 0.9 };
    }
    return null;
  },
};
