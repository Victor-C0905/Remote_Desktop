// 面包屑溢出布局规划回归（R11-04，P1）：
// 深层路径在窄容器下，此前中间段与当前段被硬裁在栏外——当前目录不可见、
// 被裁层级无处回跳。规划器策略：空间不足时从最旧中间段开始收拢为「…」，
// 首段（🏠）与末段（当前目录）恒可见。
import { describe, it, expect } from 'vitest';
import { planCrumbLayout, NONE_COLLAPSED } from '../fmCrumbLayout';

const SEP = 12; // 测试内分隔符预留宽须与实现一致

describe('planCrumbLayout', () => {
  it('全部放得下 → 不收拢', () => {
    const widths = [28, 40, 60, 80];
    const total = widths.reduce((a, b) => a + b, 0) + SEP * (widths.length - 1);
    expect(planCrumbLayout(total + 10, widths)).toEqual(NONE_COLLAPSED);
  });

  it('轻微溢出 → 只收拢最旧的中间段（下标 1）', () => {
    const widths = [28, 40, 60, 80]; // 全显 = 208 + SEP*3 = 244
    // 隐藏 [1..1] 后 = 28+60+80 + SEP*2 + ⓘ44 = 236 ≤ W < 244
    const plan = planCrumbLayout(240, widths);
    expect(plan).toEqual({ hiddenFrom: 1, hiddenTo: 1 });
  });

  it('极窄 → 中间段全收拢，首末恒可见', () => {
    const plan = planCrumbLayout(120, [28, 200, 200, 200, 60]);
    expect(plan).toEqual({ hiddenFrom: 1, hiddenTo: 3 });
  });

  it('只有首末两段 → 不收拢（交给 CSS max-width 挤压）', () => {
    expect(planCrumbLayout(30, [28, 500])).toEqual(NONE_COLLAPSED);
    expect(planCrumbLayout(10, [28, 500, 90])).toEqual({ hiddenFrom: 1, hiddenTo: 1 });
  });

  it('容器渐宽时收拢数单调递减', () => {
    const widths = [28, 90, 90, 90, 90, 60];
    const plans = [200, 260, 320, 400, 500, 620].map((w) => planCrumbLayout(w, widths));
    const collapsed = plans.map((p) => Math.max(0, p.hiddenTo - p.hiddenFrom + 1));
    for (let i = 1; i < collapsed.length; i++) {
      expect(collapsed[i]).toBeLessThanOrEqual(collapsed[i - 1]);
    }
    expect(plans[plans.length - 1]).toEqual(NONE_COLLAPSED);
  });
});
