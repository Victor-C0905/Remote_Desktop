/**
 * 凭据拆分逻辑单测：stripSecrets 落盘剥离 / hydrateCredentials 装配与旧明文迁移。
 * keyring 走 Rust invoke，此处 mock。
 */
import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { stripSecrets, hasSecrets, hydrateCredentials } from "./credentials";
import type { ServerConfig } from "../types/server";
import { AuthMethod } from "../types/server";

function makeServer(overrides: Partial<ServerConfig> = {}): ServerConfig {
  return {
    id: "srv-1",
    name: "test",
    host: "h",
    port: 9443,
    status: "disconnected",
    lastConnected: 0,
    auth: { method: AuthMethod.PUBKEY, username: "root" },
    ...overrides,
  } as ServerConfig;
}

beforeEach(() => {
  invokeMock.mockReset();
});

describe("stripSecrets", () => {
  it("剥离 password/privateKey/passphrase，保留 method/username", () => {
    const s = makeServer({
      auth: { method: AuthMethod.PASSWORD, username: "root", password: "p@ss", privateKey: "KEY", passphrase: "ph" },
    });
    const out = stripSecrets(s);
    expect(out.auth).toEqual({ method: AuthMethod.PASSWORD, username: "root" });
  });

  it("无 auth 时原样返回", () => {
    const s = makeServer();
    expect(stripSecrets(s)).toEqual(s);
  });
});

describe("hasSecrets", () => {
  it("识别敏感字段", () => {
    expect(hasSecrets({ password: "x" })).toBe(true);
    expect(hasSecrets({ privateKey: "" })).toBe(false);
    expect(hasSecrets({ username: "root" })).toBe(false);
    expect(hasSecrets(undefined)).toBe(false);
  });
});

describe("hydrateCredentials", () => {
  it("从 keyring 合并凭据回内存，且剥离结果可安全落盘", async () => {
    invokeMock.mockResolvedValueOnce(JSON.stringify({ privateKey: "SECRET-KEY" })); // credential_get srv-1
    const merged = await hydrateCredentials([makeServer()]);
    expect(merged[0].auth.privateKey).toBe("SECRET-KEY");
    // 落盘形态（再过一遍 strip）不含凭据
    expect(stripSecrets(merged[0]).auth.privateKey).toBeUndefined();
  });

  it("旧版明文凭据迁移进 keyring 并从 state 剥离", async () => {
    invokeMock.mockResolvedValueOnce(null); // credential_get → 无
    const legacy = makeServer({
      auth: { method: AuthMethod.PASSWORD, username: "root", password: "LEGACY" },
    });
    const merged = await hydrateCredentials([legacy]);
    // 写入了 keyring
    expect(invokeMock).toHaveBeenCalledWith("credential_set", {
      account: "srv-1",
      value: JSON.stringify({ password: "LEGACY" }),
    });
    // 内存保留（消费方零改动），落盘剥离
    expect(merged[0].auth.password).toBe("LEGACY");
    expect(stripSecrets(merged[0]).auth.password).toBeUndefined();
  });

  it("keyring 不可用时静默降级（凭据为空）", async () => {
    invokeMock.mockRejectedValue(new Error("no keyring"));
    const merged = await hydrateCredentials([makeServer()]);
    expect(merged[0].auth.privateKey).toBeUndefined();
  });
});
