import { describe, it, expect } from 'vitest';
import { escapeHtml } from './escapeHtml';

describe('escapeHtml', () => {
  it('普通文本原样返回', () => {
    expect(escapeHtml('终端初始化失败: timeout')).toBe('终端初始化失败: timeout');
  });

  it('转义 HTML 标签注入', () => {
    expect(escapeHtml('<img src=x onerror=alert(1)>')).toBe(
      '&lt;img src=x onerror=alert(1)&gt;',
    );
  });

  it('转义 script 标签', () => {
    expect(escapeHtml('<script>alert(1)</script>')).toBe(
      '&lt;script&gt;alert(1)&lt;/script&gt;',
    );
  });

  it('转义属性逃逸向量（引号与事件属性）', () => {
    expect(escapeHtml('"><svg onload=alert(1)>')).toBe(
      '&quot;&gt;&lt;svg onload=alert(1)&gt;',
    );
  });

  it('先转义 & 避免二次转义', () => {
    expect(escapeHtml('a & b')).toBe('a &amp; b');
    expect(escapeHtml('&lt;')).toBe('&amp;lt;');
  });

  it('单引号转义', () => {
    expect(escapeHtml("it's")).toBe('it&#39;s');
  });

  it('空字符串原样返回', () => {
    expect(escapeHtml('')).toBe('');
  });
});
