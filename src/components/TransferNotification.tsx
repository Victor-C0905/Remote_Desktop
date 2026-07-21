import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTransferProgress } from '../hooks/useTransferProgress';
import './TransferNotification.css';

/**
 * 传输进度通知组件
 *
 * GNOME 风格的传输进度卡片，显示：
 * - 实时进度条
 * - 文件名、大小、速度、剩余时间
 * - 暂停/继续/取消控制
 */
export function TransferNotification() {
  // 使用统一的进度管理 Hook
  const { transfers, clearCompleted } = useTransferProgress();
  const [isMinimized, setIsMinimized] = useState(false);

  // 格式化文件大小
  const formatSize = (bytes: number): string => {
    if (bytes === 0) return '—';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
    return (bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1) + ' ' + units[i];
  };

  // 格式化速度
  const formatSpeed = (bps: number): string => {
    if (bps === 0) return '—';
    return formatSize(bps) + '/s';
  };

  // 格式化剩余时间
  const formatEta = (secs: number): string => {
    if (secs === 0) return '—';
    if (secs < 60) return `${secs}秒`;
    if (secs < 3600) return `${Math.floor(secs / 60)}分${secs % 60}秒`;
    return `${Math.floor(secs / 3600)}小时${Math.floor((secs % 3600) / 60)}分`;
  };

  // 关闭已完成的任务
  const handleClose = () => {
    clearCompleted();
  };

  // 取消传输（暂时不实现，等待后端支持）
  const handleCancel = async (taskId: string) => {
    try {
      await invoke('cancel_transfer', { taskId });
      // 注意：不直接操作 transfers，等待后端发送更新事件
    } catch (error) {
      console.error('[TransferNotification] 取消传输失败:', error);
    }
  };

  // 移除单个任务（暂时不实现）
  const handleRemove = (taskId: string) => {
    // 注意：暂时不实现，等待 useTransferProgress Hook 提供删除方法
    console.log('[TransferNotification] 移除任务:', taskId);
  };

  // 如果没有传输任务，不显示
  if (transfers.length === 0) return null;

  return (
    <div className={`transfer-notification ${isMinimized ? 'minimized' : ''}`}>
      {/* 标题栏 */}
      <div className="tn-header">
        <span className="tn-title">文件传输</span>
        <span className="tn-count">{transfers.length} 个任务</span>
        <button
          className="tn-minimize-btn"
          onClick={() => setIsMinimized(!isMinimized)}
        >
          {isMinimized ? '▲' : '▼'}
        </button>
        <button className="tn-close-btn" onClick={handleClose}>✕</button>
      </div>

      {/* 传输列表 */}
      {!isMinimized && (
        <div className="tn-list">
          {transfers.map(task => (
            <div key={task.id} className="tn-task">
              {/* 文件图标 + 文件名 */}
              <div className="tn-task-header">
                <span className="tn-icon">
                  {task.direction === 'upload' ? '⬆️ 上传' : '⬇️ 下载'}
                </span>
                <span className="tn-filename">{task.file_name}</span>
                <span className="tn-status">
                  {task.status === 'active' ? '传输中...' :
                   task.status === 'completed' ? '已完成' :
                   task.status === 'error' ? '失败' :
                   task.status === 'paused' ? '已暂停' : '排队中'}
                </span>
              </div>

              {/* 进度条 */}
              <div className="tn-progress-bar">
                <div
                  className="tn-progress-fill"
                  style={{ width: `${task.progress}%` }}
                />
              </div>

              {/* 传输信息 */}
              <div className="tn-info">
                <span className="tn-size">
                  {formatSize(task.transferred)} / {formatSize(task.file_size)}
                </span>
                <span className="tn-speed">{formatSpeed(task.speed)}</span>
                <span className="tn-eta">剩余 {formatEta(task.eta)}</span>
              </div>

              {/* 错误信息 */}
              {task.error && (
                <div className="tn-error">{task.error}</div>
              )}

              {/* 控制按钮 */}
              <div className="tn-actions">
                {task.status === 'active' && (
                  <button onClick={() => handleCancel(task.id)}>取消</button>
                )}
                {task.status === 'paused' && (
                  <button onClick={() => handleCancel(task.id)}>取消</button>
                )}
                {(task.status === 'completed' || task.status === 'error') && (
                  <button onClick={() => handleRemove(task.id)}>关闭</button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}