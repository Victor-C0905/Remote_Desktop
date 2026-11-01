// src/stores/historyStore.ts
// 浏览器历史：最近在前、URL 去重（重访置顶并刷新时间）、容量上限淘汰。
// 持久化于 browser-data.bin；无痕会话由调用方（BrowserApp）豁免记录。

import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { browserDataStorage } from "../utils/storage";

export interface HistoryEntry {
  url: string;
  title: string;
  visitedAt: number;
}

/** 容量上限：持久化文件的体积护栏（超出淘汰最旧） */
const HISTORY_CAP = 500;

interface HistoryState {
  entries: HistoryEntry[];  // 最近访问在前
  record: (url: string, title: string) => void;
  remove: (url: string) => void;
  clear: () => void;
}

export const useHistoryStore = create<HistoryState>()(
  persist(
    (set) => ({
      entries: [],
      record: (url, title) =>
        set((s) => {
          // 内部页与空地址不入历史
          if (!url || url.startsWith("about:")) return s;
          const prev = s.entries.find((e) => e.url === url);
          const rest = s.entries.filter((e) => e.url !== url);
          return {
            entries: [{ url, title: title || prev?.title || url, visitedAt: Date.now() }, ...rest].slice(0, HISTORY_CAP),
          };
        }),
      remove: (url) => set((s) => ({ entries: s.entries.filter((e) => e.url !== url) })),
      clear: () => set({ entries: [] }),
    }),
    {
      name: "history",
      storage: createJSONStorage(() => browserDataStorage),
    },
  ),
);
