import { useState, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';

/**
 * 显示完成通知
 * 使用 Web Notification API 显示系统通知
 */
function showCompletionNotification(payload: TransferProgressPayload) {
  // 检查浏览器是否支持通知
  if (!('Notification' in window)) {
    return;
  }

  // 检查通知权限
  if (Notification.permission === 'denied') {
    return;
  }

  // 如果还没有授权，请求授权
  if (Notification.permission !== 'granted') {
    Notification.requestPermission();
    return;
  }

  // 创建通知
  const direction = payload.direction === 'upload' ? '上传' : '下载';
  const notification = new Notification(`${direction}完成`, {
    body: `${payload.file_name} 已成功${direction}`,
    icon: '/favicon.ico', // 可以替换为应用图标
    tag: `transfer-${payload.id}`, // 防止重复通知
    requireInteraction: false, // 自动关闭
  });

  // 3 秒后自动关闭
  setTimeout(() => {
    notification.close();
  }, 3000);
}

/**
 * 传输任务状态
 */
export interface TransferTask {
  id: string;
  session_id: string;
  direction: 'upload' | 'download';
  file_name: string;
  remote_path: string;
  file_size: number;
  transferred: number;
  speed: number;
  eta: number;
  status: 'pending' | 'active' | 'paused' | 'completed' | 'error';
  error?: string;
  progress: number;
}

/**
 * 传输进度事件 payload
 */
interface TransferProgressPayload {
  id: string;              // ← 改为 id，与后端一致
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
 * 传输进度监听 Hook
 *
 * 自动监听 Tauri 的 transfer-progress 事件，更新传输任务列表
 * 支持按连接 ID 隔离传输任务
 * 完成时自动显示通知并移除任务
 */
export function useTransferProgress(connectionId?: string) {
  const [transfers, setTransfers] = useState<TransferTask[]>([]);

  useEffect(() => {
    const unlisten = listen<TransferProgressPayload>('transfer-progress', (event) => {
      const payload = event.payload;

      // 如果指定了连接 ID，只处理该连接的任务
      if (connectionId && payload.session_id !== connectionId) {
        return;
      }

      setTransfers(prev => {
        const existing = prev.find(t => t.id === payload.id);

        if (existing) {
          // 更新现有任务
          const updatedTasks = prev.map(t =>
            t.id === payload.id
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

          // 检查是否刚完成（从非完成状态变为完成状态）
          const wasCompleted = existing.status === 'completed';
          const nowCompleted = payload.status === 'completed';

          if (!wasCompleted && nowCompleted) {
            // 显示通知
            showCompletionNotification(payload);

            // 3 秒后自动移除已完成的任务
            setTimeout(() => {
              setTransfers(prev => prev.filter(t => t.id !== payload.id));
            }, 3000);
          }

          return updatedTasks;
        } else {
          // 添加新任务
          return [
            ...prev,
            {
              id: payload.id,
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
  }, [connectionId]);

  // 清除已完成的任务
  const clearCompleted = () => {
    setTransfers(prev => prev.filter(t => t.status !== 'completed'));
  };

  // 清除所有任务
  const clearAll = () => {
    setTransfers([]);
  };

  // 移除单个任务
  const removeTask = (taskId: string) => {
    setTransfers(prev => prev.filter(t => t.id !== taskId));
  };

  return {
    transfers,
    clearCompleted,
    clearAll,
    removeTask,
  };
}