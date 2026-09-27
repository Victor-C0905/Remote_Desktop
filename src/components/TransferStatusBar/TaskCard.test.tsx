// src/components/TransferStatusBar/TaskCard.test.tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";

// mock Tauri invoke
const invokeMock = vi.fn(async (..._args: unknown[]) => {});
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// mock ServerManager context：返回固定 activeServerId 与可 spy 的 connectServer
const connectServerMock = vi.fn(async () => {});
vi.mock("../../context/ServerManager", () => ({
  useServerManager: () => ({
    activeServerId: "s-current",
    connectServer: connectServerMock,
  }),
}));

import { TaskCard } from "./TaskCard";
import type { TransferTask } from "../../hooks/useTransferProgress";
// 真实 serversStore：handleSwitchAndRetry 用 getState() 非响应式读取连接状态，
// 测试通过 setState 预置 "s-other" 为 connected（connectServer 是 mock，不会自行更新 store）
import { useServersStore } from "../../stores/serversStore";
import { AuthMethod } from "../../types/server";

const interruptedTask: TransferTask = {
  id: "t1",
  session_id: "s-other",
  direction: "download",
  file_name: "big.tar.gz",
  remote_path: "/tmp/big.tar.gz",
  local_path: "/home/user/Downloads/big.tar.gz",
  file_size: 100,
  transferred: 40,
  speed: 0,
  eta: 0,
  status: "interrupted",
  error: "连接已断开，传输已中断",
  progress: 40,
  start_time: Date.now(),
};

describe("TaskCard · interrupted 状态", () => {
  beforeEach(() => {
    invokeMock.mockClear();
    // 重置实现：个别用例会用 mockImplementation 模拟特定命令失败，防止泄漏到后续用例
    invokeMock.mockImplementation(async (..._args: unknown[]) => {});
    connectServerMock.mockClear();
    // 预置：目标服务器 s-other 已连接（连接成功的最终态由 connectServer 写入，此处直接模拟）
    useServersStore.setState({
      servers: [
        {
          id: "s-other",
          name: "web-01",
          host: "1.1.1.1",
          port: 22,
          auth: { method: AuthMethod.PASSWORD, username: "root" },
          status: "connected" as const,
        },
      ],
      activeServerId: "s-current",
    });
  });

  it("渲染「已中断」状态文案", () => {
    render(<TaskCard task={interruptedTask} onRemove={() => {}} />);
    expect(screen.getByText("已中断")).toBeTruthy();
  });

  it("渲染「切回服务器并重试」按钮", () => {
    render(<TaskCard task={interruptedTask} onRemove={() => {}} />);
    expect(screen.getByRole("button", { name: "切回服务器并重试" })).toBeTruthy();
  });

  it("点击重试：先切回原服务器，再调 retry_transfer", async () => {
    render(<TaskCard task={interruptedTask} onRemove={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "切回服务器并重试" }));
    await vi.waitFor(() => {
      expect(connectServerMock).toHaveBeenCalledWith("s-other");
      expect(invokeMock).toHaveBeenCalledWith("retry_transfer", { taskId: "t1" });
    });
  });

  it("防连点：重试进行中再次点击不重复发起", async () => {
    render(<TaskCard task={interruptedTask} onRemove={() => {}} />);
    const btn = screen.getByRole("button", { name: "切回服务器并重试" });
    fireEvent.click(btn);
    fireEvent.click(btn); // 第一次链路（含连接等待）未完成时连点
    await vi.waitFor(() => {
      expect(connectServerMock).toHaveBeenCalledTimes(1);
    });
    expect(invokeMock).toHaveBeenCalledWith("retry_transfer", { taskId: "t1" });
  });

  it("retry_transfer 失败（记录丢失）：用原始参数重新发起并移除旧记录", async () => {
    // 模拟后端任务已不存在：retry_transfer 报错，transfer_file 正常
    invokeMock.mockImplementation(async (...args: unknown[]) => {
      if (args[0] === "retry_transfer") throw new Error("任务不存在: t1");
    });
    const onRemove = vi.fn();
    render(<TaskCard task={interruptedTask} onRemove={onRemove} />);
    fireEvent.click(screen.getByRole("button", { name: "切回服务器并重试" }));
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("transfer_file", {
        serverId: "s-other",
        direction: "download",
        remotePath: "/tmp/big.tar.gz",
        localPath: "/home/user/Downloads/big.tar.gz",
      });
    });
    expect(onRemove).toHaveBeenCalledWith("t1");
  });

  it("切回服务器失败（状态非 connected）：不调用 retry_transfer", async () => {
    useServersStore.setState({
      servers: [
        {
          id: "s-other",
          name: "web-01",
          host: "1.1.1.1",
          port: 22,
          auth: { method: AuthMethod.PASSWORD, username: "root" },
          status: "error" as const,
        },
      ],
      activeServerId: "s-current",
    });
    render(<TaskCard task={interruptedTask} onRemove={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "切回服务器并重试" }));
    await vi.waitFor(() => {
      expect(connectServerMock).toHaveBeenCalledTimes(1);
    });
    // 让连接等待后的状态检查微任务跑完
    await new Promise((r) => setTimeout(r, 0));
    expect(invokeMock).not.toHaveBeenCalledWith("retry_transfer", { taskId: "t1" });
    expect(invokeMock).not.toHaveBeenCalledWith("transfer_file", expect.anything());
  });
});
