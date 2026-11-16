/**
 * 侧栏宽度常量与夹取（从 AppLayout.tsx 拆出）：
 * 与组件同文件导出会破坏 react-refresh 的 Fast Refresh（每次改动
 * AppLayout 全量重载，R12 实测），纯逻辑独立成模块。
 */

/** 侧栏宽度下限（图标+文案可读的最小宽） */
export const SIDEBAR_MIN_WIDTH = 160;
/** 侧栏宽度上限（避免挤占内容区） */
export const SIDEBAR_MAX_WIDTH = 420;
/** 双击重置的默认宽 */
export const SIDEBAR_DEFAULT_WIDTH = 240;

/** 请求宽度夹取到 [min, max] 并取整（px） */
export function clampSidebarWidth(
  w: number,
  min: number = SIDEBAR_MIN_WIDTH,
  max: number = SIDEBAR_MAX_WIDTH,
): number {
  return Math.min(max, Math.max(min, Math.round(w)));
}
