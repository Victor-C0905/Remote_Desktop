import { describe, it, expect, beforeEach, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { useArchiveCapabilitiesStore } from "./archiveCapabilitiesStore";
import type { ArchiveCapabilities } from "../types/archive";

const suspendMock = vi.mocked(invoke);

const caps: ArchiveCapabilities = {
  probed_at: 1,
  os: { id: "ubuntu", version: "24.04", package_manager: "apt" },
  tools: [{ name: "unzip", path: "/usr/bin/unzip", version: "UnZip 6.00" }],
  formats: [
    { id: "zip", can_list: true, can_extract_one: true, can_extract_all: true, tool: "unzip" },
    { id: "rar", can_list: false, can_extract_one: false, can_extract_all: false, reason_code: "tool_missing", missing_tool: "unrar" },
  ],
};

describe("archiveCapabilitiesStore", () => {
  beforeEach(() => {
    useArchiveCapabilitiesStore.setState({ byServer: {} });
  });

  it("拉取成功后写入 ready 并按格式查询", async () => {
    suspendMock.mockResolvedValueOnce(caps);
    await useArchiveCapabilitiesStore.getState().fetchCapabilities("s1");
    const state = useArchiveCapabilitiesStore.getState().byServer["s1"];
    expect(state.status).toBe("ready");
    expect(state.data?.formats.length).toBe(2);
    expect(useArchiveCapabilitiesStore.getState().formatFor("s1", "zip")?.tool).toBe("unzip");
    expect(suspendMock).toHaveBeenCalledWith("remote_archive_capabilities", { serverId: "s1", refresh: false });
  });

  it("拉取失败记录 failed，不抛异常", async () => {
    suspendMock.mockRejectedValueOnce(new Error("boom"));
    await useArchiveCapabilitiesStore.getState().fetchCapabilities("s1");
    expect(useArchiveCapabilitiesStore.getState().byServer["s1"].status).toBe("failed");
  });

  it("clearServer 清理缓存", async () => {
    suspendMock.mockResolvedValueOnce(caps);
    await useArchiveCapabilitiesStore.getState().fetchCapabilities("s1");
    useArchiveCapabilitiesStore.getState().clearServer("s1");
    expect(useArchiveCapabilitiesStore.getState().byServer["s1"]).toBeUndefined();
  });
});
