// src/apps/Terminal.test.tsx
import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import { TerminalApp } from './Terminal';

// Mock dependencies
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

vi.mock('../context/ServerManager', () => ({
  useServerManager: () => ({
    activeServerId: null,
  }),
}));

describe('TerminalApp', () => {
  it('renders terminal app', () => {
    render(<TerminalApp windowId="test-window-1" />);
    expect(screen.getByRole('button', { name: '新建标签页' })).toBeInTheDocument();
  });

  it('renders header bar', () => {
    render(<TerminalApp windowId="test-window-2" />);
    expect(screen.getByRole('button', { name: '搜索' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '设置' })).toBeInTheDocument();
  });

  it('renders tab bar', () => {
    render(<TerminalApp windowId="test-window-3" />);
    expect(screen.getByText('终端 1')).toBeInTheDocument();
  });

  it('renders terminal container', () => {
    const { container } = render(<TerminalApp windowId="test-window-4" />);
    expect(container.querySelector('.terminal-container')).toBeInTheDocument();
  });
});