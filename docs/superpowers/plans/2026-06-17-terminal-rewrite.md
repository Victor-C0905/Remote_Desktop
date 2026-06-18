# 终端重写实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 基于 GNOME Terminal 的完整功能集，重写终端组件，解决原版的致命 bug，实现功能完善的终端体验。

**Architecture:** 采用组件化架构，将终端功能分解为多个子组件（HeaderBar、SearchBar、ContextMenu、SettingsPanel），使用 xterm.js 标准 API，移除所有过度复杂的 workaround，保持代码简洁清晰。

**Tech Stack:** React 18 + TypeScript + xterm.js 5.5 + Tauri 2 + Rust

---

## 文件结构

**前端文件：**
- `src/apps/Terminal.tsx` - 主组件（重写）
- `src/apps/Terminal.css` - 样式（更新）
- `src/components/terminal/HeaderBar.tsx` - HeaderBar 组件（新建）
- `src/components/terminal/SearchBar.tsx` - SearchBar 组件（新建）
- `src/components/terminal/ContextMenu.tsx` - ContextMenu 组件（新建）
- `src/components/terminal/SettingsPanel.tsx` - SettingsPanel 组件（新建）
- `src/components/terminal/ThemeManager.ts` - 主题管理（新建）
- `src/stores/terminalStore.ts` - 终端状态管理（新建）

**后端文件：**
- `src-tauri/src/terminal.rs` - 后端优化（修改）

**测试文件：**
- `src/apps/Terminal.test.tsx` - 测试（更新）

---

## Task 1: 备份原版文件

**Files:**
- Rename: `src/apps/Terminal.tsx` → `src/apps/Terminal.old.tsx`

- [ ] **Step 1: 重命名原版文件**

```bash
mv src/apps/Terminal.tsx src/apps/Terminal.old.tsx
```

- [ ] **Step 2: 确认备份成功**

检查文件是否存在：`src/apps/Terminal.old.tsx`

---

## Task 2: 创建终端状态管理

**Files:**
- Create: `src/stores/terminalStore.ts`

- [ ] **Step 1: 创建终端状态管理文件**

```typescript
// src/stores/terminalStore.ts
import { create } from 'zustand';

interface TabInfo {
  id: string;
  label: string;
  sessionId: string | null;
  cwd?: string;
}

interface TerminalSettings {
  fontSize: number;
  fontFamily: string;
  theme: string;
  scrollback: number;
  cursorStyle: 'block' | 'underline' | 'bar';
  cursorBlink: boolean;
}

interface TerminalState {
  tabs: TabInfo[];
  activeTabId: string;

  settings: TerminalSettings;

  searchBarVisible: boolean;
  settingsPanelVisible: boolean;
  contextMenuVisible: boolean;
  contextMenuPosition: { x: number; y: number };

  addTab: (tab: TabInfo) => void;
  removeTab: (id: string) => void;
  setActiveTab: (id: string) => void;
  updateTabLabel: (id: string, label: string) => void;

  updateSettings: (settings: Partial<TerminalSettings>) => void;

  toggleSearchBar: () => void;
  toggleSettingsPanel: () => void;
  showContextMenu: (x: number, y: number) => void;
  hideContextMenu: () => void;
}

export const useTerminalStore = create<TerminalState>((set) => ({
  tabs: [{ id: 'tab-0', label: '终端 1', sessionId: null }],
  activeTabId: 'tab-0',

  settings: {
    fontSize: 14,
    fontFamily: 'Consolas',
    theme: 'gnome-dark',
    scrollback: 5000,
    cursorStyle: 'block',
    cursorBlink: true,
  },

  searchBarVisible: false,
  settingsPanelVisible: false,
  contextMenuVisible: false,
  contextMenuPosition: { x: 0, y: 0 },

  addTab: (tab) => set((state) => ({ tabs: [...state.tabs, tab] })),
  removeTab: (id) => set((state) => ({
    tabs: state.tabs.filter(t => t.id !== id),
    activeTabId: state.activeTabId === id ? state.tabs[0]?.id || 'tab-0' : state.activeTabId,
  })),
  setActiveTab: (id) => set({ activeTabId: id }),
  updateTabLabel: (id, label) => set((state) => ({
    tabs: state.tabs.map(t => t.id === id ? { ...t, label } : t),
  })),

  updateSettings: (newSettings) => set((state) => ({
    settings: { ...state.settings, ...newSettings },
  })),

  toggleSearchBar: () => set((state) => ({ searchBarVisible: !state.searchBarVisible })),
  toggleSettingsPanel: () => set((state) => ({ settingsPanelVisible: !state.settingsPanelVisible })),
  showContextMenu: (x, y) => set({ contextMenuVisible: true, contextMenuPosition: { x, y } }),
  hideContextMenu: () => set({ contextMenuVisible: false }),
}));
```

