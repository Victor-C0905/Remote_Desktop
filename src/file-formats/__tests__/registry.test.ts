/**
 * FileFormatRegistry 单元测试
 *
 * 验证插件注册、按优先级检测、无匹配时的空返回
 */
import { describe, it, expect } from 'vitest';
import { FileFormatRegistry } from '../registry';
import type { RemoteFileInfo } from '../types';

/** 构造测试用文件信息 */
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

describe('FileFormatRegistry', () => {
  it('按注册顺序返回首个匹配的插件结果', () => {
    const registry = new FileFormatRegistry();
    registry.register({ id: 'a', detect: () => null });
    registry.register({ id: 'b', detect: () => ({ pluginId: 'b', category: 'text', confidence: 1 }) });

    const match = registry.detect(makeInfo());
    expect(match).not.toBeNull();
    expect(match!.pluginId).toBe('b');
  });

  it('所有插件都不匹配时返回 null', () => {
    const registry = new FileFormatRegistry();
    registry.register({ id: 'a', detect: () => null });

    expect(registry.detect(makeInfo())).toBeNull();
  });

  it('空注册表返回 null', () => {
    const registry = new FileFormatRegistry();
    expect(registry.detect(makeInfo())).toBeNull();
  });
});
