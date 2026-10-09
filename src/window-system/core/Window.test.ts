// src/window-system/core/Window.test.ts

import { describe, it, expect } from 'vitest';
import { Window } from './Window';
import type { SnapZone } from '../../components/window-shell/aeroSnap';

// 工厂:创建一个默认窗口实例
function makeWindow(overrides?: { position?: { x: number; y: number }; size?: { width: number; height: number } }) {
  return new Window(
    'win-test-1',
    'test-app',
    overrides?.position ?? { x: 100, y: 100 },
    overrides?.size ?? { width: 800, height: 600 },
  );
}

describe('Window - snap 状态', () => {
  it('初始 snapZone === null,preSnapState === null', () => {
    const w = makeWindow();
    expect(w.snapZone).toBe(null);
    expect(w.preSnapState).toBe(null);
  });

  it('setSnap 保存当前位置/尺寸到 preSnapState,应用目标 rect', () => {
    const w = makeWindow({ position: { x: 100, y: 100 }, size: { width: 800, height: 600 } });
    const rect = { x: 0, y: 0, width: 960, height: 1080 };

    w.setSnap('left' as SnapZone, rect);

    expect(w.snapZone).toBe('left');
    expect(w.position).toEqual({ x: 0, y: 0 });
    expect(w.size).toEqual({ width: 960, height: 1080 });
    // preSnapState 保存了 snap 前的状态
    expect(w.preSnapState).toEqual({
      position: { x: 100, y: 100 },
      size: { width: 800, height: 600 },
    });
  });

  it('已 snap 状态再 setSnap 到新 zone:preSnapState 不被覆盖,只换 rect', () => {
    const w = makeWindow({ position: { x: 100, y: 100 }, size: { width: 800, height: 600 } });

    w.setSnap('left' as SnapZone, { x: 0, y: 0, width: 960, height: 1080 });
    const firstSnapState = w.preSnapState;
    expect(firstSnapState).not.toBe(null);

    // 再 snap 到 right
    w.setSnap('right' as SnapZone, { x: 960, y: 0, width: 960, height: 1080 });

    expect(w.snapZone).toBe('right');
    expect(w.position).toEqual({ x: 960, y: 0 });
    expect(w.size).toEqual({ width: 960, height: 1080 });
    // preSnapState 应保持首次 snap 前的值
    expect(w.preSnapState).toBe(firstSnapState);
    expect(w.preSnapState).toEqual({
      position: { x: 100, y: 100 },
      size: { width: 800, height: 600 },
    });
  });

  it('unsetSnap 恢复 preSnapState 的位置/尺寸,清空 snap 状态', () => {
    const w = makeWindow({ position: { x: 100, y: 100 }, size: { width: 800, height: 600 } });
    w.setSnap('left' as SnapZone, { x: 0, y: 0, width: 960, height: 1080 });

    w.unsetSnap();

    expect(w.snapZone).toBe(null);
    expect(w.preSnapState).toBe(null);
    expect(w.position).toEqual({ x: 100, y: 100 });
    expect(w.size).toEqual({ width: 800, height: 600 });
  });

  it('unsetSnap 在从未 snap 时调用是 no-op', () => {
    const w = makeWindow({ position: { x: 100, y: 100 }, size: { width: 800, height: 600 } });
    expect(() => w.unsetSnap()).not.toThrow();
    expect(w.position).toEqual({ x: 100, y: 100 });
    expect(w.size).toEqual({ width: 800, height: 600 });
  });

  it('maximize 不污染 snap 状态(互斥)', () => {
    const w = makeWindow();
    // 先 maximize
    w.setMaximized(true);
    w.setPosition({ x: 0, y: 0 });
    w.setSize({ width: 1920, height: 1080 });
    expect(w.maximized).toBe(true);
    expect(w.snapZone).toBe(null);
    expect(w.preSnapState).toBe(null);
  });

  it('snap 状态下 setMaximized(true):清除 snap,preMaximizeState 保存 snap 前原始尺寸(非 snap rect)', () => {
    const w = makeWindow({ position: { x: 100, y: 100 }, size: { width: 800, height: 600 } });
    // 先 snap 到左半屏
    w.setSnap('left' as SnapZone, { x: 0, y: 0, width: 960, height: 1080 });

    // snap 状态下 maximize:应清除 snap,preMaximizeState 应保存 snap 前原始尺寸(800x600)
    w.setMaximized(true);

    expect(w.maximized).toBe(true);
    expect(w.snapZone).toBe(null);
    expect(w.preSnapState).toBe(null);
    // ✅ 关键:preMaximizeState 保存的是 snap 前的原始尺寸,而非 snap rect(960x1080)
    expect(w.preMaximizeState).toEqual({
      position: { x: 100, y: 100 },
      size: { width: 800, height: 600 },
    });
  });

  it('maximize 状态下 setSnap:清除 maximize,preSnapState 保存 maximize 前原始尺寸(非最大化尺寸)', () => {
    const w = makeWindow({ position: { x: 100, y: 100 }, size: { width: 800, height: 600 } });
    // 先 maximize(保存原始 800x600 作为 preMaximizeState)
    w.setMaximized(true);
    w.setPosition({ x: 0, y: 0 });
    w.setSize({ width: 1920, height: 1080 });

    // maximize 状态下 snap:应清除 maximize,preSnapState 应保存 maximize 前原始尺寸(800x600)
    w.setSnap('left' as SnapZone, { x: 0, y: 0, width: 960, height: 1080 });

    expect(w.snapZone).toBe('left');
    expect(w.maximized).toBe(false);
    expect(w.preMaximizeState).toBe(null);
    // ✅ 关键:preSnapState 保存的是 maximize 前的原始尺寸,而非最大化尺寸(1920x1080)
    expect(w.preSnapState).toEqual({
      position: { x: 100, y: 100 },
      size: { width: 800, height: 600 },
    });
  });

  it('deserialize 后 snap 状态为默认值(运行时状态不持久化)', () => {
    const w = makeWindow();
    w.setSnap('left' as SnapZone, { x: 0, y: 0, width: 960, height: 1080 });
    const serialized = w.serialize();

    const restored = Window.deserialize(serialized);
    expect(restored.snapZone).toBe(null);
    expect(restored.preSnapState).toBe(null);
  });
});
