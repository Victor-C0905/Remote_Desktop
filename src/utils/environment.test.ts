import { describe, it, expect } from 'vitest';
import { installGlobalErrorReporting } from './environment';

/** 安装并捕获 report 输出与被注册的监听器 */
function install(): { seen: string[]; dispatch: (e: unknown) => void } {
  const seen: string[] = [];
  let handler: (e: unknown) => void = () => {};
  installGlobalErrorReporting((d) => seen.push(d), {
    addEventListener: (_t: string, h: (e: unknown) => void) => {
      handler = h;
    },
  });
  return { seen, dispatch: (e) => handler(e) };
}

describe('installGlobalErrorReporting', () => {
  it('Error 对象带 stack 时上报完整 stack', () => {
    const { seen, dispatch } = install();

    const err = new TypeError('r is not a function');
    dispatch({ message: 'r is not a function', error: err, filename: 'chunk.js', lineno: 1, colno: 2 });

    expect(seen).toHaveLength(1);
    expect(seen[0]).toContain('r is not a function');
    expect(seen[0]).toContain('TypeError');
  });

  it('无 error 对象时回退到 message + 位置（跨 chunk 抛字符串等场景）', () => {
    const { seen, dispatch } = install();

    dispatch({ message: 'Script error.', error: undefined, filename: 'a.js', lineno: 9, colno: 8 });

    expect(seen).toHaveLength(1);
    expect(seen[0]).toContain('Script error.');
    expect(seen[0]).toContain('a.js:9:8');
  });
});
