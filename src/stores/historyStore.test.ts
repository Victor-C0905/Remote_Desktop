import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  migrateHistoryState,
  selectHistory,
  useHistoryStore,
  type HistoryEntry,
} from "./historyStore";

const entry = (partial: Partial<HistoryEntry> = {}): HistoryEntry => ({
  url: "https://a.com",
  title: "A",
  visitedAt: 0,
  ...partial,
});

const state = () => useHistoryStore.getState();
const urlsOf = (serverId: string | null) =>
  selectHistory(state().entriesByServer, serverId).map((e) => e.url);

beforeEach(() => {
  useHistoryStore.setState({ entriesByServer: {} });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("historyStore 按服务器隔离", () => {
  it("记录只写入对应服务器，重访置顶并刷新时间", () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(1000);
    state().record("s1", "https://a.com", "A");
    state().record("s1", "https://b.com", "B");
    state().record("s2", "https://c.com", "C");

    expect(urlsOf("s1")).toEqual(["https://b.com", "https://a.com"]);
    expect(urlsOf("s2")).toEqual(["https://c.com"]);

    now.mockReturnValue(2000);
    state().record("s1", "https://a.com", "A2");

    const s1 = selectHistory(state().entriesByServer, "s1");
    expect(s1.map((e) => e.url)).toEqual(["https://a.com", "https://b.com"]);
    expect(s1[0].visitedAt).toBe(2000);
    expect(s1[0].title).toBe("A2");
  });

  it("标题为空时保留上一次标题", () => {
    state().record("s1", "https://a.com", "原标题");
    state().record("s1", "https://a.com", "");

    expect(selectHistory(state().entriesByServer, "s1")[0].title).toBe("原标题");
  });

  it("内部页/空地址/null 服务器不入历史", () => {
    state().record("s1", "about:blank", "");
    state().record("s1", "", "");
    state().record(null, "https://a.com", "A");

    expect(state().entriesByServer).toEqual({});
  });

  it("每服务器独立容量上限：超出淘汰最旧", () => {
    // 最近在前：site-0 最新，site-499 最旧（与 store 内部顺序一致）
    const list = Array.from({ length: 500 }, (_, i) =>
      entry({ url: `https://site-${i}.com`, title: `S${i}`, visitedAt: 1000 - i }),
    );
    useHistoryStore.setState({ entriesByServer: { s1: list } });

    state().record("s1", "https://new.com", "New");

    const s1 = selectHistory(state().entriesByServer, "s1");
    expect(s1).toHaveLength(500);
    expect(s1[0].url).toBe("https://new.com");
    expect(s1.some((e) => e.url === "https://site-499.com")).toBe(false);
  });

  it("remove/clear/clearServer 只影响指定服务器", () => {
    state().record("s1", "https://a.com", "A");
    state().record("s1", "https://b.com", "B");
    state().record("s2", "https://c.com", "C");

    state().remove("s1", "https://a.com");
    expect(urlsOf("s1")).toEqual(["https://b.com"]);

    state().clear("s1");
    expect(urlsOf("s1")).toEqual([]);
    expect(urlsOf("s2")).toEqual(["https://c.com"]);

    state().clearServer("s2");
    expect(urlsOf("s2")).toEqual([]);
  });
});

describe("migrateHistoryState", () => {
  it("v0 旧全局历史在迁移时清除", () => {
    const migrated = migrateHistoryState(
      { entries: [entry({ url: "https://old.com" })] },
      0,
    ) as { entriesByServer: Record<string, HistoryEntry[]> };

    expect(migrated.entriesByServer).toEqual({});
    expect(migrated).not.toHaveProperty("legacyEntries");
  });

  it("v1 已是按服务器结构，原样返回", () => {
    const persisted = { entriesByServer: { s1: [entry()] } };
    expect(migrateHistoryState(persisted, 1)).toBe(persisted);
  });
});
