import { useState, useEffect, useCallback, memo, useRef } from "react";
import { NotificationCenter } from "./NotificationCenter";
import { TopBar } from "./TopBar/TopBar";
import { useGlobalShortcuts, createAppShortcuts } from "../hooks/useGlobalShortcuts";
import { ServerManagerProvider, useServerManager } from "../context/ServerManager";

import { WallpaperProvider, useWallpaper, getWallpaperStyle } from "../context/WallpaperContext";
import { useSettingsStore } from "../stores/settingsStore";
import { useTheme } from "../hooks/useTheme";
import { useContrastColor } from "../hooks/useContrastColor";
import { WindowShell } from "../components/window-shell";
import { WindowManagerProvider, useWindowManager, useWindowGlobalState, useWindowState } from "../window-system/WindowManagerContext";
import { AppDefinition } from "../window-system/types";
import { listen } from "@tauri-apps/api/event";
import "./Desktop.css";

// ─── MetricsSnapshot 类型（匹配 Agent）───────────────
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

// ─── 应用内容组件（React.memo 包裹）──────────────────
// ✅ 从 registry 获取 component，不再硬编码 switch
const MemoizedAppContent = memo(function AppContent({
  app,
  windowId,
  preloadData
}: {
  app: AppDefinition;
  windowId: string;
  preloadData?: any;
}) {
  const AppComponent = app.component;
  return <AppComponent windowId={windowId} preloadData={preloadData} />;
}, (prevProps, nextProps) => {
  return prevProps.app.id === nextProps.app.id &&
         prevProps.windowId === nextProps.windowId &&
         prevProps.preloadData === nextProps.preloadData;
});

// ─── 独立窗口组件：用 useWindowState 独立订阅 ────────
const DesktopWindow = memo(function DesktopWindow({
  windowId,
  appId,
}: {
  windowId: string;
  appId: string;
}) {
  const { manager } = useWindowManager();
  const win = manager.getById(windowId);
  const app = manager.getApp(appId);

  // 用 useWindowState 独立订阅此窗口的状态
  const windowState = useWindowState(windowId);

  if (!win || !app || !windowState) return null;

  // 最大化尺寸（工作区占满屏幕）
  const maxWindowSize = {
    width: window.innerWidth,
    height: window.innerHeight - 32, // 减去 TopBar 高度
  };
  const maxWindowPosition = { x: 0, y: 0 }; // 工作区内 y=0

  return (
    <WindowShell
      key={windowId}
      windowId={windowId}
      title={app.title}
      isActive={windowState.isActive}
      isMinimized={windowState.minimized}
      isMaximized={windowState.maximized}
      mode="standard"
      position={windowState.position}
      size={windowState.size}
      onClose={() => manager.close(windowId)}
      onMinimize={() => manager.minimize(windowId)}
      onMaximize={() => {
        if (windowState.maximized) {
          manager.unmaximize(windowId);
        } else {
          manager.maximize(windowId, maxWindowPosition, maxWindowSize);
        }
      }}
      onFocus={() => manager.focus(windowId)}
      onPositionChange={(pos) => {
        const window = manager.getById(windowId);
        if (window) {
          window.setPosition(pos);
          manager.emit({ type: 'window:moved', windowId, timestamp: Date.now() });
        }
      }}
      onSizeChange={(size) => {
        const window = manager.getById(windowId);
        if (window) {
          window.setSize(size);
          manager.emit({ type: 'window:resized', windowId, timestamp: Date.now() });
        }
      }}
    >
      <MemoizedAppContent app={app} windowId={windowId} preloadData={win.preloadData} />
    </WindowShell>
  );
});

// ─── Desktop 入口 ────────────────────────────────────
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

// ─── DesktopContent ──────────────────────────────────
function DesktopContent() {
  const [overviewVisible, setOverviewVisible] = useState(false);
  const [notificationOpen, setNotificationOpen] = useState(false);
  const unreadNotifications = 2;
  const criticalNotifications = 1;
  const [metrics, setMetrics] = useState<MetricsSnapshot | null>(null);
  const [clock, setClock] = useState("");

  const { activeServer } = useServerManager();
  const { wallpaper } = useWallpaper();
  const { manager } = useWindowManager();

  // ✅ Dock ref - 用于对比色计算
  const dockRef = useRef<HTMLDivElement>(null);

  // ✅ 根据 dock 背景色自动选择高对比度文本颜色
  // ✅ 使用主题的文本颜色 (--ovelis-text-primary)
  const { textColor, actualColor } = useContrastColor(dockRef, {
    interval: 500,
    useThemeText: true,  // ✅ 使用主题文本颜色
    lightColor: '#ffffff',  // 后备: 亮色背景使用白色文本
    darkColor: '#000000',   // 后备: 暗色背景使用黑色文本
  });

  // ✅ 从 registry 派生桌面/Dock 应用列表（单一数据源）
  const registeredApps = manager.getRegisteredApps();
  const desktopApps = registeredApps.filter(app => app.showOnDesktop !== false);
  const dockApps = registeredApps.filter(app => app.showOnDock !== false);

  // 用 useWindowGlobalState 替代 globalState
  const globalState = useWindowGlobalState();

  // 全局主题应用
  const { themeId, accentColorId } = useSettingsStore();
  useTheme(themeId, accentColorId);

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

  // 监听系统指标事件
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

  // 断连时清空 metrics
  useEffect(() => {
    if (!activeServer?.id) {
      setMetrics(null);
    }
  }, [activeServer?.id]);

  /**
   * ✅ Dock 激活/最小化逻辑
   * - 如果应用没有打开：创建新窗口
   * - 如果应用已最小化：恢复并聚焦
   * - 如果应用已打开但未聚焦：聚焦该窗口
   * - 如果应用已激活（在前台）：最小化该窗口
   */
  const openApp = useCallback(async (appId: string) => {
    setOverviewVisible(false);

    const existingWindows = manager.getByAppId(appId);
    if (existingWindows.length > 0) {
      const existing = existingWindows[0];
      const activeWindowId = globalState.activeWindowId;

      if (existing.minimized) {
        // 窗口已最小化 → 恢复并聚焦
        manager.restore(existing.id);
        manager.focus(existing.id);
      } else if (existing.id !== activeWindowId) {
        // 窗口已打开但未聚焦 → 聚焦
        manager.focus(existing.id);
      } else {
        // 窗口已打开且已聚焦 → 最小化
        manager.minimize(existing.id);
      }
      return;
    }

    await manager.create(appId, { serverId: activeServer?.id });
  }, [manager, activeServer?.id, setOverviewVisible, globalState.activeWindowId]);

  /**
   * ✅ 桌面图标/Overview 打开逻辑
   * - 只支持打开应用，不支持最小化
   * - 如果应用已打开：聚焦该窗口（不最小化）
   * - 是否支持多实例：由应用自己的 allowMultipleInstances 配置决定
   */
  const createApp = useCallback(async (appId: string) => {
    setOverviewVisible(false);

    const existingWindows = manager.getByAppId(appId);
    if (existingWindows.length > 0) {
      // 应用已打开 → 聚焦该窗口（不最小化）
      const existing = existingWindows[0];
      if (existing.minimized) {
        manager.restore(existing.id);
      }
      manager.focus(existing.id);
      return;
    }

    // 应用未打开 → 创建新窗口（是否允许多实例由应用配置决定）
    await manager.create(appId, { serverId: activeServer?.id });
  }, [manager, activeServer?.id, setOverviewVisible]);

  // Global shortcuts
  const shortcuts = createAppShortcuts(
    openApp,
    () => setOverviewVisible(v => !v),
    () => setOverviewVisible(false),
    () => setNotificationOpen(false)
  );
  useGlobalShortcuts(shortcuts);

  return (
    <div className="shell" style={getWallpaperStyle(wallpaper)}>
      {/* Top Bar */}
      <TopBar
        metrics={metrics}
        clock={clock}
        unreadNotifications={unreadNotifications}
        criticalNotifications={criticalNotifications}
        onActivitiesClick={() => setOverviewVisible((v) => !v)}
        onNotificationClick={() => setNotificationOpen(true)}
      />

      {/* Workspace: TopBar 下方的工作区 */}
      <div className="workspace">
        <div className="desktop-icons">
          {desktopApps.map((app) => (
            <div
              key={app.id}
              className="desktop-icon"
              onDoubleClick={() => createApp(app.id)}
            >
              <div className="icon">{app.icon}</div>
              <div className="label">{app.desktopLabel ?? app.title}</div>
            </div>
          ))}
        </div>

        {/* Application Windows */}
        {globalState.windowList.map((winInfo) => (
          <DesktopWindow
            key={winInfo.id}
            windowId={winInfo.id}
            appId={winInfo.appId}
          />
        ))}

        {/* Dock */}
        <div className="dock-container">
          <div
            ref={dockRef}
            className="dock"
            style={{
              '--dock-text-color': actualColor,
              '--dock-text-shadow': textColor === 'light'
                ? '0 1px 2px rgba(0, 0, 0, 0.5)'
                : '0 1px 1px rgba(255, 255, 255, 0.5)',
            } as React.CSSProperties}
          >
            {dockApps.map((app) => {
              const isOpen = manager.getByAppId(app.id).length > 0;
              return (
                <div
                  key={app.id}
                  className={`dock-item${isOpen ? " running" : ""}`}
                  onClick={() => openApp(app.id)}
                >
                  <div className="icon">{app.icon}</div>
                  <div className="label">{app.dockLabel ?? app.title}</div>
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
            {desktopApps.map((app) => (
              <div
                key={app.id}
                className="overview-app"
                onClick={() => createApp(app.id)}
              >
                <div className="icon">{app.icon}</div>
                <div className="label">{app.desktopLabel ?? app.title}</div>
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
