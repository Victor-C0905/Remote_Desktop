// src/window-system/WindowManagerContext.tsx

import { createContext, useContext, useMemo, useEffect, useState, ReactNode } from 'react';
import { WindowManager } from './core/WindowManager';
import { WindowRegistry } from './core/WindowRegistry';
import { IWindowManager } from './types';
import { initWindowRegistry } from './init';

// ✅ 新增：全局状态类型
interface GlobalState {
  windowList: Array<{ id: string; appId: string; isMinimized: boolean }>;
  activeWindowId: string | null;
}

// ✅ 新增：Context 类型（包含 manager 和 globalState）
interface WindowManagerContextValue {
  manager: IWindowManager;
  globalState: GlobalState;
}

// Create context with null default
export const WindowManagerContext = createContext<WindowManagerContextValue | null>(null);

// Registry singleton (shared across all WindowManager instances)
let registryInstance: WindowRegistry | null = null;

/**
 * Provider component for WindowManager
 * Initializes registry and manager on mount
 */
export function WindowManagerProvider({ children }: { children: ReactNode }) {
  // Initialize registry singleton
  const registry = useMemo(() => {
    if (!registryInstance) {
      registryInstance = new WindowRegistry();
      initWindowRegistry(registryInstance);
    }
    return registryInstance;
  }, []);

  // Create manager instance
  const manager = useMemo(() => {
    return new WindowManager(registry);
  }, [registry]);

  // ✅ 新增：全局关键状态
  const [globalState, setGlobalState] = useState<GlobalState>({
    windowList: [],
    activeWindowId: null,
  });

  // Load persisted windows on mount
  useEffect(() => {
    manager.load();
  }, [manager]);

  // ✅ 新增：选择性事件监听（只监听关键事件）
  useEffect(() => {
    return manager.onAny(() => {
      // ✅ 修复：onAny 签名是 () => void，不接受 event 参数
      // 直接更新状态（所有关键事件都触发更新）
      setGlobalState({
        windowList: manager.getAll().map(w => ({
          id: w.id,
          appId: w.appId,
          isMinimized: w.minimized
        })),
        activeWindowId: manager.getActive()?.id || null
      });
    });
  }, [manager]);

  // Save windows on changes（保持原有逻辑）
  useEffect(() => {
    return manager.onAny(() => {
      manager.save();
    });
  }, [manager]);

  return (
    <WindowManagerContext.Provider value={{
      manager,
      globalState  // ✅ 新增：提供精简全局状态
    }}>
      {children}
    </WindowManagerContext.Provider>
  );
}

/**
 * Hook to check if WindowManager is available
 */
export function useWindowManagerContext(): boolean {
  return useContext(WindowManagerContext) !== null;
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