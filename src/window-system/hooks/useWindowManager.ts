// src/window-system/hooks/useWindowManager.ts

import { useContext } from 'react';
import { WindowManagerContext } from '../WindowManagerContext';
import { IWindowManager } from '../types';

/**
 * Hook to access the WindowManager instance
 */
export function useWindowManager(): { manager: IWindowManager } {
  const context = useContext(WindowManagerContext);
  if (!context) {
    throw new Error('[useWindowManager] WindowManagerContext not provided');
  }
  return context;
}
