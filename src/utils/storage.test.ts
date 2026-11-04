// src/utils/storage.test.ts
import { describe, it, expect, vi } from "vitest";

// 隔离 Tauri 依赖：本文件验证磁盘契约判定与写保护
vi.mock("@tauri-apps/plugin-store", () => ({ load: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { load } from "@tauri-apps/plugin-store";
import { evaluatePersistedValue, createTauriStorage, getStoreBlocks } from "./storage";

const loadMock = vi.mocked(load);

describe("evaluatePersistedValue", () => {
  it("缺失数据视为全新 store，允许初始化", () => {
    expect(evaluatePersistedValue(null, 0)).toEqual({ ok: true });
    expect(evaluatePersistedValue(undefined, 0)).toEqual({ ok: true });
  });

  it("版本一致且含 state → 允许读取", () => {
    expect(evaluatePersistedValue({ state: { servers: [] }, version: 0 }, 0))
      .toEqual({ ok: true });
  });

  it("磁盘版本高于当前支持版本 → 阻断（旧应用不得覆盖新数据）", () => {
    const verdict = evaluatePersistedValue({ state: { servers: [] }, version: 1 }, 0);
    expect(verdict.ok).toBe(false);
    if (!verdict.ok) {
      expect(verdict.reason).toBe("newer-version");
      expect(verdict.version).toBe(1);
    }
  });

  it("缺少 state → 结构不可读，阻断", () => {
    const verdict = evaluatePersistedValue({ version: 0 }, 0);
    expect(verdict.ok).toBe(false);
    if (!verdict.ok) expect(verdict.reason).toBe("unreadable");
  });

  it("非对象（数组/字符串/数字）→ 阻断", () => {
    for (const value of [[], "text", 42]) {
      const verdict = evaluatePersistedValue(value, 0);
      expect(verdict.ok).toBe(false);
      if (!verdict.ok) expect(verdict.reason).toBe("unreadable");
    }
  });
});

describe("写入保护", () => {
  it("高版本文件：读取被拒、写入不落盘、记录阻断原因", async () => {
    const store = {
      get: vi.fn(async () => ({ state: { servers: [] }, version: 1 })),
      set: vi.fn(async () => {}),
      delete: vi.fn(async () => {}),
      save: vi.fn(async () => {}),
    };
    loadMock.mockResolvedValueOnce(store as never);

    const storage = await createTauriStorage("servers.bin");
    expect(await storage.getItem("quirel-servers")).toBeNull();
    await storage.setItem("quirel-servers", JSON.stringify({ state: { servers: [] }, version: 1 }));
    expect(store.set).not.toHaveBeenCalled();

    const block = getStoreBlocks().find((b) => b.path === "servers.bin");
    expect(block?.reason).toBe("newer-version");
    expect(block?.version).toBe(1);
  });
});
