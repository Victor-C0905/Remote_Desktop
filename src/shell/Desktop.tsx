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
import { useSettingsStore } from "../stores/settingsStore";
import { useTheme } from "../hooks/useTheme";
import { DraggableWindow } from "../components/DraggableWindow";
import { WindowManagerProvider, useWindowManager } from "../window-system/WindowManagerContext";
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

export function Desktop() {
  return (
    <ServerManagerProvider>
      <WallpaperProvider>
        <WindowManagerProvider>
          <DesktopContent />
        </WindowManagerProvider>
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

  const { activeServer } = useServerManager();
  const { wallpaper } = useWallpaper();
  const manager = useWindowManager();

  // 全局主题应用（确保所有窗口都使用正确的主题 CSS 变量）
  const { themeId, accentColorId } = useSettingsStore();
  useTheme(themeId, accentColorId);

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

    // GNOME 标准：如果窗口已存在，点击 Dock 恢复时自动置顶
    const existingWindows = manager.getByAppId(appId);
    if (existingWindows.length > 0) {
      const existing = existingWindows[0];
      manager.focus(existing.id);
      return;
    }

    // FileManager 需要预加载
    if (appId === 'files') {
      try {
        await preloader.preload(activeServer?.id ?? null);
        await manager.create(appId, {
          serverId: activeServer?.id,
          preloadData: preloader.data,
        });
      } catch {
        console.error("[Desktop] FileManager 预加载失败");
        // 创建窗口但标记为错误状态
        const win = await manager.create(appId, { serverId: activeServer?.id });
        win.setPreloadState('error');
      }
    } else {
      // 其他应用：直接创建
      await manager.create(appId, { serverId: activeServer?.id });
    }
  }, [manager, preloader, activeServer?.id, setOverviewVisible]);

  // Global shortcuts
  const shortcuts = createAppShortcuts(
    openApp,
    () => setOverviewVisible(v => !v),
    () => setOverviewVisible(false),
    () => setNotificationOpen(false)
  );
  useGlobalShortcuts(shortcuts);

  // Render app content（根据 preloadState 决定展示真实组件还是骨架屏）
  const renderAppContent = (appId: string, win: ReturnType<typeof manager.getById>) => {
    if (!win) return null;

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
                    win.setPreloadData(preloader.data);
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
        return <FileManager windowId={win.id} preloadData={win.preloadData} />;
      case 'terminal':
        return <TerminalApp windowId={win.id} />;
      case 'monitor':
        return <SystemMonitor windowId={win.id} />;
      case 'settings':
        return <Settings windowId={win.id} />;
      default:
        return <div>Unknown app</div>;
    }
  };

  // 获取所有窗口（由 WindowManager 管理）
  const windows = manager.getAll();
  const activeWindow = manager.getActive();

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
        */}
        {(() => {
          const visibleWindows = windows.filter(w => !w.minimized);

          // 不重新排序，保持 DOM 树顺序不变，避免事件丢失
          return visibleWindows.map((win, index) => {
            const app = manager.getApp(win.appId);
            if (!app) return null;

            return (
              <DraggableWindow
                key={win.id}
                title={app.title}
                isActive={win.id === activeWindow?.id}
                onClose={() => manager.close(win.id)}
                onMinimize={() => manager.minimize(win.id)}
                onFocus={() => manager.focus(win.id)}
                initialPosition={win.position}
                initialSize={win.size}
                minWidth={app.minSize.width}
                minHeight={app.minSize.height}
                zIndex={win.id === activeWindow?.id ? 90 : 10 + index * 10}
              >
                {renderAppContent(win.appId, win)}
              </DraggableWindow>
            );
          });
        })()}

        {/* Dock */}
        <div className="dock-container">
          <div className="dock">
            {DOCK_APPS.map((app) => {
              const isOpen = manager.getByAppId(app.id).length > 0;
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