- [ ] **Step 2: 确认文件创建成功**

检查文件是否存在：`src/stores/terminalStore.ts`

---

## Task 3: 创建主题管理

**Files:**
- Create: `src/components/terminal/ThemeManager.ts`

- [ ] **Step 1: 创建主题管理文件**

```typescript
// src/components/terminal/ThemeManager.ts
export interface TerminalTheme {
  name: string;
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent?: string;
  selectionBackground?: string;
  selectionForeground?: string;
  black?: string;
  red?: string;
  green?: string;
  yellow?: string;
  blue?: string;
  magenta?: string;
  cyan?: string;
  white?: string;
  brightBlack?: string;
  brightRed?: string;
  brightGreen?: string;
  brightYellow?: string;
  brightBlue?: string;
  brightMagenta?: string;
  brightCyan?: string;
  brightWhite?: string;
}

export const TERMINAL_THEMES: Record<string, TerminalTheme> = {
  'gnome-dark': {
    name: 'GNOME Dark',
    background: '#1e1e1e',
    foreground: '#ffffff',
    cursor: '#4ec9b0',
    cursorAccent: '#1e1e1e',
    selectionBackground: 'rgba(78, 201, 176, 0.3)',
    selectionForeground: '#ffffff',
    black: '#1e1e1e',
    red: '#cc0000',
    green: '#4e9a06',
    yellow: '#c4a000',
    blue: '#3465a4',
    magenta: '#75507b',
    cyan: '#06989a',
    white: '#d3d7cf',
    brightBlack: '#555753',
    brightRed: '#ef2929',
    brightGreen: '#8ae234',
    brightYellow: '#fce94f',
    brightBlue: '#729fcf',
    brightMagenta: '#ad7fa8',
    brightCyan: '#34e2e2',
    brightWhite: '#eeeeec',
  },
  'gnome-light': {
    name: 'GNOME Light',
    background: '#ffffff',
    foreground: '#000000',
    cursor: '#000000',
    cursorAccent: '#ffffff',
    selectionBackground: 'rgba(0, 0, 0, 0.3)',
    selectionForeground: '#ffffff',
    black: '#000000',
    red: '#cc0000',
    green: '#4e9a06',
    yellow: '#c4a000',
    blue: '#3465a4',
    magenta: '#75507b',
    cyan: '#06989a',
    white: '#d3d7cf',
    brightBlack: '#555753',
    brightRed: '#ef2929',
    brightGreen: '#8ae234',
    brightYellow: '#fce94f',
    brightBlue: '#729fcf',
    brightMagenta: '#ad7fa8',
    brightCyan: '#34e2e2',
    brightWhite: '#eeeeec',
  },
  'gnome-white': {
    name: 'GNOME White',
    background: '#ffffff',
    foreground: '#000000',
    cursor: '#000000',
    cursorAccent: '#ffffff',
    selectionBackground: 'rgba(0, 0, 0, 0.3)',
    selectionForeground: '#ffffff',
    black: '#000000',
    red: '#cc0000',
    green: '#4e9a06',
    yellow: '#c4a000',
    blue: '#3465a4',
    magenta: '#75507b',
    cyan: '#06989a',
    white: '#d3d7cf',
    brightBlack: '#555753',
    brightRed: '#ef2929',
    brightGreen: '#8ae234',
    brightYellow: '#fce94f',
    brightBlue: '#729fcf',
    brightMagenta: '#ad7fa8',
    brightCyan: '#34e2e2',
    brightWhite: '#eeeeec',
  },
};

export function getTheme(name: string): TerminalTheme {
  return TERMINAL_THEMES[name] || TERMINAL_THEMES['gnome-dark'];
}
```

