import { describe, it, expect } from 'vitest';
import { resolveDockClickAction } from './dockAction';

describe('resolveDockClickAction', () => {
  it('无窗口 → 创建', () => {
    expect(
      resolveDockClickAction({ hasWindow: false, minimized: false, isActive: false, withinCreationGrace: false }),
    ).toBe('create');
  });

  it('已最小化 → 恢复（三态循环，不受宽限期影响）', () => {
    expect(
      resolveDockClickAction({ hasWindow: true, minimized: true, isActive: false, withinCreationGrace: false }),
    ).toBe('restore');
    expect(
      resolveDockClickAction({ hasWindow: true, minimized: true, isActive: true, withinCreationGrace: true }),
    ).toBe('restore');
  });

  it('已打开未聚焦 → 聚焦', () => {
    expect(
      resolveDockClickAction({ hasWindow: true, minimized: false, isActive: false, withinCreationGrace: false }),
    ).toBe('focus');
  });

  it('已聚焦 → 最小化（三态循环保留）', () => {
    expect(
      resolveDockClickAction({ hasWindow: true, minimized: false, isActive: true, withinCreationGrace: false }),
    ).toBe('minimize');
  });

  it('R6-?-02 回归：创建宽限期内已聚焦 → 聚焦而非最小化（双击第二下不再藏掉首窗）', () => {
    expect(
      resolveDockClickAction({ hasWindow: true, minimized: false, isActive: true, withinCreationGrace: true }),
    ).toBe('focus');
  });
});
