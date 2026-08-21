# 终端 P0 功能实现计划

> 目标：对齐 Adwaita Terminal 核心功能（复制/粘贴、搜索、快捷键）

***

## 一、当前状态分析

### 已安装的 xterm addon

| addon                    | 版本     | 使用状态  |
| ------------------------ | ------ | ----- |
| `@xterm/addon-fit`       | 0.11.0 | ✅ 已使用 |
| `@xterm/addon-web-links` | 0.12.0 | ✅ 已使用 |
| `@xterm/addon-search`    | 0.16.0 | ❌ 未使用 |
| `@xterm/addon-webgl`     | 0.19.0 | ❌ 未使用 |
| `@xterm/addon-unicode11` | 0.9.0  | ❌ 未使用 |
| `@xterm/addon-clipboard` | -      | ❌ 未安装 |

### 当前 Terminal.tsx 问题

1. `handleCopy` / `handlePaste` 函数体为空
2. `handleSearch` 函数体为空
3. 搜索 UI 存在但无实际搜索功能
4. 无快捷键系统

### 废弃文件

* `src/components/terminal/ContextMenu.tsx` - 引用已删除的 `terminalStore`

***

## 二、实现方案

### 方案决策

| 功能    | 方案选择                             | 原因                         |
| ----- | -------------------------------- | -------------------------- |
| 复制/粘贴 | 原生 `navigator.clipboard` API     | 无需额外安装包，Tauri WebView 支持   |
| 搜索功能  | `@xterm/addon-search`            | 已安装，官方 addon               |
| 快捷键   | React `useEffect` + `keydown` 事件 | 简单直接，符合 Adwaita Terminal 快捷键 |

***

## 三、分步实现

### Step 1: 实现搜索功能

**修改文件**: `src/apps/Terminal.tsx`

**改动内容**:

1. 导入 `SearchAddon`
2. 在 `TerminalInstance` 中加载 SearchAddon
3. 实现 `handleSearch` 函数调用 `searchAddon.findNext()`
4. 添加搜索结果高亮和导航（上一个/下一个）

**代码位置**:

* 导入区（第 4-13 行）

* `TerminalInstance` useEffect（第 63-171 行）

* 搜索栏 UI（第 428-441 行）

### Step 2: 实现复制/粘贴功能

**修改文件**: `src/apps/Terminal.tsx`

**改动内容**:

1. 实现 `handleCopy`: 使用 `terminal.getSelection()` + `navigator.clipboard.writeText()`
2. 实现 `handlePaste`: 使用 `navigator.clipboard.readText()` + 发送到远程 PTY
3. 需要获取当前活动 tab 的 terminal 实例（通过 ref 传递）

**技术难点**:

* `TerminalInstance` 的 terminal 实例在子组件中

* 需要通过 callback ref 或 context 将 terminal 实例传递给父组件

**解决方案**:

* 在 `TerminalApp` 中维护 `activeTerminalRef`

* `TerminalInstance` 通过 `onTerminalReady` callback 传递 terminal 实例

### Step 3: 实现快捷键系统

**修改文件**: `src/apps/Terminal.tsx`

**改动内容**:

1. 在 `TerminalApp` 中添加 `useEffect` 监听 `keydown` 事件
2. 实现以下快捷键：

| 快捷键              | 功能        | Adwaita Terminal 对应 |
| ---------------- | --------- | ----------------- |
| `Ctrl+Shift+C`   | 复制选中文本    | ✅                 |
| `Ctrl+Shift+V`   | 粘贴        | ✅                 |
| `Ctrl+Shift+F`   | 打开/关闭搜索栏  | ✅                 |
| `Ctrl+Shift+T`   | 新建标签页     | ✅                 |
| `Ctrl+Shift+W`   | 关闭当前标签页   | ✅                 |
| `Ctrl+Tab`       | 切换到下一个标签页 | ✅                 |
| `Ctrl+Shift+Tab` | 切换到上一个标签页 | ✅                 |

### Step 4: 清理废弃文件

**删除文件**:

* `src/components/terminal/ContextMenu.tsx` - 引用已删除的 terminalStore

***

## 四、具体代码改动

### 4.1 TerminalInstance Props 扩展

```typescript
interface TerminalInstanceProps {
  // ...existing props
  onTerminalReady?: (terminal: Terminal, sessionId: string | null) => void;
  searchAddonRef?: React.MutableRefObject<SearchAddon | null>;
}
```

### 4.2 搜索功能实现

```typescript
// 导入
import { SearchAddon } from '@xterm/addon-search';

// 在 TerminalInstance 中
const searchAddon = new SearchAddon();
terminal.loadAddon(searchAddon);
if (searchAddonRef) searchAddonRef.current = searchAddon;

// 搜索处理
const handleSearch = () => {
  if (searchAddonRef.current && searchText) {
    searchAddonRef.current.findNext(searchText, {
      caseSensitive: false,
      wholeWord: false,
      decorations: {
        matchBackground: '#FFD700',
        activeMatchBackground: '#FF6B6B',
      }
    });
  }
};
```

