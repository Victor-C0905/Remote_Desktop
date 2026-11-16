// src/shell/notificationFilter.test.ts
// 通知中心筛选纯函数：severity/unread/query 叠加与清空作用域（TDD 先行）。

import { describe, it, expect } from "vitest";
import {
  matchesSeverity,
  matchesQuery,
  applyNotificationFilters,
  buildClearFilter,
} from "./notificationFilter";
import type { AppNotification, NotificationUrgency } from "../stores/notificationStore";

const n = (partial: Partial<AppNotification> = {}): AppNotification => ({
  id: "n1",
  title: "连接中断",
  body: "网络断开",
  timestamp: 0,
  urgency: "normal",
  source: "prod-1",
  read: false,
  ...partial,
});

describe("matchesSeverity", () => {
  it("all 恒真；other 仅收纳未知紧急度；具体值精确匹配", () => {
    expect(matchesSeverity(n(), "all")).toBe(true);
    expect(matchesSeverity(n({ urgency: "critical" }), "other")).toBe(false);
    expect(matchesSeverity(n({ urgency: "weird" as unknown as NotificationUrgency }), "other")).toBe(true);
    expect(matchesSeverity(n({ urgency: "critical" }), "critical")).toBe(true);
    expect(matchesSeverity(n({ urgency: "critical" }), "normal")).toBe(false);
  });
});

describe("matchesQuery", () => {
  it("title/body/source 子串匹配，不区分大小写；空/空白串恒真", () => {
    expect(matchesQuery(n(), "", "prod-1")).toBe(true);
    expect(matchesQuery(n(), "   ", "prod-1")).toBe(true);
    expect(matchesQuery(n(), "中断", "prod-1")).toBe(true);
    expect(matchesQuery(n(), "PROD", "prod-1")).toBe(true);
    expect(matchesQuery(n(), "不存在", "prod-1")).toBe(false);
  });
});

describe("applyNotificationFilters", () => {
  const pool = [
    n({ id: "1", urgency: "critical", read: false, title: "A 失败" }),
    n({ id: "2", urgency: "normal", read: true, title: "B 成功" }),
    n({ id: "3", urgency: "normal", read: false, title: "C 失败" }),
  ];
  const originOf = (x: AppNotification) => x.source;

  it("severity ∧ query AND 叠加（未读状态不参与过滤：打开面板即已读）", () => {
    expect(applyNotificationFilters(pool, { filter: "all", query: "", originOf }).map((x) => x.id))
      .toEqual(["1", "2", "3"]);
    expect(applyNotificationFilters(pool, { filter: "normal", query: "", originOf }).map((x) => x.id))
      .toEqual(["2", "3"]);
    expect(applyNotificationFilters(pool, { filter: "all", query: "失败", originOf }).map((x) => x.id))
      .toEqual(["1", "3"]);
    expect(applyNotificationFilters(pool, { filter: "critical", query: "成功", originOf }))
      .toEqual([]);
  });
});

describe("buildClearFilter（清空作用域跟随筛选视图，搜索不参与）", () => {
  const pool = [
    n({ id: "1", urgency: "critical", read: false }),
    n({ id: "2", urgency: "normal", read: true }),
    n({ id: "3", urgency: "normal", read: false }),
  ];

  it("all → 全部命中；具体 chip → 仅该严重度（已读状态不影响清空范围）", () => {
    const f1 = buildClearFilter({ filter: "all" });
    expect(pool.filter(f1).map((x) => x.id)).toEqual(["1", "2", "3"]);
    const f2 = buildClearFilter({ filter: "critical" });
    expect(pool.filter(f2).map((x) => x.id)).toEqual(["1"]);
  });
});
