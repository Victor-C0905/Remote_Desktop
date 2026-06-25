// src/window-system/WindowManagerContext.tsx

import { createContext, useContext, useMemo, useEffect, useReducer, ReactNode } from 'react';
import { WindowManager } from './core/WindowManager';
import { WindowRegistry } from './core/WindowRegistry';
import { IWindowManager } from './types';
import { initWindowRegistry } from './init';

// Create context with null default
export const WindowManagerContext = createContext<IWindowManager | null>(null);

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

  // Load persisted windows on mount
  useEffect(() => {
    manager.load();
  }, [manager]);

  // Save windows on changes
  useEffect(() => {
    return manager.onAny(() => {
      manager.save();
    });
  }, [manager]);

  return (
    <WindowManagerContext.Provider value={manager}>
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