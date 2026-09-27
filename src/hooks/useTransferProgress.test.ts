// src/hooks/useTransferProgress.test.ts
import { describe, it, expect, vi } from "vitest";

// mock Tauri 事件与存储（hook 模块顶层依赖，测试环境不可用）
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("../utils/transferStorage", () => ({
  transferStorage: {
    save: vi.fn(async () => {}),
    load: vi.fn(async () => []),
    clear: vi.fn(async () => {}),
  },
}));

import {
  degradeUnfinishedTasks,
  isTerminalStatus,
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
