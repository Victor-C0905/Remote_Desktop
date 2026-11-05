import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { createLogger } from "../utils/logger";
import { formatUnknownError } from "../types/errors";
import type { ArchiveCapabilities, ArchiveFormatCapability } from "../types/archive";

const log = createLogger("ArchiveCapabilities");

/** 单服务器能力状态 */
export interface ServerArchiveCapabilities {
  status: "probing" | "ready" | "failed";
  /** 探测结果（status=ready 时有值） */
  data?: ArchiveCapabilities;
  /** 失败原因（status=failed 时） */
  error?: string;
}

interface ArchiveCapabilitiesState {
  byServer: Record<string, ServerArchiveCapabilities>;
  /** 查询能力（refresh=true 强制 Agent 重新探测） */
  fetchCapabilities: (serverId: string, refresh?: boolean) => Promise<void>;
  /** 读取单格式能力（未探测/失败时返回 undefined） */
  formatFor: (serverId: string, formatId: string | null) => ArchiveFormatCapability | undefined;
  /** 服务器删除/断开时清理 */
  clearServer: (serverId: string) => void;
}

/** 并发去重：同一服务器的进行中请求只保留一个 */
const inflight = new Map<string, Promise<void>>();

export const useArchiveCapabilitiesStore = create<ArchiveCapabilitiesState>()((set, get) => ({
  byServer: {},

  fetchCapabilities: async (serverId, refresh = false) => {
    if (!serverId) return;
    const existing = get().byServer[serverId];
    // 已有探测中的请求：直接复用，避免连接后多处触发重复探测
    const pending = inflight.get(serverId);
    if (pending && !refresh) return pending;

    set((s) => ({
      byServer: {
        ...s.byServer,
        [serverId]: { status: existing?.status === "ready" && !refresh ? "ready" : "probing", data: existing?.data },
      },
    }));

    const task = (async () => {
      try {
        const data = await invoke<ArchiveCapabilities>("remote_archive_capabilities", {
          serverId,
          refresh,
        });
        set((s) => ({
          byServer: { ...s.byServer, [serverId]: { status: "ready", data } },
        }));
      } catch (e) {
        const message = formatUnknownError(e);
        log.warn("压缩能力探测失败:", serverId, message);
        set((s) => ({
          byServer: { ...s.byServer, [serverId]: { status: "failed", error: message } },
        }));
      } finally {
        inflight.delete(serverId);
      }
    })();
    inflight.set(serverId, task);
    return task;
  },

  formatFor: (serverId, formatId) => {
    if (!formatId) return undefined;
    return get().byServer[serverId]?.data?.formats.find((f) => f.id === formatId);
  },

  clearServer: (serverId) => {
    inflight.delete(serverId);
    set((s) => {
      const next = { ...s.byServer };
      delete next[serverId];
      return { byServer: next };
    });
  },
}));
