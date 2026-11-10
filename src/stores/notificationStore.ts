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
  /** 关联服务器（清理与按服务器隔离用；系统级通知为空） */
  serverId?: string;
  /** 服务器名快照：推送时记录，服务器改名/删除后仍能辨认归属 */
  serverName?: string;
  /** 全局可见：连接建立/断开/重连等可用性通知不随当前服务器过滤 */
  global?: boolean;
  /** 旧版本落盘、无法判断归属的记录：永久隐藏（不参与任何通知视图） */
  legacy?: true;
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
  markAllRead: (filter?: (n: AppNotification) => boolean) => void;
  dismiss: (id: string) => void;
  clearAll: (filter?: (n: AppNotification) => boolean) => void;
  /** 删除服务器时联动清理其持久化通知（serversStore.removeServer 调用） */
  purgeServer: (serverId: string) => void;
}

/**
 * 通知是否可见（彻底按服务器隔离）
 *
 * - global（连接/断连/重连等可用性通知）始终可见（用户明确要求）；
 * - 无 serverId 的系统通知（壁纸、凭据等）始终可见；
 * - legacy 旧记录不参与（无法判断归属，避免跨服务器泄漏）；
 * - 其余通知只在当前活动服务器下可见，切到对应服务器才能看到。
 */
export function isNotificationVisible(
  n: AppNotification,
  activeServerId: string | null | undefined,
): boolean {
  if (n.legacy) return false;
  if (n.global) return true;
  if (!n.serverId) return true;
  return activeServerId != null && n.serverId === activeServerId;
}

/**
 * v0→v1 迁移：旧版本文件操作通知没有 serverId，若继续按「系统通知」处理，
 * 会在所有服务器下可见（跨服泄漏）。这里给这类记录打 legacy 标记，
 * 永久排除在通知中心/角标/预览条之外。
 */
export function migrateNotificationState(persisted: unknown): unknown {
  const state = (persisted ?? {}) as Record<string, unknown>;
  const list = Array.isArray(state.notifications)
    ? (state.notifications as AppNotification[])
    : [];
  return {
    ...state,
    notifications: list.slice(0, NOTIFICATION_CAP).map((n) =>
      !n.serverId && !n.global ? { ...n, legacy: true as const } : n,
    ),
  };
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
      markAllRead: (filter) =>
        set((s) => ({
          // 只把当前服务器视图内的通知标为已读，隐藏服务器的未读状态保留
          notifications: s.notifications.map((n) =>
            !filter || filter(n) ? { ...n, read: true } : n,
          ),
        })),
      dismiss: (id) =>
        set((s) => ({ notifications: s.notifications.filter((n) => n.id !== id) })),
      clearAll: (filter) =>
        set((s) => ({
          // 只清除当前服务器视图内的通知
          notifications: filter ? s.notifications.filter((n) => !filter(n)) : [],
        })),
      purgeServer: (serverId) =>
        set((s) => ({ notifications: s.notifications.filter((n) => n.serverId !== serverId) })),
    }),
    {
      name: "notifications",
      // v1：旧记录打 legacy 标记（见 migrateNotificationState）。
      // 必须与 storage.ts 的 EXPECTED_STORE_VERSIONS 同步，否则会被判为
      // 更高版本而阻断 notifications.bin 写入
      version: 1,
      migrate: (persisted) => migrateNotificationState(persisted) as never,
      storage: createJSONStorage(() => notificationsStorage),
      // 持久化剥离 live 标记（实时到达凭证不落盘）：刷新后恢复的条目自然
      // 转为存量记录，预览条不再重弹；onAction 闭包由 JSON 序列化自动丢弃
      partialize: (s) => ({
        notifications: s.notifications.slice(0, NOTIFICATION_CAP).map(({ live, ...n }) => n),
      }),
    },
  ),
);
