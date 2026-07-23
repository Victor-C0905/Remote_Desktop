// src/window-system/persistence/TauriStorageAdapter.ts

import { StorageAdapter } from './StorageAdapter';
import { createLogger } from '../../utils/logger';

const log = createLogger('TauriStorageAdapter');

/**
 * Tauri file system storage adapter
 * Uses Tauri's fs API for persistent storage
 * Falls back to localStorage when Tauri is not available
 */
export class TauriStorageAdapter implements StorageAdapter {
  private fallbackAdapter: LocalStorageAdapter;
  private tauriAvailable: boolean;

  constructor() {
    this.fallbackAdapter = new LocalStorageAdapter();
    this.tauriAvailable = this.checkTauriAvailable();
  }

  /**
   * Check if Tauri API is available
   */
  private checkTauriAvailable(): boolean {
    try {
      // Check if running in Tauri environment
      return typeof window !== 'undefined' && '__TAURI__' in window;
    } catch {
      return false;
    }
  }

  /**
   * Save data to storage
   */
  async save(key: string, data: string): Promise<void> {
    if (this.tauriAvailable) {
      try {
        // Use Tauri fs API when available
        // Note: Requires @tauri-apps/plugin-fs to be installed
        const { writeTextFile } = await import('@tauri-apps/plugin-fs');
        const { appDataDir } = await import('@tauri-apps/api/path');
        const appDataDirPath = await appDataDir();
        await writeTextFile(`${appDataDirPath}/${key}.json`, data);
      } catch (error) {
        log.error('Failed to save to Tauri fs', error);
        // Fallback to localStorage
        this.fallbackAdapter.save(key, data);
      }
    } else {
      // Use localStorage when Tauri is not available
      this.fallbackAdapter.save(key, data);
    }
  }

  /**
   * Load data from storage
   */
  async load(key: string): Promise<string | null> {
    if (this.tauriAvailable) {
      try {
        // Use Tauri fs API when available
        // Note: Requires @tauri-apps/plugin-fs to be installed
        const { readTextFile } = await import('@tauri-apps/plugin-fs');
        const { appDataDir } = await import('@tauri-apps/api/path');
        const appDataDirPath = await appDataDir();
        return await readTextFile(`${appDataDirPath}/${key}.json`);
      } catch (error) {
        // File doesn't exist or error reading
        // Fallback to localStorage
        return this.fallbackAdapter.load(key);
      }
    } else {
      // Use localStorage when Tauri is not available
      return this.fallbackAdapter.load(key);
    }
  }

  /**
   * Remove data from storage
   */
  async remove(key: string): Promise<void> {
    if (this.tauriAvailable) {
      try {
        // Note: Requires @tauri-apps/plugin-fs to be installed
        const { remove } = await import('@tauri-apps/plugin-fs');
        const { appDataDir } = await import('@tauri-apps/api/path');
        const appDataDirPath = await appDataDir();
        await remove(`${appDataDirPath}/${key}.json`);
      } catch (error) {
        // Fallback to localStorage
        this.fallbackAdapter.remove(key);
      }
    } else {
      this.fallbackAdapter.remove(key);
    }
  }

  /**
   * Clear all data from storage
   */
  async clear(): Promise<void> {
    // Clear localStorage
    this.fallbackAdapter.clear();

    // Note: Tauri file system clear would require listing all files
    // which is more complex, so we only clear localStorage
  }
}

/**
 * LocalStorage adapter (fallback)
 */
class LocalStorageAdapter implements StorageAdapter {
  save(key: string, data: string): void {
    localStorage.setItem(key, data);
  }

  load(key: string): string | null {
    return localStorage.getItem(key);
  }

  remove(key: string): void {
    localStorage.removeItem(key);
  }

  clear(): void {
    localStorage.clear();
  }
}