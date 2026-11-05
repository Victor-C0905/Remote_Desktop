import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { RemoteDirectoryPicker } from './RemoteDirectoryPicker';

const invokeMock = vi.mocked(invoke);

describe('RemoteDirectoryPicker 路径不存在处理', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('读取失败时提供“创建此目录”，创建成功后进入', async () => {
    const existing = new Set(['/']);
    invokeMock.mockImplementation(async (cmd, rawArgs) => {
      const path = (rawArgs as { path?: string } | undefined)?.path;
      if (cmd === 'remote_read_dir') {
        if (path && existing.has(path)) return { path, entries: [] };
        throw new Error('目录不存在');
      }
      if (cmd === 'remote_mkdir') {
        if (path) existing.add(path);
        return undefined;
      }
      return undefined;
    });

    render(
      <RemoteDirectoryPicker
        serverId="s1"
        initialPath="/newdir"
        onSelect={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    await waitFor(() => expect(screen.getByText('创建此目录')).toBeInTheDocument());
    fireEvent.click(screen.getByText('创建此目录'));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('remote_mkdir', { serverId: 's1', path: '/newdir' });
    });
    await waitFor(() => expect(screen.queryByText('创建此目录')).not.toBeInTheDocument());
    expect(screen.getByText('该目录下没有子文件夹')).toBeInTheDocument();
  });

  it('输入已存在路径后直接点“选择此目录”，按输入值选中', async () => {
    const existing = new Set(['/', '/data']);
    invokeMock.mockImplementation(async (cmd, rawArgs) => {
      const path = (rawArgs as { path?: string } | undefined)?.path;
      if (cmd === 'remote_read_dir') {
        if (path && existing.has(path)) return { path, entries: [] };
        throw new Error('目录不存在');
      }
      return undefined;
    });
    const onSelect = vi.fn();
    render(
      <RemoteDirectoryPicker serverId="s1" initialPath="/" onSelect={onSelect} onCancel={vi.fn()} />,
    );
    // 等初始目录加载完成，避免点击时按钮仍处于 loading 禁用态
    await waitFor(() => expect(screen.getByRole('button', { name: '选择此目录' })).toBeEnabled());

    fireEvent.change(screen.getByLabelText('输入目录路径'), { target: { value: '/data' } });
    fireEvent.click(screen.getByRole('button', { name: '选择此目录' }));
    await waitFor(() => expect(onSelect).toHaveBeenCalledWith('/data'));
  });

  it('输入不存在路径点“选择此目录” → 创建并选择，不再静默回退', async () => {
    const existing = new Set(['/']);
    invokeMock.mockImplementation(async (cmd, rawArgs) => {
      const path = (rawArgs as { path?: string } | undefined)?.path;
      if (cmd === 'remote_read_dir') {
        if (path && existing.has(path)) return { path, entries: [] };
        throw new Error('目录不存在');
      }
      if (cmd === 'remote_mkdir') {
        if (path) existing.add(path);
        return undefined;
      }
      return undefined;
    });
    const onSelect = vi.fn();
    render(
      <RemoteDirectoryPicker serverId="s1" initialPath="/" onSelect={onSelect} onCancel={vi.fn()} />,
    );
    await waitFor(() => expect(screen.getByRole('button', { name: '选择此目录' })).toBeEnabled());

    fireEvent.change(screen.getByLabelText('输入目录路径'), { target: { value: '/newfolder' } });
    fireEvent.click(screen.getByRole('button', { name: '选择此目录' }));
    await waitFor(() => expect(screen.getByText('创建并选择')).toBeInTheDocument());
    fireEvent.click(screen.getByText('创建并选择'));
    await waitFor(() => expect(onSelect).toHaveBeenCalledWith('/newfolder'));
  });
});
