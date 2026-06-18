# 终端重写设计文档

> 版本: v2.0 | 日期: 2026-06-17
>
> 基于 GNOME Terminal 的完整功能集，重写终端组件，解决原版的无法修复的 bug，实现功能完善的终端体验。

---

## 一、背景与问题

### 1.1 原版问题

原版终端实现存在两个致命 bug：
1. **打开终端第一行乱码** - 无法确定根因，尝试多种修复方案均无效
2. **无法输入命令** - 输入处理逻辑过于复杂，导致输入无响应

### 1.2 原版复杂度

原版实现包含大量 workaround 和修复逻辑：
- 隐藏 IME input 方案（解决中文输入）
- 复杂的缓冲区管理（写入节流、alternate screen 检测）
- 大量的修复注释和 workaround
- 多种模式（远程、本地、演示）的复杂切换逻辑

### 1.3 重写目标

- 实现 GNOME Terminal 的完整功能集
- 解决原版的致命 bug
- 保留所有核心功能：远程终端、多标签页、resize、中文输入、演示模式
- 添加高级功能：配色方案、字体调整、搜索、快捷键、右键菜单等
- 确保代码简洁、可维护、功能完善

---

## 二、整体架构设计

### 2.1 核心架构分层

```
前端（React + TypeScript）          后端（Rust + Tauri）
┌─────────────────────────┐       ┌──────────────────────┐
│ TerminalApp             │       │ TerminalStreamManager │
│  ├─ TabManager          │       │  ├─ sessions HashMap  │
│  ├─ TerminalInstance[]  │       │  └─ input_tx channels │
│  └─ IMEHandler          │       │                      │
└─────────────────────────┘       │ Tauri Commands       │
                                  │  ├─ remote_spawn     │
                                  │  ├─ remote_write     │
                                  │  ├─ remote_close     │
                                  │  └─ remote_resize    │
                                  └──────────────────────┘
```

### 2.2 关键设计原则

**简化输入处理：**
- 使用 xterm.js 默认的 onData API
- xterm.js v5 已经改进了 IME 支持，现代浏览器兼容性好
- 如果后续发现 IME 问题，再添加 CompositionHelper

**简化输出处理：**
- 直接 term.write()，移除复杂的缓冲区管理
- 使用 xterm.js 内部的流控制机制
- 移除所有 workaround 和修复注释

**清晰的生命周期：**
- 创建 → 连接 → 输入输出 → resize → 关闭
- 每个步骤都有明确的错误处理

---

## 四、前端组件设计

### 4.1 TerminalApp 主组件

**职责：**
- 协调所有子组件
- 管理全局状态
- 处理全局快捷键

**核心状态：**
```typescript
interface TerminalAppState {
  tabs: TabInfo[];
  activeTabId: string;
  connectionStatus: 'connected' | 'disconnected';

  // 主题和设置
  theme: TerminalTheme;
  fontSize: number;
  fontFamily: string;

  // UI 状态
  searchBarVisible: boolean;
  settingsPanelVisible: boolean;
  contextMenuVisible: boolean;
  contextMenuPosition: { x: number; y: number };
}

interface TabInfo {
  id: string;
  label: string;
  sessionId: string | null;
  terminal: Terminal | null;
  cwd?: string;  // 工作目录
}

interface TerminalTheme {
  name: string;
  background: string;
  foreground: string;
  cursor: string;
  // ... 其他颜色配置
}
```

### 4.2 HeaderBar 组件（GNOME 风格）

**设计：**
```
┌────────────────────────────────────────────────────┐
│  [+] 标签页管理  [🔍 搜索]  [⚙ 设置]  [⋮ 菜单]    │
└────────────────────────────────────────────────────┘
```

**功能：**
- 新建标签页按钮（+）
- 搜索按钮（触发搜索栏）
- 设置按钮（打开设置面板）
- 菜单按钮（更多选项）

### 4.3 TabManager 组件

**功能：**
- 标签页列表渲染
- 标签页切换（点击）
- 标签页重命名（双击）
- 标签页拖拽排序
- 标签页关闭按钮

