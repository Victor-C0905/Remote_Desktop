/**
 * 格式插件单元测试
 *
 * 验证 magic bytes 检测、扩展名检测、优先级语义
 */
import { describe, it, expect } from 'vitest';
import { pdfPlugin } from '../plugins/pdf';
import { imagePlugin } from '../plugins/image';
import { textPlugin } from '../plugins/text';
import { archivePlugin, buildExtractCommand } from '../plugins/archive';
import { htmlPlugin } from '../plugins/html';
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

describe('pdfPlugin', () => {
  it('magic bytes 命中 %PDF-', () => {
    const match = pdfPlugin.detect(makeInfo({ magicBytes: [0x25, 0x50, 0x44, 0x46, 0x2d] }));
    expect(match).toEqual({ pluginId: 'pdf', category: 'pdf', confidence: 0.95 });
  });

  it('扩展名 .pdf 低置信度命中', () => {
    const match = pdfPlugin.detect(makeInfo({ extension: 'pdf', magicBytes: [1, 2, 3] }));
    expect(match).toEqual({ pluginId: 'pdf', category: 'pdf', confidence: 0.6 });
  });

  it('普通文件不匹配', () => {
    expect(pdfPlugin.detect(makeInfo({ extension: 'txt' }))).toBeNull();
  });
});

describe('imagePlugin', () => {
  it('PNG magic 命中', () => {
    const match = imagePlugin.detect(
      makeInfo({ magicBytes: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a] }),
    );
    expect(match).toEqual({ pluginId: 'image', category: 'image', mimeType: 'image/png', confidence: 0.95 });
  });

  it('JPEG magic 命中', () => {
    const match = imagePlugin.detect(makeInfo({ magicBytes: [0xff, 0xd8, 0xff, 0xe0] }));
    expect(match).toEqual({ pluginId: 'image', category: 'image', mimeType: 'image/jpeg', confidence: 0.95 });
  });

  it('WebP magic 命中（RIFF....WEBP）', () => {
    const match = imagePlugin.detect(
      makeInfo({ magicBytes: [0x52, 0x49, 0x46, 0x46, 0x00, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50] }),
    );
    expect(match).toEqual({ pluginId: 'image', category: 'image', mimeType: 'image/webp', confidence: 0.95 });
  });

  it('SVG 扩展名命中（SVG 是文本，无 binary magic）', () => {
    const match = imagePlugin.detect(makeInfo({ extension: 'svg', isText: true }));
    expect(match).toEqual({ pluginId: 'image', category: 'image', mimeType: 'image/svg+xml', confidence: 0.6 });
  });

  it('图片扩展名低置信度命中', () => {
    const match = imagePlugin.detect(makeInfo({ extension: 'jpg', magicBytes: [1, 2, 3] }));
    expect(match).toEqual({ pluginId: 'image', category: 'image', mimeType: 'image/jpeg', confidence: 0.6 });
  });

  it('普通文本不匹配', () => {
    expect(imagePlugin.detect(makeInfo({ isText: true, extension: 'txt' }))).toBeNull();
  });
});

describe('textPlugin', () => {
  it('isText=true 且非 SVG 扩展名 → 文本', () => {
    const match = textPlugin.detect(makeInfo({ isText: true, extension: 'rs' }));
    expect(match).toEqual({ pluginId: 'text', category: 'text', confidence: 0.9 });
  });

  it('isText=true 但扩展名为 svg → 不匹配（交给 image 插件）', () => {
    expect(textPlugin.detect(makeInfo({ isText: true, extension: 'svg' }))).toBeNull();
  });

  it('isText=false 不匹配', () => {
    expect(textPlugin.detect(makeInfo({ isText: false, extension: 'txt' }))).toBeNull();
  });

  it('isText=true 但扩展名为 html → 不匹配（交给 html 插件）', () => {
    expect(textPlugin.detect(makeInfo({ isText: true, extension: 'html' }))).toBeNull();
  });
});

