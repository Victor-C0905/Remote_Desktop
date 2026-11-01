import { describe, it, expect } from 'vitest';
import { computeWindowZ, WINDOW_Z_ACTIVE, WINDOW_Z_BASE, WINDOW_Z_BAND_TOP } from './zOrder';

/** 构造 n 个窗口的 z 序（rank 0..n-1，末位=激活） */
function zList(n: number): number[] {
  return Array.from({ length: n }, (_, rank) => computeWindowZ(rank, n));
}

describe('computeWindowZ', () => {
  it('单窗口：即激活窗口，层顶 z', () => {
    expect(computeWindowZ(0, 1)).toBe(WINDOW_Z_ACTIVE);
  });

  it('两窗口：非激活 20、激活 30', () => {
    expect(computeWindowZ(0, 2)).toBe(WINDOW_Z_BASE);
    expect(computeWindowZ(1, 2)).toBe(WINDOW_Z_ACTIVE);
  });

  it('≤10 窗口：非激活 z 严格保序，激活恒层顶', () => {
    for (const n of [3, 5, 10]) {
      const zs = zList(n);
      const inactive = zs.slice(0, n - 1);
      for (let k = 1; k < inactive.length; k++) {
        expect(inactive[k]).toBeGreaterThan(inactive[k - 1]);
      }
      expect(zs[n - 1]).toBe(WINDOW_Z_ACTIVE);
    }
  });

  it('>10 窗口：非激活单调不减（鸽笼并列由创建序兜底），有界', () => {
    const zs = zList(30);
    const inactive = zs.slice(0, 29);
    for (let k = 1; k < inactive.length; k++) {
      expect(inactive[k]).toBeGreaterThanOrEqual(inactive[k - 1]);
      expect(inactive[k]).toBeLessThanOrEqual(WINDOW_Z_BAND_TOP);
    }
    expect(zs[29]).toBe(WINDOW_Z_ACTIVE);
  });

  it('任何窗口数：不越 TopBar(50) 与浮层(60)', () => {
    for (const n of [2, 15, 40, 120]) {
      for (const z of zList(n)) {
        expect(z).toBeGreaterThanOrEqual(WINDOW_Z_BASE);
        expect(z).toBeLessThan(50);
      }
    }
  });
});
