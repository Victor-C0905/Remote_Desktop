import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// mock Tauri invoke（openRemoteFile 依赖 detectFileFormat/remote_open_locally 内部的 invoke）
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import { invoke } from '@tauri-apps/api/core';
import { openRemoteFile } from '../openRemoteFile';
import type { RemoteFileInfo } from '../../file-formats/types';

/** 构造 file_info 探测结果（detectFileFormat 的返回结构，camelCase 对齐 Rust 侧） */
function fileInfo(overrides: Partial<RemoteFileInfo> = {}): RemoteFileInfo {
  return {
    path: '/x/a',
    size: 100,
    isDir: false,
    isText: true,
    extension: 'txt',
    magicBytes: [],
    ...overrides,
  };
}

describe('openRemoteFile 路由分发', () => {
  let created: { appId: string; opts: { serverId?: string; preloadData?: unknown } }[];
  const manager = {
    create: (appId: string, opts: { serverId?: string; preloadData?: unknown }) => {
      created.push({ appId, opts });
    },
  };

  beforeEach(() => {
    created = [];
    vi.mocked(invoke).mockReset();
    vi.spyOn(window, 'confirm').mockReturnValue(true);
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('文本 → editor 窗口，opened', async () => {
    vi.mocked(invoke).mockResolvedValue(fileInfo());
    const r = await openRemoteFile('s1', '/x/a.txt', manager);
    expect(r).toEqual({ kind: 'opened' });
    expect(created).toHaveLength(1);
    expect(created[0].appId).toBe('editor');
    expect(created[0].opts.preloadData).toEqual({ path: '/x/a.txt', serverId: 's1' });
  });

  it('图片（PNG magic）→ image-viewer 携带 mimeType', async () => {
    vi.mocked(invoke).mockResolvedValue(fileInfo({
      path: '/x/a.png', extension: 'png', isText: false,
      magicBytes: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
    }));
    const r = await openRemoteFile('s1', '/x/a.png', manager);
    expect(r).toEqual({ kind: 'opened' });
    expect(created[0].appId).toBe('image-viewer');
    expect(created[0].opts.preloadData).toMatchObject({ path: '/x/a.png', serverId: 's1', mimeType: 'image/png' });
  });

  it('探测失败 → 回退编辑器（旧 Agent 兼容）', async () => {
    vi.mocked(invoke).mockRejectedValue(new Error('agent too old'));
    const r = await openRemoteFile('s1', '/x/a.txt', manager);
    expect(r).toEqual({ kind: 'fallback-editor' });
    expect(created[0].appId).toBe('editor');
  });

  it('传输故障（[transport] 前缀）→ 不降级，直接 error', async () => {
    // Tauri Err(String) 的 rejection 是纯字符串（非 Error 对象），mock 对齐真实形态
    vi.mocked(invoke).mockRejectedValue('[transport] 未找到连接');
    const r = await openRemoteFile('s1', '/x/a.txt', manager);
    expect(r).toEqual({ kind: 'error', message: '连接不可用：未找到连接' });
    expect(created).toHaveLength(0);   // 不弹注定失败的编辑器
  });

  it('探测失败 + 已知大文件（fallbackSize > 5MB）→ 确认拒绝 cancelled；同意则回退编辑器', async () => {
    vi.mocked(invoke).mockRejectedValue(new Error('agent too old'));
    vi.spyOn(window, 'confirm').mockReturnValue(false);
    const cancelled = await openRemoteFile('s1', '/x/big.txt', manager, 8 * 1024 * 1024);
    expect(cancelled).toEqual({ kind: 'cancelled' });
    expect(created).toHaveLength(0);

    vi.spyOn(window, 'confirm').mockReturnValue(true);
    const ok = await openRemoteFile('s1', '/x/big.txt', manager, 8 * 1024 * 1024);
    expect(ok).toEqual({ kind: 'fallback-editor' });
    expect(created[0].appId).toBe('editor');
  });

  it('大文件用户取消 → cancelled，不开窗', async () => {
    vi.mocked(invoke).mockResolvedValue(fileInfo({
      path: '/x/big.png', extension: 'png', isText: false, size: 25 * 1024 * 1024,
      magicBytes: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
    }));
    vi.spyOn(window, 'confirm').mockReturnValue(false);
    const r = await openRemoteFile('s1', '/x/big.png', manager);
    expect(r).toEqual({ kind: 'cancelled' });
    expect(created).toHaveLength(0);
  });

  it('目录条目 → directory（由调用方处理）', async () => {
    vi.mocked(invoke).mockResolvedValue(fileInfo({ isDir: true, isText: false, extension: '' }));
    const r = await openRemoteFile('s1', '/x/dir', manager);
    expect(r).toEqual({ kind: 'directory' });
    expect(created).toHaveLength(0);
  });

  it('未知格式（非文本/无 magic/未知扩展名）→ hex-viewer', async () => {
    vi.mocked(invoke).mockResolvedValue(fileInfo({ isText: false, extension: 'xyz' }));
    const r = await openRemoteFile('s1', '/x/a.xyz', manager);
    expect(r).toEqual({ kind: 'opened' });
    expect(created[0].appId).toBe('hex-viewer');
  });

  it('HTML → browser-local：成功 opened；失败 error', async () => {
    const info = fileInfo({ path: '/x/a.html', extension: 'html', isText: true });
    vi.mocked(invoke)
      .mockResolvedValueOnce(info)             // 探测
      .mockResolvedValueOnce(undefined);       // remote_open_locally 成功
    const ok = await openRemoteFile('s1', '/x/a.html', manager);
    expect(ok).toEqual({ kind: 'opened' });
    expect(created).toHaveLength(0);           // 本地打开不创建窗口
    expect(vi.mocked(invoke).mock.calls[1]).toEqual([
      'remote_open_locally', { serverId: 's1', remotePath: '/x/a.html' },
    ]);

    vi.mocked(invoke).mockClear();
    vi.mocked(invoke)
      .mockResolvedValueOnce(info)                       // 探测成功
      .mockRejectedValueOnce(new Error('本地打开失败'));  // 本地打开失败
    const r = await openRemoteFile('s1', '/x/a.html', manager);
    expect(r).toEqual({ kind: 'error', message: expect.stringContaining('本地打开失败') });
  });
});
