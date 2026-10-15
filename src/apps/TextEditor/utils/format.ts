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
 * @param timestamp - Unix 时间戳（毫秒）
 * @returns 格式化后的时间字符串（例如：2024-01-15 14:30）
 */
export function formatTime(timestamp: number): string {
  const date = new Date(timestamp);
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