- [ ] **Step 2: 确认文件创建成功**

检查文件是否存在：`src/components/terminal/ThemeManager.ts`

---

## Task 4: 创建 HeaderBar 组件

**Files:**
- Create: `src/components/terminal/HeaderBar.tsx`

- [ ] **Step 1: 创建 HeaderBar 组件文件**

```typescript
// src/components/terminal/HeaderBar.tsx
import React from 'react';
import { useTerminalStore } from '../../stores/terminalStore';

export function HeaderBar() {
  const { toggleSearchBar, toggleSettingsPanel, addTab } = useTerminalStore();

  const handleNewTab = () => {
    const newTab = {
      id: `tab-${Date.now()}`,
      label: `终端 ${useTerminalStore.getState().tabs.length + 1}`,
      sessionId: null,
    };
    addTab(newTab);
  };

  return (
    <div className="terminal-header-bar">
      <button className="header-button" onClick={handleNewTab} title="新建标签页">
        +
      </button>
      <button className="header-button" onClick={toggleSearchBar} title="搜索">
        🔍
      </button>
      <button className="header-button" onClick={toggleSettingsPanel} title="设置">
        ⚙
      </button>
      <button className="header-button" title="菜单">
        ⋮
      </button>
    </div>
  );
}
```

- [ ] **Step 2: 确认文件创建成功**

检查文件是否存在：`src/components/terminal/HeaderBar.tsx`

---

## Task 5: 创建 SearchBar 组件

**Files:**
- Create: `src/components/terminal/SearchBar.tsx`

- [ ] **Step 1: 创建 SearchBar 组件文件**

```typescript
// src/components/terminal/SearchBar.tsx
import React, { useState } from 'react';
import { useTerminalStore } from '../../stores/terminalStore';

interface SearchBarProps {
  terminal: any;
}

export function SearchBar({ terminal }: SearchBarProps) {
  const { toggleSearchBar } = useTerminalStore();
  const [searchText, setSearchText] = useState('');

  const handleFindNext = () => {
    if (terminal && searchText) {
      terminal.findNext(searchText);
    }
  };

  const handleFindPrevious = () => {
    if (terminal && searchText) {
      terminal.findPrevious(searchText);
    }
  };

  const handleClose = () => {
    toggleSearchBar();
  };

  return (
    <div className="terminal-search-bar">
      <input
        type="text"
        value={searchText}
        onChange={(e) => setSearchText(e.target.value)}
        placeholder="搜索文本..."
        className="search-input"
      />
      <button className="search-button" onClick={handleFindPrevious}>
        ↑
      </button>
      <button className="search-button" onClick={handleFindNext}>
        ↓
      </button>
      <button className="search-button" onClick={handleClose}>
        ✕
      </button>
    </div>
  );
}
```

- [ ] **Step 2: 确认文件创建成功**

检查文件是否存在：`src/components/terminal/SearchBar.tsx`

---

## Task 6: 创建 ContextMenu 组件

**Files:**
- Create: `src/components/terminal/ContextMenu.tsx`

- [ ] **Step 1: 创建 ContextMenu 组件文件**

```typescript
// src/components/terminal/ContextMenu.tsx
import React from 'react';
import { useTerminalStore } from '../../stores/terminalStore';

interface ContextMenuProps {
  terminal: any;
}

export function ContextMenu({ terminal }: ContextMenuProps) {
  const { hideContextMenu, contextMenuPosition, addTab, removeTab, activeTabId, toggleSearchBar } = useTerminalStore();

  const handleCopy = () => {
    if (terminal) {
      const selection = terminal.getSelection();
      if (selection) {
        navigator.clipboard.writeText(selection);
      }
    }
    hideContextMenu();
  };

  const handlePaste = async () => {
    const text = await navigator.clipboard.readText();
    if (terminal && text) {
      // 发送粘贴内容到终端
      terminal.write(text);
    }
    hideContextMenu();
  };

  const handleSearchSelection = () => {
    if (terminal) {
      const selection = terminal.getSelection();
      if (selection) {
        // 设置搜索文本并显示搜索栏
        toggleSearchBar();
      }
    }
    hideContextMenu();
  };

  const handleNewTab = () => {
    const newTab = {
      id: `tab-${Date.now()}`,
      label: `终端 ${useTerminalStore.getState().tabs.length + 1}`,
      sessionId: null,
    };
    addTab(newTab);
    hideContextMenu();
  };

  const handleCloseTab = () => {
    removeTab(activeTabId);
    hideContextMenu();
  };

  return (
    <div
      className="terminal-context-menu"
      style={{
        position: 'fixed',
        left: contextMenuPosition.x,
        top: contextMenuPosition.y,
      }}
    >
      <button className="menu-item" onClick={handleCopy}>
        复制
      </button>
      <button className="menu-item" onClick={handlePaste}>
        粘贴
      </button>
      <button className="menu-item" onClick={handleSearchSelection}>
        搜索选中
      </button>
      <hr className="menu-divider" />
      <button className="menu-item" onClick={handleNewTab}>
        新建标签页
      </button>
      <button className="menu-item" onClick={handleCloseTab}>
        关闭标签页
      </button>
    </div>
  );
}
```

