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

    const position = options?.position ?? {
      x: Math.min(100 + this.windows.count() * 30, containerWidth - app.defaultSize.width - 100),
      y: Math.min(100 + this.windows.count() * 30, containerHeight - app.defaultSize.height - 100),
    };

    const size = options?.size ?? app.defaultSize;

    // Create window instance
    const windowId = this.generateWindowId(appId);
    const newWindow = new Window(windowId, appId, position, size);

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
   */
  save(): void {
    const data = this.windows.getAll().map(w => w.serialize());
    localStorage.setItem('gnome-remote-windows', JSON.stringify(data));
  }

  /**
   * Load window states from localStorage
   */
  load(): void {
    const stored = localStorage.getItem('gnome-remote-windows');
    if (!stored) return;

    try {
      const data = JSON.parse(stored) as PersistedWindowData[];
      data.forEach(windowData => {
        const window = Window.deserialize(windowData);
        this.windows.add(window);
      });
    } catch (error) {
      console.error('[WindowManager] Failed to load window states', error);
    }
  }
}