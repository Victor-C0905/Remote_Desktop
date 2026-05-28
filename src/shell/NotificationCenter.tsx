import { useState, useEffect, useCallback } from "react";
import "./NotificationCenter.css";

/* ── Types ─────────────────────────────────────────────── */

type Urgency = "low" | "normal" | "critical";

interface Notification {
  id: string;
  title: string;
  body: string;
  timestamp: number;
  urgency: Urgency;
  source: string;
  read: boolean;
}

/* ── Demo Notifications ───────────────────────────────── */

function generateDemoNotifications(): Notification[] {
  return [
    {
      id: "notif-1",
      title: "CPU 使用率过高",
      body: "CPU 使用率达到 85%，超过阈值 80%",
      timestamp: Date.now() - 1000 * 60 * 2,
      urgency: "critical",
      source: "系统监控",
      read: false,
    },
    {
      id: "notif-2",
      title: "文件下载完成",
      body: "config.tar.gz (256MB) 已下载到本地",
      timestamp: Date.now() - 1000 * 60 * 5,
      urgency: "normal",
      source: "文件管理",
      read: false,
    },
    {
      id: "notif-3",
      title: "连接已恢复",
      body: "prod-server 连接已恢复，延迟 6ms",
      timestamp: Date.now() - 1000 * 60 * 10,
      urgency: "normal",
      source: "连接",
      read: true,
    },
    {
      id: "notif-4",
      title: "磁盘空间不足",
      body: "/var 分区使用率达到 92%",
      timestamp: Date.now() - 1000 * 60 * 30,
      urgency: "critical",
      source: "系统监控",
      read: true,
    },
    {
      id: "notif-5",
      title: "终端会话创建",
      body: "新终端会话已启动 (pty-abc123)",
      timestamp: Date.now() - 1000 * 60 * 60,
      urgency: "low",
      source: "终端",
      read: true,
    },
  ];
}

/* ── Utility Functions ───────────────────────────────── */

function formatTime(timestamp: number): string {
  const now = Date.now();
  const diff = now - timestamp;

  if (diff < 1000 * 60) return "刚刚";
  if (diff < 1000 * 60 * 60) return `${Math.floor(diff / 60000)} 分钟前`;
  if (diff < 1000 * 60 * 60 * 24) return `${Math.floor(diff / 3600000)} 小时前`;
  return `${Math.floor(diff / 86400000)} 天前`;
}

function getUrgencyIcon(urgency: Urgency): string {
  switch (urgency) {
    case "critical": return "🔴";
    case "normal": return "🟡";
    case "low": return "🟢";
  }
}

function getUrgencyClass(urgency: Urgency): string {
  return `nc-notif-urgency-${urgency}`;
}

/* ── Main Component ─────────────────────────────────── */

interface NotificationCenterProps {
  isOpen: boolean;
  onClose: () => void;
}

