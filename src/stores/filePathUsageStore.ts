// src/stores/filePathUsageStore.ts
// 文件管理器目录访问统计（frecency）：按「交互类型权重 + 时间衰减」计算常用路径。
// - 只有明确目的地才计分：地址栏/搜索/侧边栏点击 5，双击进入 2；
//   面包屑、上级、前进后退、自动加载/刷新不计分
// - 同一路径 30 分钟内重复访问只刷新最近时间，不重复加分
// - 30 天半衰期；衰减后分数 >= 6 才进入「常用」（单次访问无法过线）
// 持久化于 filemanager.bin，按服务器隔离。

import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { fileManagerStorage } from "../utils/storage";

/** 访问来源：none 表示路过型导航（面包屑/上级/前进后退），不计分 */
export type VisitSource = "address" | "search" | "sidebar" | "doubleclick" | "none";

/** 各类明确目的地的计分权重 */
export const VISIT_WEIGHTS: Record<Exclude<VisitSource, "none">, number> = {
  address: 5,
  search: 5,
  sidebar: 5,
  doubleclick: 2,
};

/** 排序衰减半衰期：30 天 */
export const HALF_LIFE_MS = 30 * 24 * 60 * 60 * 1000;
/** 同一路径的去抖窗口：窗口内重复访问不重复计分 */
export const SESSION_GAP_MS = 30 * 60 * 1000;
/**
 * 进入「常用」的最低衰减分数。
 * 单次最高权重为 5，门槛设在 5.5：保证首次访问（包括一次地址栏/侧边栏直达）
 * 不会出现；两次明确访问或三次双击进入才会过线。
 */
export const MIN_FREQUENT_SCORE = 5.5;
/** 「常用」最多展示条数 */
export const FREQUENT_PATHS_LIMIT = 6;
/** 单服务器最多保留的路径数，防止长期使用无限膨胀 */
export const MAX_TRACKED_PATHS_PER_SERVER = 100;

export interface PathUsageEntry {
  /** 最近一次计分后的分数（截至 scoreAt） */
  score: number;
  /** 分数基准时间 */
  scoreAt: number;
  /** 最近一次访问时间（含去抖窗口内未计分的访问） */
  lastUsed: number;
  /** 计分次数（排查/展示用，不参与排序门槛） */
  count: number;
}

export interface PathUsageStat {
  path: string;
  /** 读取时按当前时间衰减后的分数 */
  score: number;
  count: number;
  lastUsed: number;
}

interface FilePathUsageState {
  /** serverId → 目录路径 → frecency 记录（按服务器隔离，禁止跨服务器串数据） */
  byServer: Record<string, Record<string, PathUsageEntry>>;
  /** 记录一次访问；source 为 none 时直接忽略 */
  recordVisit: (serverId: string | null, path: string, source: VisitSource) => void;
  /** 目录确认不存在时从常用中剔除（导航失败调用，避免僵尸条目常驻） */
  removePath: (serverId: string | null, path: string) => void;
  /** 删除服务器时清理其访问统计（serversStore.removeServer 调用） */
  clearServer: (serverId: string) => void;
}

/** 归一化目录路径：折叠重复斜杠、去掉末尾斜杠（根目录除外），非绝对路径拒绝 */
export function normalizeUsagePath(path: string): string | null {
  if (typeof path !== "string") return null;
  const trimmed = path.trim();
  if (!trimmed.startsWith("/")) return null;
  const collapsed = trimmed.replace(/\/+/g, "/");
  return collapsed.length > 1 && collapsed.endsWith("/") ? collapsed.slice(0, -1) : collapsed;
}

/** 读取时的衰减分数：score 以 scoreAt 为基准，按半衰期衰减到现在 */
export function effectiveScore(entry: Pick<PathUsageEntry, "score" | "scoreAt">, now = Date.now()): number {
  return entry.score * Math.pow(0.5, Math.max(0, now - entry.scoreAt) / HALF_LIFE_MS);
}

/**
 * 选出「常用」路径：按当前衰减分数降序，仅保留过线项，读取时不做任何 IO。
 * 排序同分时按最近访问时间降序。
 */
