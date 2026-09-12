/**
 * 文件格式注册表（检测层核心）
 *
 * 按注册顺序遍历插件，返回首个命中结果。
 * 插件注册顺序即优先级：高置信度格式（magic number 检测）在前。
 */

import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from './types';

export class FileFormatRegistry {
  private plugins: FileFormatPlugin[] = [];

  /** 注册插件（顺序即优先级） */
  register(plugin: FileFormatPlugin): void {
    this.plugins.push(plugin);
  }

  /** 依次遍历插件检测，返回首个命中结果；全部不匹配返回 null */
  detect(info: RemoteFileInfo): FormatMatch | null {
    for (const plugin of this.plugins) {
      const match = plugin.detect(info);
      if (match !== null) {
        return match;
      }
    }
    return null;
  }
}
