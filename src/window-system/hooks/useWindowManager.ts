// src/window-system/hooks/useWindowManager.ts

import { useContext } from 'react';
import { WindowManagerContext } from '../WindowManagerContext';
import { IWindowManager } from '../types';

// ✅ 新增：Context 类型（包含 manager 和 globalState）
interface WindowManagerContextValue {
  manager: IWindowManager;
  globalState: {
    windowList: Array<{ id: string; appId: string; isMinimized: boolean }>;
    activeWindowId: string | null;
  };
}

/**
 * Hook to access the WindowManager instance
 * Returns manager and globalState
 */
export function useWindowManager(): WindowManagerContextValue {
  const context = useContext(WindowManagerContext);
  if (!context) {
    throw new Error('[useWindowManager] WindowManagerContext not provided');
  }

  // ✅ 移除：不再强制重渲染（改为依赖 globalState）
  return context;
}