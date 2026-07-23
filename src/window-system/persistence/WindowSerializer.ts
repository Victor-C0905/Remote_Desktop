// src/window-system/persistence/WindowSerializer.ts

import { PersistedWindowData } from '../types';
import { createLogger } from '../../utils/logger';

const log = createLogger('WindowSerializer');

/**
 * Window state serializer
 * Handles serialization and versioning of window states
 */

// Current version of window state format
const CURRENT_VERSION = 1;

// Storage key for window states
const STORAGE_KEY = 'gnome-remote-windows';

// Versioned data structure
interface VersionedWindowData {
  version: number;
  windows: PersistedWindowData[];
  savedAt: number;
}

export class WindowSerializer {
  /**
   * Serialize window states with version information
   */
  serialize(windows: PersistedWindowData[]): string {
    const data: VersionedWindowData = {
      version: CURRENT_VERSION,
      windows,
      savedAt: Date.now(),
    };
    return JSON.stringify(data);
  }

  /**
   * Deserialize window states
   * Handles version migration if needed
   */
  deserialize(data: string): PersistedWindowData[] {
    try {
      const parsed = JSON.parse(data) as VersionedWindowData | PersistedWindowData[];

      // Handle legacy format (array without version)
      if (Array.isArray(parsed)) {
        return this.migrateFromLegacy(parsed);
      }

      // Handle versioned format
      if (parsed.version !== CURRENT_VERSION) {
        return this.migrate(parsed);
      }

      return parsed.windows;
    } catch (error) {
      log.error('Failed to deserialize', error);
      return [];
    }
  }

  /**
   * Migrate from legacy format (version 0)
   */
  private migrateFromLegacy(data: PersistedWindowData[]): PersistedWindowData[] {
    log.info('Migrating from legacy format');
    // Legacy format is already compatible with current format
    return data.map(w => ({
      ...w,
      layout: w.layout ?? 'free', // Add missing layout field
    }));
  }

  /**
   * Migrate from older version to current version
   */
  private migrate(data: VersionedWindowData): PersistedWindowData[] {
    log.info(`Migrating from version ${data.version} to ${CURRENT_VERSION}`);

    // Version migration logic
    // Currently only version 1, so no migration needed
    return data.windows;
  }

  /**
   * Get storage key
   */
  getStorageKey(): string {
    return STORAGE_KEY;
  }

  /**
   * Get current version
   */
  getVersion(): number {
    return CURRENT_VERSION;
  }
}