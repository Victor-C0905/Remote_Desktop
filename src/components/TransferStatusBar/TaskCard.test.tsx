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

// 通用任务工厂：按用例覆盖字段（状态/方向/local_path）
const makeTask = (overrides: Partial<TransferTask> = {}): TransferTask => ({
  id: "t2",
  session_id: "s-current",
  direction: "download",
  file_name: "report.pdf",
  remote_path: "/tmp/report.pdf",
  local_path: "/home/user/Downloads/report.pdf",
  file_size: 100,
  transferred: 40,
  speed: 0,
  eta: 0,
  status: "error",
  error: "与服务器之间的数据传输中断",
  progress: 40,
  start_time: Date.now(),
  ...overrides,
});

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

describe("TaskCard · 移除任务时回收临时文件", () => {
  beforeEach(() => {
    invokeMock.mockClear();
    // 重置实现：个别用例会用 mockImplementation 模拟特定命令失败，防止泄漏到后续用例
    invokeMock.mockImplementation(async (..._args: unknown[]) => {});
    connectServerMock.mockClear();
    // 预置：目标服务器 s-other 已连接（「重试兜底重发不删临时文件」用例需要）
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

  it("终态下载任务点 X：以 localPath 回收临时文件并移除记录", async () => {
    const onRemove = vi.fn();
    render(<TaskCard task={makeTask({ status: "error" })} onRemove={onRemove} />);
    fireEvent.click(screen.getByRole("button", { name: "关闭任务" }));
    await vi.waitFor(() => {
      expect(onRemove).toHaveBeenCalledWith("t2");
    });
    expect(invokeMock).toHaveBeenCalledWith("delete_transfer_temp", {
      localPath: "/home/user/Downloads/report.pdf",
    });
  });

  it("active 下载任务点 X：取消、回收临时文件、移除均被调用", async () => {
    const onRemove = vi.fn();
    render(<TaskCard task={makeTask({ status: "active" })} onRemove={onRemove} />);
    fireEvent.click(screen.getByRole("button", { name: "关闭任务" }));
    await vi.waitFor(() => {
      expect(onRemove).toHaveBeenCalledWith("t2");
    });
    expect(invokeMock).toHaveBeenCalledWith("cancel_transfer", { taskId: "t2" });
    expect(invokeMock).toHaveBeenCalledWith("delete_transfer_temp", {
      localPath: "/home/user/Downloads/report.pdf",
    });
  });

  it("下载任务 local_path 缺失：不调用 delete_transfer_temp", async () => {
    const onRemove = vi.fn();
    const task = makeTask({ status: "error" });
    delete task.local_path; // 可选字段缺失的防御
    render(<TaskCard task={task} onRemove={onRemove} />);
    fireEvent.click(screen.getByRole("button", { name: "关闭任务" }));
    await vi.waitFor(() => {
      expect(onRemove).toHaveBeenCalledWith("t2");
    });
    expect(invokeMock).not.toHaveBeenCalledWith("delete_transfer_temp", expect.anything());
  });

  it("上传任务点 X：不调用 delete_transfer_temp（local_path 是源文件，不能误删）", async () => {
    const onRemove = vi.fn();
    render(
      <TaskCard task={makeTask({ direction: "upload", status: "error" })} onRemove={onRemove} />
    );
    fireEvent.click(screen.getByRole("button", { name: "关闭任务" }));
    await vi.waitFor(() => {
      expect(onRemove).toHaveBeenCalledWith("t2");
    });
    expect(invokeMock).not.toHaveBeenCalledWith("delete_transfer_temp", expect.anything());
  });

  it("重试兜底重发成功路径：移除旧记录但不动临时文件（新任务续传中）", async () => {
    // 模拟后端任务记录丢失：retry_transfer 报错，transfer_file 兜底重发成功
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
    // 兜底重发成功后的移除不是放弃：新任务正在续传 .tmp，绝不能回收
    expect(invokeMock).not.toHaveBeenCalledWith("delete_transfer_temp", expect.anything());
  });
});
