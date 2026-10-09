/**
 * 压缩包内容浏览（独立窗口应用）
 *
 * 组装层：工具栏 + 内容树 + 解压对话框。
 * 逻辑在 hooks（useArchiveListing 列表加载 / useEntryActions 提取与解压），
 * 纯函数在 commands/parsers/tree，格式化在 utils/format。
 * 设计：docs/superpowers/specs/2026-09-17-archive-viewer-design.md
 */

import { useCallback, useMemo, useState } from 'react';
import { classifyArchive } from './commands';
import { useArchiveListing } from './hooks/useArchiveListing';
import { useEntryActions } from './hooks/useEntryActions';
import { ArchiveTree } from './components/ArchiveTree';
import { ExtractDialog } from './components/ExtractDialog';
import { fmtSize } from './utils/format';
import type { ArchiveTreeNode } from './tree';
import './ArchiveViewer.css';

interface ArchiveViewerProps {
  windowId: string;
  preloadData?: { path: string; serverId: string };
}

export function ArchiveViewer({ preloadData }: ArchiveViewerProps) {
  const path = preloadData?.path;
  const serverId = preloadData?.serverId;

  const fileName = useMemo(() => path?.split('/').pop() ?? '', [path]);
  const kind = useMemo(() => classifyArchive(fileName), [fileName]);

  const { tree, loading, error, stats, refresh } = useArchiveListing(path, serverId, kind);
  const { extractingPath, extractDialog, notice, openEntry, extractAll, setExtractDialog } =
    useEntryActions(path, serverId, fileName, kind);

  // 展开/收起（不可变更新 Set）
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const toggleDir = useCallback((node: ArchiveTreeNode) => {
    setExpanded(prev => {
      const next = new Set(prev);
      if (next.has(node.path)) next.delete(node.path); else next.add(node.path);
      return next;
    });
  }, []);

  return (
    <div className="archive-viewer">
      <div className="av-toolbar">
        <span className="av-name">📦 {fileName}</span>
        {tree.length > 0 && (
          <span className="av-meta">{stats.files} 个文件 · 共 {fmtSize(stats.total)}</span>
        )}
        <span className="av-spacer" />
        {notice && <span className="av-meta">{notice}</span>}
        <button className="av-btn" onClick={refresh} disabled={loading || !kind}>刷新</button>
        <button
          className="av-btn av-btn-primary"
          onClick={() => setExtractDialog('ask')}
          disabled={loading || !kind || !tree.length}
        >全部解压</button>
      </div>

      <div className="av-body">
        {loading ? (
          <div className="av-loading">读取压缩包内容…（大 tar.gz 可能需要数秒）</div>
        ) : error ? (
          <div className="av-error">
            <span>{error.msg}</span>
            {error.detail && <pre>{error.detail}</pre>}
            <button className="av-btn" onClick={refresh}>重试</button>
          </div>
        ) : (
          <ArchiveTree nodes={tree} depth={0} expanded={expanded}
            extractingPath={extractingPath} onToggleDir={toggleDir} onOpenFile={openEntry} />
        )}
      </div>

      {extractDialog && path && (
        <ExtractDialog fileName={fileName} path={path} status={extractDialog}
          onExtract={extractAll} onClose={() => setExtractDialog(null)} />
      )}
    </div>
  );
}
