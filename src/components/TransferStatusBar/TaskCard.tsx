/**
 * 单个任务卡片组件
 *
 * 显示单个传输任务的详细信息，包括文件名、进度条和控制按钮
 * 悬停时显示控制按钮（淡入动画 200ms）
 */

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { TransferTask } from '../../hooks/useTransferProgress';
import './TaskCard.css';

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
 * 上传图标（向上箭头）
 * 来源：GNOME Adwaita go-up-symbolic.svg
 */
function UploadIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
      <path d="M8 4l4 4H9.5v4h-3V8H4z" />
    </svg>
  );
}

/**
 * 暂停图标
 * 来源：GNOME Adwaita media-playback-pause-symbolic.svg
 */
function PauseIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="12" height="12" viewBox="0 0 12 12" fill="currentColor">
      <rect x="2" y="2" width="3" height="8" />
      <rect x="7" y="2" width="3" height="8" />
    </svg>
  );
}

/**
 * 继续图标（播放）
 * 来源：GNOME Adwaita media-playback-start-symbolic.svg
 */
function PlayIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="12" height="12" viewBox="0 0 12 12" fill="currentColor">
      <path d="M3 2v8l7-4z" />
    </svg>
  );
}

/**
 * 关闭图标
 * 来源：GNOME Adwaita window-close-symbolic.svg
 */
function CloseIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="12" height="12" viewBox="0 0 12 12" fill="currentColor">
      <path d="M2.5 2L6 5.5 9.5 2l1 1L7 6l3.5 3.5-1 1L6 7l-3.5 3.5-1-1L5 6 1.5 2.5z" />
    </svg>
  );
}

/**
 * 打开文件图标
 * 来源：GNOME Adwaita folder-open-symbolic.svg
 */
function FolderOpenIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="12" height="12" viewBox="0 0 12 12" fill="currentColor">
      <path d="M1 2h4l1 1h4v1H2v5l1-3h8l-1.5 4H1z" />
    </svg>
  );
}

/**
 * 重试图标
 * 来源：GNOME Adwaita view-refresh-symbolic.svg
 */
function RefreshIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="12" height="12" viewBox="0 0 12 12" fill="currentColor">
      <path d="M8.5 1v2.5l1.5-1.5c-.8-.8-1.9-1.2-3-1-2.1.2-3.7 2-3.5 4.1s2 3.7 4.1 3.5c1.5-.1 2.7-1.1 3.2-2.5h1.1c-.6 2.2-2.6 3.8-4.9 3.5-2.6-.3-4.5-2.7-4.2-5.3S5.4.5 8 1c.5.1 1 .3 1.5.6L11 .5V1h-2.5z" />
    </svg>
  );
}

// ── TaskCard 组件 ─────────────────────────────────────────────

interface TaskCardProps {
  task: TransferTask;  // 从 useTransferProgress Hook 获取
  onRemove: (taskId: string) => void;  // 移除任务回调
}