**交互：**
```typescript
// 标签页重命名
const handleTabRename = (tabId: string, newLabel: string) => {
  setTabs(prev => prev.map(t =>
    t.id === tabId ? { ...t, label: newLabel } : t
  ));
};

// 标签页拖拽排序
const handleTabDrag = (dragIndex: number, dropIndex: number) => {
  const newTabs = [...tabs];
  const [removed] = newTabs.splice(dragIndex, 1);
  newTabs.splice(dropIndex, 0, removed);
  setTabs(newTabs);
};
```

### 4.4 TerminalInstance 组件

**职责：**
- 创建和管理 xterm.js 实例
- 处理输入输出
- 同步 resize
- 处理链接点击
- 处理右键菜单

**生命周期流程：**
```
1. 挂载 → 创建 xterm 实例 + 加载 addons
2. 连接 → invoke remote_spawn_terminal
3. 监听 → listen terminal-output event
4. 输入 → term.onData → invoke remote_write
5. 输出 → event → term.write
6. resize → term.onResize → invoke remote_resize
7. 搜索 → SearchAddon.findNext/findPrevious
8. 右键 → contextmenu event → 显示菜单
9. 卸载 → invoke remote_terminal_close
```

### 4.5 SearchBar 组件

**设计：**
```
┌────────────────────────────────────────────────────┐
│  [🔍] 搜索文本...  [↑上一个] [↓下一个]  [✕关闭]   │
└────────────────────────────────────────────────────┘
```

**功能：**
- 搜索输入框
- 搜索结果导航
- 关闭搜索栏

**实现：**
```typescript
const SearchBar = ({ terminal, onClose }) => {
  const [searchText, setSearchText] = useState('');

  const handleSearch = () => {
    if (terminal && searchText) {
      terminal.findNext(searchText);
    }
  };

  const handleFindPrevious = () => {
    if (terminal && searchText) {
      terminal.findPrevious(searchText);
    }
  };

  return (
    <div className="search-bar">
      <input
        value={searchText}
        onChange={e => setSearchText(e.target.value)}
        placeholder="搜索文本..."
      />
      <button onClick={handleFindPrevious}>↑</button>
      <button onClick={handleSearch}>↓</button>
      <button onClick={onClose}>✕</button>
    </div>
  );
};
```

### 4.6 ContextMenu 组件

**菜单项：**
- 复制选中文本
- 粘贴文本
- 搜索选中文本
- 新建标签页
- 关闭当前标签页
- 分隔线
- 重命名标签页

**实现：**
```typescript
const ContextMenu = ({ position, onClose, terminal }) => {
  const handleCopy = () => {
    const selection = terminal.getSelection();
    navigator.clipboard.writeText(selection);
    onClose();
  };

  const handlePaste = async () => {
    const text = await navigator.clipboard.readText();
    terminal.write(text);
    onClose();
  };

  const handleSearchSelection = () => {
    const selection = terminal.getSelection();
    if (selection) {
      setSearchText(selection);
      setSearchBarVisible(true);
    }
    onClose();
  };

  return (
    <div className="context-menu" style={{ left: position.x, top: position.y }}>
      <button onClick={handleCopy}>复制</button>
      <button onClick={handlePaste}>粘贴</button>
      <button onClick={handleSearchSelection}>搜索选中</button>
      <hr />
      <button onClick={handleNewTab}>新建标签页</button>
      <button onClick={handleCloseTab}>关闭标签页</button>
    </div>
  );
};
```

### 4.7 ThemeManager 组件

**配色方案：**
```typescript
const TERMINAL_THEMES = {
  'gnome-dark': {
    background: '#1e1e1e',
    foreground: '#ffffff',
    cursor: '#4ec9b0',
    // ... GNOME Terminal dark theme
  },
  'gnome-light': {
    background: '#ffffff',
    foreground: '#000000',
    cursor: '#000000',
    // ... GNOME Terminal light theme
  },
  'gnome-white': {
    background: '#ffffff',
    foreground: '#000000',
    // ... GNOME Terminal white theme
  },
  'custom': {
    // 用户自定义主题
  },
};
```

### 4.8 SettingsPanel 组件

**设置项：**
- 字体大小（10-24）
- 字体类型（Consolas、Cascadia Code、Source Code Pro）
- 配色方案（dark/light/white/custom）
- 滚动历史大小（1000-10000）
- 光标样式（block/underline/bar）
- 光标闪烁（开/关）

