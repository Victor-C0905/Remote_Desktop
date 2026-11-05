import { describe, it, expect } from "vitest";
import { describeExecFailure, describeInvokeError } from "./errors";
import i18n from "../../i18n";

const fail = (stderr: string, exitCode = 2) => ({ stdout: "", stderr, exitCode });

describe("describeExecFailure", () => {
  it("缺工具时指出具体工具", () => {
    const text = describeExecFailure(fail("tar (child): gzip: Cannot exec: No such file or directory"), "extract");
    expect(text).toContain("gzip");
  });
  it("无工具名的 command not found 给通用工具缺失提示", () => {
    expect(describeExecFailure(fail("sh: 1: command not found", 127), "list")).toContain("缺少解压工具");
  });
  it("权限不足/磁盘空间不足", () => {
    expect(describeExecFailure(fail("unzip: Permission denied"), "extract")).toContain("权限不足");
    expect(describeExecFailure(fail("write error: No space left on device"), "extract")).toContain("磁盘空间");
  });
  it("压缩包损坏/不支持的压缩方法", () => {
    expect(describeExecFailure(fail("tar: This does not look like a tar archive"), "list")).toContain("损坏");
    expect(describeExecFailure(fail("unsupported compression method 99"), "list")).toContain("不支持的压缩方法");
  });
  it("同名冲突与未知退出码兜底", () => {
    expect(describeExecFailure(fail("same.txt: Cannot open: File exists"), "extract")).toContain("同名文件");
    expect(describeExecFailure(fail("some unknown error", 9), "extract")).toContain("退出码 9");
  });
});

describe("describeInvokeError", () => {
  it("Agent 超时经 agent_rejected 归类为超时", () => {
    expect(describeInvokeError({
      code: "agent_rejected",
      params: { message: "Command timed out after 600s" },
    })).toContain("超时");
  });
  it("结构化权限错误走现有词典", () => {
    expect(describeInvokeError({ code: "permission_denied" })).toBe("权限不足");
  });
  it("连接错误保留既有结构化文案", () => {
    i18n.changeLanguage("zh-CN");
    expect(describeInvokeError({ code: 106 })).toContain("连接中断");
  });
});
