// src/apps/Terminal.tsx - Adwaita Terminal 风格终端
// 标准 xterm.js 集成：每个 tab 一个独立子组件，由 React 生命周期管理
// 支持远程 PTY 连接（通过 QUIC Stream）和本地演示模式
// 集成窗口系统：每个窗口实例独立状态，支持多窗口运行
import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { SearchAddon } from '@xterm/addon-search';
// xterm.js 基础样式（必须导入，否则 canvas/text-layer 无法正确定位）
import '@xterm/xterm/css/xterm.css';
import { useServerManager } from '../context/ServerManager';
// import { useWindowState } from '../window-system/hooks/useWindowState'; // 未来集成时使用
import { useWindowEvent } from '../window-system/hooks/useWindowEvent';
import { createLogger } from '../utils/logger';
import './Terminal.css';

const log = createLogger('Terminal');

// ── Adwaita Terminal 主题 ────────────────────────────────────────────
const ADWAITA_TERMINAL_THEME = {
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
// - 支持远程 PTY 连接（通过 QUIC Stream）和本地演示模式
// ===================================================================
interface TerminalInstanceProps {
  activeServerId: string | null;
  activeServerName: string | null;
  activeServerHost: string | null;
  activeServerPort: number | null;
  fontSize: number;
  cursorBlink: boolean;
  workingDirectory?: string | null;
  // callback: terminal 实例创建后通知父组件（用于复制/粘贴）
  onTerminalReady?: (terminal: Terminal, sessionId: string | null, searchAddon: SearchAddon) => void;
}

function TerminalInstance({
  activeServerId,
  activeServerName,
  activeServerHost,
  activeServerPort,
  fontSize,
  cursorBlink,
  workingDirectory,
  onTerminalReady,
}: TerminalInstanceProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const unlistenRefs = useRef<UnlistenFn[]>([]);
  // IME 合成状态跟踪：防止合成期间焦点切换导致 compositionEnd 丢失，进而卡死输入
  const isComposingRef = useRef(false);
  const composingTimerRef = useRef<number | null>(null);
  const [initializationStatus, setInitializationStatus] = useState<string>('等待容器挂载...');

  // ── 创建 xterm 实例并连接远程 PTY ──────────
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
      log.debug('开始创建 xterm 实例，容器尺寸:', container.offsetWidth, 'x', container.offsetHeight);

      // 1. 创建 xterm 实例
      const terminal = new Terminal({
        theme: ADWAITA_TERMINAL_THEME,
        fontFamily: 'Consolas, "Source Code Pro", monospace',
        fontSize: fontSize,
        cursorBlink: cursorBlink,
        scrollback: 5000,
        allowProposedApi: true,
      });
      terminalRef.current = terminal;

      // 2. 加载 FitAddon（响应式布局）
      const fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon as any);
      fitAddonRef.current = fitAddon;

      // 3. 加载 SearchAddon（搜索功能）
      const searchAddon = new SearchAddon();
      terminal.loadAddon(searchAddon as any);
      searchAddonRef.current = searchAddon;

      // 4. 挂载到 DOM
      terminal.open(container);
      log.debug('✅ xterm 已挂载到 DOM');

      // 4.1 IME 合成保护：监听 textarea 的 composition 事件
      // 防止合成期间焦点切换导致 compositionEnd 丢失，进而卡死输入
      // 根因：xterm.js 内部 isComposing 标志若未随 compositionEnd 重置，
      //       后续所有键盘输入被当作合成文本吞掉，onData 永不触发
      const imeTextarea = (terminal as any).textarea as HTMLTextAreaElement | null;
      if (imeTextarea) {
        const onCompositionStart = () => {
          isComposingRef.current = true;
          (terminal as any).__isComposing = true;
          log.debug('IME 合成开始');
          // 卡死检测：30 秒后若仍在合成，疑似 compositionEnd 丢失，强制重置
          if (composingTimerRef.current) {
            clearTimeout(composingTimerRef.current);
          }
          composingTimerRef.current = window.setTimeout(() => {
            if (isComposingRef.current) {
              log.warn('IME 合成超时（30s），疑似卡死，强制重置 isComposing');
              isComposingRef.current = false;
              (terminal as any).__isComposing = false;
              // 派发合成结束事件，清理 xterm 内部状态
              try {
                imeTextarea.dispatchEvent(new CompositionEvent('compositionend', { data: '' }));
              } catch (e) {
                log.warn('强制 compositionend 派发失败:', e);
              }
            }
          }, 30000);
        };

        const onCompositionEnd = () => {
          isComposingRef.current = false;
          (terminal as any).__isComposing = false;
          log.debug('IME 合成结束');
          if (composingTimerRef.current) {
            clearTimeout(composingTimerRef.current);
            composingTimerRef.current = null;
          }
        };

        // 合成期间 textarea 失焦时，compositionEnd 可能不触发
        // （焦点被其他 terminal.focus() 或 window:focused 回调抢走）
        // 延迟检查：若失焦后仍在合成状态，强制重置
        const onBlur = () => {
          if (isComposingRef.current) {
            log.warn('IME 合成期间 textarea 失焦，延迟检查是否卡死');
            window.setTimeout(() => {
              if (isComposingRef.current) {
                log.warn('失焦后仍在合成状态，强制重置 isComposing');
                isComposingRef.current = false;
                (terminal as any).__isComposing = false;
                try {
                  imeTextarea.dispatchEvent(new CompositionEvent('compositionend', { data: '' }));
                } catch (e) {
                  log.warn('强制 compositionend 派发失败:', e);
                }
              }
            }, 100);
          }
        };

        imeTextarea.addEventListener('compositionstart', onCompositionStart);
        imeTextarea.addEventListener('compositionend', onCompositionEnd);
        imeTextarea.addEventListener('blur', onBlur);

        // 存储清理函数
        unlistenRefs.current.push(() => {
          imeTextarea.removeEventListener('compositionstart', onCompositionStart);
          imeTextarea.removeEventListener('compositionend', onCompositionEnd);
          imeTextarea.removeEventListener('blur', onBlur);
          if (composingTimerRef.current) {
            clearTimeout(composingTimerRef.current);
            composingTimerRef.current = null;
          }
        });
      } else {
        log.warn('无法获取 xterm textarea，IME 保护未启用');
      }

      // 5. 等待下一帧，让浏览器完成 flex 布局后再 fit()
      requestAnimationFrame(async () => {
        try {
          fitAddon.fit();
          log.debug('✅ fit() 完成，cols:', terminal.cols, 'rows:', terminal.rows);

          // 6. 尝试连接远程 PTY 或启动演示模式
          if (activeServerId) {
            setInitializationStatus('连接远程终端...');
            await connectRemotePty(terminal, activeServerId, activeServerName, activeServerHost, activeServerPort, sessionIdRef, unlistenRefs, workingDirectory);
          } else {
            setInitializationStatus('演示模式');
            terminal.write('[演示模式] 未连接远程服务器\r\n');
            runDemoShell(terminal);
          }

          // 7. 通知父组件 terminal 已就绪（用于复制/粘贴/搜索）
          if (onTerminalReady) {
            onTerminalReady(terminal, sessionIdRef.current, searchAddon);
          }

          // 8. 聚焦
          terminal.focus();
          log.info('✅ 初始化完成');
        } catch (err) {
          log.error('❌ 初始化失败:', err);
          setInitializationStatus('❌ 初始化失败: ' + err);
          // 回退到演示模式
          terminal.write(`\x1b[31m[连接失败]\x1b[0m ${err}\r\n`);
          terminal.write('[演示模式] 输入 help 查看可用命令\r\n');
          runDemoShell(terminal);
          // 即使失败也通知父组件（演示模式可用）
          if (onTerminalReady) {
            onTerminalReady(terminal, null, searchAddon);
          }
        }
      });

      // ── Resize 处理（方案 B：resize 过程中管理数据流）───────────────
      // 策略：
      // 1. ResizeObserver 监控容器尺寸变化（实时触发 fit）
      // 2. resize 过程中暂停远程数据接收（避免数据叠加在错误的尺寸上）
      // 3. resize 结束后发送 resize 到远程 PTY，让它重新发送完整内容

      let lastCols = terminal.cols;
      let lastRows = terminal.rows;
      let resizeRAF: number | null = null;

      // ResizeObserver：监控容器尺寸变化，实时触发 fit()
      const resizeObserver = new ResizeObserver(() => {
        if (resizeRAF) {
          cancelAnimationFrame(resizeRAF);
        }

        resizeRAF = requestAnimationFrame(() => {
          try {
            const container = containerRef.current;
            if (!container) return;

            // 检查容器是否可见且尺寸有效（避免最小化时的无效 resize）
            const isVisible = container.offsetWidth > 0 && container.offsetHeight > 0;
            if (!isVisible) {
              log.debug('容器不可见，跳过 resize');
              resizeRAF = null;
              return;
            }

            // 立即 fit()（让 xterm 调整到正确的尺寸）
            fitAddon.fit();

            const terminal = terminalRef.current;
            const sessionId = sessionIdRef.current;

            // 检查 cols/rows 是否有效（避免最小化时的无效尺寸）
            const minCols = 10;
            const minRows = 5;
            if (!terminal || terminal.cols < minCols || terminal.rows < minRows) {
              log.debug('尺寸太小，跳过 resize:', terminal?.cols, 'x', terminal?.rows);
              resizeRAF = null;
              return;
            }

            // fit() 后立即发送 resize 到远程（让远程 PTY 知道新尺寸）
            if (terminal && sessionId && activeServerId) {
              if (terminal.cols !== lastCols || terminal.rows !== lastRows) {
                lastCols = terminal.cols;
                lastRows = terminal.rows;

                log.debug('resize:', terminal.cols, 'x', terminal.rows);

                // 发送 resize 到远程 PTY（远程会重新发送完整屏幕内容）
                invoke('remote_terminal_resize', {
                  sessionId: sessionId,
                  cols: terminal.cols,
                  rows: terminal.rows,
                  serverId: activeServerId,
                }).catch(e => log.warn('resize 同步失败:', e));
              }
            }

            // resize 完成
            resizeRAF = null;
          } catch (e) { log.debug('resize observer 回调失败:', e); }
        });
      });

      resizeObserver.observe(container);

      // 存储 disposable 以便 cleanup
      unlistenRefs.current.push(() => {
        resizeObserver.disconnect();
        if (resizeRAF) {
          cancelAnimationFrame(resizeRAF);
        }
      });

      return () => {
        // 清理所有监听器和观察器
        unlistenRefs.current.forEach(fn => fn());
        unlistenRefs.current = [];

        // 关闭远程终端会话
        if (sessionIdRef.current) {
          invoke('remote_terminal_close', { sessionId: sessionIdRef.current })
            .catch(e => log.warn('关闭远程终端失败:', e));
          sessionIdRef.current = null;
        }

        // 释放 xterm
        try { terminal.dispose(); } catch (e) { log.debug('terminal.dispose 失败:', e); }
        terminalRef.current = null;
        fitAddonRef.current = null;

        // 清理 DOM
        while (container.firstChild) {
          container.removeChild(container.firstChild);
        }
      };
    } catch (err) {
      log.error('❌ 初始化失败:', err);
      setInitializationStatus('❌ 初始化失败: ' + err);
      container.innerHTML = `<div style="color:#ff4444;padding:12px;font-family:monospace;">终端初始化失败: ${err}</div>`;
    }
  }, [activeServerId, activeServerName, activeServerHost, activeServerPort]);

  // ── fontSize / cursorBlink 变更 ───────────────────────
  useEffect(() => {
    const t = terminalRef.current;
    if (!t) return;
    t.options.fontSize = fontSize;
    t.options.cursorBlink = cursorBlink;
    try { fitAddonRef.current?.fit(); } catch (e) { log.debug('fit 调整失败:', e); }
  }, [fontSize, cursorBlink]);

  // 渲染容器 div
  return (
    <div
      ref={containerRef}
      className="terminal-instance"
      data-status={initializationStatus}
    />
  );
}

