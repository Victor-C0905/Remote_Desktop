import { invoke } from '@tauri-apps/api/core';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ensureRemoteDirectory } from './remoteDirectory';

const invokeMock = vi.mocked(invoke);

function mockDirs(existing: string[]) {
  const dirs = new Set(existing);
  invokeMock.mockImplementation(async (cmd, rawArgs) => {
    const path = (rawArgs as { path?: string } | undefined)?.path;
    if (cmd === 'remote_read_dir') {
      if (path && dirs.has(path)) return { path, entries: [] };
      throw new Error('目录不存在');
    }
    if (cmd === 'remote_mkdir') {
      if (path) dirs.add(path);
      return undefined;
    }
    return undefined;
  });
  return dirs;
}

describe('ensureRemoteDirectory 防误建目录树', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('目录已存在 → exists，不调用 mkdir', async () => {
    mockDirs(['/', '/data']);
    await expect(ensureRemoteDirectory('s1', '/data')).resolves.toBe('exists');
    expect(invokeMock).not.toHaveBeenCalledWith('remote_mkdir', expect.anything());
  });

  it('目标缺失但父目录存在 → 只创建末级目录', async () => {
    const dirs = mockDirs(['/', '/data']);
    await expect(ensureRemoteDirectory('s1', '/data/out')).resolves.toBe('created');
    expect(invokeMock).toHaveBeenCalledWith('remote_mkdir', { serverId: 's1', path: '/data/out' });
    expect(dirs.has('/data/out')).toBe(true);
  });

  it('父目录缺失 → parent-missing，拒绝创建整条错误路径', async () => {
    mockDirs(['/']);
    await expect(ensureRemoteDirectory('s1', '/dataa/out')).resolves.toBe('parent-missing');
    expect(invokeMock).not.toHaveBeenCalledWith('remote_mkdir', expect.anything());
  });
});
