// src/components/window-shell/aeroSnap.test.ts

import { describe, it, expect } from 'vitest';
import {
  detectSnapZone,
  computeSnapRect,
  isMaximizeZone,
  SNAP_EDGE_THRESHOLD,
  SNAP_CORNER_SIZE,
  type SnapZone,
} from './aeroSnap';

// 1920x1080 容器作 fixture
const CONTAINER = { width: 1920, height: 1080 };

describe('aeroSnap - 常量', () => {
  it('SNAP_EDGE_THRESHOLD 与 SNAP_CORNER_SIZE 为正整数', () => {
    expect(SNAP_EDGE_THRESHOLD).toBeGreaterThan(0);
    expect(SNAP_CORNER_SIZE).toBeGreaterThan(0);
  });
});

describe('detectSnapZone - 四角优先', () => {
  it('左上角 → top-left', () => {
    expect(detectSnapZone(0, 0, CONTAINER)).toBe('top-left');
    expect(detectSnapZone(SNAP_CORNER_SIZE, SNAP_CORNER_SIZE, CONTAINER)).toBe('top-left');
  });

  it('右上角 → top-right', () => {
    expect(detectSnapZone(CONTAINER.width, 0, CONTAINER)).toBe('top-right');
    expect(detectSnapZone(CONTAINER.width - SNAP_CORNER_SIZE, SNAP_CORNER_SIZE, CONTAINER)).toBe('top-right');
  });

  it('左下角 → bottom-left', () => {
    expect(detectSnapZone(0, CONTAINER.height, CONTAINER)).toBe('bottom-left');
    expect(detectSnapZone(SNAP_CORNER_SIZE, CONTAINER.height - SNAP_CORNER_SIZE, CONTAINER)).toBe('bottom-left');
  });

  it('右下角 → bottom-right', () => {
    expect(detectSnapZone(CONTAINER.width, CONTAINER.height, CONTAINER)).toBe('bottom-right');
    expect(detectSnapZone(CONTAINER.width - SNAP_CORNER_SIZE, CONTAINER.height - SNAP_CORNER_SIZE, CONTAINER)).toBe('bottom-right');
  });

  it('四角优先于四边: (4, 4) → top-left,不是 top 也不是 left', () => {
    // (4,4) 同时满足 corner 与 top/left 边,应走 corner
    expect(detectSnapZone(4, 4, CONTAINER)).toBe('top-left');
  });
});

describe('detectSnapZone - corner→edge 过渡', () => {
  it('(9, 4): x 离开 corner, y 仍在 top → top', () => {
    expect(detectSnapZone(9, 4, CONTAINER)).toBe('top');
  });

  it('(4, 9): y 离开 corner, x 仍在 left → left', () => {
    expect(detectSnapZone(4, 9, CONTAINER)).toBe('left');
  });

  it('(9, 9): 两者都离开 corner → null', () => {
    expect(detectSnapZone(9, 9, CONTAINER)).toBe(null);
  });

  it('右侧镜像 (1910, 4): x 离开 right corner (1912), y 仍在 top → top', () => {
    // corner right 边界: cursorX >= 1920 - 8 = 1912
    // 1910 < 1912,不进 corner;但 y=4 < 8,进 top edge
    expect(detectSnapZone(1910, 4, CONTAINER)).toBe('top');
  });
});

describe('detectSnapZone - 四边', () => {
  it('顶部边 → top', () => {
    expect(detectSnapZone(500, 0, CONTAINER)).toBe('top');
    expect(detectSnapZone(500, SNAP_EDGE_THRESHOLD, CONTAINER)).toBe('top');
  });

  it('左边 → left', () => {
    expect(detectSnapZone(0, 500, CONTAINER)).toBe('left');
    expect(detectSnapZone(SNAP_EDGE_THRESHOLD, 500, CONTAINER)).toBe('left');
  });

  it('右边 → right', () => {
    expect(detectSnapZone(CONTAINER.width, 500, CONTAINER)).toBe('right');
    expect(detectSnapZone(CONTAINER.width - SNAP_EDGE_THRESHOLD, 500, CONTAINER)).toBe('right');
  });

  it('下边 → bottom', () => {
    expect(detectSnapZone(500, CONTAINER.height, CONTAINER)).toBe('bottom');
    expect(detectSnapZone(500, CONTAINER.height - SNAP_EDGE_THRESHOLD, CONTAINER)).toBe('bottom');
  });
});

describe('detectSnapZone - 远离边缘返回 null', () => {
  it('容器中心 → null', () => {
    expect(detectSnapZone(960, 540, CONTAINER)).toBe(null);
  });

  it('刚超出阈值 → null', () => {
    expect(detectSnapZone(500, SNAP_EDGE_THRESHOLD + 1, CONTAINER)).toBe(null);
    expect(detectSnapZone(SNAP_EDGE_THRESHOLD + 1, 500, CONTAINER)).toBe(null);
  });
});

describe('computeSnapRect - 各 zone 矩形', () => {
  const W = CONTAINER.width;
  const H = CONTAINER.height;
  const halfW = W / 2;
  const halfH = H / 2;

  it('top → 全屏 = maximize rect', () => {
    expect(computeSnapRect('top', CONTAINER)).toEqual({ x: 0, y: 0, width: W, height: H });
  });

  it('left → 左半屏', () => {
    expect(computeSnapRect('left', CONTAINER)).toEqual({ x: 0, y: 0, width: halfW, height: H });
  });

  it('right → 右半屏', () => {
    expect(computeSnapRect('right', CONTAINER)).toEqual({ x: halfW, y: 0, width: halfW, height: H });
  });

  it('bottom → 下半屏', () => {
    expect(computeSnapRect('bottom', CONTAINER)).toEqual({ x: 0, y: halfH, width: W, height: halfH });
  });

  it('top-left → 左上四分之一', () => {
    expect(computeSnapRect('top-left', CONTAINER)).toEqual({ x: 0, y: 0, width: halfW, height: halfH });
  });

  it('top-right → 右上四分之一', () => {
    expect(computeSnapRect('top-right', CONTAINER)).toEqual({ x: halfW, y: 0, width: halfW, height: halfH });
  });

  it('bottom-left → 左下四分之一', () => {
    expect(computeSnapRect('bottom-left', CONTAINER)).toEqual({ x: 0, y: halfH, width: halfW, height: halfH });
  });

  it('bottom-right → 右下四分之一', () => {
    expect(computeSnapRect('bottom-right', CONTAINER)).toEqual({ x: halfW, y: halfH, width: halfW, height: halfH });
  });
});

describe('isMaximizeZone', () => {
  it("'top' → true", () => {
    expect(isMaximizeZone('top')).toBe(true);
  });

  it('其余 zone → false', () => {
    (['left', 'right', 'bottom', 'top-left', 'top-right', 'bottom-left', 'bottom-right'] as SnapZone[]).forEach(z => {
      expect(isMaximizeZone(z)).toBe(false);
    });
  });

  it('null → false', () => {
    expect(isMaximizeZone(null)).toBe(false);
  });
});
