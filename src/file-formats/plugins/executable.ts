/**
 * 可执行脚本插件
 *
 * .sh 且含 shebang（#!）→ 双击在终端运行（类 Windows 双击运行脚本）
 * 无 shebang 的 .sh 仍是文本（text 插件兜底，编辑器打开）
 */
import type { FileFormatPlugin, FormatMatch, RemoteFileInfo } from '../types';

export const executablePlugin: FileFormatPlugin = {
  id: 'executable',
  detect(info: RemoteFileInfo): FormatMatch | null {
    if (info.extension !== 'sh') return null;
    // shebang 校验：首两字节 "#!"（0x23 0x21）
    if (info.magicBytes.length >= 2 && info.magicBytes[0] === 0x23 && info.magicBytes[1] === 0x21) {
      return { pluginId: 'executable', category: 'run-script', confidence: 0.95 };
    }
    return null; // 无 shebang → 文本插件兜底
  },
};