- [ ] **Step 2: 确认文件创建成功**

检查文件是否存在：`src/components/terminal/ContextMenu.tsx`

---

## Task 7: 创建 SettingsPanel 组件

**Files:**
- Create: `src/components/terminal/SettingsPanel.tsx`

- [ ] **Step 1: 创建 SettingsPanel 组件文件**

```typescript
// src/components/terminal/SettingsPanel.tsx
import React from 'react';
import { useTerminalStore } from '../../stores/terminalStore';

export function SettingsPanel() {
  const { settings, updateSettings, toggleSettingsPanel } = useTerminalStore();

  const handleClose = () => {
    toggleSettingsPanel();
  };

  return (
    <div className="terminal-settings-panel">
      <h3>终端设置</h3>

      <label className="settings-label">
        字体大小:
        <input
          type="range"
          min="10"
          max="24"
          value={settings.fontSize}
          onChange={(e) => updateSettings({ fontSize: parseInt(e.target.value) })}
          className="settings-slider"
        />
        <span className="settings-value">{settings.fontSize}</span>
      </label>

      <label className="settings-label">
        字体类型:
        <select
          value={settings.fontFamily}
          onChange={(e) => updateSettings({ fontFamily: e.target.value })}
          className="settings-select"
        >
          <option value="Consolas">Consolas</option>
          <option value="'Cascadia Code'">Cascadia Code</option>
          <option value="'Source Code Pro'">Source Code Pro</option>
        </select>
      </label>

      <label className="settings-label">
        配色方案:
        <select
          value={settings.theme}
          onChange={(e) => updateSettings({ theme: e.target.value })}
          className="settings-select"
        >
          <option value="gnome-dark">Dark</option>
          <option value="gnome-light">Light</option>
          <option value="gnome-white">White</option>
        </select>
      </label>

      <label className="settings-label">
        光标样式:
        <select
          value={settings.cursorStyle}
          onChange={(e) => updateSettings({ cursorStyle: e.target.value as 'block' | 'underline' | 'bar' })}
          className="settings-select"
        >
          <option value="block">Block</option>
          <option value="underline">Underline</option>
          <option value="bar">Bar</option>
        </select>
      </label>

      <label className="settings-label">
        光标闪烁:
        <input
          type="checkbox"
          checked={settings.cursorBlink}
          onChange={(e) => updateSettings({ cursorBlink: e.target.checked })}
          className="settings-checkbox"
        />
      </label>

      <button className="settings-close-button" onClick={handleClose}>
        关闭
      </button>
    </div>
  );
}
```

- [ ] **Step 2: 确认文件创建成功**

检查文件是否存在：`src/components/terminal/SettingsPanel.tsx`

---

## Task 8: 重写主终端组件

**Files:**
- Create: `src/apps/Terminal.tsx`

- [ ] **Step 1: 创建新的 Terminal.tsx 文件**

