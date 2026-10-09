import { describe, it, expect } from "vitest";
import {
  AuthErrorCode,
  getErrorInfo,
  parseConnectError,
  buildConnectFailureText,
  describeTerminalFailure,
} from "./errors";

describe("ERROR_MAP 全覆盖（每个 code 必有条目）", () => {
  for (const code of Object.values(AuthErrorCode)) {
    it(`code ${code} 有映射条目`, () => {
      const info = getErrorInfo(code);
      expect(info.title.length).toBeGreaterThan(0);
      expect(info.message.length).toBeGreaterThan(0);
      expect(info.action.length).toBeGreaterThan(0);
      expect(typeof info.retryable).toBe("boolean");
    });
  }
});

describe("parseConnectError", () => {
  it("解析结构化 ConnectError JSON", () => {
    expect(parseConnectError({ code: 201, detail: "prod-1" })).toEqual({ code: 201, detail: "prod-1" });
  });
  it("未知数值回退 Unknown", () => {
    expect(parseConnectError({ code: 12345 }).code).toBe(AuthErrorCode.Unknown);
  });
  it("裸字符串 → Unknown + detail", () => {
    expect(parseConnectError("任意旧文本")).toEqual({ code: AuthErrorCode.Unknown, detail: "任意旧文本" });
  });
  it("Error 对象 → Unknown + message", () => {
    expect(parseConnectError(new Error("boom")).detail).toBe("boom");
  });
});

describe("buildConnectFailureText", () => {
  it("包含标题/消息/建议", () => {
    const text = buildConnectFailureText({ code: AuthErrorCode.AccountLocked }, 0);
    expect(text).toContain("账户已临时锁定");
    expect(text).toContain("15 分钟");
    expect(text).toContain("建议：");
  });
  it("其他服务器正常时附加对比提示", () => {
    const text = buildConnectFailureText({ code: AuthErrorCode.ConnectionLost }, 2);
    expect(text).toContain("其他 2 台服务器连接正常");
  });
  it("无其他服务器时不附加对比提示", () => {
    expect(buildConnectFailureText({ code: AuthErrorCode.ConnectionLost }, 0)).not.toContain("台服务器连接正常");
  });
});

describe("describeTerminalFailure", () => {
  it("未连接时给出连接指引", () => {
    expect(describeTerminalFailure("未找到连接")).toContain("先连接");
  });
  it("未知错误保留原文", () => {
    expect(describeTerminalFailure("启动终端失败: xyz")).toBe("启动终端失败: xyz");
  });
});