**实现：**
```typescript
const SettingsPanel = ({ settings, onChange, onClose }) => {
  return (
    <div className="settings-panel">
      <h3>终端设置</h3>

      <label>
        字体大小:
        <input
          type="range"
          min="10"
          max="24"
          value={settings.fontSize}
          onChange={e => onChange({ fontSize: parseInt(e.target.value) })}
        />
        {settings.fontSize}
      </label>

      <label>
        字体类型:
        <select
          value={settings.fontFamily}
          onChange={e => onChange({ fontFamily: e.target.value })}
        >
          <option value="Consolas">Consolas</option>
          <option value="Cascadia Code">Cascadia Code</option>
          <option value="Source Code Pro">Source Code Pro</option>
        </select>
      </label>

      <label>
        配色方案:
        <select
          value={settings.theme}
          onChange={e => onChange({ theme: e.target.value })}
        >
          <option value="gnome-dark">Dark</option>
          <option value="gnome-light">Light</option>
          <option value="gnome-white">White</option>
          <option value="custom">Custom</option>
        </select>
      </label>

      <button onClick={onClose}>关闭</button>
    </div>
  );
};
```

### 4.9 xterm.js 配置（完整版）

```typescript
const terminal = new Terminal({
  // 主题配置
  theme: TERMINAL_THEMES[theme],

  // 渲染器配置
  rendererType: 'canvas',
  customGlyphs: false,

  // 字体配置
  fontFamily: fontFamily,
  fontSize: fontSize,
  lineHeight: 1.2,

  // 终端配置
  cursorBlink: true,
  cursorStyle: 'block',
  scrollback: 5000,

  // 链接配置
  allowProposedApi: true,
});

// 加载 addons
const fitAddon = new FitAddon();
const webLinksAddon = new WebLinksAddon();
const searchAddon = new SearchAddon();
const unicode11Addon = new Unicode11Addon();

terminal.loadAddon(fitAddon);
terminal.loadAddon(webLinksAddon);
terminal.loadAddon(searchAddon);
terminal.loadAddon(unicode11Addon);
terminal.unicode.activeVersion = '11';
```

### 4.10 快捷键处理

```typescript
// 全局快捷键
const GLOBAL_SHORTCUTS = {
  'Ctrl+Shift+F': 'toggleSearchBar',
  'Ctrl+Shift+C': 'copySelection',
  'Ctrl+Shift+V': 'pasteText',
  'Ctrl+Shift+N': 'newTab',
  'Ctrl+Shift+W': 'closeTab',
  'Ctrl+Tab': 'nextTab',
  'Ctrl+Shift+Tab': 'previousTab',
};

// 终端控制字符
const CONTROL_CHARS = {
  'Ctrl+C': '\x03',  // SIGINT
  'Ctrl+D': '\x04',  // EOF
  'Ctrl+Z': '\x1a',  // SIGTSTP
  'Ctrl+L': '\x0c',  // Clear screen
};
```

---

## 五、后端设计

### 5.1 TerminalStreamManager（保持现有设计）

**职责：**
- 管理活跃的终端会话
- 保存持久 Stream 的发送端

**当前实现已经很好，保持不变：**
```rust
pub struct TerminalStreamManager {
    sessions: Arc<Mutex<HashMap<String, mpsc::Sender<Vec<u8>>>>>,
}
```

### 5.2 Tauri Commands（保持现有设计）

**remote_spawn_terminal：**
- 创建持久 QUIC Stream
- 发送 TerminalSpawnRequest
- 启动双向数据隧道任务
- 返回 session_id

**remote_terminal_write：**
- 从 session map 获取 input_tx
- 发送数据到通道

**remote_terminal_close：**
- 移除 session（关闭通道）

**remote_terminal_resize：**
- 创建新 Stream 发送 resize 请求

### 5.3 数据隧道任务（优化版）

**保持核心逻辑，移除过度优化：**
- 移除复杂的缓冲区管理（使用 xterm.js 内部流控制）
- 移除 alternate screen 检测（让 xterm.js 自己处理）
- 移除滚动节流（让 xterm.js 自己处理）
- 保持直接读取 → emit → term.write

