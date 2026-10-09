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

  // Browser - 服务器视角网页浏览（SOCKS5 over QUIC 代理会话控制面板）
  registry.register({
    id: 'browser',
    title: '浏览器',
    icon: '🌐',
    defaultSize: { width: 420, height: 380 },
    minSize: { width: 360, height: 300 },
    allowMultipleInstances: false,
    component: BrowserApp,
    desktopLabel: '远程浏览',
    dockLabel: '浏览',
  });

  // Image Viewer - 远程图片查看（文件格式路由目标，非主动入口）
  registry.register({
    id: 'image-viewer',
    title: '图片查看器',
    icon: '🖼️',
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
    title: '十六进制查看器',
    icon: '🔢',
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
    title: 'PDF 阅读器',
    icon: '📄',
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
    title: '压缩包查看器',
    icon: '📦',
    defaultSize: { width: 760, height: 560 },
    minSize: { width: 420, height: 320 },
    allowMultipleInstances: true,
    component: ArchiveViewer,
    showOnDesktop: false,
    showOnDock: false,
  });

  log.info('Registered apps:', registry.getAll().map(a => a.id));
}
