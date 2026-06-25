// src/window-system/init.ts

import { WindowRegistry } from './core/WindowRegistry';

// Import app components
import { TerminalApp } from '../apps/Terminal';
import { FileManager } from '../apps/FileManager';
import { SystemMonitor } from '../apps/SystemMonitor';
import { Settings } from '../apps/Settings';

/**
 * Initialize the window registry with all registered applications
 */
export function initWindowRegistry(registry: WindowRegistry): void {
  // Terminal - allows multiple instances
  registry.register({
    id: 'terminal',
    title: '终端',
    icon: '🖥️',
    defaultSize: { width: 850, height: 550 },
    minSize: { width: 600, height: 400 },  // 增加最小尺寸，防止 xterm.js 显示异常
    allowMultipleInstances: true,
    component: TerminalApp,
  });

  // File Manager - single instance, with preloading
  registry.register({
    id: 'files',
    title: '文件管理器',
    icon: '📁',
    defaultSize: { width: 900, height: 650 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: false,
    lifecycle: {
      onBeforeCreate: async (_context) => {
        // Preload file manager data using serverId
        // Note: This will be handled by Desktop.tsx passing preloadData
        return null;
      },
    },
    component: FileManager,
  });

  // System Monitor - single instance
  registry.register({
    id: 'monitor',
    title: '系统监控',
    icon: '📊',
    defaultSize: { width: 900, height: 650 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: false,
    component: SystemMonitor,
  });

  // Settings - single instance
  registry.register({
    id: 'settings',
    title: '设置',
    icon: '⚙️',
    defaultSize: { width: 700, height: 550 },
    minSize: { width: 500, height: 400 },
    allowMultipleInstances: false,
    component: Settings,
  });

  console.log('[WindowRegistry] Registered apps:', registry.getAll().map(a => a.id));
}