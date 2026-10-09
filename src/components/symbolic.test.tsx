// src/components/symbolic.test.tsx
import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { SymbolicIcon } from "./symbolic";

const ALL_NAMES = [
  "dialog-error", "dialog-warning", "dialog-information",
  "view-refresh", "network-offline",
  "mail-unread", "mailbox",
  "emblem-ok", "user-trash", "window-close",
  "bell", "bell-outline",
  "pan-up", "pan-down",
] as const;

describe("SymbolicIcon", () => {
  it("按 name 渲染 svg，尺寸生效", () => {
    const { container } = render(<SymbolicIcon name="dialog-error" size={20} />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg?.getAttribute("width")).toBe("20");
    expect(svg?.getAttribute("viewBox")).toBe("0 0 16 16");
  });

  it("使用 currentColor（颜色由父级继承）", () => {
    const { container } = render(<SymbolicIcon name="bell" />);
    expect(
      container.querySelector('svg [stroke="currentColor"], svg [fill="currentColor"]')
    ).not.toBeNull();
  });

  it("全部图标可渲染", () => {
    for (const name of ALL_NAMES) {
      const { container } = render(<SymbolicIcon name={name} />);
      expect(container.querySelector("svg")).not.toBeNull();
    }
  });

  it("未知图标回退占位不崩溃", () => {
    const { container } = render(<SymbolicIcon name={"no-such" as never} />);
    expect(container.querySelector("svg")).not.toBeNull();
  });
});