### 4.3 复制/粘贴实现

```typescript
// 需要在 TerminalApp 中维护 activeTerminalRef
const activeTerminalRef = useRef<Terminal | null>(null);
const activeSessionIdRef = useRef<string | null>(null);

// handleCopy
const handleCopy = () => {
  const terminal = activeTerminalRef.current;
  if (terminal) {
    const selection = terminal.getSelection();
    if (selection) {
      navigator.clipboard.writeText(selection);
    }
  }
  setContextMenu(null);
};

// handlePaste
const handlePaste = async () => {
  const terminal = activeTerminalRef.current;
  const sessionId = activeSessionIdRef.current;
  if (!terminal) return;

  const text = await navigator.clipboard.readText();
  if (text) {
    if (sessionId && activeServerId) {
      // 远程模式：发送到 PTY
      invoke('remote_terminal_write', {
        sessionId,
        data: Array.from(new TextEncoder().encode(text)),
        serverId: activeServerId,
      });
    } else {
      // 演示模式：直接写入
      terminal.write(text);
    }
  }
  setContextMenu(null);
};
```

### 4.4 快捷键实现

```typescript
useEffect(() => {
  const handleKeyDown = (e: KeyboardEvent) => {
    // Ctrl+Shift+C: 复制
    if (e.ctrlKey && e.shiftKey && e.key === 'C') {
      e.preventDefault();
      handleCopy();
    }
    // Ctrl+Shift+V: 粘贴
    if (e.ctrlKey && e.shiftKey && e.key === 'V') {
      e.preventDefault();
      handlePaste();
    }
    // Ctrl+Shift+F: 搜索
    if (e.ctrlKey && e.shiftKey && e.key === 'F') {
      e.preventDefault();
      setShowSearch(v => !v);
    }
    // Ctrl+Shift+T: 新建标签
    if (e.ctrlKey && e.shiftKey && e.key === 'T') {
      e.preventDefault();
      handleNewTab();
    }
    // Ctrl+Shift+W: 关闭标签
    if (e.ctrlKey && e.shiftKey && e.key === 'W') {
      e.preventDefault();
      if (activeTabId) handleCloseTab(activeTabId);
    }
    // Ctrl+Tab: 下一个标签
    if (e.ctrlKey && e.key === 'Tab' && !e.shiftKey) {
      e.preventDefault();
      const idx = tabs.findIndex(t => t.id === activeTabId);
      const nextIdx = (idx + 1) % tabs.length;
      setActiveTabId(tabs[nextIdx]?.id);
    }
    // Ctrl+Shift+Tab: 上一个标签
    if (e.ctrlKey && e.shiftKey && e.key === 'Tab') {
      e.preventDefault();
      const idx = tabs.findIndex(t => t.id === activeTabId);
      const prevIdx = (idx - 1 + tabs.length) % tabs.length;
      setActiveTabId(tabs[prevIdx]?.id);
    }
  };

  window.addEventListener('keydown', handleKeyDown);
  return () => window.removeEventListener('keydown', handleKeyDown);
}, [tabs, activeTabId, activeServerId]);
```

***

## 五、验证步骤

1. **搜索功能验证**:

   * 打开终端，输出一些文本

   * 按 Ctrl+Shift+F 打开搜索栏

   * 输入搜索词，点击"查找"

   * 确认匹配文本高亮显示

2. **复制/粘贴验证**:

   * 在终端中选择文本（鼠标拖拽）

   * 按 Ctrl+Shift+C 复制

   * 按 Ctrl+Shift+V 粘贴

   * 确认粘贴内容正确发送到远程 PTY

3. **快捷键验证**:

   * Ctrl+Shift+T 新建标签页

   * Ctrl+Shift+W 关闭标签页

   * Ctrl+Tab / Ctrl+Shift+Tab 切换标签页

***

## 六、假设与决策

### 假设

1. Tauri WebView 支持 `navigator.clipboard` API（需用户授权）
2. 远程 PTY 已正确处理粘贴内容（发送原始字节）
3. 搜索高亮样式可通过 SearchAddon options 配置

### 决策

1. 不安装 `@xterm/addon-clipboard`，使用原生 clipboard API（减少依赖）
2. 搜索使用 `@xterm/addon-search`（已安装）
3. 快捷键在 `TerminalApp` 层实现（全局生效）
4. 删除废弃的 `ContextMenu.tsx`（功能已在 Terminal.tsx 中实现）

***

## 七、风险与注意事项

1. **clipboard API 权限**: Tauri WebView 可能需要配置 clipboard 权限
2. **焦点问题**: 快捷键只在终端窗口焦点时生效
3. **搜索性能**: 大量文本时搜索可能较慢，考虑限制搜索范围

