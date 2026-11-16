// src/stores/notificationStore.test.ts
// 通知彻底按服务器隔离的核心规则：可见性与旧数据迁移。
// 纯函数测试，不依赖 UI / Tauri runtime。

import { describe, it, expect } from "vitest";
import {
  isNotificationVisible,
  mergeIncoming,
  MERGE_WINDOW_MS,
  migrateNotificationState,
  useNotificationStore,
  type AppNotification,
} from "./notificationStore";

const ACTIVE = "srv-a";

/** 构造一条最小通知 */
function n(partial: Partial<AppNotification> = {}): AppNotification {
  return {
    id: "n1",
    title: "t",
    body: "b",
    timestamp: 0,
    urgency: "normal",
    source: "文件管理",
    read: false,
    ...partial,
  };
}

describe("isNotificationVisible", () => {
  it("当前服务器通知可见，他服不可见", () => {
    expect(isNotificationVisible(n({ serverId: ACTIVE }), ACTIVE)).toBe(true);
    expect(isNotificationVisible(n({ serverId: "srv-b" }), ACTIVE)).toBe(false);
  });

  it("系统通知与 global 连接通知始终可见", () => {
    expect(isNotificationVisible(n(), ACTIVE)).toBe(true);
    expect(isNotificationVisible(n({ serverId: "srv-b", global: true }), ACTIVE)).toBe(true);
  });

  it("legacy 旧记录永久隐藏（避免跨服务器泄漏）", () => {
    expect(isNotificationVisible(n({ legacy: true }), ACTIVE)).toBe(false);
    expect(isNotificationVisible(n({ serverId: ACTIVE, legacy: true }), ACTIVE)).toBe(false);
  });

  it("activeServerId 为空时只保留 global/系统通知", () => {
    expect(isNotificationVisible(n(), null)).toBe(true);
    expect(isNotificationVisible(n({ serverId: ACTIVE }), null)).toBe(false);
    expect(isNotificationVisible(n({ serverId: ACTIVE, global: true }), null)).toBe(true);
  });
});

describe("migrateNotificationState", () => {
  it("无 serverId/global 的旧记录打 legacy，有归属的保持原样", () => {
    const migrated = migrateNotificationState({
      notifications: [
        n({ id: "old" }),
        n({ id: "srv", serverId: ACTIVE }),
        n({ id: "conn", serverId: ACTIVE, global: true }),
      ],
    }) as { notifications: AppNotification[] };

    expect(migrated.notifications[0].legacy).toBe(true);
    expect(migrated.notifications[1].legacy).toBeUndefined();
    expect(migrated.notifications[2].legacy).toBeUndefined();
  });

  it("空数据/损坏数据不报错", () => {
    expect(migrateNotificationState(null)).toEqual({ notifications: [] });
    expect(migrateNotificationState({})).toEqual({ notifications: [] });
    expect(migrateNotificationState({ notifications: "bad" })).toEqual({ notifications: [] });
  });
});

describe("mergeIncoming（同源聚合）", () => {
  const existing = n({ id: "n-old", count: 2, timestamp: 1000, body: "旧正文" });

  it("窗口内同 serverId/kind/source/title 且未读 → 合并：count+1、时间正文刷新、保留原 id", () => {
    const merged = mergeIncoming(existing, n({ timestamp: 1000 + MERGE_WINDOW_MS - 1 }));
    expect(merged).not.toBeNull();
    expect(merged!.id).toBe("n-old");
    expect(merged!.count).toBe(3);
    expect(merged!.timestamp).toBe(1000 + MERGE_WINDOW_MS - 1);
    expect(merged!.body).toBe("b");
    expect(merged!.live).toBe(true);
  });

  it("超窗 / 已读 / onAction / kind 不同 / 跨服务器 → 不合并", () => {
    expect(mergeIncoming(existing, n({ timestamp: 1000 + MERGE_WINDOW_MS }))).toBeNull();
    expect(mergeIncoming(n({ ...existing, read: true }), n({ timestamp: 1500 }))).toBeNull();
    expect(
      mergeIncoming(n({ ...existing, onAction: { label: "撤销", run: () => {} } }), n({ timestamp: 1500 })),
    ).toBeNull();
    // existing 无 kind（other）与 kind: archive 不同键
    expect(mergeIncoming(existing, n({ timestamp: 1500, kind: "archive" }))).toBeNull();
    expect(mergeIncoming(existing, n({ timestamp: 1500, serverId: "srv-b" }))).toBeNull();
  });

  it("details 追加去重，保留最近 30 行（溢出丢最旧）", () => {
    const merged = mergeIncoming(
      n({ ...existing, details: ["a"] }),
      n({ timestamp: 1500, details: ["a", "b"] }),
    );
    expect(merged!.details).toEqual(["a", "b"]);

    const full = n({ ...existing, details: Array.from({ length: 30 }, (_, i) => `d${i}`) });
    const overflow = mergeIncoming(full, n({ timestamp: 1500, details: ["d0", "new"] }));
    expect(overflow!.details).toHaveLength(30);
    expect(overflow!.details).toContain("new");
    expect(overflow!.details).not.toContain("d0");
  });

  it("pushNotification：60s 内同键未读通知合并为一条，原 id 保留", () => {
    useNotificationStore.setState({ notifications: [] });
    const { pushNotification } = useNotificationStore.getState();
    pushNotification({
      title: "连接中断", body: "第一次", urgency: "normal",
      source: "prod-1", kind: "connection", serverId: "srv-a",
    });
    const firstId = useNotificationStore.getState().notifications[0].id;
    pushNotification({
      title: "连接中断", body: "第二次", urgency: "normal",
      source: "prod-1", kind: "connection", serverId: "srv-a",
    });
    const list = useNotificationStore.getState().notifications;
    expect(list).toHaveLength(1);
    expect(list[0].id).toBe(firstId);
    expect(list[0].count).toBe(2);
    expect(list[0].body).toBe("第二次");
  });
});
