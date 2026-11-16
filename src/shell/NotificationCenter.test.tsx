// src/shell/NotificationCenter.test.tsx
// 清空确认弹窗与通知中心宿主之间的模态前后顺序（Windows 标准：先弹窗后宿主）。

import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { NotificationCenter } from "./NotificationCenter";
import { useNotificationStore, type AppNotification } from "../stores/notificationStore";

function seed() {
  const n: AppNotification = {
    id: "n1",
    title: "连接中断",
    body: "网络断开",
    timestamp: Date.now(),
    urgency: "normal",
    source: "prod-1",
    serverId: "srv-a",
    read: false,
    live: true,
  };
  useNotificationStore.setState({ notifications: [n] });
}

describe("NotificationCenter 清空确认的模态顺序", () => {
  beforeEach(() => {
    seed();
  });

  it("弹窗打开时按 Esc 只关弹窗，不关闭通知中心；再按 Esc 才关通知中心", () => {
    const onClose = vi.fn();
    render(<NotificationCenter isOpen onClose={onClose} activeServerId="srv-a" />);

    // 打开清空确认弹窗（清除按钮 title = clearAllTitle）
    fireEvent.click(screen.getByTitle("清除全部"));
    expect(screen.getByText(/将清除当前视图内的/)).toBeInTheDocument();

    // 第一次 Esc：Dialog 自身的监听关闭弹窗；通知中心守卫生效不关闭
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByText(/将清除当前视图内的/)).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByText("通知")).toBeInTheDocument();

    // 第二次 Esc：弹窗已关，通知中心响应 Esc 关闭
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("未打开弹窗时按 Esc 直接关闭通知中心", () => {
    const onClose = vi.fn();
    render(<NotificationCenter isOpen onClose={onClose} activeServerId="srv-a" />);

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("打开面板即把当前服务器可见通知标为已读（用户拍板语义，未读仅指面板关闭期间到达）", () => {
    render(<NotificationCenter isOpen onClose={() => {}} activeServerId="srv-a" />);

    expect(useNotificationStore.getState().notifications[0].read).toBe(true);
  });
});
