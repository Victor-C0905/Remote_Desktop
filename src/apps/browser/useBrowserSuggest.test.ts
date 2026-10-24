// src/apps/browser/useBrowserSuggest.test.ts
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { useBrowserSuggest, looksLikeUrl } from "./useBrowserSuggest";

// 全局 setup 已 mock @tauri-apps/api/core，此处取受管 mock 控制返回值
const invokeMock = vi.mocked(invoke);

beforeEach(() => {
  vi.useFakeTimers();
  invokeMock.mockReset();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("looksLikeUrl", () => {
  it.each([
    ["https://docs.rs", true],
    ["http://a.b/c?d=1", true],
    ["docs.rs", true],
    ["localhost:3000", true],
    ["localhost:3000/search", true],
    // 内部 scheme：浏览器内部地址永不作为搜索词联想（实测教训：about:blank）
    ["about:blank", true],
    ["about:config", true],
    ["javascript:void(0)", true],
    ["data:text/plain,x", true],
    ["view-source:https://a.com", true],
    ["rust async book", false], // 含空白 → 搜索词
    ["rustlang", false], // 无点 → 搜索词
    ["铁锈 战争", false],
    ["", false],
  ])("%s → %s", (input, expected) => {
    expect(looksLikeUrl(input)).toBe(expected);
  });
});

describe("useBrowserSuggest", () => {
  it("防抖后才拉取，结果落地并展开", async () => {
    invokeMock.mockResolvedValue(["rust book", "rust lang"]);
    const { result } = renderHook(() => useBrowserSuggest("s1", "google", "rust", true));
    // 防抖窗口内不发请求（防抖 40ms：30ms 未到窗口）
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30);
    });
    expect(invokeMock).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(invokeMock).toHaveBeenCalledWith("browser_suggest", {
      serverId: "s1",
      engine: "google",
      query: "rust",
    });
    // 第 0 项 = 本地模板项（当前输入词），网络结果随后追加
    expect(result.current.suggestions).toEqual(["rust", "rust book", "rust lang"]);
    expect(result.current.open).toBe(true);
  });

  it("快速输入仅最新请求落地（竞态序号）", async () => {
    const resolvers: Array<(v: string[]) => void> = [];
    invokeMock.mockImplementation(
      () => new Promise<string[]>((res) => resolvers.push(res)),
    );
    const { result, rerender } = renderHook(
      (p: { draft: string }) => useBrowserSuggest("s1", "google", p.draft, true),
      { initialProps: { draft: "ru" } },
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    expect(invokeMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      rerender({ draft: "rust" });
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    expect(invokeMock).toHaveBeenCalledTimes(2);
    // 先回来的旧请求不得落地（表现为仅剩模板项，等待最新请求）
    await act(async () => {
      resolvers[0](["stale"]);
    });
    expect(result.current.suggestions).toEqual(["rust"]);
    // 最新请求落地（模板项 + 网络结果）
    await act(async () => {
      resolvers[1](["fresh"]);
    });
    expect(result.current.suggestions).toEqual(["rust", "fresh"]);
  });

  it("Esc 收起后同输入不弹回，输入变化恢复", async () => {
    invokeMock.mockResolvedValue(["a"]);
    const { result, rerender } = renderHook(
      (p: { draft: string }) => useBrowserSuggest("s1", "google", p.draft, true),
      { initialProps: { draft: "rust" } },
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    expect(result.current.open).toBe(true);
    act(() => {
      result.current.close();
    });
    expect(result.current.open).toBe(false);
    // draft 不变：无新请求、保持收起
    await act(async () => {
      await vi.advanceTimersByTimeAsync(600);
    });
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(result.current.open).toBe(false);
    // 输入变化：重置收起标记并重新拉取
    await act(async () => {
      rerender({ draft: "rust a" });
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    expect(invokeMock).toHaveBeenCalledTimes(2);
    expect(result.current.open).toBe(true);
  });

  it("URL 形输入不拉联想", async () => {
    const { result } = renderHook(() => useBrowserSuggest("s1", "google", "docs.rs", true));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(600);
    });
    expect(invokeMock).not.toHaveBeenCalled();
    expect(result.current.open).toBe(false);
  });

  it("moveActive 环形移动", async () => {
    invokeMock.mockResolvedValue(["a", "b", "c"]);
    const { result } = renderHook(() => useBrowserSuggest("s1", "google", "rust", true));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    // 初始未选中，↓ 到首项（列表 = [模板项, a, b, c]）
    act(() => {
      result.current.moveActive(1);
    });
    expect(result.current.activeIndex).toBe(0);
    act(() => {
      result.current.moveActive(-1);
    });
    // 首项↑ 环绕到末项
    expect(result.current.activeIndex).toBe(3);
  });
});
