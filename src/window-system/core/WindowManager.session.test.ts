// src/window-system/core/WindowManager.session.test.ts
// 窗口会话快照（sessionStorage）往返测试：刷新恢复机制的核心契约。
// 覆盖：v2 往返（几何/preloadData/焦点顺序）、v1 裸数组兼容、关闭窗口
// 不复活、空快照（新启动）不恢复。

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { WindowManager } from './WindowManager';
import { WindowRegistry } from './WindowRegistry';
import type { Window as AppWindow } from './Window';

function makeRegistry() {
  const registry = new WindowRegistry();
  for (const id of ['files', 'editor']) {
    registry.register({
      id,
      titleKey: 'apps.terminal.title',
      defaultSize: { width: 800, height: 600 },
      minSize: { width: 400, height: 300 },
      allowMultipleInstances: true,
      component: () => null,
    });
  }
  return registry;
}

function makeSoloRegistry() {
  const registry = new WindowRegistry();
  registry.register({
    id: 'solo',
    titleKey: 'apps.browser.title',
    defaultSize: { width: 800, height: 600 },
    minSize: { width: 400, height: 300 },
    allowMultipleInstances: false,
    component: () => null,
  });
  return registry;
}

/** 推进防抖计时器，落盘快照 */
function flushSave(manager: WindowManager) {
  manager.saveSession();
  vi.advanceTimersByTime(300);
}

beforeEach(() => {
  vi.useFakeTimers();
  sessionStorage.clear();
});

afterEach(() => {
  vi.useRealTimers();
  sessionStorage.clear();
});

describe('WindowManager 会话快照', () => {
  it('单实例应用的重复窗口在恢复时去重（历史快照可能含重复）', async () => {
    const m1 = new WindowManager(makeSoloRegistry());
    await m1.create('solo');
    flushSave(m1);
    // 手工注入重复窗口：模拟历史缺陷产生的重复快照条目
    const raw = JSON.parse(sessionStorage.getItem('wm.session') ?? '{}');
    raw.windows.push({ ...raw.windows[0], id: 'w-dup' });
    raw.order = [...raw.order, 'w-dup'];
    sessionStorage.setItem('wm.session', JSON.stringify(raw));

    const m2 = new WindowManager(makeSoloRegistry());
    const restored = m2.loadSession();
    // 恢复必须遵守 allowMultipleInstances=false：否则两个实例互抢同一会话
    // （browser 的收养/关闭链路被对端重建打断）
    expect(restored).toHaveLength(1);
    expect(m2.getByAppId('solo')).toHaveLength(1);
  });
  it('v2 往返：几何/preloadData/焦点顺序完整恢复', async () => {
    const m1 = new WindowManager(makeRegistry());
    const w1 = await m1.create('files');
    w1.setPreloadData({ path: '/home/u/proj' });
    const w2 = await m1.create('editor');
    w2.setPreloadData({ path: '/home/u/a.txt', serverId: 'srv1' });
    m1.focus(w2.id); // 用户最后在用 editor
    flushSave(m1);

    const raw = JSON.parse(sessionStorage.getItem('wm.session') ?? '{}');
    expect(raw.windows).toHaveLength(2);
    expect(raw.order).toEqual([w1.id, w2.id]);

    // 「刷新」：全新 manager 实例（模拟 webview 重载后的干净状态）
    const m2 = new WindowManager(makeRegistry());
    const restored = m2.loadSession();
    expect(restored).toHaveLength(2);
    expect(m2.getAll()).toHaveLength(2);

    const r1 = m2.getAll().find((w) => w.id === w1.id)!;
    const r2 = m2.getAll().find((w) => w.id === w2.id)!;
    expect(r1.position).toEqual(w1.position);
    expect(r1.size).toEqual(w1.size);
    expect(r1.preloadData).toEqual({ path: '/home/u/proj' });
    expect(r1.preloadState).toBe('ready');
    expect(r2.preloadData).toEqual({ path: '/home/u/a.txt', serverId: 'srv1' });
    // 焦点顺序末位 = 快照时的激活窗口
    expect(m2.getActive()?.id).toBe(w2.id);
  });

  it('v1 裸数组快照兼容（旧格式无损恢复）', () => {
    sessionStorage.setItem(
      'wm.session',
      JSON.stringify([
        {
          id: 'win-1',
          appId: 'files',
          position: { x: 10, y: 20 },
          size: { width: 800, height: 600 },
          minimized: false,
          layout: 'free',
          preloadData: null,
        },
      ]),
    );
    const m = new WindowManager(makeRegistry());
    expect(m.loadSession()).toEqual(['win-1']);
    const win = m.getAll().find((w) => w.id === 'win-1')!;
    expect(win.position).toEqual({ x: 10, y: 20 });
    expect(win.preloadState).toBe('loading'); // v1 无 preloadData
  });

  it('关闭的窗口不随快照复活', async () => {
    const m1 = new WindowManager(makeRegistry());
    const w1 = await m1.create('files');
    const w2 = await m1.create('editor');
    m1.close(w2.id);
    flushSave(m1);

    const m2 = new WindowManager(makeRegistry());
    m2.loadSession();
    expect(m2.getAll().map((w) => w.id)).toEqual([w1.id]);
  });

  it('无快照（关闭项目后新启动）保持干净桌面', () => {
    const m = new WindowManager(makeRegistry());
    expect(m.loadSession()).toEqual([]);
    expect(m.getAll()).toHaveLength(0);
  });

  it('快照损坏：清除并不抛错', () => {
    sessionStorage.setItem('wm.session', '{not-json');
    const m = new WindowManager(makeRegistry());
    expect(m.loadSession()).toEqual([]);
    expect(sessionStorage.getItem('wm.session')).toBeNull();
  });
});

describe('WindowManager 会话原子性契约', () => {
  function makeRegistryWithLifecycle(onAfterClose: (window: AppWindow) => void) {
    const registry = new WindowRegistry();
    registry.register({
      id: 'browser',
      titleKey: 'apps.browser.title',
      defaultSize: { width: 1000, height: 700 },
      minSize: { width: 640, height: 440 },
      allowMultipleInstances: false,
      component: () => null,
      lifecycle: { onAfterClose: onAfterClose },
    });
    return registry;
  }

  it('close 后 onAfterClose 收到窗口对象（preloadData.serverId 可取，销毁单点契约）', async () => {
    const onAfterClose = vi.fn((_window: AppWindow) => {});
    const m = new WindowManager(makeRegistryWithLifecycle(onAfterClose));
    const win = await m.create('browser');
    win.setPreloadData({ serverId: 'srv-1' });

    m.close(win.id);

    expect(m.getAll()).toHaveLength(0); // 已从窗口表移除
    expect(onAfterClose).toHaveBeenCalledTimes(1);
    // 闭包收到的 window 是销毁前的对象：serverId 仍可读（会话全量停止的依据）
    expect((onAfterClose.mock.calls[0][0] as { preloadData: { serverId: string } }).preloadData.serverId).toBe('srv-1');
  });

  it('页面刷新语义：不调用 close 就不触发 onAfterClose（会话零感知）', async () => {
    const onAfterClose = vi.fn();
    const m = new WindowManager(makeRegistryWithLifecycle(onAfterClose));
    const win = await m.create('browser');
    win.setPreloadData({ serverId: 'srv-1' });
    flushSave(m);

    // 「刷新」= 新 manager 实例读同一 sessionStorage 快照（无 close 产生）
    const m2 = new WindowManager(makeRegistryWithLifecycle(onAfterClose));
    m2.loadSession();

    expect(onAfterClose).not.toHaveBeenCalled(); // 会话零感知
    expect(m2.getAll()).toHaveLength(1);          // 窗口被新页面收养
    expect(sessionStorage.getItem('wm.session')).not.toBeNull();
  });
});
