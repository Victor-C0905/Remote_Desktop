// src/window-system/tests/WindowManagerContext.test.tsx

import { render, screen, act, fireEvent } from '@testing-library/react';
import { useRef } from 'react';
import { WindowManagerProvider, useWindowManager } from '../WindowManagerContext';
import { IWindowManager } from '../types';

function TestComponent({ managerRef }: { managerRef?: React.MutableRefObject<IWindowManager | null> }) {
  const context = useWindowManager();
  const { globalState } = context as any;

  // 暴露 manager 给测试代码
  if (managerRef) {
    managerRef.current = context;
  }

  return (
    <div>
      <span data-testid="window-count">{globalState?.windowList?.length || 0}</span>
      <span data-testid="active-id">{globalState?.activeWindowId || 'none'}</span>
    </div>
  );
}

describe('WindowManagerContext - 事件监听优化', () => {
  test('位置变化事件不触发全局重渲染', async () => {
    const managerRef = { current: null as IWindowManager | null };

    const { getByTestId } = render(
      <WindowManagerProvider>
        <TestComponent managerRef={managerRef} />
      </WindowManagerProvider>
    );

    // 等待组件渲染
    await act(async () => {
      await new Promise(resolve => setTimeout(resolve, 100));
    });

    const manager = managerRef.current!.manager;

    // 创建窗口
    await act(async () => {
      await manager.create('files');
    });

    const initialCount = getByTestId('window-count').textContent;
    expect(initialCount).toBe('1');

    // 模拟位置变化事件（不应触发重渲染）
    act(() => {
      manager.emit?.({
        type: 'window:position_changed',
        windowId: 'win-files-1',
        timestamp: Date.now()
      });
    });

    // 验证渲染次数未增加
    const countAfterPosition = getByTestId('window-count').textContent;
    expect(countAfterPosition).toBe(initialCount);
  });

  test('关键事件触发全局重渲染', async () => {
    const managerRef = { current: null as IWindowManager | null };

    const { getByTestId } = render(
      <WindowManagerProvider>
        <TestComponent managerRef={managerRef} />
      </WindowManagerProvider>
    );

    // 等待组件渲染
    await act(async () => {
      await new Promise(resolve => setTimeout(resolve, 100));
    });

    const manager = managerRef.current!.manager;

    // 创建窗口（关键事件）
    let createdWindow: any;
    await act(async () => {
      createdWindow = await manager.create('files');
    });

    expect(getByTestId('window-count').textContent).toBe('1');

    // 激活窗口（关键事件）
    act(() => {
      if (createdWindow && createdWindow.id) {
        manager.focus?.(createdWindow.id);
      }
    });

    expect(getByTestId('active-id').textContent).toBe(createdWindow?.id || 'none');
  });
});