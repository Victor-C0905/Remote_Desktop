// src/window-system/init.ts

import { WindowRegistry } from './core/WindowRegistry';
import { createLogger } from '../utils/logger';

// Import app components
import { TerminalApp } from '../apps/Terminal';
import { FileManager } from '../apps/FileManager';
import { SystemMonitor } from '../apps/SystemMonitor';
import { Settings } from '../apps/Settings';
import { TextEditor } from '../apps/TextEditor/TextEditor';

const log = createLogger('WindowInit');

/**
 * Initialize the window registry with all registered applications
 *
 * ⚠️ 单一数据源：此文件是应用清单的唯一真相源
 * Desktop/Dock/Overview 都从 registry.getAll() 派生显示列表
 * 新增应用只需在此注册，无需修改 Desktop.tsx
 */
export function initWindowRegistry(registry: WindowRegistry): void {
  // Terminal - allows multiple instances
  registry.register({
    id: 'terminal',
    title: '终端',
    icon: '🖥️',
    defaultSize: { width: 850, height: 550 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: true,
    component: TerminalApp,
    desktopLabel: '远程终端',
    dockLabel: '终端',
  });

  // File Manager - single instance
  registry.register({
    id: 'files',
    title: '文件管理器',
    icon: '📁',
    defaultSize: { width: 900, height: 650 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: false,
    component: FileManager,
    desktopLabel: '远程文件',
    dockLabel: '文件',
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

  // Text Editor - allows multiple instances for editing different files
  registry.register({
    id: 'editor',
    title: '文本编辑器',
    icon: '📝',
    defaultSize: { width: 900, height: 600 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: true,
    component: TextEditor,
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

  log.info('Registered apps:', registry.getAll().map(a => a.id));
}