// ===================================================================
// 连接远程 PTY
// ===================================================================
async function connectRemotePty(
  terminal: Terminal,
  serverId: string,
  serverName: string | null,
  serverHost: string | null,
  serverPort: number | null,
  sessionIdRef: React.MutableRefObject<string | null>,
  unlistenRefs: React.MutableRefObject<UnlistenFn[]>,
  workingDirectory?: string | null,
): Promise<string> {
  // 1. 创建远程终端会话
  const result = await invoke<{ session_id: string }>('remote_spawn_terminal', {
    serverId: serverId,
    shell: '',  // 使用默认 shell
    cols: terminal.cols,
    rows: terminal.rows,
    workingDirectory: workingDirectory || null,
  });

  const sessionId = result.session_id;
  // 存储 sessionId（通过 ref 传递给父组件）
  sessionIdRef.current = sessionId;
  (terminal as any).__sessionId = sessionId;

  log.info('✅ 远程终端创建成功:', sessionId);

  // 2. 显示连接信息
  terminal.write(`\x1b[1;32m✅ 远程终端已连接\x1b[0m\r\n`);
  terminal.write(`服务器: \x1b[1;34m${serverName || serverHost}:${serverPort || 8443}\x1b[0m\r\n`);
  terminal.write(`Session: ${sessionId}\r\n\r\n`);

  // 3. 监听终端输出事件
  const unlistenOutput = await listen<{ session_id: string; data: number[] }>(
    'terminal-output',
    (event) => {
      if (event.payload.session_id === sessionId) {
        const bytes = new Uint8Array(event.payload.data);
        terminal.write(bytes);
      }
    }
  );
  unlistenRefs.current.push(unlistenOutput);

  // 4. 监听终端断连事件
  const unlistenDisconnect = await listen<{ session_id: string }>(
    'terminal-disconnected',
    (event) => {
      if (event.payload.session_id === sessionId) {
        terminal.write('\r\n\x1b[31m[远程终端断开]\x1b[0m\r\n');
        // 移除所有监听器
        unlistenRefs.current.forEach(fn => fn());
        unlistenRefs.current = [];
        sessionIdRef.current = null;
        (terminal as any).__sessionId = null;
      }
    }
  );
  unlistenRefs.current.push(unlistenDisconnect);

  // 5. 设置键盘输入处理（在发送输入前先同步 resize）
  const onDataDisposable = terminal.onData((data: string) => {
    const sid = sessionIdRef.current;
    if (sid) {
      // 发送用户输入
      const bytes = new TextEncoder().encode(data);
      invoke('remote_terminal_write', {
        sessionId: sid,
        data: Array.from(bytes),
        serverId: serverId,
      }).catch(e => log.warn('写入失败:', e));
    }
  });
  // 存储 disposable 以便 cleanup
  unlistenRefs.current.push(() => onDataDisposable.dispose());

  // 6. resize 事件监听器（仅用于日志，远程同步由 handleResize 处理）
  const onResizeDisposable = terminal.onResize(({ cols, rows }) => {
    log.debug('本地 resize:', cols, 'x', rows);
  });
  unlistenRefs.current.push(() => onResizeDisposable.dispose());

  return sessionId;
}

