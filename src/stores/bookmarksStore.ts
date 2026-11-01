// src/stores/bookmarksStore.ts
// 浏览器书签：扁平列表（v1 无文件夹），持久化于 browser-data.bin。
// barVisible（书签栏显隐）是前端层偏好，与书签同库存储避免动 Rust 设置结构。

import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { browserDataStorage } from "../utils/storage";

export interface Bookmark {
  id: string;
  url: string;
  title: string;
  addedAt: number;
}

interface BookmarksState {
  bookmarks: Bookmark[];  // 追加序（书签栏按此顺序展示）
  barVisible: boolean;
  addBookmark: (url: string, title: string) => void;
  removeBookmark: (id: string) => void;
  setBarVisible: (v: boolean) => void;
}

let seq = 0;

export const useBookmarksStore = create<BookmarksState>()(
  persist(
    (set) => ({
      bookmarks: [],
      barVisible: true,
      addBookmark: (url, title) =>
        set((s) => {
          // 同 URL 去重（星标按钮的 toggle 由调用方先查后调）
          if (s.bookmarks.some((b) => b.url === url) || !url) return s;
          return {
            bookmarks: [...s.bookmarks, { id: `bm-${Date.now()}-${seq++}`, url, title: title || url, addedAt: Date.now() }],
          };
        }),
      removeBookmark: (id) => set((s) => ({ bookmarks: s.bookmarks.filter((b) => b.id !== id) })),
      setBarVisible: (v) => set({ barVisible: v }),
    }),
    {
      name: "bookmarks",
      storage: createJSONStorage(() => browserDataStorage),
    },
  ),
);

/** URL 是否已收藏（星标按钮的状态判定） */
export const isBookmarked = (bookmarks: Bookmark[], url: string | undefined): boolean =>
  !!url && bookmarks.some((b) => b.url === url);
