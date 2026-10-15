/**
 * TextEditor 多页签行为单测
 *
 * 页签条由 chrome-tabs 库渲染（portal 进标题栏槽位）。
 * jsdom 无 WindowShell，测试用假槽位元素承接 portal；
 * 库激活依赖 draggabilly pointerDown，切换用例直接派发
 * 库的 activeTabChange 事件验证桥接 onActivate 链路。
 */

import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { TextEditor } from './TextEditor';

const closeMock = vi.fn();
const createMock = vi.fn();

vi.mock('../../window-system/WindowManagerContext', () => ({
  useWindowManager: () => ({
    manager: { close: closeMock, create: createMock },
  }),
}));

vi.mock('../../context/ServerManager', () => ({
  useServerManager: () => ({
    activeServerId: 's1',
    activeServer: { id: 's1', name: 'dev-server' },
  }),
}));

/** 渲染前创建假标题栏槽位（模拟 WindowShell 提供的 portal 目标），返回清理函数 */
function mountWithSlot(ui: React.ReactElement, windowId: string) {
  const slot = document.createElement('div');
  slot.setAttribute('data-titlebar-slot', windowId);
  document.body.appendChild(slot);
  const result = render(ui);
  return {
    ...result,
    unmount: () => {
      result.unmount();
      slot.remove();
    },
  };
}

function tabCount(): number {
  return document.querySelectorAll('.chrome-tab').length;
}

function activeTabIndex(): number {
  const tabs = [...document.querySelectorAll('.chrome-tab')];
  return tabs.findIndex((t) => t.hasAttribute('active'));
}

describe('TextEditor 多页签', () => {
  it('初始渲染一个未命名页签与菜单栏', () => {
    const { unmount } = mountWithSlot(<TextEditor windowId="w1" />, 'w1');
    expect(screen.getByRole('menubar')).toBeInTheDocument();
    expect(tabCount()).toBe(1);
    expect(document.querySelector('.chrome-tab .chrome-tab-title')?.textContent).toBe('未命名文件');
    expect(closeMock).not.toHaveBeenCalled();
    unmount();
  });

  it('[+] 在窗口内原地新增页签（不调用 manager.create）', () => {
    const { unmount } = mountWithSlot(<TextEditor windowId="w2" />, 'w2');
    fireEvent.click(screen.getByTitle('新建页签'));
    expect(tabCount()).toBe(2);
    expect(activeTabIndex()).toBe(1); // 新页签自动激活
    expect(createMock).not.toHaveBeenCalled(); // 不开新窗口
    unmount();
  });

  it('库激活事件驱动页签切换', () => {
    const { unmount } = mountWithSlot(<TextEditor windowId="w3" />, 'w3');
    fireEvent.click(screen.getByTitle('新建页签'));
    const tabs = [...document.querySelectorAll('.chrome-tab')];
    // 库通过 draggabilly 在拖拽把手上激活页签（pointerdown，降级 mousedown）
    const handle = tabs[0].querySelector('.chrome-tab-drag-handle');
    fireEvent.pointerDown(handle as Element);
    if (activeTabIndex() !== 0) fireEvent.mouseDown(handle as Element);
    expect(activeTabIndex()).toBe(0);
    unmount();
  });

  it('干净页签直接关闭；最后一个页签关闭时关闭窗口', () => {
    const { unmount } = mountWithSlot(<TextEditor windowId="w4" />, 'w4');
    // 唯一页签未编辑（干净）→ × 直接关闭窗口
    const close = document.querySelector('.chrome-tab-close');
    expect(close).not.toBeNull();
    fireEvent.click(close as Element);
    expect(closeMock).toHaveBeenCalledTimes(1);
    unmount();
  });

  it('预加载文件页签加载失败时状态降级但不崩溃', async () => {
    const { unmount } = mountWithSlot(
      <TextEditor windowId="w5" preloadData={{ path: '/home/user/a.txt', serverId: 's1' }} />,
      'w5',
    );
    // invoke mock 返回 undefined → openFile 失败 → 错误内联横幅出现（异步上报），页签仍在
    expect(tabCount()).toBe(1);
    expect(await screen.findByRole('alert')).toBeInTheDocument();
    unmount();
  });
});
