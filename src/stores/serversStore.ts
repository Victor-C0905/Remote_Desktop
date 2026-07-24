import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { serversStorage } from "../utils/storage";
import { createLogger } from "../utils/logger";
import { ServerConfig, AuthMethod } from "../types/server";

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
  setServerStatus: (id: string, status: ServerConfig["status"], error?: string, rttMs?: number) => void;
  resetAllStatus: () => void;
}

/* ── Helper ───────────────────────────────────────────── */

const generateId = () => `srv-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

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
        set({ servers: [...get().servers, newServer] });
      },

      removeServer: (id) => {
        const servers = get().servers.filter((s) => s.id !== id);
        const activeServerId = get().activeServerId === id ? null : get().activeServerId;
        set({ servers, activeServerId });
      },

      updateServer: (id, updates) => {
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
      name: "gnome-remote-servers",
      storage: createJSONStorage(() => serversStorage),
      // 加载时重置所有服务器状态为 disconnected
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
          }
        };
      },
    }
  )
);

/* ── Utility Functions ──────────────────────────────── */

export function formatLastConnected(timestamp?: number): string {
  if (!timestamp) return "从未";
  const now = Date.now();
  const diff = now - timestamp;

  if (diff < 1000 * 60) return "刚刚";
  if (diff < 1000 * 60 * 60) return `${Math.floor(diff / 60000)} 分钟前`;
  if (diff < 1000 * 60 * 60 * 24) return `${Math.floor(diff / 3600000)} 小时前`;
  if (diff < 1000 * 60 * 60 * 24 * 7) return `${Math.floor(diff / 86400000)} 天前`;
  return new Date(timestamp).toLocaleDateString("zh-CN");
}

export function getStatusColor(status: ServerConfig["status"]): string {
  switch (status) {
    case "connected": return "#33d17a";
    case "connecting": return "#e8a416";
    case "disconnected": return "#9a9996";
    case "error": return "#e01b24";
  }
}

export function getStatusIcon(status: ServerConfig["status"]): string {
  switch (status) {
    case "connected": return "🟢";
    case "connecting": return "🟡";
    case "disconnected": return "⚫";
    case "error": return "🔴";
  }
}