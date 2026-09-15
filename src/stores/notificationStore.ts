import { create } from "zustand";

/* ── Types ─────────────────────────────────────────────── */

/** 通知严重度（与错误映射表 severity 对齐） */
export type NotificationUrgency = "low" | "normal" | "critical";

export interface AppNotification {
  id: string;
  /** 标题（如「网络连接中断」） */
  title: string;
  /** 正文（消息 + 可选对比提示） */
  body: string;
  /** 行动建议（如「请检查网络后重试」） */
  action?: string;
  timestamp: number;
  urgency: NotificationUrgency;
  /** 来源（服务器名或模块名） */
  source: string;
  /** 关联服务器（清理用；非服务器通知为空） */
  serverId?: string;
  read: boolean;
}

interface NotificationState {
  notifications: AppNotification[];
  pushNotification: (n: Omit<AppNotification, "id" | "timestamp" | "read">) => void;
  markAsRead: (id: string) => void;
  markAllRead: () => void;
  dismiss: (id: string) => void;
  clearAll: () => void;
}

/* ── 自增 ID（会话内唯一即可） ─────────────────────────── */

let nextId = 0;

/* ── Store ─────────────────────────────────────────────── */

// 通知属于瞬态 UI 状态，不持久化（遵循「存储全部走 Rust」约束，此处仅内存态）
export const useNotificationStore = create<NotificationState>((set) => ({
  notifications: [],
  pushNotification: (n) =>
    set((s) => ({
      notifications: [
        { ...n, id: `notif-${Date.now()}-${nextId++}`, timestamp: Date.now(), read: false },
        ...s.notifications,
      ].slice(0, 50), // 上限 50 条，防膨胀
    })),
  markAsRead: (id) =>
    set((s) => ({
      notifications: s.notifications.map((n) => (n.id === id ? { ...n, read: true } : n)),
    })),
  markAllRead: () =>
    set((s) => ({ notifications: s.notifications.map((n) => ({ ...n, read: true })) })),
  dismiss: (id) =>
    set((s) => ({ notifications: s.notifications.filter((n) => n.id !== id) })),
  clearAll: () => set({ notifications: [] }),
}));
