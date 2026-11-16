import { describe, it, expect, beforeEach } from "vitest";
import {
  migrateBookmarksState,
  selectBookmarks,
  useBookmarksStore,
  type Bookmark,
} from "./bookmarksStore";

const bm = (partial: Partial<Bookmark> = {}): Bookmark => ({
  id: "bm-1",
  url: "https://a.com",
  title: "A",
  addedAt: 0,
  ...partial,
});

const state = () => useBookmarksStore.getState();
const urlsOf = (serverId: string | null) =>
  selectBookmarks(state().bookmarksByServer, serverId).map((b) => b.url);

beforeEach(() => {
  useBookmarksStore.setState({ bookmarksByServer: {}, barVisible: true });
});

describe("bookmarksStore 按服务器隔离", () => {
  it("新增/删除只影响对应服务器", () => {
    const { addBookmark, removeBookmark } = state();
    addBookmark("s1", "https://a.com", "A");
    addBookmark("s2", "https://b.com", "B");

    expect(urlsOf("s1")).toEqual(["https://a.com"]);
    expect(urlsOf("s2")).toEqual(["https://b.com"]);

    const target = selectBookmarks(state().bookmarksByServer, "s1")[0];
    removeBookmark("s1", target.id);

    expect(urlsOf("s1")).toEqual([]);
    expect(urlsOf("s2")).toEqual(["https://b.com"]);
  });

  it("serverId 为 null 时不写入，选择器返回稳定的空列表", () => {
    state().addBookmark(null, "https://a.com", "A");

    expect(state().bookmarksByServer).toEqual({});
    // 缺省时复用同一空数组引用，避免 useSyncExternalStore 快照抖动
    expect(selectBookmarks({}, null)).toBe(selectBookmarks({}, "s1"));
  });

  it("同服务器同 URL 去重", () => {
    state().addBookmark("s1", "https://a.com", "A");
    state().addBookmark("s1", "https://a.com", "A2");

    expect(urlsOf("s1")).toEqual(["https://a.com"]);
  });

  it("clearServer 只清理指定服务器", () => {
    state().addBookmark("s1", "https://a.com", "A");
    state().addBookmark("s2", "https://b.com", "B");

    state().clearServer("s1");

    expect(urlsOf("s1")).toEqual([]);
    expect(urlsOf("s2")).toEqual(["https://b.com"]);
  });
});

describe("migrateBookmarksState", () => {
  it("v0 旧全局书签在迁移时清除，barVisible 保留", () => {
    const migrated = migrateBookmarksState(
      { bookmarks: [bm({ url: "https://old.com" })], barVisible: false },
      0,
    ) as { bookmarksByServer: Record<string, Bookmark[]>; barVisible: boolean };

    expect(migrated.bookmarksByServer).toEqual({});
    expect(migrated).not.toHaveProperty("legacyBookmarks");
    expect(migrated.barVisible).toBe(false);
  });

  it("v1 已是按服务器结构，原样返回", () => {
    const persisted = {
      bookmarksByServer: { s1: [bm({})] },
      barVisible: true,
    };
    expect(migrateBookmarksState(persisted, 1)).toBe(persisted);
  });
});
