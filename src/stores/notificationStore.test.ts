// src/stores/notificationStore.test.ts
// 通知彻底按服务器隔离的核心规则：可见性与旧数据迁移。
// 纯函数测试，不依赖 UI / Tauri runtime。

import { describe, it, expect } from "vitest";
import {
  isNotificationVisible,
  migrateNotificationState,
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
