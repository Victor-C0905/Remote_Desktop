// src/apps/Terminal.test.tsx
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
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

// 组件读取的是 activeServer（activeServerId 由组件内部派生）
vi.mock('../context/ServerManager', () => ({
  useServerManager: () => ({
    activeServer: null,
  }),
}));

describe('TerminalApp', () => {
  it('renders terminal app', () => {
    render(<TerminalApp windowId="test-window-1" />);
    // 新建页签按钮：提示文字「新建页签」（ChromeTabStrip pinnedRight 插槽）
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
    // 未连接服务器时 tab label 为「本地演示」（activeServer?.name || '本地演示'）
    expect(screen.getByText('本地演示')).toBeInTheDocument();
  });

  it('renders terminal container', () => {
    const { container } = render(<TerminalApp windowId="test-window-4" />);
    expect(container.querySelector('.terminal-container')).toBeInTheDocument();
  });
});