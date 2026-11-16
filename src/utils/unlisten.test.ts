import { describe, it, expect } from 'vitest';
import { safeUnlistenAll } from './unlisten';

describe('safeUnlistenAll', () => {
  it('依序调用全部清理函数', () => {
    const calls: number[] = [];
    safeUnlistenAll([() => calls.push(1), () => calls.push(2), () => calls.push(3)]);
    expect(calls).toEqual([1, 2, 3]);
  });

  it('单个清理函数抛错不中断其余清理、也不向外抛（R8-N1 回归）', () => {
    const calls: number[] = [];
    expect(() => {
      safeUnlistenAll([
        () => calls.push(1),
        () => {
          throw new TypeError('r is not a function');
        },
        () => calls.push(3),
      ]);
    }).not.toThrow();
    expect(calls).toEqual([1, 3]);
  });

  it('末尾元素抛错同样被吞掉', () => {
    expect(() => {
      safeUnlistenAll([() => {
        throw new Error('boom');
      }]);
    }).not.toThrow();
  });

  it('空数组为 no-op', () => {
    expect(() => safeUnlistenAll([])).not.toThrow();
  });
});
