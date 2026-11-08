// src/apps/BrowserApp.tsx
// 内置浏览器（WebView2 嵌入 Quirel 桌面）——Edge/Chrome 布局：
// 标题栏 = 标签条（ChromeTabStrip portal，TextEditor/Terminal 同款集成），
// 第二行 = 导航 + 地址栏（AppLayout toolbar），content = webview child HWND。
//
// 窗口非激活时 webview 隐藏（child HWND 无法被 React z-index 遮挡，spec 第 4 节）；
// 流量走 SOCKS5 → QUIC 隧道（服务器视角浏览，不变量见 spec 第 5 节）。

import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { useServerManager } from "../context/ServerManager";
import { useSettingsStore } from "../stores/settingsStore";
import { createLogger } from "../utils/logger";
import { SymbolicIcon } from "../components/symbolic";
import { ConnectionErrorState, ReconnectingState } from "../components/ConnectionState";
import { getServerErrorInfo } from "../stores/serversStore";
import { formatUnknownError } from "../types/errors";
import { AppLayout } from "../components/app-shell/AppLayout";
import { ChromeTabStrip } from "../components/ChromeTabStrip/ChromeTabStrip";
import { ContextMenu, type ContextMenuEntry } from "../components/ContextMenu/ContextMenu";
import { WINDOW_DRAG_EVENT } from "../components/window-shell/WindowShell";
import { useOpenApp } from "../window-system/hooks/useOpenApp";
import { useWindowManager, useWindowState } from "../window-system/WindowManagerContext";
import { useBrowserTabs } from "./browser/useBrowserTabs";
import { useBrowserBounds, insetWebviewRect } from "./browser/useBrowserBounds";
import { BrowserNavbar, type BrowserSettings } from "./browser/BrowserNavbar";
import { BookmarksBar } from "./browser/BookmarksBar";
import { useHistoryStore } from "../stores/historyStore";
import "./BrowserApp.css";

const log = createLogger("BrowserApp");

/** 代理会话信息（Rust 端 ProxySessionInfo） */
interface ProxySessionInfo {
  port: number;
}

interface BrowserAppProps {
  windowId: string;
}

