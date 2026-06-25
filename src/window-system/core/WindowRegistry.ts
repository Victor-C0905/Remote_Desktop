// src/window-system/core/WindowRegistry.ts

import { AppDefinition } from '../types';

/**
 * Application registry for window system
 * Stores app definitions that can be used to create windows
 */
export class WindowRegistry {
  private apps: Map<string, AppDefinition> = new Map();

  /**
   * Register an application
   */
  register(app: AppDefinition): void {
    if (this.apps.has(app.id)) {
      console.warn(`[WindowRegistry] App "${app.id}" already registered, overwriting`);
    }
    this.apps.set(app.id, app);
  }

  /**
   * Get an application definition by ID
   */
  get(appId: string): AppDefinition | undefined {
    return this.apps.get(appId);
  }

  /**
   * Get all registered applications
   */
  getAll(): AppDefinition[] {
    return Array.from(this.apps.values());
  }

  /**
   * Check if an app is registered
   */
  has(appId: string): boolean {
    return this.apps.has(appId);
  }

  /**
   * Clear all registered apps (for testing)
   */
  clear(): void {
    this.apps.clear();
  }
}