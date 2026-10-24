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
import { WINDOW_DRAG_EVENT } from "../components/window-shell/WindowShell";
import { useOpenApp } from "../window-system/hooks/useOpenApp";
import { useWindowState } from "../window-system/WindowManagerContext";
import { useBrowserTabs } from "./browser/useBrowserTabs";
import { useBrowserBounds, insetWebviewRect } from "./browser/useBrowserBounds";
import { BrowserNavbar, type BrowserSettings } from "./browser/BrowserNavbar";
import "./BrowserApp.css";

const log = createLogger("BrowserApp");

/** 模块级挂载序号：StrictMode（dev）假卸载守卫的判据（见关窗 cleanup 注释） */
let browserMountSeq = 0;

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
  const { tabs, activeId } = useBrowserTabs(connected ? serverId : null);
  const activeTab = tabs.find((tab) => tab.id === activeId) || null;

  // 标签条挂载点：窗口标题栏槽位（Chrome 形态——标题栏即标签条，TextEditor 同款）
  const [titlebarSlot, setTitlebarSlot] = useState<HTMLElement | null>(null);
  useEffect(() => {
    setTitlebarSlot(document.querySelector<HTMLElement>(`[data-titlebar-slot="${windowId}"]`));
  }, [windowId]);

  // 窗口状态：激活态同步 webview 可见性；几何变化触发 bounds 上报
  const win = useWindowState(windowId);
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
              // 恢复可见性按统一条件计算：浮层/菜单若恰好开着则保持隐藏，
              // bounds 已先行归位，后续状态变化经可见性 effect 补发
              invoke("browser_set_visible", {
                serverId,
                visible: win.isActive && !navbarOverlayOpen && !overlayOpen,
              }).catch(() => {});
            });
        }
      }
    }
    draggingPrevRef.current = dragging;
  }, [dragging, connected, serverId, navbarOverlayOpen, overlayOpen, win.isActive]);

  useBrowserBounds(
    connected ? serverId : null,
    contentRef,
    { x: win.position.x, y: win.position.y, width: win.size.width, height: win.size.height },
    connected && win.isActive,
    sessionEpoch,
  );

  // 诊断：未连接占位/webview 隐藏问题定位（输出当前分支判定依据，
  // 一次窗口打开对应一条；问题闭环后可整块移除）
  useEffect(() => {
    log.info(
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
      log.info(`高度链(content→上): ${chain.join(" ← ")}`);
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
  // ⚠️ StrictMode（dev）双挂载守卫：首次挂载的 unmount cleanup 会在第二次
  // 挂载之后才执行——若不区分真伪卸载，cleanup 发出的 browser_close_window
  // 会销毁刚建立的同 server 会话，第二次挂载的 open 又复用销毁中的 host，
  // 最终 webview 全灭（占位恒显、无法浏览）。守卫：cleanup 延迟一拍，
  // 期间若出现更新的挂载序号（新实例接管）则跳过销毁；真实关窗序号不变，
  // 销毁照常执行，无泄漏（Terminal.tsx 会话保活同款思路）。
  useEffect(() => {
    const mySeq = ++browserMountSeq;
    return () => {
      // cleanup 时刻读取：窗口可能先开、后连接（挂载时 ref 尚为 null）
      const sid = activeServerIdRef.current;
      if (!sid) return;
      window.setTimeout(() => {
        if (browserMountSeq !== mySeq) return;
        invoke("browser_close_window", { serverId: sid })
          .then(() => log.info("浏览器窗口已关闭"))
          .catch((e) => log.error("关闭浏览器窗口失败:", e));
      }, 0);
    };
  }, []);

  // 全局浮层收集：各浮层根元素带 data-browser-overlay 标记（全部条件渲染，
  // 仅打开时存在于 DOM），MutationObserver 监听 body 子树挂载/卸载判定存在性。
  // 挂载时先查一次——浮层先开、浏览器窗口后开的时序也要正确让位
  useEffect(() => {
    const sync = () => setOverlayOpen(!!document.querySelector("[data-browser-overlay]"));
    sync();
    const mo = new MutationObserver(sync);
    mo.observe(document.body, { childList: true, subtree: true });
    return () => mo.disconnect();
  }, []);

  // 窗口激活态 → webview 可见性（child HWND 无法被 React z-index 遮挡）；
  // sessionEpoch：browser_open 就绪后补发一次，恢复 suspend 期间被
  // 置 false 的 runtime.window_visible（重连场景唯一恢复途径）；
  // dragging：拖拽中强制隐藏（bounds 链路滞后会画面分离），松手即恢复；
  // navbarOverlayOpen：导航行下拉（设置菜单/联想词）展开期间隐藏，露出 React 飞出面板；
  // overlayOpen：全局浮层打开期间隐藏让位（通知中心/对话框/总览/到达提示）
  useEffect(() => {
    if (!connected || !serverId) return;
    // 拖拽恢复帧：显示由上方"先归位后显示"串行流程接管，本轮跳过，
    // 否则可见性先于 bounds 到达 runtime，webview 在旧位置闪现
    if (restorePendingRef.current) {
      restorePendingRef.current = false;
      return;
    }
    invoke("browser_set_visible", { serverId, visible: win.isActive && !dragging && !navbarOverlayOpen && !overlayOpen }).catch(() => {});
  }, [connected, serverId, win.isActive, sessionEpoch, dragging, navbarOverlayOpen, overlayOpen]);

  // 导航行下拉开合上报（Navbar 经由 effect 上报；仅落本地态，可见性由
  // 上方统一计算下发，避免与激活/拖拽恢复互相覆盖）
  const handleNavbarOverlayOpen = useCallback((open: boolean) => {
    setNavbarOverlayOpen(open);
  }, []);

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
    if (serverId) invoke("browser_close_tab", { serverId, tabId: Number(id) }).catch(() => {});
  }, [serverId]);
  const focusTab = useCallback((id: string) => {
    if (serverId) invoke("browser_focus_tab", { serverId, tabId: Number(id) }).catch(() => {});
  }, [serverId]);
  const reorderTab = useCallback((from: number, to: number) => {
    if (serverId) invoke("browser_reorder_tab", { serverId, from, to }).catch(() => {});
  }, [serverId]);

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
    />
  );

  // ── 导航行 ────────────────────────────────────────────
  const renderNavbar = () => (
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
    </div>
  );
}
