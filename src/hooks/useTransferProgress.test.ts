// src/hooks/useTransferProgress.test.ts
import { describe, it, expect, vi } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";

// mock Tauri 事件与存储（hook 模块顶层依赖，测试环境不可用）
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("../utils/transferStorage", () => ({
  TRANSFER_PURGE_SERVER_EVENT: "quirel:transfers:purge-server",
  transferStorage: {
    save: vi.fn(async () => {}),
    load: vi.fn(async () => []),
    clear: vi.fn(async () => {}),
    removeServer: vi.fn(async () => {}),
  },
}));

import { transferStorage } from "../utils/transferStorage";
import {
  degradeUnfinishedTasks,
  isTerminalStatus,
  useTransferProgress,
} from "./useTransferProgress";
// tsconfig 开启 isolatedModules：类型须用 import type 单独导入
import type { TransferTask } from "./useTransferProgress";

const task = (overrides: Partial<TransferTask>): TransferTask => ({
  id: "t1",
  session_id: "s1",
  direction: "download",
  file_name: "a.bin",
  remote_path: "/tmp/a.bin",
  file_size: 100,
  transferred: 0,
  speed: 0,
  eta: 0,
  status: "active",
  progress: 0,
  start_time: Date.now(),
  ...overrides,
});

describe("isTerminalStatus", () => {
  it("interrupted 是终态", () => {
    expect(isTerminalStatus("interrupted")).toBe(true);
  });

  it("active/pending/paused 非终态", () => {
    expect(isTerminalStatus("active")).toBe(false);
    expect(isTerminalStatus("pending")).toBe(false);
    expect(isTerminalStatus("paused")).toBe(false);
  });
});

describe("degradeUnfinishedTasks（僵尸记录降级）", () => {
  it("非终态任务降级为 interrupted，带中断文案", () => {
    const result = degradeUnfinishedTasks([
      task({ id: "t1", status: "active" }),
      task({ id: "t2", status: "paused" }),
    ]);
    expect(result.every((t) => t.status === "interrupted")).toBe(true);
    expect(result.every((t) => t.error === "连接已断开，传输已中断")).toBe(true);
  });

  it("终态任务原样保留", () => {
    const input = [
      task({ id: "t1", status: "completed" }),
      task({ id: "t2", status: "error", error: "原错误" }),
      task({ id: "t3", status: "interrupted" }),
    ];
    expect(degradeUnfinishedTasks(input)).toEqual(input);
  });
});

describe("服务器删除清理", () => {
  it("收到清理事件后移除该服务器任务并立即落盘", async () => {
    vi.mocked(transferStorage.load).mockResolvedValueOnce([
      task({ id: "t1", session_id: "s1", status: "completed" }),
      task({ id: "t2", session_id: "s2", status: "completed" }),
    ]);
    const { result } = renderHook(() => useTransferProgress());
    await waitFor(() => expect(result.current.transfers).toHaveLength(2));

    act(() => {
      window.dispatchEvent(new CustomEvent("quirel:transfers:purge-server", { detail: "s1" }));
    });

    await waitFor(() => expect(result.current.transfers.map((t) => t.id)).toEqual(["t2"]));
    expect(transferStorage.save).toHaveBeenCalledWith([expect.objectContaining({ id: "t2" })]);
  });

  it("清理事件先于加载完成时，加载结果也不会复活已删任务", async () => {
    let resolveLoad!: (tasks: TransferTask[]) => void;
    const pending = new Promise<TransferTask[]>((resolve) => {
      resolveLoad = resolve;
    });
    vi.mocked(transferStorage.load).mockReturnValueOnce(pending);

    const { result } = renderHook(() => useTransferProgress());
    act(() => {
      window.dispatchEvent(new CustomEvent("quirel:transfers:purge-server", { detail: "s1" }));
    });
    await act(async () => {
      resolveLoad([
        task({ id: "t1", session_id: "s1", status: "completed" }),
        task({ id: "t2", session_id: "s2", status: "completed" }),
      ]);
      await pending;
    });

    await waitFor(() => expect(result.current.transfers.map((t) => t.id)).toEqual(["t2"]));
  });
});
