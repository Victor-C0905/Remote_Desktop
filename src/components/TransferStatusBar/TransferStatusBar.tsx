/**
 * 传输状态栏组件
 *
 * 显示在文件管理器窗口底部状态栏右侧，用于展示文件传输进度
 * 收起状态：显示图标 + 数量 + 迷你进度条 + 百分比 + 展开按钮
 * 展开状态：浮层面板显示任务列表（后续任务实现）
 */

import { useState } from 'react';
import { useTransferProgress } from '../../hooks/useTransferProgress';
import { TransferPanel } from './TransferPanel';
import './TransferStatusBar.css';

// ── 内联 SVG 图标组件（Adwaita 风格）────────────────────────────

/**
 * 下载图标（向下箭头）
 * 来源：GNOME Adwaita go-down-symbolic.svg
 */
function DownloadIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
      <path d="M8 12l-4-4h2.5V4h3v4H12z" />
    </svg>
  );
}

/**
 * 展开图标（向下箭头）
 * 来源：GNOME Adwaita pan-down-symbolic.svg
 */
function ExpandIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="12" height="12" viewBox="0 0 12 12" fill="currentColor">
      <path d="M2 4l4 4 4-4z" />
    </svg>
  );
}

/**
 * 收起图标（向上箭头）
 * 来源：GNOME Adwaita pan-up-symbolic.svg
 */
function CollapseIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="12" height="12" viewBox="0 0 12 12" fill="currentColor">
      <path d="M10 8l-4-4-4 4z" />
    </svg>
  );
}

export function TransferStatusBar() {
  const { transfers, removeTask } = useTransferProgress();
  const [isExpanded, setIsExpanded] = useState(false);

  // 展开/收起切换函数
  const toggleExpand = () => {
    setIsExpanded(!isExpanded);
  };

  // 如果没有传输任务，显示占位状态
  if (transfers.length === 0) {
    return (
      <div
        className="transfer-status-bar"
        onClick={toggleExpand}
        role="button"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            toggleExpand();
          }
        }}
        aria-label="传输任务"
        aria-expanded={isExpanded}
      >
        <div className="tsb-left">
          <DownloadIcon className="tsb-icon" />
          <span className="tsb-count">无传输任务</span>
        </div>
        <button className="tsb-expand-btn">
          <ExpandIcon />
        </button>
      </div>
    );
  }

  // 按权重计算总进度：活动任务 70%，已完成任务 30%
  const calculateWeightedProgress = () => {
    const activeTasks = transfers.filter(t => t.status === 'active');
    const completedTasks = transfers.filter(t => t.status === 'completed');

    const activeProgress = activeTasks.length > 0
      ? activeTasks.reduce((sum, t) => sum + t.progress, 0) / activeTasks.length
      : 0;

    const completedProgress = completedTasks.length > 0
      ? completedTasks.reduce((sum, t) => sum + t.progress, 0) / completedTasks.length
      : 0;

    // 如果有活动任务，权重 70% 活动任务 + 30% 已完成任务
    // 如果没有活动任务，只显示已完成任务的进度
    if (activeTasks.length > 0) {
      return activeProgress * 0.7 + completedProgress * 0.3;
    } else if (completedTasks.length > 0) {
      return completedProgress;
    } else {
      // 其他情况（排队、失败等），计算平均值
      return transfers.length > 0
        ? transfers.reduce((sum, t) => sum + t.progress, 0) / transfers.length
        : 0;
    }
  };

  const totalProgress = calculateWeightedProgress();

  return (
    <div
      className="transfer-status-bar"
      onClick={() => setIsExpanded(!isExpanded)}
      role="button"
      aria-label={isExpanded ? '收起传输列表' : '展开传输列表'}
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          setIsExpanded(!isExpanded);
        }
      }}
    >
      {/* 图标 + 数量 */}
      <div className="tsb-icon-count">
        <DownloadIcon className="tsb-icon" />
        <span className="tsb-count" aria-label={`${transfers.length}个传输任务`}>
          {transfers.length}
        </span>
      </div>

      {/* 迷你进度条 */}
      <div
        className="tsb-progress-mini"
        role="progressbar"
        aria-valuenow={Math.round(totalProgress)}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="传输总进度"
      >
        <div
          className="tsb-progress-fill"
          style={{ width: `${totalProgress}%` }}
        />
      </div>

      {/* 百分比 */}
      <span className="tsb-percent">{Math.round(totalProgress)}%</span>

      {/* 展开/收起指示器 */}
      <div className="tsb-toggle-indicator">
        {isExpanded ? <CollapseIcon className="tsb-toggle-icon" /> : <ExpandIcon className="tsb-toggle-icon" />}
      </div>

      {/* 展开的任务列表面板 */}
      {isExpanded && (
        <TransferPanel
          transfers={transfers}
          onRemoveTask={removeTask}
          onClose={() => setIsExpanded(false)}
        />
      )}
    </div>
  );
}