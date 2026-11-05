/**
 * useEntryActions：包内条目动作 hook
 *
 * 职责：
 * - openEntry：提取单条目到 /tmp 临时目录 → 分类路由打开对应应用
 * - extractAll：全部解压到指定目录
 * - extractSelected：按精确条目集合解压到指定目录
 * - notice：动作结果反馈（成功/失败消息，各动作共用）
 *
 * 目标目录由调用方（提取对话框）解析：默认压缩包所在目录，可自定义。
 * 不含列表加载（见 useArchiveListing）。
 */

import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { invoke } from '@tauri-apps/api/core';
import { createLogger } from '../../../utils/logger';
import { useWindowManager } from '../../../window-system/WindowManagerContext';
import { describeExecFailure, describeInvokeError } from '../errors';
import { notifyArchiveFailure } from '../notifications';
import type { ExtractPolicy } from '../../../file-formats/plugins/archive';
import { openRemoteFile } from '../../openRemoteFile';
import { tmpDirFor, type ArchiveKind } from '../commands';
import { ensureRemoteDirectory } from '../../../components/RemoteDirectoryPicker/remoteDirectory';
import { MAX_EXTRACT_BYTES, MAX_SELECTED_ENTRIES } from '../limits';
import { isSafeExtractOneEntry, type ArchiveSafetyIssue } from '../pathSafety';
import type { ArchiveTreeNode } from '../tree';

const log = createLogger('ArchiveViewer');

/** remote_archive_extract 返回结构（Agent 固定命令输出） */
interface ExecResult { stdout: string; stderr: string; exitCode: number; }

interface EntryActionsState {
  /** 正在提取的条目路径（列表行降透明度） */
  extractingPath: string | null;
  /** 解压对话框：null 关闭 / 'ask' 选择目标 / 'running' 执行中 */
  extractDialog: 'ask' | 'running' | null;
  notice: string | null;
  openEntry: (entry: ArchiveTreeNode) => Promise<void>;
  extractAll: (targetDir: string, policy?: ExtractPolicy) => Promise<void>;
  /** 按精确条目集合提取（目录已由调用方展开为文件路径） */
  extractSelected: (
    paths: string[], totalBytes: number, targetDir: string, policy?: ExtractPolicy,
  ) => Promise<void>;
  setExtractDialog: (s: 'ask' | 'running' | null) => void;
}

/** 解压目标校验：必须是绝对路径且不能是根目录 */
function isSafeTarget(targetDir: string): boolean {
  const target = targetDir.trim();
  return target.startsWith('/') && target !== '/';
}

