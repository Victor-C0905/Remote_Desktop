// src/apps/browser/useBrowserTabs.ts
// 浏览器标签状态：订阅 Rust 事件（browser://tabs 全量 / browser://page-state 增量）

import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
  // 关闭页签栈（LIFO，上限 25）：快照差分维护——prev 有而新快照无的 id 即
  // 被关闭，供「重新打开已关闭的标签页」复用（会话内有效）
  const [closedTabs, setClosedTabs] = useState<TabInfo[]>([]);
  const tabsRef = useRef<TabInfo[]>([]);

  useEffect(() => {
    // 换服务器/断开：差分基准与栈清空，避免旧服务器的页签被误判为关闭
    tabsRef.current = [];
    setClosedTabs([]);
    if (!serverId) {
      setTabs([]);
      setActiveId(null);
      return;
    }
    const unlisteners: Promise<UnlistenFn>[] = [
      listen<TabsPayload>("browser://tabs", (e) => {
        if (e.payload.server_id !== serverId) return;
        const prev = tabsRef.current;
        tabsRef.current = e.payload.tabs;
        if (prev.length > 0) {
          const alive = new Set(e.payload.tabs.map((t) => t.id));
          const closed = prev.filter((t) => !alive.has(t.id)).reverse();
          if (closed.length > 0) {
            // 多个同时关闭按从右到左入栈（最右视为最近关闭，对齐 Chrome 复开顺序）
            setClosedTabs((cs) => [...closed, ...cs].slice(0, 25));
          }
        }
        setTabs(e.payload.tabs);
        setActiveId(e.payload.active);
      }),
      listen<PageStatePayload>("browser://page-state", (e) => {
        if (e.payload.server_id !== serverId) return;
        setTabs((prev) => mergePageState(prev, e.payload));
      }),
    ];
    // 订阅就绪后主动拉取一次全量标签列表：Rust 侧活跃会话不因前端重载
    // 而重发 tabs 事件（刷新恢复场景），不拉取则标签条停留空列表
    void Promise.all(unlisteners)
      .then(() => {
        if (serverId) invoke("browser_refresh_tabs", { serverId }).catch(() => {});
      })
      .catch(() => {});
    return () => {
      unlisteners.forEach((p) => p.then((un) => un()).catch(() => {}));
    };
  }, [serverId]);

  return { tabs, activeId, closedTabs, setClosedTabs };
}
