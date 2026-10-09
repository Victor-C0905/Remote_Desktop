/**
 * FileOpener 网关层单元测试
 *
 * 验证格式检测 → 打开目标的决策逻辑（大文件确认阈值、未知格式 hex 回退）
 */
import { describe, it, expect } from 'vitest';
import { decideOpenTarget, createDefaultRegistry, SIZE_LIMITS } from '../FileOpener';
import type { RemoteFileInfo } from '../types';

function makeInfo(overrides: Partial<RemoteFileInfo> = {}): RemoteFileInfo {
  return {
    path: '/tmp/f',
    size: 100,
    isDir: false,
    isText: false,
    extension: '',
    magicBytes: [],
    ...overrides,
  };
}

describe('decideOpenTarget', () => {
  it('PNG 文件路由到 image 且小文件无需确认', () => {
    const d = decideOpenTarget(
      makeInfo({ magicBytes: [0x89, 0x50, 0x4e, 0x47], extension: 'png' }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('image');
    expect(d.mimeType).toBe('image/png');
    expect(d.needsSizeConfirm).toBe(false);
  });

  it('超限图片需要大小确认', () => {
    const d = decideOpenTarget(
      makeInfo({ magicBytes: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], size: SIZE_LIMITS.image + 1 }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('image');
    expect(d.needsSizeConfirm).toBe(true);
  });

  it('PDF 文件路由到 pdf', () => {
    const d = decideOpenTarget(
      makeInfo({ magicBytes: [0x25, 0x50, 0x44, 0x46, 0x2d] }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('pdf');
  });

  it('文本文件路由到 text', () => {
    const d = decideOpenTarget(
      makeInfo({ isText: true, extension: 'rs' }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('text');
  });

  it('未知二进制格式回退到 hex', () => {
    const d = decideOpenTarget(
      makeInfo({ magicBytes: [0x01, 0x02, 0x03], extension: 'bin' }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('hex');
    expect(d.reason).toContain('未知格式');
  });

  it('目录返回 directory 目标', () => {
    const d = decideOpenTarget(
      makeInfo({ isDir: true }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('directory');
  });
});

describe('文件操作扩展决策', () => {
  it('压缩包路由到 archive，无大小确认（即使 100MB）', () => {
    const d = decideOpenTarget(
      makeInfo({ extension: 'zip', path: '/tmp/a.zip', size: 100 * 1024 * 1024 }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('archive');
    expect(d.needsSizeConfirm).toBe(false);
  });

  it('.tar.gz（extension=gz）路由到 archive', () => {
    const d = decideOpenTarget(
      makeInfo({ extension: 'gz', path: '/tmp/a.tar.gz' }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('archive');
  });

  it('ZIP magic 兜底路由到 archive（改错扩展名）', () => {
    const d = decideOpenTarget(
      makeInfo({ extension: 'dat', path: '/tmp/a.dat', magicBytes: [0x50, 0x4b, 0x03, 0x04] }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('archive');
  });

  it('HTML 路由到 browser-local，小文件无需确认', () => {
    const d = decideOpenTarget(
      makeInfo({ extension: 'html', path: '/tmp/a.html', isText: true, size: 1024 }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('browser-local');
    expect(d.needsSizeConfirm).toBe(false);
  });

  it('超大 HTML（>20MB）需要大小确认', () => {
    const d = decideOpenTarget(
      makeInfo({
        extension: 'html',
        path: '/tmp/a.html',
        isText: true,
        size: SIZE_LIMITS['browser-local'] + 1,
      }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('browser-local');
    expect(d.needsSizeConfirm).toBe(true);
  });

  it('.sh 含 shebang 也路由到 text（方案 1：双击一律编辑器，运行收右键菜单）', () => {
    const d = decideOpenTarget(
      makeInfo({ extension: 'sh', path: '/tmp/a.sh', isText: true, magicBytes: [0x23, 0x21, 0x2f, 0x62] }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('text');
  });

  it('.sh 无 shebang 路由到 text（编辑器打开）', () => {
    const d = decideOpenTarget(
      makeInfo({ extension: 'sh', path: '/tmp/a.sh', isText: true, magicBytes: [0x65, 0x63, 0x68] }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('text');
  });

  it('.py 不受影响，仍路由到 text', () => {
    const d = decideOpenTarget(
      makeInfo({ extension: 'py', path: '/tmp/a.py', isText: true, magicBytes: [0x23, 0x21] }),
      createDefaultRegistry(),
    );
    expect(d.kind).toBe('text');
  });
});
