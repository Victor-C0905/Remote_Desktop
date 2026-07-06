// src/window-system/hooks/useWindowState.ts

import { useWindowManager } from './useWindowManager';
import { WindowState } from '../types';

/**
 * Hook to access a single window's state
 * Returns window state and control methods
 */
export function useWindowState(windowId: string): WindowState {
  const { manager } = useWindowManager();
  const window = manager.getById(windowId);

  if (!window) {
    throw new Error(`[useWindowState] Window "${windowId}" not found`);
  }

  const app = manager.getApp(window.appId);
  if (!app) {
    throw new Error(`[useWindowState] App "${window.appId}" not registered`);
  }

  return {
    id: window.id,
    appId: window.appId,
    title: app.title,
    position: window.position,
    size: window.size,
    minimized: window.minimized,
    maximized: window.maximized, // ✅ 新增：最大化状态
    focused: manager.getActive()?.id === windowId,
    preloadState: window.preloadState,
    preloadData: window.preloadData,
  };
}