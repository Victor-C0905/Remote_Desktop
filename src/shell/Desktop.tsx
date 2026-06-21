import { useState, useEffect, useCallback } from "react";
import { FileManager } from "../apps/FileManager";
import { TerminalApp } from "../apps/Terminal";
import { SystemMonitor } from "../apps/SystemMonitor";
import { Settings } from "../apps/Settings";
import { NotificationCenter, NotificationBadge } from "./NotificationCenter";
import { useGlobalShortcuts, createAppShortcuts } from "../hooks/useGlobalShortcuts";
import { ServerManagerProvider, useServerManager, getStatusColor } from "../context/ServerManager";
import { formatBytesSafe, formatPercentSafe } from "../utils/offlineDefaults";
import { usePreloader } from "../hooks/usePreloader";
import { FileManagerSkeleton } from "../components/skeleton/FileManagerSkeleton";
import { WallpaperProvider, useWallpaper, getWallpaperStyle } from "../context/WallpaperContext";
import { DraggableWindow } from "../components/DraggableWindow";
import { listen } from "@tauri-apps/api/event";
import "./Desktop.css";

// 完整的 MetricsSnapshot 类型（匹配 Agent）
interface MetricsSnapshot {
  cpu_percent: number;
  mem_used_bytes: number;
  mem_total_bytes: number;
  swap_used_bytes: number;
  disks: Array<{
    mount_point: string;
    total_bytes: number;
    used_bytes: number;
  }>;
  network_rx_bytes: number;
  network_tx_bytes: number;
  uptime_secs: number;
}

interface DesktopApp {
  id: string;
  icon: string;
  label: string;
}

const DESKTOP_APPS: DesktopApp[] = [
  { id: "files",    icon: "📁", label: "远程文件" },
  { id: "terminal", icon: "🖥️", label: "远程终端" },
  { id: "monitor",  icon: "📊", label: "系统监控" },
  { id: "settings", icon: "⚙️",  label: "设置" },
];

const DOCK_APPS: DesktopApp[] = [
  { id: "files",    icon: "📁", label: "文件" },
  { id: "terminal", icon: "🖥️", label: "终端" },
  { id: "monitor",  icon: "📊", label: "监控" },
  { id: "settings", icon: "⚙️",  label: "设置" },
];

// Active window in the desktop
interface WindowState {
  id: string;
  appId: string;
  title: string;
  minimized: boolean;
  /** 预加载状态：loading=显示骨架屏, ready=数据就绪可渲染, error=加载失败 */
  preloadState: 'loading' | 'ready' | 'error';
  /** 预加载完成后的注入数据（仅 FileManager 使用） */
  preloadData: any;
  /** 窗口位置 */
  position: { x: number; y: number };
  /** 窗口大小 */
  size: { width: number; height: number };
  /** 激活时间戳，用于排序（最后激活的窗口在最上层） */
  activatedAt: number;
}

export function Desktop() {
  return (
    <ServerManagerProvider>
      <WallpaperProvider>
        <DesktopContent />
      </WallpaperProvider>
    </ServerManagerProvider>
  );
}

