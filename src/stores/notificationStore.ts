import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { notificationsStorage } from "../utils/storage";

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
  /** 可执行动作（如「撤销移动」；闭包仅存于内存，随会话消亡） */
  onAction?: { label: string; run: () => void };
  /** 明细行（如移动逐条 src → dst）；通知中心点击条目展开 */
  details?: string[];
  /** 是否弹出到达预览条（false = 仅入通知中心，不打扰）；缺省视为 true */
  toast?: boolean;
  /** 实时到达标记：pushNotification 置位、持久化时剥离——预览条只弹
   *  带 live 的通知（刷新后恢复的存量记录自然静默） */
  live?: boolean;
  read: boolean;
}

interface NotificationState {
  notifications: AppNotification[];
  pushNotification: (n: Omit<AppNotification, "id" | "timestamp" | "read">) => void;
  markAsRead: (id: string) => void;
  markAllRead: () => void;
  dismiss: (id: string) => void;
  clearAll: () => void;
  /** 删除服务器时联动清理其持久化通知（serversStore.removeServer 调用） */
  purgeServer: (serverId: string) => void;
}

/* ── 自增 ID（会话内唯一即可） ─────────────────────────── */

let nextId = 0;

/** 持久化条目上限（写入时裁剪最旧） */
const NOTIFICATION_CAP = 100;

/* ── Store ─────────────────────────────────────────────── */

// 通知按服务器持久化（notifications.bin；serverId 标记归属，删除服务器时
// purgeServer 联动清理）。onAction 撤销闭包不序列化（JSON 丢弃函数值），
// 刷新后撤销按钮自然消失——移动可逆性仅限当轮会话
export const useNotificationStore = create<NotificationState>()(
  persist(
    (set) => ({
      notifications: [],
      pushNotification: (n) =>
        set((s) => ({
          notifications: [
            { ...n, id: `notif-${Date.now()}-${nextId++}`, timestamp: Date.now(), read: false, live: true },
            ...s.notifications,
          ].slice(0, NOTIFICATION_CAP),
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
      purgeServer: (serverId) =>
        set((s) => ({ notifications: s.notifications.filter((n) => n.serverId !== serverId) })),
    }),
    {
      name: "notifications",
      storage: createJSONStorage(() => notificationsStorage),
      // 持久化剥离 live 标记（实时到达凭证不落盘）：刷新后恢复的条目自然
      // 转为存量记录，预览条不再重弹；onAction 闭包由 JSON 序列化自动丢弃
      partialize: (s) => ({
        notifications: s.notifications.slice(0, NOTIFICATION_CAP).map(({ live, ...n }) => n),
      }),
    },
  ),
);
