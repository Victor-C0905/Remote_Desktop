/**
 * 面包屑溢出布局规划（纯函数，R11-04）
 *
 * 策略（GNOME Files 模式）：容器放不下全部段时，从最旧的中间段开始收拢为
 * 单个「…」，首段（🏠）与末段（当前目录）恒可见——用户永远知道当前位置，
 * 被收拢的层级经「…」菜单回跳。
 *
 * 段下标约定：0 = 🏠（根），n-1 = 当前目录；1..n-2 为可收拢的中间段。
 * 宽度账目：可见段宽 + 相邻可见段之间的分隔符 + 收拢时「…」按钮预留。
 * 连首+末+…都放不下的极端情形不在此处理（交给段上的 CSS max-width 挤压）。
 */

export interface CrumbLayoutPlan {
  /** [hiddenFrom, hiddenTo] 闭区间内的下标被收拢；from > to = 无收拢 */
  hiddenFrom: number;
  hiddenTo: number;
}

/** 无收拢（哨兵：from > to） */
export const NONE_COLLAPSED: CrumbLayoutPlan = { hiddenFrom: 1, hiddenTo: 0 };

/** 相邻可见段之间 ▸ 分隔符的宽度预留（实测 ~11px + 间隙） */
const SEPARATOR_ALLOWANCE = 12;
/** 「…」按钮自身宽 + 其两侧分隔符差额的预留 */
const ELLIPSIS_RESERVE = 44;

export function planCrumbLayout(
  containerWidth: number,
  segmentWidths: number[],
): CrumbLayoutPlan {
  const n = segmentWidths.length;
  if (n <= 2) return NONE_COLLAPSED; // 只有首末段：交给 CSS max-width 挤压

  /** 收拢中间段 1..k 后是否放得下（k=0 即全部可见） */
  const fits = (k: number): boolean => {
    let width = segmentWidths[0] + segmentWidths[n - 1];
    for (let i = k + 1; i <= n - 2; i++) width += segmentWidths[i];
    const shownCount = 2 + (n - 2 - k);
    width += SEPARATOR_ALLOWANCE * (shownCount - 1);
    if (k > 0) width += ELLIPSIS_RESERVE;
    return width <= containerWidth;
  };

  if (fits(0)) return NONE_COLLAPSED;
  for (let k = 1; k <= n - 2; k++) {
    if (fits(k)) return { hiddenFrom: 1, hiddenTo: k };
  }
  return { hiddenFrom: 1, hiddenTo: n - 2 }; // 全部中间段收拢
}
