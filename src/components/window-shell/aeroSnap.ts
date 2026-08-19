// src/components/window-shell/aeroSnap.ts
// Aero Snap 几何计算 - 纯函数,零依赖,对齐 Windows 11 行为

/**
 * Snap 区类型
 * - 'top': 顶部边 → 最大化
 * - 'left'/'right'/'bottom': 左/右/下边 → 半屏
 * - 'top-left'/'top-right'/'bottom-left'/'bottom-right': 四角 → 1/4 屏
 */
export type SnapZone =
  | 'top'
  | 'left'
  | 'right'
  | 'bottom'
  | 'top-left'
  | 'top-right'
  | 'bottom-left'
  | 'bottom-right';

/** 矩形(屏幕坐标系) */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** 距屏幕边缘多少 px 触发边吸附(Windows 11 实测约 8px) */
export const SNAP_EDGE_THRESHOLD = 8;

/** 屏幕四角多大正方形区域触发四分之一吸附 */
export const SNAP_CORNER_SIZE = 8;

/**
 * 由鼠标在容器(workspace)内的坐标判断 snap 区。
 * 四角优先于边: cursorX≤CORNER && cursorY≤CORNER → 'top-left',不再走"左半屏"。
 *
 * @param cursorX 鼠标在容器内的 x 坐标
 * @param cursorY 鼠标在容器内的 y 坐标
 * @param containerSize 容器(workspace)尺寸
 * @returns SnapZone | null
 */
export function detectSnapZone(
  cursorX: number,
  cursorY: number,
  containerSize: { width: number; height: number },
): SnapZone | null {
  const W = containerSize.width;
  const H = containerSize.height;

  // 四角优先
  if (cursorX <= SNAP_CORNER_SIZE && cursorY <= SNAP_CORNER_SIZE) return 'top-left';
  if (cursorX >= W - SNAP_CORNER_SIZE && cursorY <= SNAP_CORNER_SIZE) return 'top-right';
  if (cursorX <= SNAP_CORNER_SIZE && cursorY >= H - SNAP_CORNER_SIZE) return 'bottom-left';
  if (cursorX >= W - SNAP_CORNER_SIZE && cursorY >= H - SNAP_CORNER_SIZE) return 'bottom-right';

  // 四边
  if (cursorY <= SNAP_EDGE_THRESHOLD) return 'top';
  if (cursorX <= SNAP_EDGE_THRESHOLD) return 'left';
  if (cursorX >= W - SNAP_EDGE_THRESHOLD) return 'right';
  if (cursorY >= H - SNAP_EDGE_THRESHOLD) return 'bottom';

  return null;
}

/**
 * 由 snap 区算出最终目标矩形。
 * 'top' = 全屏 = maximize(走 maximize 通道);
 * 其余 zone = 半屏 / 1/4 屏(走 snap 通道)。
 *
 * @param zone Snap 区(必须为 8 个有效 zone 之一)
 * @param containerSize 容器(workspace)尺寸
 * @returns 目标矩形(屏幕坐标系)
 */
export function computeSnapRect(
  zone: SnapZone,
  containerSize: { width: number; height: number },
): Rect {
  const W = containerSize.width;
  const H = containerSize.height;
  const halfW = W / 2;
  const halfH = H / 2;

  switch (zone) {
    case 'top':          return { x: 0,      y: 0,      width: W,     height: H };
    case 'left':         return { x: 0,      y: 0,      width: halfW, height: H };
    case 'right':        return { x: halfW,  y: 0,      width: halfW, height: H };
    case 'bottom':       return { x: 0,      y: halfH,  width: W,     height: halfH };
    case 'top-left':     return { x: 0,      y: 0,      width: halfW, height: halfH };
    case 'top-right':    return { x: halfW,  y: 0,      width: halfW, height: halfH };
    case 'bottom-left':  return { x: 0,      y: halfH,  width: halfW, height: halfH };
    case 'bottom-right': return { x: halfW,  y: halfH,  width: halfW, height: halfH };
  }
}

/** 'top' zone 走 maximize 通道;其余走 snap 通道。 */
export function isMaximizeZone(zone: SnapZone | null): boolean {
  return zone === 'top';
}
