/**
 * useEntryActions：包内条目动作 hook
 *
 * 职责：
 * - openEntry：提取单条目到 /tmp 临时目录 → 分类路由打开对应应用
 * - extractAll：全部解压（独立文件夹 / 当前位置）
 * - notice：动作结果反馈（成功/失败消息，两动作共用）
 *
 * 不含列表加载（见 useArchiveListing）。
 */

import { useCallback, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { createLogger } from '../../../utils/logger';
import { useWindowManager } from '../../../window-system/WindowManagerContext';
import { buildExtractCommand } from '../../../file-formats/plugins/archive';
import { openRemoteFile } from '../../openRemoteFile';
import { buildExtractOneCommand, tmpDirFor, type ArchiveKind } from '../commands';
import type { ArchiveTreeNode } from '../tree';

const log = createLogger('ArchiveViewer');

/** remote_execute_command 返回结构（与 FileManager 一致） */
interface ExecResult { stdout: string; stderr: string; exitCode: number; }

export interface EntryActionsState {
  /** 正在提取的条目路径（树行降透明度） */
  extractingPath: string | null;
  /** 解压对话框：null 关闭 / 'ask' 选择中 / 'running' 执行中 */
  extractDialog: 'ask' | 'running' | null;
  notice: string | null;
  openEntry: (entry: ArchiveTreeNode) => Promise<void>;
  extractAll: (mode: 'folder' | 'here') => Promise<void>;
  setExtractDialog: (s: 'ask' | 'running' | null) => void;
}

export function useEntryActions(
  path: string | undefined,
  serverId: string | undefined,
  fileName: string,
  kind: ArchiveKind | null,
): EntryActionsState {
  const { manager } = useWindowManager();
  const [extractingPath, setExtractingPath] = useState<string | null>(null);
  const [extractDialog, setExtractDialog] = useState<'ask' | 'running' | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  /** 双击文件：提取 → 分类路由打开（决策复用 FileOpener，窗口创建对齐 FileManager handleOpen） */
  const openEntry = useCallback(async (entry: ArchiveTreeNode) => {
    if (entry.isDir || !path || !serverId || !kind || extractingPath) return;
    setExtractingPath(entry.path);
    try {
      const tmpDir = tmpDirFor(serverId, path);
      await invoke('remote_mkdir', { serverId, path: tmpDir });
      const cmd = buildExtractOneCommand(path, entry.path, kind, tmpDir);
      const r = await invoke<ExecResult>('remote_execute_command', {
        serverId, command: cmd.command, args: cmd.args,
        workingDirectory: '/', timeoutSecs: 60,
      });
      if (r.exitCode !== 0) {
        setNotice(`提取失败: ${r.stderr || r.stdout || `exit ${r.exitCode}`}`);
        return;
      }
      const localPath = `${tmpDir}${tmpDir.endsWith('/') ? '' : '/'}${entry.path}`;
      // 分类路由：与 FileManager handleOpen 共享的打开流程（探测→决策→确认→开窗）；
      // entry.size 供探测失败回退编辑器前的大文件确认
      const outcome = await openRemoteFile(serverId, localPath, manager, entry.size);
      if (outcome.kind === 'error') {
        setNotice(`打开失败: ${outcome.message}`);
      }
    } catch (err) {
      log.error('提取/打开失败:', err);
      setNotice(`打开失败: ${err}`);
    } finally {
      setExtractingPath(null);
    }
  }, [path, serverId, kind, extractingPath, manager]);

  /** 全部解压（对齐 FileManager runExtract 语义：先 mkdir 再白名单命令） */
  const extractAll = useCallback(async (mode: 'folder' | 'here') => {
    if (!path || !serverId || extractDialog === 'running') return;
    const parent = path.slice(0, path.lastIndexOf('/')) || '/';
    const stem = fileName.replace(/\.(zip|tar|tar\.gz|tgz|tar\.bz2|tar\.xz|7z|rar)$/i, '');
    const targetDir = mode === 'folder' ? (parent === '/' ? `/${stem}` : `${parent}/${stem}`) : parent;
    setExtractDialog('running');
    try {
      if (mode === 'folder') {
        await invoke('remote_mkdir', { serverId, path: targetDir });
      }
      const cmd = buildExtractCommand(path, targetDir, fileName);
      const r = await invoke<ExecResult>('remote_execute_command', {
        serverId, command: cmd.command, args: cmd.args,
        workingDirectory: parent, timeoutSecs: 600,
      });
      if (r.exitCode === 0) {
        setNotice(`已解压到 ${targetDir}`);
      } else {
        setNotice(`解压失败: ${r.stderr || r.stdout || `exit ${r.exitCode}`}`);
      }
    } catch (err) {
      log.error('解压失败:', err);
      setNotice(`解压失败: ${err}`);
    } finally {
      setExtractDialog(null);
    }
  }, [path, serverId, fileName, extractDialog]);

  return { extractingPath, extractDialog, notice, openEntry, extractAll, setExtractDialog };
}
