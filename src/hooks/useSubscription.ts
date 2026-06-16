// src/hooks/useSubscription.ts

import { useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { SubscriptionType, SubscriptionEvent } from '../config/subscription';

export function useSubscription(
  serverId: string | null,
  types: SubscriptionType[],
  onEvent: (event: SubscriptionEvent) => void
) {
  useEffect(() => {
    if (!serverId || types.length === 0) return;

    let unlisten: UnlistenFn | null = null;

    // 订阅
    invoke('subscribe', { serverId, types })
      .then(() => {
        // 监听事件
        return listen<SubscriptionEvent>('subscription_event', (event) => {
          if (event.payload.server_id === serverId) {
            onEvent(event.payload);
          }
        });
      })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((e) => {
        console.error('订阅失败:', e);
      });

    // 清理：取消订阅
    return () => {
      if (unlisten) {
        unlisten();
      }
      invoke('unsubscribe', { serverId, types }).catch((e) => {
        console.error('取消订阅失败:', e);
      });
    };
  }, [serverId, types, onEvent]);
}

export function useMetricsSubscription(
  serverId: string | null,
  onMetrics: (metrics: any) => void
) {
  const types: SubscriptionType[] = [
    { type: 'metrics', params: { interval_secs: 1 } },
  ];

  const handleEvent = useCallback((event: SubscriptionEvent) => {
    if (event.event_type === 'metrics') {
      onMetrics(event.data);
    }
  }, [onMetrics]);

  useSubscription(serverId, types, handleEvent);
}