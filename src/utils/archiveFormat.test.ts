import { describe, it, expect } from "vitest";
import {
  archiveFormatId,
  archiveFormatLabel,
  installCommand,
  describeCapability,
  partialSupportHint,
} from "./archiveFormat";
import type { ArchiveFormatCapability } from "../types/archive";

const osApt = { id: "ubuntu", version: "24.04", package_manager: "apt" };
const osApk = { id: "alpine", version: "3.20", package_manager: "apk" };

describe("archiveFormatId", () => {
  it("识别已支持格式", () => {
    expect(archiveFormatId("a.zip")).toBe("zip");
    expect(archiveFormatId("A.TAR.GZ")).toBe("tar.gz");
    expect(archiveFormatId("a.tgz")).toBe("tar.gz");
    expect(archiveFormatId("a.tar.zst")).toBe("tar.zst");
    expect(archiveFormatId("a.tar")).toBe("tar");
    expect(archiveFormatId("a.7z")).toBe("7z");
    expect(archiveFormatId("a.rar")).toBe("rar");
  });
  it("未知/单文件压缩返回 null", () => {
    expect(archiveFormatId("a.gz")).toBeNull();
    expect(archiveFormatId("a.txt")).toBeNull();
  });
});

describe("archiveFormatLabel", () => {
  it("格式 ID 转展示名", () => {
    expect(archiveFormatLabel("tar.gz")).toBe("TAR.GZ");
    expect(archiveFormatLabel("7z")).toBe("7Z");
  });
});

describe("installCommand", () => {
  it("按包管理器生成安装命令", () => {
    expect(installCommand(osApt, "unzip")).toBe("sudo apt install unzip");
    expect(installCommand(osApk, "unzip")).toBe("sudo apk add unzip");
    expect(installCommand(osApt, "7z")).toBe("sudo apt install p7zip-full");
  });
  it("未知包管理器返回 null", () => {
    expect(installCommand({ id: "linux", version: "", package_manager: "unknown" }, "unzip")).toBeNull();
  });
});

describe("describeCapability", () => {
  const missing: ArchiveFormatCapability = {
    id: "zip", can_list: false, can_extract_one: false, can_extract_all: false,
    reason_code: "tool_missing", missing_tool: "unzip",
  };
  it("缺工具 → 文案包含安装命令", () => {
    const text = describeCapability(osApt, missing);
    expect(text).toContain("unzip");
    expect(text).toContain("sudo apt install unzip");
  });
  it("探测失败 → 通用失败文案", () => {
    const text = describeCapability(osApt, { ...missing, reason_code: "probe_failed", missing_tool: undefined });
    expect(text).toContain("探测失败");
  });
});

describe("partialSupportHint", () => {
  it("可查看不可解压才提示", () => {
    expect(partialSupportHint({
      id: "zip", can_list: true, can_extract_one: false, can_extract_all: false, tool: "unzip",
    })).toContain("无法完成解压");
    expect(partialSupportHint({
      id: "zip", can_list: true, can_extract_one: true, can_extract_all: true, tool: "unzip",
    })).toBeNull();
  });
});
