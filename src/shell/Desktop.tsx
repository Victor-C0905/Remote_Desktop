import { useState, useEffect, useCallback } from "react";
import { FileManager } from "../apps/FileManager";
import { TerminalApp } from "../apps/Terminal";
import { SystemMonitor } from "../apps/SystemMonitor";
import { Settings } from "../apps/Settings";
import { NotificationCenter, NotificationBadge } from "./NotificationCenter";
import "./Desktop.css";

interface ConnectionStatus {
  host: string;
  connected: boolean;
  latency_ms: number;
}

interface MetricsSnapshot {
  cpu_percent: number;
  mem_used_gb: string;
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
}

export function Desktop() {
  const [overviewVisible, setOverviewVisible] = useState(false);
  const [notificationOpen, setNotificationOpen] = useState(false);
  const [unreadNotifications, setUnreadNotifications] = useState(2);
  const [criticalNotifications, setCriticalNotifications] = useState(1);
  const [connection] = useState<ConnectionStatus>({
    host: "未连接",
    connected: false,
    latency_ms: 0,
  });
  const [metrics] = useState<MetricsSnapshot | null>(null);
  const [clock, setClock] = useState("");
  const [windows, setWindows] = useState<WindowState[]>([]);
  const [activeWindowId, setActiveWindowId] = useState<string | null>(null);

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

  // Super key (Meta) toggles overview
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key === "Meta" || e.key === "OS") {
        e.preventDefault();
        setOverviewVisible((v) => !v);
      }
      if (e.key === "Escape" && overviewVisible) {
        setOverviewVisible(false);
      }
    },
    [overviewVisible]
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  const openApp = (appId: string) => {
    setOverviewVisible(false);

    // Map appId to window title
    const titles: Record<string, string> = {
      files: "文件管理器",
      terminal: "终端",
      monitor: "系统监控",
      settings: "设置",
    };

    // Only allow one instance of each app
    if (windows.some((w) => w.appId === appId)) {
      // Focus existing window
      const existing = windows.find((w) => w.appId === appId);
      if (existing) {
        setWindows((ws) =>
          ws.map((w) => (w.id === existing.id ? { ...w, minimized: false } : w))
        );
        setActiveWindowId(existing.id);
      }
      return;
    }

    const newWindow: WindowState = {
      id: `win-${appId}-${Date.now()}`,
      appId,
      title: titles[appId] || appId,
      minimized: false,
    };
    setWindows((ws) => [...ws, newWindow]);
    setActiveWindowId(newWindow.id);
  };

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
    setWindows((ws) =>
      ws.map((w) => (w.id === windowId ? { ...w, minimized: false } : w))
    );
    setActiveWindowId(windowId);
  };

  // Render app content
  const renderAppContent = (appId: string) => {
    switch (appId) {
      case "files":
        return <FileManager />;
      case "terminal":
        return <TerminalApp />;
      case "monitor":
        return <SystemMonitor />;
      case "settings":
        return <Settings />;
      default:
        return <div>Unknown app</div>;
    }
  };

  // Sort windows: active on top
  const sortedWindows = [...windows].sort((a, b) => {
    if (a.id === activeWindowId) return 1;
    if (b.id === activeWindowId) return -1;
    return 0;
  });

  return (
    <div className="shell">
      {/* Top Bar */}
      <div className="top-bar">
        <button
          className="activities-btn"
          onClick={() => setOverviewVisible((v) => !v)}
        >
          活动
        </button>
        <div className="separator" />
        <div className="connection-indicator">
          <div className={`connection-dot${connection.connected ? "" : " disconnected"}`} />
          <span>{connection.host}</span>
        </div>
        <div className="spacer" />
        {metrics && (
          <div className="metrics">
            <span>CPU {metrics.cpu_percent}%</span>
            <span>MEM {metrics.mem_used_gb}G</span>
          </div>
        )}
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
      <div className="desktop-area">
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
        {sortedWindows.map((win) => (
          !win.minimized && (
            <div
              key={win.id}
              className={`app-window${win.id === activeWindowId ? " active" : ""}`}
              onMouseDown={() => focusWindow(win.id)}
            >
              <div className="app-window-titlebar">
                <div className="awt-btns">
                  <button
                    className="awt-btn close"
                    onClick={(e) => { e.stopPropagation(); closeWindow(win.id); }}
                  />
                  <button
                    className="awt-btn minimize"
                    onClick={(e) => { e.stopPropagation(); minimizeWindow(win.id); }}
                  />
                </div>
                <span className="awt-title">{win.title}</span>
                <div className="awt-spacer" />
              </div>
              <div className="app-window-content">
                {renderAppContent(win.appId)}
              </div>
            </div>
          )
        ))}

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
