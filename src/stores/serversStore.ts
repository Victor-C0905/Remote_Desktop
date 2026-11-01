import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { serversStorage } from "../utils/storage";
import { createLogger } from "../utils/logger";
import { ServerConfig, AuthMethod, StructuredError } from "../types/server";
import { getErrorInfo, parseConnectError } from "../types/errors";
import i18n from "../i18n";
import { hasSecrets, secretsSet, secretsDelete, stripSecrets, hydrateCredentials, isKeyringReady } from "../utils/credentials";

const log = createLogger('ServersStore');

/* ── Types ─────────────────────────────────────────────── */

// ServerConfig 类型已移动到 src/types/server.ts
// 重新导出以保持向后兼容
export type { ServerConfig } from "../types/server";
export { AuthMethod } from "../types/server";

export interface ServersState {
  servers: ServerConfig[];
  activeServerId: string | null;
}

export interface ServersActions {
  addServer: (server: Omit<ServerConfig, "id" | "status" | "lastConnected" | "auth"> & { auth?: Partial<ServerConfig["auth"]> }) => void;
  removeServer: (id: string) => void;
  updateServer: (id: string, updates: Partial<ServerConfig>) => void;
  setActiveServerId: (id: string | null) => void;
  setServerStatus: (id: string, status: ServerConfig["status"], error?: StructuredError | string, rttMs?: number) => void;
  /** 仅更新 RTT（心跳 server_rtt 事件）：不动 status/error，避免覆盖重连中等中间状态 */
  setServerRtt: (id: string, rttMs: number) => void;
  resetAllStatus: () => void;
}

/* ── Helper ───────────────────────────────────────────── */

