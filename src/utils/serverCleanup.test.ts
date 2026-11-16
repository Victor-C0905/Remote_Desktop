import { describe, it, expect, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

// 各按服务器数据的清理入口（mock 后断言统一编排是否覆盖全部域）
const mocks = vi.hoisted(() => ({
  purgeServer: vi.fn(),
  clearUsage: vi.fn(),
  clearCapabilities: vi.fn(),
  removeServerTasks: vi.fn(),
}));

vi.mock("../stores/notificationStore", () => ({
  useNotificationStore: { getState: () => ({ purgeServer: mocks.purgeServer }) },
}));
vi.mock("../stores/filePathUsageStore", () => ({
  useFilePathUsageStore: { getState: () => ({ clearServer: mocks.clearUsage }) },
}));
vi.mock("../stores/archiveCapabilitiesStore", () => ({
  useArchiveCapabilitiesStore: { getState: () => ({ clearServer: mocks.clearCapabilities }) },
}));
vi.mock("./transferStorage", () => ({
  transferStorage: { removeServer: mocks.removeServerTasks },
}));

import { purgeServerData } from "./serverCleanup";

const invokeMock = vi.mocked(invoke);

describe("purgeServerData", () => {
  it("删除服务器时清理全部按服务器隔离的数据", async () => {
    invokeMock.mockResolvedValueOnce(undefined);

    await purgeServerData("s1");

    expect(mocks.purgeServer).toHaveBeenCalledWith("s1");
    expect(mocks.clearUsage).toHaveBeenCalledWith("s1");
    expect(mocks.clearCapabilities).toHaveBeenCalledWith("s1");
    expect(mocks.removeServerTasks).toHaveBeenCalledWith("s1");
    expect(invokeMock).toHaveBeenCalledWith("browser_purge_server_data", { serverId: "s1" });
  });

  it("单步失败不阻断其余清理，也不 reject", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    mocks.clearCapabilities.mockImplementationOnce(() => {
      throw new Error("capability boom");
    });
    mocks.removeServerTasks.mockRejectedValueOnce(new Error("transfer boom"));

    await expect(purgeServerData("s2")).resolves.toBeUndefined();

    expect(mocks.purgeServer).toHaveBeenCalledWith("s2");
    expect(mocks.clearUsage).toHaveBeenCalledWith("s2");
    expect(invokeMock).toHaveBeenCalledWith("browser_purge_server_data", { serverId: "s2" });
  });
});
