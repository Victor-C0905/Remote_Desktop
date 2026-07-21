import { useState, useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';

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
 */
export function useTransferProgress() {
  const [transfers, setTransfers] = useState<TransferTask[]>([]);

  useEffect(() => {
    const unlisten = listen<TransferProgressPayload>('transfer-progress', (event) => {
      const payload = event.payload;

      setTransfers(prev => {
        const existing = prev.find(t => t.id === payload.id);  // ← 使用 payload.id

        if (existing) {
          // 更新现有任务
          return prev.map(t =>
            t.id === payload.id  // ← 使用 payload.id
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
              id: payload.id,  // ← 使用 payload.id
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

  // 清除已完成的任务
  const clearCompleted = () => {
    setTransfers(prev => prev.filter(t => t.status !== 'completed'));
  };

  // 清除所有任务
  const clearAll = () => {
    setTransfers([]);
  };

  return {
    transfers,
    clearCompleted,
    clearAll,
  };
}