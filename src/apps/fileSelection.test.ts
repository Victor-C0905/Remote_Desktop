import { describe, it, expect } from 'vitest';
import {
  rectsIntersect,
  normalizeRect,
  marqueeHit,
  rangeSelection,
  toggleSelection,
  entryName,
  childPath,
  isSelfOrDescendant,
  buildMoveTargets,
} from './fileSelection';

const rect = (l: number, t: number, r: number, b: number) => ({ left: l, top: t, right: r, bottom: b });

describe('rectsIntersect', () => {
  it('重叠矩形相交', () => {
    expect(rectsIntersect(rect(0, 0, 10, 10), rect(5, 5, 15, 15))).toBe(true);
  });

  it('边缘相切不算相交（贴边框选不误选）', () => {
    expect(rectsIntersect(rect(0, 0, 10, 10), rect(10, 0, 20, 10))).toBe(false);
    expect(rectsIntersect(rect(0, 0, 10, 10), rect(0, 10, 10, 20))).toBe(false);
  });

  it('分离矩形不相交', () => {
    expect(rectsIntersect(rect(0, 0, 10, 10), rect(20, 20, 30, 30))).toBe(false);
  });

  it('包含关系相交', () => {
    expect(rectsIntersect(rect(0, 0, 100, 100), rect(10, 10, 20, 20))).toBe(true);
  });
});

describe('normalizeRect', () => {
  it('左上→右下', () => {
    expect(normalizeRect(0, 0, 10, 10)).toEqual(rect(0, 0, 10, 10));
  });

  it('反方向拖拽（右下→左上）', () => {
    expect(normalizeRect(10, 10, 0, 0)).toEqual(rect(0, 0, 10, 10));
  });

  it('任意方向拖拽', () => {
    expect(normalizeRect(10, 0, 0, 10)).toEqual(rect(0, 0, 10, 10));
  });
});

describe('marqueeHit', () => {
  it('只命中与框选矩形相交的条目，保持给定顺序', () => {
    const paths = ['a', 'b', 'c', 'd'];
    const rects = [rect(0, 0, 10, 10), rect(0, 20, 10, 30), rect(0, 40, 10, 50), rect(0, 60, 10, 70)];
    expect(marqueeHit(paths, rects, rect(5, 15, 15, 45))).toEqual(['b', 'c']);
  });

  it('无命中返回空数组', () => {
    const paths = ['a'];
    expect(marqueeHit(paths, [rect(0, 0, 10, 10)], rect(100, 100, 200, 200))).toEqual([]);
  });
});

describe('rangeSelection', () => {
  const ordered = ['a', 'b', 'c', 'd', 'e'];

  it('锚点在目标之前：正序闭区间', () => {
    expect(rangeSelection(ordered, 'b', 'd')).toEqual(['b', 'c', 'd']);
  });

  it('锚点在目标之后：仍按可见顺序返回', () => {
    expect(rangeSelection(ordered, 'd', 'b')).toEqual(['b', 'c', 'd']);
  });

  it('锚点为 null：回退单选目标', () => {
    expect(rangeSelection(ordered, null, 'c')).toEqual(['c']);
  });

  it('锚点不在列表中：回退单选目标', () => {
    expect(rangeSelection(ordered, 'zzz', 'c')).toEqual(['c']);
  });

  it('目标不在列表中：回退单选目标', () => {
    expect(rangeSelection(ordered, 'a', 'zzz')).toEqual(['zzz']);
  });
});

describe('toggleSelection', () => {
  it('未选中则追加（保序）', () => {
    expect(toggleSelection(['a', 'c'], 'b')).toEqual(['a', 'c', 'b']);
  });

  it('已选中则移除', () => {
    expect(toggleSelection(['a', 'b', 'c'], 'b')).toEqual(['a', 'c']);
  });

  it('重复切换往返一致', () => {
    let s: string[] = [];
    s = toggleSelection(s, 'a');
    s = toggleSelection(s, 'b');
    s = toggleSelection(s, 'b');
    s = toggleSelection(s, 'a');
    expect(s).toEqual([]);
  });
});

describe('entryName / childPath', () => {
  it('取路径末段', () => {
    expect(entryName('/a/b/c.txt')).toBe('c.txt');
    expect(entryName('/root')).toBe('root');
  });

  it('根目录拼接不产生双斜杠', () => {
    expect(childPath('/', 'home')).toBe('/home');
    expect(childPath('/home', 'user')).toBe('/home/user');
  });
});

describe('isSelfOrDescendant', () => {
  it('目标为源自身：非法', () => {
    expect(isSelfOrDescendant('/a', '/a')).toBe(true);
  });

  it('目标为源子目录：非法', () => {
    expect(isSelfOrDescendant('/a', '/a/b')).toBe(true);
  });

  it('同前缀但非子目录：合法', () => {
    expect(isSelfOrDescendant('/a', '/ab')).toBe(false);
  });

  it('无关目录：合法', () => {
    expect(isSelfOrDescendant('/a/b', '/c/d')).toBe(false);
  });
});

describe('buildMoveTargets', () => {
  it('构造 dst = 目录 + 源末段', () => {
    expect(buildMoveTargets(['/a/x.txt', '/a/y'], '/b')).toEqual([
      { src: '/a/x.txt', dst: '/b/x.txt' },
      { src: '/a/y', dst: '/b/y' },
    ]);
  });

  it('过滤拖入自身/子目录的非法项', () => {
    expect(buildMoveTargets(['/a', '/a/b', '/c'], '/a/b')).toEqual([{ src: '/c', dst: '/a/b/c' }]);
  });

  it('根目录为目标', () => {
    expect(buildMoveTargets(['/home/u/f.txt'], '/')).toEqual([{ src: '/home/u/f.txt', dst: '/f.txt' }]);
  });
});
