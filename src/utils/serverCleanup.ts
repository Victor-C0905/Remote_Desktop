import { invoke } from "@tauri-apps/api/core";
import { createLogger } from "./logger";

const log = createLogger("ServerCleanup");

/**
 * 删除服务器时的统一数据清理入口。
 *
 * 所有按 serverId 隔离的持久化数据/缓存都必须在此登记，否则删除服务器后会
 * 留下无法触达的孤儿数据（历史缺口：传输任务、浏览器 profile）。各步骤幂等、
 * 互不依赖；单步失败只记录 error，不阻断服务器移除。
 */
export async function purgeServerData(serverId: string): Promise<void> {
  const steps: Array<{ label: string; run: () => unknown }> = [
    {
      label: "通知",
      run: async () =>
        (await import("../stores/notificationStore")).useNotificationStore.getState().purgeServer(serverId),
    },
    {
      label: "常用路径",
      run: async () =>
        (await import("../stores/filePathUsageStore")).useFilePathUsageStore.getState().clearServer(serverId),
    },
    {
      label: "压缩能力",
      run: async () =>
        (await import("../stores/archiveCapabilitiesStore")).useArchiveCapabilitiesStore.getState().clearServer(serverId),
    },
    {
      label: "传输任务",
      run: async () => (await import("./transferStorage")).transferStorage.removeServer(serverId),
    },
    {
      // 浏览器 profile/cookie/settings 目录在 Rust 侧按 serverId 存放；
      // 调用方已先停止浏览器会话，命令内部对 WebView2 句柄释放延迟做重试
      label: "浏览器数据",
      run: () => invoke("browser_purge_server_data", { serverId }),
    },
  ];

  const results = await Promise.allSettled(steps.map(async (s) => s.run()));
  results.forEach((result, i) => {
    if (result.status === "rejected") {
      log.error(`清理服务器${steps[i].label}失败:`, result.reason);
    }
  });
}