const generateId = () => `srv-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

/**
 * persist 版本迁移（当前基线 v1：keyring 信封加密 + partialize 剥离凭据）。
 *
 * 旧版本落盘数据没有 version 字段，zustand 按 0 处理：若只声明 version: 1
 * 而不提供 migrate，zustand 会跳过整个重水合（用户全部服务器条目蒸发）。
 * 故 v0→v1 为原样透传，字段级清洗由 onRehydrateStorage（状态重置 + keyring
 * 装配/旧明文迁移）承担。约定：未来改持久化结构必须递增 version 并在此补分支，
 * 禁止让 persist 静默浅合并演进。
 */
export function persistMigrate(persisted: unknown, version: number): unknown {
  void version;
  return persisted;
}

/* ── Store ────────────────────────────────────────────── */

export const useServersStore = create<ServersState & ServersActions>()(
  persist(
    (set, get) => ({
      servers: [],
      activeServerId: null,

      addServer: (server) => {
        // 提供默认的认证配置
        const defaultAuth: ServerConfig["auth"] = {
          method: AuthMethod.PASSWORD,
          username: "",
          password: undefined,
          privateKey: undefined,
          passphrase: undefined,
        };

        const newServer: ServerConfig = {
          ...server,
          id: generateId(),
          status: "disconnected",
          auth: {
            ...defaultAuth,
            ...server.auth,
          },
        };
        // 凭据进系统凭据库（keyring），servers.bin 只留非敏感字段
        if (hasSecrets(newServer.auth)) {
          void secretsSet(newServer.id, newServer.auth);
        }
        set({ servers: [...get().servers, newServer] });
      },

      removeServer: (id) => {
        void secretsDelete(id);
        const servers = get().servers.filter((s) => s.id !== id);
        const activeServerId = get().activeServerId === id ? null : get().activeServerId;
        set({ servers, activeServerId });
        // 联动清理该服务器的持久化通知（跨 store 单向调用，无循环）
        void import("./notificationStore").then((m) => m.useNotificationStore.getState().purgeServer(id));
      },

      updateServer: (id, updates) => {
        if (updates.auth && hasSecrets(updates.auth)) {
          void secretsSet(id, updates.auth);
        }
        set({
          servers: get().servers.map((s) =>
            s.id === id ? { ...s, ...updates } : s
          ),
        });
      },

      setActiveServerId: (id) => {
        set({ activeServerId: id });
      },

      setServerStatus: (id, status, error, rttMs) => {
        set({
          servers: get().servers.map((s) =>
            s.id === id
              ? {
                  ...s,
                  status,
                  error: error ?? undefined,
                  rttMs: rttMs ?? undefined,
                  lastConnected: status === "connected" ? Date.now() : s.lastConnected,
                }
              : s
          ),
        });
      },

      setServerRtt: (id, rttMs) => {
        set({
          servers: get().servers.map((s) =>
            s.id === id ? { ...s, rttMs } : s
          ),
        });
      },

      resetAllStatus: () => {
        set({
          servers: get().servers.map((s) => ({
            ...s,
            status: "disconnected" as const,
            error: undefined,
          })),
          activeServerId: null,
        });
      },
    }),
    {
      name: "quirel-servers",
      version: 1,
      migrate: persistMigrate,
      storage: createJSONStorage(() => serversStorage),
      // 落盘前剥离凭据——但仅在 keyring 确认可写之后（isKeyringReady 门控）：
      // 迁移期写入失败时保留明文落盘，防止凭据两头落空直接蒸发
      partialize: (state) => ({
        servers: state.servers.map((s) => (isKeyringReady() ? stripSecrets(s) : s)),
        activeServerId: state.activeServerId,
      }),
      // 加载时重置所有服务器状态为 disconnected；随后从 keyring 装配凭据
      //（含旧版明文凭据的一次性迁移：写入 keyring 并从 state 剥离）
      onRehydrateStorage: () => {
        log.debug("onRehydrateStorage 开始");
        return (state) => {
          log.debug("onRehydrateStorage 回调，state:", state);
          if (state && state.servers) {
            log.debug("重置服务器状态");
            state.servers = state.servers.map((s) => ({
              ...s,
              status: "disconnected" as const,
              error: undefined,
            }));
            state.activeServerId = null;
            log.debug("重置后，activeServerId:", state.activeServerId);

            // keyring 装配 + 旧明文迁移（异步，不阻塞启动）
            void hydrateCredentials(state.servers).then((merged) => {
              useServersStore.setState({
                servers: merged.map((s) => ({ ...s, status: "disconnected" as const, error: undefined })),
              });
            });
          }
        };
      },
    }
  )
);

/* ── Utility Functions ──────────────────────────────── */

export function formatLastConnected(timestamp?: number): string {
  if (!timestamp) return i18n.t("servers.never");
  const now = Date.now();
  const diff = now - timestamp;

  if (diff < 1000 * 60) return i18n.t("servers.justNow");
  if (diff < 1000 * 60 * 60) return i18n.t("servers.minutesAgo", { n: Math.floor(diff / 60000) });
  if (diff < 1000 * 60 * 60 * 24) return i18n.t("servers.hoursAgo", { n: Math.floor(diff / 3600000) });
  if (diff < 1000 * 60 * 60 * 24 * 7) return i18n.t("servers.daysAgo", { n: Math.floor(diff / 86400000) });
  return new Date(timestamp).toLocaleDateString("zh-CN");
}

export function getStatusColor(status: ServerConfig["status"]): string {
  switch (status) {
    case "connected": return "var(--quirel-success-color)";
    case "connecting": return "var(--quirel-warning-color)";
    case "reconnecting": return "#ff7800";
    case "disconnected": return "#9a9996";
    case "error": return "var(--quirel-error-color)";
  }
}

export function getStatusIcon(status: ServerConfig["status"]): string {
  switch (status) {
    case "connected": return "🟢";
    case "connecting": return "🟡";
    case "reconnecting": return "🟠";
    case "disconnected": return "⚫";
    case "error": return "🔴";
  }
}

/** getServerErrorInfo 的返回结构（供 Settings/应用错误态渲染） */
export interface ServerErrorDisplay {
  title: string;
  message: string;
  action: string;
  detail?: string;
  /** 其他服务器连接正常时的对比提示（多行） */
  hint?: string;
}

/**
 * 组装服务器的结构化错误展示信息。
 * - 结构化 error → 查 ERROR_MAP 映射
 * - 字符串 error（旧格式）→ Unknown 映射 + detail
 * - hint 为「其他 N 台服务器连接正常」对比提示（读取当前 store，零探测）
 */
export function getServerErrorInfo(server: ServerConfig): ServerErrorDisplay {
  const parsed = server.error
    ? parseConnectError(server.error)
    : { code: 999, detail: undefined };
  const info = getErrorInfo(parsed.code);
  const others = useServersStore
    .getState()
    .servers.filter((s) => s.id !== server.id && s.status === "connected").length;
  return {
    title: info.title,
    message: info.message,
    action: info.action,
    detail: parsed.detail,
    hint: others > 0
      ? i18n.t("errors.othersConnected", { count: others })
      : undefined,
  };
}