export function NotificationCenter({ isOpen, onClose }: NotificationCenterProps) {
  const [notifications, setNotifications] = useState<Notification[]>(generateDemoNotifications());
  const [filter, setFilter] = useState<Urgency | "all">("all");

  const unreadCount = notifications.filter(n => !n.read).length;
  const criticalCount = notifications.filter(n => n.urgency === "critical" && !n.read).length;

  const filteredNotifications = notifications.filter(n => {
    if (filter === "all") return true;
    return n.urgency === filter;
  }).sort((a, b) => b.timestamp - a.timestamp);

  const markAsRead = useCallback((id: string) => {
    setNotifications(prev => prev.map(n => n.id === id ? { ...n, read: true } : n));
  }, []);

  const dismissNotification = useCallback((id: string) => {
    setNotifications(prev => prev.filter(n => n.id !== id));
  }, []);

  const clearAll = useCallback(() => {
    setNotifications([]);
  }, []);

  const markAllRead = useCallback(() => {
    setNotifications(prev => prev.map(n => ({ ...n, read: true })));
  }, []);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isOpen) {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <div className="nc-overlay" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="nc-panel">
        {/* Header */}
        <div className="nc-header">
          <div className="nc-header-title">
            <span className="nc-title">通知</span>
            {unreadCount > 0 && (
              <span className="nc-badge">{unreadCount}</span>
            )}
          </div>
          <div className="nc-header-actions">
            <button className="nc-action-btn" onClick={markAllRead} title="全部标记已读">
              ✓ 全部已读
            </button>
            <button className="nc-action-btn nc-action-danger" onClick={clearAll} title="清除全部">
              🗑️ 清除
            </button>
            <button className="nc-close-btn" onClick={onClose}>×</button>
          </div>
        </div>

        {/* Filters */}
        <div className="nc-filters">
          <button
            className={`nc-filter-btn ${filter === "all" ? "active" : ""}`}
            onClick={() => setFilter("all")}
          >
            全部 ({notifications.length})
          </button>
          <button
            className={`nc-filter-btn ${filter === "critical" ? "active" : ""}`}
            onClick={() => setFilter("critical")}
          >
            🔴 紧急 ({notifications.filter(n => n.urgency === "critical").length})
          </button>
          <button
            className={`nc-filter-btn ${filter === "normal" ? "active" : ""}`}
            onClick={() => setFilter("normal")}
          >
            🟡 普通 ({notifications.filter(n => n.urgency === "normal").length})
          </button>
          <button
            className={`nc-filter-btn ${filter === "low" ? "active" : ""}`}
            onClick={() => setFilter("low")}
          >
            🟢 低 ({notifications.filter(n => n.urgency === "low").length})
          </button>
        </div>

        {/* Notification List */}
        <div className="nc-list">
          {filteredNotifications.length === 0 ? (
            <div className="nc-empty">
              <div className="nc-empty-icon">📭</div>
              <div className="nc-empty-text">没有通知</div>
            </div>
          ) : (
            filteredNotifications.map(notif => (
              <div
                key={notif.id}
                className={`nc-notif ${getUrgencyClass(notif.urgency)} ${!notif.read ? "nc-unread" : ""}`}
                onClick={() => markAsRead(notif.id)}
              >
                <div className="nc-notif-icon">{getUrgencyIcon(notif.urgency)}</div>
                <div className="nc-notif-content">
                  <div className="nc-notif-header">
                    <span className="nc-notif-title">{notif.title}</span>
                    <span className="nc-notif-source">{notif.source}</span>
                  </div>
                  <div className="nc-notif-body">{notif.body}</div>
                  <div className="nc-notif-time">{formatTime(notif.timestamp)}</div>
                </div>
                <button
                  className="nc-notif-dismiss"
                  onClick={(e) => { e.stopPropagation(); dismissNotification(notif.id); }}
                  title="删除"
                >
                  ×
                </button>
              </div>
            ))
          )}
        </div>

        {/* Footer */}
        <div className="nc-footer">
          <span className="nc-footer-text">
            {criticalCount > 0 && (
              <span className="nc-footer-warning">⚠️ {criticalCount} 个紧急通知未处理</span>
            )}
            {criticalCount === 0 && unreadCount > 0 && (
              <span>{unreadCount} 个未读通知</span>
            )}
            {criticalCount === 0 && unreadCount === 0 && (
              <span>所有通知已处理</span>
            )}
          </span>
        </div>
      </div>
    </div>
  );
}

/* ── Notification Badge Component ────────────────────── */

interface NotificationBadgeProps {
  count: number;
  criticalCount: number;
}

export function NotificationBadge({ count, criticalCount }: NotificationBadgeProps) {
  if (count === 0) return null;

  return (
    <span className={`nc-topbar-badge ${criticalCount > 0 ? "nc-badge-critical" : ""}`}>
      {count > 9 ? "9+" : count}
    </span>
  );
}