**优化后的流程：**
```rust
// 输入任务
while let Some(data) = input_rx.recv().await {
    send.write_all(&len).await?;
    send.write_all(&data).await?;
}

// 输出任务
loop {
    let len = recv.read_exact(&mut len_buf).await?;
    let data = recv.read_exact(&mut vec).await?;
    app.emit("terminal-output", json!({
        "session_id": session_id,
        "data": data,
    }));
}
```

### 5.4 错误处理（完善版）

**统一的错误处理策略：**
- 连接失败 → emit terminal-error + 显示错误消息
- Stream 断开 → emit terminal-disconnected + 清理资源
- 写入失败 → 记录日志，继续运行
- resize 失败 → 记录日志，不影响终端使用

---

## 六、数据流和错误处理

### 6.1 完整数据流

```
用户输入 → xterm.js onData → invoke remote_write → Rust channel → QUIC Stream → Agent PTY
Agent PTY 输出 → QUIC Stream → Rust emit → frontend listen → term.write → 显示
```

### 6.2 输入流（标准实现）

```typescript
// 前端：使用 xterm.js 默认 onData
terminal.onData((data: string) => {
  if (sessionId) {
    const bytes = new TextEncoder().encode(data);
    invoke('remote_terminal_write', {
      sessionId: sessionId,
      data: Array.from(bytes),
    }).catch(e => {
      console.error('[Terminal] Write failed:', e);
    });
  }
});
```

### 6.3 输出流（标准实现）

```typescript
// 前端：直接监听事件并写入
const unlisten = await listen<{ session_id: string; data: number[] }>(
  'terminal-output',
  (event) => {
    if (event.payload.session_id === sessionId) {
      const bytes = new Uint8Array(event.payload.data);
      terminal.write(bytes);
    }
  }
);
```

### 6.4 错误处理策略（完善版）

**连接阶段错误：**
```typescript
try {
  const result = await invoke('remote_spawn_terminal', { ... });
} catch (e) {
  terminal.writeln('\x1b[31m[连接失败]\x1b[0m');
  terminal.writeln(`错误: ${e}`);
  // 切换到演示模式或显示提示
}
```

**运行时错误：**
- 写入失败 → 记录日志，继续运行
- Stream 断开 → 显示断连消息，清理资源

**Resize 错误：**
- resize 失败 → 记录日志，不影响终端使用

### 6.5 资源清理（完善版）

```typescript
// 组件卸载时
useEffect(() => {
  return () => {
    // 关闭远程会话
    if (sessionId) {
      invoke('remote_terminal_close', { sessionId }).catch(console.error);
    }
    // 清理 xterm 实例
    terminal.dispose();
    // 取消事件监听
    unlisten();
  };
}, []);
```

---

## 七、演示模式和离线处理

### 7.1 离线状态检测

```typescript
// 检查连接状态
const { activeServerId } = useServerManager();

if (!activeServerId) {
  // 显示离线提示
  return <TerminalOfflineHint />;
}
```

### 7.2 演示模式（完善版）

**触发条件：**
- 无服务器连接
- 远程终端连接失败
- Stream 断开

**实现方式：**
```typescript
// 简单的本地回显
terminal.onData((data: string) => {
  if (data === '\r') {
    terminal.write('\r\n');
    const output = processDemoCommand(currentLine);
    terminal.write(output);
    currentLine = '';
  } else if (data === '\x7f') {
    // Backspace
    if (currentLine.length > 0) {
      currentLine = currentLine.slice(0, -1);
      terminal.write('\b \b');
    }
  } else {
    currentLine += data;
    terminal.write(data);
  }
});
```

### 7.3 演示命令集（保留现有）

```typescript
// 保留现有的演示命令
const DEMO_COMMANDS = {
  help: '显示帮助',
  uname: '系统信息',
  whoami: '当前用户',
  ls: '文件列表',
  date: '日期时间',
  clear: '清屏',
  neofetch: '系统概览',
};
```

### 7.4 状态切换流程

```
连接状态 → 远程终端
    ↓ (失败)
演示模式 → 本地回显
    ↓ (连接恢复)
连接状态 → 远程终端
```

### 7.5 离线提示界面

```typescript
function TerminalOfflineHint() {
  return (
    <div className="terminal-offline">
      <div className="offline-message">
        ⚠️ Connection lost
        <p>The remote session has been disconnected.</p>
        <p>Reconnect to the server to continue.</p>
      </div>
    </div>
  );
}
```

---

## 八、关键改进点

### 8.1 相比原版的改进

| 方面 | 原版 | 重写版 |
|------|------|--------|
| **功能完整性** | 基础功能 | GNOME Terminal 完整功能集 |
| **输入处理** | 隐藏 IME input + 复杂逻辑 | xterm.js 默认 onData |
| **输出处理** | 缓冲区管理 + 节流 + alternate screen 检测 | 直接 term.write |
| **搜索功能** | 无 | SearchAddon + 搜索栏 |
| **右键菜单** | 无 | ContextMenu 组件 |
| **主题管理** | 固定主题 | 多主题切换 + 自定义 |
| **设置面板** | 无 | SettingsPanel 组件 |
| **快捷键** | 部分 | 完整快捷键系统 |
| **代码复杂度** | 800+ 行 + 大量注释 | ~500 行，功能完善 |
| **Bug 风险** | 高（过度优化导致） | 低（标准实现） |
| **可维护性** | 困难（逻辑复杂） | 容易（标准流程） |

### 8.2 避免原版 bug 的策略

**第一行乱码问题：**
- 不在 fit 之前写入内容
- 使用 canvas 渲染器（避免 WebGL bug）
- 使用 Unicode11 addon（正确字符宽度）
- 不使用 customGlyphs（避免度量 bug）

**无法输入问题：**
- 使用 xterm.js 默认 onData（不使用隐藏 input）
- 简化输入处理逻辑
- 移除所有 workaround

---

## 九、实施计划

### 9.1 重写步骤

1. **备份原版** - 将原版 Terminal.tsx 重命名为 Terminal.old.tsx
2. **重写前端** - 创建新的 Terminal.tsx（功能完善版）
3. **创建子组件** - HeaderBar、SearchBar、ContextMenu、SettingsPanel
4. **优化后端** - 移除 terminal.rs 中的过度优化逻辑
5. **测试验证** - 测试所有 GNOME Terminal 功能
6. **清理代码** - 移除原版备份和注释

### 9.2 测试清单

**核心功能：**
- [ ] 远程终端连接成功
- [ ] 可以输入命令（包括中文）
- [ ] 第一行无乱码
- [ ] resize 同步正常
- [ ] 多标签页切换正常
- [ ] 断连后演示模式正常

**高级功能：**
- [ ] 配色方案切换正常
- [ ] 字体大小调整正常
- [ ] 搜索功能正常
- [ ] 复制粘贴正常
- [ ] 链接点击打开正常
- [ ] 右键菜单正常
- [ ] 快捷键正常
- [ ] 标签页重命名正常
- [ ] 标签页拖拽排序正常

---

## 十、参考资料

### 10.1 GNOME Terminal 参考

- [GNOME Terminal 官方文档](https://help.gnome.org/users/gnome-terminal/stable/)
- GNOME Terminal 功能列表
- GNOME Terminal 配色方案
- GNOME Terminal 快捷键列表

### 10.2 xterm.js 最佳实践

- [xterm.js 官方文档](https://xtermjs.org/)
- [Flow Control Guide](https://xtermjs.org/docs/guides/flowcontrol/)
- [SearchAddon 文档](https://github.com/xtermjs/xterm.js/tree/master/addons/addon-search)
- 使用 canvas 渲染器（性能好，避免 bug）
- 使用 Unicode11 addon（正确字符宽度）
- 使用标准 onData API（现代浏览器 IME 支持好）

---

## 十一、总结

本次重写基于 GNOME Terminal 的完整功能集，采用"功能完善 + 标准实现"的策略：
- 实现 GNOME Terminal 的所有核心功能
- 使用 xterm.js 标准 API 和 addons
- 移除所有过度复杂的 workaround
- 保持代码简洁清晰
- 确保功能完整、稳定、可维护

目标是解决原版的致命 bug，同时提供功能完善的终端体验，达到 GNOME Terminal 的功能水平。