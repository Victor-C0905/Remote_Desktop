// src/stores/bookmarksStore.ts
// 浏览器书签：按服务器隔离（bookmarksByServer），持久化于 browser-data.bin。
// - v1：旧版本全局书签无法判定服务器归属，迁移时直接清除（用户决定）；
// - barVisible（书签栏显隐）是前端层全局偏好，与书签同库存储。

import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { browserDataStorage } from "../utils/storage";

export interface Bookmark {
  id: string;
  url: string;
  title: string;
  addedAt: number;
}

/** 空列表常量：selector 缺省时复用同一引用，避免 useSyncExternalStore 快照抖动 */
const EMPTY_BOOKMARKS: Bookmark[] = [];

interface BookmarksState {
  /** serverId → 书签列表（追加序，书签栏按此顺序展示） */
  bookmarksByServer: Record<string, Bookmark[]>;
  barVisible: boolean;
  addBookmark: (serverId: string | null, url: string, title: string) => void;
  removeBookmark: (serverId: string | null, id: string) => void;
  setBarVisible: (v: boolean) => void;
  /** 删除服务器时清理其书签（服务器生命周期统一清理入口调用） */
  clearServer: (serverId: string) => void;
}

let seq = 0;

/** 读取某服务器的书签（null 或无数据时返回稳定的空列表） */
export function selectBookmarks(
  byServer: Record<string, Bookmark[]>,
  serverId: string | null,
): Bookmark[] {
  if (!serverId) return EMPTY_BOOKMARKS;
  return byServer[serverId] ?? EMPTY_BOOKMARKS;
}

/**
 * v0 → v1 迁移：旧全局书签没有服务器归属，按用户决定直接清除
 * （保留书签栏显隐偏好）。
 */
export function migrateBookmarksState(persisted: unknown, version: number): unknown {
  const state = (persisted ?? {}) as Record<string, unknown>;
  if (version >= 1) return state;
  return {
    bookmarksByServer: {},
    barVisible: typeof state.barVisible === "boolean" ? state.barVisible : true,
  };
}

export const useBookmarksStore = create<BookmarksState>()(
  persist(
    (set) => ({
      bookmarksByServer: {},
      barVisible: true,

      addBookmark: (serverId, url, title) =>
        set((s) => {
          if (!serverId || !url) return s;
          const list = s.bookmarksByServer[serverId] ?? [];
          // 同 URL 去重（星标按钮的 toggle 由调用方先查后调）
          if (list.some((b) => b.url === url)) return s;
          return {
            bookmarksByServer: {
              ...s.bookmarksByServer,
              [serverId]: [
                ...list,
                { id: `bm-${Date.now()}-${seq++}`, url, title: title || url, addedAt: Date.now() },
              ],
            },
          };
        }),

      removeBookmark: (serverId, id) =>
        set((s) => {
          if (!serverId) return s;
          const list = s.bookmarksByServer[serverId];
          if (!list) return s;
          return {
            bookmarksByServer: {
              ...s.bookmarksByServer,
              [serverId]: list.filter((b) => b.id !== id),
            },
          };
        }),

      setBarVisible: (v) => set({ barVisible: v }),

      clearServer: (serverId) =>
        set((s) => {
          if (!(serverId in s.bookmarksByServer)) return s;
          const next = { ...s.bookmarksByServer };
          delete next[serverId];
          return { bookmarksByServer: next };
        }),
    }),
    {
      name: "bookmarks",
      // v1：列表从全局扁平改为按服务器隔离（磁盘结构破坏性变更）
      version: 1,
      migrate: (persisted, version) => migrateBookmarksState(persisted, version) as never,
      storage: createJSONStorage(() => browserDataStorage),
      partialize: (s) => ({
        bookmarksByServer: s.bookmarksByServer,
        barVisible: s.barVisible,
      }),
    },
  ),
);

/** URL 是否已收藏（星标按钮的状态判定） */
export const isBookmarked = (bookmarks: Bookmark[], url: string | undefined): boolean =>
  !!url && bookmarks.some((b) => b.url === url);
