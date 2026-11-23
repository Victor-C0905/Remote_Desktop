// src/apps/__tests__/fileSidebar.test.ts
// 侧边栏书签分组回归：分组顺序、removable 标记、离线兜底，
// 以及「书签优先」——已书签路径不再进入自动统计的「常用」组。
import { describe, it, expect } from "vitest";
import { buildSidebarSections } from "../fileSidebar";

const base = {
  username: "root",
  homeDir: "/root",
  activeServerId: "s1",
  commonLocations: [],
};

describe("buildSidebarSections 书签分组", () => {
  it("书签分组排在最前，项带 removable 标记与完整路径悬浮提示", () => {
    const sections = buildSidebarSections({
      ...base,
      folderBookmarks: [
        { path: "/var/log", addedAt: 1 },
        { path: "/etc/nginx", addedAt: 2 },
      ],
    });

    expect(sections[0].titleKey).toBe("fileManager.bookmarks");
    expect(sections[0].items).toEqual([
      { icon: "folder", label: "log", title: "/var/log", path: "/var/log", removable: true },
      { icon: "folder", label: "nginx", title: "/etc/nginx", path: "/etc/nginx", removable: true },
    ]);
    // 位置组仍在其后
    expect(sections[1].titleKey).toBe("fileManager.location");
  });

  it("无书签时不渲染书签分组", () => {
    const sections = buildSidebarSections(base);
    expect(sections.map((s) => s.titleKey)).not.toContain("fileManager.bookmarks");
  });

  it("离线模式不渲染书签（无服务器归属）", () => {
    const sections = buildSidebarSections({
      username: null,
      homeDir: null,
      activeServerId: null,
      folderBookmarks: [{ path: "/var/log", addedAt: 1 }],
    });
    expect(sections.map((s) => s.titleKey)).not.toContain("fileManager.bookmarks");
  });

  it("已书签路径从「常用」组过滤，其他常用路径保留（书签优先）", () => {
    const sections = buildSidebarSections({
      ...base,
      folderBookmarks: [{ path: "/var/log", addedAt: 1 }],
      frequentPaths: [
        { path: "/var/log", score: 12, count: 3, lastUsed: 2 },
        { path: "/home/root/projects", score: 9, count: 2, lastUsed: 1 },
      ],
    });

    expect(sections.map((s) => s.titleKey)).toEqual([
      "fileManager.bookmarks",
      "fileManager.frequent",
      "fileManager.location",
    ]);
    expect(sections[0].items.map((i) => i.path)).toEqual(["/var/log"]);
    expect(sections[1].items.map((i) => i.path)).toEqual(["/home/root/projects"]);
  });

  it("常用组仅剩已书签路径时整组隐藏（不产生空组）", () => {
    const sections = buildSidebarSections({
      ...base,
      folderBookmarks: [{ path: "/var/log", addedAt: 1 }],
      frequentPaths: [{ path: "/var/log", score: 12, count: 3, lastUsed: 1 }],
    });

    expect(sections.map((s) => s.titleKey)).toEqual([
      "fileManager.bookmarks",
      "fileManager.location",
    ]);
  });
});
