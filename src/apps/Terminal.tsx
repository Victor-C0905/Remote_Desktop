// src/apps/Terminal.tsx - Adwaita Terminal 风格终端
// 标准 xterm.js 集成：每个 tab 一个独立子组件，由 React 生命周期管理
// 支持远程 PTY 连接（通过 QUIC Stream）和本地演示模式
// 集成窗口系统：每个窗口实例独立状态，支持多窗口运行
import React, { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { SearchAddon } from '@xterm/addon-search';
import { WebglAddon } from '@xterm/addon-webgl';
// xterm.js 基础样式（必须导入，否则 canvas/text-layer 无法正确定位）
import '@xterm/xterm/css/xterm.css';
import { useServerManager } from '../context/ServerManager';
import { ToolbarMenu } from '../components/ToolbarMenu/ToolbarMenu';
import { ChromeTabStrip } from '../components/ChromeTabStrip/ChromeTabStrip';
// import { useWindowState } from '../window-system/hooks/useWindowState'; // 未来集成时使用
import { useWindowEvent } from '../window-system/hooks/useWindowEvent';
import { describeTerminalFailure, formatUnknownError } from '../types/errors';
import { useTranslation } from 'react-i18next';
import i18n from '../i18n';
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

// ── 最小化期间的终端输出缓冲（Tier1 最小化闸门）──────────────────
// 透明窗口使 WebView2 遮挡检测失效：最小化后页面仍被判定"可见"，PTY 输出
// 持续触发 xterm 渲染吃 GPU/CPU。闸门策略：最小化期间不 write（不触发渲染），
// 只入缓冲，恢复可见时按序冲刷。
interface PendingOutput {
  chunks: Uint8Array[];
  bytes: number;
  /** 缓冲超限丢弃过最旧数据，冲刷前需插入省略提示行 */
  overflowed: boolean;
}
/** 缓冲上限 16MB：防长时间最小化 + 高频输出（如 cat 大文件）把内存打爆 */
const MAX_PENDING_OUTPUT_BYTES = 16 * 1024 * 1024;

let tabIdCounter = 0;
const newTabId = () => `tab-${++tabIdCounter}`;

// 页签标题用自增序号区分会话（不显示服务器名：服务器信息统一在底部状态栏展示）；
// 取已有标签的最大序号 + 1，避免关闭中间页签后新建出现重复编号
const nextTabNumber = (tabs: { label: string }[]) =>
  tabs.reduce((max, t) => Math.max(max, Number(/(\d+)\s*$/.exec(t.label)?.[1] ?? 0)), 0) + 1;

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
  /** 窗口创建后自动执行的命令（脚本运行：双击 .sh 由 FileManager 注入） */
  initialCommand?: string | null;
  /** 本页签是否处于激活状态（WebGL 渲染器仅挂载在活动页签上，失活回落 DOM） */
  isActive?: boolean;
  /** OS 级窗口最小化（Tier1 闸门：最小化期间输出只缓冲不渲染） */
  osMinimized?: boolean;
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
  initialCommand,
  isActive,
  osMinimized,
  onTerminalReady,
}: TerminalInstanceProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const searchAddonRef = useRef<SearchAddon | null>(null);
  /** WebGL 渲染器 addon（仅活动页签挂载；随 terminal.dispose 自动释放） */
  const webglAddonRef = useRef<WebglAddon | null>(null);
  /** 终端实例纪元：每次重建（服务器切换/重新初始化）+1，驱动 WebGL 挂载 effect 重跑 */
  const [terminalEpoch, setTerminalEpoch] = useState(0);
  /** 最小化期间的输出缓冲（null=无会话）；恢复可见时按序冲刷 */
  const pendingOutputRef = useRef<PendingOutput | null>(null);
  /** connectRemotePty 注册的冲刷函数（恢复可见时调用；会话销毁时置空） */
  const flushOutputRef = useRef<(() => void) | null>(null);
  // 渲染期直写 ref：connectRemotePty 的输出监听闭包经 ref 读取最新最小化状态
  // （项目教训：useCallback/闭包内禁止直读 props，激活/状态对齐会被首帧旧值跳过）
  const osMinimizedRef = useRef<boolean>(osMinimized ?? false);
  osMinimizedRef.current = osMinimized ?? false;
  const sessionIdRef = useRef<string | null>(null);
  const unlistenRefs = useRef<UnlistenFn[]>([]);
  // IME 合成状态跟踪：防止合成期间焦点切换导致 compositionEnd 丢失，进而卡死输入
  const isComposingRef = useRef(false);
  const composingTimerRef = useRef<number | null>(null);
  const [initializationStatus, setInitializationStatus] = useState<string>(i18n.t('terminal.waitingMount'));

  // ── 会话保活：防「拖拽重排触发 effect 假重跑」丢内容 ──────────
  // 实测（内存级事件流取证）：页签拖拽重排后约 5ms，被移动页签的同一
  // TerminalInstance（instance 编号不变 = 未重挂载）发生 cleanup→init，
  // 且 deps 完全未变——React 强制重跑了 effect；若 cleanup 立即销毁
  // xterm，终端缓冲区（输入/输出历史）随之真实丢失。编辑器丢字同理。
  // 修复策略（机制无关）：cleanup 只调度 setTimeout(0) 的延迟销毁；
  // 紧随其后的 setup 发现「终端仍在 + deps 未变」则取消销毁直接保活。
  // - 真实卸载：无人取消，延迟销毁照常执行，无泄漏
  // - deps 真变（切换服务器）：setup 同步 teardownNow 后完整重建，语义不变
  // - StrictMode 双跑：run1 建 → cleanup 调度 → run2 取消并保活，
  //   顺带消除了双跑重复建终端的浪费
  const teardownTimerRef = useRef<number | null>(null);
  /** 建立当前会话时的 deps 快照（JSON）：保活判定与服务器切换判定的依据 */
  const depsKeyRef = useRef<string | null>(null);
  // rAF 竞态防护状态移入 ref：延迟销毁在 cleanup 闭包之外执行，
  // 访问不到 setup 局部的 disposed/initRafId，必须经 ref 读写
  const disposedRef = useRef(false);
  const initRafIdRef = useRef<number | null>(null);

  /** 立即销毁当前会话（可从 setup 同步调用，也可由延迟定时器触发；全 ref 化） */
  const teardownNow = () => {
    if (teardownTimerRef.current !== null) {
      clearTimeout(teardownTimerRef.current);
      teardownTimerRef.current = null;
    }
    // 取消排队的初始化 rAF，防止为已销毁终端建立幽灵远程会话
    disposedRef.current = true;
    if (initRafIdRef.current !== null) {
      cancelAnimationFrame(initRafIdRef.current);
      initRafIdRef.current = null;
    }
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
    try { terminalRef.current?.dispose(); } catch (e) { log.debug('terminal.dispose 失败:', e); }
    terminalRef.current = null;
    fitAddonRef.current = null;
    // WebGL addon 已随 terminal.dispose 释放，清引用避免残留指向旧实例；
    // 同步释放输出缓冲与冲刷句柄（会话已销毁，缓冲数据作废）
    webglAddonRef.current = null;
    pendingOutputRef.current = null;
    flushOutputRef.current = null;

    // 清理 DOM
    const container = containerRef.current;
    if (container) {
      while (container.firstChild) {
        container.removeChild(container.firstChild);
      }
    }
  };

  /** cleanup 入口：不立即销毁，只调度延迟销毁（假重跑的 setup 会取消它） */
  const scheduleTeardown = () => {
    if (teardownTimerRef.current !== null) {
      clearTimeout(teardownTimerRef.current);
    }
    teardownTimerRef.current = window.setTimeout(() => {
      teardownTimerRef.current = null;
      teardownNow();
    }, 0);
  };

  // ── 创建 xterm 实例并连接远程 PTY ──────────
  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      setInitializationStatus(i18n.t('terminal.containerRefEmpty'));
      return;
    }

    // 取消上一次 cleanup 调度的延迟销毁：拖拽重排触发的 effect 假重跑
    //（cleanup→init 相邻发生）在此保活，终端与远程会话原样保留
    if (teardownTimerRef.current !== null) {
      clearTimeout(teardownTimerRef.current);
      teardownTimerRef.current = null;
    }

    // 会话保活判定：终端仍在 + deps 未变 = React 假重跑（页签拖拽重排），
    // 跳过重建；但必须返回延迟销毁 cleanup，保证真实卸载时仍能清理
    const depsKey = JSON.stringify([activeServerId, activeServerName, activeServerHost, activeServerPort]);
    if (terminalRef.current && depsKey === depsKeyRef.current) {
      return scheduleTeardown;
    }

    // deps 真变（如切换服务器）：先同步销毁旧会话，再走完整重建（保留原有切换语义）
    if (terminalRef.current) {
      teardownNow();
    }
    depsKeyRef.current = depsKey;

    // 清理容器（防御性：切换服务器路径已被 teardownNow 清空，此处兜底脏 DOM）
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
      // 终端纪元 +1：通知 WebGL 挂载 effect（deps 含 terminalEpoch）在
      // 新实例上重新执行——服务器切换时 isActive 不变，仅靠该信号触发重挂
      setTerminalEpoch((e) => e + 1);

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
      // ⚠️ 会话卫生：rAF 句柄与 disposed 标志存入 ref（teardownNow 在 cleanup
      //    闭包之外执行，访问不到 setup 局部变量，必须经 ref 读写）。延迟销毁
      //    真正执行时置位 disposedRef 并取消 rAF，防止为幽灵终端建立远程会话；
      //    假重跑保活路径不触碰两者，本 rAF 正常完成初始化。
      disposedRef.current = false;
      initRafIdRef.current = requestAnimationFrame(async () => {
        if (disposedRef.current) return;
        try {
          fitAddon.fit();
          log.debug('✅ fit() 完成，cols:', terminal.cols, 'rows:', terminal.rows);

          // 6. 尝试连接远程 PTY 或启动演示模式
          if (activeServerId) {
            setInitializationStatus(i18n.t('terminal.connectingRemote'));
            await connectRemotePty(terminal, activeServerId, activeServerName, activeServerHost, activeServerPort, sessionIdRef, unlistenRefs, workingDirectory, initialCommand, osMinimizedRef, pendingOutputRef, flushOutputRef);
            // await 期间 teardown 已执行：刚建立的会话无人使用，立即关闭防泄漏
            if (disposedRef.current) {
              log.warn('初始化完成前组件已清理，关闭幽灵远程会话');
              if (sessionIdRef.current) {
                invoke('remote_terminal_close', { sessionId: sessionIdRef.current }).catch(e => log.warn('关闭幽灵会话失败:', e));
                sessionIdRef.current = null;
              }
              // connectRemotePty 在 cleanup 之后 resolve，其注册的输出/断连
              // 监听器来不及进 cleanup 的清空批次，这里补清
              unlistenRefs.current.forEach(fn => fn());
              unlistenRefs.current = [];
              return;
            }
          } else {
            setInitializationStatus(i18n.t('terminal.demoMode'));
            terminal.write(i18n.t('terminal.demoNotConnected'));
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
          setInitializationStatus(i18n.t('terminal.initFailedStatus', { error: formatUnknownError(err) }));
          // 回退到演示模式
          terminal.write(`\x1b[31m${i18n.t('terminal.connectFailedPrefix')}\x1b[0m ${describeTerminalFailure(err)}\r\n`);
          terminal.write(i18n.t('terminal.demoHelpHint'));
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

      // cleanup 不再立即销毁：只调度延迟销毁（0ms）。若紧随其后的 setup 判定
      // 为假重跑（终端仍在 + deps 未变）会取消销毁实现保活；真实卸载时无人
      // 取消，teardownNow 照常执行完整清理。诊断留痕移至 scheduleTeardown。
      return scheduleTeardown;
    } catch (err) {
      log.error('❌ 初始化失败:', err);
      setInitializationStatus(i18n.t('terminal.initFailedStatus', { error: formatUnknownError(err) }));
      // 创建半途失败：清扫半初始化状态（terminalRef 可能已赋值），并把
      // depsKey 复位，避免下次 setup 把残缺终端误判为可保活
      depsKeyRef.current = null;
      teardownNow();
      container.innerHTML = `<div style="color:#ff4444;padding:12px;font-family:monospace;">${i18n.t('terminal.initFailedHtml', { error: formatUnknownError(err) })}</div>`;
    }
  }, [activeServerId, activeServerName, activeServerHost, activeServerPort]);

  // ── WebGL 渲染器：仅活动页签挂载，失活即卸载 ─────────────────────
  // GPU 占用归因：全部页签常驻 DOM 渲染是主要来源之一。WebGL 渲染器由 GPU
  // 合成、CPU 开销大幅降低；但 WebGL 上下文全浏览器上限约 16 个，多窗口×
  // 多页签常驻挂载必撞线，且失活页签（display:none）挂载 WebGL 会以 0 尺寸
  // 绘制图集纯浪费。策略：激活时挂载，失活时 dispose（xterm 自动回落 DOM
  // 渲染器，终端实例与缓冲区不受影响）；创建失败/上下文丢失时静默回落 DOM。
  // deps 含 terminalEpoch：会话重建（服务器切换）时 isActive 不变，靠纪元触发重挂。
  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    if (!isActive) {
      if (webglAddonRef.current) {
        try { webglAddonRef.current.dispose(); } catch { /* 已随终端释放 */ }
        webglAddonRef.current = null;
        log.debug('WebGL 渲染器已卸载（页签失活，回落 DOM 渲染）');
      }
      return;
    }
    if (webglAddonRef.current) return; // 已挂载
    try {
      const addon = new WebglAddon();
      // GPU 上下文丢失（驱动重置/显存回收）：卸载回落 DOM，避免持续异常
      addon.onContextLoss(() => {
        log.warn('WebGL 上下文丢失，回落 DOM 渲染');
        try { addon.dispose(); } catch { /* 已释放 */ }
        if (webglAddonRef.current === addon) webglAddonRef.current = null;
      });
      terminal.loadAddon(addon);
      webglAddonRef.current = addon;
      log.debug('WebGL 渲染器已挂载');
    } catch (e) {
      // WebGL 不可用（上下文耗尽/驱动问题/无头环境）：静默回落 DOM 渲染器
      log.warn('WebGL 渲染器挂载失败，回落 DOM:', e);
      webglAddonRef.current = null;
    }
  }, [isActive, terminalEpoch]);

  // ── 最小化闸门：恢复可见时冲刷缓冲输出 ──────────────────────────
  // 冲刷函数幂等（空缓冲 no-op），会话存续期间始终注册；会话销毁时被置空
  useEffect(() => {
    if (!osMinimized) {
      try { flushOutputRef.current?.(); } catch (e) { log.debug('冲刷缓冲输出失败:', e); }
    }
  }, [osMinimized]);

  // ── fontSize / cursorBlink 变更 ───────────────────────
  useEffect(() => {
    const t = terminalRef.current;
    if (!t) return;
    t.options.fontSize = fontSize;
    // 最小化期间强制关闭光标闪烁（Tier1 闸门：停止无意义渲染）
    t.options.cursorBlink = cursorBlink && !osMinimized;
    try { fitAddonRef.current?.fit(); } catch (e) { log.debug('fit 调整失败:', e); }
  }, [fontSize, cursorBlink, osMinimized]);

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
  initialCommand?: string | null,
  // Tier1 最小化闸门：最小化状态 ref + 缓冲/冲刷 ref（会话级缓冲挂在组件上）
  minimizedRef?: React.MutableRefObject<boolean>,
  pendingRef?: React.MutableRefObject<PendingOutput | null>,
  flushRef?: React.MutableRefObject<(() => void) | null>,
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
  terminal.write(`\x1b[1;32m${i18n.t('terminal.connectedBanner')}\x1b[0m\r\n`);
  terminal.write(i18n.t('terminal.connectedServer', {
    name: `\x1b[1;34m${serverName || serverHost}:${serverPort || 8443}\x1b[0m`,
  }) + '\r\n');
  terminal.write(`Session: ${sessionId}\r\n\r\n`);

  // 3. 监听终端输出事件
  // Tier1 最小化闸门：最小化期间不 write（不触发渲染），只入缓冲；恢复可见时
  // 由恢复 effect 或新数据到达路径按序冲刷。超 16MB 丢最旧并插入省略提示行。
  if (pendingRef) {
    pendingRef.current = { chunks: [], bytes: 0, overflowed: false };
  }
  // 幂等冲刷（空缓冲 no-op）：会话存续期间始终注册在 flushRef 上，
  // 恢复 effect 与监听器路径共用；teardownNow 销毁会话时置空
  const flushPendingOutput = () => {
    const pending = pendingRef?.current;
    if (!pending || pending.chunks.length === 0) return;
    if (pending.overflowed) {
      terminal.write(`\r\n\x1b[33m${i18n.t('terminal.outputBufferOverflow')}\x1b[0m\r\n`);
    }
    for (const chunk of pending.chunks) {
      terminal.write(chunk);
    }
    if (pendingRef) {
      pendingRef.current = { chunks: [], bytes: 0, overflowed: false };
    }
  };
  if (flushRef) {
    flushRef.current = flushPendingOutput;
  }

  const unlistenOutput = await listen<{ session_id: string; data: number[] }>(
    'terminal-output',
    (event) => {
      if (event.payload.session_id !== sessionId) return;
      const bytes = new Uint8Array(event.payload.data);
      if (minimizedRef?.current && pendingRef?.current) {
        // 最小化闸门：只缓冲不渲染，恢复时按序冲刷
        const pending = pendingRef.current;
        pending.chunks.push(bytes);
        pending.bytes += bytes.length;
        if (pending.bytes > MAX_PENDING_OUTPUT_BYTES) {
          // 丢最旧直到回到限内（至少保留最新一块）
          while (pending.bytes > MAX_PENDING_OUTPUT_BYTES && pending.chunks.length > 1) {
            pending.bytes -= pending.chunks.shift()!.length;
          }
          pending.overflowed = true;
        }
        return;
      }
      // 常规路径：先冲刷残留缓冲（恢复瞬间到数据的竞态兜底），保证输出顺序
      flushPendingOutput();
      terminal.write(bytes);
    }
  );
  unlistenRefs.current.push(unlistenOutput);

  // 4. 监听终端断连事件
  const unlistenDisconnect = await listen<{ session_id: string }>(
    'terminal-disconnected',
    (event) => {
      if (event.payload.session_id === sessionId) {
        terminal.write(`\r\n\x1b[31m${i18n.t('terminal.disconnectedBanner')}\x1b[0m\r\n`);
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

  // 7. 注入初始命令（脚本运行：PTY stdin 有内核缓冲，shell 就绪后自然读走，无需等待）
  if (initialCommand) {
    const bytes = new TextEncoder().encode(initialCommand + '\n');
    invoke('remote_terminal_write', {
      sessionId: sessionId,
      data: Array.from(bytes),
      serverId: serverId,
    }).catch(e => log.warn('初始命令写入失败:', e));
  }

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
    help: () => terminal.write(`\r\n${i18n.t('terminal.demoCommands')}\r\n`),
    ls: () => terminal.write('\r\nDesktop  Documents  Downloads  Music  Pictures  Videos  Projects\r\n'),
    pwd: () => terminal.write(`\r\n/home/${username}${cwd === '~' ? '' : '/' + cwd}\r\n`),
    whoami: () => terminal.write(`\r\n${username}\r\n`),
    date: () => terminal.write(`\r\n${new Date().toString()}\r\n`),
    clear: () => terminal.clear(),
    exit: () => terminal.write(`\r\n${i18n.t('terminal.demoExit')}\r\n`),
  };

  terminal.write(i18n.t('terminal.demoHelpHint'));
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
    /** 窗口创建后自动执行的命令（脚本运行：双击 .sh 由 FileManager 注入） */
    initialCommand?: string;
  };
}

