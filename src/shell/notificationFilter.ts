// src/shell/notificationFilter.ts
// 通知中心筛选纯函数：严重度 chip ∧ 未读开关 ∧ 搜索子串的叠加与清空作用域。
// 与组件解耦，便于单测；origin 由调用方注入（NotificationCenter.originOf）。

import type { AppNotification, NotificationUrgency } from "../stores/notificationStore";

/** 通知中心筛选视图（与严重度 chips 对齐） */
export type NotificationFilter = "all" | NotificationUrgency | "other";

/** 严重度 chip 匹配：all 恒真；other 兜底收纳三类之外的未知紧急度 */
export function matchesSeverity(n: AppNotification, filter: NotificationFilter): boolean {
  if (filter === "all") return true;
  if (filter === "other") {
    return n.urgency !== "critical" && n.urgency !== "normal" && n.urgency !== "low";
  }
  return n.urgency === filter;
}

/** 搜索子串匹配：title/body/来源（origin 由调用方算好传入），不区分大小写；空白串恒真 */
export function matchesQuery(n: AppNotification, query: string, origin: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    n.title.toLowerCase().includes(q) ||
    n.body.toLowerCase().includes(q) ||
    origin.toLowerCase().includes(q)
  );
}

export interface NotificationFilterOptions {
  filter: NotificationFilter;
  query: string;
  originOf: (n: AppNotification) => string;
}

/** 组合过滤：severity ∧ query（列表展示用，保持输入顺序） */
export function applyNotificationFilters(
  notifications: AppNotification[],
  opts: NotificationFilterOptions,
): AppNotification[] {
  return notifications.filter((n) => {
    if (!matchesSeverity(n, opts.filter)) return false;
    if (!matchesQuery(n, opts.query, opts.originOf(n))) return false;
    return true;
  });
}

/**
 * 清空按钮的作用域（用户拍板：跟随当前筛选视图）：severity。
 * 搜索词不参与清空范围——清空只受 chips 约束；「全部已读」同理不在此列。
 */
export function buildClearFilter(
  opts: { filter: NotificationFilter },
): (n: AppNotification) => boolean {
  return (n) => matchesSeverity(n, opts.filter);
}
