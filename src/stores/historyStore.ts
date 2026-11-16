// src/stores/historyStore.ts
// 浏览器历史：按服务器隔离（entriesByServer），最近在前、URL 去重（重访置顶并
// 刷新时间）、每服务器容量上限淘汰。持久化于 browser-data.bin；无痕会话由调用方
// （BrowserApp）豁免记录。
// - v1：旧版本全局历史无法判定服务器归属，迁移时直接清除（用户决定）。

import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { browserDataStorage } from "../utils/storage";

export interface HistoryEntry {
  url: string;
  title: string;
  visitedAt: number;
}

/** 容量上限：每服务器持久化条目的体积护栏（超出淘汰最旧） */
const HISTORY_CAP = 500;

/** 空列表常量：selector 缺省时复用同一引用，避免 useSyncExternalStore 快照抖动 */
const EMPTY_HISTORY: HistoryEntry[] = [];

interface HistoryState {
  /** serverId → 历史列表（最近访问在前） */
  entriesByServer: Record<string, HistoryEntry[]>;
  record: (serverId: string | null, url: string, title: string) => void;
  remove: (serverId: string | null, url: string) => void;
  clear: (serverId: string | null) => void;
  /** 删除服务器时清理其历史（服务器生命周期统一清理入口调用） */
  clearServer: (serverId: string) => void;
}

/** 读取某服务器的历史（null 或无数据时返回稳定的空列表） */
export function selectHistory(
  byServer: Record<string, HistoryEntry[]>,
  serverId: string | null,
): HistoryEntry[] {
  if (!serverId) return EMPTY_HISTORY;
  return byServer[serverId] ?? EMPTY_HISTORY;
}

/** v0 → v1 迁移：旧全局历史没有服务器归属，按用户决定直接清除 */
export function migrateHistoryState(persisted: unknown, version: number): unknown {
  const state = (persisted ?? {}) as Record<string, unknown>;
  if (version >= 1) return state;
  return { entriesByServer: {} };
}

export const useHistoryStore = create<HistoryState>()(
  persist(
    (set) => ({
      entriesByServer: {},

      record: (serverId, url, title) =>
        set((s) => {
          // 未绑定服务器 / 内部页 / 空地址不入历史
          if (!serverId || !url || url.startsWith("about:")) return s;
          const list = s.entriesByServer[serverId] ?? [];
          const prev = list.find((e) => e.url === url);
          const rest = list.filter((e) => e.url !== url);
          return {
            entriesByServer: {
              ...s.entriesByServer,
              [serverId]: [
                { url, title: title || prev?.title || url, visitedAt: Date.now() },
                ...rest,
              ].slice(0, HISTORY_CAP),
            },
          };
        }),

      remove: (serverId, url) =>
        set((s) => {
          if (!serverId) return s;
          const list = s.entriesByServer[serverId];
          if (!list) return s;
          return {
            entriesByServer: {
              ...s.entriesByServer,
              [serverId]: list.filter((e) => e.url !== url),
            },
          };
        }),

      clear: (serverId) =>
        set((s) => {
          if (!serverId || !(serverId in s.entriesByServer)) return s;
          const next = { ...s.entriesByServer };
          delete next[serverId];
          return { entriesByServer: next };
        }),

      clearServer: (serverId) =>
        set((s) => {
          if (!(serverId in s.entriesByServer)) return s;
          const next = { ...s.entriesByServer };
          delete next[serverId];
          return { entriesByServer: next };
        }),
    }),
    {
      name: "history",
      // v1：列表从全局扁平改为按服务器隔离（磁盘结构破坏性变更）
      version: 1,
      migrate: (persisted, version) => migrateHistoryState(persisted, version) as never,
      storage: createJSONStorage(() => browserDataStorage),
      partialize: (s) => ({ entriesByServer: s.entriesByServer }),
    },
  ),
);
