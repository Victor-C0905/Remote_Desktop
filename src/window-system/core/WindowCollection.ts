// src/window-system/core/WindowCollection.ts

import { Window } from './Window';

/**
 * Window collection for managing multiple windows
 * Provides CRUD operations (activation ordering lives in WindowManager's MRU stack)
 */
export class WindowCollection {
  private windows: Map<string, Window> = new Map();

  /**
   * Add a window to the collection
   */
  add(window: Window): void {
    this.windows.set(window.id, window);
  }

  /**
   * Remove a window from the collection
   */
  remove(windowId: string): boolean {
    return this.windows.delete(windowId);
  }

  /**
   * Get a window by ID
   */
  get(windowId: string): Window | undefined {
    return this.windows.get(windowId);
  }

  /**
   * Get all windows as array
   */
  getAll(): Window[] {
    return Array.from(this.windows.values());
  }

  /**
   * Get all windows for a specific app
   */
  getByAppId(appId: string): Window[] {
    return this.getAll().filter(w => w.appId === appId);
  }

  /**
   * Check if a window exists
   */
  has(windowId: string): boolean {
    return this.windows.has(windowId);
  }

  /**
   * Get the number of windows
   */
  count(): number {
    return this.windows.size;
  }

  /**
   * Clear all windows (for testing)
   */
  clear(): void {
    this.windows.clear();
  }
}