/**
 * 压缩包内容树（受控组件）
 *
 * 单栏树视图：目录可折叠（expanded 由父级管理），文件行双击触发 onOpenFile。
 */

import { fmtSize } from '../utils/format';
import type { ArchiveTreeNode } from '../tree';

interface TreeProps {
  nodes: ArchiveTreeNode[];
  depth: number;
  expanded: Set<string>;
  extractingPath: string | null;
  onToggleDir: (node: ArchiveTreeNode) => void;
  onOpenFile: (node: ArchiveTreeNode) => void;
}

/** 递归渲染节点列表（缩进按深度） */
export function ArchiveTree({ nodes, depth, expanded, extractingPath, onToggleDir, onOpenFile }: TreeProps) {
  return (
    <>
      {nodes.map((n) => (
        <div key={n.path}>
          <div
            className={`av-row${extractingPath === n.path ? ' av-extracting' : ''}`}
            data-dir={n.isDir}
            style={{ paddingLeft: 8 + depth * 16 }}
            onDoubleClick={() => (n.isDir ? onToggleDir(n) : onOpenFile(n))}
          >
            <span className="av-icon">{n.isDir ? (expanded.has(n.path) ? '📂' : '📁') : '📄'}</span>
            <span className="av-row-name">{n.name}</span>
            <span className="av-row-size">{n.isDir ? '' : fmtSize(n.size)}</span>
          </div>
          {n.isDir && expanded.has(n.path) && (
            <ArchiveTree nodes={n.children} depth={depth + 1} expanded={expanded}
              extractingPath={extractingPath} onToggleDir={onToggleDir} onOpenFile={onOpenFile} />
          )}
        </div>
      ))}
    </>
  );
}