export function TaskCard({ task, onRemove }: TaskCardProps) {
  const [isHovered, setIsHovered] = useState(false);

  /**
   * 取消传输
   */
  const handleCancel = async (e: React.MouseEvent) => {
    e.stopPropagation();  // 阻止事件冒泡，避免触发父元素的点击事件
    try {
      await invoke('cancel_transfer', { taskId: task.id });
      console.log('[TaskCard] 已取消传输:', task.id);
    } catch (error) {
      console.error('[TaskCard] 取消传输失败:', error);
    }
  };

  /**
   * 暂停传输
   */
  const handlePause = async (e: React.MouseEvent) => {
    e.stopPropagation();  // 阻止事件冒泡
    try {
      await invoke('pause_transfer', { taskId: task.id });
      console.log('[TaskCard] 已暂停传输:', task.id);
    } catch (error) {
      console.error('[TaskCard] 暂停传输失败:', error);
    }
  };

  /**
   * 继续传输
   */
  const handleResume = async (e: React.MouseEvent) => {
    e.stopPropagation();  // 阻止事件冒泡
    try {
      await invoke('resume_transfer', { taskId: task.id });
      console.log('[TaskCard] 已继续传输:', task.id);
    } catch (error) {
      console.error('[TaskCard] 继续传输失败:', error);
    }
  };

  /**
   * 重试传输
   */
  const handleRetry = async (e: React.MouseEvent) => {
    e.stopPropagation();  // 阻止事件冒泡
    try {
      await invoke('retry_transfer', { taskId: task.id });
      console.log('[TaskCard] 已重试传输:', task.id);
    } catch (error) {
      console.error('[TaskCard] 重试传输失败:', error);
    }
  };

  /**
   * 打开文件（本地路径）
   * TODO: 实现打开文件功能
   */
  const handleOpenFile = async (e: React.MouseEvent) => {
    e.stopPropagation();  // 阻止事件冒泡
    try {
      // 假设本地文件路径可以通过某种方式获取
      // await invoke('open_file', { path: localPath });
      console.log('[TaskCard] 打开文件:', task.file_name);
    } catch (error) {
      console.error('[TaskCard] 打开文件失败:', error);
    }
  };

  /**
   * 关闭任务（先取消传输，再移除）
   */
  const handleClose = async (e: React.MouseEvent) => {
    e.stopPropagation();  // 阻止事件冒泡

    try {
      // 如果任务正在进行，先取消传输
      if (task.status === 'active' || task.status === 'paused') {
        await invoke('cancel_transfer', { taskId: task.id });
        console.log('[TaskCard] 已取消传输:', task.id);
      }

      // 从列表中移除任务
      onRemove(task.id);
    } catch (error) {
      console.error('[TaskCard] 关闭任务失败:', error);
      // 即使取消失败，也尝试移除任务
      onRemove(task.id);
    }
  };

  /**
   * 获取状态对应的进度条样式类名
   */
  const getProgressClass = () => {
    if (task.status === 'error') return 'tc-progress-fill error';
    if (task.status === 'cancelled') return 'tc-progress-fill cancelled';
    return 'tc-progress-fill';
  };

  return (
    <div
      className={`task-card ${task.status}`}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      role="listitem"
      aria-label={`${task.direction === 'upload' ? '上传' : '下载'} ${task.file_name}`}
    >
      {/* 左侧：图标 + 文件名 */}
      <div className="tc-left">
        <span className="tc-icon">
          {task.direction === 'upload' ? <UploadIcon /> : <DownloadIcon />}
        </span>
        <span className="tc-filename" title={task.file_name}>
          {task.file_name}
        </span>
      </div>

      {/* 中间：进度条 + 百分比 */}
      <div className="tc-middle">
        <div
          className="tc-progress-bar"
          role="progressbar"
          aria-valuenow={Math.round(task.progress)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label={`${task.file_name} 进度`}
        >
          <div
            className={getProgressClass()}
            style={{ width: `${task.progress}%` }}
          />
        </div>
        <span className="tc-percent">{Math.round(task.progress)}%</span>
      </div>

      {/* 右侧：控制按钮（悬停显示） */}
      <div className={`tc-actions ${isHovered ? 'visible' : ''}`}>
        {/* 活动任务：暂停 + 取消 */}
        {task.status === 'active' && (
          <>
            <button
              className="tc-action-btn"
              onClick={handlePause}
              aria-label="暂停传输"
              title="暂停"
            >
              <PauseIcon />
            </button>
            <button
              className="tc-action-btn"
              onClick={handleCancel}
              aria-label="取消传输"
              title="取消"
            >
              <CloseIcon />
            </button>
          </>
        )}

        {/* 已暂停任务：继续 + 取消 */}
        {task.status === 'paused' && (
          <>
            <button
              className="tc-action-btn"
              onClick={handleResume}
              aria-label="继续传输"
              title="继续"
            >
              <PlayIcon />
            </button>
            <button
              className="tc-action-btn"
              onClick={handleCancel}
              aria-label="取消传输"
              title="取消"
            >
              <CloseIcon />
            </button>
          </>
        )}

        {/* 已完成任务：打开文件 + 关闭 */}
        {task.status === 'completed' && (
          <>
            <button
              className="tc-action-btn"
              onClick={handleOpenFile}
              aria-label="打开文件"
              title="打开文件"
            >
              <FolderOpenIcon />
            </button>
            <button
              className="tc-action-btn"
              onClick={handleClose}
              aria-label="关闭"
              title="关闭"
            >
              <CloseIcon />
            </button>
          </>
        )}

        {/* 失败任务：重试 + 关闭 */}
        {task.status === 'error' && (
          <>
            <button
              className="tc-action-btn"
              onClick={handleRetry}
              aria-label="重试传输"
              title="重试"
            >
              <RefreshIcon />
            </button>
            <button
              className="tc-action-btn"
              onClick={handleClose}
              aria-label="关闭"
              title="关闭"
            >
              <CloseIcon />
            </button>
          </>
        )}

        {/* 已取消任务：重新开始 + 关闭 */}
        {task.status === 'cancelled' && (
          <>
            <button
              className="tc-action-btn"
              onClick={handleRetry}
              aria-label="重新开始"
              title="重新开始"
            >
              <RefreshIcon />
            </button>
            <button
              className="tc-action-btn"
              onClick={handleClose}
              aria-label="关闭"
              title="关闭"
            >
              <CloseIcon />
            </button>
          </>
        )}

        {/* 排队中任务：取消 */}
        {task.status === 'pending' && (
          <button
            className="tc-action-btn"
            onClick={handleCancel}
            aria-label="取消传输"
            title="取消"
          >
            <CloseIcon />
          </button>
        )}
      </div>
    </div>
  );
}