describe('archivePlugin', () => {
  it.each(['zip', 'tar', '7z', 'rar', 'tgz'].map((ext) => [ext]))(
    '扩展名 .%s 命中 archive',
    (ext) => {
      const match = archivePlugin.detect(makeInfo({ extension: ext, path: `/tmp/a.${ext}` }));
      expect(match).toMatchObject({ pluginId: 'archive', category: 'archive' });
    },
  );

  it('复合扩展名 .tar.gz 从 path 判断（extension 只取最后一段 gz）', () => {
    const match = archivePlugin.detect(makeInfo({ extension: 'gz', path: '/tmp/a.tar.gz' }));
    expect(match).toMatchObject({ pluginId: 'archive', category: 'archive' });
  });

  it.each(['.tar.bz2', '.tar.xz'])('复合扩展名 %s 命中', (suffix) => {
    const match = archivePlugin.detect(
      makeInfo({ extension: suffix.split('.').pop()!, path: `/tmp/a${suffix}` }),
    );
    expect(match).toMatchObject({ pluginId: 'archive', category: 'archive' });
  });

  it('纯 .gz（非 .tar.gz）不命中（gzip 单文件流解压不在支持范围）', () => {
    expect(archivePlugin.detect(makeInfo({ extension: 'gz', path: '/tmp/a.gz' }))).toBeNull();
  });

  it('ZIP magic 兜底命中（改错扩展名的 zip）', () => {
    const match = archivePlugin.detect(
      makeInfo({ extension: 'dat', path: '/tmp/a.dat', magicBytes: [0x50, 0x4b, 0x03, 0x04, 0x00] }),
    );
    expect(match).toEqual({ pluginId: 'archive', category: 'archive', confidence: 0.95 });
  });

  it('普通文件不匹配', () => {
    expect(archivePlugin.detect(makeInfo({ extension: 'txt', isText: true }))).toBeNull();
  });
});

describe('buildExtractCommand', () => {
  it('.zip → unzip -o <archive> -d <target>', () => {
    const cmd = buildExtractCommand('/tmp/a.zip', '/tmp/out', 'a.zip');
    expect(cmd).toEqual({ command: 'unzip', args: ['-o', '/tmp/a.zip', '-d', '/tmp/out'] });
  });

  it('.tar → tar -xf <archive> -C <target>', () => {
    const cmd = buildExtractCommand('/tmp/a.tar', '/tmp/out', 'a.tar');
    expect(cmd).toEqual({ command: 'tar', args: ['-xf', '/tmp/a.tar', '-C', '/tmp/out'] });
  });

  it('.tar.gz → tar（自动识别压缩格式）', () => {
    const cmd = buildExtractCommand('/tmp/a.tar.gz', '/tmp/out', 'a.tar.gz');
    expect(cmd).toEqual({ command: 'tar', args: ['-xf', '/tmp/a.tar.gz', '-C', '/tmp/out'] });
  });

  it('.tgz → tar', () => {
    const cmd = buildExtractCommand('/tmp/a.tgz', '/tmp/out', 'a.tgz');
    expect(cmd.command).toBe('tar');
  });

  it('.7z → 7z x -o<target> -y', () => {
    const cmd = buildExtractCommand('/tmp/a.7z', '/tmp/out', 'a.7z');
    expect(cmd).toEqual({ command: '7z', args: ['x', '/tmp/a.7z', '-o/tmp/out', '-y'] });
  });

  it('.rar → unrar x -o+', () => {
    const cmd = buildExtractCommand('/tmp/a.rar', '/tmp/out', 'a.rar');
    expect(cmd).toEqual({ command: 'unrar', args: ['x', '-o+', '/tmp/a.rar', '/tmp/out/'] });
  });

  it('路径含空格无需转义（argv 直执行，不经 shell）', () => {
    const cmd = buildExtractCommand('/tmp/my dir/a.zip', '/tmp/my dir/out', 'a.zip');
    expect(cmd.args).toContain('/tmp/my dir/a.zip');
  });
});

describe('htmlPlugin', () => {
  it.each(['html', 'htm'])('扩展名 .%s 命中 browser-local', (ext) => {
    const match = htmlPlugin.detect(makeInfo({ extension: ext, path: `/tmp/a.${ext}` }));
    expect(match).toEqual({ pluginId: 'html', category: 'browser-local', confidence: 0.9 });
  });

  it('其他扩展名不命中', () => {
    expect(htmlPlugin.detect(makeInfo({ extension: 'js', isText: true }))).toBeNull();
  });
});
