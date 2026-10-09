// src/window-system/core/WindowCollection.ts

import { Window } from './Window';

/**
 * Window collection for managing multiple windows
 * Provides CRUD operations and sorting by activation time
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
   * Get the most recently activated window
   */
  getActive(): Window | undefined {
    const windows = this.getAll();
    if (windows.length === 0) return undefined;

    return windows.reduce((latest, current) => {
      return current.activatedAt > latest.activatedAt ? current : latest;
    });
  }

  /**
   * Get windows sorted by activation time (oldest first)
   * Used for z-index ordering (oldest = lowest z-index)
   */
  getSortedByActivation(): Window[] {
    return this.getAll().sort((a, b) => a.activatedAt - b.activatedAt);
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