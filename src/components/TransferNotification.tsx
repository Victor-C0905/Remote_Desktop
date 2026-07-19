import { useState, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import './TransferNotification.css';

/**
 * 传输任务状态
 */
interface TransferTask {
  id: string;
  session_id: string;
  direction: 'upload' | 'download';
  file_name: string;
  remote_path: string;
  local_path?: string;
  file_size: number;
  transferred: number;
  speed: number;      // 字节/秒
  eta: number;        // 秒
  status: 'pending' | 'active' | 'paused' | 'completed' | 'error';
  error?: string;
  progress: number;   // 0-100
}

/**
 * 传输进度事件 payload
 */
interface TransferProgressPayload {
  task_id: string;
  session_id: string;
  direction: string;
  file_name: string;
  remote_path: string;
  file_size: number;
  transferred: number;
  progress: number;
  speed_bps: number;
  eta_secs: number;
  status: string;
  error?: string;
}

/**
 * 传输进度通知组件
 *
 * GNOME 风格的传输进度卡片，显示：
 * - 实时进度条
 * - 文件名、大小、速度、剩余时间
 * - 暂停/继续/取消控制
 */
export function TransferNotification() {
  const [transfers, setTransfers] = useState<TransferTask[]>([]);
  const [isMinimized, setIsMinimized] = useState(false);

  // 监听传输进度事件
  useEffect(() => {
    const unlisten = listen<TransferProgressPayload>('transfer-progress', (event) => {
      const payload = event.payload;

      setTransfers(prev => {
        const existing = prev.find(t => t.id === payload.task_id);

        if (existing) {
          // 更新现有任务
          return prev.map(t =>
            t.id === payload.task_id
              ? {
                  ...t,
                  transferred: payload.transferred,
                  progress: payload.progress,
                  speed: payload.speed_bps,
                  eta: payload.eta_secs,
                  status: payload.status as TransferTask['status'],
                  error: payload.error,
                }
              : t
          );
        } else {
          // 添加新任务
          return [
            ...prev,
            {
              id: payload.task_id,
              session_id: payload.session_id,
              direction: payload.direction as 'upload' | 'download',
              file_name: payload.file_name,
              remote_path: payload.remote_path,
              file_size: payload.file_size,
              transferred: payload.transferred,
              progress: payload.progress,
              speed: payload.speed_bps,
              eta: payload.eta_secs,
              status: payload.status as TransferTask['status'],
              error: payload.error,
            },
          ];
        }
      });
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

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
        <button className="tn-close-btn">✕</button>
      </div>

      {/* 传输列表 */}
      {!isMinimized && (
        <div className="tn-list">
          {transfers.map(task => (
            <div key={task.id} className="tn-task">
              {/* 文件图标 + 文件名 */}
              <div className="tn-task-header">
                <span className="tn-icon">
                  {task.direction === 'upload' ? '⬆️' : '⬇️'}
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
                  <button onClick={() => {/* TODO: 暂停 */}}>暂停</button>
                )}
                {task.status === 'paused' && (
                  <button onClick={() => {/* TODO: 继续 */}}>继续</button>
                )}
                {(task.status === 'active' || task.status === 'paused') && (
                  <button onClick={() => {/* TODO: 取消 */}}>取消</button>
                )}
                {task.status === 'error' && (
                  <button onClick={() => {/* TODO: 重试 */}}>重试</button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}