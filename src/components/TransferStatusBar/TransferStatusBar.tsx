/**
 * 传输状态栏组件
 *
 * 显示在文件管理器窗口底部状态栏右侧，用于展示文件传输进度
 * 收起状态：显示图标 + 数量 + 迷你进度条 + 百分比 + 展开按钮
 * 展开状态：TransferPanel 浮层面板显示任务列表（按服务器分组，活跃组默认展开）
 */

import { useState, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useTransferProgress } from '../../hooks/useTransferProgress';
import { TransferPanel } from './TransferPanel';
import { SymbolicIcon } from '../symbolic';
import './TransferStatusBar.css';

export function TransferStatusBar() {
  const { t } = useTranslation();
  const { transfers, removeTask } = useTransferProgress();
  const [isExpanded, setIsExpanded] = useState(false);
  const statusbarRef = useRef<HTMLDivElement>(null);
  const isClickOnButton = useRef(false); // 标记是否点击了按钮

  // 展开/收起切换函数
  const toggleExpand = () => {
    setIsExpanded(!isExpanded);
  };

  // 处理状态栏的点击：设置标志位
  const handleStatusbarClick = () => {
    isClickOnButton.current = true;
    toggleExpand();
    // 在下一个事件循环重置标志位
    setTimeout(() => {
      isClickOnButton.current = false;
    }, 0);
  };

  // 如果没有传输任务，显示占位状态（与有任务时保持一致的布局）
  if (transfers.length === 0) {
    return (
      <div
        ref={statusbarRef}
        className="transfer-status-bar"
        onClick={handleStatusbarClick}
        role="button"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            toggleExpand();
          }
        }}
        aria-label={t('transfer.bar.taskAria')}
        aria-expanded={isExpanded}
      >
        {/* 图标 + 文本（保持与有任务时相同的布局） */}
        <div className="tsb-icon-count">
          <SymbolicIcon name="go-down" size={16} className="tsb-icon" />
          <span className="tsb-count" aria-live="polite">{t('transfer.bar.none')}</span>
        </div>

        {/* 空进度条（保持视觉一致性） */}
        <div className="tsb-progress-mini">
          <div className="tsb-progress-fill tsb-progress-empty" style={{ transform: 'scaleX(0)' }} />
        </div>

        {/* 占位百分比 */}
        <span className="tsb-percent tsb-percent-empty">—</span>

        {/* 展开/收起指示器 */}
        <div className="tsb-toggle-indicator">
          {isExpanded ? <SymbolicIcon name="pan-up" size={12} className="tsb-toggle-icon" /> : <SymbolicIcon name="pan-down" size={12} className="tsb-toggle-icon" />}
        </div>

        {/* 展开的空状态面板 */}
        {isExpanded && (
          <TransferPanel
            transfers={transfers}
            onRemoveTask={removeTask}
            onClose={() => setIsExpanded(false)}
            statusbarRef={statusbarRef}
          />
        )}
      </div>
    );
  }

  // 计算总进度：只统计未完成的任务（排除100%的已完成任务）
  const calculateWeightedProgress = () => {
    // 只统计未完成的任务（活动、排队、暂停、失败、已取消）
    const unfinishedTasks = transfers.filter(t => t.status !== 'completed');

    if (unfinishedTasks.length === 0) {
      // 所有任务都已完成，显示100%
      const completedTasks = transfers.filter(t => t.status === 'completed');
      return completedTasks.length > 0 ? 100 : 0;
    }

    // 计算未完成任务的平均进度
    const totalProgress = unfinishedTasks.reduce((sum, t) => sum + t.progress, 0);
    return totalProgress / unfinishedTasks.length;
  };

  const totalProgress = calculateWeightedProgress();

  return (
    <div
      ref={statusbarRef}
      className="transfer-status-bar"
      onClick={handleStatusbarClick}
      role="button"
      aria-label={isExpanded ? t('transfer.bar.collapse') : t('transfer.bar.expand')}
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          toggleExpand();
        }
      }}
    >
      {/* 图标 + 数量 */}
      <div className="tsb-icon-count">
        <SymbolicIcon name="go-down" size={16} className="tsb-icon" />
        <span className="tsb-count" aria-label={t('transfer.bar.countAria', { count: transfers.length })} aria-live="polite">
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
        aria-label={t('transfer.bar.totalProgressAria')}
      >
        <div
          className="tsb-progress-fill"
          style={{ transform: `scaleX(${totalProgress / 100})` }}
        />
      </div>

      {/* 百分比 */}
      <span className="tsb-percent" aria-live="polite">{Math.round(totalProgress)}%</span>

      {/* 展开/收起指示器 */}
      <div className="tsb-toggle-indicator">
        {isExpanded ? <SymbolicIcon name="pan-up" size={12} className="tsb-toggle-icon" /> : <SymbolicIcon name="pan-down" size={12} className="tsb-toggle-icon" />}
      </div>

      {/* 展开的任务列表面板 */}
      {isExpanded && (
        <TransferPanel
          transfers={transfers}
          onRemoveTask={removeTask}
          onClose={() => setIsExpanded(false)}
          statusbarRef={statusbarRef}
        />
      )}
    </div>
  );
}