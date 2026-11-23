// src/stores/folderBookmarksStore.ts
// 文件管理器「书签」：手动添加的常用文件夹，按服务器隔离，持久化于
// fileManagerStorage（与常用路径统计同存储域）。同路径去重（星标
// toggle 由调用方先查 isFolderBookmarked 后调）。

import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { fileManagerStorage } from "../utils/storage";

export interface FolderBookmark {
  path: string;
  addedAt: number;
}

/** 空列表常量：selector 缺省时复用同一引用，避免 useSyncExternalStore 快照抖动 */
const EMPTY_BOOKMARKS: FolderBookmark[] = [];

interface FolderBookmarksState {
  /** serverId → 书签列表（追加序，侧栏按此顺序展示） */
  folderBookmarksByServer: Record<string, FolderBookmark[]>;
  addFolderBookmark: (serverId: string | null, path: string) => void;
  removeFolderBookmark: (serverId: string | null, path: string) => void;
  /** 删除服务器时清理其书签（utils/serverCleanup 统一清理入口调用） */
  clearServer: (serverId: string) => void;
}

/** 读取某服务器的书签（null 或无数据时返回稳定的空列表） */
export function selectFolderBookmarks(
  byServer: Record<string, FolderBookmark[]>,
  serverId: string | null,
): FolderBookmark[] {
  if (!serverId) return EMPTY_BOOKMARKS;
  return byServer[serverId] ?? EMPTY_BOOKMARKS;
}

/** 某路径是否已收藏（星标按钮的 toggle 判定） */
export const isFolderBookmarked = (
  bookmarks: FolderBookmark[],
  path: string | undefined | null,
): boolean => !!path && bookmarks.some((b) => b.path === path);

export const useFolderBookmarksStore = create<FolderBookmarksState>()(
  persist(
    (set) => ({
      folderBookmarksByServer: {},

      addFolderBookmark: (serverId, path) =>
        set((s) => {
          if (!serverId || !path) return s;
          const list = s.folderBookmarksByServer[serverId] ?? [];
          if (list.some((b) => b.path === path)) return s;
          return {
            folderBookmarksByServer: {
              ...s.folderBookmarksByServer,
              [serverId]: [...list, { path, addedAt: Date.now() }],
            },
          };
        }),

      removeFolderBookmark: (serverId, path) =>
        set((s) => {
          if (!serverId || !path) return s;
          const list = s.folderBookmarksByServer[serverId];
          if (!list) return s;
          const next = list.filter((b) => b.path !== path);
          if (next.length === list.length) return s;
          return {
            folderBookmarksByServer: { ...s.folderBookmarksByServer, [serverId]: next },
          };
        }),

      clearServer: (serverId) =>
        set((s) => {
          if (!(serverId in s.folderBookmarksByServer)) return s;
          const next = { ...s.folderBookmarksByServer };
          delete next[serverId];
          return { folderBookmarksByServer: next };
        }),
    }),
    {
      name: "fm-folder-bookmarks",
      version: 1,
      storage: createJSONStorage(() => fileManagerStorage),
      partialize: (s) => ({ folderBookmarksByServer: s.folderBookmarksByServer }),
    },
  ),
);
