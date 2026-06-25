// src/window-system/hooks/useWindowManager.ts

import { useContext, useReducer, useEffect } from 'react';
import { WindowManagerContext } from '../WindowManagerContext';
import { IWindowManager } from '../types';

/**
 * Hook to access the WindowManager instance
 * Automatically re-renders when any window event occurs
 */
export function useWindowManager(): IWindowManager {
  const manager = useContext(WindowManagerContext);
  if (!manager) {
    throw new Error('[useWindowManager] WindowManagerContext not provided');
  }

  // Force re-render on any window event
  const [, forceUpdate] = useReducer(x => x + 1, 0);

  useEffect(() => {
    return manager.onAny(() => forceUpdate());
  }, [manager]);

  return manager;
}