export function selectFrequentPaths(
  byServer: Record<string, Record<string, PathUsageEntry>>,
  serverId: string | null,
  limit = FREQUENT_PATHS_LIMIT,
  now = Date.now(),
): PathUsageStat[] {
  if (!serverId) return [];
  const entries = byServer[serverId];
  if (!entries) return [];
  return Object.entries(entries)
    .map(([path, entry]) => ({
      path,
      score: effectiveScore(entry, now),
      count: entry.count,
      lastUsed: entry.lastUsed,
    }))
    .filter((stat) => stat.score >= MIN_FREQUENT_SCORE)
    .sort((a, b) => b.score - a.score || b.lastUsed - a.lastUsed)
    .slice(0, limit);
}

/** v0 → v1 迁移：旧记录只有 count/lastUsed，按 count 折算初始分数 */
export function migratePersistedUsage(
  persisted: unknown,
  version: number,
): { byServer: Record<string, Record<string, PathUsageEntry>> } {
  const root = (persisted && typeof persisted === "object" ? persisted : {}) as { byServer?: unknown };
  const rawServers = (root.byServer && typeof root.byServer === "object" ? root.byServer : {}) as Record<
    string,
    Record<string, unknown>
  >;

  const byServer: Record<string, Record<string, PathUsageEntry>> = {};
  for (const [serverId, rawEntries] of Object.entries(rawServers)) {
    if (!rawEntries || typeof rawEntries !== "object") continue;
    const entries: Record<string, PathUsageEntry> = {};
    for (const [path, value] of Object.entries(rawEntries)) {
      if (!value || typeof value !== "object") continue;
      const record = value as Record<string, unknown>;
      const count = Number(record.count) || 0;
      const lastUsed = Number(record.lastUsed) || Date.now();
      if (version >= 1) {
        entries[path] = {
          score: Number(record.score) || 0,
          scoreAt: Number(record.scoreAt) || lastUsed,
          lastUsed,
          count,
        };
      } else {
        entries[path] = { score: count, scoreAt: lastUsed, lastUsed, count };
      }
    }
    byServer[serverId] = entries;
  }
  return { byServer };
}

export const useFilePathUsageStore = create<FilePathUsageState>()(
  persist(
    (set) => ({
      byServer: {},

      recordVisit: (serverId, path, source) => {
        if (source === "none") return;
        const normalized = normalizeUsagePath(path);
        if (!serverId || !normalized) return;

        const now = Date.now();
        const weight = VISIT_WEIGHTS[source];

        set((state) => {
          const current = state.byServer[serverId] ?? {};
          const previous = current[normalized];

          // 去抖窗口内重复访问：只刷新最近时间，不加分、不增加计分次数
          // （以 scoreAt 为窗口起点；持续停留/反复进入不会因为 lastUsed 被刷新而无限顺延）
          if (previous && now - previous.scoreAt < SESSION_GAP_MS) {
            return {
              byServer: {
                ...state.byServer,
                [serverId]: { ...current, [normalized]: { ...previous, lastUsed: now } },
              },
            };
          }

          const next: Record<string, PathUsageEntry> = {
            ...current,
            [normalized]: {
              score: (previous ? effectiveScore(previous, now) : 0) + weight,
              scoreAt: now,
              lastUsed: now,
              count: (previous?.count ?? 0) + 1,
            },
          };

          // 超出上限时淘汰衰减分数最低的路径
          const overflow = Object.keys(next).length - MAX_TRACKED_PATHS_PER_SERVER;
          if (overflow > 0) {
            const victims = Object.entries(next)
              .sort(([, a], [, b]) => effectiveScore(a, now) - effectiveScore(b, now))
              .slice(0, overflow);
            for (const [victim] of victims) delete next[victim];
          }

          return { byServer: { ...state.byServer, [serverId]: next } };
        });
      },

      clearServer: (serverId) => {
        set((state) => {
          if (!(serverId in state.byServer)) return state;
          const next = { ...state.byServer };
          delete next[serverId];
          return { byServer: next };
        });
      },
      removePath: (serverId, path) => {
        const normalized = normalizeUsagePath(path);
        if (!serverId || !normalized) return;
        set((state) => {
          const current = state.byServer[serverId];
          if (!current || !current[normalized]) return state;
          const next = { ...current };
          delete next[normalized];
          return { byServer: { ...state.byServer, [serverId]: next } };
        });
      },
    }),
    {
      name: "filemanager-path-usage",
      // v1：frecency 模型（score/scoreAt/count），旧 count/lastUsed 记录由 migrate 折算
      version: 1,
      migrate: (persisted, version) => migratePersistedUsage(persisted, version) as never,
      storage: createJSONStorage(() => fileManagerStorage),
      partialize: (state) => ({ byServer: state.byServer }),
    },
  ),
);
