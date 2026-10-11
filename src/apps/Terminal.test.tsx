// src/apps/Terminal.test.tsx
import { render, screen } from '@testing-library/react';
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
    // 新建标签按钮：内容为 "+"，提示文字「新建标签」（对齐当前 UI）
    expect(screen.getByTitle('新建标签')).toBeInTheDocument();
  });

  it('renders header bar', () => {
    render(<TerminalApp windowId="test-window-2" />);
    // 工具栏为紧凑图标按钮（🔍/⚙️），以 title 悬浮提示作为可发现性契约
    expect(screen.getByTitle('在终端输出中搜索')).toBeInTheDocument();
    expect(screen.getByTitle('字号 / 光标闪烁设置')).toBeInTheDocument();
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