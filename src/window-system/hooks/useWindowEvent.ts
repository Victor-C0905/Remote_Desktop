// src/window-system/hooks/useWindowEvent.ts

import { useEffect } from 'react';
import { useWindowManager } from './useWindowManager';
import { WindowEventType, WindowEvent } from '../types';

/**
 * Hook to subscribe to window events
 * Automatically unsubscribes on unmount
 */
export function useWindowEvent(
  eventType: WindowEventType,
  handler: (event: WindowEvent) => void
): void {
  const manager = useWindowManager();

  useEffect(() => {
    return manager.on(eventType, handler);
  }, [manager, eventType, handler]);
}