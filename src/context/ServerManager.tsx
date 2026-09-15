import { createContext, useContext, useCallback, useEffect, useRef, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  useServersStore,
  formatLastConnected,
  getStatusIcon,
  getStatusColor,
} from "../stores/serversStore";
import type { ServerConfig } from "../stores/serversStore";
import { useNotificationStore } from "../stores/notificationStore";
import { parseConnectError, getErrorInfo, buildConnectFailureText, isRetryableConnectError } from "../types/errors";
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

/**
 * connection-lost 事件负载（Rust 统一清理块 emit）
 * - user_initiated: 用户主动断开，前端不自动重连
 * - heartbeat / quic_closed / send_failed: 网络断开，前端自动重连
 * - code: 断连原因分类（AuthErrorCode 数值；null 为通用网络断开）
 */
interface ConnectionLostPayload {
  server_id: string;
  source: "user_initiated" | "heartbeat" | "quic_closed" | "send_failed";
  code?: number | null;
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

/* ── 自动重连策略（硬性约束：最多 3 次、5 秒间隔） ───────── */

const MAX_RECONNECT_ATTEMPTS = 3;
const RECONNECT_INTERVAL_MS = 5000;

// 可重试判定由 errors.ts 的映射表接管（isRetryableConnectError）：
// 认证被拒/证书被拒等确定性失败不重试，避免反复撞密码
// 触发服务端认证失败锁定（5 次失败锁定 15 分钟）。

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
    setServerRtt,
  } = useServersStore();

  const activeServer = servers.find((s) => s.id === activeServerId) || null;

  // 用 ref 持有最新值，避免 useEffect 闭包捕获过时值
  const activeServerIdRef = useRef(activeServerId);
  activeServerIdRef.current = activeServerId;

  // 待自动重连的服务器集合（防止同一服务器重复调度重连定时器）
  const pendingReconnectRef = useRef<Set<string>>(new Set());

  // 连接核心逻辑的稳定引用（手动连接与自动重连共用，见 performConnect）
  const performConnectRef = useRef<(server: ServerConfig) => Promise<void>>(async () => {});
  // 调度重连的稳定引用（connection-lost 监听器与重连失败重试共用）
  const scheduleReconnectRef = useRef<(serverId: string, attempt: number) => void>(() => {});

  // 监听连接丢失事件
  useEffect(() => {
    log.info("设置连接丢失监听器");

    const setupListener = async () => {
      const unlisten = await listen<ConnectionLostPayload>("connection-lost", (event) => {
        const { server_id: lostServerId, source, code } = event.payload;
        log.info("收到连接丢失事件:", lostServerId, "来源:", source, "code:", code);

        // 通过 ref 读取最新值，而非闭包捕获
        if (activeServerIdRef.current === lostServerId) {
          log.info("当前活跃服务器断开，清空 activeServerId");
          setActiveServerId(null);
        }

        if (source === "user_initiated") {
          // 用户主动断开：不自动重连
          setServerStatus(lostServerId, "disconnected");
          log.warn(`服务器 ${lostServerId} 已主动断开`);
          return;
        }

        // 网络断开：进入自动重连流程（最多 3 次、5 秒间隔）
        // 状态置为 reconnecting，各应用依据 activeServerId 已清空显示离线占位，
        // 重连成功后恢复 activeServerId，应用现有 effect 自动恢复
        setServerStatus(lostServerId, "reconnecting");
        log.warn(`服务器 ${lostServerId} 连接断开（${source}），开始自动重连`);

        // 断连通知（code 携带分类：如认证超时/会话超时等明确原因）
        const lostServer = useServersStore.getState().servers.find((s) => s.id === lostServerId);
        const lostInfo = code != null ? getErrorInfo(code) : null;
        useNotificationStore.getState().pushNotification({
          title: lostInfo?.title ?? "网络连接中断",
          body: lostInfo?.message ?? "与服务器的连接已中断，正在自动重连",
          action: "自动重连中（最多 3 次）",
          urgency: "normal",
          source: lostServer ? lostServer.name || lostServer.host : "连接",
          serverId: lostServerId,
        });

        scheduleReconnectRef.current(lostServerId, 1);
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
    // 空依赖：监听器只注册一次，组件生命周期内不变
    // setServerStatus / setActiveServerId 是 Zustand 的稳定引用
  }, [setServerStatus, setActiveServerId]);

  // 监听证书信任事件（证书钉扎：存储服务器证书指纹）
  useEffect(() => {
    log.info("设置证书信任监听器");

    const setupListener = async () => {
      const unlisten = await listen<{ server_id: string; fingerprint: string }>(
        "cert-trusted",
        (event) => {
          log.info("收到证书信任事件:", event.payload);
          updateServer(event.payload.server_id, {
            certFingerprint: event.payload.fingerprint,
          });
        }
      );
      return unlisten;
    };

    let unlistenFn: (() => void) | undefined;
    setupListener().then((fn) => {
      unlistenFn = fn;
    }).catch((e) => log.error('证书信任监听设置失败:', e));

    return () => {
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [updateServer]);

  // 监听心跳 RTT 回传事件（Rust 端心跳每 5s Ping-Pong 顺便测量，免额外请求）
  // 高频低价值事件：不打日志，静默更新 store
  useEffect(() => {
    const setupListener = async () => {
      const unlisten = await listen<{ server_id: string; rtt_ms: number }>(
        "server_rtt",
        (event) => {
          setServerRtt(event.payload.server_id, event.payload.rtt_ms);
        }
      );
      return unlisten;
    };

    let unlistenFn: (() => void) | undefined;
    setupListener().then((fn) => {
      unlistenFn = fn;
    }).catch((e) => log.error('RTT 监听设置失败:', e));

    return () => {
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [setServerRtt]);

  // 注意：不在这里重置状态，因为会干扰用户连接
  // onRehydrateStorage 已经在 serversStore 中处理了重置逻辑

  // ── 连接通知派发（通知中心数据源） ──────────────────────
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  /** 连接失败通知（含多服务器对比提示——只读已有状态，零探测） */
  const notifyConnectFailure = useCallback((server: ServerConfig, err: unknown) => {
    const { code, detail } = parseConnectError(err);
    const info = getErrorInfo(code);
    const others = useServersStore
      .getState()
      .servers.filter((s) => s.id !== server.id && s.status === "connected").length;
    const body =
      `${info.message}${detail && detail !== info.title ? `\n${detail}` : ""}` +
      (others > 0
        ? `\n\nℹ️ 其他 ${others} 台服务器连接正常，仅此台无法连接\n可能是本机与该服务器之间的网络问题，建议更换网络环境后重试`
        : "");
    pushNotification({
      title: info.title,
      body,
      action: info.action,
      urgency: info.severity,
      source: server.name || server.host,
      serverId: server.id,
    });
  }, [pushNotification]);

  // 连接核心逻辑：认证凭据 → remote_connect → 状态更新 → 订阅指标
  // 手动连接（connectServer）与自动重连（attemptReconnect）共用
  const performConnect = useCallback(async (server: ServerConfig) => {
    // 准备认证凭据（使用 snake_case 命名以匹配 Rust 后端）
    // 兼容旧配置：如果 server.auth 不存在，使用默认值
    const defaultAuth = {
      method: "password" as const,
      username: "",
      password: undefined,
      privateKey: undefined,
      passphrase: undefined,
    };

    const auth = server.auth || defaultAuth;

    const credentials = {
      method: auth.method,
      username: auth.username,
      password: auth.password,
      private_key: auth.privateKey,
      passphrase: auth.passphrase,
    };

    // 注意：不记录 credentials 中的敏感信息
    log.info("调用 remote_connect:", {
      serverId: server.id,
      host: server.host,
      port: server.port,
      token: server.token || null,
      authMethod: auth.method,
      username: auth.username,
    });

    const info = await invoke<ConnectionInfo>("remote_connect", {
      serverId: server.id,
      host: server.host,
      port: server.port,
      token: server.token || null,
      credentials,
      certFingerprint: server.certFingerprint || null,
    });

    setServerStatus(server.id, "connected", undefined, info.rttMs >= 0 ? info.rttMs : undefined);
    setActiveServerId(server.id);

    // 连接成功后，自动订阅系统指标（长期状态）
    try {
      await invoke('subscribe', {
        serverId: server.id,
        types: [{ type: 'metrics', params: { interval_secs: 1 } }]
      });
    } catch (e) {
      log.warn("系统指标订阅失败:", e);
    }

    log.info("连接成功:", info.transport, `RTT=${info.rttMs}ms`);
  }, [setServerStatus, setActiveServerId]);

  // 持有最新 performConnect 引用（供 setTimeout 回调使用，避免过时闭包）
  performConnectRef.current = performConnect;

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

    // 设置连接中状态
    setServerStatus(id, "connecting");

    try {
      await performConnect(server);
    } catch (err) {
      // 结构化解析 → 映射表文案（含对比提示）→ 状态 + 通知双通道
      const others = servers.filter((s) => s.id !== id && s.status === "connected").length;
      const display = buildConnectFailureText(err, others);
      log.error("连接失败:", display);

      // store 存结构化错误（Settings/应用错误态查表渲染）；display 完整文本仅用于日志与通知
      setServerStatus(id, "error", parseConnectError(err));
      notifyConnectFailure(server, err);
    }
  }, [servers, activeServerId, setServerStatus, setActiveServerId, performConnect, notifyConnectFailure]);

  // ── 自动重连（网络断开时） ────────────────────────────────
  // 策略：最多 3 次、5 秒间隔；凭据复用已保存配置，无需用户干预。
  // 自动取消条件：服务器被删除、用户手动连接/断开（状态不再是
  // reconnecting）、其他服务器已连接、认证被拒等确定性失败。

  const attemptReconnect = useCallback(async (serverId: string, attempt: number) => {
    pendingReconnectRef.current.delete(serverId);

    // 读取最新 store 状态（setTimeout 回调内不能依赖组件闭包）
    const state = useServersStore.getState();
    const server = state.servers.find((s) => s.id === serverId);
    if (!server) return; // 服务器已删除

    // 仅在 reconnecting 状态继续：用户手动操作（连接/断开）会改变状态，即自动取消
    if (server.status !== "reconnecting") return;

    // 单连接模型：其他服务器已连接时放弃自动重连（用户已切换目标）
    if (state.activeServerId && state.activeServerId !== serverId) return;
    if (state.servers.some((s) => s.id !== serverId && s.status === "connected")) return;

    log.info(`自动重连第 ${attempt}/${MAX_RECONNECT_ATTEMPTS} 次:`, serverId);
    try {
      await performConnectRef.current(server);
      log.info("自动重连成功:", serverId);
      // 重连成功通知（连接恢复）
      useNotificationStore.getState().pushNotification({
        title: "连接已恢复",
        body: `服务器 ${server.name || server.host} 已重新连接`,
        urgency: "low",
        source: server.name || server.host,
        serverId,
      });
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log.warn(`自动重连失败 (${attempt}/${MAX_RECONNECT_ATTEMPTS}):`, msg);

      // 确定性失败（认证被拒/证书被拒等）不重试，避免触发服务端认证锁定
      // 门控从字符串白名单升级为映射表查表（isRetryableConnectError 来自 errors.ts）
      if (!isRetryableConnectError(err) || attempt >= MAX_RECONNECT_ATTEMPTS) {
        const others = useServersStore
          .getState()
          .servers.filter((s) => s.id !== serverId && s.status === "connected").length;
        const display = `自动重连失败: ${buildConnectFailureText(err, others)}`;
        log.warn(display);
        setServerStatus(serverId, "error", parseConnectError(err));
        useNotificationStore.getState().pushNotification({
          title: "自动重连失败",
          body: buildConnectFailureText(err, others),
          action: getErrorInfo(parseConnectError(err).code).action,
          urgency: "critical",
          source: server.name || server.host,
          serverId,
        });
        return;
      }
      scheduleReconnectRef.current(serverId, attempt + 1);
    }
  }, [setServerStatus]);

  const scheduleReconnect = useCallback((serverId: string, attempt: number) => {
    // 同一服务器只保留一个待执行的重连定时器
    if (pendingReconnectRef.current.has(serverId)) return;
    pendingReconnectRef.current.add(serverId);

    window.setTimeout(() => {
      void attemptReconnect(serverId, attempt);
    }, RECONNECT_INTERVAL_MS);
  }, [attemptReconnect]);

  // 持有最新调度引用（connection-lost 监听器空依赖，需通过 ref 访问）
  scheduleReconnectRef.current = scheduleReconnect;

  const disconnectServer = useCallback(async (id: string) => {
    // 注意：不调用 unsubscribe。
    // remote_disconnect 的统一清理块会 abort subscription_task 并清理所有子任务，
    // 先调 unsubscribe 会因连接已断开报"未找到连接"噪音日志。

    // 断开连接（后端清理块会接管所有子任务清理）
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