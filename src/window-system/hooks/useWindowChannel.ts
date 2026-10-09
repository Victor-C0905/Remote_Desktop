// src/window-system/hooks/useWindowChannel.ts

import { useEffect } from 'react';
import { useWindowManager } from './useWindowManager';
import {
  CustomEventType,
  TypedWindowEvent,
  EventPayloadMap
} from '../events/WindowChannel';

/**
 * Hook for typed inter-window communication
 * Provides a structured way to send and receive events
 */
export function useWindowChannel() {
  const { manager } = useWindowManager();

  /**
   * Subscribe to a typed event
   */
  const subscribe = <T extends CustomEventType>(
    eventType: T,
    handler: (event: TypedWindowEvent<T>) => void
  ) => {
    // Get channel from manager (we'll add this to WindowManager)
    // For now, use the event bus directly
    return manager.on(eventType as any, handler as any);
  };

  /**
   * Emit a typed event
   */
  const emit = <T extends CustomEventType>(
    eventType: T,
    payload: EventPayloadMap[T],
    sourceWindowId: string
  ) => {
    manager.emit({
      type: eventType as any,
      windowId: sourceWindowId,
      timestamp: Date.now(),
      payload,
    });
  };

  /**
   * Send to a specific window
   */
  const sendTo = <T extends CustomEventType>(
    _targetWindowId: string,
    eventType: T,
    payload: EventPayloadMap[T],
    sourceWindowId: string
  ) => {
    manager.emit({
      type: eventType as any,
      windowId: sourceWindowId,
      timestamp: Date.now(),
      payload,
    });
  };

  return {
    subscribe,
    emit,
    sendTo,
  };
}

/**
 * Hook to subscribe to a specific event type
 * Automatically unsubscribes on unmount
 */
export function useTypedWindowEvent<T extends CustomEventType>(
  eventType: T,
  handler: (event: TypedWindowEvent<T>) => void,
  sourceWindowId?: string // Optional: filter by source
): void {
  const { manager } = useWindowManager();

  useEffect(() => {
    const unsubscribe = manager.on(eventType as any, (event: any) => {
      // Filter by source window if specified
      if (sourceWindowId && event.windowId !== sourceWindowId) {
        return;
      }
      handler(event);
    });

    return unsubscribe;
  }, [manager, eventType, handler, sourceWindowId]);
}