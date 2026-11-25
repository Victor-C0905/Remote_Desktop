// src/stores/sidebarTreeStore.ts
// 文件管理器侧边栏目录树的「展开态」：按服务器隔离，持久化于 fileManagerStorage。
// 只记录已展开的目录路径集合；子目录条目本身由 FileManager 懒加载并缓存在内存
//（不落盘——目录内容随时可能变化，持久化会展示过期数据）。

import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { fileManagerStorage } from "../utils/storage";

/** 空列表常量：selector 缺省时复用同一引用，避免 useSyncExternalStore 快照抖动 */
const EMPTY_PATHS: string[] = [];

interface SidebarTreeState {
  /** serverId → 已展开的目录路径（追加序；渲染按集合判定） */
  expandedByServer: Record<string, string[]>;
  /** 展开/收起目录（点箭头调用；serverId 为空时忽略） */
  toggleExpanded: (serverId: string | null, path: string) => void;
  /** 收起指定目录（已确认无子目录时自动调用，清除无效展开态） */
  collapseExpanded: (serverId: string | null, path: string) => void;
  /** 删除服务器时清理其展开态（utils/serverCleanup 统一清理入口调用） */
  clearServer: (serverId: string) => void;
}

/** 读取某服务器的展开路径（null 或无数据时返回稳定的空列表） */
export function selectExpandedPaths(
  byServer: Record<string, string[]>,
  serverId: string | null,
): string[] {
  if (!serverId) return EMPTY_PATHS;
  return byServer[serverId] ?? EMPTY_PATHS;
}

/** 某目录是否处于展开态 */
export function isPathExpanded(paths: readonly string[], path: string | undefined | null): boolean {
  return !!path && paths.includes(path);
}

export const useSidebarTreeStore = create<SidebarTreeState>()(
  persist(
    (set) => ({
      expandedByServer: {},

      toggleExpanded: (serverId, path) =>
        set((s) => {
          if (!serverId || !path) return s;
          const list = s.expandedByServer[serverId] ?? [];
          const next = list.includes(path)
            ? list.filter((p) => p !== path)
            : [...list, path];
          // 收起最后一个目录时删掉该服务器键，避免持久化文件累积空数组
          const map = { ...s.expandedByServer };
          if (next.length === 0) {
            delete map[serverId];
          } else {
            map[serverId] = next;
          }
          return { expandedByServer: map };
        }),

      clearServer: (serverId) =>
        set((s) => {
          if (!(serverId in s.expandedByServer)) return s;
          const next = { ...s.expandedByServer };
          delete next[serverId];
          return { expandedByServer: next };
        }),

      collapseExpanded: (serverId, path) =>
        set((s) => {
          if (!serverId || !path) return s;
          const list = s.expandedByServer[serverId];
          if (!list || !list.includes(path)) return s;
          const next = list.filter((p) => p !== path);
          const map = { ...s.expandedByServer };
          if (next.length === 0) {
            delete map[serverId];
          } else {
            map[serverId] = next;
          }
          return { expandedByServer: map };
        }),
    }),
    {
      name: "fm-sidebar-tree",
      version: 1,
      storage: createJSONStorage(() => fileManagerStorage),
      partialize: (s) => ({ expandedByServer: s.expandedByServer }),
    },
  ),
);
