// src/components/TransferLimitCard.test.tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

// mock Tauri invoke：按命令名分发（用例以 mockImplementation 覆盖）
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { TransferLimitCard } from "./TransferLimitCard";

const noTransferCalls = () =>
  invokeMock.mock.calls.every(
    ([cmd]) => !["get_transfer_limit", "set_transfer_limit"].includes(cmd as string)
  );

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation(async (..._args: unknown[]) => {});
});

describe("TransferLimitCard · 上传大小限制设置", () => {
  it("未连接：输入禁用，显示连接提示，不发起任何请求", () => {
    render(<TransferLimitCard serverId={null} />);
    expect(screen.getByRole("spinbutton")).toBeDisabled();
    expect(screen.getByText("连接服务器后可查看")).toBeTruthy();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("旧 Agent：显示不支持提示，绝不调用新协议命令（盲发会拆掉连接）", async () => {
    invokeMock.mockImplementation(async () => ["metrics"]); // 无 transfer_limit 能力
    render(<TransferLimitCard serverId="s1" />);
    await waitFor(() => {
      expect(screen.getByText("当前 Agent 版本不支持此设置，请升级 Agent")).toBeTruthy();
    });
    expect(invokeMock).toHaveBeenCalledWith("get_agent_capabilities", { serverId: "s1" });
    expect(noTransferCalls()).toBe(true);
  });

  it("已连接 root：回填当前值并可修改，应用后提示已生效", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit")
        return { max_file_transfer_mb: 500, editable: true, persisted: true };
      if (cmd === "set_transfer_limit")
        return { max_file_transfer_mb: 2048, editable: true, persisted: true };
      return {};
    });
    render(<TransferLimitCard serverId="s1" />);
    const input = screen.getByRole("spinbutton");
    await waitFor(() => expect((input as HTMLInputElement).value).toBe("500"));
    expect(input).not.toBeDisabled();

    fireEvent.change(input, { target: { value: "2048" } });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));
    await waitFor(() => expect(screen.getByText("已生效")).toBeTruthy());
    expect(invokeMock).toHaveBeenCalledWith("set_transfer_limit", {
      serverId: "s1",
      maxFileTransferMb: 2048,
    });
  });

  it("应用成功但写回配置失败：提示重启后恢复原值", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit")
        return { max_file_transfer_mb: 500, editable: true, persisted: true };
      if (cmd === "set_transfer_limit")
        return { max_file_transfer_mb: 2048, editable: true, persisted: false };
      return {};
    });
    render(<TransferLimitCard serverId="s1" />);
    await waitFor(() =>
      expect((screen.getByRole("spinbutton") as HTMLInputElement).value).toBe("500")
    );
    fireEvent.change(screen.getByRole("spinbutton"), { target: { value: "2048" } });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));
    await waitFor(() =>
      expect(screen.getByText("已生效，但配置文件写入失败，重启后恢复原值")).toBeTruthy()
    );
  });

  it("非 root：回填当前值但输入禁用并提示需要 root", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit")
        return { max_file_transfer_mb: 500, editable: false, persisted: true };
      return {};
    });
    render(<TransferLimitCard serverId="s1" />);
    await waitFor(() =>
      expect((screen.getByRole("spinbutton") as HTMLInputElement).value).toBe("500")
    );
    expect(screen.getByRole("spinbutton")).toBeDisabled();
    expect(screen.getByText("需要以 root 用户连接才能修改")).toBeTruthy();
  });

  it("本地校验：超出值域时应用禁用且不发送请求", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit")
        return { max_file_transfer_mb: 500, editable: true, persisted: true };
      return {};
    });
    render(<TransferLimitCard serverId="s1" />);
    await waitFor(() =>
      expect((screen.getByRole("spinbutton") as HTMLInputElement).value).toBe("500")
    );
    for (const bad of ["0", "200000"]) {
      fireEvent.change(screen.getByRole("spinbutton"), { target: { value: bad } });
      expect(screen.getByRole("button", { name: "应用" })).toBeDisabled();
    }
    // 加载回填阶段必然调用 get_transfer_limit；本用例约束的是应用请求不被发送
    expect(
      invokeMock.mock.calls.some(([cmd]) => cmd === "set_transfer_limit")
    ).toBe(false);
  });

  it("查询失败：hint 显示错误文案，输入禁用", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit") throw new Error("连接已断开");
      return {};
    });
    render(<TransferLimitCard serverId="s1" />);
    await waitFor(() => {
      expect(screen.getByText("Error: 连接已断开")).toBeTruthy();
    });
    expect(screen.getByRole("spinbutton")).toBeDisabled();
    expect(screen.getByRole("button", { name: "应用" })).toBeDisabled();
  });

  it("切换服务器：上一台的反馈与回填值不泄漏", async () => {
    invokeMock.mockImplementation(async (cmd: string, args?: { serverId?: string }) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit")
        return args?.serverId === "s1"
          ? { max_file_transfer_mb: 500, editable: true, persisted: true }
          : { max_file_transfer_mb: 1024, editable: true, persisted: true };
      if (cmd === "set_transfer_limit")
        return { max_file_transfer_mb: 2048, editable: true, persisted: true };
      return {};
    });
    const { rerender } = render(<TransferLimitCard serverId="s1" />);
    const input = screen.getByRole("spinbutton");
    await waitFor(() => expect((input as HTMLInputElement).value).toBe("500"));
    fireEvent.change(input, { target: { value: "2048" } });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));
    await waitFor(() => expect(screen.getByText("已生效")).toBeTruthy());

    // 断开：A 的「已生效」消失、输入清空且禁用
    rerender(<TransferLimitCard serverId={null} />);
    expect(screen.queryByText("已生效")).toBeNull();
    expect((screen.getByRole("spinbutton") as HTMLInputElement).value).toBe("");
    expect(screen.getByRole("spinbutton")).toBeDisabled();

    // 换 B 服务器：A 的反馈不残留，B 的当前值正常回填
    rerender(<TransferLimitCard serverId="s2" />);
    await waitFor(() =>
      expect((screen.getByRole("spinbutton") as HTMLInputElement).value).toBe("1024")
    );
    expect(screen.queryByText("已生效")).toBeNull();
  });

  it("本地校验：非法值时 hint 显示值域提示文案", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit")
        return { max_file_transfer_mb: 500, editable: true, persisted: true };
      return {};
    });
    render(<TransferLimitCard serverId="s1" />);
    await waitFor(() =>
      expect((screen.getByRole("spinbutton") as HTMLInputElement).value).toBe("500")
    );
    fireEvent.change(screen.getByRole("spinbutton"), { target: { value: "0" } });
    expect(screen.getByText("请输入 1 – 102400 之间的整数")).toBeTruthy();
  });

  it("应用进行中：按钮禁用，连点不重复发起 set 请求", async () => {
    let resolveSet: (v: unknown) => void = () => {};
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "get_agent_capabilities") return ["transfer_limit"];
      if (cmd === "get_transfer_limit")
        return { max_file_transfer_mb: 500, editable: true, persisted: true };
      if (cmd === "set_transfer_limit")
        // 慢 promise：让 set 请求停留在 in-flight 态
        return new Promise((resolve) => {
          resolveSet = resolve;
        });
      return {};
    });
    render(<TransferLimitCard serverId="s1" />);
    const input = screen.getByRole("spinbutton");
    await waitFor(() => expect((input as HTMLInputElement).value).toBe("500"));
    fireEvent.change(input, { target: { value: "2048" } });
    fireEvent.click(screen.getByRole("button", { name: "应用" }));
    // set 进行中：按钮禁用，再次点击不重复发起
    expect(screen.getByRole("button", { name: "应用" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "应用" }));
    expect(
      invokeMock.mock.calls.filter(([cmd]) => cmd === "set_transfer_limit").length
    ).toBe(1);
    resolveSet({ max_file_transfer_mb: 2048, editable: true, persisted: true });
    await waitFor(() => expect(screen.getByText("已生效")).toBeTruthy());
  });
});
