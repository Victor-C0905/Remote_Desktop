import { createContext, useContext, useCallback, useEffect, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  useServersStore,
  formatLastConnected,
  getStatusIcon,
  getStatusColor,
} from "../stores/serversStore";
import type { ServerConfig } from "../stores/serversStore";

/* ── Types ─────────────────────────────────────────────── */

interface ConnectionInfo {
  server_id: string;
  host: string;
  port: number;
  transport: string;
  status: string;
  rttMs: number;
  connectedAt: number;
}

interface ServerManagerState {
  servers: ServerConfig[];
  activeServerId: string | null;
  activeServer: ServerConfig | null;
}

interface ServerManagerActions {
  addServer: (server: Omit<ServerConfig, "id" | "status" | "lastConnected">) => void;
  removeServer: (id: string) => void;
  connectServer: (id: string) => Promise<void>;
  disconnectServer: (id: string) => void;
  setActiveServer: (id: string) => void;
  updateServer: (id: string, updates: Partial<ServerConfig>) => void;
}

type ServerManagerContextType = ServerManagerState & ServerManagerActions;

/* ── Context ──────────────────────────────────────────── */

const ServerManagerContext = createContext<ServerManagerContextType | null>(null);

/* ── Provider ────────────────────────────────────────── */

interface ServerManagerProviderProps {
  children: ReactNode;
}

export function ServerManagerProvider({ children }: ServerManagerProviderProps) {
  // 使用 Zustand store
  const {
    servers,
    activeServerId,
    addServer,
    removeServer,
    updateServer,
    setActiveServerId,
    setServerStatus,
    resetAllStatus,
  } = useServersStore();

  const activeServer = servers.find((s) => s.id === activeServerId) || null;

  // 注意：不在这里重置状态，因为会干扰用户连接
  // onRehydrateStorage 已经在 serversStore 中处理了重置逻辑

  const connectServer = useCallback(async (id: string) => {
    console.log("[ServerManager] 开始连接服务器:", id);
    const server = servers.find((s) => s.id === id);
    if (!server) {
      console.error("[ServerManager] 未找到服务器:", id);
      return;
    }

    // 如果当前有其他连接，先断开（像切换WiFi一样）
    if (activeServerId && activeServerId !== id) {
      console.log("[ServerManager] 断开当前连接:", activeServerId);
      try {
        await invoke("remote_disconnect", { serverId: activeServerId });
        setServerStatus(activeServerId, "disconnected");
      } catch (e) {
        console.warn("[ServerManager] 断开旧连接时出错:", e);
      }
    }

    console.log("[ServerManager] 服务器信息:", server);

    // 设置连接中状态
    setServerStatus(id, "connecting");

    try {
      console.log("[ServerManager] 调用 remote_connect:", {
        serverId: id,
        host: server.host,
        port: server.port,
        token: server.token || null,
      });

      const info = await invoke<ConnectionInfo>("remote_connect", {
        serverId: id,
        host: server.host,
        port: server.port,
        token: server.token || null,
      });

      console.log("[ServerManager] 连接成功:", info);

      setServerStatus(id, "connected", undefined, info.rttMs >= 0 ? info.rttMs : undefined);
      setActiveServerId(id);

      console.log("[ServerManager] 连接成功:", info.transport, `RTT=${info.rttMs}ms`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error("[ServerManager] 连接失败:", msg);

      setServerStatus(id, "error", msg);
    }
  }, [servers, activeServerId, setServerStatus, setActiveServerId]);

  const disconnectServer = useCallback(async (id: string) => {
    try {
      await invoke("remote_disconnect", { serverId: id });
    } catch (e) {
      console.warn("[ServerManager] 断开连接时出错:", e);
    }

    setServerStatus(id, "disconnected");
    if (activeServerId === id) {
      setActiveServerId(null);
    }
  }, [activeServerId, setServerStatus, setActiveServerId]);

  const setActiveServer = useCallback((id: string) => {
    const server = servers.find((s) => s.id === id);
    if (server && server.status === "connected") {
      setActiveServerId(id);
    }
  }, [servers, setActiveServerId]);

  const value: ServerManagerContextType = {
    servers,
    activeServerId,
    activeServer,
    addServer,
    removeServer,
    connectServer,
    disconnectServer,
    setActiveServer,
    updateServer,
  };

  return (
    <ServerManagerContext.Provider value={value}>
      {children}
    </ServerManagerContext.Provider>
  );
}

/* ── Hook ────────────────────────────────────────────── */

export function useServerManager(): ServerManagerContextType {
  const context = useContext(ServerManagerContext);
  if (!context) {
    throw new Error("useServerManager must be used within ServerManagerProvider");
  }
  return context;
}

/* ── Re-export Utility Functions ─────────────────────── */

export { formatLastConnected, getStatusColor, getStatusIcon };