export function BrowserApp({ windowId }: BrowserAppProps) {
  const { t } = useTranslation();
  const { activeServer, servers, connectServer } = useServerManager();
  const openApp = useOpenApp();
  // 睡眠超时（分钟，0=永不；设置 → 浏览器）
  const sleepMinutes = useSettingsStore((s) => s.browserSleepMinutes);
  // SOCKS5 固定端口（设置 → 浏览器；0 = 随机端口）
  const proxyPort = useSettingsStore((s) => s.browserProxyPort);

  // 会话信息（Rust ProxySessionInfo）：当前无消费方，仅保留写入语义；
  // noUnusedLocals 禁止未读取绑定，故只解构 setter（后续展示会话状态时恢复读取位）
  const [, setSession] = useState<ProxySessionInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const activeServerIdRef = useRef<string | null>(null);
  // 会话纪元：browser_open 成功后递增。host 创建是异步慢操作（runtime 线程 +
  // Environment + 首个 webview），就绪前发出的 browser_set_visible /
  // browser_set_bounds 会被后端 host_of 静默丢弃且无重发机制（rAF 与
  // ResizeObserver 初始回调都早于就绪）——webview 将停留在初始 800×600
  // 或保持隐藏（suspend→重连场景 runtime.window_visible=false 无人恢复），
  // 表现为"无法浏览 / 占位提示常显"。以纪元递增驱动两个 effect 在 host
  // 就绪后补发，保证 webview 精确铺满 content 区且可见性正确。
  const [sessionEpoch, setSessionEpoch] = useState(0);
  // 浏览器设置（随服务器持久化于 Rust 侧 settings.json；此处为展示缓存）
  const [settings, setSettings] = useState<BrowserSettings | null>(null);
  // 会话重启纪元：无痕切换等需要"关窗重开"的操作递增，驱动 open effect 重跑
  const [restartEpoch, setRestartEpoch] = useState(0);
  // 导航行下拉展开中（设置菜单或地址栏联想词任一）：展开期间隐藏 webview
  // 露出 React 面板（child HWND 无法被 React z-index 遮挡，与窗口失活/拖拽
  // 同款机制），关闭即恢复
  const [navbarOverlayOpen, setNavbarOverlayOpen] = useState(false);
  // 全局浮层存在中（通知中心/模态对话框/桌面总览/到达提示）：child HWND 无法
  // 被 DOM z-index 遮挡（airspace），浮层出现即隐藏 webview 让位，关闭后按
  // 激活态恢复；由下方 MutationObserver 驱动
  const [overlayOpen, setOverlayOpen] = useState(false);

  const connected = activeServer?.status === "connected";
  const serverId = activeServer?.id ?? null;
  const { tabs, activeId, closedTabs, setClosedTabs } = useBrowserTabs(connected ? serverId : null);
  const activeTab = tabs.find((tab) => tab.id === activeId) || null;

  // 标签条挂载点：窗口标题栏槽位（Chrome 形态——标题栏即标签条，TextEditor 同款）
  const [titlebarSlot, setTitlebarSlot] = useState<HTMLElement | null>(null);
  useEffect(() => {
    setTitlebarSlot(document.querySelector<HTMLElement>(`[data-titlebar-slot="${windowId}"]`));
  }, [windowId]);

  // 窗口状态：激活态同步 webview 可见性；几何变化触发 bounds 上报
  const win = useWindowState(windowId);
  // 窗口管理器：最后一个页签关闭时经此关闭窗口（与终端/编辑器同语义）
  const { manager } = useWindowManager();
  const contentRef = useRef<HTMLDivElement>(null);

  // 拖拽中隐藏 webview：拖拽时 DOM 走 transform 即时位移，而 webview bounds
  // 经 gBCR→invoke→runtime→SetBounds 链路更新（百毫秒级滞后），不隐藏则
  // "画面与窗口分离"。WindowShell 拖拽开始/结束广播（WINDOW_DRAG_EVENT），
  // 按 windowId 过滤；content 区占位字样由 .br-dragging 一并隐藏，露出纯背景
  const [dragging, setDragging] = useState(false);
  const draggingPrevRef = useRef(false);
  // 拖拽结束恢复帧标记：可见性 effect 见到它跳过一次（显示由下方串行流程接管）
  const restorePendingRef = useRef(false);
  useEffect(() => {
    const onDrag = (e: Event) => {
      const d = (e as CustomEvent<{ windowId: string; dragging: boolean }>).detail;
      if (d?.windowId === windowId) setDragging(d.dragging);
    };
    window.addEventListener(WINDOW_DRAG_EVENT, onDrag);
    return () => window.removeEventListener(WINDOW_DRAG_EVENT, onDrag);
  }, [windowId]);

  // ── 可见性裁决的唯一入口 ────────────────────────────────
  // 所有路径（拖拽恢复 / 浮层开合 / 窗口激活态）都必须读"此刻最新"的状态
  // 再下发，禁止用创建闭包时的旧快照。反例（已修复的根因）：拖到顶端松手
  // 时，吸附预览在本次 commit 才从 DOM 移除，MutationObserver 尚未回调，
  // 恢复 effect 闭包里的 overlayOpen 仍是 true；它把 set_visible(false)
  // 挂在 set_bounds 的 IPC 往返之后，晚于"浮层清理"触发的 set_visible(true)
  // 落地，把 webview 又关回去——表现为最大化后页面持续隐藏。
  const visibleStateRef = useRef({
    connected,
    isActive: win.isActive,
    dragging,
    navbarOverlayOpen,
    overlayOpen,
  });
  visibleStateRef.current = {
    connected,
    isActive: win.isActive,
    dragging,
    navbarOverlayOpen,
    overlayOpen,
  };

  const sendVisible = useCallback(() => {
    if (!serverId) return;
    const s = visibleStateRef.current;
    // 拖拽/浮层期间必须隐藏，否则 child HWND 会盖住 React 层
    const visible =
      s.connected && s.isActive && !s.dragging && !s.navbarOverlayOpen && !s.overlayOpen;
    invoke("browser_set_visible", { serverId, visible }).catch(() => {});
  }, [serverId]);

  // 拖拽结束恢复：先归位（set_bounds）后显示（set_visible），同经一条 FIFO
  // 命令通道，顺序有保证。若不串行，可见性链路快于 bounds 链路（bounds 走
  // geometry effect + rAF），webview 会在旧 bounds 处闪现后再跳到新位置——
  // 即"画面原地闪过、再补偿到窗口上"的松手侧根因
  useEffect(() => {
    if (!dragging && draggingPrevRef.current && connected && serverId) {
      const r = contentRef.current?.getBoundingClientRect();
      if (r && r.width >= 1 && r.height >= 1) {
        // 与 useBrowserBounds 同口径内缩（边缘露出 DOM 圆角与 resize 手柄）
        const b = insetWebviewRect(r);
        if (b.width >= 1 && b.height >= 1) {
          restorePendingRef.current = true;
          invoke("browser_set_bounds", {
            serverId,
            x: b.x,
            y: b.y,
            width: b.width,
            height: b.height,
          })
            .catch(() => {})
            .finally(() => {
              // 必须按"此刻"的最新状态下发（不是触发拖拽时的旧快照）：
              // 浮层若恰好开着则保持隐藏，已关闭则恢复显示
              sendVisible();
            });
        }
      }
    }
    draggingPrevRef.current = dragging;
  }, [dragging, connected, serverId, sendVisible]);

  useBrowserBounds(
    // 断连期间也持续上报：webview 隐藏但 bounds 须跟随窗口移动，重连后
    // 无缝归位（host 不在册时指令被 Rust 侧静默丢弃，无副作用）
    serverId,
    contentRef,
    { x: win.position.x, y: win.position.y, width: win.size.width, height: win.size.height },
    win.isActive,
    sessionEpoch,
  );

  // 诊断：未连接占位/webview 隐藏问题定位（输出当前分支判定依据，
  // 一次窗口打开对应一条；问题闭环后可整块移除）
  useEffect(() => {
    log.debug(
      `浏览器分支判定: connected=${connected} serverId=${serverId} winIsActive=${win.isActive} ` +
        `winPos=${win.position.x},${win.position.y} winSize=${win.size.width}x${win.size.height} ` +
        `serverStatuses=${servers.map((s) => `${s.name}:${s.status}`).join(" / ")}`,
    );
    // 高度链探针：content 区 gBCR 高度 0 已实证（占位常显根因），逐层输出
    // 祖先链矩形定位断裂层（br-app → appLayout → appMain → appContent → br-content）
    const raf = requestAnimationFrame(() => {
      let el: HTMLElement | null = contentRef.current;
      const chain: string[] = [];
      while (el && chain.length < 8) {
        const r = el.getBoundingClientRect();
        const cls = String(el.className).split(" ").slice(0, 2).join(".");
        chain.push(`${cls || "(anon)"}=${Math.round(r.top)},${Math.round(r.height)}`);
        el = el.parentElement;
      }
      log.debug(`高度链(content→上): ${chain.join(" ← ")}`);
    });
    return () => cancelAnimationFrame(raf);
  }, [connected, serverId, win.isActive, windowId, win.size.height]);

  // 读取浏览器设置（随服务器持久化，Rust 侧按 server_id 定位 settings.json；
  // 断连/切换服务器时清空展示缓存，重连后重读）
  useEffect(() => {
    if (!connected || !serverId) {
      setSettings(null);
      return;
    }
    let cancelled = false;
    invoke<BrowserSettings>("browser_get_settings", { serverId })
      .then((s) => {
        if (!cancelled) setSettings(s);
      })
      .catch((e) => log.error("读取浏览器设置失败:", e));
    return () => {
      cancelled = true;
    };
  }, [connected, serverId]);

  // 打开/恢复会话（幂等）：挂载 + 断线重连 + 无痕切换重启（restartEpoch）后触发
  useEffect(() => {
    if (!connected || !serverId) return;
    if (serverId !== activeServerIdRef.current) {
      activeServerIdRef.current = serverId;
      setSession(null);
      setError(null);
    }
    let cancelled = false;
    (async () => {
      try {
        const info = await invoke<ProxySessionInfo>("browser_open", {
          serverId,
          url: null,
          sleepMinutes,
          // 固定端口透传（0/null = 随机端口），保证重启后固定端口承诺仍生效
          port: proxyPort > 0 ? proxyPort : null,
        });
        if (!cancelled) {
          setSession(info);
          // 成功即清错误横幅：此前瞬时失败（如连接注册期竞态，connection_not_found）
          // 的横幅若不清除，会在后续恢复成功后仍残留，与"浏览正常"自相矛盾（实测）
          setError(null);
          // host 已就绪：递增纪元，驱动可见性 / bounds effect 补发
          // （此前发出的同步指令均被 host_of 静默丢弃，见 sessionEpoch 注释）
          setSessionEpoch((n) => n + 1);
        }
      } catch (e) {
        if (!cancelled) setError(formatUnknownError(e));
        log.error("浏览会话打开失败:", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [connected, serverId, sleepMinutes, proxyPort, restartEpoch]);

  // 关闭窗口 → 只销毁 webview + Environment（监听保留，重开秒恢复）。
  // 会话生命周期归窗口系统所有（registry lifecycle.onAfterClose 全量停止），
  // 本组件的挂载/卸载/刷新对会话零影响（原子性，见 init.ts browser 注册项）。
  // 挂载后的 browser_open 负责收养（幂等：no-op 或原端口重建）。

  // 全局浮层收集：各浮层根元素带 data-browser-overlay 标记（全部条件渲染，
  // 仅打开时存在于 DOM），MutationObserver 监听 body 子树挂载/卸载判定存在性。
  // 挂载时先查一次——浮层先开、浏览器窗口后开的时序也要正确让位
  useEffect(() => {
    const sync = () => setOverlayOpen(!!document.querySelector("[data-browser-overlay]"));
    sync();
    // 回调合帧（审计 L10）：终端高输出/编辑器打字期间 body 子树高频变动，
    // 逐次全文档 querySelector 会持续占主线程——同一帧多次变动只查一次，
    // 且结果与上次相同则跳过 setState
    let raf: number | null = null;
    let last: boolean | null = null;
    const mo = new MutationObserver(() => {
      if (raf !== null) return;
      raf = requestAnimationFrame(() => {
        raf = null;
        const now = !!document.querySelector("[data-browser-overlay]");
        if (now !== last) {
          last = now;
          setOverlayOpen(now);
        }
      });
    });
    mo.observe(document.body, { childList: true, subtree: true });
    return () => {
      mo.disconnect();
      if (raf !== null) cancelAnimationFrame(raf);
    };
  }, []);

  // 窗口激活态 → webview 可见性（child HWND 无法被 React z-index 遮挡）；
  // ⚠️ 断连时也必须显式下发 visible=false：webview 是独立 HWND，不下发就
  // 保持最后的可见状态——离线占位显示期间 webview 悬浮在桌面上仍可交互
  // （实测）。host 不在册（会话未建/已关）时指令被 Rust 侧静默丢弃，无副作用。
  // sessionEpoch：browser_open 就绪后补发一次，恢复 suspend 期间被
  // 置 false 的 runtime.window_visible（重连场景唯一恢复途径）；
  // dragging：拖拽中强制隐藏（bounds 链路滞后会画面分离），松手即恢复；
  // navbarOverlayOpen：导航行下拉（设置菜单/联想词）展开期间隐藏，露出 React 飞出面板；
  // overlayOpen：全局浮层打开期间隐藏让位（通知中心/对话框/总览/到达提示）
  useEffect(() => {
    if (!serverId) return;
    // 拖拽恢复帧：显示由上方"先归位后显示"串行流程接管，本轮跳过，
    // 否则可见性先于 bounds 到达 runtime，webview 在旧位置闪现
    if (restorePendingRef.current) {
      restorePendingRef.current = false;
      return;
    }
    // 统一走单一裁决入口（读最新状态，杜绝旧闭包快照）
    sendVisible();
  }, [connected, serverId, win.isActive, sessionEpoch, dragging, navbarOverlayOpen, overlayOpen, sendVisible]);

  // 导航行下拉开合上报（Navbar 经由 effect 上报；仅落本地态，可见性由
  // 上方统一计算下发，避免与激活/拖拽恢复互相覆盖）
  const handleNavbarOverlayOpen = useCallback((open: boolean) => {
    setNavbarOverlayOpen(open);
  }, []);

  // ── 前端存活心跳（1s 间隔）：Rust runtime 的 webview 可见性强制执行凭证。
  // 心跳中断 >3s（前端死亡/重载/卡死）→ runtime 自行隐藏全部 webview
  // （悬浮孤儿根治——可见性不再依赖前端"礼貌地"发指令），恢复后自动重.assert
  useEffect(() => {
    if (!serverId) return;
    invoke("browser_heartbeat", { serverId }).catch(() => {});
    const timer = window.setInterval(() => {
      invoke("browser_heartbeat", { serverId }).catch(() => {});
    }, 1000);
    return () => window.clearInterval(timer);
  }, [serverId]);

  // ── 历史记录埋点：页签加载完成（loading=false 且带 URL）即记录；
  //    标题晚到经后续增量事件补齐（同 URL 重记 = 置顶刷新）。无痕会话豁免，
  //    也不读不写 historyStore
  useEffect(() => {
    if (!connected || settings?.incognito) return;
    let unlisten: (() => void) | null = null;
    let disposed = false;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const fn = await listen<{
        server_id: string;
        tab_id: number;
        url?: string;
        title?: string;
        loading?: boolean;
      }>("browser://page-state", (event) => {
        const p = event.payload;
        if (p.server_id !== serverId) return;
        if (p.loading !== false || !p.url) return;
        useHistoryStore.getState().record(p.url, p.title ?? "");
      });
      if (disposed) {
        fn();
        return;
      }
      unlisten = fn;
    })().catch((e) => log.error("历史记录监听失败:", e));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [connected, serverId, settings?.incognito]);

  // ── 操作 ──────────────────────────────────────────────
  const navigate = useCallback((url: string) => {
    if (serverId)
      invoke("browser_navigate", { serverId, url, tabId: activeId })
        .then(() => log.info("导航指令已送达:", url))
        .catch((e) => log.error("导航失败:", e));
  }, [serverId, activeId]);
  const back = useCallback(() => {
    if (serverId) invoke("browser_back", { serverId, tabId: activeId }).catch(() => {});
  }, [serverId, activeId]);
  const forward = useCallback(() => {
    if (serverId) invoke("browser_forward", { serverId, tabId: activeId }).catch(() => {});
  }, [serverId, activeId]);
  const reload = useCallback(() => {
    if (serverId) invoke("browser_reload", { serverId, tabId: activeId }).catch(() => {});
  }, [serverId, activeId]);
  const newTab = useCallback(() => {
    if (serverId) invoke("browser_create_tab", { serverId, url: null }).catch((e) => log.error("新建标签失败:", e));
  }, [serverId]);
  const closeTab = useCallback((id: string) => {
    // 最后一个页签关闭 → 关闭窗口（与终端/编辑器同语义）：直接走窗口关闭
    // 路径，cleanup 统一销毁 webview + Environment（代理监听保留），不再发
    // close_tab 避免 Rust 侧空 book 残留
    if (tabs.length <= 1) {
      manager.close(windowId);
      return;
    }
    if (serverId) invoke("browser_close_tab", { serverId, tabId: Number(id) }).catch(() => {});
  }, [serverId, tabs.length, manager, windowId]);
  const focusTab = useCallback((id: string) => {
    if (serverId) invoke("browser_focus_tab", { serverId, tabId: Number(id) }).catch(() => {});
  }, [serverId]);
  const reorderTab = useCallback((from: number, to: number) => {
    if (serverId) invoke("browser_reorder_tab", { serverId, from, to }).catch(() => {});
  }, [serverId]);

  // ── 页签右键菜单（ChromeTabStrip 宿主钩子透传；对齐通用浏览器）──
  const [tabMenu, setTabMenu] = useState<{ x: number; y: number; tabId: string } | null>(null);
  // 页签条空白区右键：新建 / 重新打开已关闭的标签页
  const [stripMenu, setStripMenu] = useState<{ x: number; y: number } | null>(null);

  const handleTabContextMenu = useCallback((tabId: string, event: MouseEvent) => {
    event.preventDefault();
    setTabMenu({ x: event.clientX + 3, y: event.clientY + 3, tabId });
  }, []);

  const handleStripContextMenu = useCallback((event: MouseEvent) => {
    event.preventDefault();
    setStripMenu({ x: event.clientX + 3, y: event.clientY + 3 });
  }, []);

  /** 重新打开最近关闭的标签页（LIFO 弹栈 → create_tab 携原 URL） */
  const reopenClosedTab = useCallback(() => {
    const tab = closedTabs[0];
    if (!tab || !serverId) return;
    invoke("browser_create_tab", { serverId, url: tab.url }).catch((e) => log.error("重新打开标签失败:", e));
    setClosedTabs((cs) => cs.slice(1));
  }, [closedTabs, serverId, setClosedTabs]);

  const tabMenuItems: ContextMenuEntry[] = (() => {
    if (!tabMenu) return [];
    const idNum = Number(tabMenu.tabId);
    const index = tabs.findIndex((t) => String(t.id) === tabMenu.tabId);
    const menuTab = index >= 0 ? tabs[index] : undefined;
    const closeTabs = (list: typeof tabs) => {
      // 循环复用既有 close_tab；最后一个页签关闭 = 关窗由 Rust 侧自然呈现
      if (serverId) list.forEach((t) => invoke("browser_close_tab", { serverId, tabId: t.id }).catch(() => {}));
    };
    const items: ContextMenuEntry[] = [
      { label: t("browser.reload"), onClick: () => { if (serverId) invoke("browser_reload", { serverId, tabId: idNum }).catch(() => {}); } },
    ];
    if (menuTab?.url && menuTab.url !== "about:blank") {
      items.push({ label: t("browser.copyPageUrl"), onClick: () => navigator.clipboard.writeText(menuTab.url ?? "") });
    }
    if (menuTab) {
      items.push({
        label: t("browser.ctxDuplicate"),
        onClick: () => { if (serverId) invoke("browser_create_tab", { serverId, url: menuTab.url }).catch(() => {}); },
      });
    }
    items.push({ separator: true });
    if (tabs.length > 1) {
      items.push({ label: t("browser.closeOthers"), onClick: () => closeTabs(tabs.filter((t) => String(t.id) !== tabMenu.tabId)) });
    }
    if (index >= 0 && index < tabs.length - 1) {
      items.push({ label: t("browser.closeRight"), onClick: () => closeTabs(tabs.slice(index + 1)) });
    }
    items.push({ separator: true });
    items.push({ label: t("browser.closeTab"), onClick: () => closeTab(tabMenu.tabId) });
    return items;
  })();

  const stripMenuItems: ContextMenuEntry[] = (() => {
    if (!stripMenu) return [];
    return [
      { label: t("browser.ctxReopenClosed"), disabled: closedTabs.length === 0, onClick: reopenClosedTab },
      { label: t("browser.newTab"), onClick: newTab },
    ];
  })();

  // ── 设置（持久化在 Rust 侧；无痕切换需关窗重开会话）────
  const changeSearchEngine = useCallback(
    (engine: BrowserSettings["searchEngine"]) => {
      if (!serverId || !settings || settings.searchEngine === engine) return;
      const prev = settings;
      setSettings({ ...settings, searchEngine: engine }); // 乐观更新（菜单即时反馈）
      invoke("browser_set_settings", {
        serverId,
        settings: { searchEngine: engine, incognito: settings.incognito },
      }).catch((e) => {
        log.error("切换搜索引擎失败:", e);
        setSettings(prev); // 失败回滚展示
      });
    },
    [serverId, settings],
  );

  const toggleIncognito = useCallback(async () => {
    if (!serverId || !settings) return;
    const next = !settings.incognito;
    try {
      // 先持久化再重开：browser_open 重建 host 时由 Rust 读 settings.json
      // 决定 Environment 用持久目录还是即弃无痕目录
      await invoke("browser_set_settings", {
        serverId,
        settings: { searchEngine: settings.searchEngine, incognito: next },
      });
      setSettings({ ...settings, incognito: next });
      // 无痕环境随 Environment 创建：关窗销毁旧 Environment（SOCKS5 监听
      // 保留），restartEpoch 驱动 open effect 走"窗口刚关→原端口重建"路径
      await invoke("browser_close_window", { serverId });
      setRestartEpoch((n) => n + 1);
    } catch (e) {
      log.error("切换无痕浏览失败:", e);
    }
  }, [serverId, settings]);

  // ── 标签条（ChromeTabStrip；tab_id number → ChromeTabItem.id string）──
  const renderTabStrip = () => (
    <ChromeTabStrip
      variant="light"
      tabs={tabs.map((tab) => ({
        id: String(tab.id),
        label: tab.title || tab.url || t("browser.newTab"),
        dirty: false,
      }))}
      activeTabId={activeId != null ? String(activeId) : ""}
      onActivate={focusTab}
      onClose={closeTab}
      onAdd={newTab}
      onReorder={reorderTab}
      onTabContextMenu={handleTabContextMenu}
      onStripContextMenu={handleStripContextMenu}
    />
  );

  // ── 导航行 + 书签栏（纵向堆叠占满 toolbar 槽）─────────
  const renderNavbar = () => (
    <div className="br-toolbar-stack">
      <BrowserNavbar
        activeTab={activeTab}
        serverId={serverId}
        incognito={settings?.incognito ?? false}
        searchEngine={settings?.searchEngine ?? "google"}
        onNavigate={navigate}
        onBack={back}
        onForward={forward}
        onReload={reload}
        onToggleIncognito={toggleIncognito}
        onSearchEngineChange={changeSearchEngine}
        onMenuOpenChange={handleNavbarOverlayOpen}
      />
      <BookmarksBar serverId={serverId} onNavigate={navigate} />
    </div>
  );

  // ── 未连接：占位提示（复用现有 ConnectionState 组件）──
  if (!connected) {
    const troubledServer =
      servers.find((s) => s.status === "reconnecting") ||
      servers.find((s) => s.status === "error") ||
      null;

    if (troubledServer) {
      return (
        <div className="br-app">
          <div className="br-empty">
            {troubledServer.status === "reconnecting" ? (
              <ReconnectingState serverName={troubledServer.name || troubledServer.host} />
            ) : (
              <ConnectionErrorState
                {...getServerErrorInfo(troubledServer)}
                onRetry={() => connectServer(troubledServer.id)}
                onOpenSettings={() => openApp("settings")}
              />
            )}
          </div>
        </div>
      );
    }

    return (
      <div className="br-app">
        <div className="br-empty">
          <div className="br-empty-icon">
            <SymbolicIcon name="network-offline" size={32} />
          </div>
          <div className="br-empty-title">{t("browser.notConnectedTitle")}</div>
          <div className="br-empty-desc">{t("browser.notConnectedDesc")}</div>
        </div>
      </div>
    );
  }

  // ── 浏览器本体 ────────────────────────────────────────
  // 启动画面判定：会话存活但首页首帧未渲染完成——激活 tab 不存在（host
  // 创建中）或 loading（首帧加载中）。webview 就绪后 child HWND 盖住 DOM，
  // 本层自然不可见（airspace）；刷新/切页时 loading 置位但因被 webview
  // 覆盖而无视觉影响。感知提速：打开浏览器即有品牌画面而非空白/离线占位。
  const booting = !error && (!activeTab || activeTab.loading);
  return (
    <div className={`br-app${dragging ? " br-dragging" : ""}`}>
      {/* 页签 portal 进窗口标题栏（Chrome 形态）；槽位缺失时回退应用内 */}
      {titlebarSlot ? createPortal(renderTabStrip(), titlebarSlot) : renderTabStrip()}
      <AppLayout toolbar={renderNavbar()}>
        {error && (
          <div className="br-error">
            <div className="br-error-title">{t("browser.errorTitle")}</div>
            <div className="br-error-desc">{error}</div>
          </div>
        )}
        <div className="br-content" ref={contentRef}>
          {/* webview child HWND 覆盖于此；占位仅在其隐藏时可见（窗口非激活 /
              窗口刚开尚未上报 bounds / 出错）。启动画面 = 首帧完成前的
              引擎品牌层，真实加载时间不变，仅消除空白等待感 */}
          {booting ? (
            <div className="br-boot">
              <div className={`br-boot-name br-boot-name--${settings?.searchEngine ?? "google"}`}>
                {settings?.searchEngine === "bing" ? "Bing" : "Google"}
              </div>
              <div className="br-boot-hint">{t("browser.booting")}</div>
            </div>
          ) : (
            <div className="br-content-placeholder">
              <SymbolicIcon name="network-offline" size={24} />
              <span>{t("browser.contentPlaceholder")}</span>
            </div>
          )}
        </div>
      </AppLayout>

      {/* 页签右键菜单（portal + data-browser-overlay：菜单弹出时 webview 让位） */}
      {tabMenu && (
        <ContextMenu
          pos={{ x: tabMenu.x, y: tabMenu.y }}
          items={tabMenuItems}
          onClose={() => setTabMenu(null)}
        />
      )}

      {/* 页签条空白区右键菜单 */}
      {stripMenu && (
        <ContextMenu
          pos={{ x: stripMenu.x, y: stripMenu.y }}
          items={stripMenuItems}
          onClose={() => setStripMenu(null)}
        />
      )}
    </div>
  );
}
