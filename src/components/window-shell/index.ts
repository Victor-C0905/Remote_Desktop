// src/components/window-shell/index.ts
// 窗口抽象层导出

export { WindowShell } from './WindowShell';
export type { WindowShellProps } from './WindowShell';
export { WindowControls } from './WindowControls';
export type { WindowControlsProps } from './WindowControls';

// ✅ Aero Snap 几何 API(纯函数,可被外部消费)
export {
  detectSnapZone,
  computeSnapRect,
  isMaximizeZone,
  SNAP_EDGE_THRESHOLD,
  SNAP_CORNER_SIZE,
} from './aeroSnap';
export type { SnapZone, Rect } from './aeroSnap';
