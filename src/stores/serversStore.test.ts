// src/stores/serversStore.test.ts
import { describe, it, expect, beforeEach, vi } from "vitest";

// mock 存储层：persist 中间件在测试环境不可调用 Tauri
vi.mock("../utils/storage", () => ({
  serversStorage: { getItem: vi.fn(), setItem: vi.fn(), removeItem: vi.fn() },
}));

import { useServersStore, getServerErrorInfo } from "./serversStore";
import { AuthErrorCode } from "../types/errors";
import { AuthMethod } from "../types/server";

const auth = { method: AuthMethod.PASSWORD, username: "root" };

const baseServer = {
  id: "s1", name: "prod-1", host: "1.1.1.1", port: 22, auth,
  status: "error" as const,
};

describe("getServerErrorInfo", () => {
  beforeEach(() => {
    useServersStore.setState({
      servers: [
        { ...baseServer, error: { code: AuthErrorCode.ConnectTimeout } },
        { id: "s2", name: "prod-2", host: "2.2.2.2", port: 22, auth, status: "connected" as const, error: undefined },
      ],
      activeServerId: null,
    });
  });

  it("结构化错误 → 查表 title/message/action", () => {
    const info = getServerErrorInfo(useServersStore.getState().servers[0]);
    expect(info.title).toBe("无法连接服务器");
    expect(info.message).toBe("连接超时，未能建立连接");
    expect(info.action).toBe("请检查网络后重试");
  });

  it("字符串错误（旧格式）→ Unknown 映射 + detail", () => {
    const info = getServerErrorInfo({ ...baseServer, error: "legacy text" });
    expect(info.title).toBe("连接失败");
    expect(info.detail).toBe("legacy text");
  });

  it("其他服务器已连接 → hint 对比提示", () => {
    const info = getServerErrorInfo(useServersStore.getState().servers[0]);
    expect(info.hint).toContain("1 台服务器");
    expect(info.hint).toContain("正常");
  });

  it("无其他服务器 → 无 hint", () => {
    useServersStore.setState({ servers: [useServersStore.getState().servers[0]] });
    expect(getServerErrorInfo(useServersStore.getState().servers[0]).hint).toBeUndefined();
  });

  it("无 error 字段 → Unknown 兜底", () => {
    const info = getServerErrorInfo({ ...baseServer, error: undefined });
    expect(info.title).toBe("连接失败");
  });
});