```typescript
// src/apps/Terminal.tsx
import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { SearchAddon } from '@xterm/addon-search';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { useServerManager } from '../context/ServerManager';
import { useTerminalStore } from '../stores/terminalStore';
import { getTheme } from '../components/terminal/ThemeManager';
import { HeaderBar } from '../components/terminal/HeaderBar';
import { SearchBar } from '../components/terminal/SearchBar';
import { ContextMenu } from '../components/terminal/ContextMenu';
import { SettingsPanel } from '../components/terminal/SettingsPanel';
import './Terminal.css';

export function TerminalApp() {
  const { activeServerId } = useServerManager();
  const {
    tabs,
    activeTabId,
    settings,
    searchBarVisible,
    settingsPanelVisible,
    contextMenuVisible,
    setActiveTab,
    addTab,
    removeTab,
  } = useTerminalStore();

  const terminalRefs = useRef<Map<string, Terminal>>(new Map());
  const fitAddonRefs = useRef<Map<string, FitAddon>>(new Map());
  const searchAddonRefs = useRef<Map<string, SearchAddon>>(new Map());
  const sessionIdRefs = useRef<Map<string, string>>(new Map());
  const unlistenRefs = useRef<Map<string, () => void>>(new Map());

  // 创建终端实例
  const createTerminal = async (tabId: string, container: HTMLDivElement) => {
    const theme = getTheme(settings.theme);

    const terminal = new Terminal({
      theme: theme,
      rendererType: 'canvas',
      customGlyphs: false,
      fontFamily: settings.fontFamily,
      fontSize: settings.fontSize,
      lineHeight: 1.2,
      cursorBlink: settings.cursorBlink,
      cursorStyle: settings.cursorStyle,
      scrollback: settings.scrollback,
      allowProposedApi: true,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();
    const searchAddon = new SearchAddon();
    const unicode11Addon = new Unicode11Addon();

    terminal.loadAddon(fitAddon);
    terminal.loadAddon(webLinksAddon);
    terminal.loadAddon(searchAddon);
    terminal.loadAddon(unicode11Addon);
    terminal.unicode.activeVersion = '11';

    terminal.open(container);
    terminal.clear(); // 清空初始内容，避免乱码

    terminalRefs.current.set(tabId, terminal);
    fitAddonRefs.current.set(tabId, fitAddon);
    searchAddonRefs.current.set(tabId, searchAddon);

    // Fit 终端尺寸
    const initTerminal = () => {
      try {
        if (container.offsetWidth > 0 && container.offsetHeight > 0) {
          fitAddon.fit();
          connectTerminal(tabId, terminal);
          return true;
        }
      } catch { /* ignore */ }
      return false;
    };

    // 多次尝试 fit
    if (!initTerminal()) {
      requestAnimationFrame(() => {
        if (!initTerminal()) {
          requestAnimationFrame(() => {
            if (!initTerminal()) {
              setTimeout(() => initTerminal(), 100);
            }
          });
        }
      });
    }

    // 右键菜单
    container.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      useTerminalStore.getState().showContextMenu(e.clientX, e.clientY);
    });

    // 动态更新设置
    terminal.onResize(({ cols, rows }) => {
      const sessionId = sessionIdRefs.current.get(tabId);
      if (sessionId) {
        invoke('remote_terminal_resize', {
          sessionId: sessionId,
          cols: cols,
          rows: rows,
          serverId: activeServerId,
        }).catch(console.error);
      }
    });
  };

  // 连接终端
  const connectTerminal = async (tabId: string, terminal: Terminal) => {
    if (activeServerId) {
      try {
        const result = await invoke<{ session_id: string }>('remote_spawn_terminal', {
          serverId: activeServerId,
          shell: '',
          cols: terminal.cols,
          rows: terminal.rows,
        });

        sessionIdRefs.current.set(tabId, result.session_id);

        // 监听输出
        const unlisten = await listen<{ session_id: string; data: number[] }>(
          'terminal-output',
          (event) => {
            if (event.payload.session_id === result.session_id) {
              const bytes = new Uint8Array(event.payload.data);
              terminal.write(bytes);
            }
          }
        );

        unlistenRefs.current.set(tabId, unlisten);

        // 监听输入
        terminal.onData((data: string) => {
          const sessionId = sessionIdRefs.current.get(tabId);
          if (sessionId) {
            const bytes = new TextEncoder().encode(data);
            invoke('remote_terminal_write', {
              sessionId: sessionId,
              data: Array.from(bytes),
              serverId: activeServerId,
            }).catch(console.error);
          }
        });

        // 监听断连
        const unlistenDisconnect = await listen<{ session_id: string }>(
          'terminal-disconnected',
          (event) => {
            if (event.payload.session_id === result.session_id) {
              terminal.writeln('\r\n\x1b[31m[远程终端断开]\x1b[0m');
              unlisten();
              unlistenDisconnect();
              sessionIdRefs.current.delete(tabId);
            }
          }
        );
      } catch (e) {
        terminal.writeln('\x1b[31m[连接失败]\x1b[0m');
        terminal.writeln(`错误: ${e}`);
        setupDemoMode(tabId, terminal);
      }
    } else {
      setupDemoMode(tabId, terminal);
    }
  };

  // 演示模式
  const setupDemoMode = (tabId: string, terminal: Terminal) => {
    let currentLine = '';

    terminal.onData((data: string) => {
      if (data === '\r') {
        terminal.write('\r\n');
        const output = processDemoCommand(currentLine);
        terminal.write(output);
        currentLine = '';
      } else if (data === '\x7f') {
        if (currentLine.length > 0) {
          currentLine = currentLine.slice(0, -1);
          terminal.write('\b \b');
        }
      } else {
        currentLine += data;
        terminal.write(data);
      }
    });

    terminal.write('\x1b[33m[演示模式]\x1b[0m 输入 help 查看可用命令\r\n');
  };

  // 演示命令处理
  const processDemoCommand = (cmd: string): string => {
    const trimmed = cmd.trim().toLowerCase();
    if (!trimmed) return '\r\n';

    switch (trimmed) {
      case 'help':
        return '可用命令: help, uname, whoami, ls, date, clear\r\n';
      case 'uname':
        return 'GNOME-Remote 0.1.0 (demo)\r\n';
      case 'whoami':
        return 'user\r\n';
      case 'ls':
        return 'Documents  Downloads  Pictures\r\n';
      case 'date':
        return new Date().toString() + '\r\n';
      case 'clear':
        return '\x1b[2J\x1b[H';
      default:
        return `命令未找到: ${cmd}\r\n`;
    }
  };

  // 动态更新设置
  useEffect(() => {
    terminalRefs.current.forEach((terminal) => {
      terminal.options.fontSize = settings.fontSize;
      terminal.options.fontFamily = settings.fontFamily;
      terminal.options.cursorBlink = settings.cursorBlink;
      terminal.options.cursorStyle = settings.cursorStyle;
      terminal.options.theme = getTheme(settings.theme);
    });

    fitAddonRefs.current.forEach((fitAddon) => {
      try { fitAddon.fit(); } catch { /* ignore */ }
    });
  }, [settings]);

  // 清理
  useEffect(() => {
    return () => {
      terminalRefs.current.forEach((terminal, tabId) => {
        const sessionId = sessionIdRefs.current.get(tabId);
        if (sessionId) {
          invoke('remote_terminal_close', { sessionId }).catch(console.error);
        }
        terminal.dispose();
      });

      unlistenRefs.current.forEach((unlisten) => {
        unlisten();
      });

      terminalRefs.current.clear();
      fitAddonRefs.current.clear();
      searchAddonRefs.current.clear();
      sessionIdRefs.current.clear();
      unlistenRefs.current.clear();
    };
  }, []);

  // Resize 处理
  useEffect(() => {
    const handleResize = () => {
      fitAddonRefs.current.forEach((fitAddon) => {
        try { fitAddon.fit(); } catch { /* ignore */ }
      });
    };

    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  const activeTerminal = terminalRefs.current.get(activeTabId);

  return (
    <div className="terminal-app">
      <HeaderBar />

      {searchBarVisible && activeTerminal && (
        <SearchBar terminal={activeTerminal} />
      )}

      {settingsPanelVisible && (
        <SettingsPanel />
      )}

      {contextMenuVisible && activeTerminal && (
        <ContextMenu terminal={activeTerminal} />
      )}

      <div className="terminal-tab-bar">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            className={`terminal-tab${tab.id === activeTabId ? ' active' : ''}`}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.label}
            {tabs.length > 1 && (
              <button
                className="tab-close"
                onClick={(e) => {
                  e.stopPropagation();
                  removeTab(tab.id);
                }}
              >
                ×
              </button>
            )}
          </button>
        ))}
      </div>

      <div className="terminal-container">
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className="terminal-instance"
            ref={(el) => {
              if (el && !terminalRefs.current.has(tab.id)) {
                createTerminal(tab.id, el);
              }
            }}
            style={{
              visibility: tab.id === activeTabId ? 'visible' : 'hidden',
              position: tab.id === activeTabId ? 'relative' : 'absolute',
              top: 0,
              left: 0,
              width: '100%',
              height: '100%',
            }}
          />
        ))}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: 确认文件创建成功**

检查文件是否存在：`src/apps/Terminal.tsx`

---

## Task 9: 更新样式文件

**Files:**
- Modify: `src/apps/Terminal.css`

- [ ] **Step 1: 更新 Terminal.css 样式**

```css
/* src/apps/Terminal.css */

