// src/apps/Terminal.test.tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { TerminalApp } from './Terminal';

// Mock dependencies
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(() => Promise.resolve(undefined)),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

// Terminal 内部经 useWindowEvent 订阅窗口事件（依赖 WindowManagerContext）；
// 渲染冒烟测试无需真窗口系统，mock 为空实现
vi.mock('../window-system/hooks/useWindowEvent', () => ({
  useWindowEvent: () => {},
}));

// 最后一个页签关闭 → 关窗语义：mock manager.close 验证调用
const closeMock = vi.fn();
vi.mock('../window-system/WindowManagerContext', () => ({
  useWindowManager: () => ({
    manager: { close: closeMock },
  }),
}));

// 组件读取的是 activeServer（activeServerId 由组件内部派生）
vi.mock('../context/ServerManager', () => ({
  useServerManager: () => ({
    activeServer: null,
  }),
}));

describe('TerminalApp', () => {
  beforeEach(() => {
    closeMock.mockClear();
  });

  it('renders terminal app', () => {
    render(<TerminalApp windowId="test-window-1" />);
    // 新建页签按钮：提示文字「新建页签」（ChromeTabStrip 行级附件位）
    expect(screen.getByTitle('新建页签')).toBeInTheDocument();
  });

  it('renders header bar', () => {
    render(<TerminalApp windowId="test-window-2" />);
    // 搜索/设置收纳进 ⌄ 更多操作菜单（Windows Terminal 式低频收纳）
    fireEvent.click(screen.getByTitle('更多操作'));
    expect(screen.getByRole('menuitemcheckbox', { name: '搜索' })).toBeInTheDocument();
    expect(screen.getByRole('menuitemcheckbox', { name: '设置' })).toBeInTheDocument();
  });

  it('renders tab bar', () => {
    render(<TerminalApp windowId="test-window-3" />);
    // 页签标题不显示服务器名，用自增序号区分会话；首个 tab 为「终端 1」
    expect(screen.getByText('终端 1')).toBeInTheDocument();
  });

  it('renders terminal container', () => {
    const { container } = render(<TerminalApp windowId="test-window-4" />);
    expect(container.querySelector('.terminal-container')).toBeInTheDocument();
  });

  it('最后一个页签关闭时关闭窗口（与 TextEditor 同语义）', () => {
    render(<TerminalApp windowId="test-window-5" />);
    const close = document.querySelector('.chrome-tab-close');
    expect(close).not.toBeNull();
    fireEvent.click(close as Element);
    expect(closeMock).toHaveBeenCalledTimes(1);
  });

  it('多页签关闭其中一个不关窗，页签数减一', () => {
    render(<TerminalApp windowId="test-window-6" />);
    fireEvent.click(screen.getByTitle('新建页签'));
    expect(document.querySelectorAll('.chrome-tab').length).toBe(2);
    const close = document.querySelector('.chrome-tab-close');
    fireEvent.click(close as Element);
    expect(closeMock).not.toHaveBeenCalled();
    expect(document.querySelectorAll('.chrome-tab').length).toBe(1);
  });
});