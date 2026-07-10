// src/window-system/core/WindowManager.ts

import { Window } from './Window';
import { WindowCollection } from './WindowCollection';
import { WindowRegistry } from './WindowRegistry';
import { WindowEventBus } from '../events/WindowEventBus';
import {
  CreateWindowOptions,
  CreateContext,
  WindowEvent,
  WindowEventType,
  IWindowManager,
  PersistedWindowData,
} from '../types';

/**
 * WindowManager - singleton orchestrator for window system
 * Manages window lifecycle, events, and persistence
 */
export class WindowManager implements IWindowManager {
  private windows: WindowCollection;
  private registry: WindowRegistry;
  private eventBus: WindowEventBus;
  private activeWindowId: string | null = null;
  private saveTimer: ReturnType<typeof setTimeout> | null = null; // ✅ 修复：使用 ReturnType<typeof setTimeout> 代替 NodeJS.Timeout

  constructor(registry: WindowRegistry) {
    this.windows = new WindowCollection();
    this.registry = registry;
    this.eventBus = new WindowEventBus();
  }

  /**
   * Generate unique window ID
   */
  private generateWindowId(appId: string): string {
    return `win-${appId}-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
  }

  /**
   * Get the app definition for a window
   */
  getApp(appId: string): ReturnType<WindowRegistry['get']> {
    return this.registry.get(appId);
  }

  /**
   * Create a new window
   * Handles lifecycle hooks and preloading
   */
  async create(appId: string, options?: CreateWindowOptions): Promise<Window> {
    const app = this.registry.get(appId);
    if (!app) {
      throw new Error(`[WindowManager] App "${appId}" not registered`);
    }

    // Check if app allows multiple instances
    if (!app.allowMultipleInstances) {
      const existingWindows = this.windows.getByAppId(appId);
      if (existingWindows.length > 0) {
        // Focus existing window instead of creating new one
        const existing = existingWindows[0];
        this.focus(existing.id);
        return existing;
      }
    }

    // Calculate initial position and size
    const containerWidth = window.innerWidth;
    const containerHeight = window.innerHeight - 32; // Subtract TopBar height

    // ── 边界约束逻辑（符合标准窗口设计）──────────────
    // 1. 顶部边界：无法超越 TopBar（y >= 0）
    const minY = 0;

    // 2. 左、右、下边界：可以穿越，但保留最小可见区域
    const MIN_VISIBLE = 100; // 最小可见区域（像素）

    const defaultX = Math.min(100 + this.windows.count() * 30, containerWidth - app.defaultSize.width - 100);
    const defaultY = Math.min(100 + this.windows.count() * 30, containerHeight - app.defaultSize.height - 100);

    // 确保默认位置在可视范围内（窗口创建时应该完全可见）
    const constrainedDefaultX = Math.max(0, Math.min(defaultX, containerWidth - app.defaultSize.width));
    const constrainedDefaultY = Math.max(minY, Math.min(defaultY, containerHeight - app.defaultSize.height));

    // 如果提供了自定义位置，应用边界约束
    let position = options?.position ?? { x: constrainedDefaultX, y: constrainedDefaultY };

    if (options?.position) {
      // 应用边界约束（允许部分移出屏幕，但保留最小可见区域）
      const minX = -(app.defaultSize.width - MIN_VISIBLE);
      const maxX = containerWidth - MIN_VISIBLE;
      const maxY = containerHeight - MIN_VISIBLE;

      position = {
        x: Math.max(minX, Math.min(options.position.x, maxX)),
        y: Math.max(minY, Math.min(options.position.y, maxY)),
      };
    }

    const size = options?.size ?? app.defaultSize;

    // Create window instance
    const windowId = this.generateWindowId(appId);
    const newWindow = new Window(windowId, appId, position, size);

    // Ensure new window has highest activatedAt (for z-index ordering)
    // Get current timestamp
    const currentTimestamp = Date.now();

    // Set new window's activatedAt to current timestamp
    newWindow.setActivatedAt(currentTimestamp);

    // Force old active window's activatedAt to be 1ms older (if any)
    // This ensures new window always gets highest z-index
    if (this.activeWindowId) {
      const oldActiveWindow = this.windows.get(this.activeWindowId);
      if (oldActiveWindow) {
        oldActiveWindow.setActivatedAt(currentTimestamp - 1);
      }
    }

    // Add to collection
    this.windows.add(newWindow);
    this.activeWindowId = windowId;

    // Emit created event
    this.eventBus.emit({
      type: 'window:created',
      windowId,
      timestamp: Date.now(),
    });

    // Handle preloading
    if (options?.preloadData) {
      // Use provided preload data (skip onBeforeCreate)
      newWindow.setPreloadData(options.preloadData);
    } else if (app.lifecycle?.onBeforeCreate) {
      // Call onBeforeCreate hook
      const context: CreateContext = {
        serverId: options?.serverId,
        existingPreloadData: options?.preloadData,
      };
      try {
        const preloadData = await app.lifecycle.onBeforeCreate(context);
        newWindow.setPreloadData(preloadData);
      } catch (error) {
        console.error(`[WindowManager] Preload failed for "${appId}"`, error);
        newWindow.setPreloadState('error');
      }
    } else {
      // No preloading needed
      newWindow.setPreloadState('ready');
    }

    // Call onAfterCreate hook
    if (app.lifecycle?.onAfterCreate) {
      app.lifecycle.onAfterCreate(newWindow);
    }

    return newWindow;
  }

  /**
   * Close a window
   */
  close(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    const app = this.registry.get(window.appId);
    if (app?.lifecycle?.onBeforeClose) {
      const shouldClose = app.lifecycle.onBeforeClose(window);
      if (!shouldClose) return; // Cancel close
    }

    this.windows.remove(windowId);

    // Update active window
    if (this.activeWindowId === windowId) {
      this.activeWindowId = this.windows.getActive()?.id ?? null;
    }

    // Emit closed event
    this.eventBus.emit({
      type: 'window:closed',
      windowId,
      timestamp: Date.now(),
    });

    // Call onAfterClose hook
    if (app?.lifecycle?.onAfterClose) {
      app.lifecycle.onAfterClose(window);
    }
  }

  /**
   * Focus a window (bring to front)
   */
  focus(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    // Restore if minimized
    if (window.minimized) {
      window.setMinimized(false);
    }

    // Update activation time
    window.activate();
    this.activeWindowId = windowId;

    // Emit focused event
    this.eventBus.emit({
      type: 'window:focused',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * Minimize a window
   */
  minimize(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    window.setMinimized(true);

    // Update active window
    if (this.activeWindowId === windowId) {
      this.activeWindowId = this.windows.getActive()?.id ?? null;
    }

    // Emit minimized event
    this.eventBus.emit({
      type: 'window:minimized',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * Restore a minimized window
   */
  restore(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    window.setMinimized(false);
    window.activate();
    this.activeWindowId = windowId;

    // Emit restored event
    this.eventBus.emit({
      type: 'window:restored',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * ✅ Maximize a window
   * Saves current position/size, then applies maximized dimensions
   */
  maximize(windowId: string, maxPosition: { x: number; y: number }, maxSize: { width: number; height: number }): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    // ✅ setMaximized 内部会自动保存当前位置/尺寸
    window.setMaximized(true);
    window.setPosition(maxPosition);
    window.setSize(maxSize);

    // Emit maximized event
    this.eventBus.emit({
      type: 'window:maximized',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * ✅ Unmaximize a window
   * Restores saved position/size (or defaults to center)
   */
  unmaximize(windowId: string): void {
    const window = this.windows.get(windowId);
    if (!window) return;

    // ✅ setMaximized(false) 内部会自动恢复保存的位置/尺寸
    window.setMaximized(false);

    // Emit unmaximized event
    this.eventBus.emit({
      type: 'window:unmaximized',
      windowId,
      timestamp: Date.now(),
    });
  }

  /**
   * Get window by ID
   */
  getById(windowId: string): Window | undefined {
    return this.windows.get(windowId);
  }

  /**
   * Get windows by app ID
   */
  getByAppId(appId: string): Window[] {
    return this.windows.getByAppId(appId);
  }

  /**
   * Get all windows
   */
  getAll(): Window[] {
    return this.windows.getAll();
  }

  /**
   * Get the active window
   */
  getActive(): Window | undefined {
    return this.activeWindowId ? this.windows.get(this.activeWindowId) : undefined;
  }

  /**
   * Subscribe to window events
   */
  on(eventType: WindowEventType, handler: (event: WindowEvent) => void): () => void {
    return this.eventBus.on(eventType, handler);
  }

  /**
   * Subscribe to any event (for React force update)
   */
  onAny(handler: () => void): () => void {
    return this.eventBus.onAny(handler);
  }

  /**
   * Emit an event
   */
  emit(event: WindowEvent): void {
    this.eventBus.emit(event);
  }

  /**
   * Save window states to localStorage
   * ✅ 优化：异步延迟执行，避免频繁写入和阻塞 UI
   */
  save(): void {
    // ✅ 清除之前的定时器
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
    }
    
    // ✅ 延迟 1 秒后执行 save（避免频繁写入）
    this.saveTimer = setTimeout(() => {
      try {
        const data = this.windows.getAll().map(w => w.serialize());
        localStorage.setItem('gnome-remote-windows', JSON.stringify(data));
      } catch (error) {
        console.error('[WindowManager] Failed to save window states:', error);
        // ✅ 尝试清理旧数据后再保存
        try {
          const data = this.windows.getAll().map(w => w.serialize());
          localStorage.removeItem('gnome-remote-windows');
          localStorage.setItem('gnome-remote-windows', JSON.stringify(data));
        } catch (retryError) {
          console.error('[WindowManager] Retry save failed:', retryError);
          // 最终失败，不影响应用运行
        }
      }
      this.saveTimer = null;
    }, 1000);
  }

  /**
   * Load window states from localStorage
   * Applies boundary constraints to restored positions
   */
  load(): void {
    const stored = localStorage.getItem('gnome-remote-windows');
    if (!stored) return;

    try {
      const data = JSON.parse(stored) as PersistedWindowData[];

      // ── 边界约束逻辑（符合标准窗口设计）──────────────
      const containerWidth = window.innerWidth;
      const containerHeight = window.innerHeight - 32; // Subtract TopBar height
      const minY = 0; // 无法超越 TopBar
      const MIN_VISIBLE = 100; // 最小可见区域

      data.forEach(windowData => {
        const window = Window.deserialize(windowData);

        // 应用边界约束（防止窗口恢复时超出屏幕边界）
        const minX = -(windowData.size.width - MIN_VISIBLE);
        const maxX = containerWidth - MIN_VISIBLE;
        const maxY = containerHeight - MIN_VISIBLE;

        const constrainedPosition = {
          x: Math.max(minX, Math.min(windowData.position.x, maxX)),
          y: Math.max(minY, Math.min(windowData.position.y, maxY)),
        };

        // 更新窗口位置（如果超出边界）
        if (windowData.position.x !== constrainedPosition.x ||
            windowData.position.y !== constrainedPosition.y) {
          window.setPosition(constrainedPosition);
        }

        this.windows.add(window);
      });
    } catch (error) {
      console.error('[WindowManager] Failed to load window states', error);
    }
  }
}