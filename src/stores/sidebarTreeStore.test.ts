import { describe, it, expect, beforeEach } from "vitest";
import {
  selectExpandedPaths,
  isPathExpanded,
  useSidebarTreeStore,
} from "./sidebarTreeStore";

const state = () => useSidebarTreeStore.getState();
const pathsOf = (serverId: string | null) =>
  selectExpandedPaths(state().expandedByServer, serverId);

beforeEach(() => {
  useSidebarTreeStore.setState({ expandedByServer: {} });
});

describe("sidebarTreeStore 按服务器隔离", () => {
  it("toggle 展开/收起，且只影响对应服务器", () => {
    state().toggleExpanded("s1", "/var");
    state().toggleExpanded("s2", "/etc");

    expect(pathsOf("s1")).toEqual(["/var"]);
    expect(pathsOf("s2")).toEqual(["/etc"]);

    state().toggleExpanded("s1", "/var");
    expect(pathsOf("s1")).toEqual([]);
    // 收起最后一个目录时移除服务器键
    expect("s1" in state().expandedByServer).toBe(false);
    expect(pathsOf("s2")).toEqual(["/etc"]);
  });

  it("serverId 为 null 时不写入，选择器返回稳定的空列表", () => {
    state().toggleExpanded(null, "/var");
    state().toggleExpanded("s1", "");

    expect(state().expandedByServer).toEqual({});
    // 缺省时复用同一空数组引用，避免 useSyncExternalStore 快照抖动
    expect(selectExpandedPaths({}, null)).toBe(selectExpandedPaths({}, "s1"));
  });

  it("多级目录按展开顺序追加，重复 toggle 可单独收起", () => {
    state().toggleExpanded("s1", "/var");
    state().toggleExpanded("s1", "/var/log");
    state().toggleExpanded("s1", "/etc");

    expect(pathsOf("s1")).toEqual(["/var", "/var/log", "/etc"]);

    state().toggleExpanded("s1", "/var/log");
    expect(pathsOf("s1")).toEqual(["/var", "/etc"]);
  });

  it("clearServer 只清理指定服务器", () => {
    state().toggleExpanded("s1", "/var");
    state().toggleExpanded("s2", "/etc");

    state().clearServer("s1");

    expect(pathsOf("s1")).toEqual([]);
    expect(pathsOf("s2")).toEqual(["/etc"]);
    expect("s1" in state().expandedByServer).toBe(false);
  });

  it("collapseExpanded 幂等清除已展开目录", () => {
    state().toggleExpanded("s1", "/var");
    state().toggleExpanded("s1", "/etc");

    state().collapseExpanded("s1", "/var");
    expect(pathsOf("s1")).toEqual(["/etc"]);

    const before = state().expandedByServer;
    state().collapseExpanded("s1", "/var");
    expect(state().expandedByServer).toBe(before);

    state().collapseExpanded("s1", "/etc");
    expect("s1" in state().expandedByServer).toBe(false);
  });

  it("isPathExpanded 判定展开态", () => {
    state().toggleExpanded("s1", "/var");

    expect(isPathExpanded(pathsOf("s1"), "/var")).toBe(true);
    expect(isPathExpanded(pathsOf("s1"), "/etc")).toBe(false);
    expect(isPathExpanded(pathsOf("s1"), undefined)).toBe(false);
  });
});
