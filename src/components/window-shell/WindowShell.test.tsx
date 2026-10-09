// src/components/window-shell/WindowShell.test.tsx
// 验证 Aero Snap tear-off 时机:点击标题栏(不拖动)不还原,只有真正拖动才还原(对齐 Windows)

import { render, fireEvent, act } from '@testing-library/react';
import { describe, test, expect, vi, beforeEach } from 'vitest';
import { WindowShell } from './WindowShell';

// jsdom 不实现 requestAnimationFrame / cancelAnimationFrame
beforeEach(() => {
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => {
    cb(0);
    return 0;
  });
  vi.stubGlobal('cancelAnimationFrame', () => {});
});

function makeProps(overrides: Record<string, any> = {}) {
  return {
    windowId: 'w1',
    title: 'TestWin',
    isActive: true,
    position: { x: 0, y: 0 },
    size: { width: 960, height: 1080 },
    snapZone: 'left' as const,
    preSnapState: { position: { x: 100, y: 100 }, size: { width: 800, height: 600 } },
    mode: 'standard' as const,
    onClose: () => {},
    onMinimize: () => {},
    onMaximize: () => {},
    onSnap: () => {},
    onUnsnap: () => {},
    onFocus: () => {},
    onPositionChange: () => {},
    onSizeChange: () => {},
    children: <div>content</div>,
    ...overrides,
  };
}

describe('WindowShell - tear-off 时机(对齐 Windows:点击不还原,拖动才还原)', () => {
  test('snap 状态点击标题栏(不移动)不触发 onUnsnap', () => {
    const onUnsnap = vi.fn();
    const { getByText } = render(<WindowShell {...makeProps({ onUnsnap })} />);

    // mousedown 在标题文本(冒泡到 HeaderBar 的 onMouseDown)
    fireEvent.mouseDown(getByText('TestWin'));

    // 修复前:handleDragStart 在 mousedown 立即 tear-off → onUnsnap 被调用 → 失败
    // 修复后:只记录起点,不 tear-off → 通过
    expect(onUnsnap).not.toHaveBeenCalled();
  });

  test('snap 状态拖动超过阈值才触发 onUnsnap', () => {
    const onUnsnap = vi.fn();
    const { getByText } = render(<WindowShell {...makeProps({ onUnsnap })} />);

    // 1. mousedown:不触发 onUnsnap
    fireEvent.mouseDown(getByText('TestWin'));
    expect(onUnsnap).not.toHaveBeenCalled();

    // 2. mousemove 超过拖拽阈值(3px)→ 触发 tear-off → onUnsnap
    act(() => {
      fireEvent.mouseMove(document, { clientX: 500, clientY: 100 });
    });
    expect(onUnsnap).toHaveBeenCalled();
  });
});