function DesktopContent() {
  const [overviewVisible, setOverviewVisible] = useState(false);
  const [notificationOpen, setNotificationOpen] = useState(false);
  const unreadNotifications = 2;
  const criticalNotifications = 1;
  const [metrics, setMetrics] = useState<MetricsSnapshot | null>(null);
  const [clock, setClock] = useState("");
  const [windows, setWindows] = useState<WindowState[]>([]);
  const [activeWindowId, setActiveWindowId] = useState<string | null>(null);

  const { activeServer } = useServerManager();
  const { wallpaper } = useWallpaper();

  // ── FileManager 预加载器 ──
  const preloader = usePreloader();

  // Clock
  useEffect(() => {
    const update = () => {
      const now = new Date();
      setClock(
        now.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })
      );
    };
    update();
    const id = setInterval(update, 10000);
    return () => clearInterval(id);
  }, []);

  // 监听系统指标事件（由 ServerManager 自动订阅）
  useEffect(() => {
    if (!activeServer?.id) return;

    const setupListener = async () => {
      const unlisten = await listen<{ server_id: string; event_type: string; data: MetricsSnapshot }>(
        'subscription_event',
        (event) => {
          if (event.payload.server_id === activeServer.id && event.payload.event_type === 'metrics') {
            setMetrics(event.payload.data);
          }
        }
      );
      return unlisten;
    };

    let unlistenFn: (() => void) | undefined;
    setupListener().then((fn) => {
      unlistenFn = fn;
    });

    return () => {
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [activeServer?.id]);

  // ── 断连时立即清空状态栏 metrics ────────────────────
  // 当 activeServer 消失（断连/切换服务器）时，同步清空 metrics 数据
  // 确保 TopBar 即时从 "CPU 23.5%" 切换到 "CPU —%"
  useEffect(() => {
    if (!activeServer?.id) {
      setMetrics(null);
    }
  }, [activeServer?.id]);

  const openApp = useCallback(async (appId: string) => {
    setOverviewVisible(false);

    const titles: Record<string, string> = {
      files: "文件管理器",
      terminal: "终端",
      monitor: "系统监控",
      settings: "设置",
    };

    // GNOME 标准：如果窗口已存在，点击 Dock 恢复时自动置顶
    if (windows.some((w) => w.appId === appId)) {
      const existing = windows.find((w) => w.appId === appId);
      if (existing) {
        // 恢复最小化窗口时，自动置顶（更新 activatedAt）
        setWindows((ws) =>
          ws.map((w) => (w.id === existing.id ? { ...w, minimized: false, activatedAt: Date.now() } : w))
        );
        setActiveWindowId(existing.id);
      }
      return;
    }

    // 计算初始位置（避免重叠）
    const existingWindows = windows.filter(w => !w.minimized);
    const offset = existingWindows.length * 30;

    // 获取应用默认尺寸
    const defaultSizes: Record<string, { width: number; height: number }> = {
      files: { width: 900, height: 650 },
      terminal: { width: 850, height: 550 },
      monitor: { width: 900, height: 650 },
      settings: { width: 700, height: 550 },
    };

    // 1. 先创建窗口框架（preloadState='loading'，显示骨架屏）
    const newWindow: WindowState = {
      id: `win-${appId}-${Date.now()}`,
      appId,
      title: titles[appId] || appId,
      minimized: false,
      preloadState: 'loading',
      preloadData: null,
      position: { x: 100 + offset, y: 100 + offset },
      size: defaultSizes[appId] || { width: 800, height: 600 },
      activatedAt: Date.now(), // 初始化激活时间戳
    };
    setWindows((ws) => [...ws, newWindow]);
    setActiveWindowId(newWindow.id);

    // 2. FileManager 走预加载通道：等待数据就绪后再渲染
    if (appId === 'files') {
      try {
        await preloader.preload(activeServer?.id ?? null);
        setWindows((ws) =>
          ws.map((w) =>
            w.id === newWindow.id
              ? { ...w, preloadState: 'ready', preloadData: preloader.data }
              : w
          )
        );
      } catch {
        console.error("[Desktop] FileManager 预加载失败");
        setWindows((ws) =>
          ws.map((w) =>
            w.id === newWindow.id
              ? { ...w, preloadState: 'error' }
              : w
          )
        );
      }
    } else {
      // 其他应用：无需父级预加载，直接标记 ready（组件内部自行处理骨架屏）
      setWindows((ws) =>
        ws.map((w) =>
          w.id === newWindow.id ? { ...w, preloadState: 'ready' } : w
        )
      );
    }
  }, [windows, preloader, activeServer?.id]);

  // Global shortcuts
  const shortcuts = createAppShortcuts(
    openApp,
    () => setOverviewVisible(v => !v),
    () => setOverviewVisible(false),
    () => setNotificationOpen(false)
  );
  useGlobalShortcuts(shortcuts);

  const closeWindow = (windowId: string) => {
    setWindows((ws) => ws.filter((w) => w.id !== windowId));
    if (activeWindowId === windowId) {
      setActiveWindowId(null);
    }
  };

  const minimizeWindow = (windowId: string) => {
    setWindows((ws) =>
      ws.map((w) => (w.id === windowId ? { ...w, minimized: true } : w))
    );
    if (activeWindowId === windowId) {
      setActiveWindowId(null);
    }
  };

  const focusWindow = (windowId: string) => {
    // GNOME 标准：点击窗口 → raise + focus
    // 恢复最小化窗口时，自动置顶（更新 activatedAt）
    setWindows((ws) =>
      ws.map((w) => (w.id === windowId ? { ...w, minimized: false, activatedAt: Date.now() } : w))
    );
    setActiveWindowId(windowId);
  };

  // Render app content（根据 preloadState 决定展示真实组件还是骨架屏）
  const renderAppContent = (appId: string, win: WindowState) => {
    switch (appId) {
      case 'files':
        if (win.preloadState === 'error') {
          return (
            <div className="fm" style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', flexDirection: 'column', gap: 12, height: '100%' }}>
              <div style={{ fontSize: 36 }}>⚠️</div>
              <div>数据加载失败</div>
              <button
                onClick={() => {
                  preloader.preload(activeServer?.id ?? null).then(() => {
                    setWindows(ws => ws.map(w =>
                      w.id === win.id ? { ...w, preloadState: 'ready', preloadData: preloader.data } : w
                    ));
                  });
                }}
                style={{ padding: '6px 16px', borderRadius: 8, cursor: 'pointer', border: '1px solid var(--border-color)' }}
              >重试</button>
            </div>
          );
        }
        if (win.preloadState !== 'ready') {
          return <FileManagerSkeleton />;
        }
        return <FileManager initialData={win.preloadData} />;
      case 'terminal':
        return <TerminalApp />;
      case 'monitor':
        return <SystemMonitor />;
      case 'settings':
        return <Settings />;
      default:
        return <div>Unknown app</div>;
    }
  };

  // Sort windows: based on activation order (last activated on top)
  // 窗口根据激活时间戳排序，最后激活的窗口在最上层
  const sortedWindows = [...windows].sort((a, b) => {
    // 按激活时间戳升序排序（最早激活的在前面，最近激活的在后面）
    // React 渲染顺序：前面的先渲染（底层），后面的后渲染（上层）
    const result = a.activatedAt - b.activatedAt;
    // 调试信息：查看窗口排序
    console.log(`[Desktop] 窗口排序: ${a.title}(${a.activatedAt}) vs ${b.title}(${b.activatedAt}) = ${result}`);
    return result;
  });

  // 调试信息：查看排序后的窗口顺序
  console.log('[Desktop] 排序后的窗口:', sortedWindows.map(w => `${w.title}(${w.activatedAt})`));
  console.log('[Desktop] 当前激活窗口:', activeWindowId);

  return (
    <div className="shell">
      {/* Top Bar */}
      <div className="top-bar" data-tauri-drag-region>
        <button
          className="activities-btn"
          onClick={() => setOverviewVisible((v) => !v)}
        >
          活动
        </button>
        <div className="separator" />
        <div className="connection-indicator">
          <div
            className="connection-dot"
            style={{ background: activeServer ? getStatusColor(activeServer.status) : "#9a9996" }}
          />
          <span>
            {activeServer?.status === "connected" ? "已连接" :
             activeServer?.status === "connecting" ? "连接中..." :
             activeServer?.status === "error" ? "连接失败" : "未连接"}
          </span>
          {activeServer && (
            <span style={{ marginLeft: "8px", opacity: 0.7 }}>
              {activeServer.name || activeServer.host}
            </span>
          )}
        </div>
        <div className="spacer" />
        <div className="metrics">
          <span>CPU {formatPercentSafe(metrics?.cpu_percent ?? 0, !metrics)}</span>
          <span>MEM {formatBytesSafe(metrics?.mem_used_bytes ?? 0, !metrics)} / {formatBytesSafe(metrics?.mem_total_bytes ?? 0, !metrics)}</span>
        </div>
        <div className="separator" />
        <span className="clock">{clock}</span>
        <button
          className="notification-btn"
          onClick={() => setNotificationOpen(true)}
          style={{ position: "relative" }}
        >
          🔔
          <NotificationBadge count={unreadNotifications} criticalCount={criticalNotifications} />
        </button>
      </div>

      {/* Desktop Area */}
      <div className="desktop-area" style={getWallpaperStyle(wallpaper)}>
        <div className="desktop-icons">
          {DESKTOP_APPS.map((app) => (
            <div
              key={app.id}
              className="desktop-icon"
              onDoubleClick={() => openApp(app.id)}
            >
              <div className="icon">{app.icon}</div>
              <div className="label">{app.label}</div>
            </div>
          ))}
        </div>

        {/* Application Windows */}
        {/* z-index 分配策略：
            - 活动窗口: z-index: 90（最上层）
            - 其他窗口: 根据激活时间戳排序，最近激活的在上层
            - 简化逻辑：直接根据 activatedAt 排序分配 z-index
        */}
        {/* Application Windows */}
        {/* z-index 分配策略（不重新排序，避免 DOM 移动导致事件丢失）：
            - activatedAt 最大值 → z-index: 90（最上层）
            - 其他窗口：根据原始顺序递增（10, 20, 30...）
        */}
        {(() => {
          const visibleWindows = windows.filter(w => !w.minimized);
          // 找出 activatedAt 最大的窗口（活动窗口）
          const maxActivatedAt = visibleWindows.length > 0 
            ? Math.max(...visibleWindows.map(w => w.activatedAt)) 
            : 0;
          
          // 不重新排序，保持 DOM 树顺序不变，避免事件丢失
          return visibleWindows.map((win, index) => (
            <DraggableWindow
              key={win.id}
              title={win.title}
              isActive={win.activatedAt === maxActivatedAt}
              onClose={() => closeWindow(win.id)}
              onMinimize={() => minimizeWindow(win.id)}
              onFocus={() => focusWindow(win.id)}
              initialPosition={win.position}
              initialSize={win.size}
              zIndex={win.activatedAt === maxActivatedAt ? 90 : 10 + index * 10}
            >
              {renderAppContent(win.appId, win)}
            </DraggableWindow>
          ));
        })()}

        {/* Dock */}
        <div className="dock-container">
          <div className="dock">
            {DOCK_APPS.map((app) => {
              const isOpen = windows.some((w) => w.appId === app.id);
              return (
                <div
                  key={app.id}
                  className={`dock-item${isOpen ? " running" : ""}`}
                  onClick={() => openApp(app.id)}
                >
                  <div className="icon">{app.icon}</div>
                  <div className="label">{app.label}</div>
                  {isOpen && <div className="dock-indicator" />}
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* Overview Overlay */}
      {overviewVisible && (
        <div className="overlay" onClick={(e) => {
          if (e.target === e.currentTarget) setOverviewVisible(false);
        }}>
          <input
            className="overview-search"
            type="text"
            placeholder="搜索应用..."
            autoFocus
          />
          <div className="overview-apps">
            {DESKTOP_APPS.map((app) => (
              <div
                key={app.id}
                className="overview-app"
                onClick={() => openApp(app.id)}
              >
                <div className="icon">{app.icon}</div>
                <div className="label">{app.label}</div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Notification Center */}
      <NotificationCenter
        isOpen={notificationOpen}
        onClose={() => setNotificationOpen(false)}
      />
    </div>
  );
}
