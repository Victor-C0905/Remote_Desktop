// src/apps/browser/useBrowserTabs.ts
// 浏览器标签状态：订阅 Rust 事件（browser://tabs 全量 / browser://page-state 增量）

import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** 单个标签元数据（Rust 端 TabInfo） */
export interface TabInfo {
  id: number;
  url: string;
  title: string;
  loading: boolean;
  sleeping: boolean;
}

/** tab 列表结构变更（Rust 端 TabsPayload，全量） */
interface TabsPayload {
  server_id: string;
  active: number | null;
  tabs: TabInfo[];
}

/** 单 tab 增量状态（Rust 端 TabPageState） */
interface PageStatePayload {
  server_id: string;
  tab_id: number;
  url?: string;
  title?: string;
  loading?: boolean;
}

/**
 * 增量合并 page-state 到列表（纯函数，可测）
 */
export function mergePageState(tabs: TabInfo[], ev: PageStatePayload): TabInfo[] {
  return tabs.map((t) =>
    t.id === ev.tab_id
      ? {
          ...t,
          url: ev.url ?? t.url,
          title: ev.title ?? t.title,
          loading: ev.loading ?? t.loading,
        }
      : t,
  );
}

/**
 * 订阅浏览器标签状态（按 server_id 过滤）
 */
export function useBrowserTabs(serverId: string | null) {
  const [tabs, setTabs] = useState<TabInfo[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);

  useEffect(() => {
    if (!serverId) {
      setTabs([]);
      setActiveId(null);
      return;
    }
    const unlisteners: Promise<UnlistenFn>[] = [
      listen<TabsPayload>("browser://tabs", (e) => {
        if (e.payload.server_id !== serverId) return;
        setTabs(e.payload.tabs);
        setActiveId(e.payload.active);
      }),
      listen<PageStatePayload>("browser://page-state", (e) => {
        if (e.payload.server_id !== serverId) return;
        setTabs((prev) => mergePageState(prev, e.payload));
      }),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((un) => un()).catch(() => {}));
    };
  }, [serverId]);

  return { tabs, activeId };
}
