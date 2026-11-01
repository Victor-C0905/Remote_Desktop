// src/window-system/zOrder.ts
// 窗口 z 层计算（纯函数 + 单测）：激活栈排名 → z-index。
//
// 层带约定（variables.css）：非激活基线 20（--z-window-inactive），激活层顶
// 30（--z-window-active），TopBar 50，应用浮层 60+。
// 激活窗口恒为 30（层顶）；非激活窗口按「非激活内序号」单调映射进 [20, 29]：
// - ≤10 个非激活窗口：严格保序无并列；
// - 更多窗口：鸽笼退化，并列由 DOM 创建序兜底（有界退化，窗口数有实际上限）；
// - 全带低于 TopBar(50) 与应用浮层(60+)，任何窗口数下不越层。

/** 激活窗口所在层（= --z-window-active，恒为全部窗口之上） */
export const WINDOW_Z_ACTIVE = 30;
/** 非激活映射带下界（= --z-window-inactive） */
export const WINDOW_Z_BASE = 20;
/** 非激活映射带上界（TopBar 50 之下、浮层 60 之上留白） */
export const WINDOW_Z_BAND_TOP = 29;

/**
 * 计算单个窗口的 z-index。
 * @param rank    激活栈排名（0=栈底/最旧；末位=激活窗口，恒为层顶）
 * @param total   存活窗口总数
 */
export function computeWindowZ(rank: number, total: number): number {
  const m = Math.max(0, total - 1); // 非激活窗口数
  if (rank >= total - 1) return WINDOW_Z_ACTIVE; // 激活窗口恒层顶
  if (m <= 1) return WINDOW_Z_BASE;
  const k = Math.max(0, Math.min(rank, m - 1)); // 非激活内序号（栈底=0）
  return WINDOW_Z_BASE + Math.floor((k * (WINDOW_Z_BAND_TOP - WINDOW_Z_BASE)) / (m - 1));
}
