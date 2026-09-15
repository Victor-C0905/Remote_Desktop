// src/shell/ArrivalToast.test.tsx
import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { ArrivalToast } from "./ArrivalToast";
import { useNotificationStore } from "../stores/notificationStore";

function push(title: string) {
  useNotificationStore.getState().pushNotification({
    title,
    body: "正文",
    urgency: "normal",
    source: "prod-1",
  });
}

describe("ArrivalToast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useNotificationStore.setState({ notifications: [] });
  });
  afterEach(() => vi.useRealTimers());

  it("初始不显示", () => {
    const { container } = render(<ArrivalToast onOpen={() => {}} />);
    expect(container.querySelector(".at-toast")).toBeNull();
  });

  it("push 后显示最新通知（标题 + 来源）", () => {
    render(<ArrivalToast onOpen={() => {}} />);
    act(() => push("通知 A"));
    expect(screen.getByText("通知 A")).toBeInTheDocument();
    expect(screen.getByText("prod-1")).toBeInTheDocument();
  });

  it("3.5s 后进入退出动画，再 200ms 完全隐藏", () => {
    const { container } = render(<ArrivalToast onOpen={() => {}} />);
    act(() => push("通知 A"));
    act(() => vi.advanceTimersByTime(3500));
    expect(container.querySelector(".at-toast-leaving")).not.toBeNull();
    act(() => vi.advanceTimersByTime(200));
    expect(container.querySelector(".at-toast")).toBeNull();
  });

  it("hover 暂停自动隐藏", () => {
    const { container } = render(<ArrivalToast onOpen={() => {}} />);
    act(() => push("通知 A"));
    fireEvent.mouseEnter(container.querySelector(".at-toast")!);
    act(() => vi.advanceTimersByTime(6000));
    expect(container.querySelector(".at-toast")).not.toBeNull();
  });

  it("点击触发 onOpen", () => {
    const onOpen = vi.fn();
    render(<ArrivalToast onOpen={onOpen} />);
    act(() => push("通知 A"));
    fireEvent.click(screen.getByRole("status"));
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it("连发替换内容并累计 +N", () => {
    render(<ArrivalToast onOpen={() => {}} />);
    act(() => push("通知 A"));
    act(() => push("通知 B"));
    expect(screen.getByText("通知 B")).toBeInTheDocument();
    expect(screen.queryByText("通知 A")).toBeNull();
    expect(screen.getByText("+1")).toBeInTheDocument();
  });
});
