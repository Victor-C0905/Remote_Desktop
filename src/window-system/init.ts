// src/window-system/init.ts

import { WindowRegistry } from './core/WindowRegistry';
import { createLogger } from '../utils/logger';

// Import app components
import { TerminalApp } from '../apps/Terminal';
import { FileManager } from '../apps/FileManager';
import { SystemMonitor } from '../apps/SystemMonitor';
import { Settings } from '../apps/Settings';
import { TextEditor } from '../apps/TextEditor/TextEditor';
import { BrowserApp } from '../apps/BrowserApp';
import { ImageViewer } from '../apps/ImageViewer/ImageViewer';
import { HexViewer } from '../apps/HexViewer/HexViewer';
import { PDFViewer } from '../apps/PDFViewer/PDFViewer';
import { ArchiveViewer } from '../apps/ArchiveViewer/ArchiveViewer';

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
    titleKey: 'apps.terminal.title',
    icon: 'terminal',
    defaultSize: { width: 850, height: 550 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: true,
    component: TerminalApp,
    desktopLabelKey: 'apps.terminal.desktopLabel',
    dockLabelKey: 'apps.terminal.dockLabel',
  });

  // File Manager - single instance
  registry.register({
    id: 'files',
    titleKey: 'apps.files.title',
    icon: 'files',
    defaultSize: { width: 900, height: 650 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: false,
    component: FileManager,
    desktopLabelKey: 'apps.files.desktopLabel',
    dockLabelKey: 'apps.files.dockLabel',
  });

  // System Monitor - single instance
  registry.register({
    id: 'monitor',
    titleKey: 'apps.monitor.title',
    icon: 'monitor',
    defaultSize: { width: 900, height: 650 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: false,
    component: SystemMonitor,
  });

  // Text Editor - allows multiple instances for editing different files
  registry.register({
    id: 'editor',
    titleKey: 'apps.editor.title',
    icon: 'editor',
    defaultSize: { width: 900, height: 600 },
    minSize: { width: 600, height: 400 },
    allowMultipleInstances: true,
    component: TextEditor,
  });

  // Settings - single instance
  registry.register({
    id: 'settings',
    titleKey: 'apps.settings.title',
    icon: 'settings',
    defaultSize: { width: 700, height: 550 },
    minSize: { width: 500, height: 400 },
    allowMultipleInstances: false,
    component: Settings,
  });

  // Browser - 内置浏览器（WebView2 嵌入，chrome 区 + content 区）
  registry.register({
    id: 'browser',
    titleKey: 'apps.browser.title',
    icon: 'browser',
    defaultSize: { width: 1000, height: 700 },
    minSize: { width: 640, height: 440 },
    allowMultipleInstances: false,
    component: BrowserApp,
    desktopLabelKey: 'apps.browser.desktopLabel',
    dockLabelKey: 'apps.browser.dockLabel',
  });

  // Image Viewer - 远程图片查看（文件格式路由目标，非主动入口）
  registry.register({
    id: 'image-viewer',
    titleKey: 'apps.imageViewer.title',
    icon: 'image',
    defaultSize: { width: 800, height: 600 },
    minSize: { width: 400, height: 300 },
    allowMultipleInstances: true,
    component: ImageViewer,
    showOnDesktop: false,
    showOnDock: false,
  });

  // Hex Viewer - 十六进制查看（未知格式回退，非主动入口）
  registry.register({
    id: 'hex-viewer',
    titleKey: 'apps.hexViewer.title',
    icon: 'hex',
    defaultSize: { width: 800, height: 550 },
    minSize: { width: 500, height: 350 },
    allowMultipleInstances: true,
    component: HexViewer,
    showOnDesktop: false,
    showOnDock: false,
  });

  // PDF Viewer - 远程 PDF 阅读（文件格式路由目标，非主动入口）
  registry.register({
    id: 'pdf-viewer',
    titleKey: 'apps.pdfViewer.title',
    icon: 'pdf',
    defaultSize: { width: 900, height: 700 },
    minSize: { width: 500, height: 400 },
    allowMultipleInstances: true,
    component: PDFViewer,
    showOnDesktop: false,
    showOnDock: false,
  });

  // Archive Viewer - 压缩包内容浏览（文件格式路由目标，非主动入口）
  registry.register({
    id: 'archive-viewer',
    titleKey: 'apps.archiveViewer.title',
    icon: 'archive',
    defaultSize: { width: 760, height: 560 },
    minSize: { width: 420, height: 320 },
    allowMultipleInstances: true,
    component: ArchiveViewer,
    showOnDesktop: false,
    showOnDock: false,
  });

  log.info('Registered apps:', registry.getAll().map(a => a.id));
}
