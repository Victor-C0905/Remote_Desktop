/**
 * 格式化工具（编辑器内共享）
 */

/**
 * 格式化文件大小
 *
 * @param bytes - 字节数
 * @returns 格式化后的文件大小字符串（例如：1.5 KB）
 */
export function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B';

  const units = ['B', 'KB', 'MB', 'GB'];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));

  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${units[i]}`;
}

/**
 * 格式化修改时间
 *
 * @param timestamp - Unix 时间戳。协议侧（Agent/FS）返回秒级时间戳，编辑器
 *   内部统一存秒（保存冲突校验的 baseMtime 也用秒）；历史调用方可能传毫秒。
 *   以 1e12 为界判别：毫秒时间戳自 2001-09 起即 > 1e12，秒级到 33658 年才越界。
 * @returns 格式化后的时间字符串（例如：2024-01-15 14:30）
 */
export function formatTime(timestamp: number): string {
  const ms = timestamp > 0 && timestamp < 1e12 ? timestamp * 1000 : timestamp;
  const date = new Date(ms);
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  const hours = String(date.getHours()).padStart(2, '0');
  const minutes = String(date.getMinutes()).padStart(2, '0');

  return `${year}-${month}-${day} ${hours}:${minutes}`;
}

/**
 * 检测换行符类型（LF / CRLF）
 *
 * @param content - 文本内容
 * @returns 换行符标识
 */
export function detectLineEnding(content: string): 'LF' | 'CRLF' {
  return content.includes('\r\n') ? 'CRLF' : 'LF';
}
