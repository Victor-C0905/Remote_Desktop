/**
 * 展开的任务列表面板组件
 *
 * 显示在状态栏上方，包含任务列表和批量操作按钮
 * 任务排序：活动 → 排队 → 已暂停 → 完成 → 失败
 * 失去焦点时自动收起
 */

import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { TransferTask } from '../../hooks/useTransferProgress';
import { TaskCard } from './TaskCard';
import './TransferPanel.css';

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

interface TransferPanelProps {
  transfers: TransferTask[];
  onRemoveTask: (taskId: string) => void;
  onClose: () => void;
  /** 状态栏容器的 ref，点击状态栏时不应该收起面板（由状态栏的 onClick 处理） */
  statusbarRef?: React.RefObject<HTMLDivElement | null>;
}

export function TransferPanel({ transfers, onRemoveTask, onClose, statusbarRef }: TransferPanelProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  /**
   * 点击外部时自动收起面板
   * 使用捕获阶段的事件监听器，确保在 React 合成事件之前触发
   */
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node;

      console.log('[TransferPanel] 外部点击检测', {
        目标元素: (target as HTMLElement).tagName,
        在面板内: panelRef.current?.contains(target),
        在状态栏内: statusbarRef?.current?.contains(target),
      });

      // 如果点击的是面板内部，不收起
      if (panelRef.current && panelRef.current.contains(target)) {
        console.log('[TransferPanel] 面板内部点击，忽略');
        return;
      }

      // 如果点击的是状态栏（按钮），不收起（由状态栏的 onClick 处理）
      if (statusbarRef?.current && statusbarRef.current.contains(target)) {
        console.log('[TransferPanel] 状态栏点击，忽略');
        return;
      }

      // 点击其他地方，收起面板
      console.log('[TransferPanel] 外部区域点击，收起面板');
      onClose();
    };

    // 使用捕获阶段（第三个参数为 true），确保在 React 合成事件之前触发
    document.addEventListener('click', handleClickOutside, true);

    // 清理监听器
    return () => {
      document.removeEventListener('click', handleClickOutside, true);
    };
  }, [onClose, statusbarRef]);

  /**
   * 过滤任务：显示所有任务（包括已完成的）
   * 只有用户手动关闭或关闭文件管理器时才移除
   */
  const visibleTransfers = transfers;

  /**
   * 排序任务
   * 优先级：活动 → 排队 → 已暂停 → 已完成 → 失败 → 已取消
   */
  const sortedTransfers = [...visibleTransfers].sort((a, b) => {
    const statusOrder = {
      active: 0,
      pending: 1,
      paused: 2,
      completed: 3,
      error: 4,
      cancelled: 5,
    };
    return statusOrder[a.status] - statusOrder[b.status];
  });

  /**
   * 计算任务统计（仅统计可见任务）
   */
  const activeCount = visibleTransfers.filter((t) => t.status === 'active').length;
  const pendingCount = visibleTransfers.filter((t) => t.status === 'pending').length;

  /**
   * 批量取消所有活动任务
   */
  const handleCancelAll = async () => {
    const activeTasks = transfers.filter((t) => t.status === 'active');
    for (const task of activeTasks) {
      try {
        await invoke('cancel_transfer', { taskId: task.id });
      } catch (error) {
        console.error('[TransferPanel] 取消任务失败:', task.id, error);
      }
    }
  };

  return (
    <div
      ref={panelRef}
      className="transfer-panel"
      role="dialog"
      aria-label="传输任务列表"
      onClick={(e) => e.stopPropagation()} // 阻止事件冒泡，防止触发父元素的 onClick
    >
      {/* 任务列表 */}
      <div className="tp-list">
        {sortedTransfers.length === 0 ? (
          <div className="tp-empty" role="status" aria-live="polite">
            <div className="tp-empty-icon">
              <DownloadIcon />
            </div>
            <p className="tp-empty-text">暂无传输任务</p>
          </div>
        ) : (
          sortedTransfers.map((task) => (
            <TaskCard
              key={task.id}
              task={task}
              onRemove={onRemoveTask}
            />
          ))
        )}
      </div>

      {/* 底部操作栏 */}
      <div className="tp-footer">
        {/* 统计信息（仅显示活动和排队） */}
        <div className="tp-stats">
          <span className="tp-stat-item">
            {activeCount > 0 && (
              <>
                <span className="tp-stat-label">活动:</span>
                <span className="tp-stat-value">{activeCount}</span>
              </>
            )}
          </span>
          <span className="tp-stat-item">
            {pendingCount > 0 && (
              <>
                <span className="tp-stat-label">排队:</span>
                <span className="tp-stat-value">{pendingCount}</span>
              </>
            )}
          </span>
        </div>

        {/* 操作按钮 */}
        <div className="tp-actions">
          {activeCount > 0 && (
            <button
              className="tp-action-btn cancel"
              onClick={handleCancelAll}
              aria-label="取消所有活动任务"
            >
              全部取消
            </button>
          )}
        </div>
      </div>
    </div>
  );
}