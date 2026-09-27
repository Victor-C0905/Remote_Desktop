/**
 * 展开的任务列表面板组件
 *
 * 显示在状态栏上方，包含任务列表和批量操作按钮
 * 任务按服务器分组：当前活跃服务器组默认展开，其他折叠为摘要行
 * 组内排序：活动 → 排队 → 已暂停 → 完成 → 失败 → 已取消 → 已中断
 * 失去焦点时自动收起
 */

import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { TransferTask } from '../../hooks/useTransferProgress';
import { useServersStore } from '../../stores/serversStore';
import { groupTransfersByServer } from './transferGrouping';
import { TaskCard } from './TaskCard';
import { createLogger } from '../../utils/logger';
import './TransferPanel.css';

const log = createLogger('TransferPanel');

// ── 工具函数 ──────────────────────────────────────────────────

function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${units[i]}`;
}

function formatSpeed(bytesPerSecond: number): string {
  if (bytesPerSecond === 0) return '0 B/s';
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
  const k = 1024;
  const i = Math.floor(Math.log(bytesPerSecond) / Math.log(k));
  return `${(bytesPerSecond / Math.pow(k, i)).toFixed(1)} ${units[i]}`;
}

// ── 内联 SVG 图标组件（Adwaita 风格）────────────────────────────

/**
 * 下载图标（向下箭头）
 * 来源：Adwaita go-down-symbolic.svg
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

      log.debug('外部点击检测', {
        目标元素: (target as HTMLElement).tagName,
        在面板内: panelRef.current?.contains(target),
        在状态栏内: statusbarRef?.current?.contains(target),
      });

      // 如果点击的是面板内部，不收起
      if (panelRef.current && panelRef.current.contains(target)) {
        log.debug('面板内部点击，忽略');
        return;
      }

      // 如果点击的是状态栏（按钮），不收起（由状态栏的 onClick 处理）
      if (statusbarRef?.current && statusbarRef.current.contains(target)) {
        log.debug('状态栏点击，忽略');
        return;
      }

      // 点击其他地方，收起面板
      log.debug('外部区域点击，收起面板');
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

  // 服务器分组：当前服务器的组默认展开，其他默认折叠
  const servers = useServersStore((s) => s.servers);
  const activeServerId = useServersStore((s) => s.activeServerId);
  const groups = groupTransfersByServer(visibleTransfers, servers, activeServerId);

  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set());

  // 活跃服务器变化时重置默认展开（只展开当前服务器）
  useEffect(() => {
    setExpandedGroups(activeServerId ? new Set([activeServerId]) : new Set());
  }, [activeServerId]);

  const toggleGroup = (serverId: string) => {
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(serverId)) {
        next.delete(serverId);
      } else {
        next.add(serverId);
      }
      return next;
    });
  };

  /**
   * 组内排序
   * 优先级：活动 → 排队 → 已暂停 → 已完成 → 失败 → 已取消 → 已中断
   */
  const sortTasks = (tasks: TransferTask[]) => {
    const statusOrder = {
      active: 0,
      pending: 1,
      paused: 2,
      completed: 3,
      error: 4,
      cancelled: 5,
      interrupted: 6,
    };
    return [...tasks].sort((a, b) => statusOrder[a.status] - statusOrder[b.status]);
  };

  /**
   * 计算任务统计（仅统计可见任务）
   */
  const activeCount = visibleTransfers.filter((t) => t.status === 'active').length;
  const pausedCount = visibleTransfers.filter((t) => t.status === 'paused').length;

  /**
   * 计算总传输大小和平均速度
   */
  const totalSize = visibleTransfers.reduce((sum, t) => sum + t.file_size, 0);
  const activeSpeed = visibleTransfers
    .filter((t) => t.status === 'active')
    .reduce((sum, t) => sum + t.speed, 0);

  /**
   * 批量暂停所有活动任务
   */
  const handlePauseAll = async () => {
    const activeTasks = transfers.filter((t) => t.status === 'active');
    for (const task of activeTasks) {
      try {
        await invoke('pause_transfer', { taskId: task.id });
      } catch (error) {
        log.error('暂停任务失败:', task.id, error);
      }
    }
  };

  /**
   * 批量继续所有已暂停任务
   */
  const handleResumeAll = async () => {
    const pausedTasks = transfers.filter((t) => t.status === 'paused');
    for (const task of pausedTasks) {
      try {
        await invoke('resume_transfer', { taskId: task.id });
      } catch (error) {
        log.error('继续任务失败:', task.id, error);
      }
    }
  };

  /**
   * 批量取消所有活动任务
   */
  const handleCancelAll = async () => {
    const activeTasks = transfers.filter((t) => t.status === 'active');
    for (const task of activeTasks) {
      try {
        await invoke('cancel_transfer', { taskId: task.id });
      } catch (error) {
        log.error('取消任务失败:', task.id, error);
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
      {/* 任务列表（按服务器分组，当前服务器组展开，其他折叠） */}
      <div className="tp-list">
        {groups.length === 0 ? (
          <div className="tp-empty" role="status" aria-live="polite">
            <div className="tp-empty-icon">
              <DownloadIcon />
            </div>
            <p className="tp-empty-text">暂无传输任务</p>
          </div>
        ) : (
          groups.map((group) => {
            const expanded = expandedGroups.has(group.serverId);
            return (
              <div key={group.serverId} className="tp-group">
                <button
                  className="tp-group-header"
                  onClick={() => toggleGroup(group.serverId)}
                  aria-expanded={expanded}
                  aria-label={`${group.serverName} 的传输任务（${group.summary}）`}
                >
                  <span className={`tp-group-dot${group.isCurrent ? ' active' : ''}`} />
                  <span className="tp-group-name">{group.serverName}</span>
                  {!expanded && <span className="tp-group-summary">{group.summary}</span>}
                  <span className="tp-group-toggle">
                    {expanded ? '▾' : '▸'}
                  </span>
                </button>
                {expanded && (
                  <div className="tp-group-tasks">
                    {sortTasks(group.tasks).map((task) => (
                      <TaskCard
                        key={task.id}
                        task={task}
                        onRemove={onRemoveTask}
                      />
                    ))}
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>

      {/* 底部操作栏 */}
      <div className="tp-footer">
        {/* 统计信息 */}
        <div className="tp-stats">
          {totalSize > 0 && (
            <span className="tp-stat-item">
              <span className="tp-stat-label">总大小:</span>
              <span className="tp-stat-value">{formatFileSize(totalSize)}</span>
            </span>
          )}
          {activeSpeed > 0 && (
            <span className="tp-stat-item">
              <span className="tp-stat-label">速度:</span>
              <span className="tp-stat-value">{formatSpeed(activeSpeed)}</span>
            </span>
          )}
          {activeCount > 0 && (
            <span className="tp-stat-item">
              <span className="tp-stat-label">活动:</span>
              <span className="tp-stat-value">{activeCount}</span>
            </span>
          )}
          {pausedCount > 0 && (
            <span className="tp-stat-item">
              <span className="tp-stat-label">暂停:</span>
              <span className="tp-stat-value">{pausedCount}</span>
            </span>
          )}
        </div>

        {/* 操作按钮 */}
        <div className="tp-actions">
          {activeCount > 0 && (
            <button
              className="tp-action-btn pause"
              onClick={handlePauseAll}
              aria-label="暂停所有活动任务"
            >
              全部暂停
            </button>
          )}
          {pausedCount > 0 && (
            <button
              className="tp-action-btn resume"
              onClick={handleResumeAll}
              aria-label="继续所有已暂停任务"
            >
              全部继续
            </button>
          )}
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