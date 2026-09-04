/**
 * 统一前端日志工具
 *
 * - **开发模式**：输出所有级别（debug/info/warn/error），带颜色
 * - **生产模式**：console 仅输出 warn 和 error，静默 debug/info
 * - **落盘（测试版）**：info/warn/error 通过 Tauri 命令 `log_write` 透传到
 *   Rust tracing，与后端日志同写一份 client.log，按时间线交错，便于排查
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
import { invoke } from "@tauri-apps/api/core";

export enum LogLevel {
  Debug = 0,
  Info = 1,
  Warn = 2,
  Error = 3,
}

/** 生产环境 console 最低输出级别：只输出 warn 和 error */
const MIN_LEVEL: LogLevel = import.meta.env.PROD ? LogLevel.Warn : LogLevel.Debug;

/** 落盘最低级别：info 起（debug 太噪，不落盘） */
const FILE_LEVEL: LogLevel = LogLevel.Info;

/** 单条日志附加数据的最大长度，防止巨大对象刷爆日志文件 */
const MAX_ARG_LENGTH = 2000;

/** 格式化时间戳 YYYY-MM-DD HH:MM:SS.mmm（北京时间） */
function timestamp(): string {
  const now = new Date();
  // 获取北京时间（UTC+8）
  const beijingTime = new Date(now.getTime() + 8 * 60 * 60 * 1000);
  const year = beijingTime.getUTCFullYear();
  const month = (beijingTime.getUTCMonth() + 1).toString().padStart(2, '0');
  const day = beijingTime.getUTCDate().toString().padStart(2, '0');
  const hours = beijingTime.getUTCHours().toString().padStart(2, '0');
  const minutes = beijingTime.getUTCMinutes().toString().padStart(2, '0');
  const seconds = beijingTime.getUTCSeconds().toString().padStart(2, '0');
  const ms = beijingTime.getUTCMilliseconds().toString().padStart(3, '0');
  return `${year}-${month}-${day} ${hours}:${minutes}:${seconds}.${ms}`;
}

/** 安全序列化任意参数（截断超长内容），用于落盘与跨 IPC 传输 */
function serializeArg(arg: unknown): string {
  let text: string;
  if (typeof arg === 'string') {
    text = arg;
  } else if (arg instanceof Error) {
    text = `${arg.name}: ${arg.message}`;
  } else {
    try {
      text = JSON.stringify(arg);
    } catch {
      text = String(arg);
    }
  }
  return text.length > MAX_ARG_LENGTH
    ? text.slice(0, MAX_ARG_LENGTH) + `…(截断, 原长度 ${text.length})`
    : text;
}

/**
 * 前端日志透传到 Rust 落盘（fire-and-forget）
 *
 * 失败静默：非 Tauri 环境（纯浏览器 dev）、IPC 异常都不影响 UI 运行，
 * 日志丢失可接受，绝不能因落盘失败反过来干扰业务。
 */
function persist(level: 'info' | 'warn' | 'error', module: string, message: string, args: unknown[]): void {
  const data = args.map(serializeArg);
  invoke("log_write", { level, module, message, data }).catch(() => {
    /* 落盘失败静默丢弃 */
  });
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
      if (FILE_LEVEL <= LogLevel.Info) {
        persist('info', module, message, args);
      }
    },
    warn(message: string, ...args: unknown[]) {
      if (MIN_LEVEL <= LogLevel.Warn) {
        console.warn(`${prefix} ${message}`, ...args);
      }
      if (FILE_LEVEL <= LogLevel.Warn) {
        persist('warn', module, message, args);
      }
    },
    error(message: string, ...args: unknown[]) {
      if (MIN_LEVEL <= LogLevel.Error) {
        console.error(`${prefix} ${message}`, ...args);
      }
      if (FILE_LEVEL <= LogLevel.Error) {
        persist('error', module, message, args);
      }
    },
  };
}
