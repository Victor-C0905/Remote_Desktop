/**
 * 格式插件单元测试
 *
 * 验证 magic bytes 检测、扩展名检测、优先级语义
 */
import { describe, it, expect } from 'vitest';
import { pdfPlugin } from '../plugins/pdf';
import { imagePlugin } from '../plugins/image';
import { textPlugin } from '../plugins/text';
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
});
