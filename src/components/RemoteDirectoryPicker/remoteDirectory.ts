/**
 * 远程目录存在性保障（统一入口）
 *
 * 安全策略：只自动创建「父目录已存在」的末级目录，避免路径打错一个字符时
 * 用 create_dir_all 生成一整条错误目录树。父目录不存在时返回 parent-missing，
 * 由调用方提示用户先定位到上级目录再创建。
 */

import { invoke } from '@tauri-apps/api/core';
import { createLogger } from '../../utils/logger';
import { normalizeRemotePath, parentRemotePath } from './path';

const log = createLogger('RemoteDirectory');

export type EnsureDirectoryResult =
  /** 目录已存在 */
  | 'exists'
  /** 父目录存在，已创建末级目录 */
  | 'created'
  /** 父目录不存在：拒绝自动创建多级路径 */
  | 'parent-missing';

/** 读取目录是否成功（失败按“不存在/不可读”处理，不抛出） */
async function canReadDirectory(serverId: string, path: string): Promise<boolean> {
  try {
    await invoke('remote_read_dir', { serverId, path });
    return true;
  } catch (err) {
    // 目录不存在属于预期情况，仅记录 debug，避免误报 warn
    log.debug('目录不可读，按不存在处理:', path, err);
    return false;
  }
}

export async function ensureRemoteDirectory(
  serverId: string,
  path: string,
): Promise<EnsureDirectoryResult> {
  const target = normalizeRemotePath(path);
  if (target === '/') return 'exists';
  if (await canReadDirectory(serverId, target)) return 'exists';

  const parent = parentRemotePath(target);
  if (!(await canReadDirectory(serverId, parent))) return 'parent-missing';

  await invoke('remote_mkdir', { serverId, path: target });
  return 'created';
}
