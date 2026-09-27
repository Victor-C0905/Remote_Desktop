/**
 * 远程文件打开路由（应用层共享流程）
 *
 * 收敛 FileManager.handleOpen 与 ArchiveViewer.openEntry 的重复路由：
 * 探测格式 → 网关决策 → 大文件确认 → 创建对应应用窗口。
 * 探测失败回退编辑器（旧 Agent 兼容），browser-local 失败返回 error 由调用方呈现。
 *
 * 不放 file-formats 层的原因：FileOpener 是纯逻辑网关（不含窗口创建），
 * 本函数需要 manager（window-system），属应用层编排。
 */

import { invoke } from '@tauri-apps/api/core';
import { detectFileFormat, decideOpenTarget, createDefaultRegistry, SIZE_LIMITS } from '../file-formats/FileOpener';

/** 最小窗口创建接口（结构化类型，避免依赖 window-system 内部类型） */
interface WindowCreator {
  create(appId: string, opts: { serverId?: string; preloadData?: unknown }): unknown;
}

/** 打开结果（调用方据此呈现反馈） */
export type OpenOutcome =
  | { kind: 'opened' }             // 已打开（含 browser-local 已完成）
  | { kind: 'cancelled' }          // 用户取消（大文件确认拒绝）
  | { kind: 'directory' }          // 目标是目录（调用方自行处理：FileManager 导航 / ArchiveViewer 忽略）
  | { kind: 'fallback-editor' }    // 探测失败，已回退编辑器打开（旧 Agent 兼容）
  | { kind: 'error'; message: string };  // 打开动作失败（如本地打开失败）

/**
 * 探测远程文件格式并路由到对应应用窗口
 *
 * @param serverId    服务器 ID
 * @param fullPath    远程文件完整路径
 * @param manager     窗口管理器（仅用 create）
 * @param fallbackSize  被打开文件的已知大小（来自调用方列表数据）；
 *                      探测失败回退编辑器前据此做大文件确认（探测失败时拿不到服务端大小）
 */
export async function openRemoteFile(
  serverId: string,
  fullPath: string,
  manager: WindowCreator,
  fallbackSize?: number,
): Promise<OpenOutcome> {
  let info;
  try {
    info = await detectFileFormat(serverId, fullPath);
  } catch (err) {
    const msg = String(err);
    // 传输故障（连接断开/超时）：编辑器同样读不到文件，降级无意义 → 直接报错
    if (msg.startsWith('[transport]')) {
      return { kind: 'error', message: `连接不可用：${msg.replace('[transport] ', '')}` };
    }
    // Agent 端错误（旧 Agent 无 file_info / 文件级失败）→ 回退编辑器，保证可用性；
    // 回退前用调用方已知大小做大文件确认（跳过探测路径的 confirm）
    if (fallbackSize !== undefined && fallbackSize > SIZE_LIMITS.text) {
      const mb = (fallbackSize / 1024 / 1024).toFixed(1);
      if (!confirm(`文件较大（${mb} MB），加载可能需要一些时间。仍要打开吗？`)) {
        return { kind: 'cancelled' };
      }
    }
    manager.create('editor', { serverId, preloadData: { path: fullPath, serverId } });
    return { kind: 'fallback-editor' };
  }

  const decision = decideOpenTarget(info, createDefaultRegistry());

  // 大文件确认（全量 base64 加载，内存峰值约为文件大小 × 2.3）
  if (decision.needsSizeConfirm) {
    const mb = (info.size / 1024 / 1024).toFixed(1);
    if (!confirm(`文件较大（${mb} MB），加载可能需要一些时间。仍要打开吗？`)) {
      return { kind: 'cancelled' };
    }
  }

  const pre = { path: fullPath, serverId };
  switch (decision.kind) {
    case 'directory':
      // 目录条目由调用方处理（本函数只路由文件）
      return { kind: 'directory' };
    case 'text':
      manager.create('editor', { serverId, preloadData: pre });
      return { kind: 'opened' };
    case 'image':
      manager.create('image-viewer', { serverId, preloadData: { ...pre, mimeType: decision.mimeType } });
      return { kind: 'opened' };
    case 'pdf':
      manager.create('pdf-viewer', { serverId, preloadData: pre });
      return { kind: 'opened' };
    case 'hex':
      manager.create('hex-viewer', { serverId, preloadData: pre });
      return { kind: 'opened' };
    case 'archive':
      manager.create('archive-viewer', { serverId, preloadData: pre });
      return { kind: 'opened' };
    case 'browser-local':
      try {
        await invoke('remote_open_locally', { serverId, remotePath: fullPath });
        return { kind: 'opened' };
      } catch (err) {
        return { kind: 'error', message: String(err) };
      }
  }
}
