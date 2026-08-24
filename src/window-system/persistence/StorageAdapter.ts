// src/window-system/persistence/StorageAdapter.ts

/**
 * Storage adapter interface
 * Allows different storage backends (Tauri Store, etc.)
 */
export interface StorageAdapter {
  /**
   * Save data to storage
   */
  save(key: string, data: string): Promise<void> | void;

  /**
   * Load data from storage
   * Returns null if key doesn't exist
   */
  load(key: string): Promise<string | null> | string | null;

  /**
   * Remove data from storage
   */
  remove(key: string): Promise<void> | void;

  /**
   * Clear all data from storage
   */
  clear(): Promise<void> | void;
}