// src/shell/NotificationCenter.tsx
// 通知中心：右侧滑出面板（呈现层，Adwaita Symbolic 风格）
// 数据源 notificationStore；交互（过滤/已读/清除）与数据层不变

import { useState, useEffect } from "react";
import "./NotificationCenter.css";
import { useNotificationStore } from "../stores/notificationStore";
import type { NotificationUrgency } from "../stores/notificationStore";
import { SymbolicIcon } from "../components/symbolic";

/* ── Utility Functions ───────────────────────────────── */

function formatTime(timestamp: number): string {
  const now = new Date();
  const date = new Date(timestamp);
  const diff = now.getTime() - timestamp;

  if (diff < 1000 * 60) return "刚刚";
  if (diff < 1000 * 60 * 60) return `${Math.floor(diff / 60000)} 分钟前`;

  // 非今天：显示具体日期（跨年带年份）
  if (date.toDateString() !== now.toDateString()) {
    const sameYear = date.getFullYear() === now.getFullYear();
    return sameYear
      ? `${date.getMonth() + 1}月${date.getDate()}日`
      : `${date.getFullYear()}年${date.getMonth() + 1}月${date.getDate()}日`;
  }

  // 今天且超过 1 小时
  return `${Math.floor(diff / 3600000)} 小时前`;
}

/** urgency → Symbolic 图标名 */
function getUrgencyIcon(urgency: NotificationUrgency) {
  switch (urgency) {
    case "critical": return "dialog-error" as const;
    case "normal": return "dialog-warning" as const;
    case "low": return "dialog-information" as const;
  }
}

/** urgency → 语义类名（左边框 + 图标着色由 CSS 令牌控制） */
function getUrgencyClass(urgency: NotificationUrgency): string {
  return `nc-notif-urgency-${urgency}`;
}

/* ── Main Component ─────────────────────────────────── */

interface NotificationCenterProps {
  isOpen: boolean;
  onClose: () => void;
}

export function NotificationCenter({ isOpen, onClose }: NotificationCenterProps) {
  // 真实数据源：zustand 内存 store（连接失败/断连/恢复通知）
  const { notifications, markAsRead, dismiss, clearAll, markAllRead } = useNotificationStore();
  const [filter, setFilter] = useState<NotificationUrgency | "all">("all");

  const unreadCount = notifications.filter((n) => !n.read).length;
  const criticalCount = notifications.filter((n) => n.urgency === "critical" && !n.read).length;

  const filteredNotifications = notifications.filter((n) => {
    if (filter === "all") return true;
    return n.urgency === filter;
  }).sort((a, b) => b.timestamp - a.timestamp);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && isOpen) {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, onClose]);

  // 打开面板即清除未读标签（badge 归零、卡片未读样式消失；期间新到通知保持未读）
  useEffect(() => {
    if (isOpen) markAllRead();
  }, [isOpen, markAllRead]);

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
              <SymbolicIcon name="emblem-ok" size={14} />
              全部已读
            </button>
            <button className="nc-action-btn nc-action-danger" onClick={clearAll} title="清除全部">
              <SymbolicIcon name="user-trash" size={14} />
              清除
            </button>
            <button className="nc-close-btn" onClick={onClose} aria-label="关闭通知中心">
              <SymbolicIcon name="window-close" size={14} />
            </button>
          </div>
        </div>

        {/* Filters（文本 + 语义色点） */}
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
            <span className="nc-dot nc-dot-critical" />
            紧急 ({notifications.filter((n) => n.urgency === "critical").length})
          </button>
          <button
            className={`nc-filter-btn ${filter === "normal" ? "active" : ""}`}
            onClick={() => setFilter("normal")}
          >
            <span className="nc-dot nc-dot-normal" />
            普通 ({notifications.filter((n) => n.urgency === "normal").length})
          </button>
          <button
            className={`nc-filter-btn ${filter === "low" ? "active" : ""}`}
            onClick={() => setFilter("low")}
          >
            <span className="nc-dot nc-dot-low" />
            低 ({notifications.filter((n) => n.urgency === "low").length})
          </button>
        </div>

        {/* Notification List */}
        <div className="nc-list">
          {filteredNotifications.length === 0 ? (
            <div className="nc-empty">
              <SymbolicIcon name="mailbox" size={32} className="nc-empty-icon" />
              <div className="nc-empty-text">没有通知</div>
            </div>
          ) : (
            filteredNotifications.map((notif) => (
              <div
                key={notif.id}
                className={`nc-notif ${getUrgencyClass(notif.urgency)} ${!notif.read ? "nc-unread" : ""}`}
                onClick={() => markAsRead(notif.id)}
              >
                {/* 卡片头部行：时间（左上）+ 关闭按钮（右上） */}
                <div className="nc-notif-top">
                  <span className="nc-notif-time">{formatTime(notif.timestamp)}</span>
                  <button
                    className="nc-notif-dismiss"
                    onClick={(e) => {
                      e.stopPropagation();
                      dismiss(notif.id);
                    }}
                    aria-label="删除通知"
                    title="删除"
                  >
                    <SymbolicIcon name="window-close" size={12} />
                  </button>
                </div>
                {/* 主体行：严重度图标 + 标题/来源/正文/建议 */}
                <div className="nc-notif-main">
                  <SymbolicIcon
                    name={getUrgencyIcon(notif.urgency)}
                    size={16}
                    className="nc-notif-sev-icon"
                  />
                  <div className="nc-notif-content">
                    <div className="nc-notif-header">
                      <span className="nc-notif-title">{notif.title}</span>
                      <span className="nc-notif-source">{notif.source}</span>
                    </div>
                    <div className="nc-notif-body">{notif.body}</div>
                    {notif.action && <div className="nc-notif-action">{notif.action}</div>}
                  </div>
                </div>
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
