import { createContext, useContext, useState, useEffect, useCallback, ReactNode } from "react";

/* ── Types ─────────────────────────────────────────────── */

export interface ServerConfig {
  id: string;
  name: string;
  host: string;
  port: number;
  token?: string;
  lastConnected?: number;
  status: "connected" | "disconnected" | "connecting" | "error";
  error?: string;
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

/* ── Storage Key ─────────────────────────────────────── */

const STORAGE_KEY = "gnome-remote-servers";

/* ── Provider ────────────────────────────────────────── */

interface ServerManagerProviderProps {
  children: ReactNode;
}

export function ServerManagerProvider({ children }: ServerManagerProviderProps) {
  const [servers, setServers] = useState<ServerConfig[]>([]);
  const [activeServerId, setActiveServerId] = useState<string | null>(null);

  // Load from localStorage on mount
  useEffect(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (stored) {
        const parsed = JSON.parse(stored) as ServerConfig[];
        // Reset status to disconnected on load
        const resetServers = parsed.map(s => ({
          ...s,
          status: "disconnected" as const,
          error: undefined,
        }));
        setServers(resetServers);
      }
    } catch (e) {
      console.warn("[ServerManager] Failed to load servers from storage:", e);
    }
  }, []);

  // Save to localStorage on change
  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(servers));
    } catch (e) {
      console.warn("[ServerManager] Failed to save servers to storage:", e);
    }
  }, [servers]);

  const activeServer = servers.find(s => s.id === activeServerId) || null;

  const generateId = () => `srv-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

  const addServer = useCallback(
    (server: Omit<ServerConfig, "id" | "status" | "lastConnected">) => {
      const newServer: ServerConfig = {
        ...server,
        id: generateId(),
        status: "disconnected",
      };
      setServers(prev => [...prev, newServer]);
    },
    []
  );

  const removeServer = useCallback((id: string) => {
    setServers(prev => prev.filter(s => s.id !== id));
    if (activeServerId === id) {
      setActiveServerId(null);
    }
  }, [activeServerId]);

  const connectServer = useCallback(async (id: string) => {
    // Set connecting status
    setServers(prev => prev.map(s => 
      s.id === id ? { ...s, status: "connecting", error: undefined } : s
    ));

    // Simulate connection (in real app, this would use QUIC)
    await new Promise(resolve => setTimeout(resolve, 1000));

    // Demo: randomly succeed or fail
    const success = Math.random() > 0.2;

    if (success) {
      setServers(prev => prev.map(s => 
        s.id === id ? { 
          ...s, 
          status: "connected", 
          lastConnected: Date.now(),
          error: undefined,
        } : s
      ));
      setActiveServerId(id);
    } else {
      setServers(prev => prev.map(s => 
        s.id === id ? { 
          ...s, 
          status: "error", 
          error: "连接失败: 无法访问服务器",
        } : s
      ));
    }
  }, []);

  const disconnectServer = useCallback((id: string) => {
    setServers(prev => prev.map(s => 
      s.id === id ? { ...s, status: "disconnected", error: undefined } : s
    ));
    if (activeServerId === id) {
      setActiveServerId(null);
    }
  }, [activeServerId]);

  const setActiveServer = useCallback((id: string) => {
    const server = servers.find(s => s.id === id);
    if (server && server.status === "connected") {
      setActiveServerId(id);
    }
  }, [servers]);

  const updateServer = useCallback((id: string, updates: Partial<ServerConfig>) => {
    setServers(prev => prev.map(s => 
      s.id === id ? { ...s, ...updates } : s
    ));
  }, []);

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