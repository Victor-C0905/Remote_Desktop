/**
 * Dock 点击动作决策（纯函数）
 *
 * 三态循环（R2 实测基线）：未聚焦→提升 / 已聚焦→最小化 / 最小化→恢复。
 * R6-?-02 修正：`manager.create` 对未注册 onBeforeCreate 的应用是同步完成的，
 * 「创建中防重入集合」对此是零宽窗口——双击 Dock 图标的第二下会落进
 * 「已聚焦→最小化」分支，把刚创建的窗口立刻藏成 display:none（表现为
 * 「首窗打开即不可见，再点一次 Dock 才出现」）。创建宽限期内的最小化
 * 降级为聚焦：宽限期外的正常单击三态循环不变。
 */
export type DockClickAction = 'create' | 'restore' | 'focus' | 'minimize';

/** 创建宽限期：覆盖系统双击间隔（Windows 默认 500ms），期内不响应误最小化 */
export const DOCK_TOGGLE_GRACE_MS = 500;

export interface DockClickContext {
  hasWindow: boolean;
  minimized: boolean;
  isActive: boolean;
  /** 窗口刚创建（宽限期内）：吞掉双击第二下的误最小化 */
  withinCreationGrace: boolean;
}

export function resolveDockClickAction(ctx: DockClickContext): DockClickAction {
  if (!ctx.hasWindow) return 'create';
  if (ctx.minimized) return 'restore';
  if (!ctx.isActive) return 'focus';
  return ctx.withinCreationGrace ? 'focus' : 'minimize';
}