export function TerminalApp({ windowId, preloadData }: TerminalAppProps) {
  const { t } = useTranslation();
  // ── 窗口系统集成 ────────────────────────────────────────────────────
  // 获取窗口状态（title、position、size、focused 等）
  // 用于窗口系统集成，确保每个窗口实例正确连接到窗口管理器
  // const windowState = useWindowState(windowId); // 未来集成时使用
  const { activeServer } = useServerManager();
  const activeServerId = activeServer?.id || null;

  // ── 工作目录配置 ────────────────────────────────────────────────────
  // 优先使用 preloadData（从文件管理器打开），否则使用用户配置的默认路径
  const [configuredDefaultPath, setConfiguredDefaultPath] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const { getSettingsStorage } = await import('../utils/storage');
      const storage = await getSettingsStorage();
      const value = await storage.getItem("terminal-default-path");
      if (!cancelled) setConfiguredDefaultPath(value || null);
    })();
    return () => { cancelled = true; };
  }, []);

  const workingDirectory = preloadData?.workingDirectory || configuredDefaultPath;

  // ── 窗口实例独立状态（每个窗口实例有自己的 tabs、设置等）────────────
  const [tabs, setTabs] = useState<TerminalTabMeta[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);

  // 页签条挂载点：窗口标题栏槽位（Chrome 形态——标题栏即标签条）。
  // WindowShell 的 HeaderBar 先于应用内容渲染，挂载时槽位必已存在；
  // 找不到时（异常形态）回退到应用内。
  const [titlebarSlot, setTitlebarSlot] = useState<HTMLElement | null>(null);
  useEffect(() => {
    setTitlebarSlot(document.querySelector<HTMLElement>(`[data-titlebar-slot="${windowId}"]`));
  }, [windowId]);
  const [showSearch, setShowSearch] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [fontSize, setFontSize] = useState(14);
  const [cursorBlink, setCursorBlink] = useState(true);
  const [searchText, setSearchText] = useState('');

  // ── OS 最小化闸门（Tier1）────────────────────────────────────
  // 透明窗口破坏 WebView2 遮挡检测：最小化后页面仍被判定"可见"，PTY 输出
  // 持续触发渲染吃 GPU/CPU。Rust 侧监听窗口 Resized + is_minimized，仅在
  // 状态翻转时发出 app-minimize-changed；本组件将状态传给各终端实例
  // （最小化期间输出只缓冲不渲染，恢复时冲刷）。全局 class 切换在 App.tsx。
  const [osMinimized, setOsMinimized] = useState(false);
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    listen<boolean>('app-minimize-changed', (event) => {
      if (!cancelled) setOsMinimized(event.payload === true);
    }).then((fn) => {
      // 竞态防护：监听建立前组件已卸载，立即注销
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // ── 活动终端的 refs（用于复制/粘贴/搜索）────────────────────────
  const activeTerminalRef = useRef<Terminal | null>(null);
  const activeSessionIdRef = useRef<string | null>(null);
  const activeSearchAddonRef = useRef<SearchAddon | null>(null);

  // ── 首次打开：创建第一个 tab ─────────────────────────
  // 记录初始 tab id：initialCommand 仅对它生效（新建 tab 不重复执行脚本）
  const initialTabIdRef = useRef<string | null>(null);
  useEffect(() => {
    if (tabs.length === 0) {
      const id = newTabId();
      initialTabIdRef.current = id;
      setTabs([{ id, label: t('terminal.tabLabel', { n: 1 }) }]);
      setActiveTabId(id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ── 新建 tab ─────────────────────────────────────────────────
  const handleNewTab = () => {
    const id = newTabId();
    setTabs((prev) => [...prev, { id, label: t('terminal.tabLabel', { n: nextTabNumber(prev) }) }]);
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

  // ── 页签条行：ChromeTabStrip + 行内附件（连接状态、⌄ 菜单）──
  // portal 进窗口标题栏（Chrome 形态，Windows Terminal 式行内附件）；
  // 槽位缺失时（异常形态）回退渲染在应用内
  const renderTabRow = () => (
    <ChromeTabStrip
      variant="dark"
      tabs={tabs.map((t) => ({ id: t.id, label: t.label, dirty: false }))}
      activeTabId={activeTabId ?? tabs[0]?.id ?? ''}
      onActivate={(id) => {
        setActiveTabId(id);
      }}
      onClose={(id) => handleCloseTab(id)}
      onAdd={handleNewTab}
      onReorder={(from, to) => {
        setTabs((prev) => {
          const arr = [...prev];
          const [moved] = arr.splice(from, 1);
          arr.splice(to, 0, moved);
          return arr;
        });
      }}
    >
      {/* 低频操作收纳进 ⌄ 菜单：搜索/设置面板的开关，开启时菜单项打勾。
          （标题栏连接状态小点已移除：底部状态栏已有完整连接信息，
          标题栏圆点是遗留重复） */}
      <ToolbarMenu
        icon="⌄"
        title={t('terminal.moreActions')}
        items={[
          { label: t('terminal.menuSearch'), checked: showSearch, onClick: () => setShowSearch((v) => !v) },
          { label: t('terminal.menuSettings'), checked: showSettings, onClick: () => setShowSettings((v) => !v) },
        ]}
      />
    </ChromeTabStrip>
  );

  return (
    <div className="terminal-app" onClick={() => { if (contextMenu) setContextMenu(null); }}>
      {/* Tab Bar —— 标题栏即标签条 */}
      {titlebarSlot ? createPortal(renderTabRow(), titlebarSlot) : renderTabRow()}

      {/* Search Bar */}
      {showSearch && (
        <div className="terminal-search-bar">
          <span className="terminal-search-label">{t('terminal.searchLabel')}</span>
          <input
            className="terminal-search-input"
            value={searchText}
            onChange={(e) => setSearchText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleSearch();
            }}
            autoFocus
            placeholder={t('terminal.searchPlaceholder')}
          />
          <button className="terminal-search-button" onClick={handleSearchPrev} title={t('terminal.searchPrev')}>↑</button>
          <button className="terminal-search-button" onClick={handleSearchNext} title={t('terminal.searchNext')}>↓</button>
          <button className="terminal-search-button" onClick={handleSearch}>{t('terminal.searchFind')}</button>
          <button className="terminal-search-button" onClick={() => setShowSearch(false)}>{t('terminal.close')}</button>
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
              isActive={tab.id === activeTabId}
              osMinimized={osMinimized}
              initialCommand={tab.id === initialTabIdRef.current ? (preloadData?.initialCommand ?? null) : null}
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
            <div className="empty-text">{t('terminal.emptyText')}</div>
            <div className="empty-hint">{t('terminal.emptyHint')}</div>
          </div>
        )}
      </div>

      {/* Status Bar - 底部状态栏（对齐 FileManager 状态栏样式）：
          左组=连接信息（服务器/host:port/会话数），右组=显示设置（字号） */}
      <div className="terminal-status-bar">
        <div className="terminal-status-group">
          {activeServer ? (
            <>
              <span className="terminal-status-item">
                <span className="terminal-status-dot on" />
                {activeServer.name}
              </span>
              <span className="terminal-status-item terminal-status-mono">
                {activeServer.host}:{activeServer.port}
              </span>
              <span className="terminal-status-item">{t('terminal.sessionCount', { count: tabs.length })}</span>
            </>
          ) : (
            <span className="terminal-status-item">
              <span className="terminal-status-dot off" />
              {t('terminal.demoLocalStatus')}
            </span>
          )}
        </div>
        <div className="terminal-status-group">
          <span className="terminal-status-item terminal-status-mono">{fontSize}px</span>
        </div>
      </div>

      {/* Settings Panel */}
      {showSettings && (
        <div className="terminal-settings-panel" onClick={(e) => e.stopPropagation()}>
          <label className="terminal-settings-label">
            {t('terminal.fontSizeLabel')}
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
            {t('terminal.cursorBlinkLabel')}
            <input
              type="checkbox"
              className="terminal-settings-checkbox"
              checked={cursorBlink}
              onChange={(e) => setCursorBlink(e.target.checked)}
            />
          </label>
          <button className="terminal-settings-close-button" onClick={() => setShowSettings(false)}>{t('terminal.close')}</button>
        </div>
      )}

      {/* Context Menu（只在未选中时显示，提供粘贴选项）*/}
      {contextMenu && (
        <div className="terminal-context-menu" style={{ left: contextMenu.x, top: contextMenu.y }} onClick={(e) => e.stopPropagation()}>
          <button className="terminal-menu-item" onClick={handlePaste}>{t('terminal.paste')}</button>
          <hr className="terminal-menu-divider" />
          <button className="terminal-menu-item" onClick={() => { setShowSearch(true); setContextMenu(null); }}>{t('terminal.menuSearch')}</button>
          <button className="terminal-menu-item" onClick={() => { setShowSettings(true); setContextMenu(null); }}>{t('terminal.menuSettings')}</button>
        </div>
      )}
    </div>
  );
}

export default Terminal