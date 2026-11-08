import { describe, expect, it } from 'vitest';
import { computeWindow } from './useVirtual';

describe('computeWindow', () => {
  const H = 30;

  it('常规窗口：含上下 overscan', () => {
    // scrollTop=300（第 10 行起）、视口 300px（10 行）、overscan=2
    const w = computeWindow(300, 300, H, 1000, 2);
    expect(w.start).toBe(8);
    // 10 可见 + 2*2 overscan = 14
    expect(w.end).toBe(22);
    expect(w.padTop).toBe(8 * H);
    expect(w.padBottom).toBe((1000 - 22) * H);
  });

  it('顶部越界钳到 0', () => {
    const w = computeWindow(0, 300, H, 100, 5);
    expect(w.start).toBe(0);
    expect(w.padTop).toBe(0);
  });

  it('条目少于一屏：全量渲染，无占位', () => {
    const w = computeWindow(0, 600, H, 5, 3);
    expect(w.start).toBe(0);
    expect(w.end).toBe(5);
    expect(w.padTop).toBe(0);
    expect(w.padBottom).toBe(0);
  });

  it('滚动到底：end 钳到条目数', () => {
    const w = computeWindow(100 * H, 300, H, 100, 2);
    expect(w.start).toBeLessThanOrEqual(100);
    expect(w.end).toBe(100);
    expect(w.padBottom).toBe(0);
  });

  it('零条目 / 零行高：空窗口', () => {
    expect(computeWindow(0, 300, H, 0, 2)).toEqual({ start: 0, end: 0, padTop: 0, padBottom: 0 });
    expect(computeWindow(0, 300, 0, 10, 2)).toEqual({ start: 0, end: 0, padTop: 0, padBottom: 0 });
  });
});
