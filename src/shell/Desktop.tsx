import { useState, useEffect, useCallback, memo } from "react";
import { FileManager } from "../apps/FileManager";
import { TerminalApp } from "../apps/Terminal";
import { SystemMonitor } from "../apps/SystemMonitor";
import { Settings } from "../apps/Settings";
import { NotificationCenter, NotificationBadge } from "./NotificationCenter";
import { useGlobalShortcuts, createAppShortcuts } from "../hooks/useGlobalShortcuts";
import { ServerManagerProvider, useServerManager, getStatusColor } from "../context/ServerManager";
import { formatBytesSafe, formatPercentSafe } from "../utils/offlineDefaults";

import { WallpaperProvider, useWallpaper, getWallpaperStyle } from "../context/WallpaperContext";
import { useSettingsStore } from "../stores/settingsStore";
import { useTheme } from "../hooks/useTheme";
import { WindowShell } from "../components/window-shell";
import { WindowManagerProvider, useWindowManager } from "../window-system/WindowManagerContext";
import { listen } from "@tauri-apps/api/event";
import "./Desktop.css";

// ✅ 新增：窗口局部状态类型
interface WindowLocalState {
  position: { x: number; y: number };
  size: { width: number; height: number };
}

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

// ✅ 新增：应用内容组件（React.memo 包裹）
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
    case 'settings':
      return <Settings windowId={windowId} />;
    default:
      return <div>Unknown app</div>;
  }
}, (prevProps, nextProps) => {
  // ✅ 只在 appId 或 windowId 变化时重渲染
  return prevProps.appId === nextProps.appId && 
         prevProps.windowId === nextProps.windowId &&
         prevProps.preloadData === nextProps.preloadData;
});

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

  // ✅ 新增：窗口局部状态管理（position/size），避免读取旧值
  const [windowLocalStates, setWindowLocalStates] = useState<Map<string, WindowLocalState>>(new Map());

  const { activeServer } = useServerManager();
  const { wallpaper } = useWallpaper();
  const { manager, globalState } = useWindowManager();

  // 全局主题应用（确保所有窗口都使用正确的主题 CSS 变量）
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

    // GNOME 标准:如果窗口已存在(包括最小化的窗口),点击 Dock 恢复时自动置顶
    const existingWindows = manager.getByAppId(appId);
    if (existingWindows.length > 0) {
      const existing = existingWindows[0];
      // 如果窗口被最小化,focus() 会自动恢复它
      manager.focus(existing.id);
      return;
    }

    // 所有应用直接创建（FileManager 自行处理数据加载和离线状态）
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

  // Render app content（使用 React.memo 包裹的组件）
  const renderAppContent = (appId: string, win: ReturnType<typeof manager.getById>) => {
    if (!win) return null;
    return <MemoizedAppContent appId={appId} windowId={win.id} preloadData={win.preloadData} />;
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
            - 最小化窗口: 保持渲染但隐藏(display: none),保留内部状态
        */}
        {(() => {
          // 不过滤最小化窗口,保持所有窗口渲染以保留内部状态
          const allWindows = windows;

          // 不重新排序，保持 DOM 树顺序不变，避免事件丢失
          return allWindows.map((win, index) => {
            const app = manager.getApp(win.appId);
            if (!app) return null;

            // ✅ 优先使用局部状态（position/size），避免读取旧值
            const localState = windowLocalStates.get(win.id);
            const currentPosition = localState?.position || win.position;
            const currentSize = localState?.size || win.size;

            return (
              <WindowShell
                key={win.id} // 使用稳定的 key,不包含 minimized 状态,避免重新挂载
                windowId={win.id}
                title={app.title}
                isActive={win.id === activeWindow?.id}
                isMinimized={win.minimized}
                mode="standard" // ✅ 所有应用默认使用标准模式（有头窗口）
                position={currentPosition}
                size={currentSize}
                onClose={() => manager.close(win.id)}
                onMinimize={() => manager.minimize(win.id)}
                onMaximize={() => {
                  // TODO: 实现最大化逻辑
                  console.log('maximize:', win.id);
                }}
                onFocus={() => manager.focus(win.id)}
                onPositionChange={(pos) => {
                  // ✅ 更新 Window 对象和局部状态
                  const window = manager.getById(win.id);
                  if (window) {
                    window.setPosition(pos);
                    // ✅ 更新局部状态，触发重渲染
                    setWindowLocalStates(prev => {
                      const newMap = new Map(prev);
                      newMap.set(win.id, { position: pos, size: currentSize });
                      return newMap;
                    });
                  }
                }}
                onSizeChange={(size) => {
                  // ✅ 更新 Window 对象和局部状态
                  const window = manager.getById(win.id);
                  if (window) {
                    window.setSize(size);
                    // ✅ 更新局部状态，触发重渲染
                    setWindowLocalStates(prev => {
                      const newMap = new Map(prev);
                      newMap.set(win.id, { position: currentPosition, size });
                      return newMap;
                    });
                  }
                }}
              >
                {renderAppContent(win.appId, win)}
              </WindowShell>
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
