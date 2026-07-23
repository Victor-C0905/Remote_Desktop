/**
 * 统一前端日志工具
 *
 * - **开发模式**：输出所有级别（debug/info/warn/error），带颜色
 * - **生产模式**：仅输出 warn 和 error，静默 debug/info
 * - 所有输出包含时间戳和模块名，方便定位问题
 *
 * @example
 * ```ts
 * const log = createLogger('FileManager');
 * log.info('读取目录', path);
 * log.debug('详细数据', entries);  // 生产环境不会输出
 * log.warn('文件过大', size);
 * log.error('读取失败', error);
 * ```
 */

export enum LogLevel {
  Debug = 0,
  Info = 1,
  Warn = 2,
  Error = 3,
}

/** 生产环境最低输出级别：只输出 warn 和 error */
const MIN_LEVEL: LogLevel = import.meta.env.PROD ? LogLevel.Warn : LogLevel.Debug;

/** 格式化时间戳 HH:MM:SS.mmm */
function timestamp(): string {
  const now = new Date();
  return `${now.getHours().toString().padStart(2, '0')}:${now.getMinutes().toString().padStart(2, '0')}:${now.getSeconds().toString().padStart(2, '0')}.${now.getMilliseconds().toString().padStart(3, '0')}`;
}

export interface Logger {
  debug(message: string, ...args: unknown[]): void;
  info(message: string, ...args: unknown[]): void;
  warn(message: string, ...args: unknown[]): void;
  error(message: string, ...args: unknown[]): void;
}

/**
 * 创建一个带模块名的 Logger 实例
 *
 * @param module - 模块/组件名（如 'FileManager', 'Terminal'）
 */
export function createLogger(module: string): Logger {
  const prefix = `[${timestamp()}][${module}]`;

  return {
    debug(message: string, ...args: unknown[]) {
      if (MIN_LEVEL <= LogLevel.Debug) {
        console.debug(`${prefix} ${message}`, ...args);
      }
    },
    info(message: string, ...args: unknown[]) {
      if (MIN_LEVEL <= LogLevel.Info) {
        console.info(`${prefix} ${message}`, ...args);
      }
    },
    warn(message: string, ...args: unknown[]) {
      if (MIN_LEVEL <= LogLevel.Warn) {
        console.warn(`${prefix} ${message}`, ...args);
      }
    },
    error(message: string, ...args: unknown[]) {
      if (MIN_LEVEL <= LogLevel.Error) {
        console.error(`${prefix} ${message}`, ...args);
      }
    },
  };
}
