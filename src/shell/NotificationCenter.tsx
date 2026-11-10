// src/shell/NotificationCenter.tsx
// 通知中心：右侧滑出面板（呈现层，Adwaita Symbolic 风格）
// 数据源 notificationStore；交互（过滤/已读/清除）与数据层不变

import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import "./NotificationCenter.css";
import { useNotificationStore, isNotificationVisible } from "../stores/notificationStore";
import type { AppNotification, NotificationUrgency } from "../stores/notificationStore";
import { useServersStore, type ServerConfig } from "../stores/serversStore";
import { SymbolicIcon } from "../components/symbolic";

/* ── Utility Functions ───────────────────────────────── */

/** 时间格式化（t 由调用方传入，避免模块级缓存旧语言） */
function formatTime(t: (k: string, o?: Record<string, unknown>) => string, timestamp: number): string {
  const now = new Date();
  const date = new Date(timestamp);
  const diff = now.getTime() - timestamp;

  if (diff < 1000 * 60) return t("notifications.time.justNow");
  if (diff < 1000 * 60 * 60) return t("notifications.time.minutesAgo", { n: Math.floor(diff / 60000) });

  // 非今天：显示具体日期（跨年带年份）
  if (date.toDateString() !== now.toDateString()) {
    const sameYear = date.getFullYear() === now.getFullYear();
    return sameYear
      ? t("notifications.time.monthDay", { m: date.getMonth() + 1, d: date.getDate() })
      : t("notifications.time.yearMonthDay", { y: date.getFullYear(), m: date.getMonth() + 1, d: date.getDate() });
  }

  // 今天且超过 1 小时
  return t("notifications.time.hoursAgo", { n: Math.floor(diff / 3600000) });
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

/** 条目来源 chip：服务器名优先，系统通知回退 source */
function originOf(n: AppNotification, servers: ServerConfig[]): string {
  if (!n.serverId) return n.source;
  const server = servers.find((s) => s.id === n.serverId);
  return server ? (server.name || server.host) : (n.serverName || n.source);
}

/* ── Main Component ─────────────────────────────────── */

interface NotificationCenterProps {
  isOpen: boolean;
  onClose: () => void;
  /** 当前活动服务器；通知按它隔离（global/系统通知不受影响） */
  activeServerId?: string | null;
}

export function NotificationCenter({ isOpen, onClose, activeServerId = null }: NotificationCenterProps) {
  const { t } = useTranslation();
  // 真实数据源：zustand 内存 store（连接失败/断连/恢复通知）
  const { notifications, markAsRead, dismiss, clearAll, markAllRead } = useNotificationStore();
  const servers = useServersStore((s) => s.servers);
  const [filter, setFilter] = useState<NotificationUrgency | "all">("all");
  // 展开明细的通知 id（点击条目切换；如移动通知的逐条 src → dst）
  const [expandedId, setExpandedId] = useState<string | null>(null);

  // 彻底按服务器隔离：只显示当前服务器 + 系统/连接通知
  const visibleNotifications = notifications.filter((n) => isNotificationVisible(n, activeServerId));
  const unreadCount = visibleNotifications.filter((n) => !n.read).length;
  const criticalCount = visibleNotifications.filter((n) => n.urgency === "critical" && !n.read).length;

  const filteredNotifications = visibleNotifications.filter((n) => {
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
  // 只标记当前服务器可见通知为已读，其它服务器的未读状态保留
  useEffect(() => {
    if (isOpen) markAllRead((n) => isNotificationVisible(n, activeServerId));
  }, [isOpen, markAllRead, activeServerId]);

  if (!isOpen) return null;

  // data-browser-overlay：浏览器 webview 子窗口让位标记（child HWND 无法被
  // DOM z-index 遮挡），BrowserApp 检测到即隐藏 webview 让位
  return (
    <div className="nc-overlay" data-browser-overlay onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="nc-panel">
        {/* Header */}
        <div className="nc-header">
          <div className="nc-header-title">
            <span className="nc-title">{t('notifications.title')}</span>
            {unreadCount > 0 && (
              <span className="nc-badge">{unreadCount}</span>
            )}
          </div>
          <div className="nc-header-actions">
            <button className="nc-action-btn" onClick={() => markAllRead((n) => isNotificationVisible(n, activeServerId))} title={t('notifications.markAllReadTitle')}>
              <SymbolicIcon name="emblem-ok" size={14} />
              {t('notifications.markAllRead')}
            </button>
            <button className="nc-action-btn nc-action-danger" onClick={() => clearAll((n) => isNotificationVisible(n, activeServerId))} title={t('notifications.clearAllTitle')}>
              <SymbolicIcon name="user-trash" size={14} />
              {t('notifications.clearAll')}
            </button>
            <button className="nc-close-btn" onClick={onClose} aria-label={t('notifications.closeAria')}>
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
            {t('notifications.filterAll', { count: visibleNotifications.length })}
          </button>
          <button
            className={`nc-filter-btn ${filter === "critical" ? "active" : ""}`}
            onClick={() => setFilter("critical")}
          >
            <span className="nc-dot nc-dot-critical" />
            {t('notifications.filterCritical', { count: visibleNotifications.filter((n) => n.urgency === "critical").length })}
          </button>
          <button
            className={`nc-filter-btn ${filter === "normal" ? "active" : ""}`}
            onClick={() => setFilter("normal")}
          >
            <span className="nc-dot nc-dot-normal" />
            {t('notifications.filterNormal', { count: visibleNotifications.filter((n) => n.urgency === "normal").length })}
          </button>
          <button
            className={`nc-filter-btn ${filter === "low" ? "active" : ""}`}
            onClick={() => setFilter("low")}
          >
            <span className="nc-dot nc-dot-low" />
            {t('notifications.filterLow', { count: visibleNotifications.filter((n) => n.urgency === "low").length })}
          </button>
        </div>

        {/* Notification List */}
        <div className="nc-list">
          {filteredNotifications.length === 0 ? (
            <div className="nc-empty">
              <SymbolicIcon name="mailbox" size={32} className="nc-empty-icon" />
              <div className="nc-empty-text">{t('notifications.empty')}</div>
            </div>
          ) : (
            filteredNotifications.map((notif) => (
              <div
                key={notif.id}
                className={`nc-notif ${getUrgencyClass(notif.urgency)} ${!notif.read ? "nc-unread" : ""}`}
                onClick={() => {
                  markAsRead(notif.id);
                  // 有明细的通知：点击展开/收起逐条清单（如移动的 src → dst）
                  if (notif.details?.length) setExpandedId((cur) => (cur === notif.id ? null : notif.id));
                }}
              >
                {/* 卡片头部行：时间（左上）+ 关闭按钮（右上） */}
                <div className="nc-notif-top">
                  <span className="nc-notif-time">{formatTime(t, notif.timestamp)}</span>
                  <button
                    className="nc-notif-dismiss"
                    onClick={(e) => {
                      e.stopPropagation();
                      dismiss(notif.id);
                    }}
                    aria-label={t('notifications.deleteAria')}
                    title={t('notifications.delete')}
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
                      <span className="nc-notif-source">{originOf(notif, servers)}</span>
                    </div>
                    <div className="nc-notif-body">{notif.body}</div>
                    {notif.action && <div className="nc-notif-action">{notif.action}</div>}
                    {expandedId === notif.id && notif.details && notif.details.length > 0 && (
                      <div className="nc-notif-details">
                        {notif.details.slice(0, 30).map((line, i) => (
                          <div key={i} className="nc-notif-detail-line">{line}</div>
                        ))}
                        {notif.details.length > 30 && (
                          <div className="nc-notif-detail-line">…{t('notifications.moreDetails', { n: notif.details.length - 30 })}</div>
                        )}
                      </div>
                    )}
                    {notif.onAction && (
                      <button
                        type="button"
                        className="nc-notif-undo"
                        onClick={(e) => {
                          e.stopPropagation();  // 不触发条目的展开/已读切换
                          notif.onAction?.run();
                          dismiss(notif.id);
                        }}
                      >
                        {notif.onAction.label}
                      </button>
                    )}
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
              <span className="nc-footer-warning">{t('notifications.criticalUnread', { count: criticalCount })}</span>
            )}
            {criticalCount === 0 && unreadCount > 0 && (
              <span>{t('notifications.unreadCount', { count: unreadCount })}</span>
            )}
            {criticalCount === 0 && unreadCount === 0 && (
              <span>{t('notifications.allHandled')}</span>
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
