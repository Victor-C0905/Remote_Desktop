// src/hooks/useSubscription.ts

import { useEffect, useCallback, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { SubscriptionType, SubscriptionEvent } from '../config/subscription';
import { createLogger } from '../utils/logger';

const log = createLogger('Subscription');

export function useSubscription(
  serverId: string | null,
  types: SubscriptionType[],
  onEvent: (event: SubscriptionEvent) => void
) {
  // 使用 ref 来跟踪订阅状态
  const subscribedRef = useRef(false);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  // 使用 ref 来存储 onEvent，避免 useEffect 重新执行
  const onEventRef = useRef(onEvent);
  onEventRef.current = onEvent;

  useEffect(() => {
    if (!serverId || types.length === 0) return;

    // 订阅
    const subscribe = async () => {
      try {
        // 发送订阅请求
        await invoke('subscribe', { serverId, types });

        // 标记已订阅
        subscribedRef.current = true;

        // 监听事件（注意：当前 Tauri Backend 尚未完善持久 Stream 管理，暂时无法接收事件）
        // 后续完善 Tauri Backend 后，这里会正常工作
        const fn = await listen<SubscriptionEvent>('subscription_event', (event) => {
          if (event.payload.server_id === serverId) {
            onEventRef.current(event.payload);
          }
        });

        unlistenRef.current = fn;
      } catch (e) {
        log.error('订阅失败:', e);
        subscribedRef.current = false;
      }
    };

    subscribe();

    // 清理：取消订阅
    return () => {
      // 取消监听
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }

      // 取消订阅
      if (subscribedRef.current) {
        invoke('unsubscribe', { serverId, types }).catch((e) => {
          log.error('取消订阅失败:', e);
        });
        subscribedRef.current = false;
      }
    };
  }, [serverId, types]); // 移除 onEvent 依赖
}

export function useMetricsSubscription(
  serverId: string | null,
  onMetrics: (metrics: any) => void
) {
  // 使用 useMemo 来稳定 types 数组
  const types = useMemo<SubscriptionType[]>(() => [
    { type: 'metrics', params: { interval_secs: 1 } },
  ], []);

  const handleEvent = useCallback((event: SubscriptionEvent) => {
    if (event.event_type === 'metrics') {
      onMetrics(event.data);
    }
  }, [onMetrics]);

  useSubscription(serverId, types, handleEvent);
}

// 新增：文件变化订阅 Hook
export function useFileChangesSubscription(
  serverId: string | null,
  path: string,
  recursive: boolean,
  onFileChange: (data: any) => void
) {
  const types: SubscriptionType[] = [
    { type: 'file_changes', params: { path, recursive } },
  ];

  const handleEvent = useCallback((event: SubscriptionEvent) => {
    if (event.event_type === 'file_changes') {
      onFileChange(event.data);
    }
  }, [onFileChange]);

  useSubscription(serverId, types, handleEvent);
}

// 新增：进程事件订阅 Hook
export function useProcessEventsSubscription(
  serverId: string | null,
  onProcessEvent: (data: any) => void
) {
  const types: SubscriptionType[] = [
    { type: 'process_events', params: { interval_secs: 2 } },
  ];

  const handleEvent = useCallback((event: SubscriptionEvent) => {
    if (event.event_type === 'process_events') {
      onProcessEvent(event.data);
    }
  }, [onProcessEvent]);

  useSubscription(serverId, types, handleEvent);
}

// 新增：应用日志订阅 Hook
export function useAppLogsSubscription(
  serverId: string | null,
  appName: string,
  level: string | undefined,
  onLog: (data: any) => void
) {
  const types: SubscriptionType[] = [
    { type: 'app_logs', params: { app_name: appName, level } },
  ];

  const handleEvent = useCallback((event: SubscriptionEvent) => {
    if (event.event_type === 'app_logs') {
      onLog(event.data);
    }
  }, [onLog]);

  useSubscription(serverId, types, handleEvent);
}

// 新增：服务状态订阅 Hook
export function useServiceStatusSubscription(
  serverId: string | null,
  service: string,
  onStatusChange: (data: any) => void
) {
  const types: SubscriptionType[] = [
    { type: 'service_status', params: { service, interval_secs: 5 } },
  ];

  const handleEvent = useCallback((event: SubscriptionEvent) => {
    if (event.event_type === 'service_status') {
      onStatusChange(event.data);
    }
  }, [onStatusChange]);

  useSubscription(serverId, types, handleEvent);
}