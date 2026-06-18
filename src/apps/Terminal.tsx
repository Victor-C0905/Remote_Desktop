// src/apps/Terminal.tsx - GNOME Terminal 风格终端
// 标准 xterm.js 集成：每个 tab 一个独立子组件，由 React 生命周期管理
import React, { useEffect, useRef, useState } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
// xterm.js 基础样式（必须导入，否则 canvas/text-layer 无法正确定位）
import '@xterm/xterm/css/xterm.css';
import { useServerManager } from '../context/ServerManager';
import './Terminal.css';

// ── GNOME Terminal 主题 ────────────────────────────────────────────
const GNOME_TERMINAL_THEME = {
  background: '#1e1e1e',
  foreground: '#ffffff',
  cursor: '#4ec9b0',
  cursorAccent: '#1e1e1e',
  selectionBackground: '#264f78',
};

// ── Tab 元数据 ────────────────────────────────────────────────────
interface TerminalTabMeta {
  id: string;
  label: string;
}

let tabIdCounter = 0;
const newTabId = () => `tab-${++tabIdCounter}`;

// ===================================================================
// TerminalInstance: 单个终端实例（每个 tab 一个）
// - 由 React 生命周期管理 xterm 的创建、配置、释放
// - 用独立组件 + ref 保证容器挂载到 DOM 后才初始化终端
// ===================================================================
interface TerminalInstanceProps {
  activeServerName: string | null;
  activeServerHost: string | null;
  activeServerPort: number | null;
  fontSize: number;
  cursorBlink: boolean;
}

function TerminalInstance({
  activeServerName,
  activeServerHost,
  activeServerPort,
  fontSize,
  cursorBlink,
}: TerminalInstanceProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const [initializationStatus, setInitializationStatus] = useState<string>('等待容器挂载...');

  // ── 创建 xterm 实例：保证容器挂载到 DOM 后才执行 ──────────
  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      setInitializationStatus('❌ 容器 ref 为空');
      return;
    }

    // 清理容器（防止 StrictMode 下重复挂载导致脏 DOM）
    while (container.firstChild) {
      container.removeChild(container.firstChild);
    }

    try {
      console.log('[Terminal] 开始创建 xterm 实例，容器尺寸:', container.offsetWidth, 'x', container.offsetHeight);

      // 1. 创建 xterm 实例
      const terminal = new Terminal({
        theme: GNOME_TERMINAL_THEME,
        fontFamily: 'Consolas, "Source Code Pro", monospace',
        fontSize: fontSize,
        cursorBlink: cursorBlink,
        scrollback: 5000,
        allowProposedApi: true,
      });
      terminalRef.current = terminal;

      // 2. 加载 FitAddon（响应式布局）
      const fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon);
      fitAddonRef.current = fitAddon;

      // 3. 加载 WebLinksAddon（URL 可点击）
      try { terminal.loadAddon(new WebLinksAddon()); } catch (e) { console.warn('[Terminal] WebLinksAddon 加载失败:', e); }

      // 4. 挂载到 DOM
      terminal.open(container);
      console.log('[Terminal] ✅ xterm 已挂载到 DOM');

      // 5. 等待下一帧，让浏览器完成 flex 布局后再 fit()
      requestAnimationFrame(() => {
        try {
          fitAddon.fit();
          console.log('[Terminal] ✅ fit() 完成，cols:', terminal.cols, 'rows:', terminal.rows);
          setInitializationStatus(`✅ 就绪 (${terminal.cols} x ${terminal.rows})`);

          // 6. 写入初始内容
          if (activeServerName && activeServerHost) {
            terminal.write(`[远程] 已连接到 ${activeServerName} (${activeServerHost}:${activeServerPort || '?'})\r\n`);
            terminal.write('(QUIC 代理未就绪，当前以演示模式运行)\r\n\r\n');
          }
          // 演示模式 shell
          runDemoShell(terminal);

          // 7. 聚焦
          terminal.focus();
          console.log('[Terminal] ✅ 写入初始内容 + 聚焦完成');
        } catch (err) {
          console.error('[Terminal] ❌ fit() 或 write() 失败:', err);
          setInitializationStatus('❌ fit/write 失败: ' + err);
        }
      });

      // 8. resize 监听
      const onResize = () => {
        try { fitAddonRef.current?.fit(); } catch (_) {}
      };
      window.addEventListener('resize', onResize);

      // ── Cleanup: 组件卸载时释放 ───────────────────────
      return () => {
        window.removeEventListener('resize', onResize);
        try { terminal.dispose(); } catch (_) {}
        terminalRef.current = null;
        fitAddonRef.current = null;
        while (container.firstChild) {
          container.removeChild(container.firstChild);
        }
      };
    } catch (err) {
      console.error('[Terminal] ❌ 初始化失败:', err);
      setInitializationStatus('❌ 初始化失败: ' + err);
      // 兜底：在容器中显示错误
      container.innerHTML = `<div style="color:#ff4444;padding:12px;font-family:monospace;">终端初始化失败: ${err}</div>`;
    }
  }, []); // 🔑 空依赖 = 只在挂载时执行一次（StrictMode 下会 mount→unmount→mount，但 cleanup 会 dispose）

  // ── fontSize / cursorBlink 变更 ───────────────────────
  useEffect(() => {
    const t = terminalRef.current;
    if (!t) return;
    t.options.fontSize = fontSize;
    t.options.cursorBlink = cursorBlink;
    try { fitAddonRef.current?.fit(); } catch (_) {}
  }, [fontSize, cursorBlink]);

  // 渲染一个简单的容器 div，由 ref 传递给 xterm.js
  return (
    <div
      ref={containerRef}
      className="terminal-instance"
      data-status={initializationStatus}
    />
  );
}

// ===================================================================
// 演示模式 shell
// ===================================================================
function runDemoShell(terminal: Terminal): void {
  let cwd = '~';
  let buffer = '';
  const username = 'user';
  const hostname = 'gnome-remote';

  const writePrompt = () => terminal.write(`\r\n\x1b[32m${username}@${hostname}\x1b[0m:\x1b[34m${cwd}\x1b[0m$ `);

  const commands: Record<string, () => void> = {
    help: () => terminal.write('\r\n可用命令：help, echo, ls, pwd, cd, clear, date, whoami, exit\r\n'),
    ls: () => terminal.write('\r\nDesktop  Documents  Downloads  Music  Pictures  Videos  Projects\r\n'),
    pwd: () => terminal.write(`\r\n/home/${username}${cwd === '~' ? '' : '/' + cwd}\r\n`),
    whoami: () => terminal.write(`\r\n${username}\r\n`),
    date: () => terminal.write(`\r\n${new Date().toString()}\r\n`),
    clear: () => terminal.clear(),
    exit: () => terminal.write('\r\n[演示模式已结束，关闭此标签或新建标签继续]\r\n'),
  };

  terminal.write('[演示模式] 输入 help 查看可用命令\r\n');
  writePrompt();

  terminal.onData((data) => {
    if (data === '\r') {
      const cmd = buffer.trim();
      buffer = '';
      if (cmd) {
        const [name, ...args] = cmd.split(/\s+/);
        if (name === 'echo') terminal.write(`\r\n${args.join(' ')}\r\n`);
        else if (name === 'cd') { cwd = args[0] || '~'; terminal.write('\r\n'); }
        else if (commands[name]) commands[name]();
        else terminal.write(`\r\n${name}: command not found\r\n`);
      } else {
        terminal.write('\r\n');
      }
      writePrompt();
    } else if (data === '\u007F' || data === '\b') {
      // 删除键：同步删除 buffer 和终端显示
      if (buffer.length > 0) {
        const lastChar = buffer[buffer.length - 1];
        buffer = buffer.slice(0, -1);
        // 根据字符宽度决定删除多少列（中文等宽字符占 2 列）
        const charWidth = isWideChar(lastChar) ? 2 : 1;
        for (let i = 0; i < charWidth; i++) {
          terminal.write('\b \b');
        }
      }
    } else if (data.charCodeAt(0) < 32) {
      // 忽略其他控制字符
    } else {
      // 正常字符输入（包括 IME 输入的多字符文本）
      buffer += data;
      terminal.write(data);
    }
  });
}

// 判断字符是否为宽字符（CJK 字符在终端中通常占 2 列）
function isWideChar(char: string): boolean {
  const code = char.charCodeAt(0);
  // CJK Unified Ideographs: U+4E00 - U+9FFF
  // CJK Unified Ideographs Extension A: U+3400 - U+4DBF
  // 全角符号等
  return (
    (code >= 0x4E00 && code <= 0x9FFF) ||
    (code >= 0x3400 && code <= 0x4DBF) ||
    (code >= 0xFF00 && code <= 0xFFEF) ||
    (code >= 0x3000 && code <= 0x303F)
  );
}

// ===================================================================
// TerminalApp: 主组件（管理多个 tab、header、设置等 UI shell）
// ===================================================================
export function TerminalApp() {
  const { activeServer } = useServerManager();
  const [tabs, setTabs] = useState<TerminalTabMeta[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [showSearch, setShowSearch] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [fontSize, setFontSize] = useState(14);
  const [cursorBlink, setCursorBlink] = useState(true);
  const [searchText, setSearchText] = useState('');

  // ── 首次打开：创建第一个 tab ─────────────────────────────────
  useEffect(() => {
    if (tabs.length === 0) {
      const id = newTabId();
      setTabs([{ id, label: activeServer?.name || '本地演示' }]);
      setActiveTabId(id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ── 新建 tab ─────────────────────────────────────────────────
  const handleNewTab = () => {
    const id = newTabId();
    setTabs((prev) => [...prev, { id, label: activeServer?.name || '本地演示' }]);
    setActiveTabId(id);
  };

  // ── 关闭 tab ─────────────────────────────────────────────────
  const handleCloseTab = (id: string, e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    const remaining = tabs.filter((t) => t.id !== id);
    setTabs(remaining);
    if (activeTabId === id) {
      setActiveTabId(remaining.length > 0 ? remaining[remaining.length - 1].id : null);
    }
  };

  // ── 右键菜单 ────────────────────────────────────────────────
  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY });
  };

  const handleCopy = () => { setContextMenu(null); };
  const handlePaste = () => { setContextMenu(null); };

  const handleSearch = () => { setContextMenu(null); };

  return (
    <div className="terminal-app" onClick={() => { if (contextMenu) setContextMenu(null); }}>
      {/* Header Bar */}
      <div className="terminal-header-bar">
        <button className="header-button" onClick={handleNewTab} title="新建标签">+ 新建</button>
        <button className="header-button" onClick={() => setShowSearch((v) => !v)} title="搜索">🔍</button>
        <button className="header-button" onClick={() => setShowSettings((v) => !v)} title="设置">⚙️</button>
        <div style={{ flex: 1 }} />
        <span className="header-button" style={{ color: '#888', cursor: 'default' }}>
          {activeServer ? `● 已连接 ${activeServer.name}` : '○ 本地演示模式'}
        </span>
      </div>

      {/* Tab Bar */}
      <div className="terminal-tab-bar">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            className={`terminal-tab ${tab.id === activeTabId ? 'active' : ''}`}
            onClick={() => setActiveTabId(tab.id)}
          >
            <span className="tab-icon">$</span>
            <span className="tab-label">{tab.label}</span>
            <span className="tab-close" onClick={(e) => handleCloseTab(tab.id, e)} title="关闭标签">×</span>
          </button>
        ))}
        <button className="terminal-new-tab" onClick={handleNewTab} title="新建标签">+</button>
      </div>

      {/* Search Bar */}
      {showSearch && (
        <div className="terminal-search-bar">
          <span style={{ color: '#888', fontSize: 12 }}>搜索:</span>
          <input
            className="search-input"
            value={searchText}
            onChange={(e) => setSearchText(e.target.value)}
            autoFocus
            placeholder="在终端中搜索..."
          />
          <button className="search-button" onClick={handleSearch}>查找</button>
          <button className="search-button" onClick={() => setShowSearch(false)}>关闭</button>
        </div>
      )}

      {/* Terminal Containers —— 每个 tab 一个 TerminalInstance 子组件 */}
      <div className="terminal-container" onContextMenu={handleContextMenu}>
        {tabs.map((tab) => (
          <div
            key={tab.id}
            style={{
              position: 'absolute',
              inset: 0,
              display: tab.id === activeTabId ? 'block' : 'none',
            }}
          >
            {/* 只在 active tab 上挂载 TerminalInstance —— 非活动 tab 只占位，不初始化 xterm */}
            {tab.id === activeTabId && (
              <TerminalInstance
                activeServerName={activeServer?.name || null}
                activeServerHost={activeServer?.host || null}
                activeServerPort={activeServer?.port || null}
                fontSize={fontSize}
                cursorBlink={cursorBlink}
              />
            )}
          </div>
        ))}
        {tabs.length === 0 && (
          <div className="terminal-empty">
            <div className="empty-icon">🖥️</div>
            <div className="empty-text">没有打开的终端</div>
            <div className="empty-hint">点击"+ 新建"启动一个新的终端</div>
          </div>
        )}
      </div>

      {/* Settings Panel */}
      {showSettings && (
        <div className="terminal-settings-panel" onClick={(e) => e.stopPropagation()}>
          <label className="settings-label">
            字号:
            <input
              type="range"
              className="settings-slider"
              min={10}
              max={24}
              value={fontSize}
              onChange={(e) => setFontSize(Number(e.target.value))}
            />
            <span className="settings-value">{fontSize}</span>
          </label>
          <label className="settings-label">
            光标闪烁:
            <input
              type="checkbox"
              className="settings-checkbox"
              checked={cursorBlink}
              onChange={(e) => setCursorBlink(e.target.checked)}
            />
          </label>
          <button className="settings-close-button" onClick={() => setShowSettings(false)}>关闭</button>
        </div>
      )}

      {/* Context Menu */}
      {contextMenu && (
        <div className="terminal-context-menu" style={{ left: contextMenu.x, top: contextMenu.y }} onClick={(e) => e.stopPropagation()}>
          <button className="menu-item" onClick={handleCopy}>复制</button>
          <button className="menu-item" onClick={handlePaste}>粘贴</button>
          <hr className="menu-divider" />
          <button className="menu-item" onClick={() => { setShowSearch(true); setContextMenu(null); }}>搜索</button>
          <button className="menu-item" onClick={() => { setShowSettings(true); setContextMenu(null); }}>设置</button>
        </div>
      )}
    </div>
  );
}

export default Terminal