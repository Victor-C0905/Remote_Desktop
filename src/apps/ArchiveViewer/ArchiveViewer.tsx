/**
 * 压缩包内容浏览（独立窗口应用）
 *
 * 组装层：工具栏 + 面包屑 + 平面列表 + 解压对话框。
 * 交互对齐 7-Zip / WinRAR：平面导航（`..` 返回上级）、强调色选中、拖拽框选、
 * 单一“解压”动作按选中范围执行，默认目标为压缩包所在目录。
 * 逻辑在 hooks（useArchiveListing 列表加载 / useEntryActions 提取与解压），
 * 纯函数在 commands/parsers/tree/conflictScan。
 */

import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AppIcon } from '../../components/AppIcon';
import { useTranslation } from 'react-i18next';
import { createLogger } from '../../utils/logger';
import type { FileEntry } from '../fileTypes';
import type { ExtractPolicy } from '../../file-formats/plugins/archive';
import { useArchiveCapabilitiesStore } from '../../stores/archiveCapabilitiesStore';
import { archiveFormatId, describeCapability, partialSupportHint } from '../../utils/archiveFormat';
import { findArchiveSafetyIssue } from './pathSafety';
import { MAX_EXTRACT_BYTES, MAX_EXTRACT_ENTRIES } from './limits';
import { classifyArchive } from './commands';
import {
  childrenAtPath, collectExtractFiles, selectedRootNodes,
} from './tree';
import type { ArchiveTreeNode } from './tree';
import { useEscCloseWindow } from '../../window-system';
import { scanConflicts, toConflictNodes } from './conflictScan';
import { useArchiveListing } from './hooks/useArchiveListing';
import { useEntryActions } from './hooks/useEntryActions';
import { ArchiveList } from './components/ArchiveList';
import { ExtractDialog } from './components/ExtractDialog';
import { ExtractConflictDialog } from './components/ExtractConflictDialog';
import { fmtSize } from './utils/format';
import './ArchiveViewer.css';

const log = createLogger('ArchiveViewer');

interface ArchiveViewerProps {
  windowId: string;
  preloadData?: { path: string; serverId: string };
}

/** remote_read_dir 响应（冲突扫描读取目录条目） */
interface ReadDirResponse { entries?: FileEntry[] }

/** 待执行的解压（冲突确认后继续）；paths=null 表示整个压缩包 */
interface PendingExtract {
  targetDir: string;
  paths: string[] | null;
  bytes: number;
}

export function ArchiveViewer({ preloadData, windowId }: ArchiveViewerProps) {
  const { t } = useTranslation();
  const path = preloadData?.path;
  const serverId = preloadData?.serverId;

  const fileName = useMemo(() => path?.split('/').pop() ?? '', [path]);
  const kind = useMemo(() => classifyArchive(fileName), [fileName]);
  const formatId = useMemo(() => archiveFormatId(fileName), [fileName]);

  // 能力探测：连接后已由 ServerManager 预取；窗口先打开时按需补取
  const serverCaps = useArchiveCapabilitiesStore((s) => (serverId ? s.byServer[serverId] : undefined));
  const fetchCapabilities = useArchiveCapabilitiesStore((s) => s.fetchCapabilities);
  useEffect(() => {
    if (serverId && !serverCaps) void fetchCapabilities(serverId);
  }, [serverId, serverCaps, fetchCapabilities]);

  const probing = !serverCaps || serverCaps.status === 'probing';
  const ready = serverCaps?.status === 'ready' && !!serverCaps.data;
  const formatCap = ready && formatId
    ? serverCaps?.data?.formats.find((f) => f.id === formatId)
    : undefined;
  // formatId 为空（未按扩展名识别）时交给列表层的"不支持格式"错误，不冒充探测失败
  const unsupported = ready && !!serverCaps?.data && !!formatId && !formatCap?.can_list;
  const canExtractOne = ready ? !!formatCap?.can_extract_one : true;
  const canExtractAll = ready ? !!formatCap?.can_extract_all : true;
  const tool = formatCap?.tool;
  const unsupportedText = formatCap && serverCaps?.data
    ? describeCapability(serverCaps.data.os, formatCap)
    : null;
  const partialHint = formatCap ? partialSupportHint(formatCap) : null;

  const { tree, loading, error, stats, refresh } = useArchiveListing(
    path, serverId, kind, tool, formatId ?? undefined, !probing && !unsupported,
  );

  // ── 导航与选择 ─────────────────────────────────────────────
  const [currentDir, setCurrentDir] = useState('');
  const [selected, setSelected] = useState<Set<string>>(new Set());
  useEffect(() => {
    setCurrentDir('');
    setSelected(new Set());
  }, [path, serverId]);

  // 列表刷新后移除已不存在的选中路径
  const allNodePaths = useMemo(() => {
    const paths = new Set<string>();
    const walk = (nodes: ArchiveTreeNode[]) => {
      for (const node of nodes) {
        paths.add(node.path);
        if (node.isDir) walk(node.children);
      }
    };
    walk(tree);
    return paths;
  }, [tree]);
  useEffect(() => {
    setSelected((prev) => {
      if (prev.size === 0) return prev;
      const next = new Set([...prev].filter((p) => allNodePaths.has(p)));
      return next.size === prev.size ? prev : next;
    });
  }, [allNodePaths]);

  const navigate = useCallback((dirPath: string) => {
    setCurrentDir(dirPath);
    setSelected(new Set());
  }, []);

  const visibleNodes = useMemo(() => childrenAtPath(tree, currentDir), [tree, currentDir]);
  const selectedRoots = useMemo(() => selectedRootNodes(tree, selected), [tree, selected]);
  const selectedFiles = useMemo(() => collectExtractFiles(selectedRoots), [selectedRoots]);
  const selectedBytes = useMemo(
    () => selectedFiles.reduce((sum, file) => sum + file.size, 0),
    [selectedFiles],
  );
  // 当前目录的子树文件（未选中任何项时，解压范围 = 当前目录）
  const currentDirFiles = useMemo(() => collectExtractFiles(visibleNodes), [visibleNodes]);
  const currentDirBytes = useMemo(
    () => currentDirFiles.reduce((sum, file) => sum + file.size, 0),
    [currentDirFiles],
  );

  const safetyIssue = useMemo(() => findArchiveSafetyIssue(tree), [tree]);
  const quotaExceeded = useMemo(
    () => stats.files > MAX_EXTRACT_ENTRIES || stats.total > MAX_EXTRACT_BYTES,
    [stats],
  );
  const {
    extractingPath, extractDialog, notice,
    openEntry, extractAll, extractSelected, setExtractDialog,
  } = useEntryActions(
    path, serverId, fileName, kind, tool, formatId ?? undefined,
    canExtractOne, canExtractAll, safetyIssue, quotaExceeded,
  );

  // ── 解压冲突：Windows 式覆盖确认（递归比对真实同名项） ──
  const [conflict, setConflict] = useState<{
    count: number;
    paths: string[];
    truncated: boolean;
    pending: PendingExtract;
  } | null>(null);
  const [checkingTarget, setCheckingTarget] = useState(false);

  // Esc 关闭窗口（BUG-VIEW-2）；解压/冲突对话框打开时让位给对话框处理
  const dialogBlocking = Boolean(extractDialog || conflict);
  useEscCloseWindow(windowId, dialogBlocking);
  const archiveConflictNodes = useMemo(() => toConflictNodes(tree), [tree]);

  /** 读取目标目录；不存在/不可读由扫描器按无冲突跳过该子树 */
  const readTargetDir = useCallback(async (dir: string) => {
    try {
      const resp = await invoke<ReadDirResponse>('remote_read_dir', { serverId, path: dir });
      return resp.entries ?? [];
    } catch (err) {
      log.debug('目标目录不可读，按无冲突处理:', dir, err);
      throw err;
    }
  }, [serverId]);

  const runExtract = useCallback((pending: PendingExtract, policy: ExtractPolicy) => {
    if (pending.paths === null) {
      void extractAll(pending.targetDir, policy);
    } else {
      void extractSelected(pending.paths, pending.bytes, pending.targetDir, policy);
    }
  }, [extractAll, extractSelected]);

  /**
   * 提取对话框确认：按范围扫描冲突，有冲突先询问覆盖/跳过。
   * 范围优先级：选中项 > 当前目录 > 整个压缩包。
   */
  const beginExtract = useCallback(async (targetDir: string) => {
    if (!path || !serverId || checkingTarget) return;
    const useSelection = selectedRoots.length > 0;
    const inSubDir = !useSelection && currentDir !== '';
    const pending: PendingExtract = useSelection
      ? {
        targetDir,
        paths: selectedFiles.map((file) => file.path),
        bytes: selectedBytes,
      }
      : inSubDir
        ? {
          targetDir,
          paths: currentDirFiles.map((file) => file.path),
          bytes: currentDirBytes,
        }
        : { targetDir, paths: null, bytes: 0 };
    const scanNodes = useSelection
      ? toConflictNodes(selectedRoots)
      : inSubDir
        ? toConflictNodes(visibleNodes)
        : archiveConflictNodes;

    setCheckingTarget(true);
    try {
      const result = await scanConflicts(targetDir, scanNodes, readTargetDir);
      if (result.count > 0 || result.truncated) {
        setExtractDialog(null);
        setConflict({
          count: result.count,
          paths: result.paths,
          truncated: result.truncated,
          pending,
        });
        return;
      }
      runExtract(pending, 'overwrite');
    } finally {
      setCheckingTarget(false);
    }
  }, [
    path, serverId, checkingTarget, currentDir, selectedRoots, selectedFiles, selectedBytes,
    currentDirFiles, currentDirBytes, visibleNodes, archiveConflictNodes,
    readTargetDir, runExtract, setExtractDialog,
  ]);

  const resolveConflict = useCallback((policy: ExtractPolicy) => {
    const current = conflict;
    setConflict(null);
    if (current) runExtract(current.pending, policy);
  }, [conflict, runExtract]);

  // ── 工具栏状态：选中项 > 当前目录 > 整个压缩包 ──────────────
  const hasSelection = selectedRoots.length > 0;
  const isSubDirScope = !hasSelection && currentDir !== '';
  const canRunSelected = hasSelection && selectedFiles.length > 0 && canExtractOne;
  const canRunCurrent = isSubDirScope && currentDirFiles.length > 0 && canExtractOne;
  const canRunAll = !hasSelection && !isSubDirScope && canExtractAll;
  const extractDisabled = loading || !kind || !tree.length || probing || unsupported
    || !(hasSelection ? canRunSelected : isSubDirScope ? canRunCurrent : canRunAll);
  const crumbs = currentDir ? currentDir.split('/').filter(Boolean) : [];

  return (
    <div className="archive-viewer">
      <div className="av-toolbar">
        <span className="av-name"><AppIcon name="package" size={15} /> {fileName}</span>
        {tree.length > 0 && (
          <span className="av-meta">{t('archive.fileCount', { count: stats.files, size: fmtSize(stats.total) })}</span>
        )}
        <span className="av-spacer" />
        {partialHint && <span className="av-meta">{partialHint}</span>}
        {safetyIssue && (
          <span className="av-meta">
            {safetyIssue === 'traversal'
              ? t('archive.unsafeArchiveShort')
              : safetyIssue === 'special'
                ? t('archive.unsafeSpecialShort')
                : t('archive.unsafeLinkShort')}
          </span>
        )}
        {quotaExceeded && <span className="av-meta">{t('archive.quotaExceededShort')}</span>}
        {notice && <span className="av-meta">{notice}</span>}
        {serverCaps?.status === 'failed' && (
          <span className="av-meta">{t('archive.capability.probeFailedShort')}</span>
        )}
        <button className="av-btn" onClick={refresh} disabled={loading || !kind || probing || unsupported}>
          {t('archive.refresh')}
        </button>
        {(unsupported || !!partialHint || serverCaps?.status === 'failed') && serverId && (
          <button className="av-btn" onClick={() => fetchCapabilities(serverId, true)} disabled={probing}>
            {t('archive.capability.recheck')}
          </button>
        )}
        <button
          className="av-btn av-btn-primary"
          onClick={() => setExtractDialog('ask')}
          disabled={extractDisabled}
        >
          {hasSelection
            ? t('archive.extractSelected', { count: selectedRoots.length })
            : isSubDirScope
              ? t('archive.extractCurrentDir')
              : t('archive.extractAll')}
        </button>
      </div>

      <div className="av-nav">
        <button className="av-crumb" onClick={() => navigate('')}>{fileName}</button>
        {crumbs.map((seg, i) => {
          const target = crumbs.slice(0, i + 1).join('/');
          return (
            <span key={target} className="av-crumb-item">
              <span className="av-crumb-sep">›</span>
              <button className="av-crumb" onClick={() => navigate(target)}>{seg}</button>
            </span>
          );
        })}
      </div>

      <div className="av-body">
        {probing ? (
          <div className="av-loading">{t('archive.capability.checking')}</div>
        ) : unsupported ? (
          <div className="av-error">
            <span>{unsupportedText ?? t('archive.capability.probeFailed', { format: formatId ?? '' })}</span>
            {serverId && (
              <button className="av-btn" onClick={() => fetchCapabilities(serverId, true)}>
                {t('archive.capability.recheck')}
              </button>
            )}
          </div>
        ) : loading ? (
          <div className="av-loading">{t('archive.loading')}</div>
        ) : error ? (
          <div className="av-error">
            <span>{error.msg}</span>
            {error.detail && <pre>{error.detail}</pre>}
            <button className="av-btn" onClick={refresh}>{t('common.retry')}</button>
          </div>
        ) : (
          <ArchiveList
            nodes={visibleNodes}
            currentPath={currentDir}
            selected={selected}
            extractingPath={extractingPath}
            onNavigate={navigate}
            onOpenFile={openEntry}
            onSelectionChange={setSelected}
          />
        )}
      </div>

      <div className="av-status">
        <span>{t('archive.itemsCount', { count: visibleNodes.length })}</span>
        {hasSelection && (
          <span>
            {t('archive.selectedSummary', {
              count: selectedRoots.length,
              files: selectedFiles.length,
              size: fmtSize(selectedBytes),
            })}
          </span>
        )}
      </div>

      {extractDialog && path && (
        <ExtractDialog
          fileName={fileName}
          path={path}
          serverId={serverId ?? ''}
          status={extractDialog}
          selectionCount={hasSelection ? selectedRoots.length : undefined}
          currentDir={isSubDirScope ? currentDir : undefined}
          onExtract={beginExtract}
          onClose={() => setExtractDialog(null)}
        />
      )}
      {conflict && (
        <ExtractConflictDialog
          count={conflict.count}
          paths={conflict.paths}
          truncated={conflict.truncated}
          onOverwrite={() => resolveConflict('overwrite')}
          onSkip={() => resolveConflict('skip')}
          onCancel={() => setConflict(null)}
        />
      )}
    </div>
  );
}
