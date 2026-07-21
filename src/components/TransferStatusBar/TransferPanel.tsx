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

interface TransferPanelProps {
  transfers: TransferTask[];
  onClose: () => void;
}

export function TransferPanel({ transfers, onClose }: TransferPanelProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  /**
   * 点击外部时自动收起面板
   */
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      // 如果点击的不是面板内部，收起面板
      if (panelRef.current && !panelRef.current.contains(event.target as Node)) {
        onClose();
      }
    };

    // 添加全局点击监听器
    document.addEventListener('mousedown', handleClickOutside);

    // 清理监听器
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [onClose]);

  /**
   * 排序任务
   * 优先级：活动 → 排队 → 已暂停 → 完成 → 失败
   */
  const sortedTransfers = [...transfers].sort((a, b) => {
    const statusOrder = {
      active: 0,
      pending: 1,
      paused: 2,
      completed: 3,
      error: 4,
    };
    return statusOrder[a.status] - statusOrder[b.status];
  });

  /**
   * 计算任务统计
   */
  const activeCount = transfers.filter((t) => t.status === 'active').length;
  const pendingCount = transfers.filter((t) => t.status === 'pending').length;
  const completedCount = transfers.filter((t) => t.status === 'completed').length;
  const errorCount = transfers.filter((t) => t.status === 'error').length;

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

  /**
   * 移除单个任务（从列表中移除）
   */
  const handleRemoveTask = async (taskId: string) => {
    // 在实际应用中，这里可能需要调用后端 API 来清除任务记录
    // 目前仅在父组件中过滤掉该任务
    console.log('[TransferPanel] 移除任务:', taskId);
  };

  return (
    <div
      ref={panelRef}
      className="transfer-panel"
      role="dialog"
      aria-label="传输任务列表"
    >
      {/* 任务列表 */}
      <div className="tp-list">
        {sortedTransfers.length === 0 ? (
          <div className="tp-empty">暂无传输任务</div>
        ) : (
          sortedTransfers.map((task) => (
            <TaskCard
              key={task.id}
              task={task}
              onRemove={handleRemoveTask}
            />
          ))
        )}
      </div>

      {/* 底部操作栏 */}
      <div className="tp-footer">
        {/* 统计信息 */}
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
          <span className="tp-stat-item">
            {completedCount > 0 && (
              <>
                <span className="tp-stat-label">完成:</span>
                <span className="tp-stat-value success">{completedCount}</span>
              </>
            )}
          </span>
          <span className="tp-stat-item">
            {errorCount > 0 && (
              <>
                <span className="tp-stat-label">失败:</span>
                <span className="tp-stat-value error">{errorCount}</span>
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