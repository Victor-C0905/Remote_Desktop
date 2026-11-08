import { useState, useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import i18n from '../i18n';
import { transferStorage } from '../utils/transferStorage';
import { createLogger } from '../utils/logger';

const log = createLogger('TransferProgress');

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
  const isUpload = payload.direction === 'upload';
  const notification = new Notification(
    i18n.t(isUpload ? 'transfer.notify.uploadFinishedTitle' : 'transfer.notify.downloadFinishedTitle'),
    {
      body: i18n.t(isUpload ? 'transfer.notify.uploadDone' : 'transfer.notify.downloadDone', { name: payload.file_name }),
      icon: '/favicon.ico', // 可以替换为应用图标
      tag: `transfer-${payload.id}`, // 防止重复通知
      requireInteraction: false, // 自动关闭
    }
  );

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
  local_path?: string; // 本地路径（断点记录丢失时重新发起传输用）
  file_size: number;
  transferred: number;
  speed: number;
  eta: number;
  status: 'pending' | 'active' | 'paused' | 'completed' | 'error' | 'cancelled' | 'interrupted';
  error?: string;
  progress: number;
  start_time: number; // 任务开始时间戳（毫秒）
  completed_at?: number; // 任务完成时间戳（毫秒）
}

/**
 * 终态判定（与 Rust 侧 TransferStatus::is_terminal 对齐）
 */
export function isTerminalStatus(status: TransferTask['status']): boolean {
  return status === 'completed' || status === 'error' || status === 'cancelled' || status === 'interrupted';
}

/**
 * 启动恢复降级：Tauri Store 里的非终态任务在 Rust 侧已不存在
 * （TransferManager 的 tasks map 是内存态，重启即空），
 * 统一降级为 interrupted，避免显示无法操作的"僵尸"活动任务。
 */
export function degradeUnfinishedTasks(tasks: TransferTask[]): TransferTask[] {
  return tasks.map(t =>
    isTerminalStatus(t.status)
      ? t
      : { ...t, status: 'interrupted' as const, error: i18n.t('transfer.interruptedError') }
  );
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
  local_path?: string;     // 本地路径（后端 emit 整个 task；中断兜底重发时需要）
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
  // 初始为空数组，通过 useEffect 异步从 Tauri Store 加载
  const [transfers, setTransfers] = useState<TransferTask[]>([]);
  // Store 加载完成标志：加载完成前不 save，避免空数组覆盖持久化数据
  const loadedRef = useRef(false);

  // 异步从 Tauri Store 加载历史传输任务，只保留最近24小时的
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const saved = await transferStorage.load();
      if (cancelled) return;
      const now = Date.now();
      const oneDayMs = 24 * 60 * 60 * 1000;
      const filtered = saved.filter(task => {
        const age = now - task.start_time;
        return age < oneDayMs;
      });
      // 僵尸降级：非终态任务在后端已不存在，统一降级为 interrupted
      setTransfers(degradeUnfinishedTasks(filtered));
      loadedRef.current = true;
    })();
    return () => { cancelled = true; };
  }, []);

  // 最新任务列表的即时引用（节流 flush 时读取，避免闭包捕获过期值）
  const transfersRef = useRef(transfers);
  transfersRef.current = transfers;

  // 监听变化并保存到 Tauri Store（加载完成后才启用，防止空数组覆盖）。
  // 节流：transfer-progress 事件为 tick 级频率，每次全量序列化+写盘会
  // 持续放大 IPC/磁盘 IO——1s 内至多落盘一次，不足 1s 的变更由尾随
  // 定时器合并追平（终态变更最坏延迟 1s 落盘）
  const lastSaveRef = useRef(0);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (!loadedRef.current) return;

    const SAVE_INTERVAL_MS = 1000;
    const flush = () => {
      lastSaveRef.current = Date.now();
      void transferStorage.save(transfersRef.current);
    };

    if (Date.now() - lastSaveRef.current >= SAVE_INTERVAL_MS) {
      // 距上次落盘已超间隔：清掉挂起的尾随保存，立即落盘
      if (saveTimerRef.current) {
        clearTimeout(saveTimerRef.current);
        saveTimerRef.current = null;
      }
      flush();
    } else if (!saveTimerRef.current) {
      saveTimerRef.current = setTimeout(() => {
        saveTimerRef.current = null;
        flush();
      }, SAVE_INTERVAL_MS);
    }
  }, [transfers]);

  // 卸载兜底：窗口关闭时把挂起未落盘的最终状态写回
  useEffect(() => {
    return () => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      if (loadedRef.current) {
        void transferStorage.save(transfersRef.current);
      }
    };
  }, []);

  useEffect(() => {
    log.debug('初始化传输任务数据:', transfers.length, '个任务');
    transfers.forEach(t => {
      log.debug(`任务: id=${t.id}, file=${t.file_name}, status=${t.status}, progress=${t.progress}%`);
    });
  }, []); // 只在初始化时打印一次

  useEffect(() => {
    const unlisten = listen<TransferProgressPayload>('transfer-progress', (event) => {
      const payload = event.payload;

      log.debug('收到事件:', {
        id: payload.id,
        file_name: payload.file_name,
        status: payload.status,
        progress: payload.progress,
        session_id: payload.session_id,
      });

      // 如果指定了连接 ID，只处理该连接的任务
      if (connectionId && payload.session_id !== connectionId) {
        log.debug('忽略其他连接的任务');
        return;
      }

      setTransfers(prev => {
        const existing = prev.find(t => t.id === payload.id);

        if (existing) {
          log.debug(`更新现有任务: id=${payload.id}, progress=${payload.progress}%`);
          // 更新现有任务
          const newStatus = payload.status as TransferTask['status'];
          const statusChanged = existing.status !== newStatus;
          const isTerminal = isTerminalStatus(newStatus);

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
          log.debug(`创建新任务: id=${payload.id}, file=${payload.file_name}`);
          // 添加新任务
          const withNew: TransferTask[] = [
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
              // 保留本地路径：中断任务的记录丢失兜底（transfer_file 重发）需要
              local_path: payload.local_path,
              start_time: Date.now(), // 记录任务开始时间
            },
          ];

          // 终态任务保留上限：超过 MAX_FINISHED 时丢弃最旧的，防任务列表
          // 与持久化存储随使用无界增长（非终态任务不受影响）
          const MAX_FINISHED_TASKS = 50;
          const finished = withNew.filter(t => isTerminalStatus(t.status));
          if (finished.length > MAX_FINISHED_TASKS) {
            const ordered = [...finished].sort((a, b) =>
              (a.completed_at ?? a.start_time) - (b.completed_at ?? b.start_time)
            );
            const evict = new Set(
              ordered.slice(0, finished.length - MAX_FINISHED_TASKS).map(t => t.id)
            );
            return withNew.filter(t => !evict.has(t.id));
          }
          return withNew;
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