import { useState, useEffect, useCallback, memo } from "react";
import { FileManager } from "../apps/FileManager";
import { TerminalApp } from "../apps/Terminal";
import { SystemMonitor } from "../apps/SystemMonitor";
import { Settings } from "../apps/Settings";
import { TextEditor } from "../apps/TextEditor/TextEditor";
import { NotificationCenter } from "./NotificationCenter";
import { TopBar } from "./TopBar/TopBar";
import { useGlobalShortcuts, createAppShortcuts } from "../hooks/useGlobalShortcuts";
import { ServerManagerProvider, useServerManager } from "../context/ServerManager";

import { WallpaperProvider, useWallpaper, getWallpaperStyle } from "../context/WallpaperContext";
import { useSettingsStore } from "../stores/settingsStore";
import { useTheme } from "../hooks/useTheme";
import { WindowShell } from "../components/window-shell";
import { WindowManagerProvider, useWindowManager, useWindowGlobalState, useWindowState } from "../window-system/WindowManagerContext";
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

interface DesktopApp {
  id: string;
  icon: string;
  label: string;
}

const DESKTOP_APPS: DesktopApp[] = [
  { id: "files",    icon: "📁", label: "远程文件" },
  { id: "terminal", icon: "🖥️", label: "远程终端" },
  { id: "monitor",  icon: "📊", label: "系统监控" },
  { id: "editor",   icon: "📝", label: "文本编辑器" },
  { id: "settings", icon: "⚙️",  label: "设置" },
];

const DOCK_APPS: DesktopApp[] = [
  { id: "files",    icon: "📁", label: "文件" },
  { id: "terminal", icon: "🖥️", label: "终端" },
  { id: "monitor",  icon: "📊", label: "监控" },
  { id: "editor",   icon: "📝", label: "编辑器" },
  { id: "settings", icon: "⚙️",  label: "设置" },
];

// ─── 应用内容组件（React.memo 包裹）──────────────────
const MemoizedAppContent = memo(function AppContent({
  appId,
  windowId,
  preloadData
}: {
  appId: string;
  windowId: string;
  preloadData?: any;
}) {
  switch (appId) {
    case 'files':
      return <FileManager windowId={windowId} preloadData={preloadData} />;
    case 'terminal':
      return <TerminalApp windowId={windowId} />;
    case 'monitor':
      return <SystemMonitor windowId={windowId} />;
    case 'editor':
      return <TextEditor windowId={windowId} preloadData={preloadData} />;
    case 'settings':
      return <Settings windowId={windowId} />;
    default:
      return <div>Unknown app</div>;
  }
}, (prevProps, nextProps) => {
  return prevProps.appId === nextProps.appId &&
         prevProps.windowId === nextProps.windowId &&
         prevProps.preloadData === nextProps.preloadData;
});

// ─── 独立窗口组件：用 useWindowState 独立订阅 ────────
// ✅ 核心优化：每个窗口用 useWindowState(windowId) 独立订阅
// 其他窗口拖动/调整大小时，此组件不会重渲染
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

  // ✅ 用 useWindowState 独立订阅此窗口的状态
  // 只有此窗口状态变化时才重渲染，其他窗口拖动不影响
  const windowState = useWindowState(windowId);

  if (!win || !app || !windowState) return null;

  // 最大化尺寸
  const maxWindowSize = {
    width: window.innerWidth,
    height: window.innerHeight - 32,
  };
  const maxWindowPosition = { x: 0, y: 0 };

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
          // 取消最大化：Window.setMaximized(false) 内部会恢复保存的位置/尺寸
          manager.unmaximize(windowId);
        } else {
          // 最大化
          manager.maximize(windowId, maxWindowPosition, maxWindowSize);
        }
      }}
      onFocus={() => manager.focus(windowId)}
      onPositionChange={(pos) => {
        // ✅ 只更新 Window 对象，useWindowState 会自动感知变化并重渲染
        const window = manager.getById(windowId);
        if (window) {
          window.setPosition(pos);
          // ✅ 触发 window:moved 事件，通知 per-window 订阅者
          manager.emit({ type: 'window:moved', windowId, timestamp: Date.now() });
        }
      }}
      onSizeChange={(size) => {
        // ✅ 只更新 Window 对象，useWindowState 会自动感知变化并重渲染
        const window = manager.getById(windowId);
        if (window) {
          window.setSize(size);
          // ✅ 触发 window:resized 事件，通知 per-window 订阅者
          manager.emit({ type: 'window:resized', windowId, timestamp: Date.now() });
        }
      }}
    >
      <MemoizedAppContent appId={appId} windowId={windowId} preloadData={win.preloadData} />
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

  // ✅ 用 useWindowGlobalState 替代 globalState
  // 只在窗口列表变化时重渲染（创建/关闭/最小化/恢复/聚焦）
  // 窗口拖动/调整大小不触发此 Hook
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

  const openApp = useCallback(async (appId: string) => {
    setOverviewVisible(false);

    const existingWindows = manager.getByAppId(appId);
    if (existingWindows.length > 0) {
      const existing = existingWindows[0];
      if (existing.minimized) {
        manager.restore(existing.id);
        manager.focus(existing.id);
      } else {
        manager.minimize(existing.id);
      }
      return;
    }

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
    <div className="shell">
      {/* Top Bar */}
      <TopBar
        metrics={metrics}
        clock={clock}
        unreadNotifications={unreadNotifications}
        criticalNotifications={criticalNotifications}
        onActivitiesClick={() => setOverviewVisible((v) => !v)}
        onNotificationClick={() => setNotificationOpen(true)}
      />

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

        {/* ✅ Application Windows — 独立订阅，拖动一个窗口不影响其他窗口 */}
        {globalState.windowList.map((winInfo) => (
          <DesktopWindow
            key={winInfo.id}
            windowId={winInfo.id}
            appId={winInfo.appId}
          />
        ))}

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
