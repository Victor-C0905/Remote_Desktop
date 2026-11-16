// AppLayout 侧栏拖拽调宽回归（R12-FM-1）：
// 侧栏此前固定 240px 无法调整。新增可选拖拽分隔柄（opt-in）：
// - 未启用 sidebarResizable 时不渲染分隔柄（Settings 等不受影响）
// - 拖拽经 mousemove 连续回报夹取后的宽度（160–420）
// - 双击分隔柄重置为默认宽
import { render, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { AppLayout } from "./AppLayout";
import { clampSidebarWidth } from "./sidebarWidth";

describe("clampSidebarWidth", () => {
  it("落在区间内原样返回（四舍五入到整数 px）", () => {
    expect(clampSidebarWidth(240.4)).toBe(240);
    expect(clampSidebarWidth(300)).toBe(300);
  });
  it("低于下限夹到 160、高于上限夹到 420", () => {
    expect(clampSidebarWidth(0)).toBe(160);
    expect(clampSidebarWidth(-50)).toBe(160);
    expect(clampSidebarWidth(999)).toBe(420);
  });
});

describe("AppLayout 可调宽侧栏", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  function mount(onChange: (w: number) => void, onReset: () => void) {
    render(
      <AppLayout
        sidebar={<div>SB</div>}
        sidebarWidth={240}
        sidebarResizable
        onSidebarWidthChange={onChange}
        onSidebarReset={onReset}
      >
        <div>MAIN</div>
      </AppLayout>,
    );
    const handle = document.querySelector('[role="separator"]') as HTMLElement;
    expect(handle).toBeTruthy();
    return handle;
  }

  it("sidebarResizable 时渲染 separator 柄；未启用时不渲染", () => {
    const first = render(
      <AppLayout sidebar={<div>SB</div>} sidebarResizable onSidebarWidthChange={vi.fn()}>
        <div>MAIN</div>
      </AppLayout>,
    );
    expect(document.querySelector('[role="separator"]')).toBeTruthy();
    first.unmount();

    render(
      <AppLayout sidebar={<div>SB</div>}>
        <div>MAIN</div>
      </AppLayout>,
    );
    expect(document.querySelector('[role="separator"]')).toBeNull();
  });

  it("按下后移动鼠标：按 clientX-布局左缘 连续回报夹取宽度", () => {
    const onChange = vi.fn();
    const handle = mount(onChange, vi.fn());
    const left = handle.parentElement!.getBoundingClientRect().left;

    fireEvent.mouseDown(handle, { clientX: left + 300 });
    fireEvent.mouseMove(window, { clientX: left + 500 }); // 超上限 → 420
    fireEvent.mouseMove(window, { clientX: left + 100000 }); // 仍夹 420
    fireEvent.mouseMove(window, { clientX: left + 200 }); // 区间内 → 200
    fireEvent.mouseUp(window);

    expect(onChange).toHaveBeenLastCalledWith(200);
    const calls = onChange.mock.calls.map((c) => c[0]);
    expect(calls).toContain(420);
  });

  it("双击分隔柄触发重置回调", () => {
    const onReset = vi.fn();
    const handle = mount(vi.fn(), onReset);
    fireEvent.dblClick(handle);
    expect(onReset).toHaveBeenCalledTimes(1);
  });
});
