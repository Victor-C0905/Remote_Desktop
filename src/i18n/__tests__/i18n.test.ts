import { describe, it, expect } from 'vitest';
import i18n, { RESOURCES, FALLBACK_LOCALE } from '../index';

/** 提取词典对象的全部叶子路径（仅允许 string 叶子，词典不用数组） */
function shape(obj: unknown, path = ''): string[] {
  if (typeof obj === 'string') return [path];
  return Object.entries(obj as Record<string, unknown>).flatMap(([k, v]) =>
    shape(v, path ? `${path}.${k}` : k)
  );
}

describe('i18n 基础设施', () => {
  it('默认初始化为 zh-CN（fallback 也是 zh-CN）', () => {
    expect(i18n.language).toBe('zh-CN');
    expect(FALLBACK_LOCALE).toBe('zh-CN');
  });

  it('t() 可取到 common 域文案', () => {
    expect(i18n.t('common.confirm')).toBe('确定');
  });

  it('每种语言包结构与 zh-CN 完全一致（类型约束之外的运行时兜底）', () => {
    const zhKeys = shape(RESOURCES['zh-CN'].resource).sort();
    for (const [code, { resource }] of Object.entries(RESOURCES)) {
      expect(shape(resource).sort(), `${code} 词典 key 结构与 zh-CN 不一致`).toEqual(zhKeys);
    }
  });

  it('fallbackLng 配置为 zh-CN', () => {
    // i18next v26 init 后将 fallbackLng 归一化为 ['zh-CN']，用 toContain 兼容字符串/数组两种表示
    expect(i18n.options.fallbackLng).toContain('zh-CN');
  });
});
