// src/window-system/hooks/useWindowState.ts

import { useWindowManager } from './useWindowManager';
import { WindowState } from '../types';

/**
 * Hook to access a single window's state
 * Returns window state and control methods
 *
 * ✅ 修复：依赖 globalState.activeWindowId，确保窗口激活状态变化时触发重新渲染
 */
export function useWindowState(windowId: string): WindowState {
  // ✅ 关键修复：获取 globalState，确保依赖 activeWindowId
  const { manager, globalState } = useWindowManager();
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
    // ✅ 关键修复：使用 globalState.activeWindowId，确保状态更新时触发重新渲染
    focused: globalState.activeWindowId === windowId,
    preloadState: window.preloadState,
    preloadData: window.preloadData,
  };
}