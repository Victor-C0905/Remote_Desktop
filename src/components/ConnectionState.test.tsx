// src/components/ConnectionState.test.tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ConnectionErrorState, ReconnectingState } from "./ConnectionState";

describe("ConnectionErrorState", () => {
  it("渲染标题/消息/建议/对比提示/重试按钮", () => {
    render(
      <ConnectionErrorState
        title="无法连接服务器"
        message="连接超时，未能建立连接"
        action="请检查网络后重试"
        hint="ℹ️ 其他 2 台服务器连接正常，仅此台无法连接"
        onRetry={() => {}}
      />
    );
    expect(screen.getByText("无法连接服务器")).toBeInTheDocument();
    expect(screen.getByText("连接超时，未能建立连接")).toBeInTheDocument();
    expect(screen.getByText("请检查网络后重试")).toBeInTheDocument();
    expect(screen.getByText("ℹ️ 其他 2 台服务器连接正常，仅此台无法连接")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重试" })).toBeInTheDocument();
  });

  it("无 hint 不渲染；无 onOpenSettings 不渲染次按钮", () => {
    render(<ConnectionErrorState title="t" message="m" onRetry={() => {}} />);
    expect(screen.queryByText(/台服务器/)).toBeNull();
    expect(screen.queryByRole("button", { name: "查看服务器" })).toBeNull();
  });

  it("重试按钮触发 onRetry", () => {
    const onRetry = vi.fn();
    render(<ConnectionErrorState title="t" message="m" onRetry={onRetry} />);
    fireEvent.click(screen.getByRole("button", { name: "重试" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("查看服务器按钮触发 onOpenSettings", () => {
    const onOpenSettings = vi.fn();
    render(
      <ConnectionErrorState title="t" message="m" onRetry={() => {}} onOpenSettings={onOpenSettings} />
    );
    fireEvent.click(screen.getByRole("button", { name: "查看服务器" }));
    expect(onOpenSettings).toHaveBeenCalledOnce();
  });
});

describe("ReconnectingState", () => {
  it("渲染重连文案与服务器名", () => {
    render(<ReconnectingState serverName="prod-1" />);
    expect(screen.getByText(/正在重新连接到 prod-1/)).toBeInTheDocument();
  });

  it("无服务器名时渲染通用文案", () => {
    render(<ReconnectingState />);
    expect(screen.getByText("正在重新连接…")).toBeInTheDocument();
  });
});