export function useEntryActions(
  path: string | undefined,
  serverId: string | undefined,
  fileName: string,
  kind: ArchiveKind | null,
  tool?: string,
  formatId?: string,
  canExtractOne = true,
  canExtractAll = true,
  safetyIssue: ArchiveSafetyIssue | null = null,
  quotaExceeded = false,
): EntryActionsState {
  const { manager } = useWindowManager();
  const { t } = useTranslation();
  const [extractingPath, setExtractingPath] = useState<string | null>(null);
  const [extractDialog, setExtractDialog] = useState<'ask' | 'running' | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  /** 双击文件：提取 → 分类路由打开（决策复用 FileOpener，窗口创建对齐 FileManager handleOpen） */
  const openEntry = useCallback(async (entry: ArchiveTreeNode) => {
    if (entry.isDir || !path || !serverId || !kind || extractingPath) return;
    if (!canExtractOne) {
      setNotice(t('archive.capability.partial', { tool: tool ?? '?' }));
      return;
    }
    if (!isSafeExtractOneEntry(entry.path, entry.kind)) {
      setNotice(
        entry.kind && entry.kind !== 'file'
          ? t('archive.unsafeEntryKind', { path: entry.path })
          : t('archive.unsafeEntry', { path: entry.path }),
      );
      return;
    }
    setExtractingPath(entry.path);
    try {
      const tmpDir = tmpDirFor(serverId, path);
      await invoke('remote_mkdir', { serverId, path: tmpDir });
      const r = await invoke<ExecResult>('remote_archive_extract', {
        serverId, archivePath: path, targetDir: tmpDir,
        formatId: formatId ?? kind, entries: [entry.path], policy: 'overwrite',
      });
      if (r.exitCode !== 0) {
        log.error('提取失败:', r.exitCode, r.stderr || r.stdout);
        const message = describeExecFailure(r, 'extract');
        setNotice(t('archive.extractFailed', { message }));
        notifyArchiveFailure({ kind: 'extractOne', serverId, archiveName: fileName, message });
        return;
      }
      const localPath = `${tmpDir}${tmpDir.endsWith('/') ? '' : '/'}${entry.path}`;
      // 分类路由：与 FileManager handleOpen 共享的打开流程（探测→决策→确认→开窗）；
      // entry.size 供探测失败回退编辑器前的大文件确认
      const outcome = await openRemoteFile(serverId, localPath, manager, entry.size);
      if (outcome.kind === 'error') {
        setNotice(t('archive.openFailed', { message: outcome.message }));
      }
    } catch (err) {
      log.error('提取/打开失败:', err);
      const message = describeInvokeError(err);
      setNotice(t('archive.openFailed', { message }));
      notifyArchiveFailure({ kind: 'extractOne', serverId, archiveName: fileName, message });
    } finally {
      setExtractingPath(null);
    }
  }, [path, serverId, kind, tool, formatId, canExtractOne, extractingPath, manager, t]);

  /** 全部解压：目标目录由提取对话框提供（默认压缩包所在目录） */
  const extractAll = useCallback(async (
    targetDir: string, policy: ExtractPolicy = 'overwrite',
  ) => {
    if (!path || !serverId || extractDialog === 'running') return;
    if (!canExtractAll) {
      setNotice(t('archive.capability.partial', { tool: tool ?? '?' }));
      return;
    }
    if (safetyIssue) {
      const key = safetyIssue === 'traversal'
        ? 'archive.unsafeArchive'
        : safetyIssue === 'special'
          ? 'archive.unsafeSpecial'
          : 'archive.unsafeLink';
      setNotice(t(key));
      return;
    }
    if (quotaExceeded) {
      setNotice(t('archive.quotaExceeded'));
      return;
    }
    if (!isSafeTarget(targetDir)) {
      setNotice(t('archive.unsafeTarget'));
      return;
    }
    const target = targetDir.trim();
    setExtractDialog('running');
    try {
      // 只自动创建末级目录；上级不存在时拒绝生成一整条错误路径
      const ensured = await ensureRemoteDirectory(serverId, target);
      if (ensured === 'parent-missing') {
        setNotice(t('archive.targetParentMissing'));
        return;
      }
      const r = await invoke<ExecResult>('remote_archive_extract', {
        serverId, archivePath: path, targetDir: target,
        formatId: formatId ?? kind ?? '', entries: null, policy,
      });
      if (r.exitCode === 0) {
        setNotice(t('archive.extractedTo', { dir: target }));
      } else {
        log.error('解压失败:', r.exitCode, r.stderr || r.stdout);
        const message = describeExecFailure(r, 'extract');
        setNotice(t('archive.extractAllFailed', { message }));
        notifyArchiveFailure({
          kind: 'extractAll', serverId, archiveName: fileName, message, targetDir: target,
        });
      }
    } catch (err) {
      log.error('解压失败:', err);
      const message = describeInvokeError(err);
      setNotice(t('archive.extractAllFailed', { message }));
      notifyArchiveFailure({ kind: 'extractAll', serverId, archiveName: fileName, message });
    } finally {
      setExtractDialog(null);
    }
  }, [path, serverId, fileName, tool, canExtractAll, safetyIssue, quotaExceeded, extractDialog, t]);

  /** 按精确条目集合解压：目录已在调用方展开为文件路径，目标目录由对话框提供 */
  const extractSelected = useCallback(async (
    paths: string[],
    totalBytes: number,
    targetDir: string,
    policy: ExtractPolicy = 'overwrite',
  ) => {
    if (!path || !serverId || !kind || extractDialog === 'running') return;
    if (!canExtractOne) {
      setNotice(t('archive.capability.partial', { tool: tool ?? '?' }));
      return;
    }
    if (paths.length === 0) {
      setNotice(t('archive.selectedNone'));
      return;
    }
    if (paths.length > MAX_SELECTED_ENTRIES) {
      setNotice(t('archive.selectedTooMany', { max: MAX_SELECTED_ENTRIES }));
      return;
    }
    if (totalBytes > MAX_EXTRACT_BYTES) {
      setNotice(t('archive.quotaExceeded'));
      return;
    }
    if (!paths.every((p) => isSafeExtractOneEntry(p, 'file'))) {
      setNotice(t('archive.unsafeArchive'));
      return;
    }
    // unzip/7z/unrar 无法关闭通配符语义；含通配符的条目名只能逐个提取
    if (kind !== 'tar' && paths.some((p) => /[*?\[]/.test(p))) {
      setNotice(t('archive.selectedGlobUnsupported'));
      return;
    }
    if (!isSafeTarget(targetDir)) {
      setNotice(t('archive.unsafeTarget'));
      return;
    }

    const target = targetDir.trim();
    setExtractDialog('running');
    try {
      const ensured = await ensureRemoteDirectory(serverId, target);
      if (ensured === 'parent-missing') {
        setNotice(t('archive.targetParentMissing'));
        return;
      }
      const r = await invoke<ExecResult>('remote_archive_extract', {
        serverId, archivePath: path, targetDir: target,
        formatId: formatId ?? kind, entries: paths, policy,
      });
      if (r.exitCode === 0) {
        setNotice(t('archive.selectedExtractedTo', { count: paths.length, dir: target }));
      } else {
        log.error('筛选提取失败:', r.exitCode, r.stderr || r.stdout);
        const message = describeExecFailure(r, 'extract');
        setNotice(t('archive.extractAllFailed', { message }));
        notifyArchiveFailure({
          kind: 'extractAll', serverId, archiveName: fileName, message, targetDir: target,
        });
      }
    } catch (err) {
      log.error('筛选提取失败:', err);
      const message = describeInvokeError(err);
      setNotice(t('archive.extractAllFailed', { message }));
      notifyArchiveFailure({ kind: 'extractAll', serverId, archiveName: fileName, message });
    } finally {
      setExtractDialog(null);
    }
  }, [
    path, serverId, fileName, kind, tool, formatId,
    canExtractOne, extractDialog, t,
  ]);

  return {
    extractingPath, extractDialog, notice,
    openEntry, extractAll, extractSelected, setExtractDialog,
  };
}
