import { useState, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { transferStorage } from '../utils/transferStorage';

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
  start_time: number; // 任务开始时间戳（毫秒）
  completed_at?: number; // 任务完成时间戳（毫秒）
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
  // 从 localStorage 加载初始状态，只保留最近24小时的任务
  const [transfers, setTransfers] = useState<TransferTask[]>(() => {
    const saved = transferStorage.load();
    const now = Date.now();
    const oneDayMs = 24 * 60 * 60 * 1000;
    
    return saved.filter(task => {
      // 只保留最近24小时的任务
      const age = now - task.start_time;
      return age < oneDayMs;
    });
  });

  // 监听变化并保存到 localStorage
  useEffect(() => {
    transferStorage.save(transfers);
  }, [transfers]);

  useEffect(() => {
    console.log('[useTransferProgress] 初始化 localStorage 数据:', transfers.length, '个任务');
    transfers.forEach(t => {
      console.log(`[useTransferProgress] - 任务: id=${t.id}, file=${t.file_name}, status=${t.status}, progress=${t.progress}%`);
    });
  }, []); // 只在初始化时打印一次

  useEffect(() => {
    const unlisten = listen<TransferProgressPayload>('transfer-progress', (event) => {
      const payload = event.payload;

      console.log('[useTransferProgress] 收到事件:', {
        id: payload.id,
        file_name: payload.file_name,
        status: payload.status,
        progress: payload.progress,
        session_id: payload.session_id,
      });

      // 如果指定了连接 ID，只处理该连接的任务
      if (connectionId && payload.session_id !== connectionId) {
        console.log('[useTransferProgress] 忽略其他连接的任务');
        return;
      }

      setTransfers(prev => {
        const existing = prev.find(t => t.id === payload.id);

        if (existing) {
          console.log(`[useTransferProgress] 更新现有任务: id=${payload.id}, progress=${payload.progress}%`);
          // 更新现有任务
          const newStatus = payload.status as TransferTask['status'];
          const statusChanged = existing.status !== newStatus;
          const isTerminal = newStatus === 'completed' || newStatus === 'error' || newStatus === 'cancelled';

          const updatedTasks = prev.map(t =>
            t.id === payload.id
              ? {
                  ...t,
                  file_size: payload.file_size, // 更新文件大小（解决显示 0B 的问题）
                  transferred: payload.transferred,
                  progress: payload.progress,
                  speed: payload.speed_bps,
                  eta: payload.eta_secs,
                  status: newStatus,
                  error: payload.error,
                  // 状态变为终态时记录完成时间
                  ...(statusChanged && isTerminal && !t.completed_at ? { completed_at: Date.now() } : {}),
                }
              : t
          );

          // 检查是否刚完成（从非完成状态变为完成状态）
          const wasCompleted = existing.status === 'completed';
          const nowCompleted = payload.status === 'completed';

          if (!wasCompleted && nowCompleted) {
            // 记录完成时间
            const finalTasks = updatedTasks.map(t =>
              t.id === payload.id ? { ...t, completed_at: Date.now() } : t
            );

            // 显示通知
            showCompletionNotification(payload);

            // 任务完成后保留在列表中，只有用户手动关闭或关闭窗口时才移除
            // 不自动移除已完成任务
            return finalTasks;
          }

          return updatedTasks;
        } else {
          console.log(`[useTransferProgress] 创建新任务: id=${payload.id}, file=${payload.file_name}`);
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
              start_time: Date.now(), // 记录任务开始时间
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