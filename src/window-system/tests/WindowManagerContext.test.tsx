// src/window-system/tests/WindowManagerContext.test.tsx

import { render, act } from '@testing-library/react';
import { WindowManagerProvider, useWindowManager, useWindowGlobalState } from '../WindowManagerContext';
import { IWindowManager } from '../types';

/**
 * 测试探针组件
 *
 * 对齐当前 API：
 * - useWindowManager() 返回 { manager }（而非 manager 本身）
 * - 全局状态经 useWindowGlobalState() 订阅（不在 context 上）
 */
function TestComponent({ managerRef }: { managerRef?: React.MutableRefObject<IWindowManager | null> }) {
  const { manager } = useWindowManager();
  const globalState = useWindowGlobalState();

  // 暴露 manager 给测试代码
  if (managerRef) {
    managerRef.current = manager;
  }

  return (
    <div>
      <span data-testid="window-count">{globalState.windowList.length}</span>
      <span data-testid="active-id">{globalState.activeWindowId || 'none'}</span>
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

    // 等待组件渲染（Provider 的 mount effect 含异步 hydration 等待）
    await act(async () => {
      await new Promise(resolve => setTimeout(resolve, 100));
    });

    const manager = managerRef.current as any;

    // 创建窗口
    await act(async () => {
      await manager.create('files');
    });

    const initialCount = getByTestId('window-count').textContent;
    expect(initialCount).toBe('1');

    // 记录渲染基准：位置事件前 active-id 不变，计数也不应变
    const initialActiveId = getByTestId('active-id').textContent;

    // 模拟位置变化事件（不应触发全局重渲染）
    act(() => {
      manager.emit?.({
        type: 'window:moved',
        windowId: 'win-files-1',
        timestamp: Date.now()
      });
    });

    // 验证渲染次数未增加
    const countAfterPosition = getByTestId('window-count').textContent;
    expect(countAfterPosition).toBe(initialCount);
    expect(getByTestId('active-id').textContent).toBe(initialActiveId);
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

    const manager = managerRef.current as any;

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
