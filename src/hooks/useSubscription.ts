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
  onEvent: (event: SubscriptionEvent) => void,
  options?: {
    key?: string;
    revision?: string | number;
    onStatus?: (active: boolean) => void;
    /** 订阅请求失败时回调（旧版 Agent 不支持该类型等）——区别于正常退订的 onStatus(false) */
    onSubscribeError?: (e: unknown) => void;
  }
) {
  // 使用 ref 来跟踪订阅状态
  const subscribedRef = useRef(false);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  // 订阅唯一键：多个订阅（metrics / file_changes）共存时互不覆盖；
  // 未显式指定时为该 hook 实例生成稳定随机键（卸载时按同一键退订）
  const autoKeyRef = useRef(`sub-${Math.random().toString(36).slice(2)}`);
  const subKey = options?.key ?? autoKeyRef.current;
  const revision = options?.revision;

  // 使用 ref 来存储 onEvent，避免 useEffect 重新执行
  const onEventRef = useRef(onEvent);
  onEventRef.current = onEvent;
  // 订阅成功/失败回调（同样走 ref：调用方内联箭头函数不会触发重订阅）
  const onStatusRef = useRef(options?.onStatus);
  onStatusRef.current = options?.onStatus;
  // 订阅请求本身失败（旧版 Agent 不支持该类型等）：与「正常退订的 onStatus(false)」
  // 区分开，调用方可据此做一次性降级提示而不必在卸载路径里甄别
  const onSubscribeErrorRef = useRef(options?.onSubscribeError);
  onSubscribeErrorRef.current = options?.onSubscribeError;

  useEffect(() => {
    if (!serverId || types.length === 0) return;

    // 订阅
    const subscribe = async () => {
      try {
        // 发送订阅请求
        await invoke('subscribe', { serverId, types, key: subKey });

        // 标记已订阅
        subscribedRef.current = true;
        onStatusRef.current?.(true);

        // 监听事件
        const fn = await listen<SubscriptionEvent>('subscription_event', (event) => {
          if (event.payload.server_id === serverId) {
            onEventRef.current(event.payload);
          }
        });

        unlistenRef.current = fn;
      } catch (e) {
        // 旧版 Agent 未实现该订阅类型时会走到这里（降级提示，不刷 error）
        log.warn('订阅失败:', e);
        subscribedRef.current = false;
        onStatusRef.current?.(false);
        onSubscribeErrorRef.current?.(e);
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
        invoke('unsubscribe', { serverId, types, key: subKey }).catch((e) => {
          log.error('取消订阅失败:', e);
        });
        subscribedRef.current = false;
      }
      // 退订后由调用方回退到兼容路径（如上传完成事件）
      onStatusRef.current?.(false);
    };
  }, [serverId, types, subKey, revision]); // 移除 onEvent 依赖
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

// 文件变化订阅 Hook（inotify 文件级增量）
export function useFileChangesSubscription(
  serverId: string | null,
  path: string,
  recursive: boolean,
  onFileChange: (data: any) => void,
  onStatus?: (active: boolean) => void,
  onSubscribeError?: (e: unknown) => void
) {
  // types 稳定化（审计 L4）：裸数组每次渲染新建，effect deps 含 types
  // 会每渲染 unsubscribe→subscribe 循环
  const types = useMemo<SubscriptionType[]>(() => [
    { type: 'file_changes', params: { path, recursive } },
  ], [path, recursive]);

  // 每个 hook 实例一条独立订阅流（useSubscription 内部自动生成唯一 key）：
  // 多个文件管理器窗口可同时监听同一目录，互不覆盖/误退订
  const handleEvent = useCallback((event: SubscriptionEvent) => {
    if (event.event_type === 'file_changes') {
      onFileChange(event.data);
    }
  }, [onFileChange]);

  useSubscription(serverId, types, handleEvent, { onStatus, onSubscribeError });
}

// 新增：进程事件订阅 Hook
export function useProcessEventsSubscription(
  serverId: string | null,
  onProcessEvent: (data: any) => void
) {
  const types = useMemo<SubscriptionType[]>(() => [
    { type: 'process_events', params: { interval_secs: 2 } },
  ], []);

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
  const types = useMemo<SubscriptionType[]>(() => [
    { type: 'app_logs', params: { app_name: appName, level } },
  ], [appName, level]);

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
  const types = useMemo<SubscriptionType[]>(() => [
    { type: 'service_status', params: { service, interval_secs: 5 } },
  ], [service]);

  const handleEvent = useCallback((event: SubscriptionEvent) => {
    if (event.event_type === 'service_status') {
      onStatusChange(event.data);
    }
  }, [onStatusChange]);

  useSubscription(serverId, types, handleEvent);
}