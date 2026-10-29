/**
 * 统一前端日志工具
 *
 * - **开发模式**：输出所有级别（debug/info/warn/error），带颜色
 * - **生产模式**：console 仅输出 warn 和 error，静默 debug/info
 * - **落盘（测试版）**：info/warn/error 通过 Tauri 命令 `log_write` 透传到
 *   Rust tracing，与后端日志同写一份 client.log，按时间线交错，便于排查；
 *   落盘侧按（级别|模块|内容）做 1s 节流合并，防止热路径刷爆 IPC 与日志文件
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
  try {
    // Promise.resolve 包装：非 Tauri 环境（纯浏览器 dev / 测试 jsdom）下
    // invoke 返回 undefined，直接 .catch 会抛 Uncaught TypeError
    Promise.resolve(invoke("log_write", { level, module, message, data })).catch(() => {
      /* 落盘失败静默丢弃 */
    });
  } catch {
    /* 非 Tauri 环境静默 */
  }
}

/** 落盘节流窗口：窗口内同（级别|模块|内容）只落盘一条，结束时补记合并数。
 *  console 输出不受影响（开发期保持全量实时），仅约束落盘频率，
 *  防止热路径（逐帧回调、轮询）里的 info/warn/error 无上限刷 IPC 与 client.log。 */
const THROTTLE_WINDOW_MS = 1000;

interface ThrottleEntry {
  /** 窗口内累计条数（含首条） */
  count: number;
  timer: ReturnType<typeof setTimeout>;
}
const throttleMap = new Map<string, ThrottleEntry>();

function persistThrottled(
  level: 'info' | 'warn' | 'error',
  module: string,
  message: string,
  args: unknown[],
): void {
  const data = args.map(serializeArg);
  const key = `${level}\u0000${module}\u0000${message}\u0000${data.join('\u0001')}`;
  const entry = throttleMap.get(key);
  if (entry) {
    // 窗口内重复内容：静默累计，首条已落盘
    entry.count += 1;
    return;
  }
  const timer = setTimeout(() => {
    const e = throttleMap.get(key);
    throttleMap.delete(key);
    if (e && e.count > 1) {
      // 窗口结束补记一条汇总（直接走 persist，不再进节流）
      persist(level, module, `${message}（落盘节流：窗口内同内容共 ${e.count} 条，已合并）`, []);
    }
  }, THROTTLE_WINDOW_MS);
  throttleMap.set(key, { count: 1, timer });
  persist(level, module, message, data);
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
  const tag = `[${module}]`;
  // 时间戳必须每条现算：若把 prefix 固化在创建期，同一 logger 的所有日志
  // 会共享模块加载时刻，client.log 时间线失真（历史 bug，勿回退）
  const line = (message: string) => `[${timestamp()}]${tag} ${message}`;

  return {
    debug(message: string, ...args: unknown[]) {
      if (MIN_LEVEL <= LogLevel.Debug) {
        console.debug(line(message), ...args);
      }
    },
    info(message: string, ...args: unknown[]) {
      if (MIN_LEVEL <= LogLevel.Info) {
        console.info(line(message), ...args);
      }
      if (FILE_LEVEL <= LogLevel.Info) {
        persistThrottled('info', module, message, args);
      }
    },
    warn(message: string, ...args: unknown[]) {
      if (MIN_LEVEL <= LogLevel.Warn) {
        console.warn(line(message), ...args);
      }
      if (FILE_LEVEL <= LogLevel.Warn) {
        persistThrottled('warn', module, message, args);
      }
    },
    error(message: string, ...args: unknown[]) {
      if (MIN_LEVEL <= LogLevel.Error) {
        console.error(line(message), ...args);
      }
      if (FILE_LEVEL <= LogLevel.Error) {
        persistThrottled('error', module, message, args);
      }
    },
  };
}
