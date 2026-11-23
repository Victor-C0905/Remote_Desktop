import { describe, it, expect, beforeEach } from "vitest";
import {
  selectFolderBookmarks,
  isFolderBookmarked,
  useFolderBookmarksStore,
} from "./folderBookmarksStore";

const state = () => useFolderBookmarksStore.getState();
const pathsOf = (serverId: string | null) =>
  selectFolderBookmarks(state().folderBookmarksByServer, serverId).map((b) => b.path);

beforeEach(() => {
  useFolderBookmarksStore.setState({ folderBookmarksByServer: {} });
});

describe("folderBookmarksStore 按服务器隔离", () => {
  it("新增/删除只影响对应服务器", () => {
    state().addFolderBookmark("s1", "/var/log");
    state().addFolderBookmark("s2", "/etc");

    expect(pathsOf("s1")).toEqual(["/var/log"]);
    expect(pathsOf("s2")).toEqual(["/etc"]);

    state().removeFolderBookmark("s1", "/var/log");
    expect(pathsOf("s1")).toEqual([]);
    expect(pathsOf("s2")).toEqual(["/etc"]);
  });

  it("serverId 为 null 时不写入，选择器返回稳定的空列表", () => {
    state().addFolderBookmark(null, "/var/log");

    expect(state().folderBookmarksByServer).toEqual({});
    // 缺省时复用同一空数组引用，避免 useSyncExternalStore 快照抖动
    expect(selectFolderBookmarks({}, null)).toBe(selectFolderBookmarks({}, "s1"));
  });

  it("同服务器同路径去重", () => {
    state().addFolderBookmark("s1", "/var/log");
    state().addFolderBookmark("s1", "/var/log");

    const list = selectFolderBookmarks(state().folderBookmarksByServer, "s1");
    expect(list).toHaveLength(1);
    expect(list[0].addedAt).toBeGreaterThan(0);
  });

  it("移除不存在的路径不产生状态变更", () => {
    state().addFolderBookmark("s1", "/var/log");
    const before = state().folderBookmarksByServer;

    state().removeFolderBookmark("s1", "/not/exist");

    expect(state().folderBookmarksByServer).toBe(before);
  });

  it("clearServer 只清理指定服务器", () => {
    state().addFolderBookmark("s1", "/var/log");
    state().addFolderBookmark("s2", "/etc");

    state().clearServer("s1");

    expect(pathsOf("s1")).toEqual([]);
    expect(pathsOf("s2")).toEqual(["/etc"]);
    expect("s1" in state().folderBookmarksByServer).toBe(false);
  });

  it("isFolderBookmarked 判定星标 toggle 状态", () => {
    state().addFolderBookmark("s1", "/var/log");
    const list = selectFolderBookmarks(state().folderBookmarksByServer, "s1");

    expect(isFolderBookmarked(list, "/var/log")).toBe(true);
    expect(isFolderBookmarked(list, "/etc")).toBe(false);
    expect(isFolderBookmarked(list, undefined)).toBe(false);
  });
});
