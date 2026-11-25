// src/apps/__tests__/fileSidebar.test.ts
// 侧边栏书签分组回归：分组顺序、removable 标记、离线兜底，
// 以及「书签优先」——已书签路径不再进入自动统计的「常用」组。
import { describe, it, expect } from "vitest";
import { buildSidebarSections, buildSidebarChildren, flattenSidebarSections } from "../fileSidebar";
import type { SidebarSection, SidebarChildrenStates } from "../fileSidebar";

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

const treeSections: SidebarSection[] = [
  {
    titleKey: "fileManager.bookmarks",
    items: [{ icon: "folder", label: "var", title: "/var", path: "/var" }],
  },
];

const loaded = (items: { label: string; path: string }[], hidden = 0) => ({
  status: "loaded" as const,
  items: items.map((it) => ({
    icon: "folder" as const,
    label: it.label,
    title: it.path,
    path: it.path,
  })),
  hidden,
});

/** 行摘要：item 输出「depth:path」，状态行输出 state（便于断言顺序与层级） */
const rowSummary = (rows: ReturnType<typeof flattenSidebarSections>[number]["rows"]) =>
  rows.map((row) => (row.kind === "item" ? `${row.depth}:${row.item.path}` : row.state));

describe("buildSidebarChildren", () => {
  it("只保留目录、按名称排序，并拼出完整子路径", () => {
    const children = buildSidebarChildren("/var", [
      { name: "log", is_dir: true },
      { name: "a.txt", is_dir: false },
      { name: "cache", is_dir: true },
    ]);

    expect(children.hidden).toBe(0);
    expect(children.items).toEqual([
      { icon: "folder", label: "cache", title: "/var/cache", path: "/var/cache" },
      { icon: "folder", label: "log", title: "/var/log", path: "/var/log" },
    ]);
  });

  it("根目录与带尾斜杠的父路径都不产生双斜杠", () => {
    const root = buildSidebarChildren("/", [{ name: "etc", is_dir: true }]);
    const nested = buildSidebarChildren("/home/root/", [{ name: "docs", is_dir: true }]);

    expect(root.items[0].path).toBe("/etc");
    expect(nested.items[0].path).toBe("/home/root/docs");
  });

  it("超过上限截断并记录被裁数量", () => {
    const entries = ["a", "b", "c", "d", "e"].map((name) => ({ name, is_dir: true }));
    const children = buildSidebarChildren("/x", entries, 2);

    expect(children.items.map((i) => i.label)).toEqual(["a", "b"]);
    expect(children.hidden).toBe(3);
  });
});

describe("flattenSidebarSections 目录树", () => {
  it("未知目录先保留空槽（取到数据后再显示箭头），不插入子行", () => {
    const [section] = flattenSidebarSections(treeSections, {
      expandedPaths: new Set<string>(),
      children: {},
    });

    expect(section.rows).toEqual([
      {
        kind: "item",
        item: treeSections[0].items[0],
        depth: 0,
        expandable: false,
        expanded: false,
      },
    ]);
    expect(section.titleKey).toBe("fileManager.bookmarks");
  });

  it("已加载且有子目录才显示箭头", () => {
    const [section] = flattenSidebarSections(treeSections, {
      expandedPaths: new Set<string>(),
      children: { "/var": loaded([{ label: "log", path: "/var/log" }]) },
    });

    expect(section.rows[0]).toMatchObject({
      kind: "item",
      expandable: true,
      expanded: false,
    });
  });

  it("展开且已加载：子目录行紧随其后，缩进 +1", () => {
    const children: SidebarChildrenStates = {
      "/var": loaded([{ label: "log", path: "/var/log" }]),
    };
    const [section] = flattenSidebarSections(treeSections, {
      expandedPaths: new Set(["/var"]),
      children,
    });

    expect(rowSummary(section.rows)).toEqual(["0:/var", "1:/var/log"]);
  });

  it("展开但未加载/加载中：显示 loading 状态行（与子目录同层缩进）", () => {
    const [pending] = flattenSidebarSections(treeSections, {
      expandedPaths: new Set(["/var"]),
      children: {},
    });
    const [loading] = flattenSidebarSections(treeSections, {
      expandedPaths: new Set(["/var"]),
      children: { "/var": { status: "loading" } },
    });

    expect(rowSummary(pending.rows)).toEqual(["0:/var", "loading"]);
    expect(rowSummary(loading.rows)).toEqual(["0:/var", "loading"]);
    expect(pending.rows[1]).toMatchObject({ depth: 1, path: "/var" });
  });

  it("加载失败显示 error（带信息），已加载空目录隐藏箭头，超限在子项后展示 more", () => {
    const [failed] = flattenSidebarSections(treeSections, {
      expandedPaths: new Set(["/var"]),
      children: { "/var": { status: "error", message: "permission denied" } },
    });
    expect(failed.rows[1]).toMatchObject({ state: "error", message: "permission denied" });

    // 已知空目录：即使处于展开集合也不展开、不显示箭头
    const [empty] = flattenSidebarSections(treeSections, {
      expandedPaths: new Set(["/var"]),
      children: { "/var": loaded([], 0) },
    });
    expect(empty.rows).toEqual([
      {
        kind: "item",
        item: treeSections[0].items[0],
        depth: 0,
        expandable: false,
        expanded: false,
      },
    ]);

    const [more] = flattenSidebarSections(treeSections, {
      expandedPaths: new Set(["/var"]),
      children: { "/var": loaded([{ label: "log", path: "/var/log" }], 42) },
    });
    expect(rowSummary(more.rows)).toEqual(["0:/var", "1:/var/log", "more"]);
    expect(more.rows[2]).toMatchObject({ count: 42, depth: 1 });
  });

  it("递归展开第二层：子目录行也带箭头并可插入孙目录", () => {
    const children: SidebarChildrenStates = {
      "/var": loaded([{ label: "log", path: "/var/log" }]),
      "/var/log": loaded([{ label: "nginx", path: "/var/log/nginx" }]),
    };
    const [section] = flattenSidebarSections(treeSections, {
      expandedPaths: new Set(["/var", "/var/log"]),
      children,
    });

    expect(rowSummary(section.rows)).toEqual([
      "0:/var",
      "1:/var/log",
      "2:/var/log/nginx",
    ]);
    expect(section.rows[1]).toMatchObject({ kind: "item", expandable: true, expanded: true });
  });

  it("allowExpand=false（离线占位项）不输出箭头与子行", () => {
    const [section] = flattenSidebarSections(treeSections, {
      expandedPaths: new Set(["/var"]),
      children: { "/var": loaded([{ label: "log", path: "/var/log" }]) },
      allowExpand: false,
    });

    expect(section.rows).toHaveLength(1);
    expect(section.rows[0]).toMatchObject({ kind: "item", expandable: false, expanded: false });
  });
});