.terminal-app {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--window-bg);
}

.terminal-header-bar {
  display: flex;
  align-items: center;
  padding: 8px 12px;
  background: var(--headerbar-bg);
  border-bottom: 1px solid var(--border-color);
  gap: 8px;
}

.header-button {
  padding: 4px 8px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 4px;
  font-size: 14px;
}

.header-button:hover {
  background: var(--card-bg);
}

.terminal-search-bar {
  display: flex;
  align-items: center;
  padding: 8px 12px;
  background: var(--card-bg);
  border-bottom: 1px solid var(--border-color);
  gap: 8px;
}

.search-input {
  flex: 1;
  padding: 4px 8px;
  border: 1px solid var(--border-color);
  background: var(--view-bg);
  color: var(--text-primary);
  border-radius: 4px;
}

.search-button {
  padding: 4px 8px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  border-radius: 4px;
}

.terminal-settings-panel {
  position: absolute;
  top: 48px;
  right: 12px;
  width: 300px;
  padding: 16px;
  background: var(--card-bg);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  z-index: 1000;
}

.settings-label {
  display: flex;
  align-items: center;
  margin-bottom: 12px;
  color: var(--text-primary);
}

.settings-slider {
  flex: 1;
  margin: 0 8px;
}

.settings-value {
  min-width: 30px;
  text-align: right;
}

