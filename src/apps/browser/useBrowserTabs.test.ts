// src/apps/browser/useBrowserTabs.test.ts
import { describe, it, expect } from "vitest";
import { mergePageState, type TabInfo } from "./useBrowserTabs";

const base: TabInfo[] = [
  { id: 1, url: "https://a.example", title: "A", loading: false, sleeping: false },
  { id: 2, url: "https://b.example", title: "B", loading: true, sleeping: false },
];

describe("mergePageState", () => {
  it("合并指定 tab 的增量字段", () => {
    const merged = mergePageState(base, {
      server_id: "s1", tab_id: 2, title: "B Page", loading: false,
    });
    expect(merged[1]).toEqual({ id: 2, url: "https://b.example", title: "B Page", loading: false, sleeping: false });
    expect(merged[0]).toEqual(base[0]);
  });

  it("未提供字段保持原值", () => {
    const merged = mergePageState(base, { server_id: "s1", tab_id: 1, loading: true });
    expect(merged[0].url).toBe("https://a.example");
    expect(merged[0].title).toBe("A");
    expect(merged[0].loading).toBe(true);
  });

  it("不相关 tab 不受影响且返回新数组", () => {
    const merged = mergePageState(base, { server_id: "s1", tab_id: 999, title: "ghost" });
    expect(merged).not.toBe(base);
    expect(merged).toEqual(base);
  });
});
