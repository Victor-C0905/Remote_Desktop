// src/utils/notify.ts
// 服务器归属通知的统一入口：自动补 serverId + 服务器名快照。
//
// 直接调用 notificationStore.pushNotification 时容易漏写 serverId，
// 这类通知会被当成系统通知在所有服务器下可见（跨服务器泄漏）。
// 与服务器相关的通知一律走这里。

import { useNotificationStore, type AppNotification } from "../stores/notificationStore";
import { useServersStore } from "../stores/serversStore";

/** 服务器通知入参：归属信息由 helper 统一补全 */
export type ServerNotificationInput = Omit<
  AppNotification,
  "id" | "timestamp" | "read" | "serverId" | "serverName"
>;

/**
 * 推送一条与服务器绑定的通知
 *
 * - serverId 为空时按系统通知推送（不写 serverId，所有视图可见）；
 * - 自动快照服务器名，供服务器改名/删除后仍能辨认归属。
 */
export function pushServerNotification(
  serverId: string | null | undefined,
  n: ServerNotificationInput,
): void {
  const server = serverId
    ? useServersStore.getState().servers.find((s) => s.id === serverId)
    : undefined;
  useNotificationStore.getState().pushNotification({
    ...n,
    ...(serverId ? { serverId, serverName: server?.name || server?.host } : {}),
  });
}
