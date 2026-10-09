// src/window-system/persistence/TauriStorageAdapter.ts

import { StorageAdapter } from './StorageAdapter';
import { createLogger } from '../../utils/logger';

const log = createLogger('TauriStorageAdapter');

/**
 * Tauri Store 存储适配器
 * 通过 @tauri-apps/plugin-store 实现 Rust 侧管理的持久化存储
 * 不再提供 localStorage 降级 — 所有持久化存储必须由 Rust 管理
 */
export class TauriStorageAdapter implements StorageAdapter {
  private storePath: string;

  constructor(storePath: string = 'windows.bin') {
    this.storePath = storePath;
  }

  /**
   * 懒加载 Tauri Store 实例
   */
  private async getStore() {
    const { load } = await import('@tauri-apps/plugin-store');
    return await load(this.storePath);
  }

  /**
   * Save data to Tauri Store
   */
  async save(key: string, data: string): Promise<void> {
    try {
      const store = await this.getStore();
      await store.set(key, JSON.parse(data));
      await store.save();
    } catch (error) {
      log.error('Failed to save to Tauri Store:', error);
    }
  }

  /**
   * Load data from Tauri Store
   */
  async load(key: string): Promise<string | null> {
    try {
      const store = await this.getStore();
      const value = await store.get<string>(key);
      return value !== null && value !== undefined ? JSON.stringify(value) : null;
    } catch (error) {
      log.error('Failed to load from Tauri Store:', error);
      return null;
    }
  }

  /**
   * Remove data from Tauri Store
   */
  async remove(key: string): Promise<void> {
    try {
      const store = await this.getStore();
      await store.delete(key);
      await store.save();
    } catch (error) {
      log.error('Failed to remove from Tauri Store:', error);
    }
  }

  /**
   * Clear all data from Tauri Store
   */
  async clear(): Promise<void> {
    try {
      const store = await this.getStore();
      await store.clear();
      await store.save();
    } catch (error) {
      log.error('Failed to clear Tauri Store:', error);
    }
  }
}