// ===================================================================
// 演示模式 shell
// ===================================================================
function runDemoShell(terminal: Terminal): void {
  let cwd = '~';
  let buffer = '';
  const username = 'user';
  const hostname = 'quirel';

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
interface TerminalAppProps {
  windowId: string;
  preloadData?: {
    workingDirectory?: string;
  };
}

export function TerminalApp({ windowId, preloadData }: TerminalAppProps) {
  // ── 窗口系统集成 ────────────────────────────────────────────────────
  // 获取窗口状态（title、position、size、focused 等）
  // 用于窗口系统集成，确保每个窗口实例正确连接到窗口管理器
  // const windowState = useWindowState(windowId); // 未来集成时使用
  const { activeServer } = useServerManager();
  const activeServerId = activeServer?.id || null;

  // ── 工作目录配置 ────────────────────────────────────────────────────
  // 优先使用 preloadData（从文件管理器打开），否则使用用户配置的默认路径
  const [configuredDefaultPath] = useState(() => {
    return localStorage.getItem("terminal-default-path") || null;
  });

  const workingDirectory = preloadData?.workingDirectory || configuredDefaultPath;

  // ── 窗口实例独立状态（每个窗口实例有自己的 tabs、设置等）────────────
  const [tabs, setTabs] = useState<TerminalTabMeta[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [showSearch, setShowSearch] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [fontSize, setFontSize] = useState(14);
  const [cursorBlink, setCursorBlink] = useState(true);
  const [searchText, setSearchText] = useState('');

  // ── 活动终端的 refs（用于复制/粘贴/搜索）────────────────────────
  const activeTerminalRef = useRef<Terminal | null>(null);
  const activeSessionIdRef = useRef<string | null>(null);
  const activeSearchAddonRef = useRef<SearchAddon | null>(null);

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

  // ── 右键菜单（Adwaita Terminal 风格：选中后右键直接复制，未选中时显示粘贴菜单）────────────────
  const handleContextMenu = async (e: React.MouseEvent) => {
    e.preventDefault();
    const terminal = activeTerminalRef.current;

    if (terminal) {
      const selection = terminal.getSelection();
      if (selection) {
        // 有选中文本：直接复制
        try {
          await navigator.clipboard.writeText(selection);
          log.info('✅ 已复制选中文本到剪贴板');
        } catch (err) {
          log.warn('❌ 复制失败:', err);
        }
        terminal.clearSelection();
      } else {
        // 无选中文本：直接粘贴
        await handlePaste();
      }
      terminal.focus();
    }
  };

  // ── 复制功能 ────────────────────────────────────────────────
  const handleCopy = async () => {
    const terminal = activeTerminalRef.current;
    if (terminal) {
      const selection = terminal.getSelection();
      if (selection) {
        try {
          await navigator.clipboard.writeText(selection);
          log.info('✅ 已复制到剪贴板');
        } catch (e) {
          log.warn('❌ 复制失败:', e);
        }
      }
    }
    setContextMenu(null);
  };

  // ── 粘贴功能 ────────────────────────────────────────────────
  const handlePaste = async () => {
    const terminal = activeTerminalRef.current;
    const sessionId = activeSessionIdRef.current;
    if (!terminal) return;

    try {
      const text = await navigator.clipboard.readText();
      if (text) {
        if (sessionId && activeServerId) {
          // 远程模式：使用 Bracketed Paste Mode
          // \x1b[200~ 开始粘贴序列 → 文本内容 → \x1b[201~ 结束粘贴序列
          // 这样 bash/readline 知道这是粘贴内容，会正确处理多行（不自动执行）
          const pasteData = '\x1b[200~' + text + '\x1b[201~';
          const bytes = new TextEncoder().encode(pasteData);
          await invoke('remote_terminal_write', {
            sessionId,
            data: Array.from(bytes),
            serverId: activeServerId,
          });
          log.info('✅ 已粘贴到远程终端（bracketed paste mode）');
        } else {
          // 演示模式：直接写入
          terminal.write(text);
          log.info('✅ 已粘贴到演示终端');
        }
      }
    } catch (e) {
      log.warn('❌ 粘贴失败:', e);
    }
    setContextMenu(null);
    // 粘贴后重新聚焦终端，避免需要手动点击
    terminal.focus();
  };

  // ── 搜索功能 ────────────────────────────────────────────────
  const handleSearch = () => {
    const searchAddon = activeSearchAddonRef.current;
    if (searchAddon && searchText) {
      try {
        searchAddon.findNext(searchText, {
          caseSensitive: false,
          wholeWord: false,
        });
        log.info('✅ 搜索:', searchText);
      } catch (e) {
        log.warn('❌ 搜索失败:', e);
      }
    }
    setContextMenu(null);
  };

  // ── 搜索上一个/下一个 ────────────────────────────────────────
  const handleSearchPrev = () => {
    const searchAddon = activeSearchAddonRef.current;
    if (searchAddon && searchText) {
      searchAddon.findPrevious(searchText, { caseSensitive: false, wholeWord: false });
    }
  };

  const handleSearchNext = () => {
    const searchAddon = activeSearchAddonRef.current;
    if (searchAddon && searchText) {
      searchAddon.findNext(searchText, { caseSensitive: false, wholeWord: false });
    }
  };

  // ── 快捷键系统 ────────────────────────────────────────────────
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Ctrl+Shift+C: 复制
      if (e.ctrlKey && e.shiftKey && e.key.toUpperCase() === 'C') {
        e.preventDefault();
        handleCopy();
        return;
      }
      // Ctrl+Shift+V: 粘贴
      if (e.ctrlKey && e.shiftKey && e.key.toUpperCase() === 'V') {
        e.preventDefault();
        handlePaste();
        return;
      }
      // Ctrl+Shift+F: 搜索
      if (e.ctrlKey && e.shiftKey && e.key.toUpperCase() === 'F') {
        e.preventDefault();
        setShowSearch(v => !v);
        return;
      }
      // Ctrl+Shift+T: 新建标签
      if (e.ctrlKey && e.shiftKey && e.key.toUpperCase() === 'T') {
        e.preventDefault();
        handleNewTab();
        return;
      }
      // Ctrl+Shift+W: 关闭标签
      if (e.ctrlKey && e.shiftKey && e.key.toUpperCase() === 'W') {
        e.preventDefault();
        if (activeTabId) handleCloseTab(activeTabId);
        return;
      }
      // Ctrl+Tab: 下一个标签
      if (e.ctrlKey && e.key === 'Tab' && !e.shiftKey) {
        e.preventDefault();
        const idx = tabs.findIndex(t => t.id === activeTabId);
        if (idx >= 0 && tabs.length > 1) {
          const nextIdx = (idx + 1) % tabs.length;
          setActiveTabId(tabs[nextIdx].id);
        }
        return;
      }
      // Ctrl+Shift+Tab: 上一个标签
      if (e.ctrlKey && e.shiftKey && e.key === 'Tab') {
        e.preventDefault();
        const idx = tabs.findIndex(t => t.id === activeTabId);
        if (idx >= 0 && tabs.length > 1) {
          const prevIdx = (idx - 1 + tabs.length) % tabs.length;
          setActiveTabId(tabs[prevIdx].id);
        }
        return;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [tabs, activeTabId, activeServerId, searchText]);

  // ── 窗口事件监听（窗口系统集成）───────────────────────────────────────
  // 监听窗口获得焦点事件：自动聚焦活动的终端
  // 注意：若活动终端正在 IME 合成中，跳过 focus() 以避免 compositionEnd 丢失导致输入卡死
  useWindowEvent('window:focused', (event) => {
    if (event.windowId === windowId) {
      const terminal = activeTerminalRef.current;
      if (terminal && (terminal as any).__isComposing) {
        log.debug('窗口获得焦点，但终端正在 IME 合成，跳过 focus()');
        return;
      }
      log.debug('窗口获得焦点，自动聚焦终端');
      terminal?.focus();
    }
  });

  // 监听窗口尺寸变化事件：触发终端 fit()（ResizeObserver 已处理，此处作为补充）
  useWindowEvent('window:resized', (event) => {
    if (event.windowId === windowId) {
      log.debug('窗口尺寸变化，触发终端 fit()');
      // ResizeObserver 已经在 TerminalInstance 中处理，这里不需要额外操作
      // 但可以添加一些额外的逻辑，比如记录窗口尺寸等
    }
  });

  return (
    <div className="terminal-app" onClick={() => { if (contextMenu) setContextMenu(null); }}>
      {/* Tab Bar - 整合原HeaderBar功能 */}
      <div className="terminal-tabs-container">
        {/* Tab List */}
        <div className="terminal-tab-list">
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

        {/* Actions and Status */}
        <div className="terminal-tab-actions">
          <button className="terminal-tab-btn" onClick={() => setShowSearch((v) => !v)} title="搜索">🔍</button>
          <button className="terminal-tab-btn" onClick={() => setShowSettings((v) => !v)} title="设置">⚙️</button>
          <span className="terminal-status">
            {activeServer ? `● ${activeServer.name}` : '○ 本地'}
          </span>
        </div>
      </div>

      {/* Search Bar */}
      {showSearch && (
        <div className="terminal-search-bar">
          <span style={{ color: '#888', fontSize: 12 }}>搜索:</span>
          <input
            className="terminal-search-input"
            value={searchText}
            onChange={(e) => setSearchText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleSearch();
            }}
            autoFocus
            placeholder="在终端中搜索..."
          />
          <button className="terminal-search-button" onClick={handleSearchPrev} title="上一个">↑</button>
          <button className="terminal-search-button" onClick={handleSearchNext} title="下一个">↓</button>
          <button className="terminal-search-button" onClick={handleSearch}>查找</button>
          <button className="terminal-search-button" onClick={() => setShowSearch(false)}>关闭</button>
        </div>
      )}

      {/* Terminal Containers —— 每个 tab 一个 TerminalInstance 子组件 */}
      {/* 所有 tab 都保持挂载，通过 display 控制显示，避免切换时卸载导致会话丢失 */}
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
            <TerminalInstance
              key={tab.id}
              activeServerId={activeServerId}
              activeServerName={activeServer?.name || null}
              activeServerHost={activeServer?.host || null}
              activeServerPort={activeServer?.port || null}
              fontSize={fontSize}
              cursorBlink={cursorBlink}
              workingDirectory={workingDirectory}
              onTerminalReady={(terminal, sessionId, searchAddon) => {
                // 更新 refs（所有 tab 都更新，但只有活动 tab 的 terminal 可见）
                activeTerminalRef.current = terminal;
                activeSessionIdRef.current = sessionId;
                activeSearchAddonRef.current = searchAddon;
              }}
            />
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
          <label className="terminal-settings-label">
            字号:
            <input
              type="range"
              className="terminal-settings-slider"
              min={10}
              max={24}
              value={fontSize}
              onChange={(e) => setFontSize(Number(e.target.value))}
            />
            <span className="terminal-settings-value">{fontSize}</span>
          </label>
          <label className="terminal-settings-label">
            光标闪烁:
            <input
              type="checkbox"
              className="terminal-settings-checkbox"
              checked={cursorBlink}
              onChange={(e) => setCursorBlink(e.target.checked)}
            />
          </label>
          <button className="terminal-settings-close-button" onClick={() => setShowSettings(false)}>关闭</button>
        </div>
      )}

      {/* Context Menu（只在未选中时显示，提供粘贴选项）*/}
      {contextMenu && (
        <div className="terminal-context-menu" style={{ left: contextMenu.x, top: contextMenu.y }} onClick={(e) => e.stopPropagation()}>
          <button className="terminal-menu-item" onClick={handlePaste}>粘贴</button>
          <hr className="terminal-menu-divider" />
          <button className="terminal-menu-item" onClick={() => { setShowSearch(true); setContextMenu(null); }}>搜索</button>
          <button className="terminal-menu-item" onClick={() => { setShowSettings(true); setContextMenu(null); }}>设置</button>
        </div>
      )}
    </div>
  );
}

export default Terminal