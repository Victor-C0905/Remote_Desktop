/**
 * FileOpener 网关层
 *
 * 职责：调用 file_info 协议探测格式 → 检测层识别 → 决定打开目标（应用 ID + 大小确认）。
 * 不直接创建窗口（窗口创建属于 FileManager 的职责，保持网关层纯逻辑可测试）。
 */
import { invoke } from '@tauri-apps/api/core';
import { FileFormatRegistry } from './registry';
import { pdfPlugin } from './plugins/pdf';
import { archivePlugin } from './plugins/archive';
import { htmlPlugin } from './plugins/html';
import { executablePlugin } from './plugins/executable';
import { imagePlugin } from './plugins/image';
import { textPlugin } from './plugins/text';
import type { FormatCategory, RemoteFileInfo } from './types';

/** 大小确认阈值（字节）：超过时提示用户确认后再加载 */
export const SIZE_LIMITS = {
  /** 图片：20MB（data URI 全量加载，base64 膨胀 1.33 倍） */
  image: 20 * 1024 * 1024,
  /** PDF：30MB（pdfjs 全量加载） */
  pdf: 30 * 1024 * 1024,
  /** 文本：5MB（编辑器全量加载 + 差异计算） */
  text: 5 * 1024 * 1024,
  /** 本地打开（HTML 下载）：20MB（下载到本地临时目录） */
  'browser-local': 20 * 1024 * 1024,
} as const;

/** 打开决策结果 */
export interface OpenDecision {
  /**
   * 目标类型：
   * - 'directory'：目录（FileManager 自行导航）
   * - 'image' | 'pdf' | 'text' | 'hex'：对应应用
   * - 'archive'：压缩包（FileManager 解压流程）
   * - 'browser-local'：HTML（下载本地，系统浏览器打开）
   * - 'run-script'：脚本（终端自动执行）
   */
  kind: 'directory' | FormatCategory;
  /** 图片类的 MIME 类型（构造 data URI 用） */
  mimeType?: string;
  /** 超过阈值需要用户确认 */
  needsSizeConfirm: boolean;
  /** hex 回退时的原因说明 */
  reason?: string;
}

/** 创建默认注册表（插件顺序即优先级：magic 精度高的在前，text 兜底在最后） */
export function createDefaultRegistry(): FileFormatRegistry {
  const registry = new FileFormatRegistry();
  registry.register(pdfPlugin);        // %PDF- magic，最精确
  registry.register(archivePlugin);    // 压缩包（zip magic + 扩展名）
  registry.register(htmlPlugin);       // HTML（browser-local）
  registry.register(executablePlugin); // .sh + shebang（run-script）
  registry.register(imagePlugin);      // 图片 magic 表
  registry.register(textPlugin);       // isText 启发式兜底
  return registry;
}

/** 调用 file_info 协议探测远程文件格式 */
export async function detectFileFormat(serverId: string, path: string): Promise<RemoteFileInfo> {
  return invoke<RemoteFileInfo>('remote_file_info', { serverId, path });
}

/** 纯决策：根据探测结果决定打开目标（无副作用，便于测试） */
export function decideOpenTarget(info: RemoteFileInfo, registry: FileFormatRegistry): OpenDecision {
  // 目录：由 FileManager 导航，不属于应用路由
  if (info.isDir) {
    return { kind: 'directory', needsSizeConfirm: false };
  }

  const match = registry.detect(info);

  if (match === null) {
    // 未知格式回退：十六进制查看器（对齐原设计"HexViewer fallback"）
    return { kind: 'hex', needsSizeConfirm: false, reason: `未知格式（扩展名 .${info.extension || '无'}）` };
  }

  // archive/run-script 无大小确认（解压结果大小不可预知，脚本与大小无关）
  const limit = SIZE_LIMITS[match.category as keyof typeof SIZE_LIMITS];
  return {
    kind: match.category,
    mimeType: match.mimeType,
    needsSizeConfirm: limit !== undefined && info.size > limit,
  };
}