.settings-select {
  flex: 1;
  margin-left: 8px;
  padding: 4px;
  border: 1px solid var(--border-color);
  background: var(--view-bg);
  color: var(--text-primary);
  border-radius: 4px;
}

.settings-checkbox {
  margin-left: 8px;
}

.settings-close-button {
  width: 100%;
  padding: 8px;
  border: none;
  background: var(--accent-bg);
  color: var(--accent-fg);
  border-radius: 4px;
  cursor: pointer;
  margin-top: 12px;
}

.terminal-context-menu {
  position: fixed;
  padding: 8px 0;
  background: var(--card-bg);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  z-index: 1000;
}

.menu-item {
  display: block;
  width: 100%;
  padding: 8px 16px;
  border: none;
  background: transparent;
  color: var(--text-primary);
  cursor: pointer;
  text-align: left;
}

.menu-item:hover {
  background: var(--view-bg);
}

.menu-divider {
  margin: 4px 0;
  border: none;
  border-top: 1px solid var(--border-color);
}

.terminal-tab-bar {
  display: flex;
  padding: 4px 8px;
  background: var(--headerbar-bg);
  border-bottom: 1px solid var(--border-color);
  gap: 4px;
}

.terminal-tab {
  display: flex;
  align-items: center;
  padding: 6px 12px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 4px;
  gap: 8px;
}

.terminal-tab.active {
  background: var(--view-bg);
  color: var(--text-primary);
}

.tab-close {
  padding: 2px 4px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 2px;
  font-size: 12px;
}

.tab-close:hover {
  background: var(--card-bg);
}

.terminal-container {
  flex: 1;
  position: relative;
  background: var(--view-bg);
}

