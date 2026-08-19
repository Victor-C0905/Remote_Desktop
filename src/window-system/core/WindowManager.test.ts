// src/window-system/core/WindowManager.test.ts

import { describe, it, expect, vi } from 'vitest';
import { WindowManager } from './WindowManager';
import { WindowRegistry } from './WindowRegistry';
import type { SnapZone } from '../../components/window-shell/aeroSnap';

// 测试用 registry:注册一个最小 app
function makeRegistry() {
  const registry = new WindowRegistry();
  registry.register({
    id: 'test-app',
    title: 'Test',
    icon: '',
    defaultSize: { width: 800, height: 600 },
    minSize: { width: 400, height: 300 },
    allowMultipleInstances: false,
    component: () => null,
  });
  return registry;
}

async function makeManagerWithWindow() {
  const manager = new WindowManager(makeRegistry());
  const win = await manager.create('test-app');
  return { manager, win };
}

describe('WindowManager - snap/unsnap', () => {
  it('snap 设置窗口 snapZone 与 position/size,触发 window:snapped 事件', async () => {
    const { manager, win } = await makeManagerWithWindow();
    const handler = vi.fn();
    manager.on('window:snapped', handler);

    const zone: SnapZone = 'left';
    const rect = { x: 0, y: 0, width: 960, height: 1080 };
    manager.snap(win.id, zone, rect);

    expect(win.snapZone).toBe('left');
    expect(win.position).toEqual({ x: 0, y: 0 });
    expect(win.size).toEqual({ width: 960, height: 1080 });
    expect(handler).toHaveBeenCalledTimes(1);
    expect(handler).toHaveBeenCalledWith(expect.objectContaining({
      type: 'window:snapped',
      windowId: win.id,
    }));
  });

  it('unsnap 还原窗口状态,触发 window:unsnapped 事件', async () => {
    const { manager, win } = await makeManagerWithWindow();
    const originalPos = { ...win.position };
    const originalSize = { ...win.size };

    manager.snap(win.id, 'left', { x: 0, y: 0, width: 960, height: 1080 });

    const handler = vi.fn();
    manager.on('window:unsnapped', handler);

    manager.unsnap(win.id);

    expect(win.snapZone).toBe(null);
    expect(win.position).toEqual(originalPos);
    expect(win.size).toEqual(originalSize);
    expect(handler).toHaveBeenCalledTimes(1);
    expect(handler).toHaveBeenCalledWith(expect.objectContaining({
      type: 'window:unsnapped',
      windowId: win.id,
    }));
  });

  it('snap 不存在的窗口 → no-op(不抛错)', async () => {
    const manager = new WindowManager(makeRegistry());
    expect(() => manager.snap('nonexistent', 'left', { x: 0, y: 0, width: 100, height: 100 })).not.toThrow();
  });

  it('unsnap 不存在的窗口 → no-op', async () => {
    const manager = new WindowManager(makeRegistry());
    expect(() => manager.unsnap('nonexistent')).not.toThrow();
  });
});
