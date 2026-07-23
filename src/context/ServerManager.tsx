import { createContext, useContext, useCallback, useEffect, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  useServersStore,
  formatLastConnected,
  getStatusIcon,
  getStatusColor,
} from "../stores/serversStore";
import type { ServerConfig } from "../stores/serversStore";
import { createLogger } from "../utils/logger";

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

/* ── Logger ──────────────────────────────────────────── */

const log = createLogger('ServerManager');

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
  } = useServersStore();

  const activeServer = servers.find((s) => s.id === activeServerId) || null;

  // 监听连接丢失事件
  useEffect(() => {
    log.info("设置连接丢失监听器");

    const setupListener = async () => {
      const unlisten = await listen<string>("connection-lost", (event) => {
        log.info("收到连接丢失事件:", event.payload);
        const lostServerId = event.payload;

        // 更新服务器状态为 disconnected
        setServerStatus(lostServerId, "disconnected");

        // 如果是当前活跃服务器，设置 activeServerId 为 null
        if (activeServerId === lostServerId) {
          log.info("当前活跃服务器断开，清空 activeServerId");
          setActiveServerId(null);
        }

        // TODO: 显示通知提示用户（可以在 UI 中添加通知系统）
        log.warn(`服务器 ${lostServerId} 连接已断开`);
      });

      return unlisten;
    };

    let unlistenFn: (() => void) | undefined;
    setupListener().then((fn) => {
      unlistenFn = fn;
    }).catch((e) => log.error('连接丢失监听设置失败:', e));

    // 清理监听器
    return () => {
      if (unlistenFn) {
        log.info("清理连接丢失监听器");
        unlistenFn();
      }
    };
  }, [activeServerId, setServerStatus, setActiveServerId]);

  // 注意：不在这里重置状态，因为会干扰用户连接
  // onRehydrateStorage 已经在 serversStore 中处理了重置逻辑

  const connectServer = useCallback(async (id: string) => {
    log.info("开始连接服务器:", id);
    const server = servers.find((s) => s.id === id);
    if (!server) {
      log.error("未找到服务器:", id);
      return;
    }

    // 如果当前有其他连接，先断开（像切换WiFi一样）
    if (activeServerId && activeServerId !== id) {
      log.info("断开当前连接:", activeServerId);
      try {
        await invoke("remote_disconnect", { serverId: activeServerId });
        setServerStatus(activeServerId, "disconnected");
      } catch (e) {
        log.warn("断开旧连接时出错:", e);
      }
    }

    log.info("服务器信息:", server);

    // 设置连接中状态
    setServerStatus(id, "connecting");

    try {
      log.info("调用 remote_connect:", {
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

      log.info("连接成功:", info);

      setServerStatus(id, "connected", undefined, info.rttMs >= 0 ? info.rttMs : undefined);
      setActiveServerId(id);

      // 连接成功后，自动订阅系统指标（长期状态）
      try {
        await invoke('subscribe', {
          serverId: id,
          types: [{ type: 'metrics', params: { interval_secs: 1 } }]
        });
      } catch (e) {
        log.warn("系统指标订阅失败:", e);
      }

      log.info("连接成功:", info.transport, `RTT=${info.rttMs}ms`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.error("连接失败:", msg);

      setServerStatus(id, "error", msg);
    }
  }, [servers, activeServerId, setServerStatus, setActiveServerId]);

  const disconnectServer = useCallback(async (id: string) => {
    // 取消订阅（不阻塞：后台任务的清理由 remote_disconnect 的 abort 接管）
    invoke('unsubscribe', {
      serverId: id,
      types: [{ type: 'metrics', params: { interval_secs: 1 } }]
    }).catch((e) => {
      log.warn("取消订阅失败（非关键）:", e);
    });

    // 断开连接（不等待 unsubscribe 完成，后端 Disconnect 会清理所有子任务）
    try {
      await invoke("remote_disconnect", { serverId: id });
    } catch (e) {
      log.warn("断开连接时出错:", e);
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