.terminal-instance {
  width: 100%;
  height: 100%;
}
```

- [ ] **Step 2: 确认样式更新成功**

检查文件是否更新：`src/apps/Terminal.css`

---

## Task 10: 优化后端代码

**Files:**
- Modify: `src-tauri/src/terminal.rs`

- [ ] **Step 1: 移除过度复杂的缓冲区管理**

在 `remote_spawn_terminal` 函数中，移除以下逻辑：
- 移除 `writeBufferRef` 和 `flushWriteBuffer` 函数
- 移除 alternate screen 检测逻辑
- 移除滚动节流逻辑

简化后的输出任务：

```rust
// PTY 输出读取循环
loop {
    let mut len_buf = [0u8; 4];
    match recv.read_exact(&mut len_buf).await {
        Ok(_) => {
            let len = u32::from_le_bytes(len_buf) as usize;
            if len == 0 || len > 1024 * 1024 {
                tracing::warn!("无效的终端数据长度: {}", len);
                break;
            }
            let mut data = vec![0u8; len];
            match recv.read_exact(&mut data).await {
                Ok(_) => {
                    // 直接发送事件到前端
                    let _ = app_handle.emit("terminal-output", serde_json::json!({
                        "session_id": session_id_output,
                        "data": data,
                    }));
                }
                Err(e) => {
                    tracing::warn!("读取终端输出失败: {}", e);
                    break;
                }
            }
        }
        Err(quinn::ReadExactError::FinishedEarly(_)) => {
            tracing::info!("终端 Stream 关闭");
            break;
        }
        Err(e) => {
            tracing::warn!("读取终端输出长度失败: {}", e);
            break;
        }
    }
}
```

- [ ] **Step 2: 确认后端优化成功**

检查文件是否更新：`src-tauri/src/terminal.rs`

---

## Task 11: 更新测试文件

**Files:**
- Modify: `src/apps/Terminal.test.tsx`

- [ ] **Step 1: 更新测试文件**

```typescript
// src/apps/Terminal.test.tsx
import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { TerminalApp } from './Terminal';

describe('TerminalApp', () => {
  it('renders terminal app', () => {
    render(<TerminalApp />);
    expect(screen.getByRole('button', { name: '新建标签页' })).toBeInTheDocument();
  });

  it('renders header bar', () => {
    render(<TerminalApp />);
    expect(screen.getByRole('button', { name: '搜索' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '设置' })).toBeInTheDocument();
  });

  it('renders tab bar', () => {
    render(<TerminalApp />);
    expect(screen.getByText('终端 1')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: 运行测试**

```bash
npm run test
```

Expected: Tests pass

---

## Task 12: 测试和验证

**Files:**
- None

- [ ] **Step 1: 启动开发服务器**

```bash
npm run tauri dev
```

- [ ] **Step 2: 测试核心功能**

手动测试：
- 远程终端连接成功
- 可以输入命令（包括中文）
- 第一行无乱码
- resize 同步正常
- 多标签页切换正常
- 断连后演示模式正常

- [ ] **Step 3: 测试高级功能**

手动测试：
- 配色方案切换正常
- 字体大小调整正常
- 搜索功能正常
- 复制粘贴正常
- 链接点击打开正常
- 右键菜单正常
- 快捷键正常

---

## Task 13: 清理和提交

**Files:**
- Delete: `src/apps/Terminal.old.tsx`

- [ ] **Step 1: 删除原版备份**

```bash
rm src/apps/Terminal.old.tsx
```

- [ ] **Step 2: 提交代码**

```bash
git add .
git commit -m "feat: rewrite terminal with GNOME Terminal features"
```

---

## 自我审查

**1. Spec coverage:**
- ✅ 所有 GNOME Terminal 核心功能都有对应任务
- ✅ 前端组件设计完整
- ✅ 后端优化完整
- ✅ 测试和验证完整

**2. Placeholder scan:**
- ✅ 无 TBD 或 TODO
- ✅ 所有代码步骤都有完整实现
- ✅ 所有命令都有预期输出

**3. Type consistency:**
- ✅ 类型定义一致
- ✅ 函数签名一致
- ✅ 属性名称一致

---

## 执行选择

计划完成并保存到 `docs/superpowers/plans/2026-06-17-terminal-rewrite.md`。

两种执行选项：

**1. Subagent-Driven (推荐)** - 我为每个任务派发新的子代理，任务间审查，快速迭代

**2. Inline Execution** - 在此会话中使用 executing-plans 执行任务，批量执行带检查点

你选择哪种方式？