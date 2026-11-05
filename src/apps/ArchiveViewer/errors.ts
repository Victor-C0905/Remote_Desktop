/**
 * 解压命令错误 → 用户可读文案
 *
 * 命令的 stderr 属于技术细节（英文、可能很长），不直接进通知正文；
 * 这里按常见错误特征归类为可执行提示，原始输出只写日志。
 */

import i18n from '../../i18n';
import { isAppError, formatOpError, formatUnknownError } from '../../types/errors';

export interface ExecFailure {
  stdout: string;
  stderr: string;
  exitCode: number;
}

const KNOWN_TOOLS = ['unzip', 'tar', '7z', '7zz', 'unrar', 'gzip', 'bzip2', 'xz', 'zstd', 'lz4'];

/** 从 stderr 中识别缺失的工具名 */
function missingTool(raw: string): string | null {
  if (!/cannot exec|command not found|not found|no such file or directory/.test(raw)) return null;
  // "tar (child): gzip: Cannot exec: ..." → 取 Cannot exec 前紧邻的工具名，
  // 不能简单 includes(tool)，否则会先命中外层的 tar
  const matched = raw.match(/([a-z0-9][a-z0-9._-]*)\s*:\s*(?:cannot exec|command not found|not found|no such file)/i);
  if (matched && KNOWN_TOOLS.includes(matched[1])) return matched[1];
  return KNOWN_TOOLS.find((tool) => new RegExp(`(^|\\s)${tool}\\s*:`).test(raw)) ?? null;
}

/** 命令执行失败（exitCode != 0）的友好文案 */
export function describeExecFailure(result: ExecFailure, op: 'list' | 'extract'): string {
  const raw = `${result.stderr}\n${result.stdout}`.toLowerCase();
  const t = i18n.t.bind(i18n);

  // 工具缺失（含 tar 的子解压器缺失）
  if (result.exitCode === 127 || /cannot exec|command not found/.test(raw)) {
    const tool = missingTool(raw);
    return tool
      ? t('archive.errToolMissing', { tool })
      : t('archive.errExecMissingTool');
  }

  if (/permission denied|权限不足|operation not permitted/.test(raw)) {
    return t('archive.errPermission');
  }
  if (/no space left on device|disk quota exceeded/.test(raw)) {
    return t('archive.errNoSpace');
  }
  if (/no such file or directory/.test(raw) && /cannot open/.test(raw)) {
    return t('archive.errArchiveMissing');
  }
  if (/cannot chdir|chdir failed|not a directory/.test(raw)) {
    return t('archive.errTargetMissing');
  }
  if (/unsupported compression method|need pk compat|compression method not supported|unsupported method/.test(raw)) {
    return t('archive.errUnsupportedMethod');
  }
  if (/does not look like a tar archive|not a tar archive|unexpected end of file|unexpected eof|truncated|corrupt|bad archive|invalid archive|malformed/.test(raw)) {
    return t('archive.errCorrupt');
  }
  if (/file exists|already exists/.test(raw)) {
    return t('archive.errConflict');
  }

  return t(op === 'list' ? 'archive.errListFailed' : 'archive.errExecFailed', {
    code: result.exitCode,
  });
}

/** invoke 被拒绝（连接错误/Agent 错误）的友好文案 */
export function describeInvokeError(err: unknown): string {
  // Agent 超时经 agent_rejected 透传，文案是英文技术串，需要单独归类
  if (isAppError(err) && err.code === 'agent_rejected') {
    const message = err.params?.message ?? '';
    if (/timed out|timeout|超时/i.test(message)) return i18n.t('archive.errTimeout');
    if (/not allowed|permission denied|权限不足/i.test(message)) return i18n.t('archive.errPermission');
  }
  if (isAppError(err)) return formatOpError(err);
  return formatUnknownError